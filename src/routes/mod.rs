//! HTTP 路由装配：healthz + 认证 + API Key 管理 + 网关/市场端点
//!
//! P0-A（rant 2026-08-17T22:21:52）：
//! - GET /healthz → {"status":"ok","version":"0.2.0"}
//! - POST /api/auth/login → 200 {api_key} / 401
//! - POST /api/api-keys / GET /api/api-keys（Bearer 认证）
//!
//! P0-B（rant 2026-08-18T09:55:57）：
//! - POST /v1/chat/completions / POST /anthropic/v1/messages（网关）
//! - GET /api/models（市场）

pub mod admin;
pub mod api_keys;
pub mod sharing;
pub mod wallet;

use std::sync::{Arc, Mutex};

use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use rusqlite::Connection;
use serde::Deserialize;

use crate::config::Config;
use crate::crypto::Crypto;
use crate::dao;
use crate::gateway;
use crate::router::RouterState;

/// 共享状态：数据库连接 + 配置 + 路由状态 + HTTP 客户端 + 密钥加密器
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub cfg: Arc<Config>,
    pub router: Arc<RouterState>,
    pub http: reqwest::Client,
    pub crypto: Crypto,
}

impl AppState {
    pub fn new(conn: Connection, cfg: Arc<Config>, crypto: Crypto) -> Self {
        Self {
            db: Arc::new(Mutex::new(conn)),
            cfg,
            router: Arc::new(RouterState::new()),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("reqwest client 构建失败"),
            crypto,
        }
    }
}

/// 统一错误响应类型
pub type ApiErr = (StatusCode, Json<serde_json::Value>);

fn unauthorized() -> ApiErr {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "unauthorized" })),
    )
}

pub fn internal(e: impl std::fmt::Display) -> ApiErr {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": format!("{e}") })),
    )
}

/// 已认证用户（Bearer 提取器）：无效 key → 401
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: i64,
    pub api_key_id: i64,
    /// 用户角色：user | admin（P1 起 require_admin 使用）
    pub role: String,
}

#[axum::async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiErr;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let key = header.strip_prefix("Bearer ").ok_or_else(unauthorized)?;
        let conn = state.db.lock().map_err(|_| internal("db lock poisoned"))?;
        match dao::find_api_key_user_and_id(&conn, key) {
            Some((user_id, api_key_id, role)) => {
                let _ = dao::touch_api_key(&conn, key);
                Ok(AuthUser {
                    user_id,
                    api_key_id,
                    role,
                })
            }
            None => Err(unauthorized()),
        }
    }
}

/// GET /healthz
pub async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

#[derive(Deserialize)]
pub struct LoginReq {
    pub email: String,
    pub password: String,
}

/// POST /api/auth/login：email+password → 200 {api_key} / 401
pub async fn login(
    State(st): State<AppState>,
    Json(req): Json<LoginReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let user_id =
        dao::verify_user_password(&conn, &req.email, &req.password).map_err(|_| unauthorized())?;
    let api_key = dao::get_or_create_api_key(&conn, user_id).map_err(internal)?;
    Ok(Json(serde_json::json!({
        "api_key": api_key,
        "user_id": user_id,
    })))
}

