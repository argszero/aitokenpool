//! API Key 管理端点（Bearer 认证）
//!
//! - P0-A（rant 2026-08-17T22:21:52）：POST 生成（atk_live_ + 24 hex）；GET 列表（脱敏）
//! - P2-B（rant 2026-08-18T12:02:40）：DELETE /api/api-keys/:id 软删（status → 'revoked'），仅属主

use axum::extract::{Path, State};
use axum::Json;

use crate::auth;
use crate::routes::{internal, ApiErr, AppState, AuthUser};

/// POST /api/api-keys
pub async fn create(
    State(st): State<AppState>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let key = crate::dao::create_api_key(&conn, auth.user_id, "").map_err(internal)?;
    Ok(Json(serde_json::json!({
        "api_key": key,
        "masked": auth::mask_api_key(&key),
    })))
}

/// GET /api/api-keys（脱敏列表）
pub async fn list(
    State(st): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<serde_json::Value>>, ApiErr> {
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let keys = crate::dao::list_api_keys(&conn, auth.user_id).map_err(internal)?;
    Ok(Json(keys))
}

/// DELETE /api/api-keys/:id（软删；非属主 / 不存在 → 404）
pub async fn remove(
    State(st): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let ok = crate::dao::revoke_api_key(&conn, auth.user_id, id).map_err(internal)?;
    if !ok {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "API Key 不存在或已撤销" })),
        ));
    }
    Ok(Json(serde_json::json!({ "id": id, "status": "revoked" })))
}
