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
//! 流式 SSE 转换不在本模块，见 src/sse.rs（P3-B 已实现；本模块只处理非流式）。

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
            "stop_reason": openai_chat_to_anthropic_stop_reason(None, false),
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

    let finish_reason = choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(|r| r.as_str());
    let stop_reason = openai_chat_to_anthropic_stop_reason(finish_reason, has_tool_use);

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
    let finish_reason = anthropic_to_openai_chat_finish_reason(stop_reason);

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

/// Responses 的 **`message` 条目**（`type: "message"`）。
///
/// **唯一真源**：整包路径（`openai_chat_to_openai_responses_resp`）与流式终局事件
/// （`sse::ResponsesStreamState::terminal_response`）都调用本函数 —— 两条路径各写一份条目
/// 形状，正是「同一协议对两条路径给出不同形状」这类缺陷的来源。
///
/// `message_item_id` 由调用方给出：整包路径沿用响应 id（历史行为，勿改），流式路径必须
/// 用它自己在增量事件里已经发过的 `msg_…` id，否则终局对象与自己的增量自相矛盾。
///
/// `content` 为字符串或 content-part 数组；没有任何可输出文本时返回 `None`（整包路径据此
/// 省略 message 条目，流式路径据此判断该条目是否存在于终局 `output` 里）。
pub fn openai_chat_message_item(message_item_id: &str, content: Option<&Value>) -> Option<Value> {
    let mut content_parts = Vec::new();

    match content {
        Some(Value::String(text)) => {
            if !text.is_empty() {
                content_parts.push(json!({"type": "output_text", "text": text, "annotations": []}));
            }
        }
        Some(Value::Array(parts)) => {
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
        _ => {}
    }

    if content_parts.is_empty() {
        return None;
    }
    Some(json!({
        "id": message_item_id,
        "type": "message",
        "role": "assistant",
        "content": content_parts
    }))
}

/// Responses 的 **`function_call` 条目**（`type: "function_call"`）。
///
/// **唯一真源**：整包路径与流式终局事件共用。`arguments` 做一次 parse → 紧凑序列化
/// （上游可能给带空白的 JSON；parse 失败则给 `{}`）。
pub fn openai_chat_tool_call_item(tool_call: &Value) -> Value {
    let id = tool_call.get("id").and_then(|i| i.as_str()).unwrap_or("");
    let func = tool_call.get("function");
    let name = func
        .and_then(|f| f.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("");
    let args = func
        .and_then(|f| f.get("arguments"))
        .and_then(|a| a.as_str())
        .unwrap_or("{}");
    let arguments: Value = serde_json::from_str(args).unwrap_or(json!({}));
    json!({
        "id": format!("fc_{id}"),
        "type": "function_call",
        "call_id": id,
        "name": name,
        "arguments": serde_json::to_string(&arguments).unwrap_or_default()
    })
}

/// OpenAI Chat 的 `message`（`content` + `tool_calls`）→ Responses 的 `output` 数组。
///
/// 本函数只负责**整包路径的拼装顺序**（既有约定：`function_call` 在前、`message` 在后）；
/// 条目形状本身来自上面两个唯一真源函数。
///
/// ⚠️ 流式路径**不**走本函数：它按自己的**宣布顺序**拼装终局 `output`
/// （`sse::ResponsesStreamState::terminal_response`），否则终局顺序会与它已经发出的
/// `response.output_item.added` 顺序矛盾。顺序规则两条路径各自与自己的权威一致，形状同源。
pub fn openai_chat_message_to_responses_output(
    message: &Value,
    message_item_id: &str,
) -> Vec<Value> {
    let mut output = Vec::new();

    if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
        for tc in tool_calls {
            output.push(openai_chat_tool_call_item(tc));
        }
    }

    if let Some(item) = openai_chat_message_item(message_item_id, message.get("content")) {
        output.push(item);
    }

    output
}

/// OpenAI Chat 的 `usage` → Responses 的 `usage`。
///
/// **唯一真源**：整包路径与流式终局事件共用。usage 缺失时给出三个 0（与整包路径一致），
/// 而不是省略该字段 —— 客户端的 `response.usage` 因此总是存在。
pub fn openai_usage_to_responses_usage(usage: Option<&Value>) -> Value {
    match usage {
        Some(u) => json!({
            "input_tokens": u.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0),
            "output_tokens": u.get("completion_tokens").and_then(Value::as_u64).unwrap_or(0),
            "total_tokens": u.get("total_tokens").and_then(Value::as_u64).unwrap_or(0)
        }),
        None => json!({"input_tokens": 0, "output_tokens": 0, "total_tokens": 0}),
    }
}

