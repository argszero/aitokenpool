//! SSE 流式协议转换（P3-B，rant 2026-08-18T18:59:29）
//!
//! openai_chat / anthropic / responses 三种协议的流式 SSE 事件互转：
//! - openai_chat 上游 → anthropic 客户端（openai_sse_to_anthropic，移植自 openlocalrouter）
//! - openai_chat 上游 → responses 客户端（openai_sse_to_openai_responses，移植）
//! - anthropic 上游 → openai_chat 客户端（anthropic_sse_to_openai，新增）
//! - anthropic 上游 → responses 客户端（anthropic_sse_to_responses，经 openai 中间态）
//! - responses 上游 → openai_chat 客户端（responses_sse_to_openai_chat，新增）
//! - responses 上游 → anthropic 客户端（P3-B 延后，gateway 侧返回 400）
//!
//! 所有转换器在流内提取 usage 并写入共享 UsageSlot，供 gateway 流尾入账。

use bytes::Bytes;
use futures_util::Stream;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fmt::Write;
use std::io;

/// 流内 usage 提取槽：(input_tokens, output_tokens)
pub type UsageSlot = std::sync::Arc<std::sync::Mutex<Option<(f64, f64)>>>;

/// 新建空 usage 槽
pub fn usage_slot() -> UsageSlot {
    std::sync::Arc::new(std::sync::Mutex::new(None))
}

fn record_usage(slot: &UsageSlot, input: f64, output: f64) {
    if let Ok(mut s) = slot.lock() {
        *s = Some((input, output));
    }
}

// ── SSE 解析工具（移植自 openlocalrouter/src/router/sse.rs）────────────────

#[inline]
pub(crate) fn strip_sse_field<'a>(line: &'a str, field: &str) -> Option<&'a str> {
    line.strip_prefix(&format!("{field}: "))
        .or_else(|| line.strip_prefix(&format!("{field}:")))
}

#[inline]
pub(crate) fn take_sse_block(buffer: &mut String) -> Option<String> {
    let mut best: Option<(usize, usize)> = None;

    for (delimiter, len) in [("\r\n\r\n", 4usize), ("\n\n", 2usize)] {
        if let Some(pos) = buffer.find(delimiter) {
            if best.is_none_or(|(best_pos, _)| pos < best_pos) {
                best = Some((pos, len));
            }
        }
    }

    let (pos, len) = best?;
    let block = buffer[..pos].to_string();
    buffer.drain(..pos + len);
    Some(block)
}

/// 追加原始字节到 UTF-8 String 缓冲，正确处理跨 chunk 边界的多字节字符
pub(crate) fn append_utf8_safe(buffer: &mut String, remainder: &mut Vec<u8>, new_bytes: &[u8]) {
    let (owned, bytes): (Option<Vec<u8>>, &[u8]) = if remainder.is_empty() {
        (None, new_bytes)
    } else if remainder.len() > 3 {
        buffer.push_str(&String::from_utf8_lossy(remainder));
        remainder.clear();
        (None, new_bytes)
    } else {
        let mut combined = std::mem::take(remainder);
        combined.extend_from_slice(new_bytes);
        (Some(combined), &[])
    };
    let input = owned.as_deref().unwrap_or(bytes);

    let mut pos = 0;
    loop {
        match std::str::from_utf8(&input[pos..]) {
            Ok(s) => {
                buffer.push_str(s);
                return;
            }
            Err(e) => {
                let valid_up_to = pos + e.valid_up_to();
                let valid_slice = &input[pos..valid_up_to];
                match std::str::from_utf8(valid_slice) {
                    Ok(valid) => buffer.push_str(valid),
                    Err(_) => buffer.push_str(&String::from_utf8_lossy(valid_slice)),
                }
                if let Some(invalid_len) = e.error_len() {
                    buffer.push('\u{FFFD}');
                    pos = valid_up_to + invalid_len;
                } else {
                    *remainder = input[valid_up_to..].to_vec();
                    return;
                }
            }
        }
    }
}

// ── OpenAI 流式 chunk 数据结构（移植）─────────────────────────────────────

#[derive(Debug, Deserialize)]
struct OpenAIStreamChunk {
    #[serde(default)]
    id: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<StreamUsage>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: Delta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default, alias = "reasoning_content")]
    reasoning: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<DeltaToolCall>>,
}

#[derive(Debug, Deserialize)]
struct DeltaToolCall {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "type", default)]
    _call_type: Option<String>,
    #[serde(default)]
    function: Option<DeltaFunction>,
}

