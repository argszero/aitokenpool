//! 交易明细汇总（rant 2026-10-09T12:28:58 验收项 1：「明细只留汇总」）。
//!
//! 背景：`transactions` 只增不减、没有保留策略 ⇒ SQLite 无限膨胀、聚合查询越来越慢并拖垮服务
//! （dev 实测 97.3 万行：`COUNT(*)` 23.7s、30 天窗口聚合 32.5s）。宿主把治本诉求收敛为两条，
//! 本模块是**第一条**：把明细折叠成**可加**的汇总行 —— 汇总维度 = 类型 + 用户 + 模型 + key +
//! 状态 + 时间（分钟级）；汇总行带可加量（`pts` 与各 token 的 `SUM`、以及行数 `COUNT`），
//! 于是「分成 / 收益 / 月流水」这些聚合仍算得出来。
//!
//! 第二条（`src/tx_archive.rs`：明细写可滚动、有保留期的 JSONL）是**本模块的前置**：汇总只允许
//! 折叠**已经进了 JSONL** 的明细（`id ≤ 归档水位`），于是将来「删掉已折叠的明细」永远不丢原件
//! —— 归档水位就是「可以安全删除到哪里」的那条分界线。`fold_pending` 因此把归档水位**当参数收**，
//! 调用方拿不到水位就折不动。
//!
//! 形态：
//! - 每个**桶**（维度的一组取值）恰好一行 —— 唯一索引钉住这一点，后到的批次走 `ON CONFLICT`
//!   **原地累加**（不是再插一行），所以读侧不需要二次聚合；
//! - 行上带 `up_to_id`（本行已折叠到的最大 `transactions.id`），表上 `MAX(up_to_id)` 就是水位
//!   —— 不另立进度文件，水位与数据**天然同一次事务**，崩溃不可能只推进一半；
//! - 每批至多 `batch` 条**明细**（不是至多 `batch` 个桶）：长库不会被一次全表扫描拖住。
//!
//! 射程（如实）：本模块**只写**这张汇总表。它不碰任何读路径、不改账本、不进 `settle` 事务；
//! 明细的**删除**与读路径的改接是后续切片，本模块只负责让汇总行先存在、且推导关系可对账。
//!
//! 与 rant 那句维度的两处如实记录：
//! ① `key` 在库里有**两个**身份 —— `key_id`（上游 key）与 `api_key_id`（分发 key）。交易表
//!    「Key」列显示的是 `api_key_id` 侧的名字（`api_keys.name`），而分成/收益按 `key_id` 归集
//!    ⇒ 只按其中之一折叠会让另一侧永远答不出来，所以两个都进维度（维度更细只是更精确）。
//! ② `counterpart` 既不是维度也不是可加量（它是展示用的对手方标识），故不进汇总。

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

/// 汇总表名。建表语句在 `db.rs` 的 v16 迁移里（schema 的唯一归属地）。
pub const TABLE: &str = "transactions_rollup";

/// 汇总维度：列名 → 从明细取值的表达式。
///
/// 这是**唯一载体**：`SELECT ... AS <列名>`、`GROUP BY <表达式>`、`ON CONFLICT(<列名>)` 三处
/// 都从它渲染，唯一索引的列序也与它一致（漂移由 `the_fold_writes_exactly_the_rollup_columns` 在
/// 测试期拿**真** `PRAGMA table_info` 兜着）。
///
/// 两个 `key` 列用 `IFNULL(..., 0)` 归一：rowid 从 1 起 ⇒ `0` 就是「没有 key」的哨兵。
/// 这一步不是为了好看 —— SQLite 里 NULL 互不相等，若让它们以 NULL 进唯一索引，同一桶的第二批
/// 会**再插一行**而不是原地累加（`a_bucket_row_is_updated_in_place_...` 钉住这条）。
pub(crate) const DIMENSION: &[(&str, &str)] = &[
    ("user_id", "user_id"),
    ("model", "model"),
    ("key_id", "IFNULL(key_id, 0)"),
    ("api_key_id", "IFNULL(api_key_id, 0)"),
    ("type", "type"),
    ("status", "status"),
    ("bucket", "substr(time, 1, 16)"),
];

