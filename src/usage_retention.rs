//! `usage_records` 的保留窗口（rant 2026-10-09T12:28:58 的独立余项：第二张只增不减的明细表）。
//!
//! 背景见 [`crate::archive`]：仓库里有**两张**只增不减的明细表。`transactions` 已由
//! 「归档 → 折叠 → 读模型 → 窗口删除」（#350–#353）收口；`usage_records` 是同一笔 `settle`
//! 写的**第二张**（`billing.rs`），此前**没有任何保留策略**（dev 实测 48.6 万行 ≈ 库内明细的一半）。
//! 归档那半与 `transactions` 共用一套机件（[`crate::archive::USAGE_RECORDS`] 规格 +
//! [`crate::archive::USAGE_SUBDIR`] 子目录），本模块是它的**删除**那半。
//!
//! ## 为什么不需要折叠 / 读模型
//!
//! `transactions` 要读模型，是因为用户能在交易页上要**任意历史窗口**（自定义时间段 / 全部时间），
//! 明细删了聚合还得答得出。`usage_records` 不是这样：它的读者**全部**是写死的**月/日**窗口
//! （`routes/ops.rs` 的本月与当天、`routes/admin.rs` 的三张用量报表、`routes/org.rs` 的部门本月
//! 已用）—— 没有任何读者回看超过「本月起点」（名册由 [`crate::usage_retention_gate`] 从
//! `src/routes/*.rs` 现读并守着）。于是「归档 → 按日历窗口删」就够。
//!
//! ## 门槛为什么是**日历谓词**而不是天数
//!
//! 门槛取 [`KEEP_SINCE`]＝读者的**同一套日历谓词**再往前退一个月（保留本月 ＋ 上月）。
//! 它与读者的下界**同族**、且严格早于它，于是「门槛 ≤ 每个读者的下界」是**结构性**的，
//! 门禁只需比较日历修饰符，不必论证「30 天够不够 31 天」。
//!
//! ## 两条纪律（与 [`crate::tx_rollup::delete_folded`] 同款）
//!
//! * **先归档、后删**：删除判据含 `id ≤ 归档水位`。归档没跑起来（水位 0）⇒ 一行不删
//!   （fail-closed）。这是必须的 —— `usage_records.cost`（**锚定货币金额**，`billing.rs`）
//!   是**只有这张表**带的列，删掉就没有第二份。
//! * **有界批次**：每轮至多 `batch` 行 ⇒ 单次持库时间有界（库在 NAS 上，见 rant 背景）。
//!
//! 射程：本模块**只删**，不碰任何读路径、不改账本、不进 `settle`。调度在 `main.rs`
//! （`try_lock` 拿库，库正忙即跳过；同步 DB I/O 走 `spawn_blocking`）。

use anyhow::{Context, Result};
use rusqlite::Connection;

/// 明细保留门槛（**唯一拼写点**）：保留当前月与上一月，更早的行可删。
///
/// 写成日历谓词而非天数，是为了与读者的下界（`date('now', 'start of month')`）**同族** ——
/// 见模块头。改这里就是改保留期，门禁 [`crate::usage_retention_gate`] 会重新核对它与每个读者。
pub const KEEP_SINCE: &str = "date('now','start of month','-1 month')";

