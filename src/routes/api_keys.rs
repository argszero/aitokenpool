//! API Key 管理端点（Bearer 认证）
//!
//! - P0-A（rant 2026-08-17T22:21:52）：POST 生成（atk_live_ + 24 hex）；GET 列表（脱敏）
//! - P2-B（rant 2026-08-18T12:02:40）：DELETE /api/api-keys/:id 软删（status → 'revoked'），仅属主
//! - P2-C（rant 2026-08-22T17:21:39）：POST 接收 {name} 持久化；PATCH /api/api-keys/:id 改名，仅属主
//! - 启停（rant 2026-11-12T14:13:23）：PATCH 同一入口也接受 `{status: active|paused}` ——
//!   「临时停一下、过后开回来」不必再走不可逆的软删。取值集合取自 `dao::LIVE_KEY_STATUSES`。

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::auth;
use crate::dao::{self, KEY_STATUS_PAUSED, LIVE_KEY_STATUSES};
use crate::routes::{internal, ApiErr, AppState, AuthUser};

/// POST /api/api-keys 请求体；name 缺省为空串（前端留空 → 「未命名」）
#[derive(Deserialize)]
pub struct CreateKeyReq {
    #[serde(default)]
    pub name: String,
}

/// PATCH /api/api-keys/:id 请求体：改名与启停**同一入口**（P2-C ＋ rant 2026-11-12T14:13:23）。
///
/// 两个字段都可省略，但至少要有一个 —— 两个都不给是客户端写错了，不是「什么都不用改」。
#[derive(Deserialize)]
pub struct PatchKeyReq {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

/// 不存在 / 非属主 / 已撤销 → 同一个 404（不区分「不是你的」与「不存在」，也就不泄露 id 是否存在）
fn key_not_found() -> ApiErr {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "API Key 不存在或已撤销" })),
    )
}

/// POST /api/api-keys
pub async fn create(
    State(st): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateKeyReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let key = dao::create_api_key(&conn, auth.user_id, &req.name).map_err(internal)?;
    Ok(Json(serde_json::json!({
        "api_key": key,
        "masked": auth::mask_api_key(&key),
    })))
}

/// PATCH /api/api-keys/:id —— 改名（P2-C）与启停（rant 2026-11-12T14:13:23）。
///
/// 非属主 / 不存在 / 已撤销 → 404。
///
/// ⚠️ **不能暂停「当前登录用的那把 key」**（下面那条守卫）。这不是保守，是这个架构逼出来的：
/// API Key 同时是**会话凭据** —— `AuthUser` 提取器就靠它认证，而 `dao::find_api_key_user_and_id`
/// 只认 `active`（rant 明确要求鉴权不动）。于是暂停自己那把 = 这次会话立刻失效；重新登录时
/// `get_or_create_api_key` 找不到 active 行，会**另发一把新 key** ⇒ 那次暂停什么也没停住，
/// 而这正是 rant 点名要堵的洞。反过来若让它不发新 key，用户就再也进不来、那一行也就再也恢复不了。
/// 两条出口都是坏的，所以取第三条：拒绝，并把原因说清楚（前端据 `current` 标记先置灰按钮）。
pub async fn patch(
    State(st): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<PatchKeyReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    if req.name.is_none() && req.status.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "参数错误" })),
        ));
    }
    if let Some(status) = req.status.as_deref() {
        if !LIVE_KEY_STATUSES.contains(&status) {
            return Err((
                StatusCode::BAD_REQUEST,
                // 值必须写成**字面量**：`i18n_pack::every_error_key_site_has_a_readable_write_form`
                // 只认「可读的写入形态」，调用表达式会让这条文案整条逃过词表门禁（en 界面上
                // 就成了中文）。两个可取值的对账放在 `the_status_400_names_every_accepted_value`。
                Json(serde_json::json!({ "error": "status 必须为 active 或 paused" })),
            ));
        }
        // 那把 key 既是「要停的东西」又是「正在用它说话的身份」—— 见上面的长注释。
        if status == KEY_STATUS_PAUSED && id == auth.api_key_id {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "当前登录用的就是这个 key，不能暂停它自己"
                })),
            ));
        }
    }
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    // 顺序：先改名（只动 name），再启停。任一守卫不通过都回 404；**已改的部分不回滚** ——
    // 两个字段各是一次 UPDATE，响应里带的是真实的 name/status，不谎报「全都成了」。
    if let Some(name) = req.name.as_ref() {
        if !dao::rename_api_key(&conn, auth.user_id, id, name).map_err(internal)? {
            return Err(key_not_found());
        }
    }
    let mut status = None;
    if let Some(next) = req.status.as_deref() {
        if !dao::set_api_key_status(&conn, auth.user_id, id, next).map_err(internal)? {
            return Err(key_not_found());
        }
        status = Some(next);
    }
    let name = match req.name {
        Some(name) => Some(name),
        None => conn
            .query_row(
                "SELECT name FROM api_keys WHERE id = ?1 AND user_id = ?2",
                rusqlite::params![id, auth.user_id],
                |r| r.get::<_, String>(0),
            )
            .ok(),
    };
    Ok(Json(serde_json::json!({
        "id": id,
        "name": name,
        "status": status,
    })))
}