/// 可加量：列名 → 从明细聚合的表达式。与维度分开是因为它们的合并方式不同（累加 vs 冲突即原地加）。
pub(crate) const ADDITIVE: &[(&str, &str)] = &[
    ("row_count", "COUNT(*)"),
    ("pts", "SUM(pts)"),
    ("tokens", "SUM(tokens)"),
    ("cached_tokens", "SUM(cached_tokens)"),
    ("output_tokens", "SUM(output_tokens)"),
];

/// 水位列：本行已折叠到的最大 `transactions.id`。合并方式取**较大者**。
pub(crate) const UP_TO: &str = "up_to_id";

/// 折叠 SQL：从一批明细聚合后 UPSERT 进汇总表。
///
/// 批次上界由调用方先算好（`id ≤ ?2`）再传进来，SQL 里不再出现 `LIMIT` —— `LIMIT` 作用在
/// **聚合之后**的行数上，会与「每批至多 `batch` 条明细」这个界混淆。
fn fold_sql() -> String {
    let sel: Vec<String> = DIMENSION
        .iter()
        .map(|(col, expr)| format!("{expr} AS {col}"))
        .chain(
            ADDITIVE
                .iter()
                .map(|(col, expr)| format!("{expr} AS {col}")),
        )
        .collect();
    let cols: Vec<&str> = DIMENSION
        .iter()
        .map(|(col, _)| *col)
        .chain(ADDITIVE.iter().map(|(col, _)| *col))
        .chain(std::iter::once(UP_TO))
        .collect();
    let group: Vec<&str> = DIMENSION.iter().map(|(_, expr)| *expr).collect();
    let conflict: Vec<&str> = DIMENSION.iter().map(|(col, _)| *col).collect();
    let update: Vec<String> = ADDITIVE
        .iter()
        .map(|(col, _)| format!("{col} = {col} + excluded.{col}"))
        .chain(std::iter::once(format!(
            "{UP_TO} = MAX({UP_TO}, excluded.{UP_TO})"
        )))
        .collect();
    format!(
        "INSERT INTO {TABLE} ({}) \
         SELECT {} , MAX(id) FROM transactions WHERE id > ?1 AND id <= ?2 \
         GROUP BY {} \
         ON CONFLICT({}) DO UPDATE SET {}",
        cols.join(", "),
        sel.join(", "),
        group.join(", "),
        conflict.join(", "),
        update.join(", "),
    )
}

/// 汇总表**除自增主键外**的列清单 —— [`fold_sql`] 写入的列。
///
/// 只有测试读它（拿它与真 `PRAGMA table_info` 相比）；发布产物里没有读者，故 `#[cfg(test)]`
/// —— 不加的话 `cargo build` 会报 `dead_code`，而 `clippy --all-targets` 看不见那条警告
/// （它连测试目标一起编，于是这个函数在它眼里「被读过」）。
#[cfg(test)]
fn written_columns() -> Vec<&'static str> {
    DIMENSION
        .iter()
        .map(|(col, _)| *col)
        .chain(ADDITIVE.iter().map(|(col, _)| *col))
        .chain(std::iter::once(UP_TO))
        .collect()
}

/// 已折叠到的最大 `transactions.id`（表空 ⇒ 0，即从头折叠）。
pub fn watermark(conn: &Connection) -> Result<i64> {
    conn.query_row(
        &format!("SELECT COALESCE(MAX({UP_TO}), 0) FROM {TABLE}"),
        [],
        |r| r.get(0),
    )
    .with_context(|| "读取汇总水位失败".to_string())
}

