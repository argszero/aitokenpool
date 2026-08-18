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

pub mod api_keys;

use std::sync::{Arc, Mutex};

use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use rusqlite::Connection;
use serde::Deserialize;

use crate::config::Config;
use crate::dao;
use crate::gateway;
use crate::router::RouterState;

/// 共享状态：数据库连接 + 配置 + 路由状态 + HTTP 客户端
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub cfg: Arc<Config>,
    pub router: Arc<RouterState>,
    pub http: reqwest::Client,
}

impl AppState {
    pub fn new(conn: Connection, cfg: Arc<Config>) -> Self {
        Self {
            db: Arc::new(Mutex::new(conn)),
            cfg,
            router: Arc::new(RouterState::new()),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("reqwest client 构建失败"),
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
#[derive(Debug, Clone, Copy)]
pub struct AuthUser {
    pub user_id: i64,
    pub api_key_id: i64,
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
            Some((user_id, api_key_id)) => {
                let _ = dao::touch_api_key(&conn, key);
                Ok(AuthUser {
                    user_id,
                    api_key_id,
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

/// 组装路由
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/auth/login", post(login))
        .route("/api/api-keys", post(api_keys::create).get(api_keys::list))
        .route("/v1/chat/completions", post(gateway::chat_completions))
        .route("/anthropic/v1/messages", post(gateway::anthropic_messages))
        .route("/api/models", get(gateway::models))
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
        AppState::new(conn, Arc::new(cfg))
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
}
