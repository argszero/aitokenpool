//! 明细行的 JSONL 归档（rant 2026-10-09T12:28:58 验收项 2）。
//!
//! 背景：`transactions` 只增不减、没有保留策略 ⇒ SQLite 无限膨胀、聚合查询越来越慢
//! （dev 实测 97.3 万行：`COUNT(*)` 23.7s、30 天窗口聚合 32.5s）。宿主把治本诉求收敛为
//! 两条，本模块是**第二条**：「详细记录写入可滚动、有保留期的 JSONL 文件，从文件查看即可」。
//!
//! 仓库里有**两张**只增不减的明细表，同一套机件服务它们：
//!
//! | 表 | 规格 | 目录 |
//! |---|---|---|
//! | `transactions` | [`TRANSACTIONS`] | `<archive.dir>` |
//! | `usage_records` | [`USAGE_RECORDS`] | `<archive.dir>/`[`USAGE_SUBDIR`] |
//!
//! 一张表一个 [`Spec`]（表名 + 文件名前缀 + 列清单 + 行→JSON）；机件只写一遍 ——
//! `SELECT` 从 `Spec::columns` 渲染、文件名从 `Spec::prefix` 渲染、`Archive` 只认 `Spec`。
//! 目录分开（`usage_records` 在子目录里）是为了让两套文件与**水位文件**互不干扰：水位文件
//! 就叫 `watermark`，靠**目录**隔离；文件名前缀是第二道保险（`numbered()` 只收自己前缀的文件）。
//!
//! 形态：
//! - 一次滚动 = 一个文件 `<dir>/<prefix><6 位序号>.jsonl`，**每行一个 JSON 对象**；
//! - 当前文件写入后再追加会超过 `max_file_size` 字节 ⇒ 开新文件（序号 +1）；
//! - 只保留最近 `max_files` 个文件，更旧的删除 —— 保留期 = 文件数 × 单文件上限；
//! - 进度（已归档到的最大 `id`）记在 `<dir>/watermark`。**先追加、后记水位**，
//!   于是崩溃只会造成「至多一次重复」，**绝不丢行**（重复行可接受：这是一份可读的明细，
//!   不是账本真源）。
//!
//! 射程（如实）：本模块**只写**，不碰任何读路径、不改账本、不进 `settle` 事务。真正的
//! 调度在 `main.rs` 里：定时任务用 `try_lock` 拿库 —— 库正忙（有请求在等慢查询）就跳过
//! 本轮，归档**绝不与请求抢锁**。

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// 归档文件名后缀。
const SUFFIX: &str = ".jsonl";
/// 进度文件名（**每个归档目录一个** —— 目录不同，名字就可以同一个）。
const WATERMARK: &str = "watermark";

/// `usage_records` 归档在交易归档目录下的**子目录名**（唯一拼写点）。
///
/// 两张表的归档共用一个根（`[archive] dir`），各自一个子目录：水位文件同名，靠目录隔离。
pub const USAGE_SUBDIR: &str = "usage";

/// 一行明细 → 一行 JSON（字段名 == 列名，值原样，不做换算/格式化）。
pub type RowJson = fn(&rusqlite::Row<'_>) -> rusqlite::Result<String>;

/// 一张可归档表的规格。
///
/// `columns` 是 `SELECT` 的列清单**也是** JSON 字段名的依据 —— 但 JSON 那半由 `json` 函数
/// 自己拼（两张表的列类型不同，`Option<i64>` 与 `f64` 不能共用一段代码）。两个载体
/// （列清单、JSON 键）都要与真 schema 对齐，由 [`tests::the_archive_covers_every_column_of_the_table`]
/// 拿迁移后的 `PRAGMA table_info` 在测试期兜着，而不是靠注释承诺。
pub struct Spec {
    /// 表名（也用于日志点名是哪个表的归档）。
    pub table: &'static str,
    /// 归档文件名前缀（序号 6 位零填充 ⇒ 字典序 == 序号序）。
    pub prefix: &'static str,
    /// 列清单，**顺序即 JSON 字段出现顺序**，也是 `SELECT` 的列清单。
    pub columns: &'static [&'static str],
    /// 一行 → 一行 JSON。
    pub json: RowJson,
}

