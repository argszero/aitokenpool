//! GitHub 登录（rant 2026-11-09T14:43:55）：设备码流程 + 身份即账号。
//!
//! 客户端契约（**字段名与状态码不得随意改** —— 客户端已按此实现）：
//!
//! | 端点 | 请求 | 成功 | 中间态 |
//! |---|---|---|---|
//! | `POST /api/auth/github/device/code` | 无 | 200 `{device_code, user_code, verification_uri, expires_in, interval}` | — |
//! | `POST /api/auth/github/device/token` | `{device_code}` | 200 `{api_key, user_id, login}` | 202 `authorization_pending` / 429 `slow_down` / 410 `expired_token` / 403 `access_denied` |
//!
//! 成功响应的凭据字段与邮箱登录（`POST /api/auth/login`）**逐字同形**：`api_key` +
//! `user_id`；已有 key 就复用（同一个 `dao::get_or_create_api_key`），不新开第二条签发路径。
//!
//! 中间态**绝不发 key**：GitHub 用 `200 + {"error": …}` 回答这四种状态，本模块先看 `error`
//! 再谈 `access_token`，并在那条分支上直接返回、不碰数据库。
//!
//! **错误体是机器码 + 英文 `detail`**：本端点的消费方是自带本地化的桌面客户端 / CLI，
//! 它按 `error` 的码分支，而不是把服务端文案直接印给用户 ⇒ 文案不进词表（网页那一半
//! 「使用 GitHub 登录」落地时，按钮文案与提示走词表）。这一点在
//! `i18n_pack` 的 `BACKEND_ERROR_SOURCES` 名册里有登记。
//!
//! 上游地址**可注入**（[`GithubEndpoints`]，挂在 `AppState::github_api` 上）：默认打
//! github.com / api.github.com，测试把它指向本地假上游。为什么是进程内字段而不是配置键
//! ——配置项只有两个（client id / secret），验收 2 要求测试与外网解耦，而验收 6 只允许
//! 两个配置项；于是注入点放在字段里，思路与 `gateway` 测试用 `[[plans]] endpoints.base_url`
//! 注假上游一致。

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use super::{internal, ApiErr, AppState};

/// GitHub 上游地址（可注入：测试指向本地假上游，生产留默认值）
#[derive(Debug, Clone)]
pub struct GithubEndpoints {
    /// OAuth 页：设备码申请 `/login/device/code` 与 token 交换 `/login/oauth/access_token`
    pub oauth_base: String,
    /// REST API：读身份 `GET /user`
    pub api_base: String,
}

impl Default for GithubEndpoints {
    fn default() -> Self {
        Self {
            oauth_base: "https://github.com".to_string(),
            api_base: "https://api.github.com".to_string(),
        }
    }
}

/// 未启用时的统一答复：503 + 机器码 + 说清该配什么
/// （**不是** 500 —— 没配是部署状态，不是内部故障）
fn disabled() -> ApiErr {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": "github_login_not_enabled",
            "detail": "Set [github] client_id and client_secret to enable GitHub login.",
        })),
    )
}

/// 取已启用的凭据；未配置 → [`disabled`]
fn creds(st: &AppState) -> Result<(String, String), ApiErr> {
    let g = &st.cfg.github;
    if !g.enabled() {
        return Err(disabled());
    }
    Ok((
        g.client_id.trim().to_string(),
        g.client_secret.trim().to_string(),
    ))
}

/// GitHub 要求带 User-Agent，缺了会被 403（不是 400，容易误判成权限问题）
const UA: (&str, &str) = ("user-agent", "aitokenpool");

/// POST /api/auth/github/device/code → 200
/// `{device_code, user_code, verification_uri, expires_in, interval}`
pub async fn device_code(State(st): State<AppState>) -> Result<Json<Value>, ApiErr> {
    let (client_id, _secret) = creds(&st)?;
    let url = format!("{}/login/device/code", st.github_api.oauth_base);
    let resp = st
        .http
        .post(&url)
        .header(UA.0, UA.1)
        .header("accept", "application/json")
        // scope 留空：不请求 user:email —— 身份只靠 /user 的 id + login（验收 3）
        .form(&[("client_id", client_id.as_str()), ("scope", "")])
        .send()
        .await
        .map_err(|e| internal(format!("GitHub 设备码请求失败: {e}")))?;
    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .map_err(|e| internal(format!("GitHub 设备码响应无法解析: {e}")))?;
    if !status.is_success() {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": format!("github_device_code_rejected: {body}") })),
        ));
    }
    // 逐字段转出：客户端只认这五个名字，上游多给的字段不外泄
    Ok(Json(json!({
        "device_code": body["device_code"].clone(),
        "user_code": body["user_code"].clone(),
        "verification_uri": body["verification_uri"].clone(),
        "expires_in": body["expires_in"].clone(),
        "interval": body["interval"].clone(),
    })))
}

#[derive(serde::Deserialize)]
pub struct DeviceTokenReq {
    pub device_code: String,
}

