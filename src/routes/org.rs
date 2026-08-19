//! 部门管理 API（P2-C，rant 2026-08-18T14:03:51）
//!
//! - GET    /api/admin/departments：部门列表（成员数 / 本月已用 / 剩余，join users + usage_records）
//! - POST   /api/admin/departments {name, quota}：新增部门（重名 409）
//! - PATCH  /api/admin/departments/:id {name?, quota?}：改部门名 / 月分配（重名 409）
//! - DELETE /api/admin/departments/:id：删除部门（有成员 409）
//! - PATCH  /api/admin/users/:id {dept_id}：成员改部门 / 移除（见 admin.rs patch_user）

use axum::extract::{Path, State};
use axum::Json;
use rusqlite::params;
use serde::Deserialize;

use crate::routes::{internal, ApiErr, AppState, AuthUser};

/// 权限判定：仅 admin（管理端点统一）
pub(crate) fn require_admin(auth: &AuthUser) -> Result<(), ApiErr> {
    if auth.role == "admin" {
        Ok(())
    } else {
        Err((
            axum::http::StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "需要管理员权限" })),
        ))
    }
}

/// 新增部门请求
#[derive(Debug, Deserialize)]
pub struct DeptReq {
    pub name: String,
    pub quota: f64,
}

/// PATCH 部门请求（字段可选）
#[derive(Debug, Default, Deserialize)]
pub struct DeptPatch {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub quota: Option<f64>,
}

/// GET /api/admin/departments：部门 + 成员数 + 本月已用（usage_records 经 users.dept_id 聚合）+ 剩余
pub async fn list(
    State(st): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<serde_json::Value>>, ApiErr> {
    require_admin(&auth)?;
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let mut stmt = conn
        .prepare(
            "SELECT d.id, d.name, d.quota, d.created_at, \
                    (SELECT COUNT(*) FROM users u WHERE u.dept_id = d.id), \
                    (SELECT COALESCE(SUM(ur.cost), 0) FROM usage_records ur \
                      JOIN users u ON u.id = ur.user_id WHERE u.dept_id = d.id \
                      AND strftime('%Y-%m', ur.time) = strftime('%Y-%m', 'now')) \
             FROM departments d ORDER BY d.id",
        )
        .map_err(internal)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?,
                "name": r.get::<_, String>(1)?,
                "quota": r.get::<_, f64>(2)?,
                "created_at": crate::dao::utc_iso(&r.get::<_, String>(3)?),
                "member_count": r.get::<_, i64>(4)?,
                "month_cost": r.get::<_, f64>(5)?,
                "remaining": r.get::<_, f64>(2)? - r.get::<_, f64>(5)?,
            }))
        })
        .map_err(internal)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(internal)?);
    }
    Ok(Json(out))
}

/// POST /api/admin/departments：新增部门（重名 → 409）
pub async fn create(
    State(st): State<AppState>,
    auth: AuthUser,
    Json(req): Json<DeptReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    require_admin(&auth)?;
    let name = req.name.trim().to_string();
    if name.is_empty() || req.quota <= 0.0 {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "name 不能为空且 quota 必须大于 0" })),
        ));
    }
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let dup: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM departments WHERE name = ?1)",
            [&name],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if dup {
        return Err((
            axum::http::StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": format!("部门「{name}」已存在") })),
        ));
    }
    conn.execute(
        "INSERT INTO departments (name, quota) VALUES (?1, ?2)",
        params![name, req.quota],
    )
    .map_err(internal)?;
    let id = conn.last_insert_rowid();
    Ok(Json(
        serde_json::json!({ "id": id, "name": name, "quota": req.quota }),
    ))
}

/// PATCH /api/admin/departments/:id：改名 / 改月分配（重名 → 409）
pub async fn patch(
    State(st): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<DeptPatch>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    require_admin(&auth)?;
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM departments WHERE id = ?1)",
            [id],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if !exists {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "部门不存在" })),
        ));
    }
    if !req.name.is_empty() {
        let dup: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM departments WHERE name = ?1 AND id != ?2)",
                params![req.name.trim(), id],
                |r| r.get(0),
            )
            .unwrap_or(false);
        if dup {
            return Err((
                axum::http::StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": format!("部门「{}」已存在", req.name.trim()) })),
            ));
        }
        conn.execute(
            "UPDATE departments SET name = ?1 WHERE id = ?2",
            params![req.name.trim(), id],
        )
        .map_err(internal)?;
    }
    if let Some(q) = req.quota {
        if q <= 0.0 {
            return Err((
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "quota 必须大于 0" })),
            ));
        }
        conn.execute(
            "UPDATE departments SET quota = ?1 WHERE id = ?2",
            params![q, id],
        )
        .map_err(internal)?;
    }
    let (name, quota): (String, f64) = conn
        .query_row(
            "SELECT name, quota FROM departments WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(internal)?;
    Ok(Json(
        serde_json::json!({ "id": id, "name": name, "quota": quota }),
    ))
}

/// DELETE /api/admin/departments/:id：删除（有成员 → 409）
pub async fn remove(
    State(st): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    require_admin(&auth)?;
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM departments WHERE id = ?1)",
            [id],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if !exists {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "部门不存在" })),
        ));
    }
    let members: i64 = conn
        .query_row("SELECT COUNT(*) FROM users WHERE dept_id = ?1", [id], |r| {
            r.get(0)
        })
        .unwrap_or(0);
    if members > 0 {
        return Err((
            axum::http::StatusCode::CONFLICT,
            Json(
                serde_json::json!({ "error": format!("部门下还有 {members} 名成员，请先调整成员部门") }),
            ),
        ));
    }
    conn.execute("DELETE FROM departments WHERE id = ?1", [id])
        .map_err(internal)?;
    Ok(Json(serde_json::json!({ "deleted": id })))
}
