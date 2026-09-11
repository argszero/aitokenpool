//! 运营者 API（P2-C，rant 2026-08-18T14:03:51）
//!
//! 运营者（role=ops）= 平台运维角色，职责最小化：运行概览 + 给任意用户充值。
//! - GET  /api/ops/runtime：平台运行概览（用户数 / 上架 key 数 / 本月调用 / 本月点数流水，全库聚合）
//!   + rant 2026-09-11T16:23:43（PR6）：今日按小时调用量 today_hours（0-23 全量补零，UTC 小时）
//!     + 上游 key 健康 key_health（按厂商聚合 total/on/off）
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

/// GET /api/ops/runtime：平台运行概览（全库聚合）
pub async fn runtime(
    State(st): State<AppState>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, ApiErr> {
    require_role(&auth)?;
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
            "SELECT COUNT(*) FROM usage_records WHERE strftime('%Y-%m', time) = strftime('%Y-%m', 'now')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let month_in: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(pts), 0) FROM transactions \
             WHERE pts > 0 AND strftime('%Y-%m', time) = strftime('%Y-%m', 'now')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0.0);
    let month_out: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(ABS(pts)), 0) FROM transactions \
             WHERE pts < 0 AND strftime('%Y-%m', time) = strftime('%Y-%m', 'now')",
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
                 WHERE date(time) = date('now') GROUP BY h",
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
