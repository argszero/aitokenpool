//! 交易事实读模型（rant 2026-10-09T12:28:58 验收项 1 的**读侧**切片）。
//!
//! 明细折叠（`src/tx_rollup.rs`）让「同样的事实」在库里有了**两个载体**：
//!
//! - **已折叠**：`transactions_rollup` 里的分钟级可加汇总行（一行代表 N 条明细）；
//! - **未折叠**：`transactions` 里 `id > 折叠水位` 的原始明细（一行就是一条调用）。
//!
//! 本模块把两者合成**一个读模型** —— 视图 `tx_facts`。它存在的理由只有一个：让
//! 「删掉已折叠的明细」这一步（验收项 1 的后续切片）**对聚合读数不可见**。在此之前，
//! 任何仍然只读 `transactions` 的聚合查询都会在删除后少算一段历史。
//!
//! ## 视图的两条臂
//!
//! ```text
//! tx_facts := transactions_rollup（去归一化后）                       -- 历史
//!           ∪ transactions WHERE id > MAX(up_to_id)                  -- 尚未折叠的尾部
//! ```
//!
//! **列集完全由 `tx_rollup::{DIMENSION, ADDITIVE}` 派生**（本模块不另立一份列名清单）：
//! 维度 7 列 + 可加 5 列 ⇒ 视图 12 列。三处**刻意不相同的映射**，都由下面的常量钉住：
//!
//! | 维度列 | 汇总臂取值 | 明细臂取值 | 为什么 |
//! |---|---|---|---|
//! | `key_id` / `api_key_id` | `NULLIF(<col>, 0)` | `t.<col>`（保留 NULL） | 汇总表为让 NULL 在唯一索引里相撞，用 `0` 当哨兵（`IFNULL(…, 0)`）。若不还原，`GROUP BY key_id` 会把「没有 key」的行归到 `id = 0` 这一组 —— 分组键**变了**，尽管 `SUM` 不变（实测：`(None, 6.0)` 变 `(0, 6.0)`）。 |
//! | `bucket` | `bucket`（16 字符分钟串） | `t.time`（19 字符整秒） | 汇总行**没有比分钟更细的时刻**可用；明细侧保留原值，见下「分钟对齐」。 |
//! | `row_count` | `row_count` | `1` | 明细一行就是一行；汇总行带的是它压了多少行 —— 所以**数行数必须 `SUM(row_count)`，不得 `COUNT(*)`**（后者数的是「视图有几行」，即分钟桶数）。 |
//!
//! ## 分钟对齐（读侧的时间窗契约）
//!
//! 汇总臂的时间列是 **16 字符**（`substr(time, 1, 16)`），明细臂是 **19 字符**
//! （`YYYY-MM-DD HH:MM:SS`）。同一个字符串比较里两者长度不同会**静默错位**：
//! `'2026-10-08 13:56' >= '2026-10-08 13:56:19'` 为**假**（短前缀更小）⇒ 带秒的
//! 左界会把这一分钟的汇总行整分钟丢掉，而明细臂照样收下它 ⇒ 折叠前后不一致。
//!
//! 契约因此是：**时间窗一律分钟对齐**（`minute_aligned()` 是唯一的截断点，`tx_where`
//! 在绑定 `start`/`end` 时调用它）。截到分钟之后两条臂对同一字符串的比较结果一致，
//! 且 `bucket` 上的索引仍然可用（没有在列上套函数）。**这是分钟级汇总的固有代价**，
//! 不是实现取舍：任何带秒的左界都无法用分钟桶精确回答。
//!
//! ## 射程（如实）
//!
//! - 本视图服务的是**聚合读数**：`SUM` / `GROUP BY` / 数行数。它**不是**明细列表 ——
//!   汇总行没有 `id`、没有 `counterpart`，一行代表 N 条调用。分页交易列表因此**仍读
//!   `transactions`**，`transactions_rollup` 的删除切片必须等它的归属定下来再做。
//! - 账本（`SUM(transactions)` 按 `type` 判方向，C2050）**不**走本视图：账单是权威，
//!   它读原始表。
//! - 视图的**定义**是代码（每次开库 `DROP`+`CREATE`）⇒ 不存在「旧库里的定义陈旧」这一态；
//!   代价是每次开库多一条 DDL。
//!
//! ## 还有哪些生产聚合仍在读原始表（本切片**故意**没搬）
//!
//! 删除落地时，这张清单上每一条要么搬进视图、要么被证明「不可能读到被删的那段」：
//!
//! | 站点 | 为什么不搬 |
//! |---|---|
//! | `routes/wallet.rs` 的分页列表与 `total` | 要的是**逐行身份**（`id`/`counterpart`），汇总行没有；`total` 必须与列表同源，否则页码数对不上能翻出来的行数。**删除按保留窗口**放行，窗口必须盖住列表自己提供的每一个预设区间（`tx_retention_gate` 守这条），于是这条永远只是「最近一段」的读者。 |
//! | `db.rs::keys_used_from_ledger` | `v < 13` 的一次性修复步：真源是**账本**，而它只可能在「还没有汇总表」的老库上跑 —— 那时一行明细都没被折叠过，更没被删过。 |
//! | `tx_rollup.rs` / `tx_archive.rs` | 折叠与归档**本来就**读明细 —— 它们是明细的写者。 |
//! | `billing.rs` / `gateway.rs` 的 `COUNT(*)` | 那些在 `#[cfg(test)]` 里（断言 settle 写了行），非生产读数。 |
//!
//! `routes/sharing.rs::row_select()` 的 earn 批量聚合**曾**在这张清单上，已随删除切片搬进视图
//! （常量 `ROW_SELECT` 改成函数 `row_select()`，计划断言一并改写）。

