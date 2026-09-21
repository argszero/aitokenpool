//! 网关兼容端点（architecture §6 API 一览）
//!
//! P0-B（rant 2026-08-18T09:55:57）：
//! - POST /v1/chat/completions（OpenAI 兼容，Bearer atk_ API Key 认证）
//! - POST /anthropic/v1/messages（Anthropic 兼容）
//! - GET  /api/models（市场页：models 表 + key 可用性）
//!
//! P3-A（rant 2026-08-18T16:15:42）：
//! - POST /v1/responses（OpenAI Responses 兼容，新增）
//! - 三协议互转：入站 openai_chat / anthropic / responses 可调用只暴露
//!   其他协议端点的 plan（协议自动转换，见 src/protocol.rs）；出站协议选择
//!   同协议优先 → anthropic → openai_chat → responses；跨协议才转换（同协议透传零损耗）。
//! - 流式 SSE 跨协议转换（P3-B）：stream:true 的跨协议请求同样转换，同协议透传。
//!   转换器清单、以及尚未覆盖的协议对（会明确返回 400），以 src/sse.rs 与
//!   forward_stream 的转换器分派为准，此处不重复列举。
//!
//! 流程：请求体取 model → 路由选 key（粘性/随机/冷却/3 次切换）→
//! 按 plan 可用端点定出站协议（需要时转换请求体）→ reqwest 转发到 plan 对应
//! base_url（openai_chat → {base}/chat/completions；anthropic → {base}/v1/messages；
//! responses → {base}/responses）→ 上游响应（需要时转换回入站协议）→
//! 解析 usage（按上游协议）→ 计量入账。

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

/// 解析上游响应中的 usage → (uncached_input_tokens, cached_tokens, output_tokens)
///（rant 2026-08-20T10:17:27：输入区分「缓存命中/未命中」——
///  openai_chat → usage.prompt_cache_hit_tokens（DeepSeek 原生顶层拼写）
///                或 usage.prompt_tokens_details.cached_tokens（OpenAI 拼写）
///                （prompt_tokens 含命中，需减除；优先级同 `UsageCapture::finish` / `sse.rs`）；
///  anthropic  → usage.cache_read_input_tokens（input_tokens 已不含命中，不减）；
///  responses  → usage.input_tokens_details.cached_tokens（input_tokens 含命中，需减除））
fn parse_usage(body: &[u8], protocol: &str) -> (f64, f64, f64) {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return (0.0, 0.0, 0.0);
    };
    let usage = v.get("usage");
    match protocol {
        "anthropic" => {
            let input = usage
                .and_then(|u| u.get("input_tokens"))
                .and_then(|x| x.as_f64())
                .unwrap_or(0.0);
            let cached = usage
                .and_then(|u| u.get("cache_read_input_tokens"))
                .and_then(|x| x.as_f64())
                .unwrap_or(0.0);
            let output = usage
                .and_then(|u| u.get("output_tokens"))
                .and_then(|x| x.as_f64())
                .unwrap_or(0.0);
            (input, cached, output)
        }
        "responses" => {
            let total_input = usage
                .and_then(|u| u.get("input_tokens"))
                .and_then(|x| x.as_f64())
                .unwrap_or(0.0);
            let cached = usage
                .and_then(|u| u.pointer("/input_tokens_details/cached_tokens"))
                .and_then(|x| x.as_f64())
                .unwrap_or(0.0);
            let output = usage
                .and_then(|u| u.get("output_tokens"))
                .and_then(|x| x.as_f64())
                .unwrap_or(0.0);
            (total_input - cached, cached, output)
        }
        _ => {
            let total_input = usage
                .and_then(|u| u.get("prompt_tokens"))
                .and_then(|x| x.as_f64())
                .unwrap_or(0.0);
            // cached 两拼写兼容（同 `UsageCapture::finish` / `sse.rs::extract_cache_read_tokens`，
            // rant 2026-08-23T14:05:02）：DeepSeek 原生顶层 → OpenAI details
            let cached = usage
                .and_then(|u| u.get("prompt_cache_hit_tokens"))
                .and_then(|x| x.as_f64())
                .or_else(|| {
                    usage
                        .and_then(|u| u.pointer("/prompt_tokens_details/cached_tokens"))
                        .and_then(|x| x.as_f64())
                })
                .unwrap_or(0.0);
            let output = usage
                .and_then(|u| u.get("completion_tokens"))
                .and_then(|x| x.as_f64())
                .unwrap_or(0.0);
            (total_input - cached, cached, output)
        }
    }
}

/// 按 plan + 出站协议解析上游完整 URL（P3-A：新增 responses → {base}/responses）
fn resolve_endpoint(cfg: &Config, plan_id: &str, protocol: &str) -> Option<String> {
    let plan = cfg.plans.iter().find(|p| p.id == plan_id)?;
    let ep = plan.endpoints.iter().find(|e| e.protocol == protocol)?;
    let base = ep.base_url.trim_end_matches('/');
    Some(match protocol {
        "anthropic" => format!("{base}/v1/messages"),
        "responses" => format!("{base}/responses"),
        _ => format!("{base}/chat/completions"),
    })
}

