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
pub mod admin_models;
pub mod api_keys;
pub mod ops;
pub mod org;
pub mod raise;
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

/// POST /api/auth/login：email+password → 200 {api_key} / 401 / 403（未验证邮箱）
pub async fn login(
    State(st): State<AppState>,
    Json(req): Json<LoginReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let user_id =
        dao::verify_user_password(&conn, &req.email, &req.password).map_err(|_| unauthorized())?;
    if !dao::user_verified(&conn, user_id) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "邮箱未验证，请先输入验证码完成验证" })),
        ));
    }
    let api_key = dao::get_or_create_api_key(&conn, user_id).map_err(internal)?;
    Ok(Json(serde_json::json!({
        "api_key": api_key,
        "user_id": user_id,
    })))
}

#[derive(Deserialize)]
pub struct RegisterReq {
    #[serde(default)]
    pub name: String,
    pub email: String,
    pub password: String,
}

/// 生成 6 位数字验证码
fn gen_verification_code() -> String {
    use rand::Rng;
    format!("{:06}", rand::thread_rng().gen_range(0..1_000_000u32))
}

/// 发送验证码（dev 模式打日志；SMTP 模式发信），返回是否 dev 模式 + 验证码
fn send_code(st: &AppState, email: &str) -> Result<(bool, String), ApiErr> {
    let code = gen_verification_code();
    let hash = sha2_hex(&code);
    let expires_at = "+10 minutes".to_string();
    {
        let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
        dao::store_verification_code(&conn, email, &hash, &expires_at).map_err(internal)?;
    }
    // 过期时间用 SQLite 表达式写入（datetime('now', '+10 minutes')）
    {
        let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
        conn.execute(
            "UPDATE email_verifications SET expires_at = datetime('now', '+10 minutes') WHERE email = ?1",
            [email],
        )
        .map_err(internal)?;
    }
    let dev = !st.cfg.mail.configured();
    crate::mail::send_verification_code(&st.cfg.mail, email, &code).map_err(internal)?;
    Ok((dev, code))
}

fn sha2_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    hex::encode(h.finalize())
}

/// POST /api/auth/register：body {name?, email, password}。校验失败 400（邮箱格式 / 密码长度）、
/// email 已注册 409；成功创建未验证用户并发 6 位验证码（10 分钟有效）→ 201
/// {id, email, name, role, verified}，dev 模式（未配置 SMTP）附 dev_code 便于本地测试。
pub async fn register(
    State(st): State<AppState>,
    Json(req): Json<RegisterReq>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiErr> {
    let email = req.email.trim().to_lowercase();
    // 邮箱格式
    let valid = email.contains('@')
        && email.split('@').count() == 2
        && !email.split('@').next().unwrap_or("").is_empty()
        && email.split('@').nth(1).unwrap_or("").contains('.');
    if !valid {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "邮箱格式不正确" })),
        ));
    }
    if req.password.len() < 8 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "密码至少 8 位" })),
        ));
    }
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    if dao::email_taken(&conn, &email) {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "该邮箱已注册" })),
        ));
    }
    let name = if req.name.trim().is_empty() {
        email.split('@').next().unwrap_or("用户").to_string()
    } else {
        req.name.trim().to_string()
    };
    let hash = crate::auth::hash_password(&req.password).map_err(internal)?;
    let user_id = dao::create_unverified_user(&conn, &email, &name, &hash).map_err(internal)?;
    drop(conn);
    let (dev, code) = send_code(&st, &email)?;
    let mut v = serde_json::json!({
        "id": user_id,
        "email": email,
        "name": name,
        "role": "user",
        "verified": false,
    });
    if dev {
        // dev 模式：验证码直接返回响应便于本地测试（生产配置 SMTP 后不返回）
        v["dev_code"] = serde_json::json!(code);
    }
    Ok((StatusCode::CREATED, Json(v)))
}

#[derive(Deserialize)]
pub struct VerifyReq {
    pub email: String,
    pub code: String,
}

#[derive(Deserialize)]
pub struct ResendReq {
    pub email: String,
}

