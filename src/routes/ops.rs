//! 运营者 API（P2-C，rant 2026-08-18T14:03:51）
//!
//! 运营者（role=ops）= 平台运维角色，职责最小化：运行概览 + 给任意用户充值。
//! - GET  /api/ops/runtime：平台运行概览（用户数 / 上架 key 数 / 本月调用 / 本月点数流水，全库聚合）
//!   + rant 2026-09-11T16:23:43（PR6）：今日按小时调用量 today_hours（0-23 全量补零，UTC 小时）
//!     + 上游 key 健康 key_health（按厂商聚合 total/on/off）
//!   + 服务版本 version（= `CARGO_PKG_VERSION`，与 /healthz 同源）与运行时长
//!     uptime_{secs,days,hours,minutes,secs_rest}（AppState.started_at 单调时钟）
//! - POST /api/ops/credits {user_id, amount}：给任意用户充值（写 transactions counterpart='运营者'）
//! - GET  /api/ops/users：全平台用户列表（含余额）
//! - 权限：require_role(&["admin","ops"])——普通 user 403

use axum::extract::State;
use axum::Json;
use rusqlite::params;
use serde::Deserialize;

use crate::routes::{internal, ApiErr, AppState, AuthUser};

/// 权限判定：admin 或 ops 均可访问运营端点
fn require_role(auth: &AuthUser) -> Result<(), ApiErr> {
    if auth.role == "admin" || auth.role == "ops" {
        Ok(())
    } else {
        Err((
            axum::http::StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "需要运营者权限" })),
        ))
    }
}

/// 充值请求
#[derive(Debug, Deserialize)]
pub struct OpsCreditReq {
    pub user_id: i64,
    pub amount: f64,
}

/// 一天的秒数（`uptime_days` / `uptime_hours` 的进位基数）
const DAY: u64 = 24 * 60 * 60;

/// 把运行秒数分解成 `(整日, 整小时, 分钟, 秒)`。
///
/// 放在后端而不是前端：前端两份语言包都要拼「6 天 4 小时」，交给前端就得在
/// JS 里再实现一遍进位（还要两语言各写一套），而这里只有一处。
fn split_uptime(secs: u64) -> (u64, u64, u64, u64) {
    (
        secs / DAY,
        (secs % DAY) / 3600,
        (secs % 3600) / 60,
        secs % 60,
    )
}