#[derive(Debug, Deserialize)]
struct DeltaFunction {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    prompt_tokens_details: Option<PromptTokensDetails>,
    #[serde(default)]
    cache_read_input_tokens: Option<u32>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct PromptTokensDetails {
    #[serde(default)]
    cached_tokens: u32,
}

// ── openai_sse_to_anthropic（移植自 openlocalrouter streaming.rs L96-509）──

/// Tool block 状态追踪
#[derive(Debug, Clone)]
struct ToolBlockState {
    anthropic_index: u32,
    id: String,
    name: String,
    started: bool,
    pending_args: String,
}

/// OpenAI SSE 流 → Anthropic SSE 流（含 tool/thinking/usage 转换）
pub fn openai_sse_to_anthropic<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    usage: UsageSlot,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();
        let mut message_id = None;
        let mut current_model = None;
        let mut next_content_index: u32 = 0;
        let mut has_sent_message_start = false;
        let mut has_emitted_message_delta = false;
        let mut pending_message_delta: Option<(Option<String>, Option<Value>)> = None;
        let mut has_sent_message_stop = false;
        let mut stream_ended_with_error = false;
        let mut latest_usage: Option<Value> = None;
        let mut current_non_tool_block_type: Option<&'static str> = None;
        let mut current_non_tool_block_index: Option<u32> = None;
        let mut tool_blocks_by_index: HashMap<usize, ToolBlockState> = HashMap::new();
        let mut open_tool_block_indices: Vec<u32> = Vec::new();

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);

                    while let Some(line) = take_sse_block(&mut buffer) {
                        if line.trim().is_empty() {
                            continue;
                        }

                        for l in line.lines() {
                            if let Some(data) = strip_sse_field(l, "data") {
                                if data.trim() == "[DONE]" {
                                    if let Some((stop_reason, usage_json)) = pending_message_delta.take() {
                                        let event = build_message_delta_event(stop_reason.as_ref(), usage_json);
                                        let sse_data = format!(
                                            "event: message_delta\ndata: {}\n\n",
                                            serde_json::to_string(&event).unwrap_or_default()
                                        );
                                        yield Ok(Bytes::from(sse_data));
                                    }

                                    let event = json!({"type": "message_stop"});
                                    let sse_data = format!(
                                        "event: message_stop\ndata: {}\n\n",
                                        serde_json::to_string(&event).unwrap_or_default()
                                    );
                                    yield Ok(Bytes::from(sse_data));
                                    has_sent_message_stop = true;
                                    continue;
                                }

                                if let Ok(chunk) = serde_json::from_str::<OpenAIStreamChunk>(data) {
                                    if message_id.is_none() && !chunk.id.is_empty() {
                                        message_id = Some(chunk.id.clone());
                                    }
                                    if current_model.is_none() && !chunk.model.is_empty() {
                                        current_model = Some(chunk.model.clone());
                                    }

                                    let chunk_usage_json =
                                        chunk.usage.as_ref().map(build_anthropic_usage_json);
                                    if let Some(ref usage_json) = chunk_usage_json {
                                        latest_usage = Some(usage_json.clone());
                                        if let Some((_, ref mut pending_usage)) = pending_message_delta {
                                            *pending_usage = Some(usage_json.clone());
                                        }
                                        // 计量：openai 原样 usage → (prompt, completion)
                                        if let Some(u) = &chunk.usage {
                                            record_usage(&usage, u.prompt_tokens as f64, u.completion_tokens as f64);
                                        }
                                    }

                                    if let Some(choice) = chunk.choices.first() {
                                        // 首个 chunk 发 message_start
                                        if !has_sent_message_start {
                                            let event = json!({
                                                "type": "message_start",
                                                "message": {
                                                    "id": message_id.clone().unwrap_or_default(),
                                                    "type": "message",
                                                    "role": "assistant",
                                                    "model": current_model.clone().unwrap_or_default(),
                                                    "usage": { "input_tokens": 0, "output_tokens": 0 }
                                                }
                                            });
                                            let sse_data = format!(
                                                "event: message_start\ndata: {}\n\n",
                                                serde_json::to_string(&event).unwrap_or_default()
                                            );
                                            yield Ok(Bytes::from(sse_data));
                                            has_sent_message_start = true;
                                        }

                                        // reasoning（thinking）
                                        if let Some(reasoning) = &choice.delta.reasoning {
                                            if current_non_tool_block_type != Some("thinking") {
                                                if let Some(index) = current_non_tool_block_index.take() {
                                                    let event = json!({
                                                        "type": "content_block_stop", "index": index
                                                    });
                                                    let sse_data = format!(
                                                        "event: content_block_stop\ndata: {}\n\n",
                                                        serde_json::to_string(&event).unwrap_or_default()
                                                    );
                                                    yield Ok(Bytes::from(sse_data));
                                                }
                                                let index = next_content_index;
                                                next_content_index += 1;
                                                let event = json!({
                                                    "type": "content_block_start",
                                                    "index": index,
                                                    "content_block": { "type": "thinking", "thinking": "" }
                                                });
                                                let sse_data = format!(
                                                    "event: content_block_start\ndata: {}\n\n",
                                                    serde_json::to_string(&event).unwrap_or_default()
                                                );
                                                yield Ok(Bytes::from(sse_data));
                                                current_non_tool_block_type = Some("thinking");
                                                current_non_tool_block_index = Some(index);
                                            }
                                            if let Some(index) = current_non_tool_block_index {
                                                let event = json!({
                                                    "type": "content_block_delta",
                                                    "index": index,
                                                    "delta": { "type": "thinking_delta", "thinking": reasoning }
                                                });
                                                let sse_data = format!(
                                                    "event: content_block_delta\ndata: {}\n\n",
                                                    serde_json::to_string(&event).unwrap_or_default()
                                                );
                                                yield Ok(Bytes::from(sse_data));
                                            }
                                        }

                                        // 文本内容
                                        if let Some(content) = &choice.delta.content {
                                            if !content.is_empty() {
                                                if current_non_tool_block_type != Some("text") {
                                                    if let Some(index) = current_non_tool_block_index.take() {
                                                        let event = json!({
                                                            "type": "content_block_stop", "index": index
                                                        });
                                                        let sse_data = format!(
                                                            "event: content_block_stop\ndata: {}\n\n",
                                                            serde_json::to_string(&event).unwrap_or_default()
                                                        );
                                                        yield Ok(Bytes::from(sse_data));
                                                    }
                                                    let index = next_content_index;
                                                    next_content_index += 1;
                                                    let event = json!({
                                                        "type": "content_block_start",
                                                        "index": index,
                                                        "content_block": { "type": "text", "text": "" }
                                                    });
                                                    let sse_data = format!(
                                                        "event: content_block_start\ndata: {}\n\n",
                                                        serde_json::to_string(&event).unwrap_or_default()
                                                    );
                                                    yield Ok(Bytes::from(sse_data));
                                                    current_non_tool_block_type = Some("text");
                                                    current_non_tool_block_index = Some(index);
                                                }
                                                if let Some(index) = current_non_tool_block_index {
                                                    let event = json!({
                                                        "type": "content_block_delta",
                                                        "index": index,
                                                        "delta": { "type": "text_delta", "text": content }
                                                    });
                                                    let sse_data = format!(
                                                        "event: content_block_delta\ndata: {}\n\n",
                                                        serde_json::to_string(&event).unwrap_or_default()
                                                    );
                                                    yield Ok(Bytes::from(sse_data));
                                                }
                                            }
                                        }

                                        // 工具调用
                                        if let Some(tool_calls) = &choice.delta.tool_calls {
                                            if !tool_calls.is_empty() {
                                                if let Some(index) = current_non_tool_block_index.take() {
                                                    let event = json!({
                                                        "type": "content_block_stop", "index": index
                                                    });
                                                    let sse_data = format!(
                                                        "event: content_block_stop\ndata: {}\n\n",
                                                        serde_json::to_string(&event).unwrap_or_default()
                                                    );
                                                    yield Ok(Bytes::from(sse_data));
                                                }
                                                current_non_tool_block_type = None;

                                                for tool_call in tool_calls {
                                                    let (anthropic_index, should_start, pending_after_start, immediate_delta) = {
                                                        let state = tool_blocks_by_index
                                                            .entry(tool_call.index)
                                                            .or_insert_with(|| {
                                                                let index = next_content_index;
                                                                next_content_index += 1;
                                                                ToolBlockState {
                                                                    anthropic_index: index,
                                                                    id: String::new(),
                                                                    name: String::new(),
                                                                    started: false,
                                                                    pending_args: String::new(),
                                                                }
                                                            });

                                                        if let Some(id) = &tool_call.id {
                                                            state.id.clone_from(id);
                                                        }
                                                        if let Some(function) = &tool_call.function {
                                                            if let Some(name) = &function.name {
                                                                state.name.clone_from(name);
                                                            }
                                                        }

                                                        let should_start =
                                                            !state.started && !state.id.is_empty() && !state.name.is_empty();
                                                        if should_start {
                                                            state.started = true;
                                                        }
                                                        let pending_after_start = if should_start && !state.pending_args.is_empty() {
                                                            Some(std::mem::take(&mut state.pending_args))
                                                        } else {
                                                            None
                                                        };
                                                        let args_delta = tool_call.function.as_ref().and_then(|f| f.arguments.clone());
                                                        let immediate_delta = args_delta.and_then(|args| {
                                                            if state.started {
                                                                Some(args)
                                                            } else {
                                                                state.pending_args.push_str(&args);
                                                                None
                                                            }
                                                        });
                                                        (state.anthropic_index, should_start, pending_after_start, immediate_delta)
                                                    };

                                                    if should_start {
                                                        let state = &tool_blocks_by_index[&tool_call.index];
                                                        let event = json!({
                                                            "type": "content_block_start",
                                                            "index": anthropic_index,
                                                            "content_block": {
                                                                "type": "tool_use",
                                                                "id": state.id,
                                                                "name": state.name
                                                            }
                                                        });
                                                        let sse_data = format!(
                                                            "event: content_block_start\ndata: {}\n\n",
                                                            serde_json::to_string(&event).unwrap_or_default()
                                                        );
                                                        yield Ok(Bytes::from(sse_data));
                                                        open_tool_block_indices.push(anthropic_index);
                                                    }

                                                    for args in [pending_after_start, immediate_delta].iter().flatten() {
                                                        let event = json!({
                                                            "type": "content_block_delta",
                                                            "index": anthropic_index,
                                                            "delta": { "type": "input_json_delta", "partial_json": args }
                                                        });
                                                        let sse_data = format!(
                                                            "event: content_block_delta\ndata: {}\n\n",
                                                            serde_json::to_string(&event).unwrap_or_default()
                                                        );
                                                        yield Ok(Bytes::from(sse_data));
                                                    }
                                                }
                                            }
                                        }

                                        // finish_reason → 延迟到 [DONE] 统一收尾
                                        if let Some(finish_reason) = &choice.finish_reason {
                                            let stop_reason = map_stop_reason(Some(finish_reason));
                                            let usage_json = chunk_usage_json.clone().or_else(|| latest_usage.clone());

                                            if has_emitted_message_delta {
                                                if let (Some((_, ref mut usage)), Some(uj)) = (&mut pending_message_delta, usage_json) {
                                                    *usage = Some(uj);
                                                }
                                                continue;
                                            }
                                            has_emitted_message_delta = true;

                                            // 关闭当前非 tool 块
                                            if let Some(index) = current_non_tool_block_index.take() {
                                                let event = json!({
                                                    "type": "content_block_stop", "index": index
                                                });
                                                let sse_data = format!(
                                                    "event: content_block_stop\ndata: {}\n\n",
                                                    serde_json::to_string(&event).unwrap_or_default()
                                                );
                                                yield Ok(Bytes::from(sse_data));
                                            }
                                            current_non_tool_block_type = None;

                                            // 关闭所有 tool 块
                                            for &index in &open_tool_block_indices {
                                                let event = json!({
                                                    "type": "content_block_stop", "index": index
                                                });
                                                let sse_data = format!(
                                                    "event: content_block_stop\ndata: {}\n\n",
                                                    serde_json::to_string(&event).unwrap_or_default()
                                                );
                                                yield Ok(Bytes::from(sse_data));
                                            }
                                            open_tool_block_indices.clear();

                                            pending_message_delta = Some((stop_reason, usage_json));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    stream_ended_with_error = true;
                    let error_event = json!({
                        "type": "error",
                        "error": { "type": "stream_error", "message": format!("Stream error: {e}") }
                    });
                    let sse_data = format!(
                        "event: error\ndata: {}\n\n",
                        serde_json::to_string(&error_event).unwrap_or_default()
                    );
                    yield Ok(Bytes::from(sse_data));
                    break;
                }
            }
        }

        // 流结束未收到 [DONE] → 补发 pending 事件
        if !stream_ended_with_error {
            if let Some((stop_reason, usage_json)) = pending_message_delta.take() {
                let event = build_message_delta_event(stop_reason.as_ref(), usage_json);
                let sse_data = format!(
                    "event: message_delta\ndata: {}\n\n",
                    serde_json::to_string(&event).unwrap_or_default()
                );
                yield Ok(Bytes::from(sse_data));

                if !has_sent_message_stop {
                    let event = json!({"type": "message_stop"});
                    let sse_data = format!(
                        "event: message_stop\ndata: {}\n\n",
                        serde_json::to_string(&event).unwrap_or_default()
                    );
                    yield Ok(Bytes::from(sse_data));
                }
            }
        }
    }
}

fn build_anthropic_usage_json(usage: &StreamUsage) -> Value {
    let cached = extract_cache_read_tokens(usage).unwrap_or(0);
    let cache_creation = usage.cache_creation_input_tokens.unwrap_or(0);
    let input_tokens = usage
        .prompt_tokens
        .saturating_sub(cached)
        .saturating_sub(cache_creation);
    let mut usage_json = json!({
        "input_tokens": input_tokens,
        "output_tokens": usage.completion_tokens
    });
    if cached > 0 {
        usage_json["cache_read_input_tokens"] = json!(cached);
    }
    if cache_creation > 0 {
        usage_json["cache_creation_input_tokens"] = json!(cache_creation);
    }
    usage_json
}

fn extract_cache_read_tokens(usage: &StreamUsage) -> Option<u32> {
    if let Some(v) = usage.cache_read_input_tokens {
        return Some(v);
    }
    usage
        .prompt_tokens_details
        .as_ref()
        .map(|d| d.cached_tokens)
        .filter(|&v| v > 0)
}

fn map_stop_reason(finish_reason: Option<&str>) -> Option<String> {
    finish_reason.map(|r| {
        match r {
            "tool_calls" | "function_call" => "tool_use",
            "length" => "max_tokens",
            _ => "end_turn",
        }
        .to_string()
    })
}

fn build_message_delta_event(stop_reason: Option<&String>, usage_json: Option<Value>) -> Value {
    let usage = usage_json.unwrap_or(json!({"input_tokens": 0, "output_tokens": 0}));
    json!({
        "type": "message_delta",
        "delta": { "stop_reason": stop_reason, "stop_sequence": null },
        "usage": usage
    })
}

// ── openai_sse_to_openai_responses（移植 + 分块缓冲加固）───────────────────

#[derive(Default)]
struct ResponsesStreamState {
    initialized: bool,
    tool_output_index: usize,
    emitted_tool_calls: std::collections::HashSet<String>,
}

/// OpenAI Chat SSE 流 → OpenAI Responses SSE 流（缓冲式解析，兼容跨 chunk 分块）
pub fn openai_sse_to_openai_responses<E: std::error::Error + Send + 'static>(
    input_stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    usage: UsageSlot,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();
        let mut state = ResponsesStreamState::default();

        tokio::pin!(input_stream);

        while let Some(result) = input_stream.next().await {
            let bytes = match result {
                Ok(b) => b,
                Err(e) => {
                    yield Err(io::Error::other(e.to_string()));
                    break;
                }
            };
            append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);

            while let Some(block) = take_sse_block(&mut buffer) {
                for line in block.lines() {
                    if line.is_empty() {
                        continue;
                    }
                    let Some(data) = strip_sse_field(line, "data") else { continue };
                    if data.trim() == "[DONE]" {
                        yield Ok(Bytes::from("event: response.completed\ndata: {}\n\n"));
                        continue;
                    }
                    let v: Value = match serde_json::from_str(data) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    // 计量：openai usage 字段 → (prompt, completion)
                    if let Some(u) = v.get("usage") {
                        let input = u.get("prompt_tokens").and_then(|x| x.as_f64()).unwrap_or(0.0);
                        let output = u.get("completion_tokens").and_then(|x| x.as_f64()).unwrap_or(0.0);
                        record_usage(&usage, input, output);
                    }

                    let id = v.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                    let model = v.get("model").and_then(|m| m.as_str()).unwrap_or("").to_string();
                    let response_id = format!("resp_{id}");
                    let Some(choices) = v.get("choices").and_then(|c| c.as_array()) else { continue };
                    let Some(first) = choices.first() else { continue };

                    let delta = first.get("delta");
                    let finish_reason = first.get("finish_reason").and_then(|f| f.as_str());
                    let delta_role = delta.and_then(|d| d.get("role")).and_then(|r| r.as_str());

                    // 首个内容 chunk 发初始事件
                    if !state.initialized && delta_role != Some("assistant") {
                        state.initialized = true;
                        let mut events = String::new();
                        let _ = write!(
                            events,
                            "event: response.created\ndata: {}\n\n",
                            serde_json::to_string(&json!({
                                "type": "response.created",
                                "response": {
                                    "id": response_id,
                                    "object": "response",
                                    "model": model,
                                    "output": [],
                                    "status": "in_progress"
                                }
                            })).unwrap_or_default()
                        );
                        let _ = write!(
                            events,
                            "event: response.output_item.added\ndata: {}\n\n",
                            serde_json::to_string(&json!({
                                "type": "response.output_item.added",
                                "output_index": 0,
                                "item": {
                                    "id": format!("msg_{id}"),
                                    "type": "message",
                                    "role": "assistant",
                                    "content": []
                                }
                            })).unwrap_or_default()
                        );
                        let _ = write!(
                            events,
                            "event: response.content_part.added\ndata: {}\n\n",
                            serde_json::to_string(&json!({
                                "type": "response.content_part.added",
                                "item_id": format!("msg_{id}"),
                                "output_index": 0,
                                "content_index": 0,
                                "part": {"type": "output_text", "text": "", "annotations": []}
                            })).unwrap_or_default()
                        );
                        yield Ok(Bytes::from(events));
                    }

                    // 内容增量
                    if state.initialized {
                        if let Some(text) = delta.and_then(|d| d.get("content")).and_then(|c| c.as_str()) {
                            if !text.is_empty() {
                                let data = serde_json::to_string(&json!({
                                    "type": "response.output_text.delta",
                                    "item_id": format!("msg_{id}"),
                                    "output_index": 0,
                                    "content_index": 0,
                                    "delta": text
                                })).unwrap_or_default();
                                yield Ok(Bytes::from(format!("event: response.output_text.delta\ndata: {data}\n\n")));
                            }
                        }
                    }

                    // 工具调用
                    if let Some(tool_calls) = delta.and_then(|d| d.get("tool_calls")).and_then(|t| t.as_array()) {
                        if !state.initialized {
                            state.initialized = true;
                            let data = serde_json::to_string(&json!({
                                "type": "response.created",
                                "response": {
                                    "id": response_id,
                                    "object": "response",
                                    "model": model,
                                    "output": [],
                                    "status": "in_progress"
                                }
                            })).unwrap_or_default();
                            yield Ok(Bytes::from(format!("event: response.created\ndata: {data}\n\n")));
                        }
                        for tc in tool_calls {
                            let tc_id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("");
                            let tc_name = tc.get("function").and_then(|f| f.get("name")).and_then(|n| n.as_str()).unwrap_or("");
                            let tc_args = tc.get("function").and_then(|f| f.get("arguments")).and_then(|a| a.as_str()).unwrap_or("");
                            let key = tc_id.to_string();
                            if !state.emitted_tool_calls.contains(&key) {
                                state.emitted_tool_calls.insert(key);
                                let output_index = state.tool_output_index;
                                state.tool_output_index += 1;
                                let item_id = format!("fc_{tc_id}");
                                let data = serde_json::to_string(&json!({
                                    "type": "response.output_item.added",
                                    "output_index": output_index,
                                    "item": {
                                        "id": item_id,
                                        "type": "function_call",
                                        "call_id": tc_id,
                                        "name": tc_name,
                                        "arguments": tc_args
                                    }
                                })).unwrap_or_default();
                                yield Ok(Bytes::from(format!("event: response.output_item.added\ndata: {data}\n\n")));
                            }
                            let item_id = format!("fc_{tc_id}");
                            let data = serde_json::to_string(&json!({
                                "type": "response.function_call_arguments.delta",
                                "item_id": item_id,
                                "output_index": state.tool_output_index.saturating_sub(1),
                                "delta": tc_args
                            })).unwrap_or_default();
                            yield Ok(Bytes::from(format!("event: response.function_call_arguments.delta\ndata: {data}\n\n")));
                        }
                    }

                    // finish_reason → response.completed
                    if let Some(fr) = finish_reason {
                        if !fr.is_empty() {
                            let data = serde_json::to_string(&json!({
                                "type": "response.completed",
                                "response": {
                                    "id": response_id,
                                    "object": "response",
                                    "model": model,
                                    "output": [],
                                    "status": "completed"
                                }
                            })).unwrap_or_default();
                            yield Ok(Bytes::from(format!("event: response.completed\ndata: {data}\n\n")));
                        }
                    }
                }
            }
        }
    }
}