/// 把一批**已归档**的明细折叠进汇总表，返回本批折叠的明细行数（0 = 已追平或无可折叠）。
///
/// `archive_watermark` 是 [`crate::tx_archive`] 的归档水位：只有 `id ≤ archive_watermark` 的
/// 明细允许进入汇总。传入 0（归档未启用）⇒ 一行都不折 —— 这是**刻意的 fail-closed**：
/// 折叠过的明细将来会被删除，而未归档的明细删了就真没了。
///
/// 折叠与读水位**同一次事务**：`up_to_id` 与数据一起落盘，不存在「数据进了、水位没进」的中间态。
pub fn fold_pending(conn: &Connection, archive_watermark: i64, batch: i64) -> Result<usize> {
    let since = watermark(conn)?;
    if batch <= 0 || archive_watermark <= since {
        return Ok(0);
    }
    // 本批的**明细**上界：按 id 升序取至多 batch 条，取这批里最大的 id。
    // （下一批从 since+1 续，故「取前 batch 条」与「id ≤ batch_max」是同一个集合。）
    let batch_max: Option<i64> = conn
        .query_row(
            "SELECT MAX(id) FROM \
             (SELECT id FROM transactions WHERE id > ?1 AND id <= ?2 ORDER BY id LIMIT ?3)",
            params![since, archive_watermark, batch],
            |r| r.get(0),
        )
        .with_context(|| "计算汇总批次上界失败".to_string())?;
    let Some(batch_max) = batch_max else {
        return Ok(0);
    };

    let tx = conn
        .unchecked_transaction()
        .with_context(|| "开启汇总事务失败".to_string())?;
    tx.execute(&fold_sql(), params![since, batch_max])
        .with_context(|| "折叠交易明细失败".to_string())?;
    let folded: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE id > ?1 AND id <= ?2",
            params![since, batch_max],
            |r| r.get(0),
        )
        .with_context(|| "统计本批明细行数失败".to_string())?;
    tx.commit()
        .with_context(|| "提交汇总事务失败".to_string())?;
    Ok(folded as usize)
}