/// GET /api/ops/runtime：平台运行概览（全库聚合）
pub async fn runtime(
    State(st): State<AppState>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, ApiErr> {
    require_role(&auth)?;

    // 版本与运行时长：不查库，先算——它们与下面的聚合无关，且 uptime 越早取越准。
    // version 取 `env!("CARGO_PKG_VERSION")`，与 /healthz 同源（绝不在前端硬编码版本号：
    // 原型里那张卡写死的 "v0.7.20" 从写下的一刻就已经过期了）。
    let uptime_secs = st.started_at.elapsed().as_secs();
    let (days, hours, minutes, secs) = split_uptime(uptime_secs);
    let version = env!("CARGO_PKG_VERSION");

    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let users: i64 = conn
        .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))
        .unwrap_or(0);
    let active_keys: i64 = conn
        .query_row("SELECT COUNT(*) FROM keys WHERE status = 'on'", [], |r| {
            r.get(0)
        })
        .unwrap_or(0);
    let month_calls: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM usage_records \
             WHERE time >= date('now', 'start of month') AND time < date('now', 'start of month', '+1 month')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    // 本月点数流水：方向由 `type` 决定，**不是** `pts` 的符号。
    //
    // 账本是 **type 编码** 的：每个 writer 都存正数 `pts`（`consume` 存 `+pts`，不是 `-pts`），
    // 方向只写在 `type` 列里。所以按符号过滤（`pts > 0` / `pts < 0`）会让「流出」**永远是 0**
    // （没有任何 writer 产负数），并把消费也算进「流入」。口径与 wallet.rs 的 7 天净额序列
    // （`type IN ('earn','topup','gift')` 为正、其余取负）一致。
    let month_in: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(pts), 0) FROM transactions \
             WHERE type IN ('earn', 'topup', 'gift') \
               AND time >= date('now', 'start of month') AND time < date('now', 'start of month', '+1 month')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0.0);
    let month_out: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(pts), 0) FROM transactions \
             WHERE type IN ('consume', 'expire', 'withdraw') \
               AND time >= date('now', 'start of month') AND time < date('now', 'start of month', '+1 month')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0.0);
    let total_txs: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap_or(0);

    // 今日按小时调用量（rant 2026-09-11T16:23:43 PR6：原型「今日调用量（按小时）」bar-list）。
    // 0-23 全量补零：GROUP BY 会省略无调用的小时，若不补零前端柱状图会整体左移
    // （与交易页 txTrendDays 同款坑）。小时口径 = UTC（库内 time 为 datetime('now')）。
    let mut today = [0i64; 24];
    {
        let mut stmt = conn
            .prepare(
                "SELECT CAST(strftime('%H', time) AS INTEGER) AS h, COUNT(*) \
                 FROM usage_records \
                 WHERE time >= date('now') AND time < date('now', '+1 day') GROUP BY h",
            )
            .map_err(internal)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))
            .map_err(internal)?;
        for row in rows {
            let (h, n) = row.map_err(internal)?;
            if (0..24).contains(&h) {
                today[h as usize] = n;
            }
        }
    }
    let today_hours: Vec<serde_json::Value> = (0..24)
        .map(|h| serde_json::json!({ "hour": h, "calls": today[h as usize] }))
        .collect();

    // 上游 key 健康（原型「上游 key 健康」mini-list）：按厂商聚合上架 key 的启用 / 停用数。
    // status 只有 'on' 与 sharing.rs PATCH 写入的 'paused' / 'off'（软删）三种（见 db.rs + sharing.rs）
    let mut key_health: Vec<serde_json::Value> = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT provider, COUNT(*), \
                        SUM(CASE WHEN status = 'on' THEN 1 ELSE 0 END) \
                 FROM keys GROUP BY provider ORDER BY COUNT(*) DESC, provider",
            )
            .map_err(internal)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })
            .map_err(internal)?;
        for row in rows {
            let (provider, total, on) = row.map_err(internal)?;
            key_health.push(serde_json::json!({
                "provider": provider,
                "total": total,
                "on": on,
                "off": total - on,
            }));
        }
    }

    Ok(Json(serde_json::json!({
        // 版本与运行时长（原型「服务版本」/「运行时长」两张卡）
        "version": version,
        "uptime_secs": uptime_secs,
        "uptime_days": days,
        "uptime_hours": hours,
        "uptime_minutes": minutes,
        "uptime_secs_rest": secs,
        "users": users,
        "active_keys": active_keys,
        "month_calls": month_calls,
        "month_in": month_in,
        "month_out": month_out,
        "total_txs": total_txs,
        "today_hours": today_hours,
        "key_health": key_health,
    })))
}

/// POST /api/ops/credits：给任意用户充值永久点数（counterpart='运营者'）
pub async fn credits(
    State(st): State<AppState>,
    auth: AuthUser,
    Json(req): Json<OpsCreditReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    require_role(&auth)?;
    if req.amount <= 0.0 {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "amount 必须大于 0" })),
        ));
    }
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM users WHERE id = ?1)",
            [req.user_id],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if !exists {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "用户不存在" })),
        ));
    }
    conn.execute(
        "INSERT OR IGNORE INTO quotas (user_id, balance) VALUES (?1, 0)",
        [req.user_id],
    )
    .map_err(internal)?;
    conn.execute(
        "UPDATE quotas SET balance = balance + ?1, updated_at = datetime('now') WHERE user_id = ?2",
        params![req.amount, req.user_id],
    )
    .map_err(internal)?;
    conn.execute(
        "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status) \
         VALUES (?1, '运营者', NULL, 'ops', 0, ?2, 'topup', '成功')",
        params![req.user_id, req.amount],
    )
    .map_err(internal)?;
    let balance: f64 = conn
        .query_row(
            "SELECT COALESCE(balance, 0) FROM quotas WHERE user_id = ?1",
            [req.user_id],
            |r| r.get(0),
        )
        .unwrap_or(0.0);
    Ok(Json(serde_json::json!({
        "user_id": req.user_id,
        "amount": req.amount,
        "balance": balance,
    })))
}