/// POST /api/auth/github/device/token `{device_code}`
///
/// 200 → `{api_key, user_id, login}`；中间态 → 202 / 429 / 410 / 403（见模块文档）。
pub async fn device_token(
    State(st): State<AppState>,
    Json(req): Json<DeviceTokenReq>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (client_id, client_secret) = creds(&st)?;
    let url = format!("{}/login/oauth/access_token", st.github_api.oauth_base);
    let resp = st
        .http
        .post(&url)
        .header(UA.0, UA.1)
        .header("accept", "application/json")
        .form(&[
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("device_code", req.device_code.as_str()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
        .await
        .map_err(|e| internal(format!("GitHub 设备码换取失败: {e}")))?;
    let body: Value = resp
        .json()
        .await
        .map_err(|e| internal(format!("GitHub 交换响应无法解析: {e}")))?;
    // 中间态在前：GitHub 用 200 + {"error": …} 回答，先看 error 才不会把
    // 「用户还没在浏览器里确认」误判成「拿到了 token」
    if let Some(err) = body.get("error").and_then(|v| v.as_str()) {
        // 每个中间态把自己的**契约码**写进响应体：客户端按 `error` 的码分支，所以这四处
        // 刻意是字面量（契约里的四个码就是这四个词），而不是把上游串原样透传。
        let (code, err_body) = match err {
            "authorization_pending" => (
                StatusCode::ACCEPTED,
                json!({ "error": "authorization_pending" }),
            ),
            "slow_down" => (
                StatusCode::TOO_MANY_REQUESTS,
                json!({ "error": "slow_down" }),
            ),
            "expired_token" => (StatusCode::GONE, json!({ "error": "expired_token" })),
            "access_denied" => (StatusCode::FORBIDDEN, json!({ "error": "access_denied" })),
            other => {
                return Err((
                    StatusCode::BAD_GATEWAY,
                    Json(json!({ "error": format!("github_exchange_failed: {other}") })),
                ))
            }
        };
        // ⚠️ 中间态**绝不发 key**：不碰数据库，直接返回
        return Ok((code, Json(err_body)));
    }
    let token = body
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "github_exchange_missing_access_token" })),
            )
        })?
        .to_string();
    let (github_id, login) = fetch_identity(&st, &token).await?;
    let (user_id, api_key) = land_identity(&st, &github_id, &login)?;
    log::info!("GitHub 登录成功：github_id={github_id} user_id={user_id}");
    Ok((
        StatusCode::OK,
        Json(json!({ "api_key": api_key, "user_id": user_id, "login": login })),
    ))
}

/// 用 access_token 读 GitHub 身份 → `(稳定数字 id, login)`
///
/// `id` 是 GitHub 账号的稳定主键，`login` 可改名 ⇒ 落库只认前者。
async fn fetch_identity(st: &AppState, token: &str) -> Result<(String, String), ApiErr> {
    let url = format!("{}/user", st.github_api.api_base);
    let resp = st
        .http
        .get(&url)
        .header(UA.0, UA.1)
        .header("accept", "application/vnd.github+json")
        .header("authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| internal(format!("GitHub 身份请求失败: {e}")))?;
    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .map_err(|e| internal(format!("GitHub 身份响应无法解析: {e}")))?;
    if !status.is_success() {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": format!("github_identity_rejected: {body}") })),
        ));
    }
    let id = body.get("id").and_then(|v| v.as_i64()).ok_or_else(|| {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "github_identity_missing_id" })),
        )
    })?;
    let login = body
        .get("login")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "github_identity_missing_login" })),
            )
        })?
        .to_string();
    Ok((id.to_string(), login))
}

// ---------------------------------------------------------------------------
// 网页授权码流程（验收 5）：登录页的「使用 GitHub 登录」
//
// 与设备码流程**共用** [`land_identity`] —— 两条流程只差「怎么拿到 access_token」，
// 拿到之后同一条落库路径，故同一 GitHub 账号从任一入口进来都回到同一行、同一把 key。
// ---------------------------------------------------------------------------

/// state 的 cookie 名。授权码流程的 CSRF 绑定：本仓库没有会话层，故用一张短命
/// HttpOnly cookie 记住「这次授权是本浏览器发起的」。
const STATE_COOKIE: &str = "atp_gh_state";
/// state 有效期（秒）：与 GitHub 授权码的有效期同量级
const STATE_TTL_SECS: u32 = 600;

/// 对外可达地址（去掉尾斜杠）：`redirect_uri` 由它派生。
///
/// 刻意**不**设成第三个配置键（验收 6 只允许 client id / secret）：OAuth App 里登记的
/// 回调地址就写成 `public_url` 本身（登录页），回头把 `code` / `state` 带回前端。
fn redirect_uri(st: &AppState) -> String {
    st.cfg.server.public_url.trim_end_matches('/').to_string()
}

/// state cookie 的属性串：`http` 的 public_url 上不加 `Secure`，否则浏览器直接不存
fn state_cookie(st: &AppState, value: &str, max_age: u32) -> String {
    let secure = if redirect_uri(st).starts_with("https://") {
        "; Secure"
    } else {
        ""
    };
    format!("{STATE_COOKIE}={value}; Path=/; Max-Age={max_age}; HttpOnly; SameSite=Lax{secure}")
}

