//! 网关协议转换（P3-A，rant 2026-08-18T16:15:42）
//!
//! OpenAI Chat / OpenAI Responses / Anthropic Messages 三协议互转，
//! 使任一入站协议可以调用只暴露其他协议端点的 plan（自动转换）。
//!
//! 设计（与 openlocalrouter src/router/transform.rs 同思路，宿主 2026-08-18 指示复用）：
//! - 入站解析/出站生成用 serde_json::Value 直接操作（无中间结构体），
//!   以「同协议透传不转换」为原则，仅跨协议时走本模块；
//! - 协议名用本项目约定：openai_chat / anthropic / responses；
//! - 出站协议选择：同协议优先 → anthropic（兼容性最好）→ openai_chat → responses；
//! - 响应转换同步完成 usage 字段映射（prompt/completion ↔ input/output），
//!   网关计量按「上游原始响应」的协议解析（见 gateway::parse_usage），互不影响。
//!
//! 流式 SSE 转换留 P3-B（openai delta / anthropic content_block_delta /
//! responses output_text 事件互转）——本模块只处理非流式。

use serde_json::{json, Value};

/// 协议名（本项目约定，与 config Endpoint.protocol 枚举一致）
pub const PROTOCOL_OPENAI_CHAT: &str = "openai_chat";
pub const PROTOCOL_ANTHROPIC: &str = "anthropic";
pub const PROTOCOL_RESPONSES: &str = "responses";

/// anthropic max_tokens 必填，缺失时的默认值（rant 约定 4096）
pub const ANTHROPIC_DEFAULT_MAX_TOKENS: u64 = 4096;

// ────────────────────────────────────────────────────────────
// 出站协议选择
// ────────────────────────────────────────────────────────────

/// 按入站协议 + plan 可用端点选择出站协议：
/// 同协议端点优先（透传，零转换损耗）；无同协议 → 按
/// anthropic → openai_chat → responses 优先级选可用协议；全不可用 → None（503）。
pub fn determine_forwarding_protocol<'a>(
    plan_protocols: &[String],
    inbound: &'a str,
) -> Option<&'a str> {
    if plan_protocols.iter().any(|p| p == inbound) {
        return Some(inbound);
    }
    [PROTOCOL_ANTHROPIC, PROTOCOL_OPENAI_CHAT, PROTOCOL_RESPONSES]
        .into_iter()
        .find(|&candidate| candidate != inbound && plan_protocols.iter().any(|p| p == candidate))
        .map(|v| v as _)
}

/// 请求体转换派发（from → to 跨协议转换；同协议原样返回）
pub fn transform_request(body: &Value, from: &str, to: &str) -> Value {
    match (from, to) {
        (PROTOCOL_OPENAI_CHAT, PROTOCOL_ANTHROPIC) => openai_chat_to_anthropic_req(body),
        (PROTOCOL_OPENAI_CHAT, PROTOCOL_RESPONSES) => openai_chat_to_openai_responses_req(body),
        (PROTOCOL_ANTHROPIC, PROTOCOL_OPENAI_CHAT) => anthropic_to_openai_chat_req(body),
        (PROTOCOL_ANTHROPIC, PROTOCOL_RESPONSES) => anthropic_to_openai_responses_req(body),
        (PROTOCOL_RESPONSES, PROTOCOL_OPENAI_CHAT) => openai_responses_to_openai_chat_req(body),
        (PROTOCOL_RESPONSES, PROTOCOL_ANTHROPIC) => openai_responses_to_anthropic_req(body),
        _ => body.clone(),
    }
}

/// 响应体转换派发（from → to 跨协议转换；同协议原样返回）
pub fn transform_response(body: &Value, from: &str, to: &str) -> Value {
    match (from, to) {
        (PROTOCOL_OPENAI_CHAT, PROTOCOL_ANTHROPIC) => openai_chat_to_anthropic_resp(body),
        (PROTOCOL_OPENAI_CHAT, PROTOCOL_RESPONSES) => openai_chat_to_openai_responses_resp(body),
        (PROTOCOL_ANTHROPIC, PROTOCOL_OPENAI_CHAT) => anthropic_to_openai_chat_resp(body),
        (PROTOCOL_ANTHROPIC, PROTOCOL_RESPONSES) => anthropic_to_openai_responses_resp(body),
        (PROTOCOL_RESPONSES, PROTOCOL_OPENAI_CHAT) => openai_responses_to_openai_chat_resp(body),
        (PROTOCOL_RESPONSES, PROTOCOL_ANTHROPIC) => openai_responses_to_anthropic_resp(body),
        _ => body.clone(),
    }
}

// ────────────────────────────────────────────────────────────
// 请求转换
// ────────────────────────────────────────────────────────────