/// `transactions` 的归档规格。
pub const TRANSACTIONS: Spec = Spec {
    table: "transactions",
    prefix: "transactions-",
    columns: &[
        "id",
        "user_id",
        "counterpart",
        "key_id",
        "api_key_id",
        "model",
        "tokens",
        "cached_tokens",
        "output_tokens",
        "pts",
        "type",
        "status",
        "time",
    ],
    json: tx_json,
};

/// `usage_records` 的归档规格。
///
/// 它比 `transactions` 少 `counterpart`/`pts`/`type`/`status`，多一个 `cost`
/// （**锚定货币金额**，`billing.rs` 的 `SettleParams::cost`）—— `cost` 是这张表独有的列，
/// 也是「删旧行之前必须先归档」的唯一理由（其余列在 `transactions` 里有近亲）。
pub const USAGE_RECORDS: Spec = Spec {
    table: "usage_records",
    prefix: "usage_records-",
    columns: &[
        "id",
        "user_id",
        "api_key_id",
        "key_id",
        "model",
        "tokens",
        "cached_tokens",
        "output_tokens",
        "cost",
        "time",
    ],
    json: usage_json,
};

/// 一份可滚动、有保留期的明细归档。
pub struct Archive {
    spec: &'static Spec,
    dir: PathBuf,
    max_bytes: u64,
    max_files: usize,
}