/// GET /api/auth/github/authorize → 200 `{authorize_url}` + `Set-Cookie: atp_gh_state=…`
///
/// 前端拿到 `authorize_url` 就整页跳过去；GitHub 认完再把 `code` / `state` 送回
/// `public_url`，由前端调 [`exchange`] 换平台凭据。
pub async fn authorize(
    State(st): State<AppState>,
) -> Result<impl axum::response::IntoResponse, ApiErr> {
    let (client_id, _secret) = creds(&st)?;
    let state = uuid::Uuid::new_v4().simple().to_string();
    let mut url = reqwest::Url::parse(&format!(
        "{}/login/oauth/authorize",
        st.github_api.oauth_base
    ))
    .map_err(|e| internal(format!("GitHub 授权地址无法构造: {e}")))?;
    url.query_pairs_mut()
        .append_pair("client_id", &client_id)
        // 由 public_url 派生（见 redirect_uri）：OAuth App 里登记的就是它
        .append_pair("redirect_uri", &redirect_uri(&st))
        // 与设备码同一取舍：不请求 user:email —— 身份只靠 /user 的 id + login（验收 3）
        .append_pair("scope", "")
        .append_pair("state", &state);
    Ok((
        StatusCode::OK,
        [(
            axum::http::header::SET_COOKIE,
            state_cookie(&st, &state, STATE_TTL_SECS),
        )],
        Json(json!({ "authorize_url": url.to_string() })),
    ))
}

#[derive(serde::Deserialize)]
pub struct ExchangeReq {
    pub code: String,
    pub state: String,
}

/// POST /api/auth/github/exchange `{code, state}` → 200 `{api_key, user_id, login}`
///
/// 字段与设备码成功响应、邮箱登录**逐字同形**（验收 2）。`state` 必须与
/// [`authorize`] 那张 cookie 逐字相同，否则 400 且**不访问上游、不碰数据库**。
pub async fn exchange(
    State(st): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<ExchangeReq>,
) -> Result<
    (
        StatusCode,
        [(axum::http::HeaderName, String); 1],
        Json<Value>,
    ),
    ApiErr,
> {
    let (client_id, client_secret) = creds(&st)?;
    // 先验 state 再换码：伪造的 state 不该消耗 GitHub 的授权码，也不该产生任何落库
    let cookie_state = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| {
            raw.split(';')
                .map(str::trim)
                .find_map(|kv| kv.strip_prefix(STATE_COOKIE)?.strip_prefix('='))
        })
        .unwrap_or_default()
        .to_string();
    if cookie_state.is_empty() || cookie_state != req.state {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "github_state_mismatch",
                "detail": "Restart sign-in from the login page: the state cookie did not match.",
            })),
        ));
    }
    let ru = redirect_uri(&st);
    let url = format!("{}/login/oauth/access_token", st.github_api.oauth_base);
    let resp = st
        .http
        .post(&url)
        .header(UA.0, UA.1)
        .header("accept", "application/json")
        .form(&[
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("code", req.code.as_str()),
            // 换码时的 redirect_uri 必须与授权时那一份逐字相同，否则 GitHub 回 400
            ("redirect_uri", ru.as_str()),
        ])
        .send()
        .await
        .map_err(|e| internal(format!("GitHub 授权码换取失败: {e}")))?;
    let body: Value = resp
        .json()
        .await
        .map_err(|e| internal(format!("GitHub 授权码响应无法解析: {e}")))?;
    if let Some(err) = body.get("error").and_then(|v| v.as_str()) {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": format!("github_exchange_failed: {err}") })),
        ));
    }
    let token = body
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "github_exchange_missing_access_token" })),
            )
        })?
        .to_string();
    let (github_id, login) = fetch_identity(&st, &token).await?;
    let (user_id, api_key) = land_identity(&st, &github_id, &login)?;
    log::info!("GitHub 网页登录成功：github_id={github_id} user_id={user_id}");
    Ok((
        StatusCode::OK,
        // 用过的 state 立刻作废：重放同一个 code/state 组合没有第二次机会
        [(axum::http::header::SET_COOKIE, state_cookie(&st, "", 0))],
        Json(json!({ "api_key": api_key, "user_id": user_id, "login": login })),
    ))
}

/// 身份落库 —— 设备码与网页授权码**共用这一条路径**（验收 5）。
///
/// 身份只认 `github_id`（`login` 可改名）；key 已有就复用，不新开第二条签发路径。
fn land_identity(st: &AppState, github_id: &str, login: &str) -> Result<(i64, String), ApiErr> {
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let (user_id, _created) =
        crate::dao::upsert_github_user(&conn, github_id, login).map_err(internal)?;
    // 已有 key 就复用（验收 2）：与邮箱登录同一条签发路径
    let api_key = crate::dao::get_or_create_api_key(&conn, user_id).map_err(internal)?;
    Ok((user_id, api_key))
}