/// OpenAI Chat 请求 → Anthropic Messages 请求
///
/// - system 消息合并提取为顶层 `system` 字段；
/// - tool 消息 → user + tool_result content block；
/// - assistant tool_calls → tool_use content blocks；
/// - tools（type:function）→ anthropic tools（input_schema）；
/// - max_tokens 缺失时补默认 4096（anthropic 必填）。
pub fn openai_chat_to_anthropic_req(body: &Value) -> Value {
    let mut result = json!({});
    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        result["model"] = json!(model);
    }

    let mut system_parts: Vec<String> = Vec::new();
    let mut messages = Vec::new();

    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        for msg in msgs {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
            match role {
                "system" => {
                    if let Some(text) = msg.get("content").and_then(|c| c.as_str()) {
                        if !text.is_empty() {
                            system_parts.push(text.to_string());
                        }
                    }
                }
                "tool" => {
                    let tool_call_id = msg
                        .get("tool_call_id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("");
                    let content = msg.get("content").cloned().unwrap_or(json!(""));
                    messages.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": tool_call_id,
                            "content": content
                        }]
                    }));
                }
                "assistant" => {
                    let has_tool_calls = msg
                        .get("tool_calls")
                        .and_then(|t| t.as_array())
                        .map(|tc| !tc.is_empty())
                        .unwrap_or(false);
                    if has_tool_calls {
                        let mut blocks = Vec::new();
                        if let Some(text) = msg.get("content").and_then(|c| c.as_str()) {
                            if !text.is_empty() {
                                blocks.push(json!({"type": "text", "text": text}));
                            }
                        }
                        for tc in msg.get("tool_calls").and_then(|t| t.as_array()).unwrap() {
                            let id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("");
                            let func = tc.get("function");
                            let name = func
                                .and_then(|f| f.get("name"))
                                .and_then(|n| n.as_str())
                                .unwrap_or("");
                            let args_str = func
                                .and_then(|f| f.get("arguments"))
                                .and_then(|a| a.as_str())
                                .unwrap_or("{}");
                            let input: Value = serde_json::from_str(args_str).unwrap_or(json!({}));
                            blocks.push(json!({
                                "type": "tool_use",
                                "id": id,
                                "name": name,
                                "input": input
                            }));
                        }
                        messages.push(json!({"role": "assistant", "content": blocks}));
                    } else {
                        messages.push(msg.clone());
                    }
                }
                _ => messages.push(msg.clone()),
            }
        }
    }

    if !system_parts.is_empty() {
        result["system"] = json!(system_parts.join("\n"));
    }
    result["messages"] = json!(messages);

    if let Some(v) = body.get("max_tokens") {
        result["max_tokens"] = v.clone();
    } else {
        result["max_tokens"] = json!(ANTHROPIC_DEFAULT_MAX_TOKENS);
    }
    if let Some(v) = body.get("temperature") {
        result["temperature"] = v.clone();
    }
    if let Some(v) = body.get("top_p") {
        result["top_p"] = v.clone();
    }
    if let Some(v) = body.get("stop") {
        result["stop_sequences"] = v.clone();
    }
    if let Some(v) = body.get("stream") {
        result["stream"] = v.clone();
    }

    // tools: openai {type:function,function:{name,description,parameters}} → anthropic {name,description,input_schema}
    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let anth_tools: Vec<Value> = tools
            .iter()
            .map(|t| {
                let func = t.get("function");
                json!({
                    "name": func.and_then(|f| f.get("name")).and_then(|n| n.as_str()).unwrap_or(""),
                    "description": func.and_then(|f| f.get("description")).cloned().unwrap_or(json!("")),
                    "input_schema": func.and_then(|f| f.get("parameters")).cloned().unwrap_or(json!({"type": "object"}))
                })
            })
            .collect();
        if !anth_tools.is_empty() {
            result["tools"] = json!(anth_tools);
        }
    }

    result
}

/// Anthropic Messages 请求 → OpenAI Chat 请求
///
/// 移植自 openlocalrouter transform::anthropic_to_openai_chat：
/// system 提取为 system 消息、content blocks（text/image/tool_use/tool_result）
/// → openai messages、tools input_schema → type:function。
pub fn anthropic_to_openai_chat_req(body: &Value) -> Value {
    let mut result = json!({});

    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        result["model"] = json!(model);
    }

    let mut messages = Vec::new();

    // System prompt
    if let Some(system) = body.get("system") {
        if let Some(text) = system.as_str() {
            let text = strip_billing_header(text);
            if !text.is_empty() {
                messages.push(json!({"role": "system", "content": text}));
            }
        } else if let Some(arr) = system.as_array() {
            for msg in arr {
                if let Some(text) = msg.get("text").and_then(|t| t.as_str()) {
                    let text = strip_billing_header(text);
                    if !text.is_empty() {
                        messages.push(json!({"role": "system", "content": text}));
                    }
                }
            }
        }
    }

    // Messages
    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        for msg in msgs {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
            let content = msg.get("content");
            messages.extend(convert_message_to_openai(role, content));
        }
    }

    normalize_system_messages(&mut messages);
    result["messages"] = json!(messages);

    // Parameters
    if let Some(v) = body.get("max_tokens") {
        result["max_tokens"] = v.clone();
    }
    if let Some(v) = body.get("temperature") {
        result["temperature"] = v.clone();
    }
    if let Some(v) = body.get("top_p") {
        result["top_p"] = v.clone();
    }
    if let Some(v) = body.get("stop_sequences") {
        result["stop"] = v.clone();
    }
    if let Some(v) = body.get("stream") {
        result["stream"] = v.clone();
    }

    // Tools
    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let openai_tools: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                        "description": t.get("description"),
                        "parameters": t.get("input_schema").cloned().unwrap_or(json!({}))
                    }
                })
            })
            .collect();
        if !openai_tools.is_empty() {
            result["tools"] = json!(openai_tools);
        }
    }

    if let Some(v) = body.get("tool_choice") {
        result["tool_choice"] = map_tool_choice(v);
    }

    result
}