/// POST /api/auth/verify：{email, code} → 校验 6 位验证码 → 激活用户 + 建 quotas → 200
/// 错误码累计 5 次失效；过期 → 400 需重发
pub async fn verify(
    State(st): State<AppState>,
    Json(req): Json<VerifyReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let email = req.email.trim().to_lowercase();
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let Some((hash, _attempts)) = dao::find_valid_verification(&conn, &email) else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "验证码不存在或已过期，请重新获取" })),
        ));
    };
    if sha2_hex(req.code.trim()) != hash {
        if dao::bump_verification_attempt(&conn, &email).map_err(internal)? {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "验证码错误次数过多，请重新获取" })),
            ));
        }
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "验证码错误" })),
        ));
    }
    dao::activate_user(&conn, &email).map_err(internal)?;
    dao::clear_verification(&conn, &email).map_err(internal)?;
    Ok(Json(serde_json::json!({ "status": "ok", "email": email })))
}

/// POST /api/auth/resend-code：{email} → 60 秒限频 → 重新生成并发送验证码
pub async fn resend_code(
    State(st): State<AppState>,
    Json(req): Json<ResendReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let email = req.email.trim().to_lowercase();
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    if dao::resend_too_soon(&conn, &email) {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({ "error": "请求过于频繁，请 60 秒后重试" })),
        ));
    }
    drop(conn);
    let (dev, code) = send_code(&st, &email)?;
    let mut v = serde_json::json!({ "status": "ok", "email": email });
    if dev {
        v["dev_code"] = serde_json::json!(code);
    }
    Ok(Json(v))
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

/// GET /api/config：前端需要的服务端配置（rant 2026-08-19T20:37:37：接入方式 URL 配置化）。
/// 返回 public_url（平台对外网关地址，不含 /v1 等路径），前端据此拼接入端点；
/// 未认证也可访问（public_url 非敏感信息），后续其它前端配置项可复用本端点。
pub async fn config(State(st): State<AppState>) -> Result<Json<serde_json::Value>, ApiErr> {
    Ok(Json(serde_json::json!({
        "public_url": st.cfg.server.public_url,
    })))
}

#[derive(Deserialize)]
pub struct ChangePasswordReq {
    pub old_password: String,
    pub new_password: String,
}

/// POST /api/auth/change-password：旧密码校验 + 新密码 argon2 更新（rant 2026-08-19T14:35:05，
/// 初始管理员登录后改密；Bearer 认证，任意已登录用户可改自己的密码）
pub async fn change_password(
    State(st): State<AppState>,
    auth: AuthUser,
    Json(req): Json<ChangePasswordReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    if req.new_password.len() < 8 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "新密码至少 8 位" })),
        ));
    }
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let hash: String = conn
        .query_row(
            "SELECT password_hash FROM users WHERE id = ?1",
            [auth.user_id],
            |r| r.get(0),
        )
        .map_err(|_| unauthorized())?;
    if !crate::auth::verify_password(&hash, &req.old_password) {
        return Err(unauthorized());
    }
    let new_hash = crate::auth::hash_password(&req.new_password).map_err(internal)?;
    conn.execute(
        "UPDATE users SET password_hash = ?1 WHERE id = ?2",
        rusqlite::params![new_hash, auth.user_id],
    )
    .map_err(internal)?;
    Ok(Json(serde_json::json!({ "status": "ok" })))
}

