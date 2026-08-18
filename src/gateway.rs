//! 网关兼容端点（architecture §4.1）
//!
//! P0-B（rant 2026-08-18T09:55:57）：
//! - POST /v1/chat/completions（OpenAI 兼容，Bearer atk_ API Key 认证）
//! - POST /anthropic/v1/messages（Anthropic 兼容）
//! - GET  /api/models（市场页：models 表 + key 可用性）
//!
//! 流程：请求体取 model → 路由选 key（粘性/随机/冷却/3 次切换）→
//! reqwest 转发到 plan 对应 base_url（openai_chat → {base}/chat/completions；
//! anthropic → {base}/v1/messages）→ 上游响应原样透传 → 解析 usage → 计量入账。
//! 上游 key 当前为明文占位（加密留 P0-C）。

use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::Json;
use futures_util::StreamExt;

use crate::billing;
use crate::config::Config;
use crate::dao;
use crate::routes::{internal, ApiErr, AppState, AuthUser};

/// 网关错误（OpenAI 兼容格式：{"error":{"message":...}}）
fn err_json(status: StatusCode, msg: &str) -> ApiErr {
    (
        status,
        Json(serde_json::json!({ "error": { "message": msg } })),
    )
}

/// 从请求体提取 model 字段
fn extract_model(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v.get("model")
        .and_then(|m| m.as_str())
        .map(|s| s.to_string())
}

/// 解析上游响应中的 usage（openai: prompt/completion_tokens；anthropic: input/output_tokens）
fn parse_usage(body: &[u8], protocol: &str) -> (f64, f64) {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return (0.0, 0.0);
    };
    let usage = v.get("usage");
    match protocol {
        "anthropic" => {
            let input = usage
                .and_then(|u| u.get("input_tokens"))
                .and_then(|x| x.as_f64())
                .unwrap_or(0.0);
            let output = usage
                .and_then(|u| u.get("output_tokens"))
                .and_then(|x| x.as_f64())
                .unwrap_or(0.0);
            (input, output)
        }
        _ => {
            let input = usage
                .and_then(|u| u.get("prompt_tokens"))
                .and_then(|x| x.as_f64())
                .unwrap_or(0.0);
            let output = usage
                .and_then(|u| u.get("completion_tokens"))
                .and_then(|x| x.as_f64())
                .unwrap_or(0.0);
            (input, output)
        }
    }
}

/// 按 plan + 协议解析上游完整 URL
fn resolve_endpoint(cfg: &Config, plan_id: &str, protocol: &str) -> Option<String> {
    let plan = cfg.plans.iter().find(|p| p.id == plan_id)?;
    let ep = plan.endpoints.iter().find(|e| e.protocol == protocol)?;
    let base = ep.base_url.trim_end_matches('/');
    Some(match protocol {
        "anthropic" => format!("{base}/v1/messages"),
        _ => format!("{base}/chat/completions"),
    })
}

/// 原样透传上游响应
fn passthrough(status: StatusCode, body: Vec<u8>) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .expect("build passthrough response")
}