/// 删除**已归档**且已早于 [`KEEP_SINCE`] 的用量明细，返回本轮删掉的行数。
///
/// 判据 = `time < 门槛` **且** `id ≤ archive_watermark`（该行已进 JSONL）。
/// `batch <= 0` 或 `archive_watermark <= 0` ⇒ 一行不删（fail-closed：归档没跑起来就绝不能删，
/// `cost` 只有这张表有 —— 同 [`crate::tx_rollup::delete_folded`] 的形状）。
pub fn delete_archived(conn: &Connection, archive_watermark: i64, batch: i64) -> Result<usize> {
    if batch <= 0 || archive_watermark <= 0 {
        return Ok(0);
    }
    let sql = format!(
        "DELETE FROM usage_records WHERE id IN ( \
           SELECT id FROM usage_records \
            WHERE time < {KEEP_SINCE} AND id <= ?1 \
            ORDER BY id LIMIT ?2)"
    );
    conn.execute(&sql, rusqlite::params![archive_watermark, batch])
        .context("删除已归档的用量明细失败")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 迁移过的内存库 + 一个用户（`usage_records.user_id` 有外键）。
    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO users (id, email, password_hash) VALUES (1, 'u1@test', 'h')",
            [],
        )
        .unwrap();
        conn
    }

    /// 插一行用量记录，`time` 用给定的 SQLite 表达式算（相对 `now`）。
    fn insert_at(conn: &Connection, time_expr: &str) -> i64 {
        conn.execute(
            &format!(
                "INSERT INTO usage_records (user_id, model, tokens, cost, time) \
                 VALUES (1, 'm', 1, 2.5, {time_expr})"
            ),
            [],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM usage_records", [], |r| r.get(0))
            .unwrap()
    }

    /// 门槛**逐字**是「上一月起点」：比当月起点早、比两月前晚。
    #[test]
    fn the_threshold_is_the_start_of_the_previous_month() {
        let conn = db();
        let (le, prev, two) = conn
            .query_row(
                &format!(
                    "SELECT \
                       ({KEEP_SINCE} <= date('now','start of month')), \
                       ({KEEP_SINCE} = date('now','start of month','-1 month')), \
                       ({KEEP_SINCE} > date('now','start of month','-2 month'))"
                ),
                [],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(le, 1, "门槛必须早于（或等于）当月起点 —— 读者的下界");
        assert_eq!(prev, 1, "门槛就是上一月起点（只留本月 + 上月）");
        assert_eq!(two, 1, "门槛应晚于两月前（不至于把上月也删掉）");
    }

    /// 删除只碰「已过门槛 **且** 已归档」的行；门槛当天与窗口内的行都留着。
    #[test]
    fn delete_removes_only_archived_rows_past_the_threshold() {
        let conn = db();
        insert_at(&conn, "date('now','start of month','-2 month')"); // 1：过门槛
        insert_at(&conn, "date('now','start of month','-1 month')"); // 2：== 门槛（不留删）
        insert_at(&conn, "datetime('now')"); // 3：窗口内
        assert_eq!(count(&conn), 3);

        // 水位 3 ⇒ 三行都已归档，可删的只有 1 号
        assert_eq!(delete_archived(&conn, 3, 100).unwrap(), 1);
        assert_eq!(count(&conn), 2, "只有过门槛的那一行应被删掉");
        let survivors: Vec<i64> = conn
            .prepare("SELECT id FROM usage_records ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(survivors, vec![2, 3], "门槛当天与窗口内的行必须留着");
    }

    /// 未归档的行**不删**（水位就是「可以安全删除到哪里」的分界线）。
    #[test]
    fn delete_never_touches_rows_past_the_archive_watermark() {
        let conn = db();
        insert_at(&conn, "date('now','start of month','-2 month')"); // 1
        insert_at(&conn, "date('now','start of month','-2 month')"); // 2
                                                                     // 水位 1 ⇒ 只有 1 号已归档；2 号虽过门槛但原件还没落盘 ⇒ 不许删
        assert_eq!(delete_archived(&conn, 1, 100).unwrap(), 1);
        assert_eq!(count(&conn), 1);
        assert_eq!(
            conn.query_row("SELECT id FROM usage_records", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2,
            "留下的必须是未归档的那行"
        );
    }

    /// fail-closed：没有归档水位（0）或没有批次 ⇒ 一行不删。
    #[test]
    fn delete_is_fail_closed_without_an_archive_watermark_or_a_batch() {
        let conn = db();
        insert_at(&conn, "date('now','start of month','-5 month')");
        assert_eq!(delete_archived(&conn, 0, 100).unwrap(), 0, "水位 0 ⇒ 不删");
        assert_eq!(delete_archived(&conn, 1, 0).unwrap(), 0, "batch 0 ⇒ 不删");
        assert_eq!(
            delete_archived(&conn, 1, -1).unwrap(),
            0,
            "batch < 0 ⇒ 不删"
        );
        assert_eq!(count(&conn), 1, "fail-closed 时一行都不能少");
    }

    /// 每轮至多 `batch` 行（长库不会被一次删光、持库时间有界）。
    #[test]
    fn delete_is_bounded_by_its_batch() {
        let conn = db();
        for _ in 0..5 {
            insert_at(&conn, "date('now','start of month','-3 month')");
        }
        assert_eq!(delete_archived(&conn, 5, 2).unwrap(), 2);
        assert_eq!(delete_archived(&conn, 5, 2).unwrap(), 2);
        assert_eq!(delete_archived(&conn, 5, 2).unwrap(), 1);
        assert_eq!(delete_archived(&conn, 5, 2).unwrap(), 0);
        assert_eq!(count(&conn), 0);
    }
}