/// 组装路由：API 路由优先，其余请求回退到 ui/ 静态托管（P2-A）
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/auth/login", post(login))
        .route("/api/auth/change-password", post(change_password))
        .route("/api/auth/register", post(register))
        .route("/api/auth/verify", post(verify))
        .route("/api/auth/resend-code", post(resend_code))
        .route("/api/me", get(me))
        .route("/api/config", get(config))
        .route("/api/api-keys", post(api_keys::create).get(api_keys::list))
        .route("/api/api-keys/:id", axum::routing::delete(api_keys::remove))
        .route("/v1/chat/completions", post(gateway::chat_completions))
        .route("/anthropic/v1/messages", post(gateway::anthropic_messages))
        .route("/v1/responses", post(gateway::responses))
        .route("/v1/models", get(gateway::v1_models))
        .route("/models", get(gateway::v1_models))
        .route("/api/models", get(gateway::models))
        .route("/api/plans", get(gateway::plans))
        // P0-C：共享 / 钱包 / 交易 / 仪表盘
        .route("/api/sharings", post(sharing::create).get(sharing::list))
        .route("/api/sharings/:id", axum::routing::patch(sharing::patch))
        .route("/api/wallet", get(wallet::wallet))
        .route("/api/transactions", get(wallet::transactions))
        .route("/api/dashboard", get(wallet::dashboard))
        // P1：管理员（充值 / 成员列表 / 用量报表）
        .route("/api/admin/credits", post(admin::credits))
        .route("/api/admin/users", get(admin::users))
        .route(
            "/api/admin/users/:id",
            axum::routing::patch(admin::patch_user),
        )
        .route("/api/admin/usage", get(admin::usage))
        // P2-C：部门管理 / 加额审批 / 运营者
        .route("/api/admin/departments", get(org::list).post(org::create))
        .route(
            "/api/admin/departments/:id",
            axum::routing::patch(org::patch).delete(org::remove),
        )
        // rant 2026-08-19T20:40:29：管理员模型信息 CRUD
        .route(
            "/api/admin/models",
            get(admin_models::list).post(admin_models::create),
        )
        .route(
            "/api/admin/models/:id",
            axum::routing::patch(admin_models::patch).delete(admin_models::remove),
        )
        .route("/api/raise-requests", post(raise::create).get(raise::list))
        .route(
            "/api/admin/raise-requests/:id/approve",
            post(raise::approve),
        )
        .route("/api/admin/raise-requests/:id/reject", post(raise::reject))
        .route("/api/ops/runtime", get(ops::runtime))
        .route("/api/ops/credits", post(ops::credits))
        .route("/api/ops/users", get(ops::users))
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
        crate::db::seed_test_users(&conn).expect("seed test users");
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

    async fn patch(
        state: AppState,
        uri: &str,
        body: &str,
        bearer: Option<&str>,
    ) -> (StatusCode, String) {
        let mut b = Request::builder()
            .method("PATCH")
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
        // 属主可见完整 key（rant 2026-08-19T18:06:25：复制 key 需随时可用）
        assert!(
            arr.iter().any(|k| k["full_key"] == new_key),
            "列表含新生成 key 的完整值 full_key"
        );
        assert!(
            arr.iter().all(|k| {
                k["full_key"]
                    .as_str()
                    .map(|fk| fk.starts_with("atk_live_"))
                    .unwrap_or(false)
            }),
            "full_key 均为真实完整 key"
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
        assert!(arr[0]["full_key"].is_string(), "撤销前列表返回属主完整 key");
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

    // --- 管理员模型信息 CRUD（rant 2026-08-19T20:40:29） ---

    #[tokio::test]
    async fn admin_models_crud_full_cycle() {
        let st = test_state("admmodels");
        let admin_bearer = login_bearer(&st, "admin@aitokenpool.local", "admin1234").await;
        let demo_bearer = login_bearer(&st, "demo@aitokenpool.local", "demo1234").await;
        // 非 admin → 403
        let (s, body) = get(st.clone(), "/api/admin/models", Some(&demo_bearer)).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "非 admin 应 403: {body}");
        // 列表：seed 后有数据且含新字段（context_length/vision）
        let (s, body) = get(st.clone(), "/api/admin/models", Some(&admin_bearer)).await;
        assert_eq!(s, StatusCode::OK, "列表应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let arr = v.as_array().expect("数组");
        assert!(arr.len() >= 10, "seed 模型 ≥10: {body}");
        let dp = arr
            .iter()
            .find(|m| m["model"] == "deepseek-v4-pro")
            .expect("deepseek-v4-pro 存在");
        assert_eq!(
            dp["context_length"], 1048576,
            "seed 写入 context_length: {body}"
        );
        assert_eq!(dp["vision"], 0, "无 vision 字段默认 0");
        assert!(dp["id"].as_i64().is_some(), "含 id");
        // 新增
        let (s, body) = post(
            st.clone(),
            "/api/admin/models",
            r#"{"provider":"test","model":"test-model-1","currency":"USD","input_per_m":1.5,"output_per_m":3.0,"context_length":128000,"max_output":16384,"vision":1,"cache_hit_input_per_m":0.1}"#,
            Some(&admin_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "新增应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["model"], "test-model-1");
        assert_eq!(v["vision"], 1);
        assert_eq!(
            v["context_window"], 128000,
            "context_window 与 context_length 对齐"
        );
        let new_id = v["id"].as_i64().unwrap();
        // 重复 (provider, model) → 409
        let (s, body) = post(
            st.clone(),
            "/api/admin/models",
            r#"{"provider":"test","model":"test-model-1"}"#,
            Some(&admin_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::CONFLICT, "重复应 409: {body}");
        // 非法输入 → 400（负数价格 / 空 model / 非法 vision）
        let (s, _) = post(
            st.clone(),
            "/api/admin/models",
            r#"{"provider":"test","model":"m2","input_per_m":-1}"#,
            Some(&admin_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "负数价格应 400");
        let (s, _) = post(
            st.clone(),
            "/api/admin/models",
            r#"{"provider":"test","model":"  "}"#,
            Some(&admin_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "空 model 应 400");
        let (s, _) = post(
            st.clone(),
            "/api/admin/models",
            r#"{"provider":"test","model":"m3","vision":2}"#,
            Some(&admin_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "vision≠0/1 应 400");
        // PATCH 部分更新（改价格 + vision）
        let (s, body) = patch(
            st.clone(),
            &format!("/api/admin/models/{new_id}"),
            r#"{"input_per_m":2.5,"vision":0}"#,
            Some(&admin_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "PATCH 应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["input_per_m"], 2.5);
        assert_eq!(v["vision"], 0);
        assert_eq!(v["model"], "test-model-1", "未改字段保留");
        // PATCH 改为已存在 (provider, model) → 409（须同时提供 provider+model 才会撞唯一）
        let (s, _) = patch(
            st.clone(),
            &format!("/api/admin/models/{new_id}"),
            r#"{"provider":"deepseek","model":"deepseek-v4-pro"}"#,
            Some(&admin_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::CONFLICT, "改名撞唯一应 409");
        // 删除
        let (s, body) = del(
            st.clone(),
            &format!("/api/admin/models/{new_id}"),
            Some(&admin_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "删除应 200: {body}");
        let (s, _) = del(
            st.clone(),
            &format!("/api/admin/models/{new_id}"),
            Some(&admin_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND, "重复删除应 404");
    }

    #[tokio::test]
    async fn market_models_include_new_fields() {
        // GET /api/models 响应补 context_length/max_output/vision/cache_hit_input_per_m
        let st = test_state("mkfields");
        let demo_bearer = login_bearer(&st, "demo@aitokenpool.local", "demo1234").await;
        let (s, body) = get(st.clone(), "/api/models", Some(&demo_bearer)).await;
        assert_eq!(s, StatusCode::OK, "models 应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let arr = v.as_array().expect("数组");
        let dp = arr
            .iter()
            .find(|m| m["model"] == "deepseek-v4-pro")
            .expect("deepseek-v4-pro 存在");
        assert_eq!(dp["context_length"], 1048576);
        assert!(dp["max_output"].as_i64().is_some());
        assert!(dp["vision"].as_i64().is_some());
        assert_eq!(dp["cache_hit_input_per_m"], 0.003625, "seed 写入缓存命中价");
    }

    #[tokio::test]
    async fn admin_users_and_usage_lists() {
        let st = test_state("adminlist");
        let admin_bearer = login_bearer(&st, "admin@aitokenpool.local", "admin1234").await;
        // users：demo + admin + ops 都在列表
        let (s, body) = get(st.clone(), "/api/admin/users", Some(&admin_bearer)).await;
        assert_eq!(s, StatusCode::OK, "users 应 200: {body}");
        let arr: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        assert_eq!(arr.len(), 3, "demo + admin + ops: {body}");
        assert!(arr.iter().any(|u| u["role"] == "admin"), "admin 在列表中");
        assert!(arr.iter().any(|u| u["role"] == "ops"), "ops 在列表中");
        assert!(
            arr.iter().any(|u| u["email"] == "demo@aitokenpool.local"),
            "demo 在列表中"
        );
        // usage：对象 {users, models, departments} 三组聚合
        let (s, body) = get(st.clone(), "/api/admin/usage", Some(&admin_bearer)).await;
        assert_eq!(s, StatusCode::OK, "usage 应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let users = v["users"].as_array().expect("users 为数组");
        assert_eq!(users.len(), 3, "每用户一行: {body}");
        assert!(
            users.iter().all(|u| u["month_tokens"] == 0.0),
            "无调用时 tokens 为 0: {body}"
        );
        assert!(v["models"].as_array().is_some(), "models 组存在");
        assert!(v["departments"].as_array().is_some(), "departments 组存在");
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

    #[tokio::test]
    async fn config_returns_public_url() {
        // rant 2026-08-19T20:37:37：接入方式 URL 配置化——GET /api/config 返回 public_url
        // （config.example.toml 配了示例真实值 → 返回该值；未认证也可访问）
        let st = test_state("cfg");
        let (s, body) = get(st.clone(), "/api/config", None).await;
        assert_eq!(s, StatusCode::OK, "config 应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            v["public_url"], "https://gateway.example.com",
            "返回 example 配置值: {body}"
        );
    }

    #[tokio::test]
    async fn change_password_rotates_and_old_stops_working() {
        // rant 2026-08-19T14:35:05：初始管理员改密端点——旧密码校验 + argon2 更新
        let st = test_state("chpw");
        let bearer = login_bearer(&st, "demo@aitokenpool.local", "demo1234").await;
        // 错误旧密码 → 401
        let (s, body) = post(
            st.clone(),
            "/api/auth/change-password",
            r#"{"old_password":"wrong","new_password":"newpass123"}"#,
            Some(&bearer),
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "旧密码错误应 401: {body}");
        // 过短新密码 → 400
        let (s, _) = post(
            st.clone(),
            "/api/auth/change-password",
            r#"{"old_password":"demo1234","new_password":"short"}"#,
            Some(&bearer),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "新密码不足 8 位应 400");
        // 正确改密 → 200
        let (s, body) = post(
            st.clone(),
            "/api/auth/change-password",
            r#"{"old_password":"demo1234","new_password":"brandnew99"}"#,
            Some(&bearer),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "改密应 200: {body}");
        // 旧密码登录失败、新密码登录成功
        let (s, _) = post(
            st.clone(),
            "/api/auth/login",
            r#"{"email":"demo@aitokenpool.local","password":"demo1234"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "旧密码应失效");
        let (s, body) = post(
            st,
            "/api/auth/login",
            r#"{"email":"demo@aitokenpool.local","password":"brandnew99"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "新密码应可登录: {body}");
    }

    #[tokio::test]
    async fn change_password_requires_bearer() {
        let (s, _) = post(
            test_state("chpw401"),
            "/api/auth/change-password",
            r#"{"old_password":"x","new_password":"yyyyyyyy"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "未认证应 401");
    }

    /* ---- 注册 / 邮箱验证（rant 2026-08-19T14:36:19 方案 B）---- */

    #[tokio::test]
    async fn register_verify_login_full_flow() {
        // 注册（dev 模式：响应带 dev_code）→ 验证 → 登录 → 每日赠送 1 点
        let st = test_state("reg");
        let (s, body) = post(
            st.clone(),
            "/api/auth/register",
            r#"{"name":"新用户","email":"newbie@example.com","password":"pass1234"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::CREATED, "注册应 201: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["email"], "newbie@example.com");
        assert_eq!(v["role"], "user");
        assert_eq!(v["verified"], false);
        let code = v["dev_code"].as_str().expect("dev 模式返回验证码");
        // 未验证登录 → 403
        let (s, body) = post(
            st.clone(),
            "/api/auth/login",
            r#"{"email":"newbie@example.com","password":"pass1234"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN, "未验证登录应 403: {body}");
        // 验证
        let (s, body) = post(
            st.clone(),
            "/api/auth/verify",
            &format!(r#"{{"email":"newbie@example.com","code":"{code}"}}"#),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "验证应 200: {body}");
        // 验证后登录 → 200
        let (s, body) = post(
            st.clone(),
            "/api/auth/login",
            r#"{"email":"newbie@example.com","password":"pass1234"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "验证后登录应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let bearer = v["api_key"].as_str().unwrap().to_string();
        // 钱包：余额 0 + 每日赠送 1 点生效（P1 懒加载）
        let (s, body) = get(st.clone(), "/api/wallet", Some(&bearer)).await;
        assert_eq!(s, StatusCode::OK, "wallet 应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["balance"], 0.0, "永久余额 0: {body}");
        assert_eq!(v["gift_balance"], 1.0, "每日赠送 1 点: {body}");
    }

    #[tokio::test]
    async fn register_duplicate_409() {
        let st = test_state("regdup");
        post(
            st.clone(),
            "/api/auth/register",
            r#"{"email":"dup@example.com","password":"pass1234"}"#,
            None,
        )
        .await;
        let (s, body) = post(
            st,
            "/api/auth/register",
            r#"{"email":"dup@example.com","password":"other123"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::CONFLICT, "重复邮箱应 409: {body}");
    }

    #[tokio::test]
    async fn register_invalid_input_400() {
        let st = test_state("reg400");
        // 非法邮箱
        let (s, _) = post(
            st.clone(),
            "/api/auth/register",
            r#"{"email":"not-an-email","password":"pass1234"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "非法邮箱应 400");
        // 弱密码
        let (s, _) = post(
            st.clone(),
            "/api/auth/register",
            r#"{"email":"ok@example.com","password":"short"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "弱密码应 400");
    }

    #[tokio::test]
    async fn verify_wrong_code_expires_and_attempt_limit() {
        let st = test_state("regverr");
        let (_, body) = post(
            st.clone(),
            "/api/auth/register",
            r#"{"email":"verr@example.com","password":"pass1234"}"#,
            None,
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let code = v["dev_code"].as_str().unwrap();
        // 错误码 ×4 → 400
        for _ in 0..4 {
            let (s, _) = post(
                st.clone(),
                "/api/auth/verify",
                r#"{"email":"verr@example.com","code":"000000"}"#,
                None,
            )
            .await;
            assert_eq!(s, StatusCode::BAD_REQUEST);
        }
        // 第 5 次错误 → 达到上限，记录删除
        let (s, body) = post(
            st.clone(),
            "/api/auth/verify",
            r#"{"email":"verr@example.com","code":"000000"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "第 5 次错误应 400: {body}");
        assert!(body.contains("次数过多"), "提示次数过多: {body}");
        // 重发 → 新码 → 验证成功
        let (s, body) = post(
            st.clone(),
            "/api/auth/resend-code",
            r#"{"email":"verr@example.com"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "重发应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let code2 = v["dev_code"].as_str().unwrap();
        assert_ne!(code, code2, "重发生成新码");
        let (s, _) = post(
            st.clone(),
            "/api/auth/verify",
            &format!(r#"{{"email":"verr@example.com","code":"{code2}"}}"#),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "新码验证应 200");
        // 过期场景：直接改 expires_at 为过去
        let (_, body) = post(
            st.clone(),
            "/api/auth/register",
            r#"{"email":"expired@example.com","password":"pass1234"}"#,
            None,
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let code3 = v["dev_code"].as_str().unwrap();
        {
            let conn = st.db.lock().unwrap();
            conn.execute(
                "UPDATE email_verifications SET expires_at = datetime('now', '-1 minute') WHERE email = 'expired@example.com'",
                [],
            )
            .unwrap();
        }
        let (s, body) = post(
            st,
            "/api/auth/verify",
            &format!(r#"{{"email":"expired@example.com","code":"{code3}"}}"#),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "过期应 400: {body}");
        assert!(body.contains("过期"), "提示过期: {body}");
    }

    #[tokio::test]
    async fn resend_code_rate_limited() {
        let st = test_state("regrl");
        let (_, body) = post(
            st.clone(),
            "/api/auth/register",
            r#"{"email":"rl@example.com","password":"pass1234"}"#,
            None,
        )
        .await;
        assert!(serde_json::from_str::<serde_json::Value>(&body).unwrap()["dev_code"].is_string());
        // 60 秒内重发 → 429
        let (s, body) = post(
            st,
            "/api/auth/resend-code",
            r#"{"email":"rl@example.com"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::TOO_MANY_REQUESTS, "限频应 429: {body}");
    }

    /* ---- P2-C：部门管理 / 加额审批 / 运营者 / 用量三组聚合 ---- */

    #[tokio::test]
    async fn dept_crud_duplicate_and_delete_nonempty() {
        let st = test_state("dept");
        let admin = login_bearer(&st, "admin@aitokenpool.local", "admin1234").await;
        // 建部门
        let (s, body) = post(
            st.clone(),
            "/api/admin/departments",
            r#"{"name":"研发","quota":80000}"#,
            Some(&admin),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "建部门应 200: {body}");
        let dept_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
            .as_i64()
            .unwrap();
        // 重名 → 409
        let (s, body) = post(
            st.clone(),
            "/api/admin/departments",
            r#"{"name":"研发","quota":100}"#,
            Some(&admin),
        )
        .await;
        assert_eq!(s, StatusCode::CONFLICT, "重名应 409: {body}");
        // 列表含新部门
        let (s, body) = get(st.clone(), "/api/admin/departments", Some(&admin)).await;
        assert_eq!(s, StatusCode::OK, "列表应 200: {body}");
        let arr: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], "研发");
        assert_eq!(arr[0]["member_count"], 0);
        // PATCH 改名 + 配额
        let (s, body) = patch(
            st.clone(),
            &format!("/api/admin/departments/{dept_id}"),
            r#"{"name":"研发中心","quota":90000}"#,
            Some(&admin),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "PATCH 应 200: {body}");
        // 分配成员（demo → 部门）
        let (s, body) = patch(
            st.clone(),
            "/api/admin/users/1",
            &format!(r#"{{"dept_id":{dept_id}}}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "成员改部门应 200: {body}");
        // 删除非空部门 → 409
        let (s, body) = del(
            st.clone(),
            &format!("/api/admin/departments/{dept_id}"),
            Some(&admin),
        )
        .await;
        assert_eq!(s, StatusCode::CONFLICT, "非空部门应 409: {body}");
        // 移除成员 → 可删
        let (s, body) = patch(
            st.clone(),
            "/api/admin/users/1",
            r#"{"dept_id":null}"#,
            Some(&admin),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "移除成员应 200: {body}");
        let (s, body) = del(
            st,
            &format!("/api/admin/departments/{dept_id}"),
            Some(&admin),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "空部门应可删: {body}");
    }

    #[tokio::test]
    async fn dept_requires_admin_and_valid_dept() {
        let st = test_state("dept403");
        // 非 admin 建部门 → 403
        let demo = login_bearer(&st, "demo@aitokenpool.local", "demo1234").await;
        let (s, _) = post(
            st.clone(),
            "/api/admin/departments",
            r#"{"name":"研发","quota":100}"#,
            Some(&demo),
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        // admin 给不存在的部门分配成员 → 404
        let admin = login_bearer(&st, "admin@aitokenpool.local", "admin1234").await;
        let (s, _) = patch(st, "/api/admin/users/1", r#"{"dept_id":999}"#, Some(&admin)).await;
        assert_eq!(s, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn raise_request_apply_dup_approve_reject() {
        let st = test_state("raise");
        let demo = login_bearer(&st, "demo@aitokenpool.local", "demo1234").await;
        let admin = login_bearer(&st, "admin@aitokenpool.local", "admin1234").await;
        // demo 申请 500 点
        let (s, body) = post(
            st.clone(),
            "/api/raise-requests",
            r#"{"amount":500,"reason":"任务增加"}"#,
            Some(&demo),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "申请应 200: {body}");
        let req_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
            .as_i64()
            .unwrap();
        // 重复 pending → 409
        let (s, body) = post(
            st.clone(),
            "/api/raise-requests",
            r#"{"amount":100,"reason":"再来"}"#,
            Some(&demo),
        )
        .await;
        assert_eq!(s, StatusCode::CONFLICT, "重复申请应 409: {body}");
        // 非法 amount → 400
        let (s, _) = post(
            st.clone(),
            "/api/raise-requests",
            r#"{"amount":-1,"reason":"x"}"#,
            Some(&demo),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        // 用户看自己的（1 条）
        let (s, body) = get(st.clone(), "/api/raise-requests", Some(&demo)).await;
        assert_eq!(s, StatusCode::OK);
        let arr: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["amount"], 500.0);
        // admin 看全部（1 条，含用户信息）
        let (s, body) = get(st.clone(), "/api/raise-requests", Some(&admin)).await;
        assert_eq!(s, StatusCode::OK);
        let arr: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["email"], "demo@aitokenpool.local");
        // 批准 → balance += 500 + 交易记录
        let (s, body) = post(
            st.clone(),
            &format!("/api/admin/raise-requests/{req_id}/approve"),
            "{}",
            Some(&admin),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "批准应 200: {body}");
        let (_, body) = get(st.clone(), "/api/wallet", Some(&demo)).await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["balance"], 12471.0 + 500.0, "批准后永久余额增加: {body}");
        let (s, body) = get(st.clone(), "/api/transactions?type=topup", Some(&demo)).await;
        assert_eq!(s, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 1);
        assert_eq!(v["items"][0]["counterpart"], "加额审批");
        // 重复批准 → 409
        let (s, _) = post(
            st.clone(),
            &format!("/api/admin/raise-requests/{req_id}/approve"),
            "{}",
            Some(&admin),
        )
        .await;
        assert_eq!(s, StatusCode::CONFLICT);
        // 再申请一条 → 驳回
        let (s, body) = post(
            st.clone(),
            "/api/raise-requests",
            r#"{"amount":50,"reason":"再试"}"#,
            Some(&demo),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        let req2 = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
            .as_i64()
            .unwrap();
        let (s, body) = post(
            st,
            &format!("/api/admin/raise-requests/{req2}/reject"),
            "{}",
            Some(&admin),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "驳回应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["status"], "rejected");
    }

    #[tokio::test]
    async fn raise_requires_admin_review() {
        let st = test_state("raise403");
        let demo = login_bearer(&st, "demo@aitokenpool.local", "demo1234").await;
        let (s, body) = post(
            st.clone(),
            "/api/raise-requests",
            r#"{"amount":100,"reason":"x"}"#,
            Some(&demo),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        let req_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
            .as_i64()
            .unwrap();
        // 非 admin 批准 → 403
        let (s, _) = post(
            st,
            &format!("/api/admin/raise-requests/{req_id}/approve"),
            "{}",
            Some(&demo),
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn ops_runtime_credits_users() {
        let st = test_state("ops");
        let ops_bearer = login_bearer(&st, "ops@aitokenpool.local", "ops1234").await;
        // 普通用户访问 ops → 403
        let demo = login_bearer(&st, "demo@aitokenpool.local", "demo1234").await;
        let (s, _) = get(st.clone(), "/api/ops/runtime", Some(&demo)).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "普通用户应 403");
        // runtime 聚合
        let (s, body) = get(st.clone(), "/api/ops/runtime", Some(&ops_bearer)).await;
        assert_eq!(s, StatusCode::OK, "ops runtime 应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["users"], 3, "demo+admin+ops: {body}");
        assert_eq!(v["month_calls"], 0);
        // users 列表（含余额）
        let (s, body) = get(st.clone(), "/api/ops/users", Some(&ops_bearer)).await;
        assert_eq!(s, StatusCode::OK);
        let arr: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        assert_eq!(arr.len(), 3);
        assert!(arr.iter().any(|u| u["email"] == "demo@aitokenpool.local"));
        // credits 给 demo 充 200
        let (s, body) = post(
            st.clone(),
            "/api/ops/credits",
            r#"{"user_id":1,"amount":200}"#,
            Some(&ops_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "ops 充值应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["balance"], 12471.0 + 200.0);
        // 交易记录 counterpart=运营者（demo 视角）
        let (s, body) = get(st.clone(), "/api/transactions?type=topup", Some(&demo)).await;
        assert_eq!(s, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["items"][0]["counterpart"], "运营者");
        // 负数金额 → 400
        let (s, _) = post(
            st,
            "/api/ops/credits",
            r#"{"user_id":1,"amount":-5}"#,
            Some(&ops_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn usage_three_group_aggregation() {
        let st = test_state("usage3");
        let admin = login_bearer(&st, "admin@aitokenpool.local", "admin1234").await;
        // 造部门 + 分配 demo + 插一条 usage_records
        let (_, body) = post(
            st.clone(),
            "/api/admin/departments",
            r#"{"name":"研发","quota":100000}"#,
            Some(&admin),
        )
        .await;
        let dept_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
            .as_i64()
            .unwrap();
        let (_, _) = patch(
            st.clone(),
            "/api/admin/users/1",
            &format!(r#"{{"dept_id":{dept_id}}}"#),
            Some(&admin),
        )
        .await;
        {
            let conn = st.db.lock().unwrap();
            conn.execute(
                "INSERT INTO usage_records (user_id, model, tokens, cost) VALUES (1, 'gpt-test', 1000, 2.5)",
                [],
            )
            .unwrap();
        }
        let (s, body) = get(st.clone(), "/api/admin/usage", Some(&admin)).await;
        assert_eq!(s, StatusCode::OK, "usage 应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        // users 组
        let demo_u = v["users"]
            .as_array()
            .unwrap()
            .iter()
            .find(|u| u["id"] == 1)
            .unwrap();
        assert_eq!(demo_u["month_tokens"], 1000.0);
        assert_eq!(demo_u["month_calls"], 1);
        assert_eq!(demo_u["dept_name"], "研发");
        // models 组
        let m = v["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["model"] == "gpt-test")
            .unwrap();
        assert_eq!(m["tokens"], 1000.0);
        assert_eq!(m["cost"], 2.5);
        assert_eq!(m["calls"], 1);
        // departments 组
        let d = v["departments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["name"] == "研发")
            .unwrap();
        assert_eq!(d["tokens"], 1000.0);
        assert_eq!(d["cost"], 2.5);
    }
}
