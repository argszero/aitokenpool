//! GitHub 登录门禁（rant 2026-11-09T14:43:55 验收 1）
//!
//! 只读语料，零生产代码：
//! - `src/routes/mod.rs` —— 两个端点必须登记在契约路径下；
//! - `src/routes/github_oauth.rs` —— 五个 / 三个字段名、四种中间态与状态码的映射；
//! - `config/config.example.toml` —— 配置项只有两个（client id / secret），且必须写明
//!   OAuth App 要开 Device Flow。
//!
//! 另有四条**行为**断言（同一份文件，走本地假上游，不打真实 github.com —— 验收 2）：
//! 中间态绝不发 key、同一身份两次登录复用同一个 key 且各表只增一行、新账号有可用余额账户
//! 且当日赠送可发放、未配置凭据返回「未启用」而不是 500。
//!
//! ⚠️ 每条断言都带**非空阳性对照**：提取器一旦失真（比如路径或字段名后来改了），断言
//! 会在空集上「通过」—— 阳性对照让这种情况变成红灯，而不是静默的绿。

/// 本文件整体只在测试期编译（`main.rs` 里 `#[cfg(test)] mod github_oauth_gate;`）。
/// 下面这个 `#[cfg(test)]` 是**给 i18n 提取器看的**：它在第一个 `#[cfg(test)]` 处截断，
/// 于是本文件里那些 `{"error": …}` 夹具不会被当成「后端错误文案产出者」（与 `src/` 顶层
/// 其它 gate 模块同一个形状，见 `i18n_pack::backend_error_sources_cover_every_file_that_emits_an_error_literal`）。
#[cfg(test)]
mod gate {
    use axum::extract::State as AxState;
    use axum::http::StatusCode;
    use axum::routing::post;
    use axum::Json;
    use axum::Router;
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};
    use tower::util::ServiceExt;

    use crate::routes::AppState;

    const ROUTES_SRC: &str = include_str!("routes/mod.rs");
    const GH_SRC: &str = include_str!("routes/github_oauth.rs");
    const EXAMPLE_CFG: &str = include_str!("../config/config.example.toml");

    // ============================================================
    // 语料侧：契约的形状
    // ============================================================

    /// R1：两个端点登记在契约路径下
    #[test]
    fn the_two_endpoints_are_registered_under_the_contract_paths() {
        // 阳性对照：同一份语料里必须找得到一个**已知存在**的路由。提取器失真时
        // （比如 ROUTES_SRC 读到空串、或路径写法整体换掉）这条先红。
        assert!(
            ROUTES_SRC.contains(r#".route("/api/auth/login", post(login))"#),
            "阳性对照失效：语料里连邮箱登录路由都没有，提取器不可信"
        );
        for path in [
            "/api/auth/github/device/code",
            "/api/auth/github/device/token",
            // 网页登录页的那一半（验收 5）
            "/api/auth/github/authorize",
            "/api/auth/github/exchange",
        ] {
            // 只认路径字面量本身：路由写成一行还是折行不是契约，挂了没有才是
            assert!(
                ROUTES_SRC.contains(&format!("\"{path}\"")),
                "路由 {path} 未登记"
            );
        }
        // 处理函数也必须接在路由上（路径在、函数没接 = 路由表与实现脱节）
        for handler in ["device_code", "device_token", "authorize", "exchange"] {
            assert!(
                ROUTES_SRC.contains(&format!("github_oauth::{handler}")),
                "处理函数 {handler} 未接到路由上"
            );
        }
    }

    /// R2：设备码响应的五个字段逐一在场
    #[test]
    fn the_device_code_response_carries_the_five_contract_fields() {
        // 阳性对照：一个必然在场的字段名
        assert!(GH_SRC.contains("\"device_code\":"));
        for field in [
            "device_code",
            "user_code",
            "verification_uri",
            "expires_in",
            "interval",
        ] {
            let needle = format!("\"{field}\":");
            assert!(GH_SRC.contains(&needle), "设备码响应缺字段 {field}");
        }
    }

    /// R3：换取响应的三个字段逐一在场，且凭据字段与邮箱登录同形
    #[test]
    fn the_token_response_carries_the_contract_fields() {
        assert!(
            GH_SRC.contains("\"api_key\":"),
            "阳性对照：api_key 必须在场"
        );
        for field in ["api_key", "user_id", "login"] {
            let needle = format!("\"{field}\":");
            assert!(GH_SRC.contains(&needle), "换取响应缺字段 {field}");
        }
    }

    /// R4：四种中间态各自映射到契约状态码，且**每一种**都先于 access_token 判定
    #[test]
    fn the_four_intermediate_states_map_to_their_contract_codes() {
        // 阳性对照：四种状态名里至少一个必须在场
        assert!(GH_SRC.contains("authorization_pending"));
        let pairs = [
            ("authorization_pending", "StatusCode::ACCEPTED"),
            ("slow_down", "StatusCode::TOO_MANY_REQUESTS"),
            ("expired_token", "StatusCode::GONE"),
            ("access_denied", "StatusCode::FORBIDDEN"),
        ];
        // 去掉全部空白再匹配：臂怎么折行不是契约，**码与状态码配在一起**才是
        let flat: String = GH_SRC.chars().filter(|c| !c.is_whitespace()).collect();
        for (err, status) in pairs {
            let needle = format!("\"{err}\"=>({status}");
            assert!(
                flat.contains(&needle),
                "中间态 {err} 必须映射到 {status}（契约状态码，客户端已按此实现）"
            );
        }
    }

    /// R5：示例配置只声明两个键，并写明 OAuth App 必须开 Device Flow
    #[test]
    fn the_example_config_declares_only_the_two_credentials() {
        let start = EXAMPLE_CFG
            .find("\n[github]\n")
            .expect("示例配置必须出现 [github] 段（否则用户不知道有这个开关）")
            + 1;
        let rest = &EXAMPLE_CFG[start..];
        let end = rest.find("\n[").unwrap_or(rest.len());
        let block = &rest[..end];
        let mut keys: Vec<&str> = block
            .lines()
            .filter(|l| !l.trim_start().starts_with('#') && l.contains('='))
            .filter_map(|l| l.split('=').next())
            .map(str::trim)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["client_id", "client_secret"],
            "配置项只有两个（client id / secret），不得扩键"
        );
        assert!(
            EXAMPLE_CFG.contains("Enable Device Flow"),
            "示例配置必须写明 OAuth App 要开 Device Flow，否则设备码流程静默不可用"
        );
    }

    // ============================================================
    // 行为侧：本地假上游（不打真实 github.com）
    // ============================================================

    #[derive(Clone)]
    struct Mock {
        /// `/login/oauth/access_token` 依次给出的响应；用完后重复最后一个
        tokens: Arc<Mutex<Vec<Value>>>,
        /// `GET /user` 的响应
        user: Value,
        /// 被打到的路径（证明请求确实进了假上游，而不是在别处「通过」）
        seen: Arc<Mutex<Vec<&'static str>>>,
    }

    async fn mock_device_code(AxState(m): AxState<Mock>) -> Json<Value> {
        m.seen.lock().unwrap().push("device/code");
        Json(json!({
            "device_code": "dc-abc",
            "user_code": "UC-123",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900,
            "interval": 5
        }))
    }

    async fn mock_token(AxState(m): AxState<Mock>) -> Json<Value> {
        let queue = m.tokens.lock().unwrap();
        if queue.is_empty() {
            return Json(json!({}));
        }
        let at = if queue.len() > 1 { 0 } else { queue.len() - 1 };
        Json(queue[at].clone())
    }

    async fn mock_user(AxState(m): AxState<Mock>) -> Json<Value> {
        m.seen.lock().unwrap().push("user");
        Json(m.user.clone())
    }

    /// 起一个本地假 GitHub，返回它的 base url 与可观测句柄
    async fn start_mock(tokens: Vec<Value>, user: Value) -> (String, Mock) {
        let mock = Mock {
            tokens: Arc::new(Mutex::new(tokens)),
            user,
            seen: Arc::new(Mutex::new(Vec::new())),
        };
        let app = Router::new()
            .route("/login/device/code", post(mock_device_code))
            .route("/login/oauth/access_token", post(mock_token))
            .route("/user", axum::routing::get(mock_user))
            .with_state(mock.clone());
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind 假上游");
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("假上游服务");
        });
        (base, mock)
    }

    /// 建状态：`base = None` → 凭据留空（未启用）；`Some` → 配好凭据并指向假上游。
    /// 不 seed 任何用户：断言「只增一行」要一个空基线。
    fn state_with(tag: &str, base: Option<&str>) -> AppState {
        let p = std::env::temp_dir().join(format!("atp_gh_{}_{}.db", std::process::id(), tag));
        let _ = std::fs::remove_file(&p);
        let conn = crate::db::open(p.to_str().unwrap()).expect("open tmp db");
        let mut cfg = crate::config::Config::load("config/config.example.toml").unwrap();
        match base {
            Some(_) => {
                cfg.github.client_id = "test-client".to_string();
                cfg.github.client_secret = "test-secret".to_string();
            }
            None => {
                cfg.github.client_id = String::new();
                cfg.github.client_secret = String::new();
            }
        }
        let crypto = crate::crypto::Crypto::new([7u8; 32]);
        let mut st = AppState::new(conn, Arc::new(cfg), crypto);
        if let Some(b) = base {
            st.github_api = crate::routes::github_oauth::GithubEndpoints {
                oauth_base: b.to_string(),
                api_base: b.to_string(),
            };
        }
        st
    }

    async fn post_json(st: AppState, uri: &str, body: &str) -> (StatusCode, Value) {
        let req = axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap();
        let resp = crate::routes::router()
            .with_state(st)
            .oneshot(req)
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
        let v = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, v)
    }

    fn count(st: &AppState, sql: &str) -> i64 {
        let conn = st.db.lock().unwrap();
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    /// 未启用 → 503 + 「未启用」，**不是** 500；阳性对照：配好凭据后同一端点必须通
    #[tokio::test]
    async fn an_unconfigured_client_answers_503_and_says_it_is_not_enabled() {
        let st = state_with("disabled", None);
        let (status, body) = post_json(st, "/api/auth/github/device/code", "").await;
        assert_eq!(
            status,
            StatusCode::SERVICE_UNAVAILABLE,
            "未启用必须是有话可说的 503，而不是 500：{body}"
        );
        let msg = body["error"].as_str().unwrap_or_default();
        assert_eq!(
            msg, "github_login_not_enabled",
            "未启用要有自己的机器码（客户端按码分支）：{body}"
        );
        let detail = body["detail"].as_str().unwrap_or_default();
        assert!(detail.contains("client_id"), "还要说清该配什么：{body}");

        // 阳性对照：同一端点、只把凭据填上，就必须 200
        let (base, _m) = start_mock(
            vec![json!({"access_token": "gho_x"})],
            json!({"id": 1, "login": "a"}),
        )
        .await;
        let st2 = state_with("disabled_control", Some(&base));
        let (s2, b2) = post_json(st2, "/api/auth/github/device/code", "").await;
        assert_eq!(s2, StatusCode::OK, "阳性对照：配好凭据后必须通：{b2}");
    }

    /// 200 + 五个字段逐一出值（不是 null）
    #[tokio::test]
    async fn the_device_code_response_fills_the_five_fields() {
        let (base, mock) = start_mock(
            vec![json!({"access_token": "gho_x"})],
            json!({"id": 1, "login": "a"}),
        )
        .await;
        let st = state_with("code_fields", Some(&base));
        let (status, body) = post_json(st, "/api/auth/github/device/code", "").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        for field in [
            "device_code",
            "user_code",
            "verification_uri",
            "expires_in",
            "interval",
        ] {
            assert!(!body[field].is_null(), "字段 {field} 必须有值：{body}");
        }
        assert_eq!(
            *mock.seen.lock().unwrap(),
            vec!["device/code"],
            "请求必须真的进假上游"
        );
    }

    /// 中间态绝不发 key：202 ＋ 零 key 行、零用户行
    #[tokio::test]
    async fn an_authorization_pending_never_issues_a_key() {
        let (base, _m) = start_mock(
            vec![json!({"error": "authorization_pending"})],
            json!({"id": 7, "login": "octo"}),
        )
        .await;
        let st = state_with("pending", Some(&base));
        let (status, body) = post_json(
            st.clone(),
            "/api/auth/github/device/token",
            r#"{"device_code":"dc-abc"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "pending 是 202：{body}");
        assert_eq!(body["error"], "authorization_pending");
        assert!(body.get("api_key").is_none(), "中间态绝不发 key：{body}");
        assert_eq!(
            count(&st, "SELECT COUNT(*) FROM api_keys"),
            0,
            "不许落 key 行"
        );
        assert_eq!(count(&st, "SELECT COUNT(*) FROM users"), 0, "不许建用户行");
    }

    /// 其余三种中间态各自的契约状态码
    #[tokio::test]
    async fn the_other_three_intermediate_states_map_to_429_410_403() {
        for (err, want) in [
            ("slow_down", StatusCode::TOO_MANY_REQUESTS),
            ("expired_token", StatusCode::GONE),
            ("access_denied", StatusCode::FORBIDDEN),
        ] {
            let (base, _m) = start_mock(
                vec![json!({"error": err})],
                json!({"id": 9, "login": "octo"}),
            )
            .await;
            let st = state_with(&format!("mid_{err}"), Some(&base));
            let (status, body) = post_json(
                st,
                "/api/auth/github/device/token",
                r#"{"device_code":"dc-abc"}"#,
            )
            .await;
            assert_eq!(status, want, "{err} 必须是 {want}：{body}");
            assert_eq!(body["error"], err);
        }
    }

    /// 同一 GitHub 身份两次登录：用户行与 key 行各只增一次，`api_key` 逐字相同
    #[tokio::test]
    async fn the_same_identity_twice_returns_the_same_key_and_adds_one_row_each() {
        // 假上游第一次给 token、第二次也给 token（同一个 GitHub 账号登录两次）
        let (base, mock) = start_mock(
            vec![json!({"access_token": "gho_x"})],
            json!({"id": 4242, "login": "octocat"}),
        )
        .await;
        let st = state_with("same_twice", Some(&base));
        let (s1, b1) = post_json(
            st.clone(),
            "/api/auth/github/device/token",
            r#"{"device_code":"dc-abc"}"#,
        )
        .await;
        assert_eq!(s1, StatusCode::OK, "{b1}");
        assert_eq!(b1["login"], "octocat", "响应要带 login：{b1}");
        let first_key = b1["api_key"]
            .as_str()
            .expect("api_key 是字符串")
            .to_string();
        assert!(
            first_key.starts_with("atk_live_"),
            "凭据与邮箱登录同形（同一个签发函数）：{first_key}"
        );
        let first_user = b1["user_id"].clone();
        assert!(first_user.as_i64().is_some(), "user_id 是数字：{b1}");

        let (s2, b2) = post_json(
            st.clone(),
            "/api/auth/github/device/token",
            r#"{"device_code":"dc-abc"}"#,
        )
        .await;
        assert_eq!(s2, StatusCode::OK, "{b2}");
        assert_eq!(b2["api_key"], b1["api_key"], "同一身份必须复用同一个 key");
        assert_eq!(b2["user_id"], first_user, "同一身份必须回到同一账号");
        assert_eq!(
            count(&st, "SELECT COUNT(*) FROM users"),
            1,
            "用户行只增一次"
        );
        assert_eq!(
            count(&st, "SELECT COUNT(*) FROM api_keys"),
            1,
            "key 行只增一次"
        );
        assert!(
            mock.seen.lock().unwrap().contains(&"user"),
            "身份必须真的从假上游读回来"
        );
    }

    /// 新账号：可用余额账户随建号创建，当日赠送可发放（与邮箱注册待遇一致）
    #[tokio::test]
    async fn a_new_account_has_a_quota_row_and_can_receive_the_daily_gift() {
        let (base, _m) = start_mock(
            vec![json!({"access_token": "gho_x"})],
            json!({"id": 555, "login": "newbie"}),
        )
        .await;
        let st = state_with("new_account", Some(&base));
        let (status, body) = post_json(
            st.clone(),
            "/api/auth/github/device/token",
            r#"{"device_code":"dc-abc"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let user_id = body["user_id"].as_i64().expect("user_id");

        let conn = st.db.lock().unwrap();
        let quotas: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM quotas WHERE user_id = ?1",
                [user_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(quotas, 1, "可用余额账户随建号创建");
        // 注册即已验证（验收 3）：不走邮箱验证码
        let verified: i64 = conn
            .query_row("SELECT verified FROM users WHERE id = ?1", [user_id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(verified, 1, "GitHub 身份即已验证，不该落在待验证态");
        // 每日赠送照常发放（验收 4）
        let granted = crate::gift::ensure_daily_gift(&conn, user_id).expect("赠送可发放");
        assert!(granted, "新账号当日赠送必须能发出来");
        let gift_balance: f64 = conn
            .query_row(
                "SELECT gift_balance FROM quotas WHERE user_id = ?1",
                [user_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(gift_balance, 1.0, "每日赠送 1 点");
    }

    // ============================================================
    // 网页授权码流程（验收 5）：登录页的「使用 GitHub 登录」
    // ============================================================

    /// 带 cookie 发请求：网页流程的 state 就活在这张 cookie 里
    async fn req_with(
        st: AppState,
        method: &str,
        uri: &str,
        body: Option<&str>,
        cookie: Option<&str>,
    ) -> (StatusCode, axum::http::HeaderMap, Value) {
        let mut b = axum::http::Request::builder().method(method).uri(uri);
        if let Some(c) = cookie {
            b = b.header("cookie", c);
        }
        let req = match body {
            Some(j) => b
                .header("content-type", "application/json")
                .body(axum::body::Body::from(j.to_string()))
                .unwrap(),
            None => b.body(axum::body::Body::empty()).unwrap(),
        };
        let resp = crate::routes::router()
            .with_state(st)
            .oneshot(req)
            .await
            .unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
        let v = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, headers, v)
    }

    fn set_cookie(headers: &axum::http::HeaderMap) -> String {
        headers
            .get(axum::http::header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string()
    }

    /// 授权地址由 `public_url` 派生（**不**新增第三个配置键），state 落一张 HttpOnly cookie；
    /// 未启用时同样是 503
    #[tokio::test]
    async fn the_authorize_endpoint_derives_its_redirect_from_public_url() {
        let (base, _m) = start_mock(
            vec![json!({"access_token": "gho_x"})],
            json!({"id": 1, "login": "a"}),
        )
        .await;
        let st = state_with("authorize", Some(&base));
        let public_url = st.cfg.server.public_url.trim_end_matches('/').to_string();
        let (status, headers, body) =
            req_with(st.clone(), "GET", "/api/auth/github/authorize", None, None).await;
        assert_eq!(status, StatusCode::OK, "配好凭据后必须给出授权地址：{body}");
        let url = body["authorize_url"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        // 阳性对照：地址确实指向（可注入的）假上游，而不是一个常量串
        assert!(
            url.starts_with(&format!("{base}/login/oauth/authorize")),
            "必须打可注入的上游：{url}"
        );
        assert!(url.contains("client_id=test-client"), "缺 client_id：{url}");
        let encoded = public_url.replace(':', "%3A").replace('/', "%2F");
        assert!(
            url.contains(&format!("redirect_uri={encoded}")),
            "redirect_uri 必须由 public_url 派生（期望 {encoded}）：{url}"
        );
        assert!(url.contains("state="), "缺 state：{url}");

        let cookie = set_cookie(&headers);
        assert!(
            cookie.starts_with("atp_gh_state=") && !cookie.starts_with("atp_gh_state=;"),
            "state 必须落一张非空 cookie：{cookie:?}"
        );
        assert!(
            cookie.contains("HttpOnly"),
            "state cookie 不能被 JS 读到：{cookie:?}"
        );
        assert!(
            cookie.contains("SameSite=Lax"),
            "state cookie 必须限制跨站：{cookie:?}"
        );

        // 未启用 → 503「未启用」而不是 500
        let off = state_with("authorize_off", None);
        let (s2, _h2, b2) = req_with(off, "GET", "/api/auth/github/authorize", None, None).await;
        assert_eq!(
            s2,
            StatusCode::SERVICE_UNAVAILABLE,
            "未启用必须是 503：{b2}"
        );
    }

    /// state 对不上 → 400，**不换码、不落库、不打上游**
    #[tokio::test]
    async fn a_state_that_does_not_match_its_cookie_buys_no_credentials() {
        let (base, mock) = start_mock(
            vec![json!({"access_token": "gho_x"})],
            json!({"id": 5, "login": "octo"}),
        )
        .await;
        let st = state_with("state_mismatch", Some(&base));
        let (status, _h, body) = req_with(
            st.clone(),
            "POST",
            "/api/auth/github/exchange",
            Some(r#"{"code":"c1","state":"forged"}"#),
            Some("atp_gh_state=real-nonce"),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "state 不匹配必须 400：{body}"
        );
        assert_eq!(body["error"], "github_state_mismatch", "{body}");
        assert!(body.get("api_key").is_none(), "不许发 key：{body}");
        assert_eq!(count(&st, "SELECT COUNT(*) FROM users"), 0, "不许建用户行");
        assert_eq!(
            count(&st, "SELECT COUNT(*) FROM api_keys"),
            0,
            "不许落 key 行"
        );
        assert!(
            mock.seen.lock().unwrap().is_empty(),
            "伪造的 state 不该消耗上游：{:?}",
            mock.seen.lock().unwrap()
        );

        // 阳性对照：同一端点、只把 cookie 换成对得上的值，就必须拿到凭据
        let (s2, _h2, b2) = req_with(
            st.clone(),
            "POST",
            "/api/auth/github/exchange",
            Some(r#"{"code":"c1","state":"nonce-ok"}"#),
            Some("atp_gh_state=nonce-ok"),
        )
        .await;
        assert_eq!(s2, StatusCode::OK, "阳性对照：对得上的 state 必须通：{b2}");
        assert!(
            !b2["api_key"].as_str().unwrap_or_default().is_empty(),
            "{b2}"
        );
    }

    /// 网页流程与设备码流程落到**同一行用户、同一把 key**（验收 5 + 2）
    #[tokio::test]
    async fn the_web_flow_lands_the_same_account_and_key_as_the_device_flow() {
        let (base, _m) = start_mock(
            vec![json!({"access_token": "gho_x"})],
            json!({"id": 42, "login": "octo"}),
        )
        .await;
        let st = state_with("web_flow", Some(&base));
        // 前端就是这么拿 state 的：先要授权地址，服务端同时把 state 写进 cookie
        let (_s, headers, _b) =
            req_with(st.clone(), "GET", "/api/auth/github/authorize", None, None).await;
        let cookie = set_cookie(&headers);
        let pair = cookie
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        let state = pair
            .strip_prefix("atp_gh_state=")
            .expect("cookie 里必须有 state")
            .to_string();
        assert!(!state.is_empty(), "state 不能是空串");

        let (status, h2, body) = req_with(
            st.clone(),
            "POST",
            "/api/auth/github/exchange",
            Some(&format!(r#"{{"code":"c1","state":"{state}"}}"#)),
            Some(&pair),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "网页流程必须换到平台凭据：{body}");
        let web_key = body["api_key"].as_str().unwrap_or_default().to_string();
        let web_uid = body["user_id"].as_i64().unwrap_or_default();
        assert!(
            !web_key.is_empty() && web_uid > 0,
            "字段与邮箱登录同形：{body}"
        );
        assert_eq!(body["login"], "octo", "登录名一并返回：{body}");
        // 用过的 state 立刻作废
        assert!(
            set_cookie(&h2).contains("Max-Age=0"),
            "用过的 state 必须作废：{:?}",
            set_cookie(&h2)
        );

        // 同一 GitHub 身份再从设备码进来：同一账号、同一把 key，两张表各只增一行
        let (s3, b3) = post_json(
            st.clone(),
            "/api/auth/github/device/token",
            r#"{"device_code":"dc-abc"}"#,
        )
        .await;
        assert_eq!(s3, StatusCode::OK, "{b3}");
        assert_eq!(
            b3["api_key"].as_str().unwrap_or_default(),
            web_key,
            "同一 GitHub 身份两次登录必须拿到同一把 key：{b3}"
        );
        assert_eq!(b3["user_id"].as_i64().unwrap_or_default(), web_uid);
        assert_eq!(
            count(&st, "SELECT COUNT(*) FROM users"),
            1,
            "用户行只增一次"
        );
        assert_eq!(
            count(&st, "SELECT COUNT(*) FROM api_keys"),
            1,
            "key 行只增一次"
        );
    }

    /// 登录页真的有这个入口，且中英双语都在：按钮、词表键、前端接线
    #[test]
    fn the_login_page_carries_a_github_sign_in_button() {
        const INDEX: &str = include_str!("../ui/index.html");
        const I18N: &str = include_str!("../ui/js/i18n.js");
        const APP: &str = include_str!("../ui/js/app.js");
        // 阳性对照：同一份语料里已知在场的登录页按钮
        assert!(
            INDEX.contains(r#"id="guest-browse-btn""#),
            "阳性对照失效：语料里连游客入口都没有"
        );
        assert!(
            INDEX.contains(r#"id="github-login-btn""#),
            "登录页缺「使用 GitHub 登录」按钮"
        );
        assert!(
            INDEX.contains(r#"data-i18n="login.github""#),
            "按钮文案必须走词表，不写死中文"
        );
        assert_eq!(
            I18N.matches("\"login.github\":").count(),
            2,
            "词表里 login.github 必须中英各一条（缺一个就是英文模式显示中文）"
        );
        assert!(
            I18N.contains("\"login.github\": \"使用 GitHub 登录\""),
            "缺中文值"
        );
        assert!(
            I18N.contains("\"login.github\": \"Sign in with GitHub\""),
            "缺英文值"
        );
        assert!(
            APP.contains("/api/auth/github/authorize"),
            "前端未取 authorize_url"
        );
        assert!(
            APP.contains("/api/auth/github/exchange"),
            "前端未接回跳换码"
        );
    }
}