use anyhow::{Context, Result};
use rusqlite::Connection;

/// 读模型视图名。**本常量是它的唯一拼写点** —— 生产 SQL 一律经 [`source`] 插入，
/// 不得在字符串字面量里手写（`tx_facts_gate` 盯着这一条）。
pub const VIEW: &str = "tx_facts";

/// `FROM <视图> <别名>` 片段，供调用方 `format!` 插进自己的查询。
pub fn source(alias: &str) -> String {
    format!("{VIEW} {alias}")
}

/// 明细臂取值：凡 `DIMENSION` 里**表达式不是裸列名**的列，必须在此显式给出明细侧的取值。
///
/// 明细侧一律取**原值**（不做汇总表那套 NULL 归一、也不做分钟截断）—— 这一条正是
/// 「折叠不可见」的承重点：汇总臂与明细臂必须在同一列上给出**同样的语义**。
/// `ADDITIVE` 里 `COUNT(*)` 对应的 `row_count` 在明细侧恒为 `1`（一行明细就是一行）。
const DETAIL_VALUE: &[(&str, &str)] = &[
    ("key_id", "t.key_id"),
    ("api_key_id", "t.api_key_id"),
    ("bucket", "t.time"),
    ("row_count", "1"),
];

/// 视图列名：`bucket` 在视图里叫 `time`（读侧与明细同名列比较，`tx_where` 才能逐字复用）。
fn view_column(col: &str) -> &str {
    if col == "bucket" {
        "time"
    } else {
        col
    }
}

/// 汇总表为让 NULL 在唯一索引里相撞而套的填充值 —— 从 `tx_rollup::DIMENSION` 的表达式
/// **现读**（`IFNULL(<列>, <填充值>)` 的第二个实参）。本模块**不另写一个 `0`**：汇总侧一旦
/// 换了填充值，视图侧的 `NULLIF` 跟着换，不会留下两处各自为政的哨兵（本模块的全部列信息
/// 都从 `DIMENSION` / `ADDITIVE` 派生，这是同一件事的第三个面）。
///
/// 填充值之所以不会与真 key 撞车：`keys` / `api_keys` 的 id 是
/// `INTEGER PRIMARY KEY AUTOINCREMENT`，从 1 起 ⇒ 库里没有 `id = 0` 的行。
fn sentinel_of(dim_expr: &str) -> Option<&str> {
    let rest = dim_expr.trim().strip_prefix("IFNULL(")?;
    let (_, fill) = rest.split_once(',')?;
    Some(fill.trim().trim_end_matches(')').trim())
}

