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

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::Json;

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
        .body(axum::body::Body::from(body))
        .expect("build passthrough response")
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
        let balance = dao::get_balance(&conn, auth.user_id);
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

        // 转发上游（anthropic 用 x-api-key，openai 用 Bearer）
        let resp = if protocol == "anthropic" {
            st.http
                .post(&url)
                .header("x-api-key", &key.encrypted_key)
                .header("content-type", "application/json")
                .header("anthropic-version", "2023-06-01")
                .body(body.clone())
                .send()
                .await
        } else {
            st.http
                .post(&url)
                .header("authorization", format!("Bearer {}", key.encrypted_key))
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
            let tokens = input_tokens + output_tokens;
            if tokens > 0.0 {
                let mut conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
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
                    // 入账失败不影响透传（响应已成功），仅记录日志
                    log::error!("计量入账失败 key_id={} user={}: {e}", key.id, auth.user_id);
                }
                drop(conn);
                st.router.mark_sticky(auth.user_id, model, key_id);
            }
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

/// POST /v1/chat/completions（OpenAI 兼容）
#[axum::debug_handler]
pub async fn chat_completions(
    State(st): State<AppState>,
    auth: AuthUser,
    body: String,
) -> Result<Response, ApiErr> {
    let model = extract_model(&body)
        .ok_or_else(|| err_json(StatusCode::BAD_REQUEST, "请求体缺少 model 字段"))?;
    forward(&st, auth, &model, body.clone(), "openai_chat").await
}

/// POST /anthropic/v1/messages（Anthropic 兼容）
pub async fn anthropic_messages(
    State(st): State<AppState>,
    auth: AuthUser,
    body: String,
) -> Result<Response, ApiErr> {
    let model = extract_model(&body)
        .ok_or_else(|| err_json(StatusCode::BAD_REQUEST, "请求体缺少 model 字段"))?;
    forward(&st, auth, &model, body.clone(), "anthropic").await
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
        let mut cfg = crate::config::Config::load("config/config.example.toml").unwrap();
        cfg.plans.push(crate::config::Plan {
            id: plan_id.to_string(),
            provider: "test".to_string(),
            type_: "paygo".to_string(),
            key_prefix: "sk-".to_string(),
            interactive_only: false,
            endpoints: vec![crate::config::Endpoint {
                protocol: "openai_chat".to_string(),
                base_url: base_url.to_string(),
            }],
        });
        crate::db::seed_models(&conn, &cfg).expect("seed models");
        AppState::new(conn, Arc::new(cfg))
    }

    /// 注入测试 key（属主 user_id，模型 model，plan）
    fn insert_key(st: &AppState, id: i64, owner: i64, model: &str, plan: &str) {
        let conn = st.db.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO keys (id, provider, plan, model, status, owner_id, encrypted_key, quota, used) \
             VALUES (?1, 'test', ?2, ?3, 'on', ?4, 'sk-test', 1000, 0)",
            rusqlite::params![id, plan, model, owner],
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
}
