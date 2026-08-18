//! 加额审批 API（P2-C，rant 2026-08-18T14:03:51，US-20）
//!
//! - POST /api/raise-requests {amount, reason}：用户自助申请（正整数；pending 重复 409）
//! - GET  /api/raise-requests?status=：用户看自己的；admin 看全部（role 区分）
//! - POST /api/admin/raise-requests/:id/approve：批准 → 永久 balance += amount + transactions(type='topup', counterpart='加额审批')
//! - POST /api/admin/raise-requests/:id/reject：驳回

use axum::extract::{Query, State};
use axum::Json;
use rusqlite::params;
use serde::Deserialize;

use crate::routes::{internal, ApiErr, AppState, AuthUser};

/// 申请请求
#[derive(Debug, Deserialize)]
pub struct RaiseReq {
    pub amount: f64,
    pub reason: String,
}

/// 列表查询参数
#[derive(Debug, Default, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub status: String,
}

/// POST /api/raise-requests：成员自助申请加额
pub async fn create(
    State(st): State<AppState>,
    auth: AuthUser,
    Json(req): Json<RaiseReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let reason = req.reason.trim().to_string();
    if !req.amount.is_finite() || req.amount <= 0.0 || req.amount.fract() != 0.0 {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "amount 必须为正整数" })),
        ));
    }
    if reason.is_empty() {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "reason 不能为空" })),
        ));
    }
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    // 已有 pending 申请 → 409（防重复提交）
    let pending: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM raise_requests WHERE user_id = ?1 AND status = 'pending'",
            [auth.user_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if pending > 0 {
        return Err((
            axum::http::StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "已有待审批的加额申请，请等待管理员处理" })),
        ));
    }
    conn.execute(
        "INSERT INTO raise_requests (user_id, amount, reason, status) VALUES (?1, ?2, ?3, 'pending')",
        params![auth.user_id, req.amount, reason],
    )
    .map_err(internal)?;
    let id = conn.last_insert_rowid();
    Ok(Json(serde_json::json!({
        "id": id,
        "amount": req.amount,
        "reason": reason,
        "status": "pending",
    })))
}

/// GET /api/raise-requests：用户看自己的；admin 看全部；?status= 过滤
pub async fn list(
    State(st): State<AppState>,
    auth: AuthUser,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<serde_json::Value>>, ApiErr> {
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let status = q.status.trim().to_string();
    let is_admin = auth.role == "admin";
    let (base, param): (&str, bool) = if is_admin {
        (
            "SELECT r.id, r.user_id, u.email, u.name, r.amount, r.reason, r.status, r.created_at              FROM raise_requests r JOIN users u ON u.id = r.user_id",
            false,
        )
    } else {
        (
            "SELECT r.id, r.user_id, u.email, u.name, r.amount, r.reason, r.status, r.created_at              FROM raise_requests r JOIN users u ON u.id = r.user_id WHERE r.user_id = ?1",
            true,
        )
    };
    let sql = if status.is_empty() {
        format!("{base} ORDER BY r.id DESC")
    } else if is_admin {
        format!("{base} WHERE r.status = ?1 ORDER BY r.id DESC")
    } else {
        format!("{base} AND r.status = ?2 ORDER BY r.id DESC")
    };
    let mapper = |r: &rusqlite::Row| {
        Ok(serde_json::json!({
            "id": r.get::<_, i64>(0)?,
            "user_id": r.get::<_, i64>(1)?,
            "email": r.get::<_, String>(2)?,
            "name": r.get::<_, String>(3)?,
            "amount": r.get::<_, f64>(4)?,
            "reason": r.get::<_, String>(5)?,
            "status": r.get::<_, String>(6)?,
            "created_at": r.get::<_, String>(7)?,
        }))
    };
    let mut stmt = conn.prepare(&sql).map_err(internal)?;
    let rows = if param && status.is_empty() {
        stmt.query_map([auth.user_id], mapper)
            .map_err(internal)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(internal)?
    } else if !param && !status.is_empty() {
        stmt.query_map([&status], mapper)
            .map_err(internal)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(internal)?
    } else if param && !status.is_empty() {
        stmt.query_map(params![auth.user_id, &status], mapper)
            .map_err(internal)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(internal)?
    } else {
        stmt.query_map([], mapper)
            .map_err(internal)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(internal)?
    };
    Ok(Json(rows))
}

/// POST /api/admin/raise-requests/:id/approve：批准 → 永久点数 + 交易记录
pub async fn approve(
    State(st): State<AppState>,
    auth: AuthUser,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    crate::routes::org::require_admin(&auth)?;
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let row: Option<(i64, f64, String)> = conn
        .query_row(
            "SELECT user_id, amount, status FROM raise_requests WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok();
    let Some((user_id, amount, status)) = row else {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "申请不存在" })),
        ));
    };
    if status != "pending" {
        return Err((
            axum::http::StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "该申请已处理" })),
        ));
    }
    // 永久余额 += amount
    conn.execute(
        "INSERT OR IGNORE INTO quotas (user_id, balance) VALUES (?1, 0)",
        [user_id],
    )
    .map_err(internal)?;
    conn.execute(
        "UPDATE quotas SET balance = balance + ?1, updated_at = datetime('now') WHERE user_id = ?2",
        params![amount, user_id],
    )
    .map_err(internal)?;
    // 交易记录（type=topup, counterpart='加额审批'）
    conn.execute(
        "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status) \
         VALUES (?1, '加额审批', NULL, 'raise', 0, ?2, 'topup', '成功')",
        params![user_id, amount],
    )
    .map_err(internal)?;
    conn.execute(
        "UPDATE raise_requests SET status = 'approved', reviewed_by = ?1, reviewed_at = datetime('now') WHERE id = ?2",
        params![auth.user_id, id],
    )
    .map_err(internal)?;
    Ok(Json(serde_json::json!({
        "id": id,
        "status": "approved",
        "user_id": user_id,
        "amount": amount,
    })))
}

/// POST /api/admin/raise-requests/:id/reject：驳回
pub async fn reject(
    State(st): State<AppState>,
    auth: AuthUser,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    crate::routes::org::require_admin(&auth)?;
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let status: Option<String> = conn
        .query_row(
            "SELECT status FROM raise_requests WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .ok();
    let Some(status) = status else {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "申请不存在" })),
        ));
    };
    if status != "pending" {
        return Err((
            axum::http::StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "该申请已处理" })),
        ));
    }
    conn.execute(
        "UPDATE raise_requests SET status = 'rejected', reviewed_by = ?1, reviewed_at = datetime('now') WHERE id = ?2",
        params![auth.user_id, id],
    )
    .map_err(internal)?;
    Ok(Json(serde_json::json!({ "id": id, "status": "rejected" })))
}