/// GET /api/ops/users：全平台用户列表（含余额）
pub async fn users(
    State(st): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<serde_json::Value>>, ApiErr> {
    require_role(&auth)?;
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let mut stmt = conn
        .prepare(
            "SELECT u.id, u.email, u.name, u.role, COALESCE(q.balance, 0), COALESCE(q.gift_balance, 0) \
             FROM users u LEFT JOIN quotas q ON q.user_id = u.id ORDER BY u.id",
        )
        .map_err(internal)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?,
                "email": r.get::<_, String>(1)?,
                "name": r.get::<_, String>(2)?,
                "role": r.get::<_, String>(3)?,
                "balance": r.get::<_, f64>(4)?,
                "gift_balance": r.get::<_, f64>(5)?,
            }))
        })
        .map_err(internal)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(internal)?);
    }
    Ok(Json(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `split_uptime` 的进位：**已知真值**断言，不是自证。
    ///
    /// 每例都写明 `天:时:分:秒` 的期望值 —— 若把 `/` 与 `%` 写反、或漏掉某一位的归一，
    /// 会在这里被点名，而不是在只断言「能拼回去」的地方悄悄溜过
    /// （拼回式断言对「天=总秒数、其余全 0」这种错法同样成立）。
    #[test]
    fn split_uptime_carries_correctly() {
        assert_eq!(split_uptime(0), (0, 0, 0, 0), "刚启动");
        assert_eq!(split_uptime(59), (0, 0, 0, 59));
        assert_eq!(split_uptime(60), (0, 0, 1, 0), "满 60 秒进 1 分");
        assert_eq!(split_uptime(3599), (0, 0, 59, 59));
        assert_eq!(split_uptime(3600), (0, 1, 0, 0), "满 60 分进 1 小时");
        assert_eq!(split_uptime(86399), (0, 23, 59, 59));
        assert_eq!(split_uptime(86400), (1, 0, 0, 0), "满 24 小时进 1 天");
        assert_eq!(split_uptime(86460), (1, 0, 1, 0));
        // 原型里那张卡的示例值：「6 天 4 小时」
        assert_eq!(split_uptime(6 * 86400 + 4 * 3600), (6, 4, 0, 0));
        // 长跑一年有余：不得溢出位宽
        assert_eq!(
            split_uptime(400 * 86400 + 23 * 3600 + 59 * 60 + 59),
            (400, 23, 59, 59)
        );
    }

    /// 分解结果必须**无损**（拼回去等于原值）且各位**已归一**。
    ///
    /// 与上面的真值例互补：真值例证明具体取值对，这一条覆盖一片区间，
    /// 保证不存在「某个具体秒数进位不对」的漏网值。
    #[test]
    fn split_uptime_is_lossless_and_normalized() {
        for secs in (0..100_000u64).step_by(7) {
            let (d, h, m, s) = split_uptime(secs);
            assert_eq!(
                d * 86400 + h * 3600 + m * 60 + s,
                secs,
                "拼不回原值: {secs}"
            );
            assert!(
                h < 24 && m < 60 && s < 60,
                "位未归一: {secs} -> {h}/{m}/{s}"
            );
        }
    }
}