/// OpenAI Responses 请求 → OpenAI Chat 请求
///
/// 移植自 openlocalrouter transform::openai_responses_to_openai_chat：
/// input（string / 数组）+ instructions（→ system）+ max_output_tokens → max_tokens。
pub fn openai_responses_to_openai_chat_req(body: &Value) -> Value {
    let mut result = json!({});

    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        result["model"] = json!(model);
    }

    let mut messages = Vec::new();

    // instructions → system message
    if let Some(instructions) = body.get("instructions").and_then(|i| i.as_str()) {
        if !instructions.is_empty() {
            messages.push(json!({"role": "system", "content": instructions}));
        }
    }

    // input → messages
    if let Some(input) = body.get("input") {
        match input {
            Value::String(text) => {
                messages.push(json!({"role": "user", "content": text}));
            }
            Value::Array(arr) => {
                for item in arr {
                    messages.push(convert_responses_message(item));
                }
            }
            _ => {}
        }
    }

    result["messages"] = json!(messages);

    // Parameters
    if let Some(v) = body.get("max_output_tokens").or(body.get("max_tokens")) {
        result["max_tokens"] = v.clone();
    }
    if let Some(v) = body.get("temperature") {
        result["temperature"] = v.clone();
    }
    if let Some(v) = body.get("top_p") {
        result["top_p"] = v.clone();
    }
    if let Some(v) = body.get("stream") {
        result["stream"] = v.clone();
    }

    // tools：responses {type:function,name,description,parameters} → openai {type:function,function:{...}}
    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let openai_tools: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                        "description": t.get("description"),
                        "parameters": t.get("parameters").cloned().unwrap_or(json!({}))
                    }
                })
            })
            .collect();
        if !openai_tools.is_empty() {
            result["tools"] = json!(openai_tools);
        }
    }

    result
}

/// OpenAI Chat 请求 → OpenAI Responses 请求
///
/// - system 消息合并 → 顶层 `instructions`；
/// - messages → `input` 数组（user/assistant 交错保留）；
/// - max_tokens → max_output_tokens；
/// - tools → responses 格式（type:function + name/description/parameters 平铺）。
pub fn openai_chat_to_openai_responses_req(body: &Value) -> Value {
    let mut result = json!({});

    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        result["model"] = json!(model);
    }

    let mut instructions: Vec<String> = Vec::new();
    let mut input = Vec::new();

    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        for msg in msgs {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
            if role == "system" {
                if let Some(text) = msg.get("content").and_then(|c| c.as_str()) {
                    if !text.is_empty() {
                        instructions.push(text.to_string());
                    }
                }
                continue;
            }
            let content = msg.get("content").cloned().unwrap_or(Value::Null);
            input.push(json!({"role": role, "content": content}));
        }
    }

    if !instructions.is_empty() {
        result["instructions"] = json!(instructions.join("\n"));
    }
    result["input"] = json!(input);

    if let Some(v) = body.get("max_tokens") {
        result["max_output_tokens"] = v.clone();
    }
    if let Some(v) = body.get("temperature") {
        result["temperature"] = v.clone();
    }
    if let Some(v) = body.get("top_p") {
        result["top_p"] = v.clone();
    }
    if let Some(v) = body.get("stream") {
        result["stream"] = v.clone();
    }

    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let rs_tools: Vec<Value> = tools
            .iter()
            .map(|t| {
                let func = t.get("function");
                json!({
                    "type": "function",
                    "name": func.and_then(|f| f.get("name")).and_then(|n| n.as_str()).unwrap_or(""),
                    "description": func.and_then(|f| f.get("description")).cloned().unwrap_or(json!("")),
                    "parameters": func.and_then(|f| f.get("parameters")).cloned().unwrap_or(json!({"type": "object"}))
                })
            })
            .collect();
        if !rs_tools.is_empty() {
            result["tools"] = json!(rs_tools);
        }
    }

    result
}

/// OpenAI Responses 请求 → Anthropic 请求（链式：responses → openai_chat → anthropic）
pub fn openai_responses_to_anthropic_req(body: &Value) -> Value {
    let chat = openai_responses_to_openai_chat_req(body);
    openai_chat_to_anthropic_req(&chat)
}

/// Anthropic 请求 → OpenAI Responses 请求（链式：anthropic → openai_chat → responses）
pub fn anthropic_to_openai_responses_req(body: &Value) -> Value {
    let chat = anthropic_to_openai_chat_req(body);
    openai_chat_to_openai_responses_req(&chat)
}

// ────────────────────────────────────────────────────────────
// 响应转换
// ────────────────────────────────────────────────────────────

