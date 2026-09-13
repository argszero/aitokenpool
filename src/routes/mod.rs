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

/// 上游请求的时限（连接与读取共用同一个数字，沿用 P0-B 写下的 120 s）。
pub(crate) const UPSTREAM_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// 非流式出站客户端：`timeout` 是**总**时限（建连到读完响应体），适合一次性响应。
pub(crate) fn upstream_client(timeout: std::time::Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .expect("reqwest client 构建失败")
}

/// 流式出站客户端：**不设总时限**。
///
/// `ClientBuilder::timeout` 是「整个请求 + 响应体读完」的总时限，用在流式上会把**仍在产出数据**
/// 的长回答一起截断（客户端看到的是 `error decoding response body`，而非超时字样）。
/// 流式要的是另外两个语义：`connect_timeout` 限制建连；`read_timeout` 限制**每一次**读取，
/// 且每次成功读取后重置——只有真正静默超时才算死，有进展的长流不会被打断。
pub(crate) fn upstream_stream_client(timeout: std::time::Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(timeout)
        .read_timeout(timeout)
        .build()
        .expect("reqwest client 构建失败")
}

/// 共享状态：数据库连接 + 配置 + 路由状态 + HTTP 客户端 + 密钥加密器 + 进程启动时刻
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub cfg: Arc<Config>,
    pub router: Arc<RouterState>,
    /// 非流式出站客户端（**总**时限）：`gateway::forward` 使用。
    pub http: reqwest::Client,
    /// 流式出站客户端（无总时限，只有连接 + 逐次读取时限）：`gateway::forward_stream` 使用。
    pub http_stream: reqwest::Client,
    pub crypto: Crypto,
    /// 进程启动时刻（单调时钟）：`/api/ops/runtime` 用它算运行时长。
    /// 取 `Instant` 而非墙上时间——运行时长不怕系统时钟被调整。
    pub started_at: std::time::Instant,
}

impl AppState {
    pub fn new(conn: Connection, cfg: Arc<Config>, crypto: Crypto) -> Self {
        Self {
            db: Arc::new(Mutex::new(conn)),
            cfg,
            router: Arc::new(RouterState::new()),
            http: upstream_client(UPSTREAM_TIMEOUT),
            http_stream: upstream_stream_client(UPSTREAM_TIMEOUT),
            crypto,
            started_at: std::time::Instant::now(),
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
    // 与其他收邮箱的 handler 一致：邮箱归一化为小写再查（users.email 无 COLLATE NOCASE，
    // 注册时已 deflate 为小写；此处不归一化会让大小写变体登录失败）
    let email = req.email.trim().to_lowercase();
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let user_id =
        dao::verify_user_password(&conn, &email, &req.password).map_err(|_| unauthorized())?;
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

/// 生成 6 位数字验证码 + 其 sha256 hex（写入方与发信方共用同一个码）
fn new_verification_code() -> (String, String) {
    let code = gen_verification_code();
    let hash = sha2_hex(&code);
    (code, hash)
}

/// 发送验证码（dev 模式打日志；SMTP 模式发信），返回是否 dev 模式。
/// 约定：**调用方先在自己的锁作用域内写入验证码记录**（`dao::store_verification_code` /
/// `dao::begin_resend_verification`），本函数只负责发出——写入与限频检查必须原子，而发信（含重试）
/// 不能持锁。
fn send_code(st: &AppState, email: &str, code: &str) -> Result<bool, ApiErr> {
    let dev = !st.cfg.mail.configured();
    if let Err(e) = crate::mail::send_verification_code(&st.cfg.mail, email, code) {
        // SMTP 发送失败（重试后仍失败）→ 清除验证码记录（解除 60s 重发限频，用户可立即重发），
        // 返回 502 + 明确错误提示（rant 2026-08-21T23:52:17：半注册账号兜底）
        let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
        let _ = dao::clear_verification(&conn, email);
        drop(conn);
        log::error!("验证码发送失败（{email}，重试后仍失败）: {e:#}");
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": "验证码发送失败，请重试" })),
        ));
    }
    Ok(dev)
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
    // 验证码记录与建号在同一锁作用域内写入：不存在「用户已建、码未写」的中间态
    let (code, code_hash) = new_verification_code();
    dao::store_verification_code(&conn, &email, &code_hash).map_err(internal)?;
    drop(conn);
    let dev = send_code(&st, &email, &code)?;
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

/// POST /api/auth/resend-code：{email} → 邮箱从未注册 → 统一返回 ok（不发送，防枚举，
/// 同 forgot-password）；否则 60 秒限频 → 重新生成并发送验证码
pub async fn resend_code(
    State(st): State<AppState>,
    Json(req): Json<ResendReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let email = req.email.trim().to_lowercase();
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    // 防枚举：邮箱从未注册过就统一返回 ok（不发送、不建验证码记录），与 forgot-password 一致。
    // 只按「用户是否存在」判断，不看 verified：重发按钮本身就是「没收到码」的补救路径，
    // 用户此刻必然还是 verified=0。
    let exists = dao::find_user_by_email(&conn, &email).is_some();
    drop(conn);
    if !exists {
        return Ok(Json(serde_json::json!({ "status": "ok", "email": email })));
    }
    let (code, code_hash) = new_verification_code();
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    // 限频检查与写入必须在同一把锁内（dao::begin_resend_verification）：拆成两段各自加锁时，
    // 并发重发会同时通过检查 ⇒ 同一邮箱一次突发收到多封验证码（已实测）
    let written = dao::begin_resend_verification(&conn, &email, &code_hash).map_err(internal)?;
    drop(conn);
    if !written {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({ "error": "请求过于频繁，请 60 秒后重试" })),
        ));
    }
    let dev = send_code(&st, &email, &code)?;
    let mut v = serde_json::json!({ "status": "ok", "email": email });
    if dev {
        v["dev_code"] = serde_json::json!(code);
    }
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct ForgotPasswordReq {
    pub email: String,
}