/// GET /api/api-keys（脱敏列表）
///
/// 每行多一个 `current`（rant 2026-11-12T14:13:23）：「这行就是当前会话用的 key」由**服务端**
/// 标记，前端不靠猜 —— 设置页据此把那一行的暂停按钮置灰，而不是等用户点了才吃一个 400。
pub async fn list(
    State(st): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<serde_json::Value>>, ApiErr> {
    let mut keys = {
        let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
        dao::list_api_keys(&conn, auth.user_id).map_err(internal)?
    };
    for k in keys.iter_mut() {
        if let Some(obj) = k.as_object_mut() {
            let id = obj.get("id").and_then(|v| v.as_i64());
            obj.insert(
                "current".to_string(),
                serde_json::Value::Bool(id == Some(auth.api_key_id)),
            );
        }
    }
    Ok(Json(keys))
}

/// DELETE /api/api-keys/:id（软删；非属主 / 不存在 / 已撤销 → 404）
pub async fn remove(
    State(st): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let ok = dao::revoke_api_key(&conn, auth.user_id, id).map_err(internal)?;
    if !ok {
        return Err(key_not_found());
    }
    Ok(Json(serde_json::json!({ "id": id, "status": "revoked" })))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 非法 `status` 的文案必须点名**每一个**被接受的取值。
    ///
    /// 文案本身是字面量（词表门禁只认字面量），所以这里也**从源码里取那句话**，而不是在测试
    /// 里再抄一份：将来加第三种可切换状态却忘了改文案时，这条先红。
    #[test]
    fn the_status_400_names_every_accepted_value() {
        const SRC: &str = include_str!("api_keys.rs");
        let line = SRC
            .lines()
            .find(|l| l.contains("\"error\": \"status 必须为"))
            .expect("非法 status 的 400 文案没找到 —— 它必须写成字面量");
        let rest = &line[line.find("\"error\": \"").unwrap() + "\"error\": \"".len()..];
        let msg = &rest[..rest.find('"').unwrap()];
        for s in LIVE_KEY_STATUSES {
            assert!(msg.contains(s), "400 文案没有点名可接受的取值 {s:?}：{msg}");
        }
        assert!(!msg.contains("revoked"), "撤销不是可接受的目标状态：{msg}");
    }

    /// 状态集本身：`revoked` 不在可切换集合里（软删是终态）。
    #[test]
    fn live_statuses_are_active_and_paused_only() {
        assert_eq!(
            LIVE_KEY_STATUSES,
            [dao::KEY_STATUS_ACTIVE, dao::KEY_STATUS_PAUSED]
        );
    }
}