/// OpenAI Chat 响应 → Anthropic Messages 响应
///
/// 移植自 openlocalrouter transform::openai_chat_to_anthropic：
/// choices[0].message → content blocks（thinking/text/refusal/tool_use）、
/// usage prompt/completion → input/output、finish_reason → stop_reason。
pub fn openai_chat_to_anthropic_resp(body: &Value) -> Value {
    let choices = body.get("choices").and_then(|c| c.as_array());
    let choice = choices.and_then(|c| c.first());

    let message = choice.and_then(|c| c.get("message"));
    let Some(message) = message else {
        return json!({
            "id": body.get("id").and_then(|i| i.as_str()).unwrap_or(""),
            "type": "message",
            "role": "assistant",
            "content": [],
            "model": body.get("model").and_then(|m| m.as_str()).unwrap_or(""),
            "stop_reason": null,
            "stop_sequence": null,
            "usage": json!({"input_tokens": 0, "output_tokens": 0})
        });
    };

    let mut content = Vec::new();
    let mut has_tool_use = false;

    if let Some(reasoning) = message.get("reasoning_content").and_then(|r| r.as_str()) {
        if !reasoning.is_empty() {
            content.push(json!({"type": "thinking", "thinking": reasoning}));
        }
    }

    if let Some(msg_content) = message.get("content") {
        if let Some(text) = msg_content.as_str() {
            if !text.is_empty() {
                content.push(json!({"type": "text", "text": text}));
            }
        } else if let Some(parts) = msg_content.as_array() {
            for part in parts {
                let part_type = part.get("type").and_then(|t| t.as_str()).unwrap_or("");
                match part_type {
                    "text" | "output_text" => {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                content.push(json!({"type": "text", "text": text}));
                            }
                        }
                    }
                    "refusal" => {
                        if let Some(refusal) = part.get("refusal").and_then(|r| r.as_str()) {
                            if !refusal.is_empty() {
                                content.push(json!({"type": "text", "text": refusal}));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
        if !tool_calls.is_empty() {
            has_tool_use = true;
        }
        for tc in tool_calls {
            let id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("");
            let func = tc.get("function");
            let name = func
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("");
            let args_str = func
                .and_then(|f| f.get("arguments"))
                .and_then(|a| a.as_str())
                .unwrap_or("{}");
            let input: Value = serde_json::from_str(args_str).unwrap_or(json!({}));

            content.push(json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input
            }));
        }
    }

    let stop_reason = choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(|r| r.as_str())
        .map(|r| match r {
            "length" => "max_tokens",
            "tool_calls" | "function_call" => "tool_use",
            _ => "end_turn",
        })
        .or(if has_tool_use { Some("tool_use") } else { None });

    let usage_json = build_anthropic_usage(body.get("usage"));

    json!({
        "id": body.get("id").and_then(|i| i.as_str()).unwrap_or(""),
        "type": "message",
        "role": "assistant",
        "content": content,
        "model": body.get("model").and_then(|m| m.as_str()).unwrap_or(""),
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": usage_json
    })
}

/// Anthropic 响应 → OpenAI Chat 响应
///
/// content blocks（text/tool_use/thinking）→ choices[0].message（content + tool_calls）、
/// usage input/output → prompt/completion、stop_reason → finish_reason。
pub fn anthropic_to_openai_chat_resp(body: &Value) -> Value {
    let mut text_parts = String::new();
    let mut tool_calls = Vec::new();

    if let Some(content) = body.get("content").and_then(|c| c.as_array()) {
        for block in content {
            match block.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                        text_parts.push_str(t);
                    }
                }
                Some("tool_use") => {
                    let id = block.get("id").and_then(|i| i.as_str()).unwrap_or("");
                    let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let input = block.get("input").cloned().unwrap_or(json!({}));
                    tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": serde_json::to_string(&input).unwrap_or_default()
                        }
                    }));
                }
                _ => {} // thinking / tool_result 等不进 assistant 消息
            }
        }
    }

    let stop_reason = body.get("stop_reason").and_then(|r| r.as_str());
    let finish_reason = match stop_reason {
        Some("max_tokens") => "length",
        Some("tool_use") => "tool_calls",
        Some("stop_sequence") => "stop",
        Some("end_turn") | None => "stop",
        Some(other) => other,
    };

    let usage = body.get("usage");
    let input_tokens = usage
        .and_then(|u| u.get("input_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .and_then(|u| u.get("output_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0);

    let has_tool_calls = !tool_calls.is_empty();
    let content_val = if text_parts.is_empty() && has_tool_calls {
        Value::Null
    } else {
        json!(text_parts)
    };

    json!({
        "id": body.get("id").and_then(|i| i.as_str()).unwrap_or(""),
        "object": "chat.completion",
        "model": body.get("model").and_then(|m| m.as_str()).unwrap_or(""),
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": content_val,
                "tool_calls": if has_tool_calls { json!(tool_calls) } else { Value::Null }
            },
            "finish_reason": json!(finish_reason)
        }],
        "usage": {
            "prompt_tokens": input_tokens,
            "completion_tokens": output_tokens,
            "total_tokens": input_tokens + output_tokens
        }
    })
}

/// OpenAI Chat 响应 → OpenAI Responses 响应
///
/// 移植自 openlocalrouter transform::openai_chat_to_openai_responses：
/// message.content → output[0].message.content（output_text）、
/// tool_calls → function_call output、usage → {input_tokens, output_tokens, total_tokens}。
pub fn openai_chat_to_openai_responses_resp(body: &Value) -> Value {
    let choices = body.get("choices").and_then(|c| c.as_array());
    let choice = choices.and_then(|c| c.first());

    let mut output = Vec::new();

    if let Some(message) = choice.and_then(|c| c.get("message")) {
        let mut content_parts = Vec::new();

        if let Some(text) = message.get("content").and_then(|c| c.as_str()) {
            if !text.is_empty() {
                content_parts.push(json!({"type": "output_text", "text": text, "annotations": []}));
            }
        } else if let Some(parts) = message.get("content").and_then(|c| c.as_array()) {
            for part in parts {
                let part_type = part.get("type").and_then(|t| t.as_str()).unwrap_or("");
                match part_type {
                    "text" | "output_text" => {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            content_parts.push(
                                json!({"type": "output_text", "text": text, "annotations": []}),
                            );
                        }
                    }
                    _ => {}
                }
            }
        }

        if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
            for tc in tool_calls {
                let id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("");
                let func = tc.get("function");
                let name = func
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("");
                let args = func
                    .and_then(|f| f.get("arguments"))
                    .and_then(|a| a.as_str())
                    .unwrap_or("{}");
                let arguments: Value = serde_json::from_str(args).unwrap_or(json!({}));
                output.push(json!({
                    "id": format!("fc_{id}"),
                    "type": "function_call",
                    "call_id": id,
                    "name": name,
                    "arguments": serde_json::to_string(&arguments).unwrap_or_default()
                }));
            }
        }

        if !content_parts.is_empty() {
            output.push(json!({
                "id": body.get("id").and_then(|i| i.as_str()).unwrap_or(""),
                "type": "message",
                "role": "assistant",
                "content": content_parts
            }));
        }
    }

    let usage = body.get("usage");
    let usage_json = match usage {
        Some(u) => json!({
            "input_tokens": u.get("prompt_tokens").and_then(serde_json::Value::as_u64).unwrap_or(0),
            "output_tokens": u.get("completion_tokens").and_then(serde_json::Value::as_u64).unwrap_or(0),
            "total_tokens": u.get("total_tokens").and_then(serde_json::Value::as_u64).unwrap_or(0)
        }),
        None => json!({"input_tokens": 0, "output_tokens": 0, "total_tokens": 0}),
    };

    json!({
        "id": body.get("id").and_then(|i| i.as_str()).unwrap_or(""),
        "object": "response",
        "model": body.get("model").and_then(|m| m.as_str()).unwrap_or(""),
        "output": output,
        "usage": usage_json
    })
}