// ── anthropic_sse_to_openai（新增）────────────────────────────────────────

#[derive(Debug, Clone)]
struct OpenaiToolState {
    openai_index: usize,
    id: String,
    name: String,
    role_emitted: bool,
}

/// Anthropic SSE 流 → OpenAI Chat SSE 流
/// content_block_delta 文本 → choices[0].delta.content；message_start → role；
/// message_delta usage + stop → finish_reason + [DONE]；tool_use 增量 → tool_calls delta。
pub fn anthropic_sse_to_openai<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    usage: UsageSlot,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();
        let mut msg_id = String::new();
        let mut model = String::new();
        let mut input_tokens: f64 = 0.0;
        let mut output_tokens: f64 = 0.0;
        let mut role_emitted = false;
        let mut tools: HashMap<u32, OpenaiToolState> = HashMap::new();
        let mut next_tool_index: usize = 0;
        let mut finished = false;

        let emit_role = |role_emitted: &mut bool, msg_id: &str, model: &str| {
            if *role_emitted {
                return Bytes::new();
            }
            *role_emitted = true;
            let data = json!({
                "id": format!("chatcmpl-{msg_id}"),
                "object": "chat.completion.chunk",
                "created": 0,
                "model": model,
                "choices": [{
                    "index": 0,
                    "delta": { "role": "assistant", "content": "" },
                    "finish_reason": null
                }]
            });
            let sse = format!("data: {}\n\n", serde_json::to_string(&data).unwrap_or_default());
            Bytes::from(sse)
        };

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);
                    while let Some(block) = take_sse_block(&mut buffer) {
                        for line in block.lines() {
                            let Some(data) = strip_sse_field(line, "data") else { continue };
                            let Ok(v) = serde_json::from_str::<Value>(data) else { continue };
                            match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                                "message_start" => {
                                    if let Some(msg) = v.get("message") {
                                        msg_id = msg.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                                        model = msg.get("model").and_then(|x| x.as_str()).unwrap_or("").to_string();
                                        if let Some(u) = msg.get("usage") {
                                            input_tokens = u.get("input_tokens").and_then(|x| x.as_f64()).unwrap_or(0.0);
                                        }
                                    }
                                }
                                "content_block_start" => {
                                    if let Some(cb) = v.get("content_block") {
                                        if cb.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                                            let index = v.get("index").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                                            let oi = next_tool_index;
                                            next_tool_index += 1;
                                            let state = OpenaiToolState {
                                                openai_index: oi,
                                                id: cb.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                                                name: cb.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                                                role_emitted: false,
                                            };
                                            tools.insert(index, state);
                                        }
                                    }
                                }
                                "content_block_delta" => {
                                    let Some(delta) = v.get("delta") else { continue };
                                    let dtype = delta.get("type").and_then(|t| t.as_str()).unwrap_or("");
                                    let index = v.get("index").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                                    match dtype {
                                        "text_delta" => {
                                            let text = delta.get("text").and_then(|t| t.as_str()).unwrap_or("");
                                            if text.is_empty() { continue; }
                                            let mut events = String::new();
                                            if !role_emitted {
                                                let b = emit_role(&mut role_emitted, &msg_id, &model);
                                                events.push_str(&String::from_utf8_lossy(&b));
                                            }
                                            let data = json!({
                                                "id": format!("chatcmpl-{msg_id}"),
                                                "object": "chat.completion.chunk",
                                                "created": 0,
                                                "model": model,
                                                "choices": [{
                                                    "index": 0,
                                                    "delta": { "content": text },
                                                    "finish_reason": null
                                                }]
                                            });
                                            events.push_str(&format!("data: {}\n\n", serde_json::to_string(&data).unwrap_or_default()));
                                            yield Ok(Bytes::from(events));
                                        }
                                        "input_json_delta" => {
                                            let partial = delta.get("partial_json").and_then(|t| t.as_str()).unwrap_or("");
                                            let Some(state) = tools.get_mut(&index) else { continue };
                                            if !state.role_emitted {
                                                let b = emit_role(&mut state.role_emitted, &msg_id, &model);
                                                if !b.is_empty() {
                                                    yield Ok(b);
                                                }
                                            }
                                            let data = json!({
                                                "id": format!("chatcmpl-{msg_id}"),
                                                "object": "chat.completion.chunk",
                                                "created": 0,
                                                "model": model,
                                                "choices": [{
                                                    "index": 0,
                                                    "delta": {
                                                        "tool_calls": [{
                                                            "index": state.openai_index,
                                                            "id": state.id,
                                                            "function": { "name": state.name, "arguments": partial }
                                                        }]
                                                    },
                                                    "finish_reason": null
                                                }]
                                            });
                                            let sse = format!("data: {}\n\n", serde_json::to_string(&data).unwrap_or_default());
                                            yield Ok(Bytes::from(sse));
                                        }
                                        _ => {}
                                    }
                                }
                                "message_delta" => {
                                    if let Some(u) = v.get("usage") {
                                        output_tokens = u.get("output_tokens").and_then(|x| x.as_f64()).unwrap_or(0.0);
                                    }
                                    record_usage(&usage, input_tokens, output_tokens);
                                    let stop_reason = v.get("delta").and_then(|d| d.get("stop_reason")).and_then(|s| s.as_str());
                                    let finish = match stop_reason {
                                        Some("max_tokens") => "length",
                                        Some("tool_use") => "tool_calls",
                                        _ => "stop",
                                    };
                                    let data = json!({
                                        "id": format!("chatcmpl-{msg_id}"),
                                        "object": "chat.completion.chunk",
                                        "created": 0,
                                        "model": model,
                                        "choices": [{
                                            "index": 0,
                                            "delta": {},
                                            "finish_reason": finish
                                        }]
                                    });
                                    let sse = format!("data: {}\n\n", serde_json::to_string(&data).unwrap_or_default());
                                    yield Ok(Bytes::from(sse));
                                    finished = true;
                                }
                                "message_stop" => {
                                    if !finished {
                                        record_usage(&usage, input_tokens, output_tokens);
                                        let data = json!({
                                            "id": format!("chatcmpl-{msg_id}"),
                                            "object": "chat.completion.chunk",
                                            "created": 0,
                                            "model": model,
                                            "choices": [{
                                                "index": 0,
                                                "delta": {},
                                                "finish_reason": "stop"
                                            }]
                                        });
                                        let sse = format!("data: {}\n\n", serde_json::to_string(&data).unwrap_or_default());
                                        yield Ok(Bytes::from(sse));
                                        finished = true;
                                    }
                                    yield Ok(Bytes::from("data: [DONE]\n\n"));
                                }
                                "error" => {
                                    // 尽力关闭流
                                    yield Ok(Bytes::from("data: [DONE]\n\n"));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Err(e) => {
                    yield Ok(Bytes::from("data: [DONE]\n\n"));
                    let _ = e;
                    break;
                }
            }
        }
        // 上游无 message_stop 直接断流 → 补 finish + [DONE]
        if !finished {
            record_usage(&usage, input_tokens, output_tokens);
            let data = json!({
                "id": format!("chatcmpl-{msg_id}"),
                "object": "chat.completion.chunk",
                "created": 0,
                "model": model,
                "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }]
            });
            let sse = format!("data: {}\n\n", serde_json::to_string(&data).unwrap_or_default());
            yield Ok(Bytes::from(sse));
            yield Ok(Bytes::from("data: [DONE]\n\n"));
        }
    }
}

