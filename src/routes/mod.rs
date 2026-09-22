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

use axum::extract::{DefaultBodyLimit, FromRequestParts, State};
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

/// 口令下限：**按字符计**。`String::len()` 是 UTF-8 字节数，用它会把下限悄悄放宽到
/// 2–3 个 CJK 字符（`密码abc` = 5 字符 / 9 字节），与产品宣告的「至少 8 位」不符。
/// 唯一真源：`ui/index.html` 的注册占位符、`ui/js/i18n.js` 的 `err.weakPassword` 与 ERR_MAP
/// 字面量都写「位」；客户端 `ui/js/app.js` 用 `Array.from(...).length` 同口径。
pub(crate) const MIN_PASSWORD_CHARS: usize = 8;

/// 口令是否短于下限（字符口径）。三处请求校验必须走它。
pub(crate) fn password_too_short(pw: &str) -> bool {
    pw.chars().count() < MIN_PASSWORD_CHARS
}

/// 网关请求体的上限（rant 2026-09-18T09:14:18；2026-09-21 由 8 MiB 抬到 277 MiB）。
///
/// 这个数是**两层**的唯一来源，两层必须相等：
///
/// 1. 三条网关路由各自的 `DefaultBodyLimit::max(GATEWAY_BODY_LIMIT)` —— axum 提取器
///    真正读取的那一层。`String` / `Json` 等的 2 MiB 上限来自 `axum-core` 的
///    `DEFAULT_LIMIT`，只认 `DefaultBodyLimitKind` 扩展，因此**只有 axum 自己的
///    `DefaultBodyLimit` 抬得动它**（这就是原始缺陷：tower-http 那层不写该扩展）。
/// 2. `router()` 末尾那层全局 `RequestBodyLimitLayer::new(GATEWAY_BODY_LIMIT)` ——
///    外层粗闸，`Content-Length` 超限时**不读体直接 413**（body 是纯文本
///    `length limit exceeded`，不带提取器那句前缀）。
///
/// ⚠️ **外层更小的话，实际生效的就是外层**：它抬不动提取器，但会**抢答**。2026-09-21
/// 把本常量从 8 MiB 抬到 277 MiB 时，外层还停在 `70 * 1024 * 1024`，实测 71 MiB 的体
/// 在外层就被 413 掉 ⇒ 抬了个寂寞。故外层也改成引用本常量，由 `body_limit_gate.rs`
/// 钉住「全树只有一个请求体上限的数」。
///
/// 为什么**不**全局放宽提取器：认证 / 注册 / 找回密码等未认证端点若也放宽，等于给匿名
/// 请求一个同等大小的内存放大面；只有网关三条路由需要大请求体（长上下文 / 多模态）。
///
/// ⚠️ **内存代价**：提取器把请求体整个缓冲进内存（`String` / `Json`），转发前还会
/// `body.clone()` 一次 ⇒ 单请求峰值约为本值的 2~3 倍。**改这个数必须连同宿主的可用内存
/// 一起看**（prod 主机 1.8 GiB、无 swap、应用容器未设 `mem_limit`）。
pub(crate) const GATEWAY_BODY_LIMIT: usize = 277 * 1024 * 1024;

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
    // 口令哈希只在锁内**取出**，argon2 校验放到锁外：KDF 故意昂贵（默认参数实测 ~0.24 s），
    // 放在共享 DB 互斥量里运行时，一次未认证登录就会让全进程所有 DB 路径排队等这么久
    // （C2089 实测：`max_lock_wait ≈ 236 ms`；移到锁外后同一读数为 `0.00 ms`）。
    // ⚠️ 「锁外」不等于「不占 worker」：KDF 是阻塞 CPU，必须在 blocking pool 上跑，
    // 否则并发登录会把 async worker 占满、连 /healthz 都排队（C2105）。
    let found = {
        let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
        dao::find_user_by_email(&conn, &email)
    };
    let Some((user_id, hash)) = found else {
        return Err(unauthorized());
    };
    if !crate::auth::verify_password_async(hash, req.password.clone()).await {
        return Err(unauthorized());
    }
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
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
/// 不能持锁；同理它也**不能占住 async worker**：`crate::mail::send_verification_code` 是阻塞 I/O，
/// 失败时 3 次尝试 + 2×2 s 重试间隔（SMTP 静默丢弃时每次尝试还有 15 s 超时），在 worker 线程上原地
/// 调用会让整个运行时停止服务（单 worker 运行时实测被卡 4.24 s / 最坏 49 s）⇒ 放进 blocking pool。
async fn send_code(st: &AppState, email: &str, code: &str) -> Result<bool, ApiErr> {
    let dev = !st.cfg.mail.configured();
    let cfg = st.cfg.mail.clone();
    let to = email.to_string();
    let code_owned = code.to_string();
    let sent = tokio::task::spawn_blocking(move || {
        crate::mail::send_verification_code(&cfg, &to, &code_owned)
    })
    .await
    .map_err(|e| internal(format!("发信任务失败: {e}")))?;
    if let Err(e) = sent {
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
    if password_too_short(&req.password) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "密码至少 8 位" })),
        ));
    }
    // 已注册邮箱快速失败：只读、且能在跑昂贵的 KDF 之前就返回
    {
        let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
        if dao::email_taken(&conn, &email) {
            return Err((
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": "该邮箱已注册" })),
            ));
        }
    }
    let name = if req.name.trim().is_empty() {
        email.split('@').next().unwrap_or("用户").to_string()
    } else {
        req.name.trim().to_string()
    };
    // argon2 在锁外**且**在 blocking pool 上（默认参数实测 ~0.24 s；理由同 login —— 既不能占着
    // 共享 DB 互斥量算哈希，也不能占着 async worker）
    let hash = crate::auth::hash_password_async(req.password.clone())
        .await
        .map_err(internal)?;
    let (code, code_hash) = new_verification_code();
    // 锁作用域限定在块内：KDF 之后要 await 发信，锁必须在此之前释放（不能把连接守卫带过 await）
    let user_id = {
        let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
        // 复查：KDF 期间锁已释放，同一邮箱可能已被并发注册（users.email 有 UNIQUE 约束，
        // 但这里是「已注册」这一语义，应当给出与首次检查一致的 409，而不是让 UNIQUE 变成 500）
        if dao::email_taken(&conn, &email) {
            return Err((
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": "该邮箱已注册" })),
            ));
        }
        let user_id = dao::create_unverified_user(&conn, &email, &name, &hash).map_err(internal)?;
        // 验证码记录与建号在同一锁作用域内写入：不存在「用户已建、码未写」的中间态
        dao::store_verification_code(&conn, &email, &code_hash).map_err(internal)?;
        user_id
    };
    let dev = send_code(&st, &email, &code).await?;
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
    let exists = {
        let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
        // 防枚举：邮箱从未注册过就统一返回 ok（不发送、不建验证码记录），与 forgot-password 一致。
        // 只按「用户是否存在」判断，不看 verified：重发按钮本身就是「没收到码」的补救路径，
        // 用户此刻必然还是 verified=0。
        dao::find_user_by_email(&conn, &email).is_some()
    };
    if !exists {
        return Ok(Json(serde_json::json!({ "status": "ok", "email": email })));
    }
    let (code, code_hash) = new_verification_code();
    let written = {
        let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
        // 限频检查与写入必须在同一把锁内（dao::begin_resend_verification）：拆成两段各自加锁时，
        // 并发重发会同时通过检查 ⇒ 同一邮箱一次突发收到多封验证码（已实测）
        dao::begin_resend_verification(&conn, &email, &code_hash).map_err(internal)?
    };
    if !written {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({ "error": "请求过于频繁，请 60 秒后重试" })),
        ));
    }
    let dev = send_code(&st, &email, &code).await?;
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
    let exists = {
        let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
        dao::find_user_by_email(&conn, &email).is_some()
    };
    if !exists {
        // 防枚举：统一返回 ok（不发送）
        return Ok(Json(serde_json::json!({ "status": "ok", "email": email })));
    }
    let (code, code_hash) = new_verification_code();
    let written = {
        let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
        // 限频检查与写入必须在同一把锁内（同 resend-code：否则并发重发同时通过检查）
        dao::begin_resend_verification(&conn, &email, &code_hash).map_err(internal)?
    };
    if !written {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({ "error": "请求过于频繁，请 60 秒后重试" })),
        ));
    }
    let dev = send_code(&st, &email, &code).await?;
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
    if password_too_short(&req.new_password) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "新密码至少 8 位" })),
        ));
    }
    let email = req.email.trim().to_lowercase();
    // ① 码校验在锁内（不含 KDF）
    {
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
    }
    // ② argon2 在锁外**且**在 blocking pool 上（默认参数实测 ~0.24 s；理由同 login）
    let new_hash = crate::auth::hash_password_async(req.new_password.clone())
        .await
        .map_err(internal)?;
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    // ③ 落库前复查：KDF 期间码可能已过期/已被消费 —— 写入口令的一刻仍需持有有效授权
    let code_still_valid = match dao::find_valid_verification(&conn, &email) {
        Some((hash, _attempts)) => sha2_hex(req.code.trim()) == hash,
        None => false,
    };
    if !code_still_valid {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "验证码不存在或已过期，请重新获取" })),
        ));
    }
    // 重置密码 + 激活（未验证账号由验证码证明所有权后顺带激活）
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
    if password_too_short(&req.new_password) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "新密码至少 8 位" })),
        ));
    }
    // 旧哈希只在锁内取出；两次 argon2（校验旧口令 + 生成新哈希）都在锁外**且**在 blocking pool 上
    // （默认参数实测 ~0.24 s/次；理由同 login）
    let hash: String = {
        let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
        conn.query_row(
            "SELECT password_hash FROM users WHERE id = ?1",
            [auth.user_id],
            |r| r.get(0),
        )
        .map_err(|_| unauthorized())?
    };
    if !crate::auth::verify_password_async(hash, req.old_password.clone()).await {
        return Err(unauthorized());
    }
    let new_hash = crate::auth::hash_password_async(req.new_password.clone())
        .await
        .map_err(internal)?;
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
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
        // 网关三条：请求体可能很大（长上下文 / 多模态），放宽到 GATEWAY_BODY_LIMIT。
        .route(
            "/v1/chat/completions",
            post(gateway::chat_completions).layer(DefaultBodyLimit::max(GATEWAY_BODY_LIMIT)),
        )
        .route(
            "/anthropic/v1/messages",
            post(gateway::anthropic_messages).layer(DefaultBodyLimit::max(GATEWAY_BODY_LIMIT)),
        )
        .route(
            "/v1/responses",
            post(gateway::responses).layer(DefaultBodyLimit::max(GATEWAY_BODY_LIMIT)),
        )
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
        // 请求体的**外层粗闸**（全路由）：只对 `Content-Length` 做判断，超限则不读体
        // 直接 413。它**不是**上限的权威 —— 权威是三条网关路由上的 `DefaultBodyLimit`
        // （`String` / `Json` 的 2 MiB 默认值只认那条扩展，这层抬不动它）。但若这里写
        // 得更小，就会抢先 413 ⇒ 两层共用 `GATEWAY_BODY_LIMIT` 这一个数。
        .layer(tower_http::limit::RequestBodyLimitLayer::new(
            GATEWAY_BODY_LIMIT,
        ))
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

    /// 口令下限与它自己的宣告同单位：**字符**，不是 UTF-8 字节。
    ///
    /// `密码abc` 是 5 字符 / 9 字节 —— 下限只要按字节算，它就会被放行（9 ≥ 8），而四处宣告
    /// （注册占位符 / 两包 `err.weakPassword` / ERR_MAP 字面量）都写「至少 8 位」。
    #[test]
    fn password_minimum_is_counted_in_characters_not_bytes() {
        // 字符口径的边界。四条里三条在「字节口径」下会被放行 —— 它们才是这个测试的意义。
        assert!(password_too_short("密码abc"), "5 字符 / 9 字节");
        assert!(password_too_short("密码"), "2 字符 / 6 字节");
        assert!(
            password_too_short("😀😀😀😀"),
            "4 字符 / 16 字节 / 8 个 UTF-16 单元"
        );
        assert!(!password_too_short("密码abcdef"), "8 字符 / 14 字节");
        assert!(!password_too_short("abcdefgh"), "8 字符 ASCII");
        // 下限的**数**也不写死在测试里：从两份语言包的服务端原话里读出来。
        let i18n = include_str!("../../ui/js/i18n.js");
        let en_at = i18n.find("var EN = {").expect("EN 包");
        for pack in [&i18n[..en_at], &i18n[en_at..]] {
            let at = pack
                .find("err.weakPassword")
                .expect("该包里应有 err.weakPassword");
            let n: usize = pack[at..]
                .chars()
                .skip_while(|c| !c.is_ascii_digit())
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .expect("那句话里应有一个数");
            assert_eq!(n, MIN_PASSWORD_CHARS, "语言包宣告的位数与常量不一致");
        }
    }

    /// 端点级：被「字节口径」放行的那一个口令，现在必须被 register 拒掉（缺陷的生产表现）。
    #[tokio::test]
    async fn register_rejects_a_password_short_in_characters_long_in_bytes() {
        let st = test_state("pw_chars");
        let (s, body) = post(
            st,
            "/api/auth/register",
            r#"{"name":"pw","email":"pw-chars@example.com","password":"密码abc"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "body={body}");
        assert!(body.contains("密码至少 8 位"), "body={body}");
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

    /// 未认证请求就能触发的**阻塞段**不得拖住运行时 —— 本探针把网关跑在**单 worker** 的
    /// 运行时上，让一个请求卡在阻塞段里，再从**运行时之外**发一条与它毫无关系的 `GET /healthz`：
    /// 健康检查必须在阻塞段结束**之前**返回。三条入口各对应一种阻塞段，各自**独立成一个测试**：
    ///
    /// - `smtp`：阻塞 **I/O**（lettre 发信，失败时 3 次尝试 + 2×2 s 重试间隔）。改前读数 ≈ 3.8 s；
    ///   窗口起点是**假 SMTP 收到第一个连接**（即注册已进入发信），不是在发请求时。
    /// - `dev`：阻塞 **CPU**（注册里的 argon2 **哈希**，`auth::hash_password`，默认参数 ~0.24 s）。
    /// - `login`：阻塞 **CPU**（登录里的 argon2 **校验**，`auth::verify_password`）。
    ///
    /// 后两臂的阻塞段（argon2）是**原地且不可观测**的，于是改为**在请求飞行期间轮询** `/healthz`。
    /// （C2105：2 核 CI runner 上未认证注册的 argon2 曾让并发的 `/healthz` 等了 1.29 s，而同一测试
    /// 在多核开发机上通常只读到 ~0.4 ms ⇒ 间歇性红灯；放进 blocking pool 后三臂皆 ~0.5 ms。）
    ///
    /// **判据（C2106 修订，见下文 `prompt`）**：不是「**没有任何一次**往返被拖慢」，而是「阻塞段期间
    /// 运行时**持续**在服务无关请求」。原先用「最大等待 ÷ 请求总时长 < 1/4」，但它是个**极值**统计量：
    /// CI 的 CPU 争用会把**个别**往返（客户端或 worker 任一侧的线程被换出）单独抬高，与本缺陷无关 ——
    /// 同一棵树 `6af5e950` 的 PR run 34782411063 绿、3 分钟后的 push run 34782558242 红（实测等待
    /// 254.787 ms ÷ 总时长 865.123 ms = 0.295 > 0.25）⇒ 极值判据在 2 核 runner 上会**间歇性假红**。
    /// 改成**逐样本计数**即对争用免疫：被串行化时**一个及时样本都产生不了**（改前循环至多留下 1~2 个
    /// 样本，且首个样本 ≈ 剩余时长），未被串行化时则有几十个。smtp 臂仍是单次读数（窗口由「已进入
    /// 发信」锚定、整段 ~3.8 s ⇒ 抖动只占几个百分点），故保留比值判据。
    #[test]
    fn slow_smtp_send_does_not_stall_the_runtime() {
        stall_probe("smtp");
    }

    /// argon2 **哈希**（`auth::hash_password`）不得在 async worker 上原地跑 —— 见 `stall_probe`。
    #[test]
    fn argon2_hash_does_not_stall_the_runtime() {
        stall_probe("dev");
    }

    /// argon2 **校验**（`auth::verify_password`）不得在 async worker 上原地跑 —— 见 `stall_probe`。
    #[test]
    fn argon2_verify_does_not_stall_the_runtime() {
        stall_probe("login");
    }

    /// 三臂共用的探针（`arm` ∈ smtp/dev/login，见上文）。它必须是**普通函数**而不是 `#[test]`：
    /// 三臂分开成三个测试，A/B 才读得出**互不遮蔽**的红集合（同一个测试里第一条断言失败就会中止
    /// 后面的臂）。
    fn stall_probe(arm: &str) {
        use std::io::{Read, Write};
        use std::net::{SocketAddr, TcpListener, TcpStream};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        /// 极简 HTTP/1.1 客户端（够本测试用）：发一个请求、读到 EOF、返回 (状态码, body)。
        /// 故意不用 reqwest/tower —— 客户端必须跑在 tokio 运行时**之外**，否则它自己也会被卡住。
        fn http_call(
            addr: SocketAddr,
            method: &str,
            path: &str,
            body: Option<&str>,
        ) -> (u16, String) {
            let body = body.unwrap_or("");
            let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(10)).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
            let req = format!(
                "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            s.write_all(req.as_bytes()).unwrap();
            let mut buf = Vec::new();
            s.read_to_end(&mut buf).unwrap();
            let text = String::from_utf8_lossy(&buf).to_string();
            let status = text
                .split_whitespace()
                .nth(1)
                .and_then(|c| c.parse().ok())
                .unwrap_or(0);
            let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
            (status, body)
        }

        // 假 SMTP：接受连接后立刻关闭（每次都失败），第一次接受时通知主线程「注册已进入发信」。
        let smtp = TcpListener::bind("127.0.0.1:0").unwrap();
        let smtp_addr = smtp.local_addr().unwrap();
        let accepts = Arc::new(AtomicUsize::new(0));
        let (accepted_tx, accepted_rx) = mpsc::channel::<()>();
        {
            let accepts = accepts.clone();
            std::thread::spawn(move || {
                let mut first = true;
                for conn in smtp.incoming().flatten() {
                    accepts.fetch_add(1, Ordering::SeqCst);
                    if first {
                        first = false;
                        let _ = accepted_tx.send(());
                    }
                    drop(conn);
                }
            });
        }

        // 三条入口，三种「未认证请求即可触发的阻塞段」：
        //   smtp  = 阻塞 I/O（发信重试）
        //   dev   = 阻塞 CPU（注册里的 argon2 **哈希**）
        //   login = 阻塞 CPU（登录里的 argon2 **校验**）
        // 后两条是同一缺陷类的两个函数（`hash_password` / `verify_password`），都必须能在
        // async worker 上**原地**跑而不拖住运行时。
        let mut st = test_state(match arm {
            "smtp" => "c2092s",
            "dev" => "c2092d",
            _ => "c2092l",
        });
        if arm == "smtp" {
            let mut cfg = (*st.cfg).clone();
            cfg.mail.smtp_host = "127.0.0.1".to_string();
            cfg.mail.smtp_port = smtp_addr.port();
            cfg.mail.from = "noreply@test.local".to_string();
            st.cfg = Arc::new(cfg);
        }

        // 真网关：单 worker 运行时 + 真 TCP 监听（客户端在运行时之外，所以必须真的走网络）。
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let app = router().with_state(st.clone());
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        rt.spawn(async move {
            let l = tokio::net::TcpListener::from_std(listener).unwrap();
            axum::serve(l, app).await.unwrap();
        });

        // 客户端线程发一条**未认证**请求并等它跑完；它会占住整个（单 worker）运行时。
        let (path, body, want) = match arm {
            // 已播种的已验证用户 + 正确口令 ⇒ 走完 argon2 校验路径。
            "login" => (
                "/api/auth/login",
                r#"{"email":"demo@aitokenpool.local","password":"demo1234"}"#.to_string(),
                200u16,
            ),
            _ => (
                "/api/auth/register",
                format!(r#"{{"email":"c2092-{arm}@example.com","password":"pass1234"}}"#),
                if arm == "smtp" { 502 } else { 201 },
            ),
        };
        let t_reg = Instant::now();
        let reg = {
            let body = body.clone();
            std::thread::spawn(move || http_call(addr, "POST", path, Some(&body)))
        };

        // 窗口起点：等「注册已进入发信」。另两臂没有可观测信号 ⇒ 不等。
        if arm == "smtp" {
            accepted_rx
                .recv_timeout(Duration::from_secs(30))
                .expect("注册应在 30s 内进入发信（假 SMTP 收到连接）");
        }

        // 关键读数：请求**正在阻塞**时，从运行时之外发无关请求，记下**每一次**的等待。
        // smtp 臂有「已进入发信」的可观测信号 ⇒ 单次读数就落在窗口内；另两臂没有任何信号（
        // argon2 是不可观测的原地 CPU 段）⇒ 在请求飞行期间轮询。
        let mut waits: Vec<Duration> = Vec::new();
        let mut hs = 0u16;
        let mut hb = String::new();
        if arm == "smtp" {
            let t = Instant::now();
            let (s, b) = http_call(addr, "GET", "/healthz", None);
            waits.push(t.elapsed());
            hs = s;
            hb = b;
        } else {
            let deadline = Instant::now() + Duration::from_secs(30);
            while !reg.is_finished() && Instant::now() < deadline {
                let t = Instant::now();
                let (s, b) = http_call(addr, "GET", "/healthz", None);
                waits.push(t.elapsed());
                hs = s;
                hb = b;
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        let (rs, rb) = reg.join().unwrap();
        let reg_elapsed = t_reg.elapsed();
        let health_max = waits.iter().copied().max().unwrap_or(Duration::ZERO);
        // 「及时」＝远早于阻塞请求结束（< 总时长的 1/4）。逐样本计数，不取最大值 —— 见函数文档。
        let prompt = waits.iter().filter(|w| **w * 4 < reg_elapsed).count();
        let total_accepts = accepts.load(Ordering::SeqCst);
        println!(
            "### C2105 arm={arm} healthz_status={hs} healthz_wait_max={health_max:?} \
                 request_status={rs} accepts={total_accepts} reg_elapsed={reg_elapsed:?} \
                 probes={} prompt={prompt}",
            waits.len()
        );

        assert_eq!(hs, 200, "{arm}: /healthz 应 200（body={hb}）");
        assert_eq!(rs, want, "{arm}: POST {path} 应 {want}（body={rb}）");
        if arm == "smtp" {
            assert!(
                health_max < Duration::from_secs(1),
                "{arm}: 请求阻塞在飞行中时，并发的 /healthz 不应等它 —— 实测 {health_max:?}\
                     （改前单 worker 上会被推迟到阻塞段结束：发信 ≈3.8s）"
            );
            // 不变量：无关请求**不得被串到请求的阻塞段后面**。用「等待 ÷ 请求总时长」的比值表达，
            // 与机器快慢无关 —— 被串行化时比值 ≈1（等待本身就是阻塞段），不串行时 ≈0.0003。
            assert!(
                health_max * 4 < reg_elapsed,
                "{arm}: 无关请求不应被串到阻塞段后面 —— /healthz 最大等待 {health_max:?} 已达\
                     请求总时长 {reg_elapsed:?} 的 {:.3}（应 < 0.25）",
                health_max.as_secs_f64() / reg_elapsed.as_secs_f64()
            );
            assert_eq!(
                total_accepts, 3,
                "应恰好 3 次尝试（证明真的走完了阻塞的发信路径）"
            );
        } else {
            // 不变量（C2106 修订）：无关请求**不得被串到阻塞段后面** ⇒ 阻塞段在飞行期间，运行时应当
            // **持续**在服务它，而不是「恰好有一次没被拖慢」。极值判据（最大等待 ÷ 总时长 < 1/4）在
            // 2 核 CI runner 上会间歇性假红：争用会把个别往返单独抬高，但它动不了**绝大多数**样本
            // （改后实测 ~40 个样本中仅 1 个离群）⇒ 改用计数。被串行化时循环至多留下 1~2 个样本、
            // 且首个样本 ≈ 剩余时长 ⇒ 及时样本数为 0。
            assert!(
                prompt >= 3,
                "{arm}: 阻塞请求在飞行中时，运行时应**持续**服务无关请求 —— 探针发了 {} 次 /healthz，\
                 其中及时完成（< 请求总时长 {reg_elapsed:?} 的 1/4）的只有 {prompt} 次，最大等待 \
                 {health_max:?}；被串到阻塞段后面时 0 次",
                waits.len()
            );
        }
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

    /// 部门 PATCH：**被拒绝的请求不留副作用**，且 name 的「空」判断读的就是要落库的那个值。
    ///
    /// 三条断言对应两条不变量（`org::patch` 的文档注释）：
    /// - quota 非法（≤ 0）→ **400**，且名字/配额都必须保持原样 —— 原实现先改名、后校验 quota，
    ///   这个 400 是**带着已生效的改名**返回的（调用方以为什么都没变）。
    /// - 纯空白 name → 视为未提供（与 `#[serde(default)]` 省略同义），部门名**不许被写空** ——
    ///   原实现用未 trim 的输入做「非空」判断、用 trim 后的值落库，于是 `{"name":"   "}` 把名字写成 `""`。
    /// - 重名（409）这条腿本来就写在写语句之前 ⇒ 改前也通过，作为**阴性对照**证明本测试不是
    ///   「凡 PATCH 皆红」。
    #[tokio::test]
    async fn dept_patch_rejected_requests_leave_no_trace() {
        let st = test_state("deptpatch");
        let admin = login_bearer(&st, "admin@aitokenpool.local", "admin1234").await;
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
        let url = format!("/api/admin/departments/{dept_id}");
        // 助手：读回该部门，断言 (name, quota) —— 每次拒绝后都用它验证「什么都没变」
        async fn dept_of(st: AppState, admin: &str, dept_id: i64) -> (String, f64) {
            let (s, body) = get(st, "/api/admin/departments", Some(admin)).await;
            assert_eq!(s, StatusCode::OK, "列表应 200: {body}");
            let arr: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
            let d = arr
                .iter()
                .find(|d| d["id"].as_i64() == Some(dept_id))
                .expect("部门仍在列表里");
            (
                d["name"].as_str().unwrap().to_string(),
                d["quota"].as_f64().unwrap(),
            )
        }

        // ① quota = 0 → 400，且**名字不许被改掉**（原实现此时已把名字改成「研发中心」）
        let (s, body) = patch(
            st.clone(),
            &url,
            r#"{"name":"研发中心","quota":0}"#,
            Some(&admin),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "quota=0 应 400: {body}");
        assert_eq!(
            dept_of(st.clone(), &admin, dept_id).await,
            ("研发".to_string(), 80000.0),
            "被拒绝的 PATCH 不许留下任何已生效的改动"
        );

        // ② 纯空白 name → 视为未提供：quota 照常生效，名字保持「研发」（原实现把名字写成空串）
        let (s, body) = patch(
            st.clone(),
            &url,
            r#"{"name":"   ","quota":100}"#,
            Some(&admin),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "空白 name 视为未提供应 200: {body}");
        assert_eq!(
            dept_of(st.clone(), &admin, dept_id).await,
            ("研发".to_string(), 100.0),
            "空白 name 不许把部门名写空（create 明令 name 不能为空）"
        );

        // ③ 阴性对照：重名 409 这条腿本来就在写语句之前
        let (s, body) = post(
            st.clone(),
            "/api/admin/departments",
            r#"{"name":"市场","quota":1000}"#,
            Some(&admin),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "建第二个部门应 200: {body}");
        let (s, body) = patch(
            st.clone(),
            &url,
            r#"{"name":"市场","quota":90000}"#,
            Some(&admin),
        )
        .await;
        assert_eq!(s, StatusCode::CONFLICT, "重名应 409: {body}");
        assert_eq!(
            dept_of(st.clone(), &admin, dept_id).await,
            ("研发".to_string(), 100.0),
            "409 同样不许留下任何已生效的改动"
        );

        // ④ 阳性对照：合法请求两处都要真的写进去
        let (s, body) = patch(
            st.clone(),
            &url,
            r#"{"name":"研发中心","quota":90000}"#,
            Some(&admin),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "合法 PATCH 应 200: {body}");
        assert_eq!(
            dept_of(st.clone(), &admin, dept_id).await,
            ("研发中心".to_string(), 90000.0),
            "合法请求必须改名 + 改配额"
        );
        // 只给 quota（name 省略）→ 只改配额
        let (s, body) = patch(st.clone(), &url, r#"{"quota":95000}"#, Some(&admin)).await;
        assert_eq!(s, StatusCode::OK, "省略 name 应 200: {body}");
        assert_eq!(
            dept_of(st, &admin, dept_id).await,
            ("研发中心".to_string(), 95000.0),
            "省略 name 不许清空名字"
        );
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
    /// 这个字段自 `85982e8`（PR #80）起就在算、就在返回（`ops.rs::runtime` 内计算并作为 `total_txs` 返回），
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

    /// C2133：没有部门的那个桶，名字必须是**语言中性**的 —— 后端不得自造显示文案。
    ///
    /// 修前 SQL 是 `COALESCE(d.name, '（未分配）')`，而前端对 `departments[].name` 只过 `esc()`
    /// （`app.js` 的 `barRow(d.name, …)` → `#usage-dept`）⇒ 只要有一个未分配部门且本月有用量的
    /// 用户，`en` 界面的用量卡片上就出现中文。现在后端回空串，标签由前端
    /// `T("common.unassigned")` 提供（同一个键、同一张页面的成员表早就在用）。
    #[tokio::test]
    async fn usage_department_bucket_without_a_department_is_language_neutral() {
        fn is_cjk(c: char) -> bool {
            matches!(c, '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}')
        }

        let st = test_state("usagedeptneutral");
        let admin = login_bearer(&st, "admin@aitokenpool.local", "admin1234").await;
        // demo 未分配部门 ⇒ 它的用量落进「无部门」那个桶
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

        // ① 不变量：桶名语言中性（显示文案归前端语言包）
        let depts = v["departments"].as_array().unwrap();
        assert_eq!(
            depts.len(),
            1,
            "只有 demo 有用量且它未分配部门 ⇒ 恰好一个桶: {body}"
        );
        let name = depts[0]["name"].as_str().expect("桶名是字符串");
        assert!(
            !name.chars().any(is_cjk),
            "无部门桶名必须语言中性（en 界面会原样渲染它），实测 {name:?}"
        );
        assert_eq!(depts[0]["cost"], 2.5, "桶仍带着聚合值: {body}");

        // ② 阳性对照：真实部门名（用户数据）原样透传 —— 本不变量只约束**自造标签**
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
        let (s, body) = get(st.clone(), "/api/admin/usage", Some(&admin)).await;
        assert_eq!(s, StatusCode::OK, "usage 应 200: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let d = v["departments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["name"] == "研发")
            .expect("真实部门名原样透传");
        assert_eq!(d["cost"], 2.5, "换了部门仍是同一条聚合: {body}");
    }

    /* ---- C2089：口令 KDF（argon2）必须在共享 DB 互斥量**之外**运行 ---- */

    /// 观测「共享 DB 互斥量是否被某个请求长时间攥住」的计数器：另起一条线程反复 `try_lock`，
    /// 成功一次计一次。请求期间这个计数接近 0 ⇒ 该请求从第一次查询到最后一次写都占着锁。
    struct LockWatch {
        stop: Arc<std::sync::atomic::AtomicBool>,
        hits: Arc<std::sync::atomic::AtomicU64>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    fn spawn_lock_watch(st: &AppState) -> LockWatch {
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        let stop = Arc::new(AtomicBool::new(false));
        let hits = Arc::new(AtomicU64::new(0));
        let db = st.db.clone();
        let (s, h) = (stop.clone(), hits.clone());
        let handle = std::thread::spawn(move || {
            while !s.load(Ordering::SeqCst) {
                if db.try_lock().is_ok() {
                    h.fetch_add(1, Ordering::SeqCst);
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        });
        LockWatch {
            stop,
            hits,
            handle: Some(handle),
        }
    }

    impl LockWatch {
        fn hits(&self) -> u64 {
            self.hits.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn stop(mut self) -> u64 {
            self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
            if let Some(h) = self.handle.take() {
                h.join().unwrap();
            }
            self.hits()
        }
    }

    /// 阳性对照：锁空闲时观测者必须能抢到锁 —— 否则「请求期间 0 次」不可信（可能只是观测器没跑）
    async fn assert_lock_watch_alive(w: &LockWatch) {
        let before = w.hits();
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        let got = w.hits() - before;
        assert!(
            got >= 3,
            "阳性对照失败：锁空闲 60ms 内观测者应多次拿到锁，实测 {got} 次 —— 观测器未运行，本次测量无效"
        );
    }

    /// 造一个「故意昂贵」的口令哈希：KDF 参数写在 PHC 串里，校验方读串自带的参数 ⇒
    /// 夹具能自己决定该用户每次校验要花多久（默认参数约 0.24 s，这里约 0.8 s）。
    fn heavy_password_hash(pw: &str) -> String {
        use argon2::password_hash::{rand_core::OsRng, PasswordHasher, SaltString};
        use argon2::{Algorithm, Argon2, Params, Version};
        let params = Params::new(64 * 1024, 2, 1, None).expect("argon2 params");
        let a = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let salt = SaltString::generate(&mut OsRng);
        a.hash_password(pw.as_bytes(), &salt)
            .expect("hash")
            .to_string()
    }

    fn set_password_hash(st: &AppState, email: &str, hash: &str) {
        let conn = st.db.lock().unwrap();
        conn.execute(
            "UPDATE users SET password_hash = ?1 WHERE email = ?2",
            rusqlite::params![hash, email],
        )
        .unwrap();
    }

    /// 登录的 argon2 **校验**必须在锁外。
    ///
    /// 判别量是**计数**（请求期间观测者成功获取锁的次数），不是时长阈值：修复前该请求从第一次
    /// 查询到最后一次写都持有锁 ⇒ 0 次；修复后 KDF 的 ~0.8 s 里锁是空闲的 ⇒ 数百次。夹具的哈希
    /// 用重参数生成，「校验确实跑了很久」由 PHC 串自身保证。
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn login_verifies_the_password_outside_the_db_lock() {
        let st = test_state("kdf_login");
        set_password_hash(
            &st,
            "demo@aitokenpool.local",
            &heavy_password_hash("demo1234"),
        );
        let watch = spawn_lock_watch(&st);
        assert_lock_watch_alive(&watch).await;
        let before = watch.hits();

        let (status, body) = post(
            st.clone(),
            "/api/auth/login",
            r#"{"email":"demo@aitokenpool.local","password":"demo1234"}"#,
            None,
        )
        .await;
        let during = watch.stop() - before;

        assert_eq!(
            status,
            StatusCode::OK,
            "登录应成功（夹具正在跑重参数校验）: {body}"
        );
        assert!(
            during >= 20,
            "argon2 校验必须在 DB 锁之外运行：请求期间观测者应能拿到锁，实测 {during} 次（修复前为 0 次）"
        );
    }

    /// 注册的 argon2 **哈希**必须在锁外。（默认参数 KDF 约 0.24 s；断言同登录。）
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn register_hashes_the_password_outside_the_db_lock() {
        let st = test_state("kdf_register");
        let watch = spawn_lock_watch(&st);
        assert_lock_watch_alive(&watch).await;
        let before = watch.hits();

        let (status, body) = post(
            st.clone(),
            "/api/auth/register",
            r#"{"name":"kdf","email":"kdf-register@example.com","password":"password-1234"}"#,
            None,
        )
        .await;
        let during = watch.stop() - before;

        assert_eq!(status, StatusCode::CREATED, "注册应 201: {body}");
        assert!(
            during >= 20,
            "argon2 哈希必须在 DB 锁之外运行：请求期间观测者应能拿到锁，实测 {during} 次（修复前为 0 次）"
        );
    }

    /// 重置密码的 argon2 **哈希**必须在锁外（码校验留在锁内，落库前复查码仍有效）。
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn reset_password_hashes_outside_the_db_lock() {
        let st = test_state("kdf_reset");
        let (s, body) = post(
            st.clone(),
            "/api/auth/register",
            r#"{"name":"r","email":"kdf-reset@example.com","password":"old-password-1"}"#,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::CREATED, "注册应 201: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let code = v["dev_code"].as_str().expect("dev 模式返回验证码");

        let watch = spawn_lock_watch(&st);
        assert_lock_watch_alive(&watch).await;
        let before = watch.hits();

        let (status, body) = post(
            st.clone(),
            "/api/auth/reset-password",
            &format!(
                r#"{{"email":"kdf-reset@example.com","code":"{code}","new_password":"new-password-1"}}"#
            ),
            None,
        )
        .await;
        let during = watch.stop() - before;

        assert_eq!(status, StatusCode::OK, "重置密码应 200: {body}");
        assert!(
            during >= 20,
            "argon2 哈希必须在 DB 锁之外运行：请求期间观测者应能拿到锁，实测 {during} 次（修复前为 0 次）"
        );
    }

    /// 改密的两次 argon2（校验旧口令 + 生成新哈希）必须在锁外。
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn change_password_hashes_outside_the_db_lock() {
        let st = test_state("kdf_change");
        set_password_hash(
            &st,
            "demo@aitokenpool.local",
            &heavy_password_hash("demo1234"),
        );
        let bearer = login_bearer(&st, "demo@aitokenpool.local", "demo1234").await;

        let watch = spawn_lock_watch(&st);
        assert_lock_watch_alive(&watch).await;
        let before = watch.hits();

        let (status, body) = post(
            st.clone(),
            "/api/auth/change-password",
            r#"{"old_password":"demo1234","new_password":"demo12345"}"#,
            Some(&bearer),
        )
        .await;
        let during = watch.stop() - before;

        assert_eq!(status, StatusCode::OK, "改密应 200: {body}");
        assert!(
            during >= 20,
            "argon2 必须在 DB 锁之外运行：请求期间观测者应能拿到锁，实测 {during} 次（修复前为 0 次）"
        );
    }
}
