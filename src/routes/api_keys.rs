//! API Key 管理端点（Bearer 认证）
//!
//! P0-A（rant 2026-08-17T22:21:52）：
//! - POST /api/api-keys：生成（atk_live_ + 24 hex，与 UI 原型一致）
//! - GET  /api/api-keys：列表（key 脱敏 atk_live_****xxxx）

use axum::extract::State;
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
