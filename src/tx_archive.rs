//! 交易明细 JSONL 归档（rant 2026-10-09T12:28:58 验收项 2）。
//!
//! 背景：`transactions` 只增不减、没有保留策略 ⇒ SQLite 无限膨胀、聚合查询越来越慢
//! （dev 实测 97.3 万行：`COUNT(*)` 23.7s、30 天窗口聚合 32.5s）。宿主把治本诉求收敛为
//! 两条，本模块是**第二条**：「详细记录写入可滚动、有保留期的 JSONL 文件，从文件查看即可」。
//! （第一条「明细只留汇总」——按 类型+用户+模型+key+状态+分钟 折叠成可加汇总行——是后续
//! 改动；本模块是它的**前置**：先保证明细有落处，折叠时删掉的行才不会丢。）
//!
//! 形态：
//! - 一次滚动 = 一个文件 `<dir>/transactions-<6 位序号>.jsonl`，**每行一个事务的 JSON 对象**；
//! - 当前文件写入后再追加会超过 `max_file_size` 字节 ⇒ 开新文件（序号 +1）；
//! - 只保留最近 `max_files` 个文件，更旧的删除 —— 保留期 = 文件数 × 单文件上限；
//! - 进度（已归档到的最大 `transactions.id`）记在 `<dir>/watermark`。**先追加、后记水位**，
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

/// 归档文件名前缀（序号 6 位零填充 ⇒ 字典序 == 序号序，人眼看文件列表也按时间）。
const PREFIX: &str = "transactions-";
/// 归档文件名后缀。
const SUFFIX: &str = ".jsonl";
/// 进度文件名。
const WATERMARK: &str = "watermark";

/// 归档的列 —— **顺序即 JSON 字段出现顺序**，也是 `SELECT` 的列清单。
///
/// 这份清单是 `transactions` 表的第二个载体（真源是 `db.rs` 的建表 + `ensure_column`），
/// 漂移（新增列只改一处）由 `the_archive_covers_every_column_of_the_table` 用**真 schema**
/// （`PRAGMA table_info`）在测试期兜着，而不是靠注释承诺。
const COLUMNS: &[&str] = &[
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
];

/// 一份可滚动、有保留期的交易明细归档。
pub struct TxArchive {
    dir: PathBuf,
    max_bytes: u64,
    max_files: usize,
}

impl TxArchive {
    /// 建目录（幂等）。`max_files` 至少为 1 —— 否则保留策略会把刚写的文件也删掉。
    pub fn new(dir: impl Into<PathBuf>, max_bytes: u64, max_files: usize) -> Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir).with_context(|| format!("创建归档目录失败: {}", dir.display()))?;
        Ok(Self {
            dir,
            max_bytes,
            max_files: max_files.max(1),
        })
    }

    /// 归档目录（供启动日志点名「从哪个文件看」）。
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// 现存归档文件，按序号升序（旧 → 新）。
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

    /// 已归档到的最大 `transactions.id`（无进度文件 ⇒ 0，即从头归档）。
    pub fn load_watermark(&self) -> i64 {
        fs::read_to_string(self.dir.join(WATERMARK))
            .ok()
            .and_then(|s| s.trim().parse::<i64>().ok())
            .unwrap_or(0)
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
                        .strip_prefix(PREFIX)?
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
        self.dir.join(format!("{PREFIX}{seq:06}{SUFFIX}"))
    }
}

/// `SELECT` 的列清单**从 `COLUMNS` 渲染** —— 于是「列清单」只有一个载体。
fn select_sql() -> String {
    format!(
        "SELECT {} FROM transactions WHERE id > ?1 ORDER BY id LIMIT ?2",
        COLUMNS.join(", ")
    )
}

/// 一行事务 → 一行 JSON（字段名 == 列名，值原样，不做换算/格式化）。
fn row_json(row: &rusqlite::Row<'_>) -> rusqlite::Result<String> {
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

/// 把 `transactions` 里**尚未归档**的行（`id > watermark`，至多 `batch` 条）追加到 JSONL，
/// 成功后推进水位；返回本轮写入的行数（0 = 已追平）。
pub fn archive_pending(conn: &Connection, ar: &TxArchive, batch: i64) -> Result<usize> {
    let since = ar.load_watermark();
    let mut stmt = conn.prepare(&select_sql())?;
    let mut rows = stmt.query(rusqlite::params![since, batch])?;
    let mut lines: Vec<String> = Vec::new();
    let mut max_id = since;
    while let Some(row) = rows.next()? {
        max_id = row.get::<_, i64>(0)?;
        lines.push(row_json(row)?);
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
            "atp_txarchive_{}_{}_{}_{}",
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
    /// 同时种下 users 1..=8：`transactions.user_id` 有外键，没用户就插不进去。
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

    #[test]
    fn append_rolls_by_size_and_keeps_only_the_retention_window() {
        let dir = tmpdir("roll");
        // 每行 ~50 字节，max_bytes 150 ⇒ 约 2 行一滚；只留 2 个文件。
        let ar = TxArchive::new(&dir, 150, 2).unwrap();
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
        let ar = TxArchive::new(&dir, 1_000_000, 7).unwrap();

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
        let ar = TxArchive::new(&dir, 1_000_000, 7).unwrap();
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

    /// 一个事实两个载体：归档列清单（`COLUMNS` + JSON 字段名）必须**覆盖 `transactions`
    /// 的每一列**（真源是迁移后的 `PRAGMA table_info`，不是抄的）。新增列只改一处 ⇒ 红。
    #[test]
    fn the_archive_covers_every_column_of_the_table() {
        let conn = migrated_db();
        let mut stmt = conn.prepare("PRAGMA table_info(transactions)").unwrap();
        let table: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(table.len() >= 13, "transactions 应至少有 13 列: {table:?}");

        // ① SELECT 的列清单 == 表列集合
        let mut selected: Vec<String> = COLUMNS.iter().map(|s| s.to_string()).collect();
        selected.sort();
        let mut expected = table.clone();
        expected.sort();
        assert_eq!(
            selected, expected,
            "COLUMNS 必须与 transactions 的列集合一致（新增列只改一处即为漂移）"
        );

        // ② 真跑一遍，JSON 的**字段名**也必须 == 表列集合（json! 里的键是另一个载体）
        let dir = tmpdir("covers");
        let ar = TxArchive::new(&dir, 1_000_000, 7).unwrap();
        insert_tx(&conn, 1, 1.0, "consume");
        assert_eq!(archive_pending(&conn, &ar, 10).unwrap(), 1);
        let line = lines_of(&ar.files()[0]).remove(0);
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        let mut keys: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, expected, "JSON 字段名必须覆盖 transactions 的每一列");
        let _ = fs::remove_dir_all(&dir);
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
        let ar = TxArchive::new(&dir, 1_000_000, 7).unwrap();
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
}