impl Archive {
    /// 建目录（幂等）。`max_files` 至少为 1 —— 否则保留策略会把刚写的文件也删掉。
    pub fn new(
        spec: &'static Spec,
        dir: impl Into<PathBuf>,
        max_bytes: u64,
        max_files: usize,
    ) -> Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir).with_context(|| format!("创建归档目录失败: {}", dir.display()))?;
        Ok(Self {
            spec,
            dir,
            max_bytes,
            max_files: max_files.max(1),
        })
    }

    /// 本归档服务的表规格。
    pub fn spec(&self) -> &'static Spec {
        self.spec
    }

    /// 归档目录（供启动日志点名「从哪个文件看」）。
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// 现存归档文件，按序号升序（旧 → 新）。只收**本规格前缀**的文件。
    pub fn files(&self) -> Vec<PathBuf> {
        self.numbered().into_iter().map(|(_, p)| p).collect()
    }

    /// 追加若干 JSON 行（每行一个对象，调用方负责不含换行）；必要时滚动，随后按保留期清理。
    pub fn append(&self, lines: &[String]) -> Result<()> {
        if lines.is_empty() {
            return Ok(());
        }
        let mut buf = String::new();
        for l in lines {
            buf.push_str(l);
            buf.push('\n');
        }
        let target = self.target_file(buf.len() as u64);
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&target)
            .with_context(|| format!("打开归档文件失败: {}", target.display()))?;
        f.write_all(buf.as_bytes())?;
        self.retain();
        Ok(())
    }

    /// 保留最近 `max_files` 个文件，更旧的删除。
    pub fn retain(&self) {
        let files = self.files();
        if files.len() > self.max_files {
            for stale in &files[..files.len() - self.max_files] {
                let _ = fs::remove_file(stale);
            }
        }
    }

    /// 已归档到的最大 `id`（无进度文件 ⇒ 0，即从头归档）。
    pub fn load_watermark(&self) -> i64 {
        watermark_at(&self.dir)
    }

    /// 记进度。**必须在 `append` 成功之后调用**（见模块头「至多一次重复」）。
    pub fn save_watermark(&self, id: i64) -> Result<()> {
        fs::write(self.dir.join(WATERMARK), id.to_string())
            .with_context(|| "写入归档进度失败".to_string())
    }

    /// `(序号, 路径)` 升序。
    fn numbered(&self) -> Vec<(u64, PathBuf)> {
        let mut v: Vec<(u64, PathBuf)> = match fs::read_dir(&self.dir) {
            Ok(rd) => rd
                .flatten()
                .filter_map(|e| {
                    let p = e.path();
                    let name = p.file_name()?.to_str()?;
                    let seq = name
                        .strip_prefix(self.spec.prefix)?
                        .strip_suffix(SUFFIX)?
                        .parse::<u64>()
                        .ok()?;
                    Some((seq, p))
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        v.sort_by_key(|(seq, _)| *seq);
        v
    }

    /// 本批应落到哪个文件：当前（序号最大）文件装得下就续写，否则开新文件。
    fn target_file(&self, incoming: u64) -> PathBuf {
        match self.numbered().last() {
            Some((_, newest))
                if fs::metadata(newest).map(|m| m.len()).unwrap_or(u64::MAX) + incoming
                    <= self.max_bytes =>
            {
                newest.clone()
            }
            Some((seq, _)) => self.path_for(seq + 1),
            None => self.path_for(1),
        }
    }

    fn path_for(&self, seq: u64) -> PathBuf {
        self.dir
            .join(format!("{}{seq:06}{SUFFIX}", self.spec.prefix))
    }
}

/// 归档目录里已归档到的最大 `id`（无进度文件 ⇒ 0，即从头归档）。
///
/// 独立成自由函数是给**汇总**用的：`tx_rollup` 只折叠已归档的明细，而它不该为了读一个数字
/// 去构造一个 `Archive`（那要连带决定单文件大小与保留文件数）。水位文件的名字只有这一个载体。
pub fn watermark_at(dir: &Path) -> i64 {
    fs::read_to_string(dir.join(WATERMARK))
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(0)
}

/// `SELECT` 的列清单**从 `Spec::columns` 渲染**，表名从 `Spec::table` 渲染 ——
/// 于是「列清单」与「表名」各只有一个载体。
fn select_sql(spec: &Spec) -> String {
    format!(
        "SELECT {} FROM {} WHERE id > ?1 ORDER BY id LIMIT ?2",
        spec.columns.join(", "),
        spec.table
    )
}

/// 一行事务 → 一行 JSON（字段名 == 列名，值原样，不做换算/格式化）。
fn tx_json(row: &rusqlite::Row<'_>) -> rusqlite::Result<String> {
    let v = serde_json::json!({
        "id": row.get::<_, i64>(0)?,
        "user_id": row.get::<_, i64>(1)?,
        "counterpart": row.get::<_, String>(2)?,
        "key_id": row.get::<_, Option<i64>>(3)?,
        "api_key_id": row.get::<_, Option<i64>>(4)?,
        "model": row.get::<_, String>(5)?,
        "tokens": row.get::<_, f64>(6)?,
        "cached_tokens": row.get::<_, f64>(7)?,
        "output_tokens": row.get::<_, f64>(8)?,
        "pts": row.get::<_, f64>(9)?,
        "type": row.get::<_, String>(10)?,
        "status": row.get::<_, String>(11)?,
        "time": row.get::<_, String>(12)?,
    });
    Ok(v.to_string())
}

/// 一行用量记录 → 一行 JSON（与 [`tx_json`] 同规：字段名 == 列名，值原样）。
fn usage_json(row: &rusqlite::Row<'_>) -> rusqlite::Result<String> {
    let v = serde_json::json!({
        "id": row.get::<_, i64>(0)?,
        "user_id": row.get::<_, i64>(1)?,
        "api_key_id": row.get::<_, Option<i64>>(2)?,
        "key_id": row.get::<_, Option<i64>>(3)?,
        "model": row.get::<_, String>(4)?,
        "tokens": row.get::<_, f64>(5)?,
        "cached_tokens": row.get::<_, f64>(6)?,
        "output_tokens": row.get::<_, f64>(7)?,
        "cost": row.get::<_, f64>(8)?,
        "time": row.get::<_, String>(9)?,
    });
    Ok(v.to_string())
}

/// 把 `ar` 对应的表里**尚未归档**的行（`id > watermark`，至多 `batch` 条）追加到 JSONL，
/// 成功后推进水位；返回本轮写入的行数（0 = 已追平）。
pub fn archive_pending(conn: &Connection, ar: &Archive, batch: i64) -> Result<usize> {
    let since = ar.load_watermark();
    let mut stmt = conn.prepare(&select_sql(ar.spec()))?;
    let mut rows = stmt.query(rusqlite::params![since, batch])?;
    let mut lines: Vec<String> = Vec::new();
    let mut max_id = since;
    while let Some(row) = rows.next()? {
        max_id = row.get::<_, i64>(0)?;
        lines.push((ar.spec().json)(row)?);
    }
    if lines.is_empty() {
        return Ok(0);
    }
    ar.append(&lines)?;
    ar.save_watermark(max_id)?;
    Ok(lines.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

    /// 每个测试自己的临时目录（`cargo test` 并行跑 ⇒ 名字必须唯一）。
    fn tmpdir(tag: &str) -> PathBuf {
        let n = TMP_SEQ.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!(
            "atp_archive_{}_{}_{}_{}",
            std::process::id(),
            tag,
            n,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&d);
        d
    }

    fn lines_of(p: &Path) -> Vec<String> {
        fs::read_to_string(p)
            .unwrap()
            .lines()
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// 建一个按真 schema 迁移过的内存库 —— 列集合由 `db::migrate` 决定，不是抄的。
    /// 同时种下 users 1..=8：两张明细表的 `user_id` 都有外键，没用户就插不进去。
    fn migrated_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        for id in 1..=8 {
            conn.execute(
                "INSERT INTO users (id, email, password_hash) VALUES (?1, ?2, 'h')",
                rusqlite::params![id, format!("u{id}@test")],
            )
            .unwrap();
        }
        conn
    }

    fn insert_tx(conn: &Connection, user_id: i64, pts: f64, ty: &str) {
        conn.execute(
            "INSERT INTO transactions (user_id, counterpart, key_id, api_key_id, model, \
             tokens, cached_tokens, output_tokens, pts, type, status) \
             VALUES (?1, '', NULL, NULL, 'm', 10, 1, 2, ?2, ?3, '成功')",
            rusqlite::params![user_id, pts, ty],
        )
        .unwrap();
    }

    fn insert_usage(conn: &Connection, user_id: i64, cost: f64) {
        conn.execute(
            "INSERT INTO usage_records (user_id, api_key_id, key_id, model, tokens, \
             cached_tokens, output_tokens, cost) \
             VALUES (?1, NULL, NULL, 'm', 10, 1, 2, ?2)",
            rusqlite::params![user_id, cost],
        )
        .unwrap();
    }

    /// 往 `spec` 对应的表里种一行（**测试**的分派，不是生产代码的分支）。
    fn seed_row(conn: &Connection, table: &str) {
        match table {
            "transactions" => insert_tx(conn, 1, 1.0, "consume"),
            "usage_records" => insert_usage(conn, 1, 2.5),
            other => panic!("没有为 {other} 准备夹具"),
        }
    }

    #[test]
    fn append_rolls_by_size_and_keeps_only_the_retention_window() {
        let dir = tmpdir("roll");
        // 每行 ~50 字节，max_bytes 150 ⇒ 约 2 行一滚；只留 2 个文件。
        let ar = Archive::new(&TRANSACTIONS, &dir, 150, 2).unwrap();
        let pad = "x".repeat(30);
        for i in 0..12 {
            ar.append(&[format!("{{\"id\":{i},\"pad\":\"{pad}\"}}")])
                .unwrap();
        }
        let files = ar.files();
        assert_eq!(files.len(), 2, "保留期应只留 2 个文件，实际 {files:?}");
        assert_eq!(
            ar.load_watermark(),
            0,
            "归档只写文件，不该顺带写水位（水位由 archive_pending 管）"
        );
        // 文件按序号升序，且**最后一行**仍是最新的那条（保留策略删的是最旧，不是最新）
        let last = files.last().unwrap();
        let tail = fs::read_to_string(last).unwrap();
        assert!(
            tail.contains("{\"id\":11"),
            "最新行必须留在最后一个文件里: {tail}"
        );
        // 每一行都是合法 JSON 对象
        for f in &files {
            for l in lines_of(f) {
                let v: serde_json::Value = serde_json::from_str(&l).expect("每行都应能解析为 JSON");
                assert!(v.is_object(), "每行都应是对象: {l}");
            }
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn archive_pending_writes_new_rows_and_advances_the_watermark() {
        let dir = tmpdir("pending");
        let conn = migrated_db();
        insert_tx(&conn, 1, 1.5, "consume");
        insert_tx(&conn, 2, 2.5, "earn");
        insert_tx(&conn, 1, 3.0, "gift");
        let ar = Archive::new(&TRANSACTIONS, &dir, 1_000_000, 7).unwrap();

        assert_eq!(ar.load_watermark(), 0, "无进度文件时水位为 0（从头归档）");
        assert_eq!(archive_pending(&conn, &ar, 100).unwrap(), 3);
        assert_eq!(ar.load_watermark(), 3);

        // 幂等：没有新行就不会重复写、水位不动
        assert_eq!(archive_pending(&conn, &ar, 100).unwrap(), 0);
        assert_eq!(ar.load_watermark(), 3);

        let files = ar.files();
        assert_eq!(files.len(), 1);
        assert_eq!(lines_of(&files[0]).len(), 3, "三行明细应各占一行");

        // 新行只写新增的那些
        insert_tx(&conn, 1, 4.0, "consume");
        assert_eq!(archive_pending(&conn, &ar, 100).unwrap(), 1);
        assert_eq!(lines_of(&files[0]).len(), 4);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn archive_pending_is_bounded_by_its_batch() {
        let dir = tmpdir("batch");
        let conn = migrated_db();
        for _ in 0..5 {
            insert_tx(&conn, 1, 1.0, "consume");
        }
        let ar = Archive::new(&TRANSACTIONS, &dir, 1_000_000, 7).unwrap();
        // 每轮最多 batch 行 ⇒ 长库不会被一次全表扫描拖住（定时任务持锁时间有界）
        assert_eq!(archive_pending(&conn, &ar, 2).unwrap(), 2);
        assert_eq!(ar.load_watermark(), 2);
        assert_eq!(archive_pending(&conn, &ar, 2).unwrap(), 2);
        assert_eq!(ar.load_watermark(), 4);
        assert_eq!(archive_pending(&conn, &ar, 2).unwrap(), 1);
        assert_eq!(ar.load_watermark(), 5);
        assert_eq!(archive_pending(&conn, &ar, 2).unwrap(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 一个事实两个载体：归档列清单（`Spec::columns` + JSON 字段名）必须**覆盖该表的每一列**
    /// （真源是迁移后的 `PRAGMA table_info`，不是抄的）。新增列只改一处 ⇒ 红。
    ///
    /// 两张表都要过 —— 「列清单」这个载体每张表各有一份，漏掉哪张都算漂移。
    #[test]
    fn the_archive_covers_every_column_of_the_table() {
        let conn = migrated_db();
        for spec in [&TRANSACTIONS, &USAGE_RECORDS] {
            let mut stmt = conn
                .prepare(&format!("PRAGMA table_info({})", spec.table))
                .unwrap();
            let table: Vec<String> = stmt
                .query_map([], |r| r.get::<_, String>(1))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            assert!(
                table.len() >= 10,
                "{} 应至少有 10 列: {table:?}",
                spec.table
            );

            // ① SELECT 的列清单 == 表列集合
            let mut selected: Vec<String> = spec.columns.iter().map(|s| s.to_string()).collect();
            selected.sort();
            let mut expected = table.clone();
            expected.sort();
            assert_eq!(
                selected, expected,
                "{} 的 Spec::columns 必须与表的列集合一致（新增列只改一处即为漂移）",
                spec.table
            );

            // ② 真跑一遍，JSON 的**字段名**也必须 == 表列集合（json 里的键是另一个载体）
            let dir = tmpdir(&format!("covers_{}", spec.table));
            let ar = Archive::new(spec, &dir, 1_000_000, 7).unwrap();
            seed_row(&conn, spec.table);
            let before = archive_pending(&conn, &ar, 10).unwrap();
            assert!(before >= 1, "{} 至少应归档到刚种下的那一行", spec.table);
            let line = lines_of(&ar.files()[0]).remove(0);
            let v: serde_json::Value = serde_json::from_str(&line).unwrap();
            let mut keys: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            assert_eq!(
                keys, expected,
                "{} 的 JSON 字段名必须覆盖该表的每一列",
                spec.table
            );
            let _ = fs::remove_dir_all(&dir);
        }
    }

    /// 值原样搬运（不做换算/格式化）：`pts` 的小数、`type` 字符串、空 `counterpart` 都要照抄。
    #[test]
    fn the_archived_values_are_copied_verbatim() {
        let dir = tmpdir("verbatim");
        let conn = migrated_db();
        conn.execute(
            "INSERT INTO transactions (user_id, counterpart, key_id, api_key_id, model, \
             tokens, cached_tokens, output_tokens, pts, type, status) \
             VALUES (7, '42', NULL, NULL, 'deepseek-v4-pro', 1234, 56, 78, 1.23456, 'consume', '成功')",
            [],
        )
        .unwrap();
        let ar = Archive::new(&TRANSACTIONS, &dir, 1_000_000, 7).unwrap();
        assert_eq!(archive_pending(&conn, &ar, 10).unwrap(), 1);
        let v: serde_json::Value = serde_json::from_str(&lines_of(&ar.files()[0])[0]).unwrap();
        assert_eq!(v["user_id"], 7);
        assert_eq!(v["counterpart"], "42");
        assert_eq!(v["model"], "deepseek-v4-pro");
        assert_eq!(v["tokens"], 1234.0);
        assert_eq!(v["pts"], 1.23456);
        assert_eq!(v["type"], "consume");
        assert_eq!(v["status"], "成功");
        assert_eq!(
            v["key_id"],
            serde_json::Value::Null,
            "NULL 列应序列化为 null"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// `usage_records` 的独有列 `cost`（锚定货币金额）必须逐字进 JSON —— 这正是「删旧行之前
    /// 必须先归档」的理由：删掉之后这张表是它唯一的落脚处。
    #[test]
    fn the_usage_archive_carries_the_cost_column_verbatim() {
        let dir = tmpdir("usage_cost");
        let conn = migrated_db();
        conn.execute(
            "INSERT INTO usage_records (user_id, api_key_id, key_id, model, tokens, \
             cached_tokens, output_tokens, cost) \
             VALUES (3, 5, 6, 'deepseek-v4-pro', 1234, 56, 78, 1.23456)",
            [],
        )
        .unwrap();
        let ar = Archive::new(&USAGE_RECORDS, &dir, 1_000_000, 7).unwrap();
        assert_eq!(archive_pending(&conn, &ar, 10).unwrap(), 1);
        let v: serde_json::Value = serde_json::from_str(&lines_of(&ar.files()[0])[0]).unwrap();
        assert_eq!(v["user_id"], 3);
        assert_eq!(v["api_key_id"], 5);
        assert_eq!(v["key_id"], 6);
        assert_eq!(v["model"], "deepseek-v4-pro");
        assert_eq!(v["tokens"], 1234.0);
        assert_eq!(v["cached_tokens"], 56.0);
        assert_eq!(v["output_tokens"], 78.0);
        assert_eq!(v["cost"], 1.23456);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 两份归档**住在一个根目录下**（交易在 `<dir>`、用量在 `<dir>/usage`）时必须互不干扰：
    /// 水位文件同名、靠目录隔离；文件名前缀是第二道保险 —— `files()` 只收自己前缀的文件，
    /// 量表的保留策略也就删不到交易的文件。
    #[test]
    fn the_two_archives_share_a_root_without_stepping_on_each_other() {
        let root = tmpdir("nested");
        let conn = migrated_db();
        let tx = Archive::new(&TRANSACTIONS, &root, 1_000_000, 7).unwrap();
        let us = Archive::new(&USAGE_RECORDS, root.join(USAGE_SUBDIR), 1_000_000, 7).unwrap();

        insert_tx(&conn, 1, 1.0, "consume");
        insert_tx(&conn, 1, 2.0, "consume");
        insert_usage(&conn, 1, 0.5);

        assert_eq!(archive_pending(&conn, &tx, 10).unwrap(), 2);
        assert_eq!(archive_pending(&conn, &us, 10).unwrap(), 1);

        // 各自的文件清单只含自己的文件（同一个根目录下的 read_dir 不串味）
        assert_eq!(tx.files().len(), 1);
        assert_eq!(us.files().len(), 1);
        assert!(
            tx.files()[0]
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("transactions-"),
            "交易归档的文件名不受用量归档影响: {:?}",
            tx.files()[0]
        );
        assert!(
            us.files()[0]
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("usage_records-"),
            "用量归档的文件名不受交易归档影响: {:?}",
            us.files()[0]
        );
        // 水位各自独立：2 行交易 vs 1 行用量
        assert_eq!(tx.load_watermark(), 2);
        assert_eq!(us.load_watermark(), 1);
        // 交易那份文件里只有交易行（用量行没有混进同一个文件）
        assert_eq!(lines_of(&tx.files()[0]).len(), 2);
        assert_eq!(lines_of(&us.files()[0]).len(), 1);
        let _ = fs::remove_dir_all(&root);
    }
}