/// 解密上游 key（P0-C：keys.encrypted_key 为 v1: 密文，转发前解密）
fn decrypt_key(st: &AppState, key: &dao::KeyRow) -> Option<String> {
    match st.crypto.decrypt(&key.encrypted_key) {
        Ok(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
        Err(e) => {
            log::error!("解密上游 key 失败 key_id={}: {e}", key.id);
            None
        }
    }
}

/// 计量入账（非流式/流式共用）：tokens>0 才入账；失败仅记日志不影响透传
fn settle_usage(
    st: &AppState,
    auth: AuthUser,
    key: &dao::KeyRow,
    model: &str,
    input_tokens: f64,
    output_tokens: f64,
) {
    let tokens = input_tokens + output_tokens;
    if tokens <= 0.0 {
        return;
    }
    // 锁作用域严格限定在同步区内（绝不在 await 期间持有 MutexGuard）
    let mut conn = match st.db.lock() {
        Ok(c) => c,
        Err(e) => {
            log::error!("计量入账失败（db lock poisoned）: {e}");
            return;
        }
    };
    let price = dao::get_model_price(&conn, &key.provider, model);
    let (pts, cost) = match price {
        Some((i_per_m, o_per_m, currency)) => {
            let pts = billing::calc_points(
                input_tokens,
                output_tokens,
                i_per_m,
                o_per_m,
                st.cfg.points.points_per_unit,
                &currency,
                &st.cfg.points.anchor_currency,
            );
            let cost = billing::to_anchor(
                billing::raw_cost(input_tokens, output_tokens, i_per_m, o_per_m),
                &currency,
                &st.cfg.points.anchor_currency,
            );
            (pts, cost)
        }
        None => (0.0, 0.0),
    };
    let params = billing::SettleParams {
        consumer_id: auth.user_id,
        api_key_id: Some(auth.api_key_id),
        key_id: key.id,
        owner_id: key.owner_id,
        model: model.to_string(),
        tokens,
        pts,
        cost,
    };
    if let Err(e) = billing::settle(&mut conn, &params) {
        log::error!("计量入账失败 key_id={} user={}: {e}", key.id, auth.user_id);
    }
    drop(conn);
    st.router.mark_sticky(auth.user_id, model, key.id);
}

/// 核心转发逻辑（openai_chat / anthropic 共用）
async fn forward(
    st: &AppState,
    auth: AuthUser,
    model: &str,
    body: String,
    protocol: &str,
) -> Result<Response, ApiErr> {
    // 余额预检（上游调用前）：余额 ≤ 0 → 402
    // 锁作用域严格限定在同步读区内，绝不在 await 期间持有 MutexGuard
    let (balance, keys) = {
        let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
        // P1：懒加载当日赠送（赠送也计入可用余额）；可用余额 = gift + permanent
        let _ = crate::gift::ensure_daily_gift(&conn, auth.user_id);
        let balance = dao::get_available_balance(&conn, auth.user_id);
        let keys = dao::find_keys_by_model(&conn, model).map_err(internal)?;
        (balance, keys)
    };
    if balance <= 0.0 {
        return Err(err_json(StatusCode::PAYMENT_REQUIRED, "点数余额不足"));
    }

    if keys.is_empty() {
        return Err(err_json(
            StatusCode::SERVICE_UNAVAILABLE,
            "该模型暂无可用 key",
        ));
    }

    for _ in 0..crate::router::MAX_SWITCHES {
        // 选 key（粘性优先 / 随机 / 排除冷却）
        let Some(key_id) = st.router.pick(&keys, auth.user_id, model) else {
            return Err(err_json(
                StatusCode::SERVICE_UNAVAILABLE,
                "该模型暂无可用 key",
            ));
        };
        let key = keys
            .iter()
            .find(|k| k.id == key_id)
            .expect("pick 返回的 key 必然在候选集");

        // 解析端点；plan 无对应协议端点 → 视为该 key 不可用
        let Some(url) = resolve_endpoint(&st.cfg, &key.plan, protocol) else {
            st.router.mark_unhealthy(key_id);
            continue;
        };
        // 解密上游 key；解密失败 → 视为该 key 不可用
        let Some(plain_key) = decrypt_key(st, key) else {
            st.router.mark_unhealthy(key_id);
            continue;
        };

        // 转发上游（anthropic 用 x-api-key，openai 用 Bearer）
        let resp = if protocol == "anthropic" {
            st.http
                .post(&url)
                .header("x-api-key", &plain_key)
                .header("content-type", "application/json")
                .header("anthropic-version", "2023-06-01")
                .body(body.clone())
                .send()
                .await
        } else {
            st.http
                .post(&url)
                .header("authorization", format!("Bearer {plain_key}"))
                .header("content-type", "application/json")
                .body(body.clone())
                .send()
                .await
        };

        let resp = match resp {
            Ok(r) => r,
            // 网络错误 → key 非健康，静默切换
            Err(_) => {
                st.router.mark_unhealthy(key_id);
                continue;
            }
        };
        let status = resp.status();
        let bytes = resp.bytes().await.unwrap_or_default().to_vec();

        if status.is_success() {
            // 成功：解析 usage → 计量入账 → 粘性
            let (input_tokens, output_tokens) = parse_usage(&bytes, protocol);
            settle_usage(st, auth, key, model, input_tokens, output_tokens);
            return Ok(passthrough(status, bytes));
        } else if status == StatusCode::UNAUTHORIZED
            || status == StatusCode::FORBIDDEN
            || status == StatusCode::TOO_MANY_REQUESTS
            || status.is_server_error()
        {
            // 401/403/429/5xx → key 非健康，静默切换
            st.router.mark_unhealthy(key_id);
            continue;
        } else {
            // 其它 4xx（400/404 等）→ 用户请求错误，不切换直接透传
            return Ok(passthrough(status, bytes));
        }
    }
    Err(err_json(
        StatusCode::SERVICE_UNAVAILABLE,
        "该模型暂无可用 key",
    ))
}

/// SSE 流式转发（P0-C）：请求体带 stream:true 时走此分支。
///
/// 流程：余额预检 → 路由选 key（初始连接失败可故障转移，最高 3 次）→ 拿到 200 后
/// 逐块透传上游响应体（data: 行原样转发，保持 event/comment 原始格式）→
/// 流尾解析 usage（openai 最后 chunk 的 usage / anthropic message_delta 的
/// output_tokens + message_start 的 input_tokens）→ 复用 settle_usage 入账 →
/// 记粘性。客户端提前断开 → 响应体被 drop → 上游连接自动中止，不入账。
async fn forward_stream(
    st: &AppState,
    auth: AuthUser,
    model: &str,
    body: String,
    protocol: &str,
) -> Result<Response, ApiErr> {
    // 余额预检（与 forward 一致，锁作用域严格块内）
    let (balance, keys) = {
        let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
        // P1：懒加载当日赠送（赠送也计入可用余额）；可用余额 = gift + permanent
        let _ = crate::gift::ensure_daily_gift(&conn, auth.user_id);
        let balance = dao::get_available_balance(&conn, auth.user_id);
        let keys = dao::find_keys_by_model(&conn, model).map_err(internal)?;
        (balance, keys)
    };
    if balance <= 0.0 {
        return Err(err_json(StatusCode::PAYMENT_REQUIRED, "点数余额不足"));
    }
    if keys.is_empty() {
        return Err(err_json(
            StatusCode::SERVICE_UNAVAILABLE,
            "该模型暂无可用 key",
        ));
    }

    // 选 key 并建立上游连接（此阶段失败可切换；连接成功后不再切换）
    for _ in 0..crate::router::MAX_SWITCHES {
        let Some(key_id) = st.router.pick(&keys, auth.user_id, model) else {
            return Err(err_json(
                StatusCode::SERVICE_UNAVAILABLE,
                "该模型暂无可用 key",
            ));
        };
        let key = keys
            .iter()
            .find(|k| k.id == key_id)
            .expect("pick 返回的 key 必然在候选集");

        let Some(url) = resolve_endpoint(&st.cfg, &key.plan, protocol) else {
            st.router.mark_unhealthy(key_id);
            continue;
        };
        let Some(plain_key) = decrypt_key(st, key) else {
            st.router.mark_unhealthy(key_id);
            continue;
        };

        let resp = if protocol == "anthropic" {
            st.http
                .post(&url)
                .header("x-api-key", &plain_key)
                .header("content-type", "application/json")
                .header("anthropic-version", "2023-06-01")
                .body(body.clone())
                .send()
                .await
        } else {
            st.http
                .post(&url)
                .header("authorization", format!("Bearer {plain_key}"))
                .header("content-type", "application/json")
                .body(body.clone())
                .send()
                .await
        };

        let resp = match resp {
            Ok(r) => r,
            Err(_) => {
                st.router.mark_unhealthy(key_id);
                continue;
            }
        };
        let status = resp.status();
        if !status.is_success() {
            // 连接阶段失败 → 与 P0-B 相同的故障转移判定
            if status == StatusCode::UNAUTHORIZED
                || status == StatusCode::FORBIDDEN
                || status == StatusCode::TOO_MANY_REQUESTS
                || status.is_server_error()
            {
                st.router.mark_unhealthy(key_id);
                continue;
            }
            // 其它 4xx → 用户请求错误，读 body 透传
            let bytes = resp.bytes().await.unwrap_or_default().to_vec();
            return Ok(passthrough(status, bytes));
        }

        // 连接成功：构建 SSE 透传流（usage 捕获 + 流尾入账）
        let key = key.clone();
        let st = st.clone();
        let model = model.to_string();
        let protocol = protocol.to_string();
        let capture = std::sync::Arc::new(std::sync::Mutex::new(UsageCapture::new(&protocol)));
        let cap_fwd = std::sync::Arc::clone(&capture);
        let fwd = resp.bytes_stream().map(move |item| {
            if let Ok(bytes) = &item {
                if let Ok(mut cap) = cap_fwd.lock() {
                    cap.push(bytes);
                }
            }
            item.map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })
        });
        // 流尾：解析 usage → 入账（客户端未完整接收时该 future 不会执行 → 不入账）
        let finalize = futures_util::stream::once(async move {
            let (input, output) = {
                let mut cap = capture.lock().expect("usage capture lock");
                cap.finish()
            };
            settle_usage(&st, auth, &key, &model, input, output);
            Ok::<axum::body::Bytes, Box<dyn std::error::Error + Send + Sync>>(
                axum::body::Bytes::new(),
            )
        });
        let body = Body::from_stream(fwd.chain(finalize));
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .header("cache-control", "no-cache")
            .header("x-accel-buffering", "no")
            .body(body)
            .expect("build sse response"));
    }
    Err(err_json(
        StatusCode::SERVICE_UNAVAILABLE,
        "该模型暂无可用 key",
    ))
}

