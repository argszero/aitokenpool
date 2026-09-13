//! 点数赠送规则（user-stories v1.7 → P1 落地）
//!
//! P1（rant 2026-08-18T11:03:02）：
//! - 新人赠送：注册（users.created_at）起连续 10 天内每天 1 点，当日有效
//!   （expires_at = 当天 23:59:59）；第 11 天起不再赠送
//! - 懒加载触发：凡是需要「可用余额含当日赠送」的读取点，各自在读取前调用 `ensure_daily_gift`
//!   （触发点即本函数的调用点，不在此列举）；只读报表（如 `/api/dashboard`）只反映已入账的赠送，不触发
//! - 防薅羊毛：每人每日仅一笔；过期未用自动失效（查询时惰性清理）
//! - 消费扣减顺序：先扣最早到期的赠送点数，不足再扣永久 balance

use anyhow::Result;
use rusqlite::Connection;

/// 每日赠送点数
pub const GIFT_DAILY_AMOUNT: f64 = 1.0;
/// 赠送窗口（注册起连续天数）
pub const GIFT_DAYS: i64 = 10;

/// 惰性清理：把已过期的 active 赠送标记 expired，并重算 gift_balance
/// （重算 = 自愈：任何路径下 gift_balance 都能与 gift_grants 对齐）
///
/// 过期点数会**真的离开账户**（`balance + gift_balance` 变小），所以与赠送（`type='gift'`）、
/// 消费（`type='consume'`）、充值（`type='topup'`）一样必须留下账本行：写一笔 `type='expire'`
/// 的支出（C2050：此前只有这一步不记账 ⇒ `SUM(transactions)` 与 `balance + gift_balance` 不可对账）。
/// 一次清扫只写一行、且只记**真正过期的那部分**（赠 1.0 先消费 0.4 ⇒ 记 0.6）。
pub fn expire_past_gifts(conn: &Connection, user_id: i64) -> Result<()> {
    // 先算「即将过期的 active 赠送总额」（改状态前算，改完就查不到了）
    let expiring: f64 = conn.query_row(
        "SELECT COALESCE(SUM(amount), 0) FROM gift_grants \
         WHERE user_id = ?1 AND status = 'active' AND expires_at < datetime('now')",
        [user_id],
        |r| r.get(0),
    )?;
    conn.execute(
        "UPDATE gift_grants SET status = 'expired' \
         WHERE user_id = ?1 AND status = 'active' AND expires_at < datetime('now')",
        [user_id],
    )?;
    conn.execute(
        "UPDATE quotas SET gift_balance = COALESCE((
            SELECT SUM(amount) FROM gift_grants WHERE user_id = ?1 AND status = 'active'
         ), 0) WHERE user_id = ?1",
        [user_id],
    )?;
    // 账本行：仅当真的有过期点数才写（幂等 —— 第二次清扫看不到 active 的过期行，故不会再写）
    if expiring > 0.0 {
        conn.execute(
            "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status) \
             VALUES (?1, '', NULL, '', 0, ?2, 'expire', '成功')",
            rusqlite::params![user_id, expiring],
        )?;
    }
    Ok(())
}