/// OpenAI Responses 响应 → OpenAI Chat 响应
///
/// output（message.output_text / function_call）→ choices[0].message
/// （content + tool_calls）、usage {input,output} → prompt/completion。
pub fn openai_responses_to_openai_chat_resp(body: &Value) -> Value {
    let mut text_parts = String::new();
    let mut tool_calls = Vec::new();

    if let Some(output) = body.get("output").and_then(|o| o.as_array()) {
        for item in output {
            match item.get("type").and_then(|t| t.as_str()) {
                Some("message") => {
                    if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                        for part in content {
                            if part.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                                if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                    text_parts.push_str(text);
                                }
                            }
                        }
                    }
                }
                Some("function_call") => {
                    let id = item.get("call_id").and_then(|i| i.as_str()).unwrap_or("");
                    let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let args = item
                        .get("arguments")
                        .and_then(|a| a.as_str())
                        .unwrap_or("{}");
                    tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": {"name": name, "arguments": args}
                    }));
                }
                _ => {}
            }
        }
    }

    let usage = body.get("usage");
    let input_tokens = usage
        .and_then(|u| u.get("input_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .and_then(|u| u.get("output_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0);

    let has_tool_calls = !tool_calls.is_empty();
    let content_val = if text_parts.is_empty() && has_tool_calls {
        Value::Null
    } else {
        json!(text_parts)
    };

    json!({
        "id": body.get("id").and_then(|i| i.as_str()).unwrap_or(""),
        "object": "chat.completion",
        "model": body.get("model").and_then(|m| m.as_str()).unwrap_or(""),
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": content_val,
                "tool_calls": if has_tool_calls { json!(tool_calls) } else { Value::Null }
            },
            "finish_reason": if has_tool_calls { json!("tool_calls") } else { json!("stop") }
        }],
        "usage": {
            "prompt_tokens": input_tokens,
            "completion_tokens": output_tokens,
            "total_tokens": input_tokens + output_tokens
        }
    })
}

/// Anthropic 响应 → OpenAI Responses 响应（链式：anthropic → openai_chat → responses）
pub fn anthropic_to_openai_responses_resp(body: &Value) -> Value {
    let chat = anthropic_to_openai_chat_resp(body);
    openai_chat_to_openai_responses_resp(&chat)
}

/// OpenAI Responses 响应 → Anthropic 响应（链式：responses → openai_chat → anthropic）
pub fn openai_responses_to_anthropic_resp(body: &Value) -> Value {
    let chat = openai_responses_to_openai_chat_resp(body);
    openai_chat_to_anthropic_resp(&chat)
}

// ────────────────────────────────────────────────────────────
// helpers（移植自 openlocalrouter transform.rs）
// ────────────────────────────────────────────────────────────

const BILLING_HEADER_PREFIX: &str = "x-anthropic-billing-header:";

fn strip_billing_header(text: &str) -> &str {
    if !text.starts_with(BILLING_HEADER_PREFIX) {
        return text;
    }
    let Some(line_end) = text
        .as_bytes()
        .iter()
        .position(|b| *b == b'\n' || *b == b'\r')
    else {
        return "";
    };
    let bytes = text.as_bytes();
    let mut rest_start = line_end + 1;
    if bytes[line_end] == b'\r' && bytes.get(line_end + 1) == Some(&b'\n') {
        rest_start += 1;
    }
    let rest = &text[rest_start..];
    rest.strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'))
        .or_else(|| rest.strip_prefix('\r'))
        .unwrap_or(rest)
}

