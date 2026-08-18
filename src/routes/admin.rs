//! 管理员 API（对齐原型管理视图「成员充值/部门」）
//!
//! P1（rant 2026-08-18T11:03:02）：
//! - POST /api/admin/credits {user_id, amount, note?}：给成员永久点数充值（role=admin）
//! - GET  /api/admin/users：成员列表（id/email/name/balance/gift_balance/role）
//! - GET  /api/admin/usage：用量报表（每用户本月 tokens/点数/调用次数）
//! - 权限：require_admin（AuthUser.role == "admin"，否则 403）

use axum::extract::State;
use axum::Json;
use rusqlite::params;
use serde::Deserialize;

use crate::routes::{internal, ApiErr, AppState, AuthUser};

/// 充值请求
#[derive(Debug, Deserialize)]
pub struct CreditReq {
    pub user_id: i64,
    pub amount: f64,
    #[serde(default)]
    pub note: String,
}

/// 权限中间件判定（处理函数内调用）
fn require_admin(auth: &AuthUser) -> Result<(), ApiErr> {
    if auth.role == "admin" {
        Ok(())
    } else {
        Err((
            axum::http::StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "需要管理员权限" })),
        ))
    }
}

/// POST /api/admin/credits：给成员永久点数充值 + 写 transactions（type=topup）
pub async fn credits(
    State(st): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreditReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    require_admin(&auth)?;
    if req.amount <= 0.0 {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "amount 必须大于 0" })),
        ));
    }
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    // 目标用户存在？
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
         VALUES (?1, ?2, NULL, 'recharge', 0, ?3, 'topup', '成功')",
        params![req.user_id, auth.user_id.to_string(), req.amount],
    )
    .map_err(internal)?;
    let balance: f64 = conn
        .query_row(
            "SELECT balance FROM quotas WHERE user_id = ?1",
            [req.user_id],
            |r| r.get(0),
        )
        .unwrap_or(0.0);
    Ok(Json(serde_json::json!({
        "user_id": req.user_id,
        "amount": req.amount,
        "balance": balance,
        "note": req.note,
    })))
}

/// GET /api/admin/users：成员列表
pub async fn users(
    State(st): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<serde_json::Value>>, ApiErr> {
    require_admin(&auth)?;
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

/// GET /api/admin/usage：用量报表（每用户本月 tokens/点数/调用次数）
pub async fn usage(
    State(st): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<serde_json::Value>>, ApiErr> {
    require_admin(&auth)?;
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let mut stmt = conn
        .prepare(
            "SELECT u.id, u.email, u.name, \
                    COALESCE(SUM(ur.tokens), 0), COALESCE(SUM(ur.cost), 0), COUNT(ur.id) \
             FROM users u \
             LEFT JOIN usage_records ur ON ur.user_id = u.id \
                 AND strftime('%Y-%m', ur.time) = strftime('%Y-%m', 'now') \
             GROUP BY u.id ORDER BY u.id",
        )
        .map_err(internal)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?,
                "email": r.get::<_, String>(1)?,
                "name": r.get::<_, String>(2)?,
                "month_tokens": r.get::<_, f64>(3)?,
                "month_cost": r.get::<_, f64>(4)?,
                "month_calls": r.get::<_, i64>(5)?,
            }))
        })
        .map_err(internal)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(internal)?);
    }
    Ok(Json(out))
}