/// 新人每日赠送（懒加载）：在 10 天窗口内且今天未赠 → 补 1 点（当天 23:59:59 过期）
pub fn ensure_daily_gift(conn: &Connection, user_id: i64) -> Result<bool> {
    // 用户必须存在（防薅：只绑定注册用户）
    let created_at: Option<String> = conn
        .query_row(
            "SELECT created_at FROM users WHERE id = ?1",
            [user_id],
            |r| r.get(0),
        )
        .ok();
    let Some(created_at) = created_at else {
        return Ok(false);
    };

    // 惰性清理过期赠送
    expire_past_gifts(conn, user_id)?;

    // 10 天窗口判定（注册日 = 第 1 天；julianday 差值 < 10）
    let in_window: bool = conn
        .query_row(
            "SELECT julianday(date('now')) - julianday(date(?1)) < ?2",
            rusqlite::params![created_at, GIFT_DAYS as f64],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if !in_window {
        return Ok(false);
    }

    // 今天已赠 → 跳过
    let today_granted: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM gift_grants WHERE user_id = ?1 AND date(granted_at) = date('now'))",
            [user_id],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if today_granted {
        return Ok(false);
    }

    // 补发：expires_at = 当天 23:59:59
    conn.execute(
        "INSERT INTO gift_grants (user_id, amount, granted_at, expires_at, status) \
         VALUES (?1, ?2, datetime('now'), strftime('%Y-%m-%d 23:59:59', 'now'), 'active')",
        rusqlite::params![user_id, GIFT_DAILY_AMOUNT],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO quotas (user_id, balance, gift_balance) VALUES (?1, 0, 0)",
        [user_id],
    )?;
    conn.execute(
        "UPDATE quotas SET gift_balance = gift_balance + ?1 WHERE user_id = ?2",
        rusqlite::params![GIFT_DAILY_AMOUNT, user_id],
    )?;
    // 赠送写 transactions（rant 2026-08-22T00:04:21：每日赠送不入账 → 交易记录/汇总永远对不上）
    conn.execute(
        "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status) \
         VALUES (?1, '', NULL, '', 0, ?2, 'gift', '成功')",
        rusqlite::params![user_id, GIFT_DAILY_AMOUNT],
    )?;
    Ok(true)
}