fn convert_message_to_openai(role: &str, content: Option<&Value>) -> Vec<Value> {
    let mut result = Vec::new();
    let Some(c) = content else {
        result.push(json!({"role": role, "content": null}));
        return result;
    };

    if let Some(text) = c.as_str() {
        result.push(json!({"role": role, "content": text}));
        return result;
    }

    if let Some(blocks) = c.as_array() {
        let mut text_parts = String::new();
        let mut content_parts = Vec::new();
        let mut tool_calls = Vec::new();

        for block in blocks {
            let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match block_type {
                "text" => {
                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                        text_parts.push_str(text);
                        content_parts.push(json!({"type": "text", "text": text}));
                    }
                }
                "image" => {
                    if let Some(source) = block.get("source") {
                        let media_type = source
                            .get("media_type")
                            .and_then(|m| m.as_str())
                            .unwrap_or("image/png");
                        let data = source.get("data").and_then(|d| d.as_str()).unwrap_or("");
                        content_parts.push(json!({
                            "type": "image_url",
                            "image_url": {"url": format!("data:{};base64,{}", media_type, data)}
                        }));
                    }
                }
                "tool_use" => {
                    let id = block.get("id").and_then(|i| i.as_str()).unwrap_or("");
                    let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let input = block.get("input").cloned().unwrap_or(json!({}));
                    tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": serde_json::to_string(&input).unwrap_or_default()
                        }
                    }));
                }
                "tool_result" => {
                    let tool_use_id = block
                        .get("tool_use_id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("");
                    let content_val = block.get("content");
                    let content_str = match content_val {
                        Some(Value::String(s)) => s.clone(),
                        Some(v) => v.to_string(),
                        None => String::new(),
                    };
                    result.push(json!({
                        "role": "tool",
                        "tool_call_id": tool_use_id,
                        "content": content_str
                    }));
                }
                _ => {}
            }
        }

        if !content_parts.is_empty() || !tool_calls.is_empty() {
            let mut msg = json!({"role": role});

            if text_parts.len() <= 50 && content_parts.len() == 1 {
                msg["content"] = json!(text_parts);
            } else if content_parts.is_empty() {
                msg["content"] = Value::Null;
            } else {
                msg["content"] = json!(content_parts);
            }

            if !tool_calls.is_empty() {
                msg["tool_calls"] = json!(tool_calls);
            }

            result.push(msg);
        }

        return result;
    }

    result.push(json!({"role": role, "content": c}));
    result
}

fn normalize_system_messages(messages: &mut Vec<Value>) {
    let system_count = messages
        .iter()
        .filter(|m| m.get("role").and_then(|v| v.as_str()) == Some("system"))
        .count();
    if system_count <= 1 {
        return;
    }

    let mut parts = Vec::new();
    messages.retain(|m| {
        if m.get("role").and_then(|v| v.as_str()) != Some("system") {
            return true;
        }
        match m.get("content") {
            Some(Value::String(text)) if !text.is_empty() => parts.push(text.clone()),
            _ => {}
        }
        false
    });

    if !parts.is_empty() {
        messages.insert(0, json!({"role": "system", "content": parts.join("\n")}));
    }
}

fn map_tool_choice(tool_choice: &Value) -> Value {
    match tool_choice {
        Value::String(s) => match s.as_str() {
            "any" => json!("required"),
            _ => json!(s),
        },
        Value::Object(obj) => match obj.get("type").and_then(|t| t.as_str()) {
            Some("any") => json!("required"),
            Some("auto") => json!("auto"),
            Some("none") => json!("none"),
            Some("tool") => {
                let name = obj.get("name").and_then(|n| n.as_str()).unwrap_or("");
                json!({"type": "function", "function": {"name": name}})
            }
            _ => tool_choice.clone(),
        },
        _ => tool_choice.clone(),
    }
}

/// openai usage（prompt/completion + cached）→ anthropic usage（input/output + cache_*）
fn build_anthropic_usage(usage: Option<&Value>) -> Value {
    let Some(usage) = usage else {
        return json!({"input_tokens": 0, "output_tokens": 0});
    };

    let cached = usage
        .get("cache_read_input_tokens")
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            usage
                .pointer("/prompt_tokens_details/cached_tokens")
                .and_then(serde_json::Value::as_u64)
        })
        .unwrap_or(0);
    let cache_creation = usage
        .get("cache_creation_input_tokens")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let input_tokens = usage
        .get("prompt_tokens")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        .saturating_sub(cached)
        .saturating_sub(cache_creation);
    let output_tokens = usage
        .get("completion_tokens")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);

    let mut usage_json = json!({
        "input_tokens": input_tokens,
        "output_tokens": output_tokens
    });
    if cached > 0 {
        usage_json["cache_read_input_tokens"] = json!(cached);
    }
    if cache_creation > 0 {
        usage_json["cache_creation_input_tokens"] = json!(cache_creation);
    }
    usage_json
}

/// Responses 入站消息 → openai chat 消息（developer → system、content 数组文本合并）
fn convert_responses_message(item: &Value) -> Value {
    let raw_role = item.get("role").and_then(|r| r.as_str()).unwrap_or("user");
    let role = match raw_role {
        "developer" => "system",
        other => other,
    };
    let content = item.get("content").cloned().unwrap_or(Value::Null);

    if content.is_string() {
        return json!({"role": role, "content": content});
    }

    if let Some(arr) = content.as_array() {
        let mut texts = Vec::new();
        for part in arr {
            if let Some(part_type) = part.get("type").and_then(|t| t.as_str()) {
                match part_type {
                    "input_text" | "output_text" | "text" => {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            texts.push(text.to_string());
                        }
                    }
                    "input_image" => {
                        return json!({"role": role, "content": content});
                    }
                    _ => {}
                }
            }
        }
        if !texts.is_empty() {
            return json!({"role": role, "content": texts.join("\n")});
        }
    }

    json!({"role": role, "content": content})
}