/// 删除**已折叠、已归档、且早于保留窗口**的明细，返回本批删除的行数（0 = 无需删或本轮已删空）。
///
/// 这是验收项 1「明细只留汇总」的**收尾动作** —— 没有它，`transactions` 仍然只增不减，折叠只是
/// 在旁边多长出一张同样在涨的表。三道上界缺一不可，每道都在防一种「删了就回不来」：
///
/// 1. **`id ≤ MIN(汇总水位, 归档水位)`** —— 只删汇总表已经算过、且原件已经进 JSONL 的行。
///    汇总水位是 `fold_pending` 自己写下的 `up_to_id`，归档水位由调用方传入（与折叠**同源**）；
///    取小 ⇒ 归档一旦落后（或归档目录丢失 ⇒ 水位 0）就一行都不删，与折叠同一条 fail-closed 纪律。
/// 2. **`time < now - retain_days`** —— 保留窗口内的明细仍逐条留在库里（交易页翻页、共享页按行
///    读的都是 `transactions` 本身）。窗口是**明细**的保留期；**汇总行永远不删**。
/// 3. **`LIMIT batch`** —— 单次持库时间有界，后台维护绝不与请求抢锁（与折叠、归档同一条纪律）。
///
/// 按 `id` 升序删 ⇒ 先删最旧的；`id` 单调（`AUTOINCREMENT`，删了也不会被复用），所以扫描一撞到
/// 保留窗口就停，不会为了找几行而翻遍全表。
///
/// **对读模型无影响**：被删的行都满足 `id ≤ 汇总水位`，而视图 `tx_facts` 的明细臂只取
/// `id > 汇总水位` —— 这些行在删除前后都**只**由汇总臂表示
/// （`the_read_model_answers_the_same_aggregates_across_the_deletion` 用真库逐列钉住这条）。
pub fn delete_folded(
    conn: &Connection,
    archive_watermark: i64,
    retain_days: i64,
    batch: i64,
) -> Result<usize> {
    let upto = watermark(conn)?.min(archive_watermark);
    if batch <= 0 || upto <= 0 {
        return Ok(0);
    }
    let window = format!("-{} days", retain_days.max(0));
    conn.execute(
        "DELETE FROM transactions WHERE id IN ( \
           SELECT id FROM transactions \
           WHERE id <= ?1 AND time < datetime('now', ?2) \
           ORDER BY id LIMIT ?3 \
         )",
        params![upto, window, batch],
    )
    .with_context(|| "删除已折叠的明细失败".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建一个按真 schema 迁移过的内存库，并种下 users 1..=8
    /// （`transactions.user_id` 有外键，没用户就插不进去）。
    fn migrated_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        for id in 1..=8 {
            conn.execute(
                "INSERT INTO users (id, email, password_hash) VALUES (?1, ?2, 'h')",
                params![id, format!("u{id}@test")],
            )
            .unwrap();
        }
        conn
    }

    /// 一条明细。`time` 显式给（分钟桶是判据的一部分，不能靠 `datetime('now')`）。
    #[allow(clippy::too_many_arguments)]
    fn insert_tx(
        conn: &Connection,
        user_id: i64,
        key_id: Option<i64>,
        api_key_id: Option<i64>,
        model: &str,
        ty: &str,
        status: &str,
        time: &str,
        pts: f64,
        tokens: f64,
    ) {
        conn.execute(
            "INSERT INTO transactions (user_id, counterpart, key_id, api_key_id, model, \
             tokens, cached_tokens, output_tokens, pts, type, status, time) \
             VALUES (?1, '', ?2, ?3, ?4, ?5, 1.0, 2.0, ?6, ?7, ?8, ?9)",
            params![user_id, key_id, api_key_id, model, tokens, pts, ty, status, time],
        )
        .unwrap();
    }

    /// 汇总表里的 `(维度, 可加量)` —— 读回来做断言用。
    fn rollup_rows(conn: &Connection) -> Vec<(String, f64, f64, i64)> {
        let sql = format!(
            "SELECT bucket || '|' || user_id || '|' || type || '|' || model || '|' || \
             IFNULL(key_id, 0) || '|' || IFNULL(api_key_id, 0) || '|' || status, \
             pts, tokens, row_count FROM {TABLE} ORDER BY bucket, user_id, type, model"
        );
        let mut stmt = conn.prepare(&sql).unwrap();
        let v: Vec<(String, f64, f64, i64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        v
    }

    /// 明细侧的同口径聚合（维度串与 [`rollup_rows`] 完全一致）——
    /// 「汇总能不能替明细答」这条判据的参照量必须来自**明细本身**，不能来自另一个汇总读法。
    fn detail_rows(conn: &Connection) -> Vec<(String, f64, f64, i64)> {
        let mut stmt = conn
            .prepare(
                "SELECT substr(time, 1, 16) || '|' || user_id || '|' || type || '|' || model || '|' || \
                 IFNULL(key_id, 0) || '|' || IFNULL(api_key_id, 0) || '|' || status, \
                 SUM(pts), SUM(tokens), COUNT(*) FROM transactions \
                 GROUP BY substr(time, 1, 16), user_id, type, model, \
                 IFNULL(key_id, 0), IFNULL(api_key_id, 0), status \
                 ORDER BY substr(time, 1, 16), user_id, type, model",
            )
            .unwrap();
        let v: Vec<(String, f64, f64, i64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        v
    }

    #[test]
    fn fold_pending_groups_by_the_minute_and_sums_the_additive_quantities() {
        let conn = migrated_db();
        // 同一个用户/模型/key/类型/状态，**同一分钟**三行 ⇒ 一个桶，三个可加量都相加。
        for (sec, pts, tokens) in [("05", 1.5, 100.0), ("17", 2.0, 200.0), ("59", 0.5, 300.0)] {
            insert_tx(
                &conn,
                1,
                Some(9),
                Some(4),
                "m",
                "consume",
                "成功",
                &format!("2026-10-09 12:34:{sec}"),
                pts,
                tokens,
            );
        }
        // 相邻的一分钟 ⇒ 另一个桶（分钟级是判据的一部分）
        insert_tx(
            &conn,
            1,
            Some(9),
            Some(4),
            "m",
            "consume",
            "成功",
            "2026-10-09 12:35:00",
            7.0,
            700.0,
        );
        assert_eq!(fold_pending(&conn, 1_000, 100).unwrap(), 4);

        let rows = rollup_rows(&conn);
        assert_eq!(rows.len(), 2, "两个分钟桶应各一行: {rows:?}");
        let (bucket, pts, tokens, n) = &rows[0];
        assert_eq!(bucket, "2026-10-09 12:34|1|consume|m|9|4|成功");
        assert_eq!(*n, 3, "同一桶的三条明细应折成一行、行数 3");
        assert!((pts - 4.0).abs() < 1e-9, "pts 应相加: {pts}");
        assert!((tokens - 600.0).abs() < 1e-9, "tokens 应相加: {tokens}");
        assert_eq!(rows[1].3, 1, "另一分钟那一行应自成一行");
        // cached/output 也各自相加（夹具每条 1.0 / 2.0）
        let (cached, output): (f64, f64) = conn
            .query_row(
                &format!(
                    "SELECT cached_tokens, output_tokens FROM {TABLE} WHERE bucket = '2026-10-09 12:34'"
                ),
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(
            (cached - 3.0).abs() < 1e-9,
            "cached_tokens 应相加: {cached}"
        );
        assert!(
            (output - 6.0).abs() < 1e-9,
            "output_tokens 应相加: {output}"
        );
    }

    /// 汇总的**全部**意义在这一条：折叠之后，汇总表单独就能答出明细表能答的那些聚合
    /// （rant：「保证分成、收益、月流水这些还能算出来」）。
    #[test]
    fn the_rollup_answers_exactly_what_the_details_answer() {
        let conn = migrated_db();
        // 确定性夹具：跨用户 / 模型 / 两个 key 身份 / 类型 / 状态 / 分钟 / 含 NULL key
        let mut n = 0;
        for user in 1..=3i64 {
            for (i, ty) in ["consume", "earn", "topup", "gift"].iter().enumerate() {
                for minute in 0..3 {
                    for status in ["成功", "失败"] {
                        n += 1;
                        let key_id = if i % 2 == 0 { Some(7) } else { None };
                        let api_key_id = if minute == 2 { None } else { Some(3) };
                        insert_tx(
                            &conn,
                            user,
                            key_id,
                            api_key_id,
                            if i % 3 == 0 { "deepseek-v4-pro" } else { "" },
                            ty,
                            status,
                            &format!("2026-10-0{} 08:{:02}:00", 1 + minute, 5 * i + minute),
                            (i as f64 + 1.0) * 0.25,
                            (i as f64 + 1.0) * 1234.0,
                        );
                    }
                }
            }
        }
        assert_eq!(n, 3 * 4 * 3 * 2, "夹具规模");

        let before = detail_rows(&conn);
        // 折到追平。批次刻意取 10（< 夹具 72 条）⇒ 顺带走过多批次的合并路径，
        // 而不是「一批恰好装下」这条不承重的捷径。
        let mut folded = 0;
        loop {
            let k = fold_pending(&conn, 1_000_000, 10).unwrap();
            if k == 0 {
                break;
            }
            folded += k;
        }
        assert_eq!(folded, n, "折叠的明细条数应等于夹具规模");
        let after = rollup_rows(&conn);

        assert_eq!(after.len(), before.len(), "桶数应逐条相同（维度集合一致）");
        for (a, b) in after.iter().zip(before.iter()) {
            assert_eq!(a.0, b.0, "维度串应逐条相同");
            assert!(
                (a.1 - b.1).abs() < 1e-9,
                "{} 的 pts: {} vs {}",
                a.0,
                a.1,
                b.1
            );
            assert!(
                (a.2 - b.2).abs() < 1e-9,
                "{} 的 tokens: {} vs {}",
                a.0,
                a.2,
                b.2
            );
            assert_eq!(a.3, b.3, "{} 的行数", a.0);
        }

        // 明细仍在（本切片不删），但汇总表**单独**也能答这些 —— 逐条与明细侧的答案对账：
        // ① 分成/收益：按 key_id + type='earn'
        let want: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(pts),0) FROM transactions WHERE key_id = 7 AND type = 'earn'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let got: f64 = conn
            .query_row(
                &format!(
                    "SELECT COALESCE(SUM(pts),0) FROM {TABLE} WHERE key_id = 7 AND type = 'earn'"
                ),
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!((got - want).abs() < 1e-9, "key 7 的 earn: {got} vs {want}");
        // ② 月流水（此夹具全在 10 月，按 bucket 前缀取月）
        let want: f64 = conn
            .query_row("SELECT COALESCE(SUM(pts),0) FROM transactions", [], |r| {
                r.get(0)
            })
            .unwrap();
        let got: f64 = conn
            .query_row(
                &format!(
                    "SELECT COALESCE(SUM(pts),0) FROM {TABLE} WHERE substr(bucket,1,7) = '2026-10'"
                ),
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!((got - want).abs() < 1e-9, "月流水: {got} vs {want}");
        // ③ 类型构成
        let want: Vec<(String, f64)> = {
            let mut s = conn
                .prepare("SELECT type, COALESCE(SUM(pts),0) FROM transactions GROUP BY type ORDER BY type")
                .unwrap();
            let v: Vec<(String, f64)> = s
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            v
        };
        let got: Vec<(String, f64)> = {
            let mut s = conn
                .prepare(&format!(
                    "SELECT type, COALESCE(SUM(pts),0) FROM {TABLE} GROUP BY type ORDER BY type"
                ))
                .unwrap();
            let v: Vec<(String, f64)> = s
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            v
        };
        assert_eq!(got, want, "类型构成应逐条相同");
    }

    #[test]
    fn a_bucket_row_is_updated_in_place_when_a_later_batch_lands_in_the_same_minute() {
        let conn = migrated_db();
        // 同一桶先折一批、再加一批、再折 ⇒ 仍是一行（唯一索引 + ON CONFLICT 原地累加）
        insert_tx(
            &conn,
            1,
            None,
            None,
            "m",
            "consume",
            "成功",
            "2026-10-09 12:34:01",
            1.0,
            10.0,
        );
        assert_eq!(fold_pending(&conn, 1_000, 1).unwrap(), 1);
        insert_tx(
            &conn,
            1,
            None,
            None,
            "m",
            "consume",
            "成功",
            "2026-10-09 12:34:59",
            2.0,
            20.0,
        );
        assert_eq!(fold_pending(&conn, 1_000, 1).unwrap(), 1);

        let rows = rollup_rows(&conn);
        assert_eq!(
            rows.len(),
            1,
            "同桶两批必须原地累加、不得再插一行: {rows:?}"
        );
        assert_eq!(rows[0].3, 2, "行数应累加");
        assert!((rows[0].1 - 3.0).abs() < 1e-9, "pts 应累加: {}", rows[0].1);
        // 这条同时钉住 IFNULL 归一：两个 NULL key 若以 NULL 进唯一索引，上面会变成两行。
    }

    #[test]
    fn folding_is_bounded_by_its_batch_and_idempotent() {
        let conn = migrated_db();
        for i in 0..5 {
            insert_tx(
                &conn,
                1,
                Some(9),
                Some(4),
                "m",
                "consume",
                "成功",
                &format!("2026-10-09 12:3{i}:00"),
                1.0,
                10.0,
            );
        }
        // 每批至多 2 条**明细**（不是至多 2 个桶）
        assert_eq!(fold_pending(&conn, 1_000, 2).unwrap(), 2);
        assert_eq!(watermark(&conn).unwrap(), 2);
        assert_eq!(fold_pending(&conn, 1_000, 2).unwrap(), 2);
        assert_eq!(watermark(&conn).unwrap(), 4);
        assert_eq!(fold_pending(&conn, 1_000, 2).unwrap(), 1);
        assert_eq!(watermark(&conn).unwrap(), 5);
        // 追平 ⇒ 0，且水位不动（不重复折叠）
        assert_eq!(fold_pending(&conn, 1_000, 2).unwrap(), 0);
        assert_eq!(watermark(&conn).unwrap(), 5);
        // 桶数 = 5（每分钟一个），行数总和 = 5
        let rows = rollup_rows(&conn);
        assert_eq!(rows.len(), 5);
        assert_eq!(rows.iter().map(|r| r.3).sum::<i64>(), 5);
    }

    #[test]
    fn the_fold_never_gets_ahead_of_the_archive() {
        let conn = migrated_db();
        for i in 0..4 {
            insert_tx(
                &conn,
                1,
                Some(9),
                Some(4),
                "m",
                "consume",
                "成功",
                &format!("2026-10-09 12:3{i}:00"),
                1.0,
                10.0,
            );
        }
        // 归档水位为 0（如归档未启用）⇒ 一行都不折：折叠过的明细将来要删，未归档的删了就没了
        assert_eq!(fold_pending(&conn, 0, 100).unwrap(), 0);
        assert_eq!(watermark(&conn).unwrap(), 0);
        assert_eq!(rollup_rows(&conn).len(), 0);
        // 归档只跟到第 2 条 ⇒ 只折前 2 条
        assert_eq!(fold_pending(&conn, 2, 100).unwrap(), 2);
        assert_eq!(watermark(&conn).unwrap(), 2);
        // 归档追平 ⇒ 剩下的也能折
        assert_eq!(fold_pending(&conn, 4, 100).unwrap(), 2);
        assert_eq!(watermark(&conn).unwrap(), 4);
    }

    #[test]
    fn the_fold_writes_exactly_the_rollup_columns() {
        let conn = migrated_db();
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({TABLE})"))
            .unwrap();
        let mut table: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        table.sort();
        assert!(
            table.contains(&"id".to_string()),
            "汇总表应有自增主键: {table:?}"
        );
        // 表列（除自增主键）必须**恰好**等于折叠写入的列 —— 新增列只改一处即红
        let mut written = written_columns();
        written.sort();
        let expected: Vec<String> = table.iter().filter(|c| *c != "id").cloned().collect();
        assert_eq!(
            written, expected,
            "折叠写入的列必须与 transactions_rollup 的列集合一致"
        );
        // 阳性对照：扫描器确实读到了这张表的列（不是空集）
        assert!(table.len() >= 14, "汇总表列数: {table:?}");
    }

    /// 唯一定位一个桶的那枚索引必须**恰好**是 `DIMENSION` 的列集合 —— `ON CONFLICT` 的目标就是它。
    ///
    /// 漂移的形状值得单独兜着：给 `DIMENSION` 加一列却忘了给索引加 ⇒ `ON CONFLICT` 匹配不上
    /// 任何唯一约束，而 SQLite **在执行时**才报错 —— 第一批明细照常成功，只有「同一桶的第二批」
    /// 才会现形。所以这条判据不能靠「折一次看看」来兜（那样两棵树都绿），直接拿真 `PRAGMA` 对账。
    /// 比较用**集合语义**：`ON CONFLICT` 认的是列的组合，索引里列的书写次序不是承重项。
    #[test]
    fn the_unique_index_is_exactly_the_dimension() {
        let conn = migrated_db();
        let unique: Vec<String> = {
            let mut s = conn
                .prepare(&format!(
                    "SELECT name FROM pragma_index_list('{TABLE}') WHERE \"unique\" = 1 AND origin = 'c'"
                ))
                .unwrap();
            s.query_map([], |r| r.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(unique.len(), 1, "汇总表应恰有一枚自建唯一索引: {unique:?}");
        let mut cols: Vec<String> = {
            let mut s = conn
                .prepare(&format!(
                    "SELECT name FROM pragma_index_info('{}')",
                    unique[0]
                ))
                .unwrap();
            s.query_map([], |r| r.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        cols.sort();
        let mut dim: Vec<String> = DIMENSION.iter().map(|(c, _)| c.to_string()).collect();
        dim.sort();
        assert_eq!(
            cols, dim,
            "唯一索引的列集合必须恰好是 DIMENSION（ON CONFLICT 的目标），否则只有同桶第二批才会报错"
        );
        // 阳性对照：两侧都非空
        assert!(cols.len() >= 7, "维度列数: {cols:?}");
    }

    /// 明细行数（删除测试的读数）。
    fn tx_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap()
    }

    /// 种一批明细：`ages` 里每个元素是「多少天前」，用它算出 `time`。
    /// 窗口判据是 `time < datetime('now', '-N days')`，所以夹具必须相对 `now` 取，
    /// 写死字面量会把「跑测试时已经过了那个点没有」变成隐藏前提。
    fn seed_aged(conn: &Connection, ages: &[i64]) {
        for age in ages {
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status, time) \
                 VALUES (1, '', NULL, 'm', 1, 1, 'consume', '成功', datetime('now', ?1))",
                params![format!("-{age} days")],
            )
            .unwrap();
        }
    }

    #[test]
    fn delete_folded_only_releases_rows_older_than_the_retention_window() {
        let conn = migrated_db();
        // 40/20/1 天前各一条 —— 窗口 30 天 ⇒ 只应放出 40 天前那条。
        seed_aged(&conn, &[40, 20, 1]);
        assert_eq!(fold_pending(&conn, 1_000, 100).unwrap(), 3, "先全折");
        assert_eq!(delete_folded(&conn, 1_000, 30, 100).unwrap(), 1);
        assert_eq!(tx_count(&conn), 2, "窗口内的明细必须留下");
        // 已经删过 ⇒ 幂等（同窗口再跑不删任何行）
        assert_eq!(delete_folded(&conn, 1_000, 30, 100).unwrap(), 0);
        // 窗口收到 0 天 ⇒ 剩下的（20/1 天前）也都过了窗口
        assert_eq!(delete_folded(&conn, 1_000, 0, 100).unwrap(), 2);
        assert_eq!(tx_count(&conn), 0);
        // 汇总行一个不少：明细删光之后聚合仍然答得出（这是「明细只留汇总」的全部意义）
        let rows = rollup_rows(&conn);
        assert_eq!(rows.len(), 3, "三个不同的分钟桶应各一行: {rows:?}");
        assert_eq!(rows.iter().map(|r| r.3).sum::<i64>(), 3, "行数守恒");
    }

    #[test]
    fn delete_folded_never_gets_ahead_of_the_fold_or_the_archive() {
        let conn = migrated_db();
        seed_aged(&conn, &[60, 50]);
        // 一条都没折过 ⇒ 水位 0 ⇒ 一行都不删（即使它们都老得过了窗口）
        assert_eq!(watermark(&conn).unwrap(), 0);
        assert_eq!(delete_folded(&conn, 1_000, 30, 100).unwrap(), 0);
        assert_eq!(tx_count(&conn), 2, "没折过不许删");

        // 折了，但归档水位只到第 1 行 ⇒ 最多删到 id 1（归档没跟上的那半不许动）
        assert_eq!(fold_pending(&conn, 1_000, 100).unwrap(), 2);
        assert_eq!(delete_folded(&conn, 1, 30, 100).unwrap(), 1);
        assert_eq!(tx_count(&conn), 1, "归档没覆盖到的行不许删");
        // 归档追平 ⇒ 剩下的那条才可删
        assert_eq!(delete_folded(&conn, 1_000, 30, 100).unwrap(), 1);
        assert_eq!(tx_count(&conn), 0);
    }

    #[test]
    fn delete_folded_is_bounded_by_its_batch() {
        let conn = migrated_db();
        seed_aged(&conn, &[90, 80, 70]);
        assert_eq!(fold_pending(&conn, 1_000, 100).unwrap(), 3);
        // 批量 2 ⇒ 每轮最多删 2 条，剩下的下一轮再删（后台维护不长时间持库）
        assert_eq!(delete_folded(&conn, 1_000, 30, 2).unwrap(), 2);
        assert_eq!(tx_count(&conn), 1);
        assert_eq!(delete_folded(&conn, 1_000, 30, 2).unwrap(), 1);
        assert_eq!(tx_count(&conn), 0);
        // 边界：批次 <= 0 或水位 <= 0 ⇒ 一行都不删（不无限删）
        seed_aged(&conn, &[90]);
        assert_eq!(fold_pending(&conn, 1_000, 100).unwrap(), 1);
        assert_eq!(delete_folded(&conn, 1_000, 30, 0).unwrap(), 0);
        assert_eq!(delete_folded(&conn, 0, 30, 100).unwrap(), 0);
        assert_eq!(tx_count(&conn), 1, "非法参数下明细必须还在");
    }

    #[test]
    fn delete_folded_leaves_the_read_model_answering_the_same_numbers() {
        // 「删明细」之所以安全，全靠读模型 `tx_facts` 同时覆盖汇总与**未折叠**明细。
        // 这条把「删前 / 删后」真的对比一次：同一批数据，删之前用视图读、删之后再用视图读。
        let conn = migrated_db();
        for (sec, pts, tokens) in [(5, 2.0, 20.0), (17, 3.0, 30.0), (59, 4.0, 40.0)] {
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status, time) \
                 VALUES (1, '', NULL, 'm', ?1, ?2, 'consume', '成功', \
                         datetime('now', '-40 days', 'start of day', '+12 hours', ?3))",
                params![tokens, pts, format!("+{sec} seconds")],
            )
            .unwrap();
        }
        crate::tx_facts::ensure_view(&conn).unwrap();
        // 名字经 `tx_facts::VIEW` 取：`tx_facts_gate` R1 只允许视图名在 `tx_facts.rs` 里成字面量。
        let read_sql = format!(
            "SELECT COALESCE(SUM(pts), 0), COALESCE(SUM(tokens), 0), COALESCE(SUM(row_count), 0) \
             FROM {} WHERE user_id = 1",
            crate::tx_facts::VIEW
        );
        let read = |c: &Connection| -> (f64, f64, i64) {
            c.query_row(&read_sql, [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .unwrap()
        };
        let before = read(&conn);
        assert_eq!(fold_pending(&conn, 1_000, 100).unwrap(), 3);
        assert_eq!(delete_folded(&conn, 1_000, 30, 100).unwrap(), 3);
        assert_eq!(tx_count(&conn), 0, "明细确实清空了");
        let after = read(&conn);
        assert_eq!(
            before, after,
            "读模型在删明细前后必须逐字相同（否则「只留汇总」就是在丢账）"
        );
        // 阳性对照：数确实非零，否则上面的恒等是「0 == 0」的假等式
        assert!(before.0 > 0.0 && before.2 == 3, "对照: {before:?}");
    }
}