/// 消费扣减：先扣最早到期的赠送点数（expires_at ASC），返回仍需从永久扣的剩余点数
/// 在调用方事务内执行（tx：&Connection 兼容 rusqlite::Transaction）
pub fn deduct_gift_first(conn: &Connection, user_id: i64, mut pts: f64) -> Result<f64> {
    // 惰性清理（确保不扣已过期）
    expire_past_gifts(conn, user_id)?;

    let mut stmt = conn.prepare(
        "SELECT id, amount FROM gift_grants \
         WHERE user_id = ?1 AND status = 'active' \
         ORDER BY expires_at ASC, id ASC",
    )?;
    let grants: Vec<(i64, f64)> = stmt
        .query_map([user_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    for (id, amount) in grants {
        if pts <= 0.0 {
            break;
        }
        let take = amount.min(pts);
        let new_amount = amount - take;
        conn.execute(
            "UPDATE gift_grants SET amount = ?1, status = CASE WHEN ?1 <= 0 THEN 'used' ELSE 'active' END WHERE id = ?2",
            rusqlite::params![new_amount, id],
        )?;
        pts -= take;
    }

    // 重算 gift_balance（与剩余 active 对齐）
    conn.execute(
        "UPDATE quotas SET gift_balance = COALESCE((
            SELECT SUM(amount) FROM gift_grants WHERE user_id = ?1 AND status = 'active'
         ), 0) WHERE user_id = ?1",
        [user_id],
    )?;
    Ok(pts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn tmp_db(tag: &str) -> (Connection, std::path::PathBuf) {
        let p = std::env::temp_dir().join(format!("atp_gift_{}_{}.db", std::process::id(), tag));
        let _ = std::fs::remove_file(&p);
        let conn = db::open(p.to_str().unwrap()).expect("open tmp db");
        (conn, p)
    }

    /// 注册一个新用户（created_at 可控）并初始化配额
    fn register(conn: &Connection, email: &str, created: &str) -> i64 {
        conn.execute(
            "INSERT INTO users (email, password_hash, name, role, created_at) VALUES (?1, 'x', '新用户', 'user', ?2)",
            rusqlite::params![email, created],
        )
        .unwrap();
        let id = conn.last_insert_rowid();
        conn.execute(
            "INSERT OR IGNORE INTO quotas (user_id, balance) VALUES (?1, 0)",
            [id],
        )
        .unwrap();
        id
    }

    /// 相对当前时间的动态时间戳（避免测试硬编码日期跨天后失效）
    fn ts(conn: &Connection, days: i64) -> String {
        conn.query_row(
            "SELECT datetime('now', ?1)",
            [format!("{days} days")],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn first_gift_granted_with_today_expiry() {
        let (conn, p) = tmp_db("g1");
        let uid = register(&conn, "u1@t.local", &ts(&conn, 0)); // 今天注册
        let granted = ensure_daily_gift(&conn, uid).unwrap();
        assert!(granted, "注册当天应补发 1 点");
        let (bal, gift): (f64, f64) = conn
            .query_row(
                "SELECT balance, gift_balance FROM quotas WHERE user_id = ?1",
                [uid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(gift, 1.0);
        assert_eq!(bal, 0.0);
        let (expires, status, amount): (String, String, f64) = conn
            .query_row(
                "SELECT expires_at, status, amount FROM gift_grants WHERE user_id = ?1",
                [uid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(status, "active");
        assert_eq!(amount, 1.0);
        assert!(
            expires.ends_with("23:59:59"),
            "当日有效（当天 23:59:59 过期）: {expires}"
        );
        drop(conn);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn same_day_no_duplicate_gift() {
        let (conn, p) = tmp_db("g2");
        let uid = register(&conn, "u2@t.local", &ts(&conn, 0));
        assert!(ensure_daily_gift(&conn, uid).unwrap());
        assert!(!ensure_daily_gift(&conn, uid).unwrap(), "同天不重复赠送");
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM gift_grants WHERE user_id = ?1",
                [uid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
        drop(conn);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn outside_window_no_gift() {
        let (conn, p) = tmp_db("g3");
        // 11 天前注册 → 超出 10 天窗口
        let uid = register(&conn, "u3@t.local", &ts(&conn, -11));
        let granted = ensure_daily_gift(&conn, uid).unwrap();
        assert!(!granted, "第 11 天起不再赠送");
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM gift_grants WHERE user_id = ?1",
                [uid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0);
        drop(conn);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn expired_gift_cleaned_lazily() {
        let (conn, p) = tmp_db("g4");
        let uid = register(&conn, "u4@t.local", &ts(&conn, 0));
        assert!(ensure_daily_gift(&conn, uid).unwrap());
        // 手工把 expires_at 改成过去 → 惰性清理应标记 expired 并扣 gift_balance
        conn.execute(
            "UPDATE gift_grants SET expires_at = datetime('now', '-3 days') WHERE user_id = ?1",
            [uid],
        )
        .unwrap();
        expire_past_gifts(&conn, uid).unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM gift_grants WHERE user_id = ?1",
                [uid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "expired");
        let gift: f64 = conn
            .query_row(
                "SELECT gift_balance FROM quotas WHERE user_id = ?1",
                [uid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(gift, 0.0, "过期赠送从 gift_balance 扣减");
        drop(conn);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn gift_writes_transaction_record() {
        // rant 2026-08-22T00:04:21：每日赠送必须写 transactions（否则交易记录/汇总对不上余额）
        let (conn, p) = tmp_db("g7");
        let uid = register(&conn, "u7@t.local", &ts(&conn, 0));
        assert!(ensure_daily_gift(&conn, uid).unwrap());
        let (typ, pts, tokens): (String, f64, f64) = conn
            .query_row(
                "SELECT type, pts, tokens FROM transactions WHERE user_id = ?1",
                [uid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(typ, "gift", "赠送应写 type='gift' 的交易记录");
        assert_eq!(pts, GIFT_DAILY_AMOUNT, "赠送点数入账");
        assert_eq!(tokens, 0.0);
        drop(conn);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn deduct_gift_first_oldest_expiry() {
        let (conn, p) = tmp_db("g5");
        let uid = register(&conn, "u5@t.local", &ts(&conn, 0));
        // 两笔 active 赠送：今天到期 1 点、明天到期 1 点（最早到期先扣）
        conn.execute(
            "INSERT INTO gift_grants (user_id, amount, granted_at, expires_at, status) \
             VALUES (?1, 1, datetime('now'), strftime('%Y-%m-%d 23:59:59', 'now'), 'active')",
            [uid],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO gift_grants (user_id, amount, granted_at, expires_at, status) \
             VALUES (?1, 1, datetime('now'), strftime('%Y-%m-%d 23:59:59', 'now', '+1 day'), 'active')",
            [uid],
        )
        .unwrap();
        conn.execute(
            "UPDATE quotas SET gift_balance = 2 WHERE user_id = ?1",
            [uid],
        )
        .unwrap();
        // 扣 1.5 点：先花今天到期的 1 点（used）再花明天到期的 0.5
        let remaining = deduct_gift_first(&conn, uid, 1.5).unwrap();
        assert!(remaining.abs() < 1e-9, "赠送覆盖 1.5 点，剩余永久应为 0");
        let (st1, amt2): (String, f64) = conn
            .query_row(
                "SELECT (SELECT status FROM gift_grants WHERE user_id = ?1 ORDER BY expires_at ASC LIMIT 1), \
                        (SELECT amount FROM gift_grants WHERE user_id = ?1 ORDER BY expires_at DESC LIMIT 1)",
                [uid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(st1, "used", "最早到期先扣");
        assert!(
            (amt2 - 0.5).abs() < 1e-9,
            "明天这笔剩 0.5 仍 active: {amt2}"
        );
        let gift: f64 = conn
            .query_row(
                "SELECT gift_balance FROM quotas WHERE user_id = ?1",
                [uid],
                |r| r.get(0),
            )
            .unwrap();
        assert!((gift - 0.5).abs() < 1e-9, "gift_balance 与剩余对齐: {gift}");
        drop(conn);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn deduct_overflow_falls_to_permanent() {
        let (conn, p) = tmp_db("g6");
        let uid = register(&conn, "u6@t.local", &ts(&conn, 0));
        conn.execute(
            "INSERT INTO gift_grants (user_id, amount, granted_at, expires_at, status) \
             VALUES (?1, 1, datetime('now'), strftime('%Y-%m-%d 23:59:59', 'now'), 'active')",
            [uid],
        )
        .unwrap();
        conn.execute(
            "UPDATE quotas SET gift_balance = 1, balance = 10 WHERE user_id = ?1",
            [uid],
        )
        .unwrap();
        // 扣 3 点：gift 1 点 + 永久 2 点
        let remaining = deduct_gift_first(&conn, uid, 3.0).unwrap();
        assert!(
            (remaining - 2.0).abs() < 1e-9,
            "剩余 2 点从永久扣: {remaining}"
        );
        let gift: f64 = conn
            .query_row(
                "SELECT gift_balance FROM quotas WHERE user_id = ?1",
                [uid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(gift, 0.0);
        drop(conn);
        let _ = std::fs::remove_file(p);
    }

    // ---- C2050：赠送过期的账本行 ----
    //
    // 规格（不是实现的复述）：账本是账单 —— 按 `type` 判方向的
    // `SUM(transactions)` 必须恒等于账户里的 `balance + gift_balance`。
    // 三种生命周期（赠送 / 消费 / **过期**）都要成立；过期与消费的唯一区别是「谁把点数拿走了」。

    /// 账本净额（方向由 `type` 决定，与 wallet.rs / ops.rs 同口径 —— 所有 writer 都存正数 pts）
    fn ledger_net(conn: &Connection, uid: i64) -> f64 {
        conn.query_row(
            "SELECT COALESCE(SUM(CASE WHEN type IN ('earn','topup','gift') THEN pts ELSE -pts END), 0) \
             FROM transactions WHERE user_id = ?1",
            [uid],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// 账户总额（赠送余额 + 永久余额）
    fn account_total(conn: &Connection, uid: i64) -> f64 {
        conn.query_row(
            "SELECT COALESCE(balance, 0) + COALESCE(gift_balance, 0) FROM quotas WHERE user_id = ?1",
            [uid],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// 某类型的账本行：点数列表（按 id）
    fn tx_pts(conn: &Connection, uid: i64, typ: &str) -> Vec<f64> {
        let mut stmt = conn
            .prepare("SELECT pts FROM transactions WHERE user_id = ?1 AND type = ?2 ORDER BY id")
            .unwrap();
        let v = stmt
            .query_map(rusqlite::params![uid, typ], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<f64>>>()
            .unwrap();
        v
    }

    #[test]
    fn gift_expiry_is_recorded_in_the_ledger() {
        // C2050：过期的赠送点数会真的离开账户 ⇒ 必须写账本行，否则账单与账户不可对账。
        let (conn, p) = tmp_db("g8");
        let uid = register(&conn, "u8@t.local", &ts(&conn, 0));

        // 腿 1 赠送（阳性对照：不变量本就成立）
        assert!(ensure_daily_gift(&conn, uid).unwrap());
        assert_eq!(ledger_net(&conn, uid), 1.0);
        assert_eq!(account_total(&conn, uid), 1.0);

        // 腿 2 消费 0.4（阳性对照）：扣赠送本身不改账本 —— consume 行由调用方
        // `billing::settle` 写（gift.rs 只管余额），这里照 settle 的写法补上那一行。
        let rest = deduct_gift_first(&conn, uid, 0.4).unwrap();
        assert!(rest.abs() < 1e-9);
        conn.execute(
            "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status) \
             VALUES (?1, '2', 1, 'm', 0, 0.4, 'consume', '成功')",
            [uid],
        )
        .unwrap();
        assert_eq!(account_total(&conn, uid), 0.6);
        assert_eq!(ledger_net(&conn, uid), 0.6);

        // 腿 3 过期 —— 本次缺陷所在：点数离账，账本必须同步
        conn.execute(
            "UPDATE gift_grants SET expires_at = datetime('now', '-1 minute') WHERE user_id = ?1",
            [uid],
        )
        .unwrap();
        expire_past_gifts(&conn, uid).unwrap();

        assert_eq!(account_total(&conn, uid), 0.0, "过期后账户归零");
        assert_eq!(
            tx_pts(&conn, uid, "expire"),
            vec![0.6],
            "过期的账本行只记真正过期的部分（赠 1.0 消费 0.4 ⇒ 0.6）"
        );
        let ln = ledger_net(&conn, uid);
        let at = account_total(&conn, uid);
        assert!(
            (ln - at).abs() < 1e-9,
            "不变量：账本净额 == 账户总额（ledger={ln} account={at}）"
        );
        drop(conn);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn gift_expiry_is_idempotent_and_writes_nothing_when_nothing_expired() {
        let (conn, p) = tmp_db("g9");
        let uid = register(&conn, "u9@t.local", &ts(&conn, 0));
        assert!(ensure_daily_gift(&conn, uid).unwrap());

        // 未过期 ⇒ 不写行（阳性对照：无事件则无账本行）
        expire_past_gifts(&conn, uid).unwrap();
        assert!(
            tx_pts(&conn, uid, "expire").is_empty(),
            "没有点数过期时不应写 expire 行"
        );
        assert_eq!(account_total(&conn, uid), 1.0);
        assert_eq!(ledger_net(&conn, uid), 1.0);

        // 过期后重复清扫 ⇒ 仍然只有一行（第二次看不到 active 的过期行）
        conn.execute(
            "UPDATE gift_grants SET expires_at = datetime('now', '-1 minute') WHERE user_id = ?1",
            [uid],
        )
        .unwrap();
        expire_past_gifts(&conn, uid).unwrap();
        expire_past_gifts(&conn, uid).unwrap();
        expire_past_gifts(&conn, uid).unwrap();
        assert_eq!(tx_pts(&conn, uid, "expire"), vec![1.0], "清扫必须幂等");
        assert_eq!(account_total(&conn, uid), 0.0);
        assert_eq!(ledger_net(&conn, uid), 0.0, "账本与账户同时归零");
        drop(conn);
        let _ = std::fs::remove_file(p);
    }
}