/// Anthropic SSE 流 → OpenAI Responses SSE 流（经 openai chat 中间态）
pub fn anthropic_sse_to_responses<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    usage: UsageSlot,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send {
    let inner = anthropic_sse_to_openai(stream, usage.clone());
    openai_sse_to_openai_responses(Box::pin(inner), usage)
}

// ── responses_sse_to_openai_chat（新增）───────────────────────────────────

/// OpenAI Responses SSE 流 → OpenAI Chat SSE 流
/// response.output_text.delta → choices delta；function_call 增量 → tool_calls delta；
/// response.completed → finish_reason + [DONE]。
pub fn responses_sse_to_openai_chat<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    usage: UsageSlot,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();
        let mut msg_id = String::new();
        let mut model = String::new();
        let mut role_emitted = false;
        let mut tool_states: HashMap<String, usize> = HashMap::new(); // call_id -> openai index
        let mut next_tool_index: usize = 0;
        let mut finished = false;

        let emit_role = |role_emitted: &mut bool, msg_id: &str, model: &str| {
            if *role_emitted {
                return String::new();
            }
            *role_emitted = true;
            let data = json!({
                "id": format!("chatcmpl-{msg_id}"),
                "object": "chat.completion.chunk",
                "created": 0,
                "model": model,
                "choices": [{
                    "index": 0,
                    "delta": { "role": "assistant", "content": "" },
                    "finish_reason": null
                }]
            });
            format!("data: {}\n\n", serde_json::to_string(&data).unwrap_or_default())
        };

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);
                    while let Some(block) = take_sse_block(&mut buffer) {
                        for line in block.lines() {
                            let Some(data) = strip_sse_field(line, "data") else { continue };
                            let Ok(v) = serde_json::from_str::<Value>(data) else { continue };
                            match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                                "response.created" => {
                                    if let Some(r) = v.get("response") {
                                        msg_id = r.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                                        model = r.get("model").and_then(|x| x.as_str()).unwrap_or("").to_string();
                                    }
                                }
                                "response.output_text.delta" => {
                                    let text = v.get("delta").and_then(|t| t.as_str()).unwrap_or("");
                                    if text.is_empty() { continue; }
                                    let mut events = String::new();
                                    if !role_emitted {
                                        events.push_str(&emit_role(&mut role_emitted, &msg_id, &model));
                                    }
                                    let data = json!({
                                        "id": format!("chatcmpl-{msg_id}"),
                                        "object": "chat.completion.chunk",
                                        "created": 0,
                                        "model": model,
                                        "choices": [{
                                            "index": 0,
                                            "delta": { "content": text },
                                            "finish_reason": null
                                        }]
                                    });
                                    events.push_str(&format!("data: {}\n\n", serde_json::to_string(&data).unwrap_or_default()));
                                    yield Ok(Bytes::from(events));
                                }
                                "response.output_item.added" => {
                                    let Some(item) = v.get("item") else { continue };
                                    if item.get("type").and_then(|t| t.as_str()) != Some("function_call") { continue; }
                                    let call_id = item.get("call_id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                                    let name = item.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
                                    let args = item.get("arguments").and_then(|x| x.as_str()).unwrap_or("").to_string();
                                    let oi = next_tool_index;
                                    next_tool_index += 1;
                                    tool_states.insert(call_id.clone(), oi);
                                    let mut events = String::new();
                                    if !role_emitted {
                                        events.push_str(&emit_role(&mut role_emitted, &msg_id, &model));
                                    }
                                    let data = json!({
                                        "id": format!("chatcmpl-{msg_id}"),
                                        "object": "chat.completion.chunk",
                                        "created": 0,
                                        "model": model,
                                        "choices": [{
                                            "index": 0,
                                            "delta": {
                                                "tool_calls": [{
                                                    "index": oi,
                                                    "id": call_id,
                                                    "function": { "name": name, "arguments": args }
                                                }]
                                            },
                                            "finish_reason": null
                                        }]
                                    });
                                    events.push_str(&format!("data: {}\n\n", serde_json::to_string(&data).unwrap_or_default()));
                                    yield Ok(Bytes::from(events));
                                }
                                "response.function_call_arguments.delta" => {
                                    let call_id = v.get("item_id").and_then(|x| x.as_str()).unwrap_or("").trim_start_matches("fc_").to_string();
                                    let args = v.get("delta").and_then(|x| x.as_str()).unwrap_or("");
                                    let Some(&oi) = tool_states.get(&call_id) else { continue };
                                    let mut events = String::new();
                                    if !role_emitted {
                                        events.push_str(&emit_role(&mut role_emitted, &msg_id, &model));
                                    }
                                    let data = json!({
                                        "id": format!("chatcmpl-{msg_id}"),
                                        "object": "chat.completion.chunk",
                                        "created": 0,
                                        "model": model,
                                        "choices": [{
                                            "index": 0,
                                            "delta": {
                                                "tool_calls": [{
                                                    "index": oi,
                                                    "function": { "arguments": args }
                                                }]
                                            },
                                            "finish_reason": null
                                        }]
                                    });
                                    events.push_str(&format!("data: {}\n\n", serde_json::to_string(&data).unwrap_or_default()));
                                    yield Ok(Bytes::from(events));
                                }
                                "response.completed" => {
                                    if let Some(u) = v.get("response").and_then(|r| r.get("usage")) {
                                        let input = u.get("input_tokens").and_then(|x| x.as_f64()).unwrap_or(0.0);
                                        let output = u.get("output_tokens").and_then(|x| x.as_f64()).unwrap_or(0.0);
                                        record_usage(&usage, input, output);
                                    }
                                    if !finished {
                                        let data = json!({
                                            "id": format!("chatcmpl-{msg_id}"),
                                            "object": "chat.completion.chunk",
                                            "created": 0,
                                            "model": model,
                                            "choices": [{
                                                "index": 0,
                                                "delta": {},
                                                "finish_reason": "stop"
                                            }]
                                        });
                                        let sse = format!("data: {}\n\n", serde_json::to_string(&data).unwrap_or_default());
                                        yield Ok(Bytes::from(sse));
                                        finished = true;
                                    }
                                    yield Ok(Bytes::from("data: [DONE]\n\n"));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Err(e) => {
                    yield Ok(Bytes::from("data: [DONE]\n\n"));
                    let _ = e;
                    break;
                }
            }
        }
        if !finished {
            let data = json!({
                "id": format!("chatcmpl-{msg_id}"),
                "object": "chat.completion.chunk",
                "created": 0,
                "model": model,
                "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }]
            });
            let sse = format!("data: {}\n\n", serde_json::to_string(&data).unwrap_or_default());
            yield Ok(Bytes::from(sse));
            yield Ok(Bytes::from("data: [DONE]\n\n"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;

    fn sse_chunks(
        items: Vec<String>,
    ) -> impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
        stream::iter(items.into_iter().map(|s| Ok(Bytes::from(s.into_bytes()))))
    }

    fn collect(stream: impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static) -> String {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let mut out = String::new();
                tokio::pin!(stream);
                while let Some(item) = stream.next().await {
                    if let Ok(b) = item {
                        out.push_str(&String::from_utf8_lossy(&b));
                    }
                }
                out
            })
    }

    #[test]
    fn openai_to_anthropic_text_and_usage() {
        let slot = usage_slot();
        let chunks = sse_chunks(vec![
            "data: {\"id\":\"c1\",\"model\":\"m1\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hel\"},\"finish_reason\":null}]}\n\n".to_string(),
            "data: {\"id\":\"c1\",\"model\":\"m1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"lo\"},\"finish_reason\":null}]}\n\n".to_string(),
            "data: {\"id\":\"c1\",\"model\":\"m1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":50}}\n\n".to_string(),
            "data: [DONE]\n\n".to_string(),
        ]);
        let out = collect(openai_sse_to_anthropic(chunks, slot.clone()));
        assert!(out.contains("event: message_start"), "message_start: {out}");
        assert!(out.contains("\"text\":\"Hel\""), "text delta: {out}");
        assert!(out.contains("\"text\":\"lo\""), "text delta2: {out}");
        assert!(out.contains("event: message_delta"), "message_delta: {out}");
        assert!(
            out.contains("\"stop_reason\":\"end_turn\""),
            "stop_reason: {out}"
        );
        assert!(out.contains("event: message_stop"), "message_stop: {out}");
        let Some((i, o)) = *slot.lock().unwrap() else {
            panic!("usage missing");
        };
        assert_eq!((i, o), (100.0, 50.0), "usage recorded");
    }

    #[test]
    fn openai_to_responses_text() {
        let slot = usage_slot();
        let chunks = sse_chunks(vec![
            "data: {\"id\":\"c1\",\"model\":\"m1\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"\"},\"finish_reason\":null}]}\n\n".to_string(),
            "data: {\"id\":\"c1\",\"model\":\"m1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hi\"},\"finish_reason\":null}]}\n\n".to_string(),
            "data: {\"id\":\"c1\",\"model\":\"m1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":3}}\n\n".to_string(),
            "data: [DONE]\n\n".to_string(),
        ]);
        let out = collect(openai_sse_to_openai_responses(chunks, slot.clone()));
        assert!(out.contains("event: response.created"), "created: {out}");
        assert!(
            out.contains("event: response.output_text.delta"),
            "delta: {out}"
        );
        assert!(out.contains("\"delta\":\"Hi\""), "text: {out}");
        assert!(
            out.contains("event: response.completed"),
            "completed: {out}"
        );
        let Some((i, o)) = *slot.lock().unwrap() else {
            panic!("usage missing");
        };
        assert_eq!((i, o), (7.0, 3.0), "usage recorded");
    }

    #[test]
    fn anthropic_to_openai_text_and_done() {
        let slot = usage_slot();
        let chunks = sse_chunks(vec![
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"model\":\"claude-x\",\"role\":\"assistant\",\"usage\":{\"input_tokens\":9,\"output_tokens\":0}}}\n\n".to_string(),
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n".to_string(),
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"He\"}}\n\n".to_string(),
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"llo\"}}\n\n".to_string(),
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n".to_string(),
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":4}}\n\n".to_string(),
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string(),
        ]);
        let out = collect(anthropic_sse_to_openai(chunks, slot.clone()));
        assert!(out.contains("\"role\":\"assistant\""), "role: {out}");
        assert!(out.contains("\"content\":\"He\""), "text: {out}");
        assert!(out.contains("\"content\":\"llo\""), "text2: {out}");
        assert!(out.contains("\"finish_reason\":\"stop\""), "finish: {out}");
        assert!(out.contains("data: [DONE]"), "done: {out}");
        let Some((i, o)) = *slot.lock().unwrap() else {
            panic!("usage missing");
        };
        assert_eq!((i, o), (9.0, 4.0), "usage recorded");
    }

    #[test]
    fn anthropic_to_openai_tool_use() {
        let slot = usage_slot();
        let chunks = sse_chunks(vec![
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"model\":\"claude-x\",\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n".to_string(),
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"get_weather\"}}\n\n".to_string(),
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"city\\\":\\\"SZ\\\"}\"}}\n\n".to_string(),
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n".to_string(),
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":2}}\n\n".to_string(),
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string(),
        ]);
        let out = collect(anthropic_sse_to_openai(chunks, slot.clone()));
        assert!(out.contains("\"tool_calls\""), "tool_calls: {out}");
        assert!(out.contains("\"name\":\"get_weather\""), "name: {out}");
        assert!(
            out.contains("\"arguments\":\"{\\\"city\\\":\\\"SZ\\\"}\""),
            "args: {out}"
        );
        assert!(
            out.contains("\"finish_reason\":\"tool_calls\""),
            "finish tool: {out}"
        );
    }

    #[test]
    fn responses_to_openai_text() {
        let slot = usage_slot();
        let chunks = sse_chunks(vec![
            "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"r1\",\"model\":\"gpt-x\",\"output\":[],\"status\":\"in_progress\"}}\n\n".to_string(),
            "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[]}}\n\n".to_string(),
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"OK\"}\n\n".to_string(),
            "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"r1\",\"object\":\"response\",\"model\":\"gpt-x\",\"output\":[],\"status\":\"completed\",\"usage\":{\"input_tokens\":12,\"output_tokens\":6}}}\n\n".to_string(),
        ]);
        let out = collect(responses_sse_to_openai_chat(chunks, slot.clone()));
        assert!(out.contains("\"role\":\"assistant\""), "role: {out}");
        assert!(out.contains("\"content\":\"OK\""), "text: {out}");
        assert!(out.contains("\"finish_reason\":\"stop\""), "finish: {out}");
        assert!(out.contains("data: [DONE]"), "done: {out}");
        let Some((i, o)) = *slot.lock().unwrap() else {
            panic!("usage missing");
        };
        assert_eq!((i, o), (12.0, 6.0), "usage recorded");
    }

    #[test]
    fn anthropic_to_responses_chain() {
        let slot = usage_slot();
        let chunks = sse_chunks(vec![
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"model\":\"claude-x\",\"usage\":{\"input_tokens\":5,\"output_tokens\":0}}}\n\n".to_string(),
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n".to_string(),
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n".to_string(),
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string(),
        ]);
        let out = collect(anthropic_sse_to_responses(chunks, slot.clone()));
        assert!(out.contains("event: response.created"), "created: {out}");
        assert!(
            out.contains("event: response.output_text.delta"),
            "delta: {out}"
        );
        assert!(
            out.contains("event: response.completed"),
            "completed: {out}"
        );
        let Some((i, o)) = *slot.lock().unwrap() else {
            panic!("usage missing");
        };
        assert_eq!((i, o), (5.0, 2.0), "usage recorded");
    }

    #[test]
    fn sse_helpers_split_across_chunks() {
        // 事件被拆到两个 chunk，且 UTF-8 字符跨边界
        let mut buffer = String::new();
        let mut rem = Vec::new();
        append_utf8_safe(
            &mut buffer,
            &mut rem,
            "event: x\ndata: {\"a\":\"".as_bytes(),
        );
        append_utf8_safe(&mut buffer, &mut rem, "你\"}\n\n".as_bytes());
        let block = take_sse_block(&mut buffer).expect("block");
        assert!(block.contains("你"), "utf8 across chunks: {block}");
        assert!(buffer.is_empty());
    }

    #[test]
    fn anthropic_to_openai_missing_message_delta() {
        // 只有 message_stop 没有 message_delta → 补 finish + [DONE]
        let slot = usage_slot();
        let chunks = sse_chunks(vec![
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"model\":\"claude-x\",\"usage\":{\"input_tokens\":3,\"output_tokens\":0}}}\n\n".to_string(),
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"x\"}}\n\n".to_string(),
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string(),
        ]);
        let out = collect(anthropic_sse_to_openai(chunks, slot.clone()));
        assert!(out.contains("data: [DONE]"), "done: {out}");
        let Some((i, o)) = *slot.lock().unwrap() else {
            panic!("usage missing");
        };
        assert_eq!((i, o), (3.0, 0.0), "usage recorded");
    }
}