// ────────────────────────────────────────────────────────────
// 测试
// ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn determine_same_protocol_first() {
        let endpoints = vec![
            "anthropic".to_string(),
            "openai_chat".to_string(),
            "responses".to_string(),
        ];
        assert_eq!(
            determine_forwarding_protocol(&endpoints, "openai_chat"),
            Some("openai_chat")
        );
        assert_eq!(
            determine_forwarding_protocol(&endpoints, "responses"),
            Some("responses")
        );
    }

    #[test]
    fn determine_fallback_priority_anthropic() {
        // 只有 anthropic 端点：任何入站协议都转 anthropic
        let only_anth = vec!["anthropic".to_string()];
        assert_eq!(
            determine_forwarding_protocol(&only_anth, "openai_chat"),
            Some("anthropic")
        );
        assert_eq!(
            determine_forwarding_protocol(&only_anth, "responses"),
            Some("anthropic")
        );
        assert_eq!(
            determine_forwarding_protocol(&only_anth, "anthropic"),
            Some("anthropic")
        );
        // openai_chat + responses 均无 → anthropic 优先
        let oc_rs = vec!["openai_chat".to_string(), "responses".to_string()];
        assert_eq!(
            determine_forwarding_protocol(&oc_rs, "anthropic"),
            Some("openai_chat")
        );
        assert_eq!(
            determine_forwarding_protocol(&oc_rs, "openai_chat"),
            Some("openai_chat")
        );
    }

    #[test]
    fn determine_none_when_unavailable() {
        let endpoints: Vec<String> = vec![];
        assert_eq!(
            determine_forwarding_protocol(&endpoints, "openai_chat"),
            None
        );
    }

    #[test]
    fn openai_chat_to_anthropic_system_and_tools() {
        let input = json!({
            "model": "deepseek-chat",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Hello"}
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get weather",
                    "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}
                }
            }]
        });
        let out = openai_chat_to_anthropic_req(&input);
        // system 提取为顶层字段
        assert_eq!(out["system"], "You are helpful.");
        // messages 只保留 user
        assert_eq!(out["messages"].as_array().unwrap().len(), 1);
        assert_eq!(out["messages"][0]["role"], "user");
        // max_tokens 缺失补默认 4096
        assert_eq!(out["max_tokens"], 4096);
        // tools 映射 input_schema
        assert_eq!(out["tools"][0]["name"], "get_weather");
        assert_eq!(
            out["tools"][0]["input_schema"]["properties"]["city"]["type"],
            "string"
        );
    }

    #[test]
    fn openai_chat_to_anthropic_tool_messages() {
        let input = json!({
            "model": "m",
            "messages": [
                {"role": "assistant", "content": null, "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "get_weather", "arguments": "{\"city\":\"Tokyo\"}"}
                }]},
                {"role": "tool", "tool_call_id": "call_1", "content": "Sunny"}
            ]
        });
        let out = openai_chat_to_anthropic_req(&input);
        let msgs = out["messages"].as_array().unwrap();
        // assistant tool_calls → tool_use block
        assert_eq!(msgs[0]["role"], "assistant");
        assert_eq!(msgs[0]["content"][0]["type"], "tool_use");
        assert_eq!(msgs[0]["content"][0]["id"], "call_1");
        assert_eq!(msgs[0]["content"][0]["name"], "get_weather");
        assert_eq!(msgs[0]["content"][0]["input"]["city"], "Tokyo");
        // tool → user + tool_result
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[1]["content"][0]["type"], "tool_result");
        assert_eq!(msgs[1]["content"][0]["tool_use_id"], "call_1");
        assert_eq!(msgs[1]["content"][0]["content"], "Sunny");
    }

    #[test]
    fn anthropic_to_openai_chat_req_roundtrip() {
        let input = json!({
            "model": "claude-x",
            "max_tokens": 1024,
            "system": "Be concise.",
            "messages": [{"role": "user", "content": "Hi"}],
            "tools": [{
                "name": "get_weather",
                "description": "Get weather",
                "input_schema": {"type": "object"}
            }]
        });
        let out = anthropic_to_openai_chat_req(&input);
        assert_eq!(out["messages"][0]["role"], "system");
        assert_eq!(out["messages"][0]["content"], "Be concise.");
        assert_eq!(out["messages"][1]["role"], "user");
        assert_eq!(out["tools"][0]["type"], "function");
        assert_eq!(out["tools"][0]["function"]["name"], "get_weather");
        assert_eq!(out["max_tokens"], 1024);
    }

    #[test]
    fn anthropic_to_openai_tool_use_req() {
        let input = json!({
            "model": "m",
            "max_tokens": 100,
            "messages": [{
                "role": "assistant",
                "content": [{"type": "tool_use", "id": "call_1", "name": "get_weather", "input": {"city": "Tokyo"}}]
            }]
        });
        let out = anthropic_to_openai_chat_req(&input);
        let msg = &out["messages"][0];
        assert!(msg.get("tool_calls").is_some());
        assert_eq!(msg["tool_calls"][0]["id"], "call_1");
        assert_eq!(msg["tool_calls"][0]["function"]["name"], "get_weather");
        assert_eq!(
            msg["tool_calls"][0]["function"]["arguments"],
            "{\"city\":\"Tokyo\"}"
        );
    }

    #[test]
    fn responses_req_to_openai_chat() {
        let input = json!({
            "model": "gpt-x",
            "instructions": "Be brief.",
            "input": [
                {"role": "user", "content": [{"type": "input_text", "text": "Hello"}]},
                {"role": "assistant", "content": [{"type": "output_text", "text": "Hi there"}]}
            ],
            "max_output_tokens": 512
        });
        let out = openai_responses_to_openai_chat_req(&input);
        assert_eq!(out["messages"][0]["role"], "system");
        assert_eq!(out["messages"][0]["content"], "Be brief.");
        assert_eq!(out["messages"][1]["role"], "user");
        assert_eq!(out["messages"][1]["content"], "Hello");
        assert_eq!(out["messages"][2]["role"], "assistant");
        assert_eq!(out["messages"][2]["content"], "Hi there");
        assert_eq!(out["max_tokens"], 512);
    }

    #[test]
    fn openai_chat_req_to_responses_roundtrip() {
        let input = json!({
            "model": "m",
            "messages": [
                {"role": "system", "content": "Sys"},
                {"role": "user", "content": "Hello"}
            ],
            "max_tokens": 100
        });
        let out = openai_chat_to_openai_responses_req(&input);
        assert_eq!(out["instructions"], "Sys");
        assert_eq!(out["input"][0]["role"], "user");
        assert_eq!(out["input"][0]["content"], "Hello");
        assert_eq!(out["max_output_tokens"], 100);
        // 链式：responses → openai_chat → anthropic 全链路无 panic 且消息保留
        let anth = openai_responses_to_anthropic_req(&out);
        assert_eq!(anth["system"], "Sys");
        assert_eq!(anth["messages"][0]["role"], "user");
        assert_eq!(anth["messages"][0]["content"], "Hello");
    }

    #[test]
    fn openai_chat_resp_to_anthropic() {
        let input = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello!"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5}
        });
        let out = openai_chat_to_anthropic_resp(&input);
        assert_eq!(out["type"], "message");
        assert_eq!(out["content"][0]["type"], "text");
        assert_eq!(out["content"][0]["text"], "Hello!");
        assert_eq!(out["stop_reason"], "end_turn");
        assert_eq!(out["usage"]["input_tokens"], 10);
        assert_eq!(out["usage"]["output_tokens"], 5);
    }

    #[test]
    fn anthropic_resp_to_openai_chat() {
        let input = json!({
            "id": "msg_123",
            "type": "message",
            "model": "claude-x",
            "content": [
                {"type": "text", "text": "Hello!"},
                {"type": "tool_use", "id": "call_1", "name": "get_weather", "input": {"city": "Tokyo"}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });
        let out = anthropic_to_openai_chat_resp(&input);
        let choice = &out["choices"][0];
        assert_eq!(choice["message"]["content"], "Hello!");
        assert_eq!(choice["message"]["tool_calls"][0]["id"], "call_1");
        assert_eq!(
            choice["message"]["tool_calls"][0]["function"]["name"],
            "get_weather"
        );
        assert_eq!(out["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(out["usage"]["prompt_tokens"], 10);
        assert_eq!(out["usage"]["completion_tokens"], 5);
    }

    #[test]
    fn openai_chat_resp_to_responses() {
        let input = json!({
            "id": "chatcmpl-1",
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hi"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 7, "completion_tokens": 3, "total_tokens": 10}
        });
        let out = openai_chat_to_openai_responses_resp(&input);
        assert_eq!(out["object"], "response");
        assert_eq!(out["output"][0]["type"], "message");
        assert_eq!(out["output"][0]["content"][0]["type"], "output_text");
        assert_eq!(out["output"][0]["content"][0]["text"], "Hi");
        assert_eq!(out["usage"]["input_tokens"], 7);
        assert_eq!(out["usage"]["output_tokens"], 3);
    }

    #[test]
    fn responses_resp_to_openai_chat() {
        let input = json!({
            "id": "resp_1",
            "object": "response",
            "model": "gpt-4o",
            "output": [{
                "id": "msg_1",
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "Hello", "annotations": []}]
            }],
            "usage": {"input_tokens": 7, "output_tokens": 3, "total_tokens": 10}
        });
        let out = openai_responses_to_openai_chat_resp(&input);
        assert_eq!(out["choices"][0]["message"]["content"], "Hello");
        assert_eq!(out["usage"]["prompt_tokens"], 7);
        assert_eq!(out["usage"]["completion_tokens"], 3);
        // 链式 → anthropic
        let anth = openai_responses_to_anthropic_resp(&input);
        assert_eq!(anth["content"][0]["type"], "text");
        assert_eq!(anth["content"][0]["text"], "Hello");
        assert_eq!(anth["usage"]["input_tokens"], 7);
        // 反向链式 → responses（anthropic → openai → responses）
        let back = anthropic_to_openai_responses_resp(&anth);
        assert_eq!(back["object"], "response");
        assert_eq!(back["output"][0]["content"][0]["text"], "Hello");
    }

    #[test]
    fn anthropic_resp_to_responses_chain() {
        let input = json!({
            "id": "msg_2",
            "type": "message",
            "model": "claude-x",
            "content": [{"type": "text", "text": "Bye"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 4, "output_tokens": 2}
        });
        let out = anthropic_to_openai_responses_resp(&input);
        assert_eq!(out["object"], "response");
        assert_eq!(out["output"][0]["content"][0]["text"], "Bye");
        assert_eq!(out["usage"]["input_tokens"], 4);
        assert_eq!(out["usage"]["output_tokens"], 2);
    }

    #[test]
    fn transform_request_dispatch_same_protocol_passthrough() {
        let body = json!({"model": "m", "messages": [{"role": "user", "content": "hi"}]});
        let out = transform_request(&body, "openai_chat", "openai_chat");
        assert_eq!(out, body);
        // 未知协议对 → 原样
        let out2 = transform_request(&body, "grpc", "openai_chat");
        assert_eq!(out2, body);
    }
}