/// 按入站协议 + plan 可用端点定出站协议（P3-A）：
/// 同协议端点优先；无同协议 → anthropic → openai_chat → responses 优先级选可用；
/// 全不可用 → None（该 key 不可用，走故障转移）。
fn resolve_outbound<'a>(cfg: &Config, plan_id: &str, inbound: &'a str) -> Option<&'a str> {
    let plan = cfg.plans.iter().find(|p| p.id == plan_id)?;
    let protocols: Vec<String> = plan.endpoints.iter().map(|e| e.protocol.clone()).collect();
    crate::protocol::determine_forwarding_protocol(&protocols, inbound)
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
    cached_tokens: f64,
    output_tokens: f64,
) {
    // 锁作用域严格限定在同步区内（绝不在 await 期间持有 MutexGuard）
    let tokens = input_tokens + cached_tokens + output_tokens;
    if tokens <= 0.0 {
        return;
    }
    let mut conn = match st.db.lock() {
        Ok(c) => c,
        Err(e) => {
            log::error!("计量入账失败（db lock poisoned）: {e}");
            return;
        }
    };
    let price = dao::get_model_price(&conn, &key.provider, model);
    let (pts, cost) = match price {
        Some((i_per_m, o_per_m, cache_hit_per_m, p_in, p_out, p_cache, currency)) => {
            // rant 2026-08-20T11:58:40：DeepSeek 高峰时段（北京 9-12/14-18）价格翻倍——
            // 按当前北京时间选高峰/空闲价；未配置高峰价的模型不受影响
            let peak = billing::is_peak_hour(&chrono::Utc::now());
            let (i2, c2, o2) = billing::effective_prices(
                peak,
                i_per_m,
                cache_hit_per_m,
                o_per_m,
                p_in,
                p_cache,
                p_out,
            );
            let pts = billing::calc_points(
                input_tokens,
                cached_tokens,
                output_tokens,
                i2,
                c2,
                o2,
                st.cfg.points.points_per_unit,
                &currency,
                &st.cfg.points.anchor_currency,
            );
            let cost = billing::to_anchor(
                billing::raw_cost(input_tokens, cached_tokens, output_tokens, i2, c2, o2),
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
        cached_tokens,
        output_tokens,
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

        // P3-A：定出站协议（同协议优先；跨协议按 anthropic → openai_chat → responses 降级）
        let Some(outbound) = resolve_outbound(&st.cfg, &key.plan, protocol) else {
            // plan 无任何可用协议端点 → 视为该 key 不可用
            st.router.mark_unhealthy(key_id);
            continue;
        };
        let needs_transform = outbound != protocol;

        // 解析出站端点；无对应协议端点 → 视为该 key 不可用
        let Some(url) = resolve_endpoint(&st.cfg, &key.plan, outbound) else {
            st.router.mark_unhealthy(key_id);
            continue;
        };
        // 解密上游 key；解密失败 → 视为该 key 不可用
        let Some(plain_key) = decrypt_key(st, key) else {
            st.router.mark_unhealthy(key_id);
            continue;
        };

        // 跨协议 → 转换请求体（入站协议 → 出站协议）；同协议原样透传
        let up_body = if needs_transform {
            match serde_json::from_str::<serde_json::Value>(&body) {
                Ok(v) => {
                    let transformed = crate::protocol::transform_request(&v, protocol, outbound);
                    match serde_json::to_string(&transformed) {
                        Ok(s) => s,
                        Err(_) => body.clone(),
                    }
                }
                Err(_) => body.clone(),
            }
        } else {
            body.clone()
        };

        // 转发上游（anthropic 用 x-api-key，openai/responses 用 Bearer）
        let resp = if outbound == "anthropic" {
            st.http
                .post(&url)
                .header("x-api-key", &plain_key)
                .header("content-type", "application/json")
                .header("anthropic-version", "2023-06-01")
                .body(up_body)
                .send()
                .await
        } else {
            st.http
                .post(&url)
                .header("authorization", format!("Bearer {plain_key}"))
                .header("content-type", "application/json")
                .body(up_body)
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
        // 响应体读取失败**不得**折叠成空 body：状态行只说明响应头，不说明 body 到达。
        // 2xx + 空 body 会以 `200 OK` + `application/json` 交给客户端（不是任何客户端能
        // 解析的成功），且 parse_usage 得到 (0,0,0) ⇒ settle_usage 提前返回，该次调用
        // 从账本与运营计数（month_calls = COUNT(*) FROM usage_records）中同时消失 —— 而
        // 上游已真实消耗 token。缺数据 != 数据为 0（rant 2026-08-23T14:05:02 同款口径）。
        // 非 2xx 沿用原分流（401/403/429/5xx 换 key、其它 4xx 透传），仍按空体处理。
        let bytes = match resp.bytes().await {
            Ok(b) => b.to_vec(),
            Err(e) => {
                log::error!(
                    "上游响应体读取失败 key_id={} status={}: {e}",
                    key.id,
                    status
                );
                if status.is_success() {
                    return Err(err_json(
                        StatusCode::BAD_GATEWAY,
                        "上游响应读取失败（响应体未完整到达）",
                    ));
                }
                Vec::new()
            }
        };

        if status.is_success() {
            // 成功：解析 usage（按上游出站协议）→ 计量入账 → 粘性
            let (input_tokens, cached_tokens, output_tokens) = parse_usage(&bytes, outbound);
            settle_usage(
                st,
                auth,
                key,
                model,
                input_tokens,
                cached_tokens,
                output_tokens,
            );
            // 跨协议 → 转换响应体回入站协议
            if needs_transform {
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    let transformed = crate::protocol::transform_response(&v, outbound, protocol);
                    if let Ok(new_bytes) = serde_json::to_vec(&transformed) {
                        return Ok(passthrough(status, new_bytes));
                    }
                }
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

/// SSE 响应流（P3-B：透传或协议转换后的字节流）
type SseStream = std::pin::Pin<
    Box<dyn futures_util::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send>,
>;

/// 出站流式请求体：为 openai_chat 上游主动打开 usage 上报（否则该协议下流内
/// 根本没有 usage，见 `UsageCapture` 注释），同协议透传与跨协议转换两条路径共用。
///
/// - 只看**出站**协议：anthropic 的 `message_start` 与 responses 的
///   `response.completed` 恒带 usage，无需（也无处）请求
/// - 客户端已显式设置 `stream_options.include_usage` 时保留其值（含 `false`）
/// - body 非法 JSON（服务端兜底透传）时原样返回，不阻断转发
fn with_include_usage(body: &str, outbound: &str) -> String {
    if outbound != "openai_chat" {
        return body.to_string();
    }
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(body) else {
        return body.to_string();
    };
    let Some(obj) = v.as_object_mut() else {
        return body.to_string();
    };
    match obj.get_mut("stream_options") {
        Some(so) => {
            if let Some(so) = so.as_object_mut() {
                so.entry("include_usage").or_insert(serde_json::json!(true));
            }
        }
        None => {
            obj.insert(
                "stream_options".to_string(),
                serde_json::json!({ "include_usage": true }),
            );
        }
    }
    serde_json::to_string(&v).unwrap_or_else(|_| body.to_string())
}

/// SSE 流式转发（P0-C 透传 + P3-B 跨协议转换）：请求体带 stream:true 时走此分支。
///
/// 流程：余额预检 → 路由选 key（初始连接失败可故障转移，最高 3 次）→ 拿到 200 后
/// 逐块转发上游响应体；跨协议时（出站协议 ≠ 入站协议）把上游 SSE 流经 src/sse.rs
/// 转换器转为客户端协议 SSE 流（P3-B）→ 流内提取 usage → 复用 settle_usage 入账 →
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

        // P3-B：定出站协议（同协议优先；跨协议按 anthropic → openai_chat → responses 降级）
        let Some(outbound) = resolve_outbound(&st.cfg, &key.plan, protocol) else {
            st.router.mark_unhealthy(key_id);
            continue;
        };
        let needs_transform = outbound != protocol;

        let Some(url) = resolve_endpoint(&st.cfg, &key.plan, outbound) else {
            st.router.mark_unhealthy(key_id);
            continue;
        };
        let Some(plain_key) = decrypt_key(st, key) else {
            st.router.mark_unhealthy(key_id);
            continue;
        };

        // 跨协议 → 转换请求体（P3-A transform_request 已处理 stream:true）；同协议原样透传
        // 两条路径都补 openai_chat 出站的 usage 上报开关
        let up_body = if needs_transform {
            match serde_json::from_str::<serde_json::Value>(&body) {
                Ok(v) => {
                    let transformed = crate::protocol::transform_request(&v, protocol, outbound);
                    match serde_json::to_string(&transformed) {
                        Ok(s) => with_include_usage(&s, outbound),
                        Err(_) => with_include_usage(&body, outbound),
                    }
                }
                Err(_) => with_include_usage(&body, outbound),
            }
        } else {
            with_include_usage(&body, outbound)
        };

        let resp = if outbound == "anthropic" {
            st.http_stream
                .post(&url)
                .header("x-api-key", &plain_key)
                .header("content-type", "application/json")
                .header("anthropic-version", "2023-06-01")
                .body(up_body)
                .send()
                .await
        } else {
            st.http_stream
                .post(&url)
                .header("authorization", format!("Bearer {plain_key}"))
                .header("content-type", "application/json")
                .body(up_body)
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

        // 连接成功：构建 SSE 流（同协议透传 / 跨协议转换），流尾 usage 入账
        let key = key.clone();
        let st = st.clone();
        let model = model.to_string();
        let protocol = protocol.to_string();
        let outbound = outbound.to_string();

        let (fwd, finalize): (SseStream, SseStream) = if needs_transform {
            // P3-B：responses 上游 → anthropic 客户端暂未实现，明确报错
            if outbound == "responses" && protocol == "anthropic" {
                return Err(err_json(
                    StatusCode::BAD_REQUEST,
                    "responses 上游流式转换到 anthropic 客户端暂未支持（P3-B 延后）",
                ));
            }
            let usage_slot = crate::sse::usage_slot();
            let raw = resp
                .bytes_stream()
                .map(|item| item.map_err(std::io::Error::other));
            let converted: SseStream = match (outbound.as_str(), protocol.as_str()) {
                ("openai_chat", "anthropic") => {
                    Box::pin(crate::sse::openai_sse_to_anthropic(raw, usage_slot.clone()))
                }
                ("openai_chat", "responses") => Box::pin(
                    crate::sse::openai_sse_to_openai_responses(raw, usage_slot.clone()),
                ),
                ("anthropic", "openai_chat") => {
                    Box::pin(crate::sse::anthropic_sse_to_openai(raw, usage_slot.clone()))
                }
                ("anthropic", "responses") => Box::pin(crate::sse::anthropic_sse_to_responses(
                    raw,
                    usage_slot.clone(),
                )),
                ("responses", "openai_chat") => Box::pin(crate::sse::responses_sse_to_openai_chat(
                    raw,
                    usage_slot.clone(),
                )),
                (u, c) => {
                    return Err(err_json(
                        StatusCode::BAD_REQUEST,
                        &format!("流式协议转换 {u} → {c} 暂未支持"),
                    ));
                }
            };
            let slot_final = usage_slot.clone();
            let finalize = futures_util::stream::once(async move {
                // None = 转换器从未记录 usage（与「记录到 0」不同），同下面同协议路径的
                // `usage_seen`：本次调用无计量依据，留痕但不估算
                let slot = slot_final.lock().ok().and_then(|s| *s);
                if slot.is_none() {
                    log::error!(
                        "流式响应（跨协议）未上报 usage，本次调用无法计量（未入账）: key_id={} model={} protocol={}",
                        key.id,
                        model,
                        protocol
                    );
                }
                let (input, cached, output) = slot.unwrap_or((0.0, 0.0, 0.0));
                settle_usage(&st, auth, &key, &model, input, cached, output);
                Ok::<axum::body::Bytes, std::io::Error>(axum::body::Bytes::new())
            });
            (converted, Box::pin(finalize))
        } else {
            // 同协议透传：usage 捕获 + 流尾入账
            let capture = std::sync::Arc::new(std::sync::Mutex::new(UsageCapture::new(&protocol)));
            let cap_fwd = std::sync::Arc::clone(&capture);
            let fwd = resp.bytes_stream().map(move |item| {
                if let Ok(bytes) = &item {
                    if let Ok(mut cap) = cap_fwd.lock() {
                        cap.push(bytes);
                    }
                }
                item.map_err(|e| -> std::io::Error { std::io::Error::other(e) })
            });
            let finalize = futures_util::stream::once(async move {
                let (input, cached, output, seen) = {
                    let mut cap = capture.lock().expect("usage capture lock");
                    let (i, c, o) = cap.finish();
                    (i, c, o, cap.usage_seen)
                };
                if !seen {
                    // 未入账且**非**「上游报了 0」——本次调用没有任何计量依据。
                    // 记录以便发现：不做估算（新计价机制，非修复），也不改客户端可见行为。
                    log::error!(
                        "流式响应未上报 usage，本次调用无法计量（未入账）: key_id={} model={} protocol={}",
                        key.id,
                        model,
                        protocol
                    );
                }
                settle_usage(&st, auth, &key, &model, input, cached, output);
                Ok::<axum::body::Bytes, std::io::Error>(axum::body::Bytes::new())
            });
            (Box::pin(fwd), Box::pin(finalize))
        };
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
    cached_tokens: f64,
    /// 是否**见到过** usage 对象
    ///
    /// 用于区分「上游报了 0」与「上游什么都没报」——两者在
    /// `(input, cached, output)` 三元组上都退化为全 0，而后者意味着本次调用
    /// **完全没有计量依据**（openai 协议下 usage 需 `stream_options.include_usage`
    /// 才下发，见 `with_include_usage`）。与 `Ok(None)` 槽同源：语义是
    /// 「没有数据」，不是「数据为 0」。
    usage_seen: bool,
}

impl UsageCapture {
    fn new(protocol: &str) -> Self {
        Self {
            protocol: protocol.to_string(),
            tail: Vec::new(),
            input_tokens: 0.0,
            cached_tokens: 0.0,
            usage_seen: false,
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        // anthropic：message_start 事件携带 input_tokens / cache_read_input_tokens
        //（在流头部，尾部缓冲会丢）
        if self.protocol == "anthropic" && self.input_tokens == 0.0 {
            if let Some((input, cached)) = parse_anthropic_usage(chunk) {
                self.input_tokens = input;
                self.cached_tokens = cached;
                self.usage_seen = true;
            }
        }
        self.tail.extend_from_slice(chunk);
        if self.tail.len() > 64 * 1024 {
            self.tail.drain(0..(self.tail.len() - 64 * 1024));
        }
    }

    /// 流尾解析 usage → (input_tokens, cached_tokens, output_tokens)
    fn finish(&mut self) -> (f64, f64, f64) {
        let text = String::from_utf8_lossy(&self.tail);
        let mut input = self.input_tokens;
        let mut cached = self.cached_tokens;
        let mut output = 0.0;
        for line in text.lines() {
            let Some(data) = line.trim_start().strip_prefix("data:") else {
                continue;
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(data.trim()) else {
                continue;
            };
            // 任意协议：只要出现 usage 对象即视为「有数据」（值全 0 也算）
            if v.get("usage").is_some() {
                self.usage_seen = true;
            }
            if self.protocol == "anthropic" {
                if let Some(u) = v.get("usage") {
                    // message_delta / message_start 均带 usage 字段
                    input = u
                        .get("input_tokens")
                        .and_then(|x| x.as_f64())
                        .unwrap_or(input);
                    cached = u
                        .get("cache_read_input_tokens")
                        .and_then(|x| x.as_f64())
                        .unwrap_or(cached);
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
                // cached 三拼写兼容（rant 2026-08-23T14:05:02）：DeepSeek 原生顶层 → OpenAI details
                cached = u
                    .get("prompt_cache_hit_tokens")
                    .and_then(|x| x.as_f64())
                    .or_else(|| {
                        u.pointer("/prompt_tokens_details/cached_tokens")
                            .and_then(|x| x.as_f64())
                    })
                    .unwrap_or(0.0);
                output = u
                    .get("completion_tokens")
                    .and_then(|x| x.as_f64())
                    .unwrap_or(0.0);
            }
        }
        // input disjoint：input_tokens/prompt_tokens 含缓存命中部分，扣除后避免重复计费
        //（同 sse.rs 已修复逻辑，rant 2026-08-23T14:05:02；max 防下溢）
        ((input - cached).max(0.0), cached, output)
    }
}

/// 从 anthropic 流式 chunk 提取 message_start 的 usage → (input_tokens, cache_read_input_tokens)
fn parse_anthropic_usage(chunk: &[u8]) -> Option<(f64, f64)> {
    let text = String::from_utf8_lossy(chunk);
    for line in text.lines() {
        let Some(data) = line.trim_start().strip_prefix("data:") else {
            continue;
        };
        let v: serde_json::Value = serde_json::from_str(data.trim()).ok()?;
        if v.get("type").and_then(|t| t.as_str()) == Some("message_start") {
            let input = v
                .pointer("/message/usage/input_tokens")
                .and_then(|t| t.as_f64())
                .unwrap_or(0.0);
            let cached = v
                .pointer("/message/usage/cache_read_input_tokens")
                .and_then(|t| t.as_f64())
                .unwrap_or(0.0);
            return Some((input, cached));
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

/// POST /v1/responses（OpenAI Responses 兼容，P3-A 新增）
pub async fn responses(
    State(st): State<AppState>,
    auth: AuthUser,
    body: String,
) -> Result<Response, ApiErr> {
    let model = extract_model(&body)
        .ok_or_else(|| err_json(StatusCode::BAD_REQUEST, "请求体缺少 model 字段"))?;
    if body_streaming(&body) {
        forward_stream(&st, auth, &model, body.clone(), "responses").await
    } else {
        forward(&st, auth, &model, body.clone(), "responses").await
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

/// GET /api/plans（上架表单数据源：config [[plans]] 单一真源，需认证）
/// 返回 id / provider / name / type / endpoints。
///
/// `name` 是 config 的**原值**（config 未写 `name` 就是空串）：显示文案归客户端语言包。
/// 后端曾在这里按 `type` 自造显示名（`API（按量）` / `Token Plan` / `Coding Plan`），
/// 而 `en` 界面把响应**数据**字段原样渲染（`I18n.mapErr` 只翻译 `error` 字段）⇒
/// 英文界面上出现中文（C2133）。空串是语言中性标记，前端用 `planLabel()` 按 `type` 取键。
pub async fn plans(
    State(st): State<AppState>,
    _auth: AuthUser,
) -> Result<Json<Vec<serde_json::Value>>, ApiErr> {
    let list: Vec<serde_json::Value> = st
        .cfg
        .plans
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "provider": p.provider,
                "name": p.name,
                "type": p.type_,
                "interactive_only": p.interactive_only,
                "endpoints": p.endpoints.iter().map(|e| serde_json::json!({
                    "protocol": e.protocol,
                    "base_url": e.base_url,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(Json(list))
}

/// GET /v1/models（OpenAI 兼容模型列表，rant 2026-08-18T18:10:18）
///
/// 认证可选：无 token → 纯 OpenAI 列表；带有效 Bearer → data[] 附加 available_keys。
/// 与 /api/models 的关系：/api/models 保留（市场页专用，含价格 input_per_m/output_per_m）；
/// /v1/models 为 OpenAI 标准格式（OpenAI SDK / 工具用）。
pub async fn v1_models(
    State(st): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    // 可选认证：header 有合法 Bearer → 附加 available_keys；无/非法 → 纯列表
    let with_availability = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|key| dao::find_api_key_user_and_id(&conn, key).is_some())
        .unwrap_or(false);
    let data = dao::list_models_openai(&conn, with_availability).map_err(internal)?;
    Ok(Json(serde_json::json!({ "object": "list", "data": data })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::router;
    use axum::body::Body;
    use axum::http::Request;
    use std::sync::Arc;
    use tower::util::ServiceExt;

    // ── parse_usage：缓存命中/未命中拆分（rant 2026-08-20T10:17:27）──

    #[test]
    fn parse_usage_openai_splits_cached_tokens() {
        let body = br#"{"usage":{"prompt_tokens":1000,"completion_tokens":50,"prompt_tokens_details":{"cached_tokens":900}}}"#;
        let (input, cached, output) = parse_usage(body, "openai_chat");
        assert_eq!(input, 100.0, "未命中 = prompt - cached");
        assert_eq!(cached, 900.0);
        assert_eq!(output, 50.0);
    }

    #[test]
    fn parse_usage_anthropic_cache_read() {
        // anthropic input_tokens 已不含命中，原样返回
        let body =
            br#"{"usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":900}}"#;
        let (input, cached, output) = parse_usage(body, "anthropic");
        assert_eq!(input, 100.0);
        assert_eq!(cached, 900.0);
        assert_eq!(output, 50.0);
    }

    #[test]
    fn parse_usage_responses_input_tokens_details() {
        let body = br#"{"usage":{"input_tokens":1000,"output_tokens":50,"input_tokens_details":{"cached_tokens":700}}}"#;
        let (input, cached, output) = parse_usage(body, "responses");
        assert_eq!(input, 300.0, "未命中 = input_tokens - cached");
        assert_eq!(cached, 700.0);
        assert_eq!(output, 50.0);
    }

    #[test]
    fn parse_usage_missing_cache_defaults_zero() {
        let body = br#"{"usage":{"prompt_tokens":100,"completion_tokens":10}}"#;
        let (input, cached, output) = parse_usage(body, "openai_chat");
        assert_eq!(input, 100.0);
        assert_eq!(cached, 0.0);
        assert_eq!(output, 10.0);
    }

    /// rant 2026-08-23T08:20:38 / 2026-08-23T14:05:02：DeepSeek 原生顶层缓存拼写
    /// `prompt_cache_hit_tokens` 对**非流式**响应同样必须被识别（此前只有流式的
    /// `UsageCapture::finish` 与 `sse.rs` 认它，非流式按未命中全价计费 → 多收 5.5x）。
    #[test]
    fn parse_usage_openai_deepseek_native_cache_spelling() {
        // DeepSeek 原生顶层拼写：prompt_tokens 含命中，仍需减除
        let body = br#"{"usage":{"prompt_tokens":1000,"completion_tokens":50,"prompt_cache_hit_tokens":900}}"#;
        let (input, cached, output) = parse_usage(body, "openai_chat");
        assert_eq!(input, 100.0, "未命中 = prompt_tokens - 命中");
        assert_eq!(cached, 900.0, "DeepSeek 原生拼写必须被识别");
        assert_eq!(output, 50.0);

        // 阳性对照：OpenAI 拼写（`prompt_tokens_details.cached_tokens`）不受影响
        let body = br#"{"usage":{"prompt_tokens":1000,"completion_tokens":50,"prompt_tokens_details":{"cached_tokens":900}}}"#;
        let (input, cached, output) = parse_usage(body, "openai_chat");
        assert_eq!(input, 100.0);
        assert_eq!(cached, 900.0);
        assert_eq!(output, 50.0);

        // 两拼写并存 → 与流式同优先级：DeepSeek 原生优先
        let body = br#"{"usage":{"prompt_tokens":1000,"completion_tokens":50,"prompt_cache_hit_tokens":900,"prompt_tokens_details":{"cached_tokens":800}}}"#;
        let (input, cached, output) = parse_usage(body, "openai_chat");
        assert_eq!(input, 100.0, "未命中 = 1000 - 900");
        assert_eq!(
            cached, 900.0,
            "双拼写并存时 DeepSeek 原生优先（同 UsageCapture）"
        );
        assert_eq!(output, 50.0);
    }

    /// 同一条 usage 无论走流式 `UsageCapture::finish` 还是非流式 `parse_usage`，
    /// 必须解析出**完全相同**的三元组。C2037 修了流式的「无 usage」分支，
    /// C2039 修的是非流式的缓存拼写分支 —— 这条断言把两者钉在一起，防止再次分叉。
    #[test]
    fn usage_parsers_agree_on_cache_spelling() {
        let cases: [(&str, (f64, f64, f64)); 3] = [
            // DeepSeek 原生顶层拼写（此前非流式漏认）
            (
                r#"{"usage":{"prompt_tokens":1000,"completion_tokens":50,"prompt_cache_hit_tokens":900}}"#,
                (100.0, 900.0, 50.0),
            ),
            // OpenAI 拼写
            (
                r#"{"usage":{"prompt_tokens":1000,"completion_tokens":50,"prompt_tokens_details":{"cached_tokens":900}}}"#,
                (100.0, 900.0, 50.0),
            ),
            // 无任何缓存字段 → 全部按未命中（缺数据即 0，并非 C2037 的「无 usage」）
            (
                r#"{"usage":{"prompt_tokens":1000,"completion_tokens":50}}"#,
                (1000.0, 0.0, 50.0),
            ),
        ];
        for (body, expected) in cases {
            let non_stream = parse_usage(body.as_bytes(), "openai_chat");
            let mut cap = UsageCapture::new("openai_chat");
            cap.push(format!("data: {body}\n\n").as_bytes());
            let stream = cap.finish();
            assert_eq!(non_stream, expected, "非流式解析 {body}");
            assert_eq!(stream, expected, "流式解析 {body}");
            assert_eq!(non_stream, stream, "两条解析路径对同一 usage 必须一致");
        }
    }

    /// 测试状态：config.example + 追加本地 test plan；db 开库 + seed models + 注入测试 key
    fn test_state(tag: &str, plan_id: &str, base_url: &str) -> AppState {
        test_state_eps(tag, plan_id, &["openai_chat", "anthropic"], base_url)
    }

    /// 测试状态（P3-A）：可指定 plan 只暴露部分协议端点（验证协议转换路径）
    fn test_state_eps(tag: &str, plan_id: &str, protocols: &[&str], base_url: &str) -> AppState {
        let p = std::env::temp_dir().join(format!("atp_gw_{}_{}.db", std::process::id(), tag));
        let _ = std::fs::remove_file(&p);
        let conn = crate::db::open(p.to_str().unwrap()).expect("open tmp db");
        crate::db::seed_test_users(&conn).expect("seed test users");
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
            name: String::new(),
            type_: "paygo".to_string(),
            interactive_only: false,
            endpoints: protocols
                .iter()
                .map(|p| crate::config::Endpoint {
                    protocol: p.to_string(),
                    base_url: base_url.to_string(),
                })
                .collect(),
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

    /// 请求体上限（rant 2026-09-18T09:14:18）：三条网关路由必须收得下 > 2 MiB 的请求体。
    ///
    /// 改前：`axum-core` 的 `DEFAULT_LIMIT = 2_097_152` 生效 —— `main.rs:239` 那层
    /// `RequestBodyLimitLayer(70MB)` 抬不动它（它不写 `DefaultBodyLimitKind` 扩展），
    /// 3 MiB 的体在提取器里就被拒成 413；改后：per-route `DefaultBodyLimit::max` 抬到 8 MiB。
    #[tokio::test]
    async fn gateway_routes_accept_bodies_past_the_default_limit() {
        for (tag, uri) in [
            ("bl_chat", "/v1/chat/completions"),
            ("bl_anth", "/anthropic/v1/messages"),
            ("bl_resp", "/v1/responses"),
        ] {
            let st = test_state(tag, "test-plan", "http://127.0.0.1:9");
            let key = login_key(st.clone()).await;
            let pad = "a".repeat(3 * 1024 * 1024);
            let body = format!(r#"{{"model":"no-such-model","pad":"{pad}"}}"#);
            let (status, bytes) = post_raw(st, uri, &body, Some(&key)).await;
            let msg = String::from_utf8_lossy(&bytes);
            assert_ne!(
                status,
                StatusCode::PAYLOAD_TOO_LARGE,
                "{uri}: 3 MiB 的体被提取器的默认上限拒了（改前即此断言失败）"
            );
            // 体确实进了 handler：拿到的是网关自己的「无可用 key」而不是提取器的 413
            assert!(
                msg.contains("暂无可用 key"),
                "{uri}: 期望 handler 的错误响应，实际 {status} {msg}"
            );
        }
    }

    /// 负对照：同一次改动**不得**顺带放宽未认证端点。
    #[tokio::test]
    async fn auth_endpoints_stay_at_the_default_limit() {
        let st = test_state("bl_neg", "test-plan", "http://127.0.0.1:9");
        let pad = "a".repeat(3 * 1024 * 1024);
        let body = format!(r#"{{"email":"x@y.z","password":"{pad}"}}"#);
        let (status, _) = post_raw(st, "/api/auth/login", &body, None).await;
        assert_eq!(
            status,
            StatusCode::PAYLOAD_TOO_LARGE,
            "/api/auth/login 不该被放宽（未认证端点缓冲 8 MiB 是另一种风险）"
        );
    }

    /// 假上游：返回固定 usage（listener 预绑定避免并行测试端口冲突）
    async fn fake_upstream(listener: tokio::net::TcpListener) {
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(|_body: String| async {
                Json(serde_json::json!({
                    "id": "cmpl-test",
                    "object": "chat.completion",
                    "model": "test-model",
                    "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}],
                    "usage": { "prompt_tokens": 100, "completion_tokens": 50 }
                }))
            }),
        );
        axum::serve(listener, app).await.unwrap();
    }

    /// 假上游：只提供 anthropic 端点（/v1/messages），记录收到的请求体（验证转换）
    async fn fake_anthropic_upstream(
        listener: tokio::net::TcpListener,
        received: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    ) {
        let app = axum::Router::new().route(
            "/v1/messages",
            axum::routing::post(move |body: String| async move {
                if let Ok(mut r) = received.lock() {
                    *r = Some(body);
                }
                Json(serde_json::json!({
                    "id": "msg-test",
                    "type": "message",
                    "role": "assistant",
                    "model": "test-model",
                    "content": [{"type": "text", "text": "ok-anthropic"}],
                    "stop_reason": "end_turn",
                    "usage": { "input_tokens": 100, "output_tokens": 50 }
                }))
            }),
        );
        axum::serve(listener, app).await.unwrap();
    }

    /// 假上游：只提供 responses 端点（/responses）
    async fn fake_responses_upstream(listener: tokio::net::TcpListener) {
        let app = axum::Router::new().route(
            "/responses",
            axum::routing::post(|_body: String| async {
                Json(serde_json::json!({
                    "id": "resp-test",
                    "object": "response",
                    "model": "test-model",
                    "output": [{
                        "id": "msg_1",
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": "ok-responses", "annotations": []}]
                    }],
                    "usage": { "input_tokens": 100, "output_tokens": 50, "total_tokens": 150 }
                }))
            }),
        );
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

    /// 假上游：openai usage 带缓存命中拆分（cached_tokens=900 / prompt_tokens=1000）
    async fn fake_upstream_cached(listener: tokio::net::TcpListener) {
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(|_body: String| async {
                Json(serde_json::json!({
                    "id": "cmpl-cached",
                    "object": "chat.completion",
                    "model": "test-model",
                    "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}],
                    "usage": {
                        "prompt_tokens": 1000,
                        "completion_tokens": 50,
                        "prompt_tokens_details": {"cached_tokens": 900}
                    }
                }))
            }),
        );
        axum::serve(listener, app).await.unwrap();
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
        let up = tokio::spawn(fake_upstream(listener));
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
            // pts = (100×10 + 50×20)/1e6 USD = 0.002 USD × 7.2 CNY锚定 = 0.0144 点（1 点 = 1 CNY）
            assert!(
                (bal_c - (12471.0 - 0.0144)).abs() < 1e-9,
                "consumer={bal_c}"
            );
            let bal_o: f64 = conn
                .query_row("SELECT balance FROM quotas WHERE user_id = 2", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert!((bal_o - 0.01296).abs() < 1e-9, "owner={bal_o}");
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

    /// rant 2026-08-20T10:17:27：缓存命中/未命中分开计费。
    /// 假上游返回 cached_tokens=900 → usage_records.cached_tokens=900 且按「未命中×价 + 命中×命中价」扣点
    #[tokio::test]
    async fn e2e_cache_hit_billing_split() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let up = tokio::spawn(fake_upstream_cached(listener));
        let base = format!("http://127.0.0.1:{port}");

        let st = test_state("e2ecache", "test-local", &base);
        {
            let conn = st.db.lock().unwrap();
            // 未命中价 10 USD/M、命中价 2 USD/M、输出 20 USD/M
            conn.execute(
                "INSERT OR REPLACE INTO models (provider, model, currency, input_per_m, output_per_m, cache_hit_input_per_m) \
                 VALUES ('test', 'test-model', 'USD', 10.0, 20.0, 2.0)",
                [],
            )
            .unwrap();
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
        insert_key(&st, 300, 2, "test-model", "test-local");
        insert_key(&st, 301, 2, "test-model", "test-local");

        let key = login_key(st.clone()).await;
        let (s, _) = post_raw(
            st.clone(),
            "/v1/chat/completions",
            r#"{"model":"test-model","messages":[{"role":"user","content":"hi"}]}"#,
            Some(&key),
        )
        .await;
        assert_eq!(s, StatusCode::OK);

        let (cached, tokens, cost): (f64, f64, f64) = {
            let conn = st.db.lock().unwrap();
            conn.query_row(
                "SELECT cached_tokens, tokens, cost FROM usage_records ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
        };
        assert_eq!(cached, 900.0, "usage_records 记录缓存命中 token");
        assert_eq!(tokens, 1050.0);
        // cost = 100×10/1e6 + 900×2/1e6 + 50×20/1e6 = 0.0038 USD → CNY 锚定 ×7.2 = 0.02736
        assert!((cost - 0.02736).abs() < 1e-9, "cost={cost}");
        // 消费者扣 0.02736 点；属主得 round5(0.02736×0.9) = round5(0.024624) = 0.02462（90%）
        let (bal_c, bal_o): (f64, f64) = {
            let conn = st.db.lock().unwrap();
            let c = conn
                .query_row("SELECT balance FROM quotas WHERE user_id = 1", [], |r| {
                    r.get(0)
                })
                .unwrap();
            let o = conn
                .query_row("SELECT balance FROM quotas WHERE user_id = 2", [], |r| {
                    r.get(0)
                })
                .unwrap();
            (c, o)
        };
        assert!(
            (bal_c - (12471.0 - 0.02736)).abs() < 1e-9,
            "consumer={bal_c}"
        );
        assert!((bal_o - 0.02462).abs() < 1e-9, "owner={bal_o}");

        up.abort();
    }

    /// 假上游：返回 **DeepSeek 原生**拼写的非流式 usage（顶层 `prompt_cache_hit_tokens`）
    async fn fake_upstream_deepseek_cached(listener: tokio::net::TcpListener) {
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(|_body: String| async {
                Json(serde_json::json!({
                    "id": "cmpl-ds-cached",
                    "object": "chat.completion",
                    "model": "test-model",
                    "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}],
                    "usage": {
                        "prompt_tokens": 1000,
                        "completion_tokens": 50,
                        "prompt_cache_hit_tokens": 900
                    }
                }))
            }),
        );
        axum::serve(listener, app).await.unwrap();
    }

    /// C2039：非流式响应若只带 DeepSeek 原生拼写，也必须按「未命中×价 + 命中×命中价」计费。
    /// 与 `e2e_cache_hit_billing_split` 唯一的差别是 usage 的**拼写**，因此两者的
    /// cached/tokens/cost 断言必须完全一致 —— 修前此用例读到 cached=0、cost=0.0792
    /// （1000 全按未命中价：1000×10/1e6 + 50×20/1e6 = 0.011 USD → ×7.2），即本夹具下多收 2.9x。
    #[tokio::test]
    async fn e2e_nonstream_deepseek_cache_spelling_billing() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let up = tokio::spawn(fake_upstream_deepseek_cached(listener));
        let base = format!("http://127.0.0.1:{port}");

        let st = test_state("e2edscache", "test-local", &base);
        {
            let conn = st.db.lock().unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO models (provider, model, currency, input_per_m, output_per_m, cache_hit_input_per_m) \
                 VALUES ('test', 'test-model', 'USD', 10.0, 20.0, 2.0)",
                [],
            )
            .unwrap();
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
        insert_key(&st, 300, 2, "test-model", "test-local");
        insert_key(&st, 301, 2, "test-model", "test-local");

        let key = login_key(st.clone()).await;
        let (s, _) = post_raw(
            st.clone(),
            "/v1/chat/completions",
            r#"{"model":"test-model","messages":[{"role":"user","content":"hi"}]}"#,
            Some(&key),
        )
        .await;
        assert_eq!(s, StatusCode::OK);

        let (cached, tokens, cost): (f64, f64, f64) = {
            let conn = st.db.lock().unwrap();
            conn.query_row(
                "SELECT cached_tokens, tokens, cost FROM usage_records ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
        };
        assert_eq!(cached, 900.0, "DeepSeek 原生拼写必须记入缓存命中");
        assert_eq!(tokens, 1050.0);
        // 与 OpenAI 拼写用例同值：100×10/1e6 + 900×2/1e6 + 50×20/1e6 = 0.0038 USD → ×7.2 = 0.02736
        assert!((cost - 0.02736).abs() < 1e-9, "cost={cost}");
        let (bal_c, bal_o): (f64, f64) = {
            let conn = st.db.lock().unwrap();
            let c = conn
                .query_row("SELECT balance FROM quotas WHERE user_id = 1", [], |r| {
                    r.get(0)
                })
                .unwrap();
            let o = conn
                .query_row("SELECT balance FROM quotas WHERE user_id = 2", [], |r| {
                    r.get(0)
                })
                .unwrap();
            (c, o)
        };
        assert!(
            (bal_c - (12471.0 - 0.02736)).abs() < 1e-9,
            "consumer={bal_c}"
        );
        assert!((bal_o - 0.02462).abs() < 1e-9, "owner={bal_o}");

        up.abort();
    }

    /// P3-A：openai chat 请求 → 只有 anthropic 端点的 plan → 请求/响应自动转换
    #[tokio::test]
    async fn e2e_openai_chat_to_anthropic_conversion() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let received = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
        let recv2 = std::sync::Arc::clone(&received);
        let up = tokio::spawn(fake_anthropic_upstream(listener, recv2));
        let base = format!("http://127.0.0.1:{port}");

        // plan 只暴露 anthropic 端点
        let st = test_state_eps("conv_oa", "test-anth", &["anthropic"], &base);
        {
            let conn = st.db.lock().unwrap();
            models_row(&conn, "test", "test-model", 10.0, 20.0);
            conn.execute(
                "INSERT OR IGNORE INTO users (id, email, password_hash, name, role) VALUES (2, 'ownera@t.local', 'x', '分享者', 'user')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO quotas (user_id, balance) VALUES (2, 0)",
                [],
            )
            .unwrap();
        }
        insert_key(&st, 400, 2, "test-model", "test-anth");
        let key = login_key(st.clone()).await;

        let (s, body) = post_raw(
            st.clone(),
            "/v1/chat/completions",
            r#"{"model":"test-model","messages":[{"role":"system","content":"Sys"},{"role":"user","content":"hi"}],"max_tokens":10}"#,
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
        // 响应转换回 openai chat 格式
        assert_eq!(v["choices"][0]["message"]["content"], "ok-anthropic");
        assert_eq!(v["choices"][0]["finish_reason"], "stop");
        // usage 按 anthropic input/output 映射为 prompt/completion
        assert_eq!(v["usage"]["prompt_tokens"], 100);
        assert_eq!(v["usage"]["completion_tokens"], 50);

        // 上游收到的是 anthropic 格式请求（system 顶层 + max_tokens 保留 + 无 choices 字段）
        let upstream = received.lock().unwrap().clone().expect("upstream got body");
        let uv: serde_json::Value = serde_json::from_str(&upstream).unwrap();
        assert_eq!(uv["system"], "Sys", "system 提取为顶层字段: {upstream}");
        assert_eq!(uv["max_tokens"], 10, "max_tokens 保留: {upstream}");
        assert_eq!(uv["messages"][0]["role"], "user");
        assert!(
            uv.get("choices").is_none(),
            "不是 openai 请求体: {upstream}"
        );

        // 计量入账：100×10/1e6 + 50×20/1e6 = 0.002 USD × 7.2 = 0.0144 点（1 点 = 1 CNY）
        let conn = st.db.lock().unwrap();
        let bal: f64 = conn
            .query_row("SELECT balance FROM quotas WHERE user_id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!((bal - (12471.0 - 0.0144)).abs() < 1e-9, "consumer={bal}");
        drop(conn);
        up.abort();
    }

    /// P3-A：anthropic 请求 → 只有 openai_chat 端点的 plan → 自动转换（反向）
    #[tokio::test]
    async fn e2e_anthropic_to_openai_chat_conversion() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let up = tokio::spawn(fake_upstream(listener));
        let base = format!("http://127.0.0.1:{port}");

        // plan 只暴露 openai_chat 端点
        let st = test_state_eps("conv_ao", "test-oc", &["openai_chat"], &base);
        {
            let conn = st.db.lock().unwrap();
            models_row(&conn, "test", "test-model", 10.0, 20.0);
        }
        insert_key(&st, 401, 1, "test-model", "test-oc");
        let key = login_key(st.clone()).await;

        let (s, body) = post_raw(
            st.clone(),
            "/anthropic/v1/messages",
            r#"{"model":"test-model","messages":[{"role":"user","content":"hi"}],"max_tokens":100}"#,
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
        // 响应转换回 anthropic 格式
        assert_eq!(v["type"], "message");
        assert_eq!(v["content"][0]["type"], "text");
        assert_eq!(v["content"][0]["text"], "ok");
        assert_eq!(v["stop_reason"], "end_turn");
        assert_eq!(v["usage"]["input_tokens"], 100);
        assert_eq!(v["usage"]["output_tokens"], 50);
        up.abort();
    }

    /// P3-A：/v1/responses 请求 → openai_chat 上游 → responses 格式响应
    #[tokio::test]
    async fn e2e_responses_endpoint_conversion() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let up = tokio::spawn(fake_upstream(listener));
        let base = format!("http://127.0.0.1:{port}");

        // plan 只暴露 openai_chat 端点 → responses 入站走转换
        let st = test_state_eps("conv_rs", "test-rs", &["openai_chat"], &base);
        {
            let conn = st.db.lock().unwrap();
            models_row(&conn, "test", "test-model", 10.0, 20.0);
        }
        insert_key(&st, 402, 1, "test-model", "test-rs");
        let key = login_key(st.clone()).await;

        let (s, body) = post_raw(
            st.clone(),
            "/v1/responses",
            r#"{"model":"test-model","instructions":"Be brief","input":[{"role":"user","content":"hi"}],"max_output_tokens":100}"#,
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
        assert_eq!(v["object"], "response");
        assert_eq!(v["output"][0]["type"], "message");
        assert_eq!(v["output"][0]["content"][0]["type"], "output_text");
        assert_eq!(v["output"][0]["content"][0]["text"], "ok");
        assert_eq!(v["usage"]["input_tokens"], 100);
        assert_eq!(v["usage"]["output_tokens"], 50);
        up.abort();
    }

    /// P3-A：/v1/responses 同协议透传（plan 有 responses 端点）
    #[tokio::test]
    async fn e2e_responses_same_protocol_passthrough() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let up = tokio::spawn(fake_responses_upstream(listener));
        let base = format!("http://127.0.0.1:{port}");

        let st = test_state_eps("passthru_rs", "test-rs2", &["responses"], &base);
        {
            let conn = st.db.lock().unwrap();
            models_row(&conn, "test", "test-model", 10.0, 20.0);
        }
        insert_key(&st, 403, 1, "test-model", "test-rs2");
        let key = login_key(st.clone()).await;

        let (s, body) = post_raw(
            st.clone(),
            "/v1/responses",
            r#"{"model":"test-model","input":[{"role":"user","content":"hi"}]}"#,
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
        assert_eq!(v["object"], "response");
        assert_eq!(v["output"][0]["content"][0]["text"], "ok-responses");
        // 透传保留原 usage 字段（input/output_tokens）
        assert_eq!(v["usage"]["input_tokens"], 100);
        assert_eq!(v["usage"]["output_tokens"], 50);
        up.abort();
    }

    /// P3-B（rant 2026-08-18T18:59:29）：流式跨协议转换 — openai 客户端 stream
    /// → plan 只暴露 anthropic 端点 → 上游 anthropic SSE → 转回 openai SSE 事件。
    #[tokio::test]
    async fn sse_cross_protocol_openai_to_anthropic_conversion() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let up = tokio::spawn(fake_anthropic_sse_upstream(listener));
        let base = format!("http://127.0.0.1:{port}");

        // plan 只暴露 anthropic 端点 → 必须走协议转换
        let st = test_state_eps("conv_sse", "test-anth-s", &["anthropic"], &base);
        {
            let conn = st.db.lock().unwrap();
            models_row(&conn, "test", "test-model", 10.0, 20.0);
            // 独立分享者（owner=2，与消费者 user=1 分离 → 净扣 1.4 点可断言）
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
        insert_key(&st, 404, 2, "test-model", "test-anth-s");
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
        assert!(
            text.contains("\"role\":\"assistant\""),
            "openai role chunk: {text}"
        );
        assert!(
            text.contains("\"content\":\"hi\""),
            "openai 文本增量: {text}"
        );
        assert!(text.contains("data: [DONE]"), "openai [DONE]: {text}");
        assert!(
            !text.contains("event: message_start"),
            "不应透传 anthropic 事件"
        );
        // 入账：80×10/1e6 + 30×20/1e6 = 0.0014 USD × 7.2 = 0.01008 点
        let conn = st.db.lock().unwrap();
        let bal: f64 = conn
            .query_row("SELECT balance FROM quotas WHERE user_id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!((bal - (12471.0 - 0.01008)).abs() < 1e-9, "consumer={bal}");
        drop(conn);
        up.abort();
    }

    /// P3-A 补充（rant 2026-08-18T18:10:18）：GET /v1/models 无认证 → OpenAI 列表格式
    #[tokio::test]
    async fn v1_models_public_list_format() {
        let st = test_state("v1m", "test-plan", "http://127.0.0.1:9");
        for uri in ["/v1/models", "/models"] {
            let resp = router()
                .with_state(st.clone())
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(uri)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "uri={uri}");
            let bytes = axum::body::to_bytes(resp.into_body(), 4 * 1024 * 1024)
                .await
                .unwrap();
            let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(v["object"], "list", "uri={uri}");
            let data = v["data"].as_array().expect("data 数组");
            assert!(!data.is_empty(), "models 已 seed");
            let first = &data[0];
            assert_eq!(first["object"], "model");
            assert!(first.get("id").is_some());
            assert_eq!(first["created"], 0);
            assert_eq!(first["owned_by"], "aitokenpool");
            assert_eq!(
                first["display_name"], first["id"],
                "display_name 与 id 一致"
            );
            assert!(first.get("context_window").is_some());
            // 无认证 → 不附加 available_keys
            assert!(
                first.get("available_keys").is_none(),
                "无认证不应有 available_keys"
            );
        }
    }

    /// P3-A 补充：带有效 Bearer → data[].available_keys 存在
    #[tokio::test]
    async fn v1_models_with_token_adds_available_keys() {
        let st = test_state("v1mk", "test-plan", "http://127.0.0.1:9");
        let key = login_key(st.clone()).await;
        let resp = router()
            .with_state(st.clone())
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/models")
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
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let data = v["data"].as_array().unwrap();
        assert!(!data.is_empty());
        assert!(
            data[0].get("available_keys").is_some(),
            "带 token 附加 available_keys"
        );
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
        assert!(first.get("context_window").is_some());
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

    #[tokio::test]
    async fn plans_endpoint_lists_config_plans() {
        // rant 2026-08-18T16:14:21 Bug 1：上架表单 Plan 数据源 = config [[plans]]（单一真源）
        let st = test_state("plans", "test-plan", "http://127.0.0.1:9");
        let key = login_key(st.clone()).await;
        let resp = router()
            .with_state(st.clone())
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/plans")
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
        // config.example.toml 的 [[plans]] 至少包含 deepseek-paygo / zhipu-coding / zhipu-paygo
        let ids: Vec<&str> = arr
            .iter()
            .filter_map(|p| p.get("id").and_then(|v| v.as_str()))
            .collect();
        assert!(
            ids.contains(&"deepseek-paygo"),
            "plans 来自 config [[plans]]，包含 deepseek-paygo，got {ids:?}"
        );
        assert!(ids.contains(&"zhipu-coding"));
        assert!(ids.contains(&"zhipu-paygo"));
        // 字段结构：id / provider / name / type / endpoints
        let dp = arr
            .iter()
            .find(|p| p.get("id") == Some(&serde_json::json!("deepseek-paygo")))
            .unwrap();
        assert_eq!(dp["provider"], "deepseek");
        assert_eq!(dp["type"], "paygo");

        // `name` 是 config 的**原值**：config.example.toml 的 `[[plans]]` 全都不写 `name`
        // ⇒ 这里就是空串。后端**不得**按 `type` 自造显示名（`API（按量）` 这类），因为响应
        // 的**数据**字段是前端原样渲染的、`en` 界面会直接显示中文（C2133）；显示文案由客户端
        // `planLabel()` 从语言包取。
        assert_eq!(
            dp["name"], "",
            "plans[].name 必须是 config 原值（未配置即空串），不能是后端自造的显示名"
        );
        assert!(
            !arr.iter().any(|p| p["name"]
                .as_str()
                .is_some_and(|n| n.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)))),
            "plans[].name 里出现了中文 —— 数据字段里的中文会在 en 界面原样显示：{arr:?}"
        );
        assert!(dp["endpoints"].is_array() && !dp["endpoints"].as_array().unwrap().is_empty());
        // 无认证 → 401
        let resp2 = router()
            .with_state(st)
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/plans")
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

    /// 假上游：SSE 流式但不带任何 usage，同时记录收到的请求体
    /// （验证网关是否请求了 `stream_options.include_usage`）
    async fn fake_sse_upstream_no_usage(
        listener: tokio::net::TcpListener,
        received: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    ) {
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(move |body: String| async move {
                if let Ok(mut r) = received.lock() {
                    *r = Some(body);
                }
                let body = "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n\
                            data: [DONE]\n\n";
                (
                    [("content-type", "text/event-stream")],
                    axum::body::Body::from(body),
                )
            }),
        );
        axum::serve(listener, app).await.unwrap();
    }

    /// 出站流式请求必须主动要求 usage：openai_chat 下 usage 是**opt-in**，
    /// 不设置 `stream_options.include_usage` 上游就不会下发 → 本次调用静默不计费。
    /// 客户端已设置时保留其值；anthropic/responses 出站不添加（它们恒带 usage）。
    #[test]
    fn with_include_usage_sets_flag_for_openai_only() {
        let out = with_include_usage(r#"{"model":"m","stream":true}"#, "openai_chat");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["stream_options"]["include_usage"], true, "out={out}");

        // 客户端自己的设置优先（含 false）
        let out = with_include_usage(
            r#"{"model":"m","stream":true,"stream_options":{"include_usage":false}}"#,
            "openai_chat",
        );
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["stream_options"]["include_usage"], false, "out={out}");

        // 同 key 合并，不覆盖客户端其它 stream_options
        let out = with_include_usage(
            r#"{"model":"m","stream":true,"stream_options":{"foo":1}}"#,
            "openai_chat",
        );
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["stream_options"]["include_usage"], true);
        assert_eq!(v["stream_options"]["foo"], 1, "out={out}");

        // 非 openai_chat 出站：原样返回（anthropic 的 message_start、responses 的
        // response.completed 都恒带 usage）
        for ob in ["anthropic", "responses"] {
            let src = r#"{"model":"m","stream":true}"#;
            assert_eq!(with_include_usage(src, ob), src, "outbound={ob}");
        }

        // 非法 JSON：原样返回，不阻断转发
        assert_eq!(with_include_usage("not json", "openai_chat"), "not json");
    }

    /// 回归：无 usage 的 openai 流必须让上游**被请求**上报 usage
    /// （修复前 outbound body 原样透传，没有 stream_options）
    #[tokio::test]
    async fn sse_openai_stream_requests_usage_from_upstream() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let received = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
        let recv2 = std::sync::Arc::clone(&received);
        let up = tokio::spawn(fake_sse_upstream_no_usage(listener, recv2));
        let base = format!("http://127.0.0.1:{port}");

        let st = test_state("sse_iu", "test-sse-iu", &base);
        {
            let conn = st.db.lock().unwrap();
            models_row(&conn, "test", "test-model", 10.0, 20.0);
        }
        insert_key(&st, 310, 1, "test-model", "test-sse-iu");
        let key = login_key(st.clone()).await;

        let (s, _body) = post_raw(
            st.clone(),
            "/v1/chat/completions",
            r#"{"model":"test-model","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
            Some(&key),
        )
        .await;
        assert_eq!(s, StatusCode::OK);

        let out = received.lock().unwrap().clone().expect("upstream got body");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v["stream_options"]["include_usage"], true,
            "出站流式请求必须带上 include_usage: {out}"
        );
        up.abort();
    }

    /// 无 usage 的流：仍不入账（不估算），且 capture 能区分「没见到」与「见到 0」
    #[tokio::test]
    async fn sse_stream_without_usage_is_not_billed_but_flagged() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let received = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
        let recv2 = std::sync::Arc::clone(&received);
        let up = tokio::spawn(fake_sse_upstream_no_usage(listener, recv2));
        let base = format!("http://127.0.0.1:{port}");

        let st = test_state("sse_nu", "test-sse-nu", &base);
        {
            let conn = st.db.lock().unwrap();
            models_row(&conn, "test", "test-model", 10.0, 20.0);
        }
        insert_key(&st, 311, 1, "test-model", "test-sse-nu");
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

        let conn = st.db.lock().unwrap();
        let bal: f64 = conn
            .query_row("SELECT balance FROM quotas WHERE user_id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!((bal - 12471.0).abs() < 1e-9, "无 usage 不入账: {bal}");
        let n_ur: i64 = conn
            .query_row("SELECT COUNT(*) FROM usage_records", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n_ur, 0, "无 usage 无 usage_records");
        let n_tx: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n_tx, 0, "无 usage 无 transactions");
        drop(conn);
        up.abort();

        // 「没见到 usage」必须与「见到了 usage=0」可区分——这是 fail-closed 留痕的依据
        let mut cap = UsageCapture::new("openai_chat");
        cap.push(br#"data: {"choices":[{"delta":{"content":"hi"}}]}"#);
        cap.push(b"\n\n");
        cap.push(br#"data: [DONE]"#);
        let _ = cap.finish();
        assert!(!cap.usage_seen, "全程无 usage 对象 → usage_seen=false");

        let mut cap = UsageCapture::new("openai_chat");
        cap.push(br#"data: {"usage":{"prompt_tokens":0,"completion_tokens":0},"choices":[]}"#);
        let _ = cap.finish();
        assert!(cap.usage_seen, "带 usage（即便全 0）→ usage_seen=true");
    }

    /// 假上游：SSE 流式（anthropic 风格：message_start 带 input，message_delta 带 output）
    async fn fake_anthropic_sse_upstream(listener: tokio::net::TcpListener) {
        let app = axum::Router::new().route(
            "/v1/messages",
            axum::routing::post(|_body: String| async {
                let body = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"test-model\",\"usage\":{\"input_tokens\":80,\"output_tokens\":1}}}\n\n\
                            event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
                            event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n\
                            event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
                            event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":30}}\n\n\
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
        // 计量：100×10/1e6 + 50×20/1e6 = 0.002 USD × 7.2 = 0.0144 点
        let conn = st.db.lock().unwrap();
        let bal: f64 = conn
            .query_row("SELECT balance FROM quotas WHERE user_id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!((bal - (12471.0 - 0.0144)).abs() < 1e-9, "consumer={bal}");
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
        // 80×10/1e6 + 30×20/1e6 = 0.0014 USD × 7.2 = 0.01008 点
        let conn = st.db.lock().unwrap();
        let bal: f64 = conn
            .query_row("SELECT balance FROM quotas WHERE user_id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!((bal - (12471.0 - 0.01008)).abs() < 1e-9, "consumer={bal}");
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
        let (i, _c, o) = cap.finish();
        assert_eq!(i, 10.0);
        assert_eq!(o, 5.0);

        // openai + DeepSeek 原生顶层拼写 prompt_cache_hit_tokens（rant 2026-08-23T14:05:02）
        let mut cap = UsageCapture::new("openai_chat");
        cap.push(
            br#"data: {"usage":{"prompt_tokens":100,"prompt_cache_hit_tokens":90,"completion_tokens":50},"choices":[]}"#,
        );
        let (i, c, o) = cap.finish();
        assert_eq!(i, 10.0, "DeepSeek cached 应从 prompt 扣除（disjoint）");
        assert_eq!(c, 90.0);
        assert_eq!(o, 50.0);

        // openai + OpenAI 拼写 prompt_tokens_details.cached_tokens：同样 disjoint
        let mut cap = UsageCapture::new("openai_chat");
        cap.push(
            br#"data: {"usage":{"prompt_tokens":100,"prompt_tokens_details":{"cached_tokens":90},"completion_tokens":50},"choices":[]}"#,
        );
        let (i, c, o) = cap.finish();
        assert_eq!(i, 10.0);
        assert_eq!(c, 90.0);
        assert_eq!(o, 50.0);

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
        let (i, _c, o) = cap.finish();
        assert_eq!(i, 80.0, "message_start 的 input_tokens 提前捕获");
        assert_eq!(o, 30.0, "message_delta 的 output_tokens 流尾解析");

        // anthropic + cache_read_input_tokens：input 同样 disjoint（与 sse.rs anthropic 路径一致）
        let mut cap = UsageCapture::new("anthropic");
        cap.push(
            br#"event: message_start
data: {"type":"message_start","message":{"usage":{"input_tokens":80,"cache_read_input_tokens":70,"output_tokens":1}}}"#,
        );
        cap.push(b"\n\n");
        cap.push(
            br#"event: message_delta
data: {"type":"message_delta","usage":{"output_tokens":30}}"#,
        );
        let (i, c, o) = cap.finish();
        assert_eq!(i, 10.0, "cache_read 应从 input 扣除（disjoint）");
        assert_eq!(c, 70.0);
        assert_eq!(o, 30.0);
    }

    // ── 出站超时策略（C2084）：流式**不能**用「总时限」─────────────────────────

    /// 假上游：SSE 慢速滴流 —— 每 100 ms 一帧，共 15 帧，随后一帧带 usage，最后 `[DONE]`。
    /// 流总时长 ≈1.5 s，用来区分「总时限」与「逐次读取时限」这两种语义。
    async fn fake_sse_slow_upstream(listener: tokio::net::TcpListener) {
        let app = axum::Router::new().route(
            "/chat/completions",
            axum::routing::post(|_body: String| async {
                let s = async_stream::stream! {
                    for i in 1..=15u32 {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        yield Ok::<_, std::io::Error>(axum::body::Bytes::from(format!(
                            "data: {{\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"choices\":[{{\"delta\":{{\"content\":\"c{i}\"}}}}]}}\n\n"
                        )));
                    }
                    yield Ok(axum::body::Bytes::from_static(
                        b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":50}}\n\n",
                    ));
                    yield Ok(axum::body::Bytes::from_static(b"data: [DONE]\n\n"));
                };
                (
                    [("content-type", "text/event-stream")],
                    axum::body::Body::from_stream(s),
                )
            }),
        );
        axum::serve(listener, app).await.unwrap();
    }

    /// 读取 SSE 响应直到流结束或出错 —— 返回 (状态码, 已收到的字节, 错误文本)。
    /// 与 `post_raw` 的区别：出错时**不 panic**，而是把错误交回调用方
    /// （被总时限截断的现场，只会表现为「流中途出错」）。
    async fn drain_sse(
        st: AppState,
        uri: &str,
        body: &str,
        bearer: &str,
    ) -> (StatusCode, Vec<u8>, Option<String>) {
        use futures_util::StreamExt;
        let resp = router()
            .with_state(st)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {bearer}"))
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let mut out = Vec::new();
        let mut err = None;
        let mut stream = resp.into_body().into_data_stream();
        while let Some(item) = stream.next().await {
            match item {
                Ok(b) => out.extend_from_slice(&b),
                Err(e) => {
                    err = Some(e.to_string());
                    break;
                }
            }
        }
        (status, out, err)
    }

    fn slow_upstream_state(tag: &str, plan: &str, base: &str, key_id: i64) -> AppState {
        let st = test_state(tag, plan, base);
        {
            let conn = st.db.lock().unwrap();
            models_row(&conn, "test", "test-model", 10.0, 20.0);
        }
        insert_key(&st, key_id, 1, "test-model", plan);
        st
    }

    fn sse_req_body() -> &'static str {
        r#"{"model":"test-model","stream":true,"messages":[{"role":"user","content":"hi"}]}"#
    }

    fn usage_records(st: &AppState) -> i64 {
        let conn = st.db.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM usage_records", [], |r| r.get(0))
            .unwrap()
    }

    /// 长流不能被「总时限」截断：流式客户端只有**逐次读取**时限（每次成功读取后重置），
    /// 所以只要上游还在产出数据，总时长超过该时限的流也必须完整走完、并在流尾入账一次。
    #[tokio::test]
    async fn sse_slow_stream_survives_a_read_timeout() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let up = tokio::spawn(fake_sse_slow_upstream(listener));
        let base = format!("http://127.0.0.1:{port}");

        let mut st = slow_upstream_state("sse_slow", "test-sse-slow", &base, 310);
        // 生产「流式」客户端工厂：逐次读取时限 200 ms（≪ 流的 1.5 s 总时长）
        st.http_stream =
            crate::routes::upstream_stream_client(std::time::Duration::from_millis(200));
        let key = login_key(st.clone()).await;

        let t0 = std::time::Instant::now();
        let (s, body, err) =
            drain_sse(st.clone(), "/v1/chat/completions", sse_req_body(), &key).await;
        let text = String::from_utf8_lossy(&body);
        assert_eq!(s, StatusCode::OK, "err={err:?}");
        assert!(err.is_none(), "有进展的长流不应被截断：err={err:?}");
        assert!(text.contains("data: [DONE]"), "流必须走到终点");
        assert_eq!(
            text.matches("\"content\":\"c").count(),
            15,
            "15 帧应全部到达"
        );
        // 这条是「总时限 vs 逐次读取时限」的判别式：总时长必须真的超过单次读取间隔
        assert!(
            t0.elapsed() >= std::time::Duration::from_millis(1400),
            "流应当真的持续约 1.5 s，实测 {:?}",
            t0.elapsed()
        );
        assert_eq!(usage_records(&st), 1, "流尾正常结束应入账一次");
        up.abort();
    }

    /// 对照组（**已知缺陷注入**）：同一个慢速上游，但把流式客户端换成**非流式**工厂
    /// —— 它的 `timeout` 是**总**时限，200 ms 就会把这条 1.5 s 的流截断。
    ///
    /// 该臂同时证明网关确实在用 `st.http_stream`：若 `forward_stream` 仍读 `st.http`，
    /// 这里就不会截断（修复前本仓库正是这个行为）。
    #[tokio::test]
    async fn sse_slow_stream_is_cut_by_a_total_deadline() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let up = tokio::spawn(fake_sse_slow_upstream(listener));
        let base = format!("http://127.0.0.1:{port}");

        let mut st = slow_upstream_state("sse_cut", "test-sse-cut", &base, 311);
        // 非流式工厂 = 总时限客户端（故意注入缺陷策略）
        st.http_stream = crate::routes::upstream_client(std::time::Duration::from_millis(200));
        let key = login_key(st.clone()).await;

        let t0 = std::time::Instant::now();
        let (s, body, err) =
            drain_sse(st.clone(), "/v1/chat/completions", sse_req_body(), &key).await;
        let text = String::from_utf8_lossy(&body);
        assert_eq!(s, StatusCode::OK, "响应头已发出，截断发生在 body 阶段");
        assert!(
            !text.contains("data: [DONE]"),
            "总时限必须截断这条流（err={err:?}）；实际尾部: {}",
            &text[text.len().saturating_sub(120)..]
        );
        assert!(
            text.matches("\"content\":\"c").count() < 15,
            "被截断的流不可能收到全部 15 帧"
        );
        assert!(
            t0.elapsed() < std::time::Duration::from_millis(1000),
            "200 ms 的总时限应立刻截断，实测 {:?}",
            t0.elapsed()
        );
        assert_eq!(
            usage_records(&st),
            0,
            "被截断的流不入账（流尾 finalize 不执行）"
        );
        up.abort();
    }

    /// C2086：脚本化 HTTP 上游 —— 每接受一个连接写一条**预先构造**的原始响应后关闭。
    /// 必须用裸 socket：`fake_upstream` 这类 axum 假上游只能发出格式正确的 body，
    /// 「状态行 200 但 body 未完整到达」这个形状在它们身上无法表达。
    async fn scripted_http_upstream(
        listener: tokio::net::TcpListener,
        responses: Vec<String>,
        hits: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut idx = 0usize;
        loop {
            let (mut sock, _) = match listener.accept().await {
                Ok(v) => v,
                Err(_) => return,
            };
            hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut buf = [0u8; 8192];
            let _ =
                tokio::time::timeout(std::time::Duration::from_millis(300), sock.read(&mut buf))
                    .await;
            let pick = idx.min(responses.len() - 1);
            idx += 1;
            let _ = sock.write_all(responses[pick].as_bytes()).await;
            let _ = sock.flush().await;
            drop(sock);
        }
    }

    /// 构造一条原始响应；`claimed_len` 故意大于实发字节数以模拟 body 未完整到达
    fn scripted_http_response(status: &str, body: &str, claimed_len: Option<usize>) -> String {
        let n = claimed_len.unwrap_or(body.len());
        format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {n}\r\nconnection: close\r\n\r\n{body}"
        )
    }

    /// C2086 测试状态：单模型定价 + 属主用户，便于同时断言「入账」与「未入账」
    fn scripted_upstream_state(tag: &str, port: u16, keys: usize) -> AppState {
        let base = format!("http://127.0.0.1:{port}");
        let st = test_state_eps(tag, "test-c2086", &["openai_chat"], &base);
        {
            let conn = st.db.lock().unwrap();
            models_row(&conn, "test", "test-model", 10.0, 20.0);
            conn.execute(
                "INSERT OR IGNORE INTO users (id, email, password_hash, name, role) VALUES (2, 'c2086-owner@t.local', 'x', '分享者', 'user')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO quotas (user_id, balance) VALUES (2, 0)",
                [],
            )
            .unwrap();
        }
        for i in 0..keys {
            insert_key(&st, 700 + i as i64, 2, "test-model", "test-c2086");
        }
        st
    }

    fn consumer_balance(st: &AppState) -> f64 {
        let conn = st.db.lock().unwrap();
        conn.query_row("SELECT balance FROM quotas WHERE user_id = 1", [], |r| {
            r.get(0)
        })
        .unwrap()
    }

    /// C2085 缺陷：状态行 200 而响应体未完整到达时，**不得**把它变成「成功的空响应」。
    /// 客户端拿到的必须是显式失败（502），且该次调用不得被记成「零用量成功」
    /// （否则账本与 month_calls 同时漏掉一次真实消耗的调用）。
    /// 本测试自带阳性对照：同样一条上游，body 完整时仍须 200 且恰好入账一次。
    #[tokio::test]
    async fn forward_body_read_failure_is_not_a_success() {
        const OK_BODY: &str = r#"{"id":"cmpl-c2086","object":"chat.completion","model":"test-model","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":100,"completion_tokens":50}}"#;

        // ---- 阳性对照：完整 200 + usage → 200 且恰好入账一次 ----
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let up = tokio::spawn(scripted_http_upstream(
            listener,
            vec![scripted_http_response("200 OK", OK_BODY, None)],
            hits.clone(),
        ));
        let st = scripted_upstream_state("c2086ok", port, 1);
        let key = login_key(st.clone()).await;
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
            "阳性对照：body 完整时必须 200，body: {}",
            String::from_utf8_lossy(&body)
        );
        assert_eq!(usage_records(&st), 1, "阳性对照：完整响应恰好入账一次");
        assert_eq!(
            hits.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "阳性对照：只应发出一次上游请求"
        );
        assert!(
            (consumer_balance(&st) - (12471.0 - 0.0144)).abs() < 1e-9,
            "阳性对照：消费者应按 usage 扣费，实测 {}",
            consumer_balance(&st)
        );
        up.abort();

        // ---- 缺陷臂：content-length 999、实发不足 → 502，不入账、不重试、余额不变 ----
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let up = tokio::spawn(scripted_http_upstream(
            listener,
            vec![scripted_http_response("200 OK", OK_BODY, Some(999))],
            hits.clone(),
        ));
        let st = scripted_upstream_state("c2086trunc", port, 2);
        let key = login_key(st.clone()).await;
        let before = consumer_balance(&st);
        let (s, body) = post_raw(
            st.clone(),
            "/v1/chat/completions",
            r#"{"model":"test-model","messages":[{"role":"user","content":"hi"}]}"#,
            Some(&key),
        )
        .await;
        assert_eq!(
            s,
            StatusCode::BAD_GATEWAY,
            "body 未完整到达时必须显式失败，而不是伪造 200 + 空 body；实测 body: {}",
            String::from_utf8_lossy(&body)
        );
        assert!(
            String::from_utf8_lossy(&body).contains("上游响应读取失败"),
            "502 应说明原因，实测 {}",
            String::from_utf8_lossy(&body)
        );
        assert_eq!(usage_records(&st), 0, "未完成的响应不得入账");
        assert_eq!(consumer_balance(&st), before, "未完成的响应不得扣费");
        assert_eq!(
            hits.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "2xx 之后不得重发（上游可能已完成工作，重试可能让 key 属主被双扣）"
        );
        up.abort();
    }

    /// C2086 非目标回归：非 2xx 且 body 读不到时，**原分流必须不变** ——
    /// 5xx 仍然换 key（可故障转移），全部 key 失败后仍是 503、且不入账。
    #[tokio::test]
    async fn unreadable_body_on_a_server_error_still_fails_over() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let up = tokio::spawn(scripted_http_upstream(
            listener,
            vec![scripted_http_response(
                "500 Internal Server Error",
                r#"{"error":{"message":"upstream-boom"}}"#,
                Some(999),
            )],
            hits.clone(),
        ));
        let st = scripted_upstream_state("c2086failover", port, 2);
        let key = login_key(st.clone()).await;
        let (s, body) = post_raw(
            st.clone(),
            "/v1/chat/completions",
            r#"{"model":"test-model","messages":[{"role":"user","content":"hi"}]}"#,
            Some(&key),
        )
        .await;
        assert_eq!(
            s,
            StatusCode::SERVICE_UNAVAILABLE,
            "两个 key 都 5xx 后应 503，body: {}",
            String::from_utf8_lossy(&body)
        );
        assert_eq!(
            hits.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "5xx 仍须换 key（body 读不到不改变故障转移判定）"
        );
        assert_eq!(st.router.cooldown_len(), 2, "失败的 key 仍应进入冷却");
        assert_eq!(usage_records(&st), 0, "失败的调用不入账");
        up.abort();
    }
}