/// SSE 流式 usage 捕获：转发时记录尾部数据（≤64KB）用于流尾解析；
/// anthropic 的 input_tokens 在 message_start（头部）单独提前捕获
struct UsageCapture {
    protocol: String,
    tail: Vec<u8>,
    input_tokens: f64,
}

impl UsageCapture {
    fn new(protocol: &str) -> Self {
        Self {
            protocol: protocol.to_string(),
            tail: Vec::new(),
            input_tokens: 0.0,
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        // anthropic：message_start 事件携带 input_tokens（在流头部，尾部缓冲会丢）
        if self.protocol == "anthropic" && self.input_tokens == 0.0 {
            if let Some(v) = parse_anthropic_input(chunk) {
                self.input_tokens = v;
            }
        }
        self.tail.extend_from_slice(chunk);
        if self.tail.len() > 64 * 1024 {
            self.tail.drain(0..(self.tail.len() - 64 * 1024));
        }
    }

    /// 流尾解析 usage → (input_tokens, output_tokens)
    fn finish(&mut self) -> (f64, f64) {
        let text = String::from_utf8_lossy(&self.tail);
        let mut input = self.input_tokens;
        let mut output = 0.0;
        for line in text.lines() {
            let Some(data) = line.trim_start().strip_prefix("data:") else {
                continue;
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(data.trim()) else {
                continue;
            };
            if self.protocol == "anthropic" {
                if let Some(u) = v.get("usage") {
                    // message_delta / message_start 均带 usage 字段
                    input = u
                        .get("input_tokens")
                        .and_then(|x| x.as_f64())
                        .unwrap_or(input);
                    output = u
                        .get("output_tokens")
                        .and_then(|x| x.as_f64())
                        .unwrap_or(output);
                }
            } else if let Some(u) = v.get("usage") {
                // openai：最后 chunk 的 usage（stream_options.include_usage 时）
                input = u
                    .get("prompt_tokens")
                    .and_then(|x| x.as_f64())
                    .unwrap_or(0.0);
                output = u
                    .get("completion_tokens")
                    .and_then(|x| x.as_f64())
                    .unwrap_or(0.0);
            }
        }
        (input, output)
    }
}

/// 从 anthropic 流式 chunk 提取 input_tokens（message_start 事件）
fn parse_anthropic_input(chunk: &[u8]) -> Option<f64> {
    let text = String::from_utf8_lossy(chunk);
    for line in text.lines() {
        let Some(data) = line.trim_start().strip_prefix("data:") else {
            continue;
        };
        let v: serde_json::Value = serde_json::from_str(data.trim()).ok()?;
        if v.get("type").and_then(|t| t.as_str()) == Some("message_start") {
            if let Some(tokens) = v.pointer("/message/usage/input_tokens") {
                return tokens.as_f64();
            }
        }
    }
    None
}

/// POST /v1/chat/completions（OpenAI 兼容）
#[axum::debug_handler]
pub async fn chat_completions(
    State(st): State<AppState>,
    auth: AuthUser,
    body: String,
) -> Result<Response, ApiErr> {
    let model = extract_model(&body)
        .ok_or_else(|| err_json(StatusCode::BAD_REQUEST, "请求体缺少 model 字段"))?;
    if body_streaming(&body) {
        forward_stream(&st, auth, &model, body.clone(), "openai_chat").await
    } else {
        forward(&st, auth, &model, body.clone(), "openai_chat").await
    }
}

/// POST /anthropic/v1/messages（Anthropic 兼容）
pub async fn anthropic_messages(
    State(st): State<AppState>,
    auth: AuthUser,
    body: String,
) -> Result<Response, ApiErr> {
    let model = extract_model(&body)
        .ok_or_else(|| err_json(StatusCode::BAD_REQUEST, "请求体缺少 model 字段"))?;
    if body_streaming(&body) {
        forward_stream(&st, auth, &model, body.clone(), "anthropic").await
    } else {
        forward(&st, auth, &model, body.clone(), "anthropic").await
    }
}

/// 请求体是否要求流式（stream:true）
fn body_streaming(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("stream").and_then(|s| s.as_bool()))
        .unwrap_or(false)
}

/// GET /api/models（市场页：models 表 + key 可用性，需认证）
pub async fn models(
    State(st): State<AppState>,
    _auth: AuthUser,
) -> Result<Json<Vec<serde_json::Value>>, ApiErr> {
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let list = dao::list_models_with_availability(&conn).map_err(internal)?;
    Ok(Json(list))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::router;
    use axum::body::Body;
    use axum::http::Request;
    use std::sync::Arc;
    use tower::util::ServiceExt;

    /// 测试状态：config.example + 追加本地 test plan；db 开库 + seed models + 注入测试 key
    fn test_state(tag: &str, plan_id: &str, base_url: &str) -> AppState {
        let p = std::env::temp_dir().join(format!("atp_gw_{}_{}.db", std::process::id(), tag));
        let _ = std::fs::remove_file(&p);
        let conn = crate::db::open(p.to_str().unwrap()).expect("open tmp db");
        // demo 注册时间拨到赠送窗口外（2020 年）→ 网关测试不触发每日赠送，
        // 消费扣减断言确定（赠送路径由 gift/routes/billing 测试覆盖）
        conn.execute(
            "UPDATE users SET created_at = '2020-01-01 00:00:00' WHERE id = 1",
            [],
        )
        .unwrap();
        let mut cfg = crate::config::Config::load("config/config.example.toml").unwrap();
        cfg.plans.push(crate::config::Plan {
            id: plan_id.to_string(),
            provider: "test".to_string(),
            type_: "paygo".to_string(),
            key_prefix: "sk-".to_string(),
            interactive_only: false,
            endpoints: vec![
                crate::config::Endpoint {
                    protocol: "openai_chat".to_string(),
                    base_url: base_url.to_string(),
                },
                crate::config::Endpoint {
                    protocol: "anthropic".to_string(),
                    base_url: base_url.to_string(),
                },
            ],
        });
        crate::db::seed_models(&conn, &cfg).expect("seed models");
        let crypto = crate::crypto::Crypto::new([9u8; 32]);
        AppState::new(conn, Arc::new(cfg), crypto)
    }

    /// 注入测试 key（属主 user_id，模型 model，plan；key 值加密落库）
    fn insert_key(st: &AppState, id: i64, owner: i64, model: &str, plan: &str) {
        let encrypted = st.crypto.encrypt(b"sk-test").expect("encrypt test key");
        let conn = st.db.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO keys (id, provider, plan, model, status, owner_id, encrypted_key, quota, used) \
             VALUES (?1, 'test', ?2, ?3, 'on', ?4, ?5, 1000, 0)",
            rusqlite::params![id, plan, model, owner, encrypted],
        )
        .unwrap();
    }