/// 汇总臂对某个维度列的取值：DIMENSION 归一化过的列（`IFNULL(…)`）在视图里还原成 NULL。
fn rollup_value(col: &str, dim_expr: &str) -> String {
    match sentinel_of(dim_expr) {
        Some(sentinel) => format!("NULLIF({col}, {sentinel})"),
        None => col.to_string(),
    }
}

/// 明细臂对某个维度/可加列的取值。
fn detail_value(col: &str) -> String {
    DETAIL_VALUE
        .iter()
        .find(|(c, _)| *c == col)
        .map(|(_, v)| (*v).to_string())
        .unwrap_or_else(|| format!("t.{col}"))
}

/// 视图的列名清单（汇总臂与明细臂逐列同名，UNION ALL 的列名取自左臂）。
///
/// 只有测试读它（拿它与真 `PRAGMA table_info` 相比）；发布产物里没有读者，故 `#[cfg(test)]`
/// —— 不加的话 `cargo build` 会报 `dead_code`，而 `clippy --all-targets` 看不见那条警告
/// （它连测试目标一起编，于是这个函数在它眼里「被读过」）。
#[cfg(test)]
pub fn view_columns() -> Vec<String> {
    crate::tx_rollup::DIMENSION
        .iter()
        .map(|(c, _)| view_column(c).to_string())
        .chain(
            crate::tx_rollup::ADDITIVE
                .iter()
                .map(|(c, _)| (*c).to_string()),
        )
        .collect()
}

/// `CREATE VIEW` 语句 —— **从 `tx_rollup` 的声明渲染**，不另立列名清单。
pub fn view_sql() -> String {
    let rollup: Vec<String> = crate::tx_rollup::DIMENSION
        .iter()
        .map(|(c, e)| format!("{} AS {}", rollup_value(c, e), view_column(c)))
        .chain(
            crate::tx_rollup::ADDITIVE
                .iter()
                .map(|(c, _)| (*c).to_string()),
        )
        .collect();
    let detail: Vec<String> = crate::tx_rollup::DIMENSION
        .iter()
        .map(|(c, _)| format!("{} AS {}", detail_value(c), view_column(c)))
        .chain(
            crate::tx_rollup::ADDITIVE
                .iter()
                .map(|(c, _)| format!("{} AS {c}", detail_value(c))),
        )
        .collect();
    format!(
        "CREATE VIEW {VIEW} AS \
         SELECT {} FROM {} \
         UNION ALL \
         SELECT {} FROM transactions t \
         CROSS JOIN (SELECT COALESCE(MAX({}), 0) AS wm FROM {}) w \
         WHERE t.id > w.wm",
        rollup.join(", "),
        crate::tx_rollup::TABLE,
        detail.join(", "),
        crate::tx_rollup::UP_TO,
        crate::tx_rollup::TABLE,
    )
}

/// 建（重建）视图。`migrate()` 每次都调 ⇒ **代码即定义**，不会留下陈旧定义。
pub fn ensure_view(conn: &Connection) -> Result<()> {
    conn.execute_batch(&format!("DROP VIEW IF EXISTS {VIEW}; {}", view_sql()))
        .with_context(|| format!("建立读模型视图 {VIEW} 失败"))
}