/// POST /api/auth/forgot-password：{email} → 若邮箱已注册（含未验证）则发验证码，
/// 用于重置密码。邮箱不存在也返回 ok（防枚举探测）。60 秒限频（复用 resend 限频）。
/// 未验证账号同样可用本接口——重置密码的验证码本身就证明了邮箱所有权（宿主 2026-08-20）。
pub async fn forgot_password(
    State(st): State<AppState>,
    Json(req): Json<ForgotPasswordReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let email = req.email.trim().to_lowercase();
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let exists = dao::find_user_by_email(&conn, &email).is_some();
    drop(conn);
    if !exists {
        // 防枚举：统一返回 ok（不发送）
        return Ok(Json(serde_json::json!({ "status": "ok", "email": email })));
    }
    let (code, code_hash) = new_verification_code();
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    // 限频检查与写入必须在同一把锁内（同 resend-code：否则并发重发同时通过检查）
    let written = dao::begin_resend_verification(&conn, &email, &code_hash).map_err(internal)?;
    drop(conn);
    if !written {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({ "error": "请求过于频繁，请 60 秒后重试" })),
        ));
    }
    let dev = send_code(&st, &email, &code)?;
    let mut v = serde_json::json!({ "status": "ok", "email": email });
    if dev {
        v["dev_code"] = serde_json::json!(code);
    }
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct ResetPasswordReq {
    pub email: String,
    pub code: String,
    pub new_password: String,
}