/// GET /api/me：当前登录用户信息（P2-A 前端会话）→ {id, email, name, role}
pub async fn me(
    State(st): State<AppState>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let (email, name, role): (String, String, String) = conn
        .query_row(
            "SELECT email, name, role FROM users WHERE id = ?1",
            [auth.user_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(internal)?;
    Ok(Json(serde_json::json!({
        "id": auth.user_id,
        "email": email,
        "name": name,
        "role": role,
    })))
}

/// 组装路由：API 路由优先，其余请求回退到 ui/ 静态托管（P2-A）
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/auth/login", post(login))
        .route("/api/me", get(me))
        .route("/api/api-keys", post(api_keys::create).get(api_keys::list))
        .route("/api/api-keys/:id", axum::routing::delete(api_keys::remove))
        .route("/v1/chat/completions", post(gateway::chat_completions))
        .route("/anthropic/v1/messages", post(gateway::anthropic_messages))
        .route("/api/models", get(gateway::models))
        // P0-C：共享 / 钱包 / 交易 / 仪表盘
        .route("/api/sharings", post(sharing::create).get(sharing::list))
        .route("/api/sharings/:id", axum::routing::patch(sharing::patch))
        .route("/api/wallet", get(wallet::wallet))
        .route("/api/transactions", get(wallet::transactions))
        .route("/api/dashboard", get(wallet::dashboard))
        // P1：管理员（充值 / 成员列表 / 用量报表）
        .route("/api/admin/credits", post(admin::credits))
        .route("/api/admin/users", get(admin::users))
        .route("/api/admin/usage", get(admin::usage))
        // P2-A：静态托管（ui/ 目录；API 路由优先，未命中回退到文件服务）
        .fallback_service(tower_http::services::ServeDir::new("ui"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use std::sync::Arc;
    use tower::util::ServiceExt;

    fn test_state(tag: &str) -> AppState {
        let p = std::env::temp_dir().join(format!("atp_route_{}_{}.db", std::process::id(), tag));
        let _ = std::fs::remove_file(&p);
        let conn = crate::db::open(p.to_str().unwrap()).expect("open tmp db");
        let cfg = crate::config::Config::load("config/config.example.toml").unwrap();
        crate::db::seed_models(&conn, &cfg).expect("seed models");
        let crypto = crate::crypto::Crypto::new([9u8; 32]);
        AppState::new(conn, Arc::new(cfg), crypto)
    }

    async fn post(
        state: AppState,
        uri: &str,
        body: &str,
        bearer: Option<&str>,
    ) -> (StatusCode, String) {
        let mut b = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(k) = bearer {
            b = b.header("authorization", format!("Bearer {k}"));
        }
        let resp = router()
            .with_state(state)
            .oneshot(b.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    async fn get(state: AppState, uri: &str, bearer: Option<&str>) -> (StatusCode, String) {
        let mut b = Request::builder().method("GET").uri(uri);
        if let Some(k) = bearer {
            b = b.header("authorization", format!("Bearer {k}"));
        }
        let resp = router()
            .with_state(state)
            .oneshot(b.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    async fn del(state: AppState, uri: &str, bearer: Option<&str>) -> (StatusCode, String) {
        let mut b = Request::builder().method("DELETE").uri(uri);
        if let Some(k) = bearer {
            b = b.header("authorization", format!("Bearer {k}"));
        }
        let resp = router()
            .with_state(state)
            .oneshot(b.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn healthz_ok() {
        let (s, body) = get(test_state("healthz"), "/healthz", None).await;
        assert_eq!(s, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["status"], "ok");
        assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn login_ok_and_returns_key() {
        let st = test_state("login");
        let (s, body) = post(
            st.clone(),
            "/api/auth/login",
            r#"{"email":"demo@aitokenpool.local","password":"demo1234"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "正确口令应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let key = v["api_key"].as_str().expect("返回 api_key");
        assert!(key.starts_with("atk_live_"));
        // 再次登录返回同一 key（get-or-create）
        let (_, body2) = post(
            st,
            "/api/auth/login",
            r#"{"email":"demo@aitokenpool.local","password":"demo1234"}"#,
            None,
        )
        .await;
        let v2: serde_json::Value = serde_json::from_str(&body2).unwrap();
        assert_eq!(v2["api_key"].as_str().unwrap(), key, "重复登录应复用 key");
    }

    #[tokio::test]
    async fn login_wrong_password_401() {
        let (s, _) = post(
            test_state("login401"),
            "/api/auth/login",
            r#"{"email":"demo@aitokenpool.local","password":"wrong"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn api_keys_create_and_list_with_bearer() {
        let st = test_state("keys");
        // 登录拿 key
        let (_, body) = post(
            st.clone(),
            "/api/auth/login",
            r#"{"email":"demo@aitokenpool.local","password":"demo1234"}"#,
            None,
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let bearer = v["api_key"].as_str().unwrap().to_string();
        // POST 生成
        let (s, body) = post(st.clone(), "/api/api-keys", "{}", Some(&bearer)).await;
        assert_eq!(s, StatusCode::OK, "有效 Bearer 生成 key: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let new_key = v["api_key"].as_str().unwrap();
        assert!(new_key.starts_with("atk_live_"));
        // GET 列表脱敏
        let (s, body) = get(st.clone(), "/api/api-keys", Some(&bearer)).await;
        assert_eq!(s, StatusCode::OK);
        let arr: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        assert!(arr.len() >= 2, "列表含登录 key + 新生成 key");
        assert!(
            arr.iter()
                .all(|k| k["key"].as_str().unwrap().contains("****")),
            "key 全部脱敏"
        );
        // 无效 Bearer → 401
        let (s, _) = get(
            st,
            "/api/api-keys",
            Some("atk_live_000000000000000000000000"),
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn api_keys_without_bearer_401() {
        let (s, _) = get(test_state("nobearer"), "/api/api-keys", None).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn api_key_delete_revokes_own_only() {
        let st = test_state("keydel");
        let demo_bearer = login_bearer(&st, "demo@aitokenpool.local", "demo1234").await;
        // 生成一个 key → 拿到 id（从列表）
        let (_, body) = post(st.clone(), "/api/api-keys", "{}", Some(&demo_bearer)).await;
        assert!(serde_json::from_str::<serde_json::Value>(&body).unwrap()["api_key"].is_string());
        let (_, body) = get(st.clone(), "/api/api-keys", Some(&demo_bearer)).await;
        let arr: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        let new_id = arr[0]["id"].as_i64().expect("有 id");
        // 删除
        let (s, body) = del(
            st.clone(),
            &format!("/api/api-keys/{new_id}"),
            Some(&demo_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "删除应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["status"], "revoked");
        // 列表不再显示（revoked 过滤）
        let (_, body) = get(st.clone(), "/api/api-keys", Some(&demo_bearer)).await;
        let arr: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        assert!(
            !arr.iter().any(|k| k["id"] == new_id),
            "撤销后不再出现在列表"
        );
        // 再删 → 404（已撤销）
        let (s, _) = del(st, &format!("/api/api-keys/{new_id}"), Some(&demo_bearer)).await;
        assert_eq!(s, StatusCode::NOT_FOUND);
    }

    /// 登录并返回 Bearer
    async fn login_bearer(st: &AppState, email: &str, password: &str) -> String {
        let (s, body) = post(
            st.clone(),
            "/api/auth/login",
            &format!(r#"{{"email":"{email}","password":"{password}"}}"#),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "登录应成功: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        v["api_key"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn admin_credits_rejects_non_admin() {
        let st = test_state("admin403");
        let demo_bearer = login_bearer(&st, "demo@aitokenpool.local", "demo1234").await;
        let (s, body) = post(
            st.clone(),
            "/api/admin/credits",
            r#"{"user_id":2,"amount":50}"#,
            Some(&demo_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN, "非 admin 应 403: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v["error"].as_str().is_some(), "返回错误信息");
    }

    #[tokio::test]
    async fn admin_credits_recharges_permanent_and_writes_topup() {
        let st = test_state("admincred");
        let admin_bearer = login_bearer(&st, "admin@aitokenpool.local", "admin1234").await;
        // 充值前余额
        let (s, body) = get(st.clone(), "/api/wallet", Some(&admin_bearer)).await;
        assert_eq!(s, StatusCode::OK, "admin 可看钱包: {body}");
        // 给 demo（user_id=1）充 50 点
        let (s, body) = post(
            st.clone(),
            "/api/admin/credits",
            r#"{"user_id":1,"amount":50,"note":"P1 test"}"#,
            Some(&admin_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "充值应成功: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["amount"], 50.0);
        assert_eq!(v["balance"], 12471.0 + 50.0, "demo 余额增加 50");
        // transactions 出现 topup 记录（demo 视角）
        let demo_bearer = login_bearer(&st, "demo@aitokenpool.local", "demo1234").await;
        let (s, body) = get(
            st.clone(),
            "/api/transactions?type=topup",
            Some(&demo_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "topup 过滤应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let arr = v["items"].as_array().expect("items 为数组");
        assert_eq!(arr.len(), 1, "一条 topup 记录: {body}");
        assert_eq!(arr[0]["pts"], 50.0);
        // 管理员给自己的充值非法 amount → 400
        let (s, body) = post(
            st.clone(),
            "/api/admin/credits",
            r#"{"user_id":1,"amount":-5}"#,
            Some(&admin_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "负数金额应 400: {body}");
    }

    #[tokio::test]
    async fn admin_users_and_usage_lists() {
        let st = test_state("adminlist");
        let admin_bearer = login_bearer(&st, "admin@aitokenpool.local", "admin1234").await;
        // users：demo + admin 都在列表
        let (s, body) = get(st.clone(), "/api/admin/users", Some(&admin_bearer)).await;
        assert_eq!(s, StatusCode::OK, "users 应 200: {body}");
        let arr: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        assert_eq!(arr.len(), 2, "demo + admin: {body}");
        assert!(arr.iter().any(|u| u["role"] == "admin"), "admin 在列表中");
        assert!(
            arr.iter().any(|u| u["email"] == "demo@aitokenpool.local"),
            "demo 在列表中"
        );
        // usage：每用户本月聚合
        let (s, body) = get(st.clone(), "/api/admin/usage", Some(&admin_bearer)).await;
        assert_eq!(s, StatusCode::OK, "usage 应 200: {body}");
        let arr: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        assert_eq!(arr.len(), 2);
        assert!(
            arr.iter().all(|u| u["month_tokens"] == 0.0),
            "无调用时 tokens 为 0: {body}"
        );
        // 非 admin 访问 users → 403
        let demo_bearer = login_bearer(&st, "demo@aitokenpool.local", "demo1234").await;
        let (s, _) = get(st.clone(), "/api/admin/users", Some(&demo_bearer)).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn wallet_shows_daily_gift_balance() {
        let st = test_state("wallet_gift");
        let demo_bearer = login_bearer(&st, "demo@aitokenpool.local", "demo1234").await;
        // demo 今天注册（seed 默认 created_at=now）→ 10 天窗口内 → 首次访问 wallet 补发 1 点
        let (s, body) = get(st.clone(), "/api/wallet", Some(&demo_bearer)).await;
        assert_eq!(s, StatusCode::OK, "wallet 应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["gift_balance"], 1.0, "当日赠送 1 点: {body}");
        assert_eq!(v["balance"], 12471.0, "永久余额不变: {body}");
        // 重复访问不重复赠送
        let (_, body2) = get(st.clone(), "/api/wallet", Some(&demo_bearer)).await;
        let v2: serde_json::Value = serde_json::from_str(&body2).unwrap();
        assert_eq!(v2["gift_balance"], 1.0, "同天不重复: {body2}");
    }

    #[tokio::test]
    async fn static_hosting_serves_ui_index() {
        // P2-A：GET / → ui/index.html（200 + html）；GET /css/style.css → 200
        let st = test_state("serve");
        let (s, body) = get(st.clone(), "/", None).await;
        assert_eq!(s, StatusCode::OK, "根路径应返回 index.html: {body:?}");
        assert!(
            body.contains("<!DOCTYPE html>") || body.contains("<html"),
            "返回内容应为 HTML: {}",
            &body[..body.len().min(80)]
        );
        let (s, _) = get(st, "/css/style.css", None).await;
        assert_eq!(s, StatusCode::OK, "静态资源应 200");
    }

    #[tokio::test]
    async fn me_returns_user_info_and_role() {
        let st = test_state("me");
        let demo_bearer = login_bearer(&st, "demo@aitokenpool.local", "demo1234").await;
        let (s, body) = get(st.clone(), "/api/me", Some(&demo_bearer)).await;
        assert_eq!(s, StatusCode::OK, "me 应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["email"], "demo@aitokenpool.local");
        assert_eq!(v["role"], "user");
        assert!(v["name"].as_str().is_some(), "含 name: {body}");
        // admin 登录 → role=admin
        let admin_bearer = login_bearer(&st, "admin@aitokenpool.local", "admin1234").await;
        let (s, body) = get(st, "/api/me", Some(&admin_bearer)).await;
        assert_eq!(s, StatusCode::OK, "admin me 应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["role"], "admin");
    }
}