    async fn post_raw(
        st: AppState,
        uri: &str,
        body: &str,
        bearer: Option<&str>,
    ) -> (StatusCode, Vec<u8>) {
        let mut b = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(k) = bearer {
            b = b.header("authorization", format!("Bearer {k}"));
        }
        let resp = router()
            .with_state(st)
            .oneshot(b.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 8 * 1024 * 1024)
            .await
            .unwrap()
            .to_vec();
        (status, bytes)
    }

    async fn login_key(st: AppState) -> String {
        let (_, body) = post_raw(
            st,
            "/api/auth/login",
            r#"{"email":"demo@aitokenpool.local","password":"demo1234"}"#,
            None,
        )
        .await;
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        v["api_key"].as_str().unwrap().to_string()
    }

    /// 假上游：返回固定 usage
    async fn fake_upstream(port: u16) {
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(|_body: String| async {
                Json(serde_json::json!({
                    "id": "cmpl-test",
                    "object": "chat.completion",
                    "model": "test-model",
                    "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"}}],
                    "usage": { "prompt_tokens": 100, "completion_tokens": 50 }
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .unwrap();
        axum::serve(listener, app).await.unwrap();
    }

    fn models_row(
        conn: &rusqlite::Connection,
        provider: &str,
        model: &str,
        input: f64,
        output: f64,
    ) {
        conn.execute(
            "INSERT OR REPLACE INTO models (provider, model, currency, input_per_m, output_per_m) \
             VALUES (?1, ?2, 'USD', ?3, ?4)",
            rusqlite::params![provider, model, input, output],
        )
        .unwrap();
    }

    #[tokio::test]
    async fn no_bearer_401() {
        let st = test_state("noauth", "test-plan", "http://127.0.0.1:9");
        let (s, _) = post_raw(st, "/v1/chat/completions", r#"{"model":"m"}"#, None).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn missing_model_400() {
        let st = test_state("nomodel", "test-plan", "http://127.0.0.1:9");
        let key = login_key(st.clone()).await;
        let (s, _) = post_raw(st, "/v1/chat/completions", r#"{}"#, Some(&key)).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn insufficient_balance_402() {
        let st = test_state("insuf", "test-plan", "http://127.0.0.1:9");
        // demo 余额清零
        {
            let conn = st.db.lock().unwrap();
            conn.execute("UPDATE quotas SET balance = 0 WHERE user_id = 1", [])
                .unwrap();
        }
        let key = login_key(st.clone()).await;
        let (s, body) = post_raw(st, "/v1/chat/completions", r#"{"model":"m"}"#, Some(&key)).await;
        assert_eq!(s, StatusCode::PAYMENT_REQUIRED);
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(v["error"]["message"].as_str().unwrap().contains("余额不足"));
    }

    #[tokio::test]
    async fn dead_upstream_failover_503() {
        // 两个 key 都指向不可达端口 → 3 次尝试全失败 → 503 + 冷却 2 个 key
        let st = test_state("dead", "test-dead", "http://127.0.0.1:9");
        insert_key(&st, 100, 1, "dead-model", "test-dead");
        insert_key(&st, 101, 1, "dead-model", "test-dead");
        let key = login_key(st.clone()).await;
        let (s, body) = post_raw(
            st.clone(),
            "/v1/chat/completions",
            r#"{"model":"dead-model"}"#,
            Some(&key),
        )
        .await;
        assert_eq!(
            s,
            StatusCode::SERVICE_UNAVAILABLE,
            "body: {}",
            String::from_utf8_lossy(&body)
        );
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("暂无可用 key"));
        assert_eq!(st.router.cooldown_len(), 2, "两次失败后两个 key 均进入冷却");
    }

    #[tokio::test]
    async fn e2e_success_ledger_and_sticky() {
        // 起假上游
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let up = tokio::spawn(fake_upstream(port));
        let base = format!("http://127.0.0.1:{port}");

        let st = test_state("e2e", "test-local", &base);
        // 价格行：10/20 USD per M
        {
            let conn = st.db.lock().unwrap();
            models_row(&conn, "test", "test-model", 10.0, 20.0);
            // 属主用户（id=2）+ 两个 key 均属主 2（避免随机选中消费者自有的 key 使
            // 净额断言不确定：消费者始终 -2.0，属主始终 +1.8）
            conn.execute(
                "INSERT OR IGNORE INTO users (id, email, password_hash, name, role) VALUES (2, 'owner@t.local', 'x', '分享者', 'user')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO quotas (user_id, balance) VALUES (2, 0)",
                [],
            )
            .unwrap();
        }
        insert_key(&st, 200, 2, "test-model", "test-local");
        insert_key(&st, 201, 2, "test-model", "test-local");

        let key = login_key(st.clone()).await;

        // 第一次调用 → 成功 + 入账
        let (s, body) = post_raw(
            st.clone(),
            "/v1/chat/completions",
            r#"{"model":"test-model","messages":[{"role":"user","content":"hi"}]}"#,
            Some(&key),
        )
        .await;
        assert_eq!(
            s,
            StatusCode::OK,
            "body: {}",
            String::from_utf8_lossy(&body)
        );
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["usage"]["prompt_tokens"], 100);
        assert_eq!(v["usage"]["completion_tokens"], 50);

        // 入账断言（锁在块内释放，绝不让 MutexGuard 跨 await）
        let used_key: i64 = {
            let conn = st.db.lock().unwrap();
            let bal_c: f64 = conn
                .query_row("SELECT balance FROM quotas WHERE user_id = 1", [], |r| {
                    r.get(0)
                })
                .unwrap();
            // pts = 100×10/1e6 + 50×20/1e6 = 0.002 USD × 1000 = 2.0
            assert!((bal_c - (12471.0 - 2.0)).abs() < 1e-9, "consumer={bal_c}");
            let bal_o: f64 = conn
                .query_row("SELECT balance FROM quotas WHERE user_id = 2", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert!((bal_o - 1.8).abs() < 1e-9, "owner={bal_o}");
            let n_tx: i64 = conn
                .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
                .unwrap();
            assert_eq!(n_tx, 2);
            let n_ur: i64 = conn
                .query_row("SELECT COUNT(*) FROM usage_records", [], |r| r.get(0))
                .unwrap();
            assert_eq!(n_ur, 1);
            conn.query_row(
                "SELECT key_id FROM usage_records ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap()
        };

        // 第二次调用 → 粘性复用同一 key
        let (s2, _) = post_raw(
            st.clone(),
            "/v1/chat/completions",
            r#"{"model":"test-model","messages":[{"role":"user","content":"hi"}]}"#,
            Some(&key),
        )
        .await;
        assert_eq!(s2, StatusCode::OK);
        let used_key2: i64 = {
            let conn = st.db.lock().unwrap();
            conn.query_row(
                "SELECT key_id FROM usage_records ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(used_key, used_key2, "粘性：第二次调用复用同一 key");

        up.abort();
    }

    #[tokio::test]
    async fn models_endpoint_lists_with_availability() {
        let st = test_state("mk", "test-plan", "http://127.0.0.1:9");
        let key = login_key(st.clone()).await;
        let resp = router()
            .with_state(st.clone())
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/models")
                    .header("authorization", format!("Bearer {key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap();
        assert!(!arr.is_empty(), "models 已从 example 文件 seed");
        // 字段结构
        let first = &arr[0];
        assert!(first.get("provider").is_some());
        assert!(first.get("model").is_some());
        assert!(first.get("input_per_m").is_some());
        assert!(first.get("output_per_m").is_some());
        assert!(first.get("available_keys").is_some());
        // 无认证 → 401
        let resp2 = router()
            .with_state(st)
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/models")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::UNAUTHORIZED);
    }

    /// 假上游：SSE 流式（openai 风格，尾部带 usage + [DONE]）
    async fn fake_sse_upstream(listener: tokio::net::TcpListener) {
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(|_body: String| async {
                let body = "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n\
                            data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n\
                            data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":50},\"choices\":[]}\n\n\
                            data: [DONE]\n\n";
                (
                    [("content-type", "text/event-stream")],
                    axum::body::Body::from(body),
                )
            }),
        );
        axum::serve(listener, app).await.unwrap();
    }

    /// 假上游：SSE 流式（anthropic 风格：message_start 带 input，message_delta 带 output）
    async fn fake_anthropic_sse_upstream(listener: tokio::net::TcpListener) {
        let app = axum::Router::new().route(
            "/v1/messages",
            axum::routing::post(|_body: String| async {
                let body = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":80,\"output_tokens\":1}}}\n\n\
                            event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"hi\"}}\n\n\
                            event: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":30}}\n\n\
                            event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
                (
                    [("content-type", "text/event-stream")],
                    axum::body::Body::from(body),
                )
            }),
        );
        axum::serve(listener, app).await.unwrap();
    }

    #[tokio::test]
    async fn sse_openai_stream_passthrough_and_settle() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let up = tokio::spawn(fake_sse_upstream(listener));
        let base = format!("http://127.0.0.1:{port}");

        let st = test_state("sse", "test-sse", &base);
        {
            let conn = st.db.lock().unwrap();
            models_row(&conn, "test", "test-model", 10.0, 20.0);
            conn.execute(
                "INSERT OR IGNORE INTO users (id, email, password_hash, name, role) VALUES (2, 'owner2@t.local', 'x', '分享者', 'user')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO quotas (user_id, balance) VALUES (2, 0)",
                [],
            )
            .unwrap();
        }
        insert_key(&st, 300, 2, "test-model", "test-sse");
        let key = login_key(st.clone()).await;

        let (s, body) = post_raw(
            st.clone(),
            "/v1/chat/completions",
            r#"{"model":"test-model","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
            Some(&key),
        )
        .await;
        assert_eq!(
            s,
            StatusCode::OK,
            "body: {}",
            String::from_utf8_lossy(&body)
        );
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("data: [DONE]"), "SSE 原文透传含 [DONE]");
        assert!(
            text.contains("\"content\":\"hel\"") && text.contains("\"content\":\"lo\""),
            "chunk 逐块透传"
        );
        // 计量：100×10/1e6 + 50×20/1e6 = 0.002 USD × 1000 = 2.0 点
        let conn = st.db.lock().unwrap();
        let bal: f64 = conn
            .query_row("SELECT balance FROM quotas WHERE user_id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!((bal - (12471.0 - 2.0)).abs() < 1e-9, "consumer={bal}");
        let n_ur: i64 = conn
            .query_row("SELECT COUNT(*) FROM usage_records", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n_ur, 1, "流尾 usage 入账一次");
        drop(conn);
        up.abort();
    }

    #[tokio::test]
    async fn sse_anthropic_stream_settle() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let up = tokio::spawn(fake_anthropic_sse_upstream(listener));
        let base = format!("http://127.0.0.1:{port}");

        let st = test_state("ssa", "test-ssa", &base);
        {
            let conn = st.db.lock().unwrap();
            models_row(&conn, "test", "test-model", 10.0, 20.0);
            conn.execute(
                "INSERT OR IGNORE INTO users (id, email, password_hash, name, role) VALUES (2, 'owner3@t.local', 'x', '分享者', 'user')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO quotas (user_id, balance) VALUES (2, 0)",
                [],
            )
            .unwrap();
        }
        insert_key(&st, 301, 2, "test-model", "test-ssa");
        let key = login_key(st.clone()).await;

        let (s, body) = post_raw(
            st.clone(),
            "/anthropic/v1/messages",
            r#"{"model":"test-model","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
            Some(&key),
        )
        .await;
        assert_eq!(
            s,
            StatusCode::OK,
            "body: {}",
            String::from_utf8_lossy(&body)
        );
        let text = String::from_utf8_lossy(&body);
        assert!(
            text.contains("event: message_stop"),
            "anthropic 事件原样透传"
        );
        // 80×10/1e6 + 30×20/1e6 = 0.0014 USD × 1000 = 1.4 点
        let conn = st.db.lock().unwrap();
        let bal: f64 = conn
            .query_row("SELECT balance FROM quotas WHERE user_id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!((bal - (12471.0 - 1.4)).abs() < 1e-9, "consumer={bal}");
        drop(conn);
        up.abort();
    }

    #[tokio::test]
    async fn sse_client_disconnect_skips_settle() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let up = tokio::spawn(fake_sse_upstream(listener));
        let base = format!("http://127.0.0.1:{port}");

        let st = test_state("sse_disc", "test-sse-d", &base);
        {
            let conn = st.db.lock().unwrap();
            models_row(&conn, "test", "test-model", 10.0, 20.0);
        }
        insert_key(&st, 302, 1, "test-model", "test-sse-d");
        let key = login_key(st.clone()).await;

        let resp = router()
            .with_state(st.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {key}"))
                    .body(Body::from(
                        r#"{"model":"test-model","stream":true,"messages":[{"role":"user","content":"hi"}]}"#.to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // 只读一帧就断开（drop body → 上游中止，流尾 finalize 不执行 → 不入账）
        use futures_util::StreamExt;
        let mut stream = resp.into_body().into_data_stream();
        let _first = stream.next().await;
        drop(stream);
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let conn = st.db.lock().unwrap();
        let n_ur: i64 = conn
            .query_row("SELECT COUNT(*) FROM usage_records", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n_ur, 0, "客户端断开后不应入账");
        drop(conn);
        up.abort();
    }

    #[test]
    fn usage_capture_parses_openai_and_anthropic() {
        // openai：usage 在尾部（SSE 事件以换行分隔）
        let mut cap = UsageCapture::new("openai_chat");
        cap.push(
            br#"data: {"choices":[{"delta":{"content":"hi"}}]}"#
                .to_vec()
                .as_slice(),
        );
        cap.push(b"\n\n");
        cap.push(br#"data: {"usage":{"prompt_tokens":10,"completion_tokens":5},"choices":[]}"#);
        cap.push(b"\n\n");
        cap.push(br#"data: [DONE]"#);
        let (i, o) = cap.finish();
        assert_eq!(i, 10.0);
        assert_eq!(o, 5.0);

        // anthropic：input 在头部 message_start，output 在尾部 message_delta
        let mut cap = UsageCapture::new("anthropic");
        cap.push(
            br#"event: message_start
data: {"type":"message_start","message":{"usage":{"input_tokens":80,"output_tokens":1}}}"#,
        );
        cap.push(b"\n\n");
        cap.push(
            br#"event: message_delta
data: {"type":"message_delta","usage":{"output_tokens":30}}"#,
        );
        let (i, o) = cap.finish();
        assert_eq!(i, 80.0, "message_start 的 input_tokens 提前捕获");
        assert_eq!(o, 30.0, "message_delta 的 output_tokens 流尾解析");
    }
}