/// 把时间窗的界截到**分钟**（`YYYY-MM-DD HH:MM`，16 字符）—— 见模块文档「分钟对齐」。
///
/// 唯一的截断点：`tx_where` 绑定 `start`/`end` 时调用。库内 `time` 是 19 字符
/// （`YYYY-MM-DD HH:MM:SS`），汇总桶是 16 字符；两者在同一比较里必须同长。
pub fn minute_aligned(s: &str) -> String {
    s.trim().chars().take(16).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx_rollup::{fold_pending, watermark};
    use rusqlite::params;

    /// 建一个按真 schema 迁移过的内存库（`migrate` 里已经把视图建好），并种下 users 1..=4
    /// —— `transactions.user_id` 有外键，没用户就插不进去。
    fn migrated_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        for id in 1..=4 {
            conn.execute(
                "INSERT INTO users (id, email, password_hash) VALUES (?1, ?2, 'h')",
                params![id, format!("u{id}@test")],
            )
            .unwrap();
        }
        conn
    }

    /// 一条明细。`time` 显式给（分钟桶与带秒的界都是判据的一部分，不能靠 `datetime('now')`）。
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

    /// 夹具。每一类都是某条判据的分母：
    /// - **同一分钟两行**（同一桶）⇒ 桶数 ≠ 行数（`COUNT(*)` 那颗雷的分母）；
    /// - **没有 key 的行**（两列都是 NULL）⇒ 视图必须还原出 NULL，不能是 `0`；
    /// - **只有一个 key 列有值**的行 ⇒ 两个 key 列各自归一，不能共用一个判据；
    /// - 跨分钟、跨用户、多类型/状态/模型 ⇒ 维度分组有多个桶。
    fn seed(conn: &Connection) {
        insert_tx(
            conn,
            1,
            Some(7),
            Some(11),
            "m-a",
            "consume",
            "成功",
            "2026-10-08 13:56:05",
            1.5,
            100.0,
        );
        insert_tx(
            conn,
            1,
            Some(7),
            Some(11),
            "m-a",
            "consume",
            "成功",
            "2026-10-08 13:56:40",
            2.5,
            200.0,
        );
        insert_tx(
            conn,
            1,
            Some(7),
            Some(11),
            "m-a",
            "consume",
            "成功",
            "2026-10-08 13:57:10",
            0.5,
            50.0,
        );
        insert_tx(
            conn,
            1,
            None,
            None,
            "m-b",
            "earn",
            "入账",
            "2026-10-08 13:56:05",
            4.0,
            400.0,
        );
        insert_tx(
            conn,
            2,
            None,
            None,
            "m-c",
            "topup",
            "成功",
            "2026-10-07 23:59:59",
            10.0,
            0.0,
        );
        insert_tx(
            conn,
            2,
            Some(9),
            Some(12),
            "m-a",
            "expire",
            "成功",
            "2026-10-08 13:56:59",
            3.0,
            0.0,
        );
        insert_tx(
            conn,
            3,
            None,
            Some(13),
            "m-c",
            "gift",
            "成功",
            "2026-10-08 14:00:00",
            2.0,
            0.0,
        );
    }

    /// 折叠到追平 —— 一次调用只折**一批**（坑 #827），必须循环到返回 0。
    fn fold_to_exhaustion(conn: &Connection) {
        while fold_pending(conn, i64::MAX, 4).unwrap() > 0 {}
    }

    /// 明细侧的分组表达式：`DIMENSION` 里为唯一索引套了 `IFNULL(…, 哨兵)` 的列，
    /// 语义上是那个**裸列**（NULL 就是 NULL）；其余列（如 `bucket` 的 `substr(time, 1, 16)`）
    /// 就是它自己的表达式。
    ///
    /// 这条表达式**两侧共用** —— 视图列名与明细列名逐字相同，于是同一段文本既能在
    /// `transactions` 上求值、也能在视图上求值。「同一句查询、两个载体」的分歧（key 的
    /// NULL/`0`、`bucket` 的分钟串）就是被它抓住的。
    fn group_expr(dim_expr: &str) -> String {
        let e = dim_expr.trim();
        match e.strip_prefix("IFNULL(").and_then(|r| r.split_once(',')) {
            Some((inner, _)) => inner.trim().to_string(),
            None => e.to_string(),
        }
    }

    /// `(分组值, SUM(pts))` 的多重集，按分组值升序。
    fn group_rows(conn: &Connection, source: &str, expr: &str) -> Vec<(String, f64)> {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT IFNULL(CAST({expr} AS TEXT), '∅'), COALESCE(SUM(pts), 0) \
                 FROM {source} GROUP BY {expr} ORDER BY 1"
            ))
            .unwrap();
        let rows: Vec<(String, f64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        rows
    }

    /// 视图与明细的**逐项分歧**清单（空 = 视图是明细的可加镜像）。
    ///
    /// 两个方向都取自**声明**：可加量用 `tx_rollup::ADDITIVE` 的表达式，维度用
    /// `tx_rollup::DIMENSION` 的分组表达式 —— 本模块不另立一份列名/口径清单。
    fn mismatches(conn: &Connection) -> Vec<String> {
        let mut bad = Vec::new();
        for (col, expr) in crate::tx_rollup::ADDITIVE {
            let view: f64 = conn
                .query_row(
                    &format!("SELECT COALESCE(SUM({col}), 0) FROM {VIEW}"),
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            // 明细侧：`ADDITIVE` 的表达式**本身就是聚合**（`SUM(pts)` / `COUNT(*)`），
            // 直接当选择项用 —— 再套一层 `SUM(` 会得到 `SUM(SUM(pts))` / `SUM(COUNT(*))`，
            // SQLite 报「误用聚合函数」。这条读法由常量的形状决定，不是本测试的选择。
            let detail: f64 = conn
                .query_row(
                    &format!("SELECT COALESCE({expr}, 0) FROM transactions"),
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            if (view - detail).abs() > 1e-9 {
                bad.push(format!("可加量 {col}：视图 {view} ≠ 明细 {detail}"));
            }
        }
        for (col, expr) in crate::tx_rollup::DIMENSION {
            let e = group_expr(expr);
            let view = group_rows(conn, VIEW, &e);
            let detail = group_rows(conn, "transactions", &e);
            if view != detail {
                bad.push(format!(
                    "维度 {col}（按 {e} 分组）：视图 {view:?} ≠ 明细 {detail:?}"
                ));
            }
        }
        bad
    }

    /// 在某个载体上求和给定窗口的 `pts`。`aligned` 走 [`minute_aligned`]（读侧契约），
    /// 否则用原始带秒的界（牙用）。
    fn window_sum(conn: &Connection, source: &str, start: &str, end: &str, aligned: bool) -> f64 {
        let (s, e) = if aligned {
            (minute_aligned(start), minute_aligned(end))
        } else {
            (start.to_string(), end.to_string())
        };
        conn.query_row(
            &format!("SELECT COALESCE(SUM(pts), 0) FROM {source} WHERE time >= ?1 AND time < ?2"),
            params![s, e],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// **阳性对照**：视图存在且列集与 `tx_rollup` 的声明逐列同名同序 ——
    /// 没有它，「不一致清单为空」与「视图根本不在」在读数上无法区分（坑 #814）。
    #[test]
    fn the_view_exposes_exactly_the_rollup_columns() {
        let conn = migrated_db();
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({VIEW})")).unwrap();
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            cols,
            view_columns(),
            "视图列集必须与 `tx_rollup` 的声明逐列同名同序"
        );
        assert_eq!(
            cols.len(),
            crate::tx_rollup::DIMENSION.len() + crate::tx_rollup::ADDITIVE.len(),
            "维度 + 可加量的条数变了"
        );
        assert!(
            cols.contains(&"time".to_string()) && cols.contains(&"row_count".to_string()),
            "视图里没有 `time` / `row_count` —— 列名映射变了？{cols:?}"
        );
    }

    /// **轴**：视图在**每一个折叠状态**下都是明细的可加镜像（未折叠 / 部分折叠 / 全折叠）。
    ///
    /// 这正是「删掉已折叠的明细对聚合读数不可见」这条承诺的前置：只有视图真的等于明细，
    /// 删除之后读数才等于删除之前。
    #[test]
    fn the_view_is_an_additive_image_of_the_details_in_every_fold_state() {
        let conn = migrated_db();
        seed(&conn);

        // ① 未折叠（汇总表空 ⇒ 视图的两条臂里只有明细那条有货）
        assert!(
            mismatches(&conn).is_empty(),
            "未折叠态就不一致：{:?}",
            mismatches(&conn)
        );

        // ② 部分折叠（两条臂同时有货 —— 水位刚走过去，最容易漏掉一条）
        assert!(
            fold_pending(&conn, i64::MAX, 4).unwrap() > 0,
            "夹具里应当有可折的明细"
        );
        assert!(
            mismatches(&conn).is_empty(),
            "部分折叠态不一致：{:?}",
            mismatches(&conn)
        );

        // ③ 折叠到追平（汇总表独答全部历史）
        fold_to_exhaustion(&conn);
        assert!(
            mismatches(&conn).is_empty(),
            "全折叠态不一致：{:?}",
            mismatches(&conn)
        );
        // 阳性对照：③ 的「一致」必须建立在「真的折进去了」之上
        assert!(
            watermark(&conn).unwrap() > 0,
            "一行都没折 ⇒ 上面那条「一致」是空的"
        );
    }

    /// **事实**：视图的**行数**不是调用数 —— 数调用只能 `SUM(row_count)`。
    ///
    /// `transactions_trend` 的 `count` 因此用 `SUM(t.row_count)` 而不是 `COUNT(*)`
    /// （后者数的是「视图有几行」，即分钟桶数）。这条测试把那个分野钉在夹具里，
    /// 免得将来的读者以为两个写法等价。
    #[test]
    fn counting_view_rows_is_not_counting_calls() {
        let conn = migrated_db();
        seed(&conn);
        let calls_before: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        fold_to_exhaustion(&conn);
        let buckets: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {VIEW}"), [], |r| r.get(0))
            .unwrap();
        let calls: i64 = conn
            .query_row(
                &format!("SELECT COALESCE(SUM(row_count), 0) FROM {VIEW}"),
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            calls, calls_before,
            "调用数必须仍等于明细行数（折叠不可见）"
        );
        assert_ne!(
            buckets, calls_before,
            "夹具必须让「视图行数」与「调用数」不相等 —— 否则这条判据没有分母"
        );
    }

    /// **轴**：带秒的时间窗在折叠前后同答 —— 读侧的分钟对齐（[`minute_aligned`]）是承重点。
    ///
    /// 前端发的是 `new Date(...).toISOString()`，秒几乎从不为 0；而汇总臂的时间列只有
    /// 16 字符 ⇒ 带秒的界会让两条臂对**同一个**分钟桶给出不同答案（短前缀更小）。
    #[test]
    fn a_window_with_seconds_is_answered_the_same_way_before_and_after_folding() {
        let conn = migrated_db();
        seed(&conn);
        let (start, end) = ("2026-10-08 13:56:05", "2026-10-08 13:57:10");

        let before = window_sum(&conn, "transactions", start, end, true);
        fold_to_exhaustion(&conn);
        let after = window_sum(&conn, VIEW, start, end, true);
        assert_eq!(before, after, "对齐后的窗口在折叠前后必须同答");
        // 阳性对照：这个窗口确实有货（两条 0 相等会假绿）
        assert!(before > 0.0, "窗口里没有数据 ⇒ 上面的相等没有分母");

        // **牙**：不对齐，两条臂就分道扬镳（汇总桶被整分钟丢掉）
        let raw_unaligned = window_sum(&conn, "transactions", start, end, false);
        let view_unaligned = window_sum(&conn, VIEW, start, end, false);
        assert_ne!(
            raw_unaligned, view_unaligned,
            "不对齐本该暴露两条臂的长度差 —— 这条不成立说明夹具没踩到边界"
        );
    }

    /// **牙**：把哨兵的还原拿掉（`NULLIF(key_id, 0)` → `key_id`），维度判据必须点名。
    ///
    /// 这条同时证「`mismatches` 有牙」与「还原是承重的」：少了它，没有 key 的行会以
    /// `key_id = 0` 进分组 —— `SUM` 不变、**分组键变了**（`None` 变 `0`）。
    ///
    /// ⚠️ 必须**先折叠**：哨兵的还原只作用于**汇总臂**，而未折叠时那条臂一行都没有
    /// （视图此时逐字等于明细）⇒ 变异无从显形。第一版就是漏了这一步，读数「变异前后一致」
    /// 看着像「门禁没牙」，其实是夹具没把哨兵送进视图（坑：牙必须吃到它要咬的那块肉）。
    #[test]
    fn a_view_that_drops_the_null_key_sentinel_stops_matching_the_details() {
        let conn = migrated_db();
        seed(&conn);
        fold_to_exhaustion(&conn);
        assert!(mismatches(&conn).is_empty(), "对照：原视图本该一致");

        let mutated = view_sql()
            .replace("NULLIF(key_id, 0)", "key_id")
            .replace("NULLIF(api_key_id, 0)", "api_key_id");
        assert_ne!(
            mutated,
            view_sql(),
            "变异没落到视图 SQL 上 —— 夹具跟不上实现了？"
        );
        conn.execute_batch(&format!("DROP VIEW IF EXISTS {VIEW}; {mutated}"))
            .unwrap();
        // 阳性对照：变异后的视图里**确实**坐着一行哨兵 —— 否则「不一致」不可能出现
        let sentinels: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM {VIEW} WHERE key_id = 0 OR api_key_id = 0"),
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            sentinels > 0,
            "折叠后视图里没有哨兵行 ⇒ 夹具没踩到 NULL key"
        );

        let bad = mismatches(&conn);
        assert!(
            bad.iter()
                .any(|m| m.starts_with("维度 key_id") || m.starts_with("维度 api_key_id")),
            "哨兵不还原本该被维度判据点名：{bad:?}"
        );
    }

    /// **轴**：读模型要用的三条索引真的在迁移里建出来了，且水位查询**走**索引。
    ///
    /// 水位 `MAX(up_to_id)` 是**每一条**读视图的查询都要先算的（明细臂以它划界）。没有索引时
    /// 它就是一次全表扫汇总表 —— 20 万桶实测 6.8ms，而两条臂合计只要 0.6ms，头重脚轻。
    /// 所以这条不是「锦上添花」的索引，它是读模型**能不能用**的前提；而它在 `db.rs` 里
    /// 只是一行 `CREATE INDEX IF NOT EXISTS`，没人守就会在下一次重排里静默消失。
    #[test]
    fn the_watermark_lookup_has_the_index_it_needs() {
        let conn = migrated_db();
        let idx: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = ?1")
            .unwrap()
            .query_map([crate::tx_rollup::TABLE], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        for want in [
            "idx_transactions_rollup_up_to",
            "idx_transactions_rollup_user_bucket",
            "idx_transactions_rollup_type_bucket",
        ] {
            assert!(
                idx.iter().any(|n| n == want),
                "读模型的索引 {want} 不在迁移建的集合里：{idx:?}"
            );
        }

        let plan: Vec<String> = conn
            .prepare(&format!(
                "EXPLAIN QUERY PLAN SELECT COALESCE(MAX({}), 0) FROM {}",
                crate::tx_rollup::UP_TO,
                crate::tx_rollup::TABLE
            ))
            .unwrap()
            .query_map([], |r| r.get(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let plan = plan.join(" | ");
        assert!(
            !plan.contains("SCAN"),
            "水位查询仍在扫全表汇总表（索引没被优化器用上）：{plan}"
        );
    }
}
