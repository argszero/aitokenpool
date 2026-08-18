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
            "SELECT u.id, u.email, u.name, u.role, COALESCE(q.balance, 0), COALESCE(q.gift_balance, 0), \
                    u.dept_id, COALESCE(d.name, '') \
             FROM users u LEFT JOIN quotas q ON q.user_id = u.id \
             LEFT JOIN departments d ON d.id = u.dept_id ORDER BY u.id",
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
                "dept_id": r.get::<_, Option<i64>>(6)?,
                "dept_name": r.get::<_, String>(7)?,
            }))
        })
        .map_err(internal)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(internal)?);
    }
    Ok(Json(out))
}

/// PATCH /api/admin/users/:id：成员改部门 / 移除（P2-C；{dept_id: null} = 移出部门）
#[derive(Debug, Deserialize)]
pub struct UserPatch {
    pub dept_id: Option<i64>,
}

pub async fn patch_user(
    State(st): State<AppState>,
    auth: AuthUser,
    axum::extract::Path(id): axum::extract::Path<i64>,
    Json(req): Json<UserPatch>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    crate::routes::org::require_admin(&auth)?;
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM users WHERE id = ?1)",
            [id],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if !exists {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "用户不存在" })),
        ));
    }
    if let Some(did) = req.dept_id {
        let dept_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM departments WHERE id = ?1)",
                [did],
                |r| r.get(0),
            )
            .unwrap_or(false);
        if !dept_exists {
            return Err((
                axum::http::StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "部门不存在" })),
            ));
        }
        conn.execute(
            "UPDATE users SET dept_id = ?1 WHERE id = ?2",
            params![did, id],
        )
        .map_err(internal)?;
    } else {
        conn.execute("UPDATE users SET dept_id = NULL WHERE id = ?1", [id])
            .map_err(internal)?;
    }
    let (email, dept_id): (String, Option<i64>) = conn
        .query_row(
            "SELECT email, dept_id FROM users WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(internal)?;
    Ok(Json(serde_json::json!({
        "id": id,
        "email": email,
        "dept_id": dept_id,
    })))
}

/// GET /api/admin/usage：用量报表（P2-C 扩展为三组聚合）
/// 返回 { users: [{id,email,name,dept_id,dept_name,month_tokens,month_cost,month_calls}],
///         models: [{model,tokens,cost,calls}],
///         departments: [{id,name,tokens,cost,calls}] }
pub async fn usage(
    State(st): State<AppState>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, ApiErr> {
    require_admin(&auth)?;
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    // 按成员
    let mut stmt = conn
        .prepare(
            "SELECT u.id, u.email, u.name, u.dept_id, COALESCE(d.name, ''), \
                    COALESCE(SUM(ur.tokens), 0), COALESCE(SUM(ur.cost), 0), COUNT(ur.id) \
             FROM users u \
             LEFT JOIN usage_records ur ON ur.user_id = u.id \
                 AND strftime('%Y-%m', ur.time) = strftime('%Y-%m', 'now') \
             LEFT JOIN departments d ON d.id = u.dept_id \
             GROUP BY u.id ORDER BY u.id",
        )
        .map_err(internal)?;
    let mut users = Vec::new();
    {
        let rows = stmt
            .query_map([], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "email": r.get::<_, String>(1)?,
                    "name": r.get::<_, String>(2)?,
                    "dept_id": r.get::<_, Option<i64>>(3)?,
                    "dept_name": r.get::<_, String>(4)?,
                    "month_tokens": r.get::<_, f64>(5)?,
                    "month_cost": r.get::<_, f64>(6)?,
                    "month_calls": r.get::<_, i64>(7)?,
                }))
            })
            .map_err(internal)?;
        for r in rows {
            users.push(r.map_err(internal)?);
        }
    }
    // 按模型
    let mut stmt = conn
        .prepare(
            "SELECT COALESCE(model, ''), COALESCE(SUM(tokens), 0), COALESCE(SUM(cost), 0), COUNT(*) \
             FROM usage_records WHERE strftime('%Y-%m', time) = strftime('%Y-%m', 'now') \
             GROUP BY model ORDER BY SUM(cost) DESC",
        )
        .map_err(internal)?;
    let mut models = Vec::new();
    {
        let rows = stmt
            .query_map([], |r| {
                Ok(serde_json::json!({
                    "model": r.get::<_, String>(0)?,
                    "tokens": r.get::<_, f64>(1)?,
                    "cost": r.get::<_, f64>(2)?,
                    "calls": r.get::<_, i64>(3)?,
                }))
            })
            .map_err(internal)?;
        for r in rows {
            models.push(r.map_err(internal)?);
        }
    }
    // 按部门
    let mut stmt = conn
        .prepare(
            "SELECT COALESCE(d.name, '（未分配）'), COALESCE(SUM(ur.tokens), 0), COALESCE(SUM(ur.cost), 0), COUNT(ur.id) \
             FROM usage_records ur JOIN users u ON u.id = ur.user_id \
             LEFT JOIN departments d ON d.id = u.dept_id \
             WHERE strftime('%Y-%m', ur.time) = strftime('%Y-%m', 'now') \
             GROUP BY d.id ORDER BY SUM(ur.cost) DESC",
        )
        .map_err(internal)?;
    let mut departments = Vec::new();
    {
        let rows = stmt
            .query_map([], |r| {
                Ok(serde_json::json!({
                    "name": r.get::<_, String>(0)?,
                    "tokens": r.get::<_, f64>(1)?,
                    "cost": r.get::<_, f64>(2)?,
                    "calls": r.get::<_, i64>(3)?,
                }))
            })
            .map_err(internal)?;
        for r in rows {
            departments.push(r.map_err(internal)?);
        }
    }
    Ok(Json(serde_json::json!({
        "users": users,
        "models": models,
        "departments": departments,
    })))
}