/// POST /api/auth/reset-password：{email, code, new_password} → 校验邮箱验证码 →
/// 重置密码（argon2）。验证码正确即证明邮箱所有权 → 未验证账号顺带激活（verified=1）。
/// 已注册（无论验证与否）均可通过此流程找回密码（宿主 2026-08-20）。
pub async fn reset_password(
    State(st): State<AppState>,
    Json(req): Json<ResetPasswordReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    if req.new_password.len() < 8 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "新密码至少 8 位" })),
        ));
    }
    let email = req.email.trim().to_lowercase();
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    // 邮箱必须已注册（防任意邮箱开账号）
    if dao::find_user_by_email(&conn, &email).is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "该邮箱未注册" })),
        ));
    }
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
    // 重置密码 + 激活（未验证账号由验证码证明所有权后顺带激活）
    let new_hash = crate::auth::hash_password(&req.new_password).map_err(internal)?;
    conn.execute(
        "UPDATE users SET password_hash = ?1, verified = 1 WHERE email = ?2",
        rusqlite::params![new_hash, email],
    )
    .map_err(internal)?;
    dao::activate_user(&conn, &email).map_err(internal)?; // 确保 quotas 存在
    dao::clear_verification(&conn, &email).map_err(internal)?;
    Ok(Json(serde_json::json!({ "status": "ok", "email": email })))
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
        .route("/api/auth/forgot-password", post(forgot_password))
        .route("/api/auth/reset-password", post(reset_password))
        .route("/api/me", get(me))
        .route("/api/config", get(config))
        .route("/api/api-keys", post(api_keys::create).get(api_keys::list))
        .route(
            "/api/api-keys/:id",
            axum::routing::delete(api_keys::remove).patch(api_keys::rename),
        )
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
        .route("/api/transactions/trend", get(wallet::transactions_trend))
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
        // POST 生成（rant 2026-08-22T17:21:39：create 须接收 {name} 并持久化）
        let (s, body) = post(
            st.clone(),
            "/api/api-keys",
            r#"{"name":"我的测试Key"}"#,
            Some(&bearer),
        )
        .await;
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
            arr.iter().any(|k| k["name"] == "我的测试Key"),
            "创建时传入的 name 已持久化入列表"
        );
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
        // rant 2026-08-24T12:41:25：列表返回 last_used（新 key 未使用 → null；用过 → 时间串）
        assert!(
            arr.iter().all(|k| k["last_used"].is_null()
                || k["last_used"]
                    .as_str()
                    .map(|s| !s.is_empty())
                    .unwrap_or(false)),
            "last_used 字段存在且为 null（未用）或时间串（已用）"
        );
        assert!(
            arr.iter()
                .any(|k| k["full_key"] == new_key && k["last_used"].is_null()),
            "新生成且未调用的 key last_used 应为 null"
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

    #[tokio::test]
    async fn api_key_rename_owner_only() {
        let st = test_state("keyrename");
        let demo_bearer = login_bearer(&st, "demo@aitokenpool.local", "demo1234").await;
        // 生成带名字的 key → 拿到 id
        let (_, body) = post(
            st.clone(),
            "/api/api-keys",
            r#"{"name":"原名"}"#,
            Some(&demo_bearer),
        )
        .await;
        assert!(serde_json::from_str::<serde_json::Value>(&body).unwrap()["api_key"].is_string());
        let (_, body) = get(st.clone(), "/api/api-keys", Some(&demo_bearer)).await;
        let arr: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        let new_id = arr[0]["id"].as_i64().expect("有 id");
        assert_eq!(arr[0]["name"], "原名", "创建名字已入库");
        // 属主改名 → 200，列表持久化
        let (s, body) = patch(
            st.clone(),
            &format!("/api/api-keys/{new_id}"),
            r#"{"name":"新名"}"#,
            Some(&demo_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "属主改名应 200: {body}");
        let (_, body) = get(st.clone(), "/api/api-keys", Some(&demo_bearer)).await;
        let arr: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        assert_eq!(arr[0]["name"], "新名", "改名持久化，刷新后仍为新名");
        // 非属主改名 → 404（admin 改 demo 的 key）
        let admin_bearer = login_bearer(&st, "admin@aitokenpool.local", "admin1234").await;
        let (s, _) = patch(
            st.clone(),
            &format!("/api/api-keys/{new_id}"),
            r#"{"name":"越权"}"#,
            Some(&admin_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND, "非属主改名应 404");
        // 撤销后再改名 → 404
        let (_, body) = del(
            st.clone(),
            &format!("/api/api-keys/{new_id}"),
            Some(&demo_bearer),
        )
        .await;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap()["status"],
            "revoked"
        );
        let (s, _) = patch(
            st,
            &format!("/api/api-keys/{new_id}"),
            r#"{"name":"复活"}"#,
            Some(&demo_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND, "已撤销 key 改名应 404");
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
    async fn admin_models_rejects_negative_cache_hit_price() {
        // 负数缓存命中价会一路无钳制进入结算（pts 变负 ⇒ 消费者被充值、分享者被倒扣），
        // 故必须与其余 5 个价格字段一样在入口被拒。修复前 CREATE/PATCH 均返回 200 并落库。
        let st = test_state("admnegcache");
        let admin_bearer = login_bearer(&st, "admin@aitokenpool.local", "admin1234").await;

        // CREATE：负数 cache_hit_input_per_m → 400
        let (s, body) = post(
            st.clone(),
            "/api/admin/models",
            r#"{"provider":"neg","model":"neg-cache","currency":"USD","input_per_m":1.0,"output_per_m":2.0,"cache_hit_input_per_m":-3.0}"#,
            Some(&admin_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "负数缓存命中价应 400: {body}");

        // 阳性对照：0 是合法值（= 命中部分免费），必须放行
        let (s, body) = post(
            st.clone(),
            "/api/admin/models",
            r#"{"provider":"neg","model":"zero-cache","currency":"USD","input_per_m":1.0,"output_per_m":2.0,"cache_hit_input_per_m":0.0}"#,
            Some(&admin_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "0 缓存命中价应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let new_id = v["id"].as_i64().unwrap();

        // PATCH：改成负数 → 400，且库中该值不被写入
        let (s, body) = patch(
            st.clone(),
            &format!("/api/admin/models/{new_id}"),
            r#"{"cache_hit_input_per_m":-9.0}"#,
            Some(&admin_bearer),
        )
        .await;
        assert_eq!(
            s,
            StatusCode::BAD_REQUEST,
            "PATCH 负数缓存命中价应 400: {body}"
        );
        let (_, body) = get(st.clone(), "/api/admin/models", Some(&admin_bearer)).await;
        let arr: serde_json::Value = serde_json::from_str(&body).unwrap();
        let row = arr
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == new_id)
            .expect("模型仍在");
        assert_eq!(
            row["cache_hit_input_per_m"], 0.0,
            "被拒的 PATCH 不得改库: {body}"
        );

        // 兄弟字段（高峰变体）此前已被校验，须保持 400
        let (s, _) = post(
            st.clone(),
            "/api/admin/models",
            r#"{"provider":"neg","model":"neg-peak","currency":"USD","input_per_m":1.0,"peak_cache_hit_input_per_m":-3.0}"#,
            Some(&admin_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "高峰缓存命中价负数仍应 400");
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
        assert_eq!(
            dp["cache_hit_input_per_m"], 0.15,
            "seed 写入缓存命中价（DeepSeek 官方 CNY）"
        );
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

    /// 登录必须对邮箱做大小写归一化（与 register/verify/resend/forgot/reset 五个 handler 一致）：
    /// 注册会把邮箱 deflate 为小写存库，`users.email` 无 COLLATE NOCASE，故 login 若原样比较，
    /// 大小写变体（如移动端键盘首字母自动大写）会被判为「用户不存在」→ 401。
    #[tokio::test]
    async fn login_normalizes_email_case() {
        let st = test_state("logincase");
        // 用混合大小写注册；响应里的 email 即为落库的规范形式
        let (s, body) = post(
            st.clone(),
            "/api/auth/register",
            r#"{"name":"大小写","email":"MixedCase@Example.com","password":"pass1234"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::CREATED, "注册应 201: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            v["email"], "mixedcase@example.com",
            "注册后应落库为小写规范形式: {body}"
        );
        let code = v["dev_code"].as_str().expect("dev 模式返回验证码");
        let (s, body) = post(
            st.clone(),
            "/api/auth/verify",
            &format!(r#"{{"email":"MixedCase@Example.com","code":"{code}"}}"#),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "验证应 200: {body}");
        // 阳性对照：规范小写登录必须成功（保证下面的断言不是「什么都放行」）
        let (s, body) = post(
            st.clone(),
            "/api/auth/login",
            r#"{"email":"mixedcase@example.com","password":"pass1234"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "小写邮箱登录应 200: {body}");
        // 待测性质：大小写变体登录必须同样成功
        let (s, body) = post(
            st.clone(),
            "/api/auth/login",
            r#"{"email":"MixedCase@Example.com","password":"pass1234"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "大小写变体登录应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(
            v["api_key"].as_str().unwrap().starts_with("atk_live_"),
            "应返回 api_key: {body}"
        );
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
    async fn register_smtp_failure_502() {
        // SMTP 指向不可达主机 → 发送失败（重试后仍失败）→ 注册返回 502 + 明确错误，
        // 且验证码记录被清除（解除 60s 限频，用户可立即重发）——rant 2026-08-21T23:52:17
        let p =
            std::env::temp_dir().join(format!("atp_route_{}_{}.db", std::process::id(), "reg502"));
        let _ = std::fs::remove_file(&p);
        let conn = crate::db::open(p.to_str().unwrap()).expect("open tmp db");
        crate::db::seed_test_users(&conn).expect("seed test users");
        let mut cfg = crate::config::Config::load("config/config.example.toml").unwrap();
        cfg.mail.smtp_host = "127.0.0.1".to_string();
        cfg.mail.smtp_port = 1; // 不可达端口：连接立即失败
        cfg.mail.from = "noreply@test.local".to_string();
        crate::db::seed_models(&conn, &cfg).expect("seed models");
        let crypto = crate::crypto::Crypto::new([9u8; 32]);
        let st = AppState::new(conn, Arc::new(cfg), crypto);
        let (s, body) = post(
            st.clone(),
            "/api/auth/register",
            r#"{"email":"smtp502@test.local","password":"password123"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_GATEWAY, "SMTP 发送失败应 502: {body}");
        assert!(
            body.contains("验证码发送失败"),
            "错误信息应明确可操作: {body}"
        );
        // 验证码记录应已被清除 → 同邮箱立即 resend 不应被 60s 限频卡住（429）而是走到发送（502）
        let (s2, _) = post(
            st,
            "/api/auth/resend-code",
            r#"{"email":"smtp502@test.local"}"#,
            None,
        )
        .await;
        assert_ne!(
            s2,
            StatusCode::TOO_MANY_REQUESTS,
            "发送失败后重发不应被限频卡住（验证码记录已清除）"
        );
    }

    #[tokio::test]
    async fn forgot_reset_password_full_flow() {
        // 宿主 2026-08-20：已注册（含未验证）账号应可走「邮箱验证码 → 重置密码」，
        // 未验证账号重置后顺带激活。
        let st = test_state("fr");
        // 邮箱不存在 → 返回 ok（防枚举），无 dev_code
        let (s, body) = post(
            st.clone(),
            "/api/auth/forgot-password",
            r#"{"email":"ghost@example.com"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "不存在邮箱也应 200: {body}");
        let v2: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v2.get("dev_code").is_none(), "不存在邮箱不应发码");
        // 注册一个不验证的账号
        let (s, body) = post(
            st.clone(),
            "/api/auth/register",
            r#"{"name":"重置用户","email":"reset@example.com","password":"oldpass123"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::CREATED, "注册应 201: {body}");
        // 注册刚发过码 → forgot 触发 60 秒限频（合理行为）
        let (s, _) = post(
            st.clone(),
            "/api/auth/forgot-password",
            r#"{"email":"reset@example.com"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::TOO_MANY_REQUESTS, "60 秒内应限频 429");
        // 清掉限频记录（直接操作 DB，测试辅助）→ 再 forgot 发码
        {
            let conn = st.db.lock().unwrap();
            conn.execute(
                "DELETE FROM email_verifications WHERE email = 'reset@example.com'",
                [],
            )
            .unwrap();
        }
        let (s, body) = post(
            st.clone(),
            "/api/auth/forgot-password",
            r#"{"email":"reset@example.com"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "forgot 应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let code = v["dev_code"].as_str().expect("dev 模式返回验证码");
        // 错误验证码 → 400
        let (s, _) = post(
            st.clone(),
            "/api/auth/reset-password",
            r#"{"email":"reset@example.com","code":"000000","new_password":"newpass456"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "错验证码应 400");
        // 正确验证码重置 → 200
        let (s, body) = post(
            st.clone(),
            "/api/auth/reset-password",
            &format!(
                r#"{{"email":"reset@example.com","code":"{code}","new_password":"newpass456"}}"#
            ),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "重置应 200: {body}");
        // 新密码可登录（未验证账号顺带激活 → 不再 403）
        let (s, body) = post(
            st.clone(),
            "/api/auth/login",
            r#"{"email":"reset@example.com","password":"newpass456"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "重置后新密码登录应 200: {body}");
        // 旧密码失效
        let (s, _) = post(
            st.clone(),
            "/api/auth/login",
            r#"{"email":"reset@example.com","password":"oldpass123"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "旧密码应 401");
        // 新密码过短 → 400
        let (s, _) = post(
            st.clone(),
            "/api/auth/reset-password",
            r#"{"email":"reset@example.com","code":"123456","new_password":"short"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "短密码应 400");
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

    #[tokio::test]
    async fn resend_code_unknown_email_not_sent() {
        // 防枚举：从未注册过的邮箱 → 统一返回 ok（不发送、不建验证码记录），与 forgot-password 一致。
        // 未修前该分支会走到 send_code（dev 模式回带 dev_code）＝把任意地址变成免费发信原语。
        let st = test_state("regunknown");
        let (s, body) = post(
            st.clone(),
            "/api/auth/resend-code",
            r#"{"email":"ghost@example.com"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "未注册邮箱也应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(
            v.get("dev_code").is_none(),
            "未注册邮箱不应发码（更不能建记录）: {body}"
        );
        // 未建记录 ⇒ 立即再发一次仍是 200，而不是 429（限频只应作用于真实重发）
        let (s2, body2) = post(
            st.clone(),
            "/api/auth/resend-code",
            r#"{"email":"ghost@example.com"}"#,
            None,
        )
        .await;
        assert_eq!(
            s2,
            StatusCode::OK,
            "未注册邮箱不应进入限频（无记录）: {body2}"
        );
        // 阳性对照：真实账号重发必须仍然发码（否则测试可被「永远返回 ok」蒙混通过）
        let (s, body) = post(
            st.clone(),
            "/api/auth/register",
            r#"{"email":"known@example.com","password":"pass1234"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::CREATED, "注册应 201: {body}");
        {
            let conn = st.db.lock().unwrap();
            conn.execute(
                "DELETE FROM email_verifications WHERE email = 'known@example.com'",
                [],
            )
            .unwrap();
        }
        let (s, body) = post(
            st,
            "/api/auth/resend-code",
            r#"{"email":"known@example.com"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "真实账号重发应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(
            v["dev_code"].is_string(),
            "真实账号重发必须发码（阳性对照）: {body}"
        );
    }

    /// 写入方与读取方必须是同一口径：`store_verification_code` 写入的 `expires_at` 必须能被读取谓词
    /// （`expires_at > datetime('now')`）解释。旧写法把字面量 `'+10 minutes'` 当**参数**存进去，再靠
    /// 紧随其后的一次 UPDATE 救回——两次加锁之间存在「记录已存在却判为过期」的中间态，且第二次写失败
    /// 即永久不可用（用户拿到码却验不过，重发又撞 60 秒限频）。
    #[tokio::test]
    async fn verification_code_is_usable_exactly_as_written() {
        let st = test_state("verok");
        let code = gen_verification_code();
        let hash = sha2_hex(&code);
        let conn = st.db.lock().unwrap();
        dao::store_verification_code(&conn, "written@example.com", &hash).unwrap();
        // 待测性质：写入后**不需要任何第二次写入**就立即可读（写入值落在读取谓词之内）
        let (stored, _attempts) = dao::find_valid_verification(&conn, "written@example.com")
            .expect("写入后必须立即可读——写入值必须在读取谓词之内");
        assert_eq!(stored, hash, "取回的应是刚写入的码哈希");
        let expires: String = conn
            .query_row(
                "SELECT expires_at FROM email_verifications WHERE email = ?1",
                ["written@example.com"],
                |r| r.get(0),
            )
            .unwrap();
        assert_ne!(
            expires, "+10 minutes",
            "落库的必须是 SQL 计算的 datetime，而不是被当作参数存进来的字面量: {expires}"
        );
    }

    /// 限频检查与写入必须原子（同一把锁内）。旧写法各自加锁、两段之间 `drop(conn)`：并发重发会
    /// 同时通过检查 ⇒ 同一邮箱一次突发收到多封验证码（修复前实测：32 并发 2~4 封；把既有的无锁间隙
    /// 放大到 300ms 后 32/32 全部放行）。
    #[tokio::test]
    async fn resend_limiter_and_write_share_one_critical_section() {
        let st = test_state("resatomic");
        let first = {
            let conn = st.db.lock().unwrap();
            dao::begin_resend_verification(&conn, "burst@example.com", &sha2_hex("111111")).unwrap()
        };
        assert!(first, "首次写入应放行");
        let second = {
            let conn = st.db.lock().unwrap();
            dao::begin_resend_verification(&conn, "burst@example.com", &sha2_hex("222222")).unwrap()
        };
        assert!(
            !second,
            "60 秒内第二次必须被限频拦下（检查与写入同一临界区）"
        );
        let conn = st.db.lock().unwrap();
        let (hash, _attempts) = dao::find_valid_verification(&conn, "burst@example.com").unwrap();
        assert_eq!(
            hash,
            sha2_hex("111111"),
            "被限频拦下的那次不得写入新码（否则并发重发仍会各自换码）"
        );
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
        // rant 2026-09-11T16:23:43（PR6）：今日按小时必须 0-23 全量补零（缺小时会让前端柱状图左移）
        let hours = v["today_hours"].as_array().expect("today_hours 数组");
        assert_eq!(hours.len(), 24, "24 个小时桶: {body}");
        assert_eq!(hours[0]["hour"], 0);
        assert_eq!(hours[23]["hour"], 23);
        assert!(
            hours.iter().all(|h| h["calls"] == 0),
            "无调用时全为 0: {body}"
        );
        // 上游 key 健康：按厂商聚合（测试库仅 seed 的 deepseek 一条，status='on'）
        let kh = v["key_health"].as_array().expect("key_health 数组");
        assert!(!kh.is_empty(), "至少有 seed 的上游 key: {body}");
        assert!(kh.iter().all(|k| k["off"] == 0), "seed key 均为 on: {body}");
        // 服务版本 / 运行时长（原型「服务版本」「运行时长」两张卡）
        assert_eq!(
            v["version"],
            env!("CARGO_PKG_VERSION"),
            "version 应与 /healthz 同源: {body}"
        );
        let secs = v["uptime_secs"].as_u64().expect("uptime_secs: {body}");
        let d = v["uptime_days"].as_u64().expect("uptime_days");
        let h = v["uptime_hours"].as_u64().expect("uptime_hours");
        let m = v["uptime_minutes"].as_u64().expect("uptime_minutes");
        let s = v["uptime_secs_rest"].as_u64().expect("uptime_secs_rest");
        assert_eq!(
            d * 86400 + h * 3600 + m * 60 + s,
            secs,
            "四位分解应能拼回 uptime_secs: {body}"
        );
        assert!(h < 24 && m < 60 && s < 60, "各位未归一（进位漏了）: {body}");
        assert!(
            secs < 60,
            "AppState 刚构造，运行时长应 < 60s，实得 {secs}s —— 取错了起点？ {body}"
        );
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

    /// `total_txs` —— 运营概览「交易量」卡的数据源。
    ///
    /// 这个字段自 `85982e8`（PR #80）起就在算、就在返回（`ops.rs:101`/`:175`），
    /// 但**两侧都没有任何断言**：v1.22 的零 mock 重构（`89963f3`）删掉 mock 分支的
    /// 那张卡后，前端漏了重接，而这个字段照旧返回，于是谁都发现不了。
    /// 本测试把「返回了」与「口径是全库 / 累计全部类型」同时钉住。
    #[tokio::test]
    async fn ops_runtime_total_txs_is_global_and_all_types() {
        let st = test_state("opstxs");
        let ops_bearer = login_bearer(&st, "ops@aitokenpool.local", "ops1234").await;
        // 空库 → 0（先证有值，再证口径，避免「恒为 0 也算过」）
        let (s, body) = get(st.clone(), "/api/ops/runtime", Some(&ops_bearer)).await;
        assert_eq!(s, StatusCode::OK, "ops runtime 应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["total_txs"], 0, "空库应为 0: {body}");

        // 造 3 条、**跨两个用户**：admin 的 topup（走真实充值接口）+ demo 的 consume / earn。
        // 跨用户是刻意的：这样「全局 3」既不同于 demo 自己的 2，也不同于 consume-only 的 1，
        // 两种退化的口径都能被这条断言抓住。
        let uid = |email: &str| -> i64 {
            st.db
                .lock()
                .unwrap()
                .query_row("SELECT id FROM users WHERE email = ?1", [email], |r| {
                    r.get(0)
                })
                .unwrap()
        };
        let demo_id = uid("demo@aitokenpool.local");
        let admin_id = uid("admin@aitokenpool.local");
        let (s2, body2) = post(
            st.clone(),
            "/api/ops/credits",
            &format!(r#"{{"user_id":{admin_id},"amount":77}}"#),
            Some(&ops_bearer),
        )
        .await;
        assert_eq!(s2, StatusCode::OK, "充值应 200: {body2}");
        {
            let conn = st.db.lock().unwrap();
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status) \
                 VALUES (?1, 'x', 1, 'm', 10, -2.0, 'consume', '成功')",
                [demo_id],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status) \
                 VALUES (?1, 'x', 1, 'm', 10, 1.0, 'earn', '成功')",
                [demo_id],
            )
            .unwrap();
        }
        let (_, body) = get(st.clone(), "/api/ops/runtime", Some(&ops_bearer)).await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            v["total_txs"], 3,
            "全库 3 条（admin 的 topup + demo 的 consume/earn）——'累计全部类型': {body}"
        );
        // 反面一：按 type 过滤 → 只有 1 条
        let consume_only: i64 = st
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM transactions WHERE type = 'consume'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(consume_only, 1, "前置条件：consume 仅 1 条");
        assert_ne!(
            v["total_txs"].as_i64().unwrap(),
            consume_only,
            "total_txs 不能退化成按 type 过滤: {body}"
        );
        // 反面二：按用户过滤 → 最多 2 条（demo）
        let max_per_user: i64 = st
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT COALESCE(MAX(c), 0) FROM (SELECT COUNT(*) AS c FROM transactions GROUP BY user_id)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(max_per_user, 2, "前置条件：单用户最多 2 条");
        assert_ne!(
            v["total_txs"].as_i64().unwrap(),
            max_per_user,
            "total_txs 不能退化成按用户过滤: {body}"
        );
    }

    /// `month_in` / `month_out` —— 运营概览「点数流入 / 点数流出」的口径必须按 `type`，**不能按符号**。
    ///
    /// 账本是 **type 编码** 的：每个 writer 都存正数 `pts`（`consume` 存 `+pts`，不是 `-pts`），
    /// 方向只在 `type` 列里。旧实现按符号聚合（`WHERE pts > 0` / `pts < 0`）⇒「流出」恒为 0、
    /// 消费被算进「流入」。本测试用**真实写入路径**（`POST /api/ops/credits` + `billing::settle`）
    /// 造 `topup 100` / `consume 7` / `earn round5(7*0.9)`，断言 `in = 100 + earn`、`out = 7`。
    #[tokio::test]
    async fn ops_runtime_month_flow_is_type_based() {
        let st = test_state("opsflow");
        let ops_bearer = login_bearer(&st, "ops@aitokenpool.local", "ops1234").await;
        let uid = |email: &str| -> i64 {
            st.db
                .lock()
                .unwrap()
                .query_row("SELECT id FROM users WHERE email = ?1", [email], |r| {
                    r.get(0)
                })
                .unwrap()
        };
        let demo = uid("demo@aitokenpool.local");
        let admin = uid("admin@aitokenpool.local");

        // 真实充值路径：一行 topup 100
        let (s, body) = post(
            st.clone(),
            "/api/ops/credits",
            &format!(r#"{{"user_id":{demo},"amount":100}}"#),
            Some(&ops_bearer),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "充值应 200: {body}");

        // 真实结算路径：consume `pts` / earn `round5(pts*0.9)`
        let pts = 7.0;
        {
            let mut conn = st.db.lock().unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO quotas (user_id, balance) VALUES (?1, 1000)",
                [demo],
            )
            .unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO quotas (user_id, balance) VALUES (?1, 0)",
                [admin],
            )
            .unwrap();
            let p = crate::billing::SettleParams {
                consumer_id: demo,
                api_key_id: None,
                key_id: 1,
                owner_id: admin,
                model: "test-model".into(),
                tokens: 1000.0,
                cached_tokens: 0.0,
                output_tokens: 0.0,
                pts,
                cost: 0.0,
            };
            crate::billing::settle(&mut conn, &p).unwrap();
        }

        // 期望值从夹具 + 生产常量推导，不写死
        let earn = crate::billing::round5(pts * crate::billing::SHARE_RATIO);
        let want_in = 100.0 + earn; // topup + earn
        let want_out = pts; // consume

        let (s, body) = get(st.clone(), "/api/ops/runtime", Some(&ops_bearer)).await;
        assert_eq!(s, StatusCode::OK, "{body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(
            (v["month_in"].as_f64().unwrap() - want_in).abs() < 1e-9,
            "流入应只算 earn/topup/gift（{want_in}），不得把 consume 也算进来: {body}"
        );
        assert!(
            (v["month_out"].as_f64().unwrap() - want_out).abs() < 1e-9,
            "流出应算 consume（{want_out}），不得恒为 0: {body}"
        );

        // 阳性对照：一个只有 consume 的月份 ⇒ 流入必须为 0（证明「流入」不是「全部求和」）
        let st2 = test_state("opsflow2");
        let ops2 = login_bearer(&st2, "ops@aitokenpool.local", "ops1234").await;
        {
            let conn = st2.db.lock().unwrap();
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status) \
                 VALUES (1, 'x', 1, 'm', 10, ?1, 'consume', '成功')",
                [pts],
            )
            .unwrap();
        }
        let (_, body) = get(st2.clone(), "/api/ops/runtime", Some(&ops2)).await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            v["month_in"].as_f64().unwrap(),
            0.0,
            "只有消费的月份，流入应为 0: {body}"
        );
        assert!(
            (v["month_out"].as_f64().unwrap() - pts).abs() < 1e-9,
            "只有消费的月份，流出应为 {pts}: {body}"
        );
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