/// OpenAI Chat 响应 → OpenAI Responses 响应
///
/// 移植自 openlocalrouter transform::openai_chat_to_openai_responses：
/// message.content → output[0].message.content（output_text）、
/// tool_calls → function_call output、usage → {input_tokens, output_tokens, total_tokens}。
/// output / usage 的构造走上面的两个唯一真源函数（流式终局事件也调用它们）。
pub fn openai_chat_to_openai_responses_resp(body: &Value) -> Value {
    let choices = body.get("choices").and_then(|c| c.as_array());
    let choice = choices.and_then(|c| c.first());

    let mut output = Vec::new();

    if let Some(message) = choice.and_then(|c| c.get("message")) {
        // message item 的 id 沿用响应 id（历史行为；与流式的 `msg_…` 前缀分叉已记录，
        // 不在本次改动范围内 —— 见 sse.rs 终局对象的注释）。
        output = openai_chat_message_to_responses_output(
            message,
            body.get("id").and_then(|i| i.as_str()).unwrap_or(""),
        );
    }

    json!({
        "id": body.get("id").and_then(|i| i.as_str()).unwrap_or(""),
        "object": "response",
        "model": body.get("model").and_then(|m| m.as_str()).unwrap_or(""),
        "output": output,
        "usage": openai_usage_to_responses_usage(body.get("usage"))
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
            "finish_reason": openai_responses_to_openai_chat_finish_reason(has_tool_calls)
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
// 完成信号（completion signal）
//
// 同一个协议对有两条翻译路径：流式（`crate::sse`）与整包（本模块）。客户端只会看到
// 其中一条，两条给出不同的完成信号时，「是否正常结束 / 要不要派发工具 / 是否被截断」
// 在流式与非流式下就有了不同答案。下面三个函数是各自的**唯一真源**，两条路径都调用
// 它们，任何一侧都不得再写内联 match（内联副本正是这类分叉的来源）。
// ────────────────────────────────────────────────────────────

/// Anthropic `stop_reason` → OpenAI Chat `finish_reason`。
///
/// OpenAI 的 `finish_reason` 是封闭集合，anthropic 的其余取值（`refusal`、`pause_turn`、
/// 以及未来新增值）与 `None` 一样按「正常结束」收敛为 `stop`——不要把 anthropic 的
/// 原始值透传给 OpenAI 客户端。
pub fn anthropic_to_openai_chat_finish_reason(stop_reason: Option<&str>) -> &'static str {
    match stop_reason {
        Some("max_tokens") => "length",
        Some("tool_use") => "tool_calls",
        _ => "stop",
    }
}

/// OpenAI Chat `finish_reason` → Anthropic `stop_reason`。
///
/// `has_tool_calls`：本次响应是否带过 tool call。上游可能把 `finish_reason` 留空
/// （`null`）却仍然返回了 tool_calls，此时按 `tool_use` 收尾，否则客户端不会派发工具。
pub fn openai_chat_to_anthropic_stop_reason(
    finish_reason: Option<&str>,
    has_tool_calls: bool,
) -> Option<String> {
    finish_reason
        .map(|r| match r {
            "length" => "max_tokens",
            "tool_calls" | "function_call" => "tool_use",
            _ => "end_turn",
        })
        .map(str::to_string)
        .or_else(|| has_tool_calls.then(|| "tool_use".to_string()))
}

/// OpenAI Responses 侧没有 `finish_reason` 字段，完成信号只能由内容推断：
/// 带过 tool call → `tool_calls`，否则 `stop`。
pub fn openai_responses_to_openai_chat_finish_reason(has_tool_calls: bool) -> &'static str {
    if has_tool_calls {
        "tool_calls"
    } else {
        "stop"
    }
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
///
/// 缓存命中拼写优先级（与流式同族实现 `sse::extract_cache_read_tokens` 一致，
/// rant 2026-08-23T08:20:38）：DeepSeek 原生顶层 `prompt_cache_hit_tokens`
/// → Anthropic `cache_read_input_tokens` → OpenAI `prompt_tokens_details.cached_tokens`。
/// 值为 0 的拼写视为「未上报」，继续向后回落，因此同一份上游 usage 无论走流式
/// 还是非流式转换，都会得到相同的 `(input, cached)` 分解。
fn build_anthropic_usage(usage: Option<&Value>) -> Value {
    let Some(usage) = usage else {
        return json!({"input_tokens": 0, "output_tokens": 0});
    };

    let cached = usage
        .get("prompt_cache_hit_tokens")
        .and_then(serde_json::Value::as_u64)
        .filter(|&v| v > 0)
        .or_else(|| {
            usage
                .get("cache_read_input_tokens")
                .and_then(serde_json::Value::as_u64)
                .filter(|&v| v > 0)
        })
        .or_else(|| {
            usage
                .pointer("/prompt_tokens_details/cached_tokens")
                .and_then(serde_json::Value::as_u64)
                .filter(|&v| v > 0)
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

    /// 把一份上游 `usage` 装进最小可用的 openai_chat 响应体，返回翻译后的 anthropic usage。
    fn anthropic_usage_of(usage: Value) -> Value {
        openai_chat_to_anthropic_resp(&json!({
            "id": "chatcmpl-1",
            "model": "m",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "hi"},
                "finish_reason": "stop"
            }],
            "usage": usage
        }))["usage"]
            .clone()
    }

    /// 非流式 openai_chat → anthropic 翻译必须识别 DeepSeek 原生顶层拼写
    /// `prompt_cache_hit_tokens`（本缺陷的轴），并使命中量与 `input_tokens` 互斥。
    ///
    /// 夹具沿用流式同族测试 `sse::tests::deepseek_cache_hit_tokens_spelling` 的数字：
    /// `prompt_tokens=100` 含 90 命中 → `input=10 / cached=90 / output=50`。
    #[test]
    fn openai_chat_resp_to_anthropic_deepseek_cache_spelling() {
        let ds = anthropic_usage_of(json!({
            "prompt_tokens": 100,
            "prompt_cache_hit_tokens": 90,
            "completion_tokens": 50
        }));
        assert_eq!(ds["cache_read_input_tokens"], 90, "hit forwarded: {ds}");
        assert_eq!(ds["input_tokens"], 10, "input disjoint: {ds}");
        assert_eq!(ds["output_tokens"], 50, "output: {ds}");

        // 优先级与 sse.rs 一致：DeepSeek 原生在前
        let both = anthropic_usage_of(json!({
            "prompt_tokens": 100,
            "prompt_cache_hit_tokens": 80,
            "prompt_tokens_details": {"cached_tokens": 90},
            "completion_tokens": 50
        }));
        assert_eq!(both["cache_read_input_tokens"], 80, "deepseek wins: {both}");
        assert_eq!(both["input_tokens"], 20, "input disjoint: {both}");

        // 值为 0 的拼写＝未上报，继续回落（与 sse.rs 的 `> 0` 语义一致）
        let zero = anthropic_usage_of(json!({
            "prompt_tokens": 100,
            "prompt_cache_hit_tokens": 0,
            "prompt_tokens_details": {"cached_tokens": 90},
            "completion_tokens": 50
        }));
        assert_eq!(
            zero["cache_read_input_tokens"], 90,
            "zero falls through: {zero}"
        );
        assert_eq!(zero["input_tokens"], 10, "input disjoint: {zero}");
    }

    /// 阳性对照：OpenAI 拼写 `prompt_tokens_details.cached_tokens` 与 Anthropic 拼写
    /// `cache_read_input_tokens` 在**修复前后都必须绿**——否则上面那条红不构成证据。
    #[test]
    fn openai_chat_resp_to_anthropic_other_cache_spellings() {
        let openai = anthropic_usage_of(json!({
            "prompt_tokens": 100,
            "prompt_tokens_details": {"cached_tokens": 90},
            "completion_tokens": 50
        }));
        assert_eq!(openai["cache_read_input_tokens"], 90, "hit: {openai}");
        assert_eq!(openai["input_tokens"], 10, "input disjoint: {openai}");

        let anthropic = anthropic_usage_of(json!({
            "prompt_tokens": 100,
            "cache_read_input_tokens": 90,
            "completion_tokens": 50
        }));
        assert_eq!(anthropic["cache_read_input_tokens"], 90, "hit: {anthropic}");
        assert_eq!(anthropic["input_tokens"], 10, "input disjoint: {anthropic}");
    }

    /// 跨路径一致性：同一份上游 usage，流式转换器（`sse.rs`）与非流式翻译器
    /// （本模块）必须报出**相同**的缓存命中分解。
    ///
    /// 这正是当初能拦住「只认两种拼写」的结构性约束——两条路径各自演化时，
    /// 任何一条漏掉某个拼写都会在这里对不上。
    #[test]
    fn anthropic_usage_breakdown_agrees_across_transports() {
        use crate::sse::{openai_sse_to_anthropic, usage_slot};
        use futures_util::StreamExt;

        let usage = json!({
            "prompt_tokens": 100,
            "prompt_cache_hit_tokens": 90,
            "completion_tokens": 50
        });

        // ── 非流式：整份响应一次到达 ─────────────────────────────────────
        let non_stream = openai_chat_to_anthropic_resp(&json!({
            "id": "c1",
            "model": "m1",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "x"},
                "finish_reason": "stop"
            }],
            "usage": usage.clone()
        }))["usage"]
            .clone();

        // ── 流式：同一份 usage 装进一个 SSE chunk ────────────────────────
        let slot = usage_slot();
        let chunk = format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({
                "id": "c1",
                "model": "m1",
                "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                "usage": usage.clone()
            })
        );
        let inbound =
            futures_util::stream::iter(vec![Ok::<_, std::io::Error>(bytes::Bytes::from(chunk))]);
        let streamed = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let mut text = String::new();
                let mut out = std::pin::pin!(openai_sse_to_anthropic(inbound, slot.clone()));
                while let Some(item) = out.next().await {
                    if let Ok(b) = item {
                        text.push_str(&String::from_utf8_lossy(&b));
                    }
                }
                text
            });

        // 流式 usage 落在结尾的 message_delta（message_start 早于 usage 到达）
        let stream_usage = streamed
            .lines()
            .filter_map(|l| l.strip_prefix("data: "))
            .filter_map(|d| serde_json::from_str::<Value>(d).ok())
            .find(|e| e["type"] == "message_delta")
            .map(|e| e["usage"].clone())
            .unwrap_or_else(|| panic!("no message_delta usage in stream: {streamed}"));

        let Some(captured) = *slot.lock().unwrap() else {
            panic!("streaming usage slot is empty");
        };
        assert_eq!((10.0, 90.0, 50.0), captured, "streaming capture");

        // ── 两条路径必须逐字段一致 ───────────────────────────────────────
        for field in ["input_tokens", "cache_read_input_tokens", "output_tokens"] {
            assert_eq!(
                stream_usage[field], non_stream[field],
                "stream vs non-stream disagree on {field}: \
                 stream={stream_usage} non-stream={non_stream}"
            );
        }
        assert_eq!(non_stream["cache_read_input_tokens"], 90, "{non_stream}");
        assert_eq!(non_stream["input_tokens"], 10, "{non_stream}");
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

    /// 完成信号的三个唯一真源：覆盖各自协议文档中的全部取值 + 未知取值对照。
    ///
    /// 流式（`crate::sse`）与整包翻译器都只调用这三个函数，所以这里钉住的是**两条路径
    /// 共用的口径**；`sse::tests::completion_signal_agrees_between_stream_and_whole_body`
    /// 再从真实路径两侧各跑一遍（同一输入必须同值）。
    #[test]
    fn completion_signal_helpers_cover_every_documented_value() {
        // Anthropic stop_reason → OpenAI finish_reason
        for (stop_reason, expected) in [
            ("end_turn", "stop"),
            ("stop_sequence", "stop"),
            ("max_tokens", "length"),
            ("tool_use", "tool_calls"),
            // OpenAI 的 finish_reason 是封闭集合：anthropic 专有的取值必须收敛为「正常结束」
            ("pause_turn", "stop"),
            ("refusal", "stop"),
            // 未知取值（含 anthropic 未来新增值、以及误传 OpenAI 的取值）同样收敛，绝不透传
            ("content_filter", "stop"),
            ("not_a_reason", "stop"),
        ] {
            assert_eq!(
                anthropic_to_openai_chat_finish_reason(Some(stop_reason)),
                expected,
                "anthropic stop_reason={stop_reason}"
            );
        }
        assert_eq!(anthropic_to_openai_chat_finish_reason(None), "stop");

        // OpenAI finish_reason → Anthropic stop_reason
        for (finish_reason, expected) in [
            ("stop", "end_turn"),
            ("length", "max_tokens"),
            ("tool_calls", "tool_use"),
            ("function_call", "tool_use"),
            ("content_filter", "end_turn"),
            ("not_a_reason", "end_turn"),
        ] {
            assert_eq!(
                openai_chat_to_anthropic_stop_reason(Some(finish_reason), false).as_deref(),
                Some(expected),
                "openai finish_reason={finish_reason}"
            );
        }
        // 上游没有给 finish_reason：没有 tool call 就「没有信号」，带过 tool call 则必须 tool_use
        assert_eq!(openai_chat_to_anthropic_stop_reason(None, false), None);
        assert_eq!(
            openai_chat_to_anthropic_stop_reason(None, true).as_deref(),
            Some("tool_use")
        );
        // 明确给了 finish_reason 时，内容不再覆盖它（保持既有语义）
        assert_eq!(
            openai_chat_to_anthropic_stop_reason(Some("stop"), true).as_deref(),
            Some("end_turn")
        );

        // Responses 侧没有 finish_reason，只能由内容推断
        assert_eq!(
            openai_responses_to_openai_chat_finish_reason(true),
            "tool_calls"
        );
        assert_eq!(openai_responses_to_openai_chat_finish_reason(false), "stop");
    }
}
