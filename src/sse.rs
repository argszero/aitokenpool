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

/// 流内 usage 提取槽：(input_tokens, cached_tokens, output_tokens)
pub type UsageSlot = std::sync::Arc<std::sync::Mutex<Option<(f64, f64, f64)>>>;

/// 新建空 usage 槽
pub fn usage_slot() -> UsageSlot {
    std::sync::Arc::new(std::sync::Mutex::new(None))
}

fn record_usage(slot: &UsageSlot, input: f64, cached: f64, output: f64) {
    if let Ok(mut s) = slot.lock() {
        *s = Some((input, cached, output));
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
    // DeepSeek 原生顶层拼写（rant 2026-08-23T08:20:38 P0 缓存计费）：
    // prompt_tokens = prompt_cache_hit_tokens + prompt_cache_miss_tokens
    #[serde(default)]
    prompt_cache_hit_tokens: u32,
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
                                    // 上游以 [DONE] 明确收尾 ⇒ 终局 stop_reason 必须告知客户端，且与
                                    // 整包翻译器同源。上游一路没有给过 finish_reason（末尾 chunk 仍为
                                    // null）时，按「无 finish_reason + 是否带过 tool call」推导，口径与
                                    // openai_chat_to_anthropic_resp 完全一致（否则客户端拿不到 tool_use）。
                                    let should_emit_terminal =
                                        pending_message_delta.is_some() || has_sent_message_start;
                                    if should_emit_terminal {
                                        let (stop_reason, usage_json) =
                                            pending_message_delta.take().unwrap_or_else(|| {
                                                (
                                                    crate::protocol::openai_chat_to_anthropic_stop_reason(
                                                        None,
                                                        !tool_blocks_by_index.is_empty(),
                                                    ),
                                                    latest_usage.clone(),
                                                )
                                            });
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
                                        // 计量：openai 原样 usage → (prompt, cached, completion)
                                        // input 做 disjoint：prompt_tokens 含缓存命中部分，扣除后避免重复计费（rant 2026-08-23T08:20:38）
                                        if let Some(u) = &chunk.usage {
                                            let cached = extract_cache_read_tokens(u).unwrap_or(0);
                                            record_usage(
                                                &usage,
                                                u.prompt_tokens.saturating_sub(cached) as f64,
                                                cached as f64,
                                                u.completion_tokens as f64,
                                            );
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
                                            // 终局 stop_reason 与整包翻译器同源（protocol 侧唯一真源）
                                            let stop_reason = crate::protocol::openai_chat_to_anthropic_stop_reason(
                                                Some(finish_reason),
                                                !tool_blocks_by_index.is_empty(),
                                            );
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
    // 优先级（rant 2026-08-23T08:20:38）：DeepSeek 原生顶层 → OpenAI → Anthropic
    if usage.prompt_cache_hit_tokens > 0 {
        return Some(usage.prompt_cache_hit_tokens);
    }
    if let Some(v) = usage
        .prompt_tokens_details
        .as_ref()
        .map(|d| d.cached_tokens)
        .filter(|&v| v > 0)
    {
        return Some(v);
    }
    usage.cache_read_input_tokens
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

/// responses 流式转换的累积状态。
///
/// 除了「已发出哪些增量事件」之外，还累积**终局 `response.completed` 需要的内容**：
/// `[DONE]` 分支读不到任何 chunk，唯一能给出完整对象的来源就是这里累积的状态。
#[derive(Default)]
struct ResponsesStreamState {
    initialized: bool,
    /// 下一个可用的 `output_index`。**唯一且按宣布顺序单调递增**。
    ///
    /// 修复前这里是两个各自从 0 起算的计数器：message 条目的索引写死 0（初始化块），
    /// `tool_output_index` 也从 0 起 ⇒ 一次响应同时含文本与工具调用时，两个不同条目都被
    /// 宣布在 `output_index: 0`，客户端按 `output_index` 跟踪条目（协议给它就是这个用途）
    /// 时后一个覆盖前一个 —— 通常丢掉的是 agent 客户端正在等的 `function_call`。
    next_output_index: usize,
    /// message 条目被宣布时拿到的索引（从未宣布过 = `None`）。
    message_output_index: Option<usize>,
    /// `call_id` → 该 `function_call` 条目被宣布时拿到的索引。
    tool_call_indices: HashMap<String, usize>,
    upstream_id: String,
    model: String,
    text: String,
    tool_calls: Vec<Value>,
    usage: Option<Value>,
    completed: bool,
}

impl ResponsesStreamState {
    /// 给一个**即将宣布**的条目分配 `output_index`（唯一、单调递增）。
    fn next_index(&mut self) -> usize {
        let index = self.next_output_index;
        self.next_output_index += 1;
        index
    }

    /// 宣布 message 条目：返回 (它的 `output_index`, 事件文本)。
    ///
    /// 事件文本含 `response.output_item.added` + `response.content_part.added` 两条。
    /// 同一个条目只拿到一个索引（第二次调用沿用第一次的索引、不再递增计数器）；调用方只在
    /// `message_output_index` 还是 `None` 时调用它（即第一次拿到文本时），因此事件只发一次。
    fn announce_message_item(&mut self, item_id: &str) -> (usize, String) {
        let index = match self.message_output_index {
            Some(index) => index,
            None => {
                let index = self.next_index();
                self.message_output_index = Some(index);
                index
            }
        };
        let mut events = String::new();
        let _ = write!(
            events,
            "event: response.output_item.added\ndata: {}\n\n",
            serde_json::to_string(&json!({
                "type": "response.output_item.added",
                "output_index": index,
                "item": {
                    "id": item_id,
                    "type": "message",
                    "role": "assistant",
                    "content": []
                }
            }))
            .unwrap_or_default()
        );
        let _ = write!(
            events,
            "event: response.content_part.added\ndata: {}\n\n",
            serde_json::to_string(&json!({
                "type": "response.content_part.added",
                "item_id": item_id,
                "output_index": index,
                "content_index": 0,
                "part": {"type": "output_text", "text": "", "annotations": []}
            }))
            .unwrap_or_default()
        );
        (index, events)
    }

    /// 终局 `response.completed` 的 `response` 对象。
    ///
    /// 条目**形状**与整包路径**同源**（`protocol::openai_chat_message_item` /
    /// `protocol::openai_chat_tool_call_item` / `protocol::openai_usage_to_responses_usage`），
    /// 因此两条路径给出同一组字段：`id` / `object` / `model` / `output` / `usage`。
    ///
    /// 条目**顺序**则按本路径自己的权威：**宣布顺序**（各条目在流里被
    /// `response.output_item.added` 宣布时拿到的 `output_index` 升序）。整包路径的顺序是
    /// 它的既有约定（`function_call` 在前、`message` 在后）—— 两条路径的顺序本就不同，
    /// 但各自与自己的权威一致；流式若照抄整包的顺序，终局就会与自己已发出的增量事件矛盾。
    ///
    /// 三处**已记录**的分叉：① `id` 带 `resp_` 前缀（增量事件用的是同一个 id，客户端只可能
    /// 看到一条路径）；② message item 的 id 用 `msg_…` —— 必须与本路径已发出的
    /// `response.output_item.added` / 增量事件的 `item_id` 一致；③ **不变式**：终局列出的条目
    /// == 本流宣布过的条目（message 条目在第一次拿到文本时才宣布 ⇒ 没有文本就没有它，
    /// 与整包路径一致）。
    /// `status` 是流式独有的字段（整包路径没有；截断语义属宿主裁定族，此处沿用
    /// `completed`，不引入 `incomplete`）。
    fn terminal_response(&self) -> Value {
        let mut message = json!({"role": "assistant", "content": self.text});
        if !self.tool_calls.is_empty() {
            message["tool_calls"] = Value::Array(self.tool_calls.clone());
        }
        // 终局 output = 本流**宣布过的**条目，按各自的 output_index 升序排列
        let mut announced: Vec<(usize, Value)> = Vec::new();
        if let Some(index) = self.message_output_index {
            if let Some(item) = crate::protocol::openai_chat_message_item(
                &format!("msg_{}", self.upstream_id),
                message.get("content"),
            ) {
                announced.push((index, item));
            }
        }
        for tc in &self.tool_calls {
            let call_id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("");
            if let Some(index) = self.tool_call_indices.get(call_id) {
                announced.push((*index, crate::protocol::openai_chat_tool_call_item(tc)));
            }
        }
        announced.sort_by_key(|(index, _)| *index);
        let output: Vec<Value> = announced.into_iter().map(|(_, item)| item).collect();

        json!({
            "id": format!("resp_{}", self.upstream_id),
            "object": "response",
            "model": self.model,
            "output": output,
            "usage": crate::protocol::openai_usage_to_responses_usage(self.usage.as_ref()),
            "status": "completed"
        })
    }

    /// 终局事件，**每个流至多一次**。
    ///
    /// `finish_reason` 分支与 `[DONE]` 分支共用本函数：正常的上游先发 `finish_reason`
    /// chunk 再发 `[DONE]`，谁先到谁发；上游只发 `[DONE]`（从未给 `finish_reason`）时由
    /// `[DONE]` 分支补发**完整**对象 —— 修复前该分支发的是 `data: {}`（无 `type`、
    /// 无 `response`），客户端按 `event.response` 读会拿到空，且正常流里终局事件出现两次。
    fn completed_event(&mut self) -> Option<Bytes> {
        if self.completed {
            return None;
        }
        self.completed = true;
        let data = serde_json::to_string(&json!({
            "type": "response.completed",
            "response": self.terminal_response()
        }))
        .unwrap_or_default();
        Some(Bytes::from(format!(
            "event: response.completed\ndata: {data}\n\n"
        )))
    }
}

/// OpenAI Chat SSE 流 → OpenAI Responses SSE 流（缓冲式解析，兼容跨 chunk 分块）
///
/// 终局事件 `response.completed` **恰好一次**，且负载是完整的 `response` 对象（内容由流内
/// 累积状态构造、形状与整包路径同源，见 `ResponsesStreamState::terminal_response`）。
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
                        // 上游正常收尾：终局事件在此补发（若 finish_reason chunk 已发过则不再重复），
                        // 且必须是完整对象 —— 此前这里发 `data: {}`，客户端读到的终局没有
                        // `type` 也没有 `response`，与整包路径的形状完全不同。
                        if let Some(event) = state.completed_event() {
                            yield Ok(event);
                        }
                        continue;
                    }
                    let v: Value = match serde_json::from_str(data) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    // 计量：openai usage 字段 → (prompt, cached, completion)
                    // 统一走 StreamUsage 提取（三拼写兼容）+ input disjoint（rant 2026-08-23T08:20:38）
                    if let Some(u) = v.get("usage") {
                        if !u.is_null() {
                            // 终局对象要按整包路径的规则映射这份 usage（缺失则给 0）
                            state.usage = Some(u.clone());
                        }
                        if let Ok(su) = serde_json::from_value::<StreamUsage>(u.clone()) {
                            let cached = extract_cache_read_tokens(&su).unwrap_or(0);
                            record_usage(
                                &usage,
                                su.prompt_tokens.saturating_sub(cached) as f64,
                                cached as f64,
                                su.completion_tokens as f64,
                            );
                        }
                    }

                    let id = v.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                    let model = v.get("model").and_then(|m| m.as_str()).unwrap_or("").to_string();
                    if !id.is_empty() {
                        state.upstream_id = id.clone();
                    }
                    if !model.is_empty() {
                        state.model = model.clone();
                    }
                    let response_id = format!("resp_{id}");
                    let Some(choices) = v.get("choices").and_then(|c| c.as_array()) else { continue };
                    let Some(first) = choices.first() else { continue };

                    let delta = first.get("delta");
                    let finish_reason = first.get("finish_reason").and_then(|f| f.as_str());
                    let delta_role = delta.and_then(|d| d.get("role")).and_then(|r| r.as_str());

                    // 首个内容 chunk 发初始事件
                    //
                    // message 条目**不在这里**宣布：声明「流宣布过的条目」与终局 `output`
                    // 的内容必须一致，而终局里的 message 条目由文本决定（没有文本 ⇒ 整包路径
                    // 也没有这个条目）。宣布改在真正拿到文本的地方（内容增量块），于是
                    // 「宣布过的条目」与「终局列出的条目」是同一个集合，`output_index` 也就是
                    // 该条目在 `output` 数组里的位置。
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
                        yield Ok(Bytes::from(events));
                    }

                    // 内容增量
                    if state.initialized {
                        if let Some(text) = delta.and_then(|d| d.get("content")).and_then(|c| c.as_str()) {
                            if !text.is_empty() {
                                state.text.push_str(text);
                                let mut events = String::new();
                                // message 条目在**第一次有文本时**宣布：拿到自己的 `output_index`
                                // （与工具调用条目共用同一个单调计数器 ⇒ 不会撞车）。
                                let msg_index = match state.message_output_index {
                                    Some(index) => index,
                                    None => {
                                        let (index, announce) = state.announce_message_item(&format!("msg_{id}"));
                                        events.push_str(&announce);
                                        index
                                    }
                                };
                                let _ = write!(
                                    events,
                                    "event: response.output_text.delta\ndata: {}\n\n",
                                    serde_json::to_string(&json!({
                                        "type": "response.output_text.delta",
                                        "item_id": format!("msg_{id}"),
                                        "output_index": msg_index,
                                        "content_index": 0,
                                        "delta": text
                                    })).unwrap_or_default()
                                );
                                yield Ok(Bytes::from(events));
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
                            // 累积成 chat 形状的 tool_call（终局对象由它构造 function_call 项）：
                            // 同一个 call id 的 arguments 增量必须拼接，否则终局拿不到完整 JSON。
                            match state
                                .tool_calls
                                .iter_mut()
                                .find(|e| e.get("id").and_then(|i| i.as_str()) == Some(tc_id))
                            {
                                Some(existing) => {
                                    if !tc_name.is_empty() {
                                        existing["function"]["name"] = json!(tc_name);
                                    }
                                    let args = existing["function"]["arguments"]
                                        .as_str()
                                        .unwrap_or("")
                                        .to_string();
                                    existing["function"]["arguments"] = json!(format!("{args}{tc_args}"));
                                }
                                None => state.tool_calls.push(json!({
                                    "id": tc_id,
                                    "type": "function",
                                    "function": {"name": tc_name, "arguments": tc_args}
                                })),
                            }
                            // 首次见到该 call id ⇒ 宣布它的条目，并从**同一个**单调计数器拿到
                            // `output_index`（与 message 条目共用一个计数器 ⇒ 每个条目唯一）。
                            let key = tc_id.to_string();
                            let item_id = format!("fc_{tc_id}");
                            let output_index = match state.tool_call_indices.get(&key) {
                                Some(index) => *index,
                                None => {
                                    let index = state.next_index();
                                    state.tool_call_indices.insert(key, index);
                                    let data = serde_json::to_string(&json!({
                                        "type": "response.output_item.added",
                                        "output_index": index,
                                        "item": {
                                            "id": item_id,
                                            "type": "function_call",
                                            "call_id": tc_id,
                                            "name": tc_name,
                                            "arguments": tc_args
                                        }
                                    })).unwrap_or_default();
                                    yield Ok(Bytes::from(format!("event: response.output_item.added\ndata: {data}\n\n")));
                                    index
                                }
                            };
                            // 增量事件用**该条目自己的**索引（同一 chunk 里多个 tool_call、或
                            // 同一个 call id 的续块，都不能靠「刚宣布的那个」推出来）
                            let data = serde_json::to_string(&json!({
                                "type": "response.function_call_arguments.delta",
                                "item_id": item_id,
                                "output_index": output_index,
                                "delta": tc_args
                            })).unwrap_or_default();
                            yield Ok(Bytes::from(format!("event: response.function_call_arguments.delta\ndata: {data}\n\n")));
                        }
                    }

                    // finish_reason → response.completed（与 [DONE] 分支共用，全局只发一次）
                    if let Some(fr) = finish_reason {
                        if !fr.is_empty() {
                            if let Some(event) = state.completed_event() {
                                yield Ok(event);
                            }
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
        let mut cached_tokens: f64 = 0.0;
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
                                            cached_tokens = u.get("cache_read_input_tokens").and_then(|x| x.as_f64()).unwrap_or(cached_tokens);
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
                                        cached_tokens = u.get("cache_read_input_tokens").and_then(|x| x.as_f64()).unwrap_or(cached_tokens);
                                    }
                                    record_usage(&usage, (input_tokens - cached_tokens).max(0.0), cached_tokens, output_tokens);
                                    let stop_reason = v.get("delta").and_then(|d| d.get("stop_reason")).and_then(|s| s.as_str());
                                    // 完成信号与整包翻译器同源（protocol 侧唯一真源）
                                    let finish =
                                        crate::protocol::anthropic_to_openai_chat_finish_reason(stop_reason);
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
                                        record_usage(&usage, (input_tokens - cached_tokens).max(0.0), cached_tokens, output_tokens);
                                        let data = json!({
                                            "id": format!("chatcmpl-{msg_id}"),
                                            "object": "chat.completion.chunk",
                                            "created": 0,
                                            "model": model,
                                            "choices": [{
                                                "index": 0,
                                                "delta": {},
                                                // 上游没给 message_delta（没有 stop_reason）→ 与整包翻译器同源
                                                "finish_reason":
                                                    crate::protocol::anthropic_to_openai_chat_finish_reason(None)
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
            record_usage(&usage, (input_tokens - cached_tokens).max(0.0), cached_tokens, output_tokens);
            let data = json!({
                "id": format!("chatcmpl-{msg_id}"),
                "object": "chat.completion.chunk",
                "created": 0,
                "model": model,
                "choices": [{
                    "index": 0,
                    "delta": {},
                    "finish_reason": crate::protocol::anthropic_to_openai_chat_finish_reason(None)
                }]
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
                                        let cached = u
                                            .pointer("/input_tokens_details/cached_tokens")
                                            .and_then(|x| x.as_f64())
                                            .unwrap_or(0.0);
                                        let output = u.get("output_tokens").and_then(|x| x.as_f64()).unwrap_or(0.0);
                                        // input disjoint：input_tokens 含缓存命中部分（rant 2026-08-23T08:20:38）
                                        record_usage(&usage, (input - cached).max(0.0), cached, output);
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
                                                // Responses 侧没有 finish_reason，只能由内容推断；与整包
                                                // 翻译器同源。已经吐过 tool_calls 增量时必须报 tool_calls，
                                                // 否则 OpenAI 客户端不会派发工具。
                                                "finish_reason":
                                                    crate::protocol::openai_responses_to_openai_chat_finish_reason(
                                                        !tool_states.is_empty()
                                                    )
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
                "choices": [{
                    "index": 0,
                    "delta": {},
                    "finish_reason": crate::protocol::openai_responses_to_openai_chat_finish_reason(
                        !tool_states.is_empty()
                    )
                }]
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
        let Some((i, _c, o)) = *slot.lock().unwrap() else {
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
        let Some((i, _c, o)) = *slot.lock().unwrap() else {
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
        let Some((i, _c, o)) = *slot.lock().unwrap() else {
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
        let Some((i, _c, o)) = *slot.lock().unwrap() else {
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
        let Some((i, _c, o)) = *slot.lock().unwrap() else {
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
        let Some((i, _c, o)) = *slot.lock().unwrap() else {
            panic!("usage missing");
        };
        assert_eq!((i, o), (3.0, 0.0), "usage recorded");
    }

    // ── 缓存计费三拼写 + disjoint（rant 2026-08-23T08:20:38 P0）──

    #[test]
    fn deepseek_cache_hit_tokens_spelling() {
        // DeepSeek 原生顶层 prompt_cache_hit_tokens：prompt_tokens=100 含 90 命中
        // → input disjoint 为 10、cached=90、output=50；下游 input_tokens 同样不含命中
        let slot = usage_slot();
        let chunks = sse_chunks(vec![
            "data: {\"id\":\"c1\",\"model\":\"m1\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"x\"},\"finish_reason\":null}]}\n\n".to_string(),
            "data: {\"id\":\"c1\",\"model\":\"m1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":100,\"prompt_cache_hit_tokens\":90,\"completion_tokens\":50}}\n\n".to_string(),
            "data: [DONE]\n\n".to_string(),
        ]);
        let out = collect(openai_sse_to_anthropic(chunks, slot.clone()));
        let Some((i, c, o)) = *slot.lock().unwrap() else {
            panic!("usage missing");
        };
        assert_eq!((i, c, o), (10.0, 90.0, 50.0), "deepseek disjoint usage");
        assert!(
            out.contains("\"input_tokens\":10"),
            "downstream input_tokens disjoint: {out}"
        );
        assert!(
            out.contains("\"cache_read_input_tokens\":90"),
            "downstream cache_read forwarded: {out}"
        );
    }

    #[test]
    fn openai_cached_tokens_spelling_disjoint() {
        // OpenAI 拼写 prompt_tokens_details.cached_tokens
        let slot = usage_slot();
        let chunks = sse_chunks(vec![
            "data: {\"id\":\"c1\",\"model\":\"m1\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"x\"},\"finish_reason\":null}]}\n\n".to_string(),
            "data: {\"id\":\"c1\",\"model\":\"m1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":100,\"prompt_tokens_details\":{\"cached_tokens\":90},\"completion_tokens\":50}}\n\n".to_string(),
            "data: [DONE]\n\n".to_string(),
        ]);
        let out = collect(openai_sse_to_anthropic(chunks, slot.clone()));
        let Some((i, c, o)) = *slot.lock().unwrap() else {
            panic!("usage missing");
        };
        assert_eq!((i, c, o), (10.0, 90.0, 50.0), "openai disjoint usage");
        assert!(
            out.contains("\"cache_read_input_tokens\":90"),
            "downstream cache_read forwarded: {out}"
        );
    }

    #[test]
    fn anthropic_cache_read_spelling_disjoint() {
        // Anthropic 拼写 cache_read_input_tokens（message_start 计 input，message_delta 计 output）
        let slot = usage_slot();
        let chunks = sse_chunks(vec![
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"model\":\"claude-x\",\"usage\":{\"input_tokens\":100,\"cache_read_input_tokens\":90,\"output_tokens\":0}}}\n\n".to_string(),
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n".to_string(),
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\n".to_string(),
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n".to_string(),
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":50}}\n\n".to_string(),
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string(),
        ]);
        let out = collect(anthropic_sse_to_openai(chunks, slot.clone()));
        assert!(out.contains("data: [DONE]"), "done: {out}");
        let Some((i, c, o)) = *slot.lock().unwrap() else {
            panic!("usage missing");
        };
        assert_eq!((i, c, o), (10.0, 90.0, 50.0), "anthropic disjoint usage");
    }

    // ── 完成信号：流式与整包两条路径必须同值（同一协议对只有一个答案）──

    /// 流式输出里最后一个非 null 的 `finish_reason`
    fn last_finish_reason(out: &str) -> Option<String> {
        let mut last = None;
        for line in out.lines() {
            let Some(rest) = line.strip_prefix("data: ") else {
                continue;
            };
            let Ok(v) = serde_json::from_str::<Value>(rest) else {
                continue;
            };
            if let Some(fr) = v["choices"]
                .get(0)
                .and_then(|c| c.get("finish_reason"))
                .and_then(|f| f.as_str())
            {
                last = Some(fr.to_string());
            }
        }
        last
    }

    /// 流式输出里 `message_delta.stop_reason`：外层 `None` = 整个事件都没发，
    /// 内层 `None` = 事件发了但 `stop_reason` 为 `null`
    fn message_delta_stop_reason(out: &str) -> Option<Option<String>> {
        let mut found = None;
        for line in out.lines() {
            let Some(rest) = line.strip_prefix("data: ") else {
                continue;
            };
            let Ok(v) = serde_json::from_str::<Value>(rest) else {
                continue;
            };
            if v.get("type").and_then(|t| t.as_str()) != Some("message_delta") {
                continue;
            }
            found = Some(
                v["delta"]
                    .get("stop_reason")
                    .and_then(|s| s.as_str())
                    .map(str::to_string),
            );
        }
        found
    }

    /// openai_chat 上游流（finish_reason 与是否带 tool call 可变）→ anthropic 客户端的终局 stop_reason
    fn stream_openai_to_anthropic(
        finish_reason: Option<&str>,
        with_tool_call: bool,
    ) -> Option<Option<String>> {
        let mut items = vec![
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n".to_string(),
        ];
        if with_tool_call {
            items.push("data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"function\":{\"name\":\"f\",\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n".to_string());
        } else {
            items.push("data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"x\"},\"finish_reason\":null}]}\n\n".to_string());
        }
        let fr = match finish_reason {
            Some(v) => format!("\"{v}\""),
            None => "null".to_string(),
        };
        items.push(format!(
            "data: {{\"id\":\"c1\",\"model\":\"m\",\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":{fr}}}],\"usage\":{{\"prompt_tokens\":1,\"completion_tokens\":2}}}}\n\n"
        ));
        items.push("data: [DONE]\n\n".to_string());
        message_delta_stop_reason(&collect(openai_sse_to_anthropic(
            sse_chunks(items),
            usage_slot(),
        )))
    }

    /// 同一份上游数据走整包翻译器
    fn body_openai_to_anthropic(
        finish_reason: Option<&str>,
        with_tool_call: bool,
    ) -> Option<String> {
        let mut message = json!({
            "role": "assistant",
            "content": if with_tool_call { Value::Null } else { json!("x") }
        });
        if with_tool_call {
            message["tool_calls"] = json!([{
                "id": "call_a", "type": "function",
                "function": {"name": "f", "arguments": "{}"}
            }]);
        }
        let body = json!({
            "id": "c1", "model": "m",
            "choices": [{"index": 0, "message": message, "finish_reason": finish_reason}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 2}
        });
        crate::protocol::openai_chat_to_anthropic_resp(&body)["stop_reason"]
            .as_str()
            .map(str::to_string)
    }

    /// anthropic 上游流 → openai_chat 客户端的终局 finish_reason
    fn stream_anthropic_to_openai(stop_reason: &str) -> Option<String> {
        let items = vec![
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"model\":\"claude-x\",\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n".to_string(),
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"x\"}}\n\n".to_string(),
            format!(
                "event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"{stop_reason}\"}},\"usage\":{{\"output_tokens\":2}}}}\n\n"
            ),
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string(),
        ];
        last_finish_reason(&collect(anthropic_sse_to_openai(
            sse_chunks(items),
            usage_slot(),
        )))
    }

    /// responses 上游流（是否带 function call 可变）→ openai_chat 客户端的终局 finish_reason
    fn stream_responses_to_openai(with_tool_call: bool) -> Option<String> {
        let mut items = vec![
            "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"m1\"}}\n\n".to_string(),
        ];
        if with_tool_call {
            items.push("event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_a\",\"name\":\"f\",\"arguments\":\"\"}}\n\n".to_string());
            items.push("event: response.function_call_arguments.delta\ndata: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"call_a\",\"delta\":\"{}\"}\n\n".to_string());
        } else {
            items.push("event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"x\"}\n\n".to_string());
        }
        items.push("event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"object\":\"response\",\"model\":\"m1\",\"output\":[],\"status\":\"completed\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n\n".to_string());
        last_finish_reason(&collect(responses_sse_to_openai_chat(
            sse_chunks(items),
            usage_slot(),
        )))
    }

    /// 同一份上游数据走整包翻译器（responses → openai_chat）
    fn body_responses_to_openai(with_tool_call: bool) -> Option<String> {
        let output = if with_tool_call {
            json!([{"type": "function_call", "call_id": "call_a", "name": "f", "arguments": "{}"}])
        } else {
            json!([{"type": "message", "content": [{"type": "output_text", "text": "x"}]}])
        };
        let body = json!({
            "id": "resp_1", "object": "response", "model": "m1",
            "output": output,
            "usage": {"input_tokens": 1, "output_tokens": 2}
        });
        crate::protocol::openai_responses_to_openai_chat_resp(&body)["choices"][0]["finish_reason"]
            .as_str()
            .map(str::to_string)
    }

    /// 同一个上游完成信号，流式路径与整包路径必须给出同一个完成信号。
    ///
    /// 这是本轴的核心断言：客户端只会看到其中一条路径，两者不一致时
    /// 「是否正常结束 / 要不要派发工具 / 是否被截断」在流式与非流式下答案不同。
    #[test]
    fn completion_signal_agrees_between_stream_and_whole_body() {
        // ① openai_chat 上游 → anthropic 客户端（finish_reason 缺失 + 已吐 tool call 是活场景）
        for fr in [
            None,
            Some("stop"),
            Some("length"),
            Some("tool_calls"),
            Some("function_call"),
            Some("content_filter"),
        ] {
            for with_tool_call in [false, true] {
                let streamed = stream_openai_to_anthropic(fr, with_tool_call).expect(
                    "上游已 [DONE] 收尾，流式路径必须发出 message_delta（否则客户端拿不到 stop_reason）",
                );
                let whole = body_openai_to_anthropic(fr, with_tool_call);
                assert_eq!(
                    streamed, whole,
                    "openai→anthropic finish_reason={fr:?} tool_call={with_tool_call}"
                );
            }
        }

        // ② anthropic 上游 → openai_chat 客户端（未知 stop_reason 必须收敛，不能透传）
        for sr in [
            "end_turn",
            "max_tokens",
            "tool_use",
            "stop_sequence",
            "pause_turn",
            "refusal",
            "not_a_reason",
        ] {
            let streamed = stream_anthropic_to_openai(sr).expect("流式路径必须给出 finish_reason");
            let whole = crate::protocol::anthropic_to_openai_chat_resp(&json!({
                "id": "m1",
                "model": "claude-x",
                "content": [{"type": "text", "text": "x"}],
                "stop_reason": sr,
                "usage": {"input_tokens": 1, "output_tokens": 2}
            }))["choices"][0]["finish_reason"]
                .as_str()
                .map(str::to_string)
                .expect("整包路径必须给出 finish_reason");
            assert_eq!(streamed, whole, "anthropic→openai stop_reason={sr}");
        }

        // ③ responses 上游 → openai_chat 客户端（已吐 tool_calls 增量 ⇒ 终局必须是 tool_calls）
        for with_tool_call in [false, true] {
            let streamed =
                stream_responses_to_openai(with_tool_call).expect("流式路径必须给出 finish_reason");
            let whole =
                body_responses_to_openai(with_tool_call).expect("整包路径必须给出 finish_reason");
            assert_eq!(
                streamed, whole,
                "responses→openai tool_call={with_tool_call}"
            );
            let expected = if with_tool_call { "tool_calls" } else { "stop" };
            assert_eq!(
                streamed, expected,
                "responses→openai tool_call={with_tool_call}"
            );
        }
    }

    // ── 轴：responses 终局事件的形状（流式 vs 整包）─────────────────────────

    /// SSE 文本 → `(event 名, data 负载)` 列表。
    ///
    /// 计数必须按 SSE 的 `event:` 名，不能按负载里的 `type`：负载残缺时（正是本缺陷的
    /// 形态）按 `type` 计数会把整条事件漏掉。
    fn sse_events(out: &str) -> Vec<(String, String)> {
        let mut events = Vec::new();
        for block in out.split("\n\n") {
            let mut name = None;
            let mut data = None;
            for line in block.lines() {
                if let Some(n) = strip_sse_field(line, "event") {
                    name = Some(n.to_string());
                }
                if let Some(d) = strip_sse_field(line, "data") {
                    data = Some(d.to_string());
                }
            }
            if let Some(name) = name {
                events.push((name, data.unwrap_or_default()));
            }
        }
        events
    }

    /// 一个 openai_chat 上游 chunk
    fn chat_chunk(delta: Value, finish_reason: Option<&str>, usage: Option<Value>) -> String {
        let mut v = json!({
            "id": "c1",
            "model": "m1",
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish_reason}]
        });
        if let Some(u) = usage {
            v["usage"] = u;
        }
        format!("data: {v}\n\n")
    }

    /// 跑流式路径 → `(终局 response 对象, 完整 SSE 文本)`，并断言终局事件恰好一次、
    /// 负载带 `response` 对象。
    fn streamed_responses_terminal(chunks: Vec<String>) -> (Value, String) {
        let out = collect(openai_sse_to_openai_responses(
            sse_chunks(chunks),
            usage_slot(),
        ));
        let completed: Vec<(String, String)> = sse_events(&out)
            .into_iter()
            .filter(|(name, _)| name == "response.completed")
            .collect();
        assert_eq!(completed.len(), 1, "终局事件必须恰好一次：{out}");
        let payload: Value = serde_json::from_str(&completed[0].1)
            .unwrap_or_else(|e| panic!("终局负载必须是 JSON（{e}）：{}", completed[0].1));
        assert_eq!(payload["type"], "response.completed", "负载类型：{payload}");
        assert!(
            payload["response"].is_object(),
            "终局负载必须带 response 对象（修复前 [DONE] 分支发的是 `data: {{}}`）：{payload}"
        );
        (payload["response"].clone(), out)
    }

    /// 终局对象的内容必须是整段流累积出来的，不是空壳
    #[test]
    fn responses_terminal_event_is_emitted_once_with_the_full_response_object() {
        let (terminal, out) = streamed_responses_terminal(vec![
            chat_chunk(json!({"role": "assistant", "content": ""}), None, None),
            chat_chunk(json!({"content": "Hel"}), None, None),
            chat_chunk(json!({"content": "lo"}), None, None),
            chat_chunk(
                json!({"tool_calls": [{"index": 0, "id": "call_1", "type": "function",
                    "function": {"name": "get_weather", "arguments": "{\"city\""}}]}),
                None,
                None,
            ),
            chat_chunk(
                json!({"tool_calls": [{"index": 0, "id": "call_1", "type": "function",
                    "function": {"arguments": ": \"SF\"}"}}]}),
                None,
                None,
            ),
            chat_chunk(
                json!({}),
                Some("tool_calls"),
                Some(json!({"prompt_tokens": 7, "completion_tokens": 3, "total_tokens": 10})),
            ),
            "data: [DONE]\n\n".to_string(),
        ]);

        assert_eq!(terminal["id"], "resp_c1");
        assert_eq!(terminal["object"], "response");
        assert_eq!(terminal["model"], "m1");
        assert_eq!(terminal["status"], "completed");
        assert_eq!(
            terminal["usage"],
            json!({"input_tokens": 7, "output_tokens": 3, "total_tokens": 10}),
            "usage 必须按整包路径的规则映射"
        );
        let output = terminal["output"].as_array().expect("output 是数组");
        assert_eq!(
            output.len(),
            2,
            "一个 function_call + 一个 message：{terminal}"
        );
        // 顺序 = **宣布顺序**：本流的文本先到（message 条目先被宣布 ⇒ 索引 0），工具调用后到
        assert_eq!(
            output_type_sequence(&terminal["output"]),
            vec!["message", "function_call"],
            "终局条目顺序必须与流自己的宣布顺序一致：{terminal}"
        );
        assert_eq!(output[0]["type"], "message");
        assert_eq!(output[0]["id"], "msg_c1", "必须与增量事件的 item_id 一致");
        assert_eq!(output[0]["content"][0]["type"], "output_text");
        assert_eq!(output[0]["content"][0]["text"], "Hello");
        assert_eq!(output[1]["type"], "function_call");
        assert_eq!(output[1]["id"], "fc_call_1");
        assert_eq!(output[1]["call_id"], "call_1");
        assert_eq!(output[1]["name"], "get_weather");
        assert_eq!(
            output[1]["arguments"], "{\"city\":\"SF\"}",
            "arguments 增量必须拼接成完整 JSON（并与整包路径同样做一次 parse → 紧凑序列化）"
        );
        assert!(!out.contains("data: {}"), "不得再有空负载的终局事件：{out}");
    }

    /// 上游只发 `[DONE]`、从未给 `finish_reason`：终局事件仍必须是完整对象
    #[test]
    fn responses_terminal_event_without_finish_reason_is_still_complete() {
        // 上游的真实形态：先一个只有 role 的 chunk，再内容 chunk，然后直接 [DONE]
        let text_chunks = || {
            vec![
                chat_chunk(json!({"role": "assistant", "content": ""}), None, None),
                chat_chunk(json!({"content": "Hi"}), None, None),
            ]
        };

        // ① 连 usage 都没有 → 与整包路径一致给三个 0（而不是省略字段）
        let mut chunks = text_chunks();
        chunks.push("data: [DONE]\n\n".to_string());
        let (terminal, _) = streamed_responses_terminal(chunks);
        assert_eq!(terminal["id"], "resp_c1");
        assert_eq!(terminal["model"], "m1");
        assert_eq!(terminal["output"][0]["content"][0]["text"], "Hi");
        assert_eq!(
            terminal["usage"],
            json!({"input_tokens": 0, "output_tokens": 0, "total_tokens": 0})
        );

        // ② 有 usage chunk 但没有 finish_reason → usage 照样带上
        let mut chunks = text_chunks();
        chunks.push(chat_chunk(
            json!({}),
            None,
            Some(json!({"prompt_tokens": 5, "completion_tokens": 2, "total_tokens": 7})),
        ));
        chunks.push("data: [DONE]\n\n".to_string());
        let (terminal, _) = streamed_responses_terminal(chunks);
        assert_eq!(terminal["usage"]["input_tokens"], 5);
        assert_eq!(terminal["usage"]["output_tokens"], 2);
    }

    /// 整包路径的同一份上游内容
    fn whole_body_responses(text: &str, tool_calls: &[Value], usage: Option<Value>) -> Value {
        let mut message = json!({"role": "assistant", "content": text});
        if !tool_calls.is_empty() {
            message["tool_calls"] = Value::Array(tool_calls.to_vec());
        }
        let mut body = json!({
            "id": "c1",
            "model": "m1",
            "choices": [{"index": 0, "message": message, "finish_reason": "stop"}]
        });
        if let Some(u) = usage {
            body["usage"] = u;
        }
        crate::protocol::openai_chat_to_openai_responses_resp(&body)
    }

    /// 流式路径的同一份上游内容（tool_call 的 arguments 拆成两块 —— OpenAI 流式的真实形态）
    fn streamed_responses_chunks(
        text: &str,
        tool_calls: &[Value],
        usage: Option<Value>,
    ) -> Vec<String> {
        let mut chunks = vec![chat_chunk(
            json!({"role": "assistant", "content": ""}),
            None,
            None,
        )];
        if !text.is_empty() {
            chunks.push(chat_chunk(json!({"content": text}), None, None));
        }
        for tc in tool_calls {
            let id = tc["id"].clone();
            let name = tc["function"]["name"].clone();
            let args = tc["function"]["arguments"]
                .as_str()
                .unwrap_or("")
                .to_string();
            let split = args
                .char_indices()
                .nth(4)
                .map(|(i, _)| i)
                .unwrap_or(args.len());
            let (head, tail) = args.split_at(split);
            chunks.push(chat_chunk(
                json!({"tool_calls": [{"index": 0, "id": id.clone(), "type": "function",
                    "function": {"name": name, "arguments": head}}]}),
                None,
                None,
            ));
            if !tail.is_empty() {
                chunks.push(chat_chunk(
                    json!({"tool_calls": [{"index": 0, "id": id, "type": "function",
                        "function": {"arguments": tail}}]}),
                    None,
                    None,
                ));
            }
        }
        chunks.push(chat_chunk(json!({}), Some("stop"), usage));
        chunks.push("data: [DONE]\n\n".to_string());
        chunks
    }

    /// 两条路径的 `response` 对象逐字段比较，返回第一处差异（`None` = 同形）。
    ///
    /// 已记录的两处分叉**不忽略**，而是断言它恰好是已记录的关系：① 流式 id 带 `resp_`
    /// 前缀；② 流式的 message item id 用 `msg_…`（与它自己的增量事件一致）而整包沿用响应 id。
    /// 把分叉字段删掉再比，等于给它们发永久免检证。
    fn responses_parity_diff(streamed: &Value, whole: &Value) -> Option<String> {
        for key in ["object", "model", "usage"] {
            if streamed.get(key) != whole.get(key) {
                return Some(format!(
                    "{key} 不同：{:?} vs {:?}",
                    streamed.get(key),
                    whole.get(key)
                ));
            }
        }
        // 流式终局多一个 `status`（整包路径没有该字段；截断语义属宿主裁定族，未动）
        if streamed.get("status") != Some(&json!("completed")) {
            return Some(format!(
                "status 不是 completed：{:?}",
                streamed.get("status")
            ));
        }
        for key in whole.as_object().expect("整包是对象").keys() {
            if streamed.get(key).is_none() {
                return Some(format!("流式终局缺少整包字段 {key}"));
            }
        }
        match (streamed["id"].as_str(), whole["id"].as_str()) {
            (Some(s), Some(w)) if s == format!("resp_{w}") => {}
            other => return Some(format!("id 前缀分叉不再是已记录的关系：{other:?}")),
        }
        let (Some(s_out), Some(w_out)) =
            (streamed["output"].as_array(), whole["output"].as_array())
        else {
            return Some(format!(
                "output 不是数组：{:?} vs {:?}",
                streamed.get("output"),
                whole.get("output")
            ));
        };
        if s_out.len() != w_out.len() {
            return Some(format!(
                "output 长度不同：{} vs {}",
                s_out.len(),
                w_out.len()
            ));
        }
        // 条目形状**按身份比，不按下标比**：两条路径的**顺序规则刻意不同**（流式＝宣布顺序，
        // 整包＝function_call 在前、message 在后，见各自的断言），按下标比会把「顺序规则
        // 不同」误报成「形状不同」，也会把「形状不同」漏报成「顺序不同」。
        let (s_items, w_items) = (
            output_items_by_identity(s_out),
            output_items_by_identity(w_out),
        );
        if s_items.len() != w_items.len() {
            return Some(format!(
                "条目身份集合不同：{:?} vs {:?}",
                s_items.keys().collect::<Vec<_>>(),
                w_items.keys().collect::<Vec<_>>()
            ));
        }
        for (key, s) in &s_items {
            let Some(w) = w_items.get(key) else {
                return Some(format!("整包路径缺少条目 {key}"));
            };
            if s["type"] != w["type"] {
                return Some(format!(
                    "条目 {key} 的 type 不同：{:?} vs {:?}",
                    s["type"], w["type"]
                ));
            }
            match s["type"].as_str() {
                Some("function_call") => {
                    for field in ["id", "call_id", "name", "arguments"] {
                        if s[field] != w[field] {
                            return Some(format!(
                                "条目 {key} 的 {field} 不同：{:?} vs {:?}",
                                s[field], w[field]
                            ));
                        }
                    }
                }
                Some("message") => {
                    for field in ["role", "content"] {
                        if s[field] != w[field] {
                            return Some(format!(
                                "条目 {key} 的 {field} 不同：{:?} vs {:?}",
                                s[field], w[field]
                            ));
                        }
                    }
                    let (si, wi) = (
                        s["id"].as_str().unwrap_or(""),
                        w["id"].as_str().unwrap_or(""),
                    );
                    if si != format!("msg_{wi}") {
                        return Some(format!(
                            "message 条目 id 前缀分叉不再是已记录的关系：{si} vs {wi}"
                        ));
                    }
                }
                other => return Some(format!("条目 {key} 的 type 未知：{other:?}")),
            }
        }
        None
    }

    /// 终局 `output` → 身份键 → 条目。
    ///
    /// 身份键：`message` 条目用 `"message"`（本形状下每个响应至多一个 message 条目）；
    /// `function_call` 条目用协议自己用来关联工具调用的 `call_id`。
    fn output_items_by_identity(output: &[Value]) -> std::collections::BTreeMap<String, &Value> {
        let mut map = std::collections::BTreeMap::new();
        for item in output {
            let key = match item["type"].as_str().unwrap_or("") {
                "message" => "message".to_string(),
                "function_call" => {
                    format!("function_call:{}", item["call_id"].as_str().unwrap_or(""))
                }
                other => format!("unknown:{other}"),
            };
            map.insert(key, item);
        }
        map
    }

    /// 终局 `output` 的条目类型序列（用来把两条路径各自的**顺序规则**钉成显式断言）。
    fn output_type_sequence(output: &Value) -> Vec<String> {
        output
            .as_array()
            .expect("output 是数组")
            .iter()
            .map(|item| item["type"].as_str().unwrap_or("").to_string())
            .collect()
    }

    /// 同一份上游内容：流式终局对象与整包对象必须同形（形状字段，不只完成信号）
    ///
    /// 两件事同时成立：① 条目**形状**按身份逐字段相同；② 两条路径各自遵守自己的
    /// **顺序规则**（流式＝宣布顺序 ⇒ 有文本时 message 在前；整包＝既有约定 ⇒ function_call
    /// 在前）—— 规则不同是刻意的，这里把两条都写成显式断言，而不是把整包也改掉。
    #[test]
    fn responses_terminal_shape_agrees_between_stream_and_whole_body() {
        let cases: Vec<(&str, Vec<Value>, Option<Value>)> = vec![
            (
                "Hello",
                vec![],
                Some(json!({"prompt_tokens": 7, "completion_tokens": 3, "total_tokens": 10})),
            ),
            (
                "Hello",
                vec![json!({"id": "call_1", "type": "function",
                    "function": {"name": "get_weather", "arguments": "{\"city\":\"SF\"}"}})],
                Some(json!({"prompt_tokens": 9, "completion_tokens": 4, "total_tokens": 13})),
            ),
            (
                "两个工具调用",
                vec![
                    json!({"id": "call_1", "type": "function",
                        "function": {"name": "a", "arguments": "{\"x\":1}"}}),
                    json!({"id": "call_2", "type": "function",
                        "function": {"name": "b", "arguments": "{}"}}),
                ],
                Some(json!({"prompt_tokens": 11, "completion_tokens": 6, "total_tokens": 17})),
            ),
            (
                "",
                vec![json!({"id": "call_2", "type": "function",
                    "function": {"name": "f", "arguments": "{}"}})],
                None,
            ),
            ("只有参数没有 usage 的文本", vec![], None),
        ];
        for (text, tool_calls, usage) in cases {
            let (streamed, out) = streamed_responses_terminal(streamed_responses_chunks(
                text,
                &tool_calls,
                usage.clone(),
            ));
            let whole = whole_body_responses(text, &tool_calls, usage);

            // 不变式：流**宣布过的条目** == 终局 `output` 的条目（同集合、同顺序）
            let announced = announced_items(&out);
            assert_eq!(
                announced
                    .iter()
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>(),
                output_item_ids(&streamed["output"]),
                "终局 output 必须恰好是本流宣布过的条目、且按宣布顺序（{text:?}）：{out}"
            );

            // 顺序规则：流式＝宣布顺序（有文本时 message 先），整包＝既有约定（工具调用先）
            let mut expected_streamed: Vec<&str> = vec!["function_call"; tool_calls.len()];
            let mut expected_whole: Vec<&str> = expected_streamed.clone();
            if !text.is_empty() {
                // 流式：message 条目在文本到达时先被宣布 ⇒ 排在前面
                expected_streamed.insert(0, "message");
                // 整包：既有约定是工具调用在前、message 在后
                expected_whole.push("message");
            }
            assert_eq!(
                output_type_sequence(&streamed["output"]),
                expected_streamed,
                "流式顺序规则 = 宣布顺序（{text:?}）"
            );
            assert_eq!(
                output_type_sequence(&whole["output"]),
                expected_whole,
                "整包顺序规则 = 工具调用在前（{text:?}）"
            );

            if let Some(diff) = responses_parity_diff(&streamed, &whole) {
                panic!("流式与整包形状不一致（{text:?}）：{diff}\n流式：{streamed}\n整包：{whole}\n{out}");
            }
        }
    }

    // ── 轴：条目的身份（`output_index`）与终局顺序（C2077/C2078）──────────────

    /// 流里 `response.output_item.added` 事件按发出顺序的 `(item id, output_index)`
    fn announced_items(out: &str) -> Vec<(String, usize)> {
        sse_events(out)
            .into_iter()
            .filter(|(name, _)| name == "response.output_item.added")
            .map(|(_, data)| {
                let v: Value = serde_json::from_str(&data)
                    .unwrap_or_else(|e| panic!("条目宣布事件负载必须是 JSON（{e}）：{data}"));
                let id = v["item"]["id"].as_str().unwrap_or("").to_string();
                let index = v["output_index"]
                    .as_u64()
                    .unwrap_or_else(|| panic!("output_index 必须是整数：{v}"))
                    as usize;
                (id, index)
            })
            .collect()
    }

    /// 终局 `output` 的条目 id 序列
    fn output_item_ids(output: &Value) -> Vec<String> {
        output
            .as_array()
            .expect("output 是数组")
            .iter()
            .map(|item| item["id"].as_str().unwrap_or("").to_string())
            .collect()
    }

    /// 轴：每个被宣布的条目拿到**唯一**的 `output_index`，按宣布顺序单调递增，且终局
    /// `output` 按同一顺序列出它们。
    ///
    /// 修复前（C2077 量到）：message 条目的索引写死 0，而 `tool_output_index` 从 0 起 ⇒
    /// 「文本 + 工具调用」的响应里 message 与第一个 function_call **都是 0**，客户端按
    /// `output_index` 跟踪条目时后一个覆盖前一个（通常丢掉 agent 客户端在等的工具调用）。
    #[test]
    fn responses_item_indices_are_unique_and_match_the_announced_order() {
        let chunks = vec![
            chat_chunk(json!({"role": "assistant", "content": ""}), None, None),
            chat_chunk(json!({"content": "Hello"}), None, None),
            // 同一个 chunk 里两个工具调用（索引必须各自分配）
            chat_chunk(
                json!({"tool_calls": [
                    {"index": 0, "id": "call_1", "type": "function",
                     "function": {"name": "a", "arguments": "{\"x\":"}},
                    {"index": 1, "id": "call_2", "type": "function",
                     "function": {"name": "b", "arguments": "{}"}}
                ]}),
                None,
                None,
            ),
            // 第一个工具调用的**续块**：增量事件必须仍用它自己的索引（不能靠「刚宣布的那个」）
            chat_chunk(
                json!({"tool_calls": [
                    {"index": 0, "id": "call_1", "type": "function", "function": {"arguments": "1}"}}
                ]}),
                None,
                None,
            ),
            chat_chunk(json!({}), Some("tool_calls"), None),
        ];
        let (terminal, out) = streamed_responses_terminal(chunks);

        // ① 三个条目、索引恰好 {0,1,2}（断言**多重集**，不只断长度）
        let announced = announced_items(&out);
        assert_eq!(
            announced
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>(),
            vec!["msg_c1", "fc_call_1", "fc_call_2"],
            "宣布顺序：message 先（文本先到），随后各 function_call：{out}"
        );
        let indices: Vec<usize> = announced.iter().map(|(_, index)| *index).collect();
        let mut sorted = indices.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            vec![0, 1, 2],
            "索引必须互不相同：{indices:?}\n{out}"
        );
        assert_eq!(indices, vec![0, 1, 2], "且按宣布顺序单调递增：{indices:?}");
        assert!(
            out.contains("\"arguments\":\"{\\\"x\\\":1}\""),
            "两段参数必须拼成完整 JSON：{out}"
        );

        // ② 终局 `output` 的顺序 == 宣布顺序（逐项按 id 对齐，不按下标猜）
        assert_eq!(
            output_item_ids(&terminal["output"]),
            announced
                .iter()
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>(),
            "流宣布了什么顺序，终局就列什么顺序：{terminal}"
        );

        // ③ 每个参数增量事件必须引用**它自己的**条目索引
        let mut deltas = 0;
        for (name, data) in sse_events(&out) {
            if name != "response.function_call_arguments.delta" {
                continue;
            }
            let v: Value = serde_json::from_str(&data).expect("增量负载是 JSON");
            let item_id = v["item_id"].as_str().unwrap_or("");
            let expected = announced
                .iter()
                .find(|(id, _)| id == item_id)
                .map(|(_, index)| *index)
                .unwrap_or_else(|| panic!("增量事件的 item_id 必须宣布过：{item_id}\n{out}"));
            assert_eq!(
                v["output_index"].as_u64().unwrap_or(999) as usize,
                expected,
                "参数增量必须引用该条目自己的索引：{v}"
            );
            deltas += 1;
        }
        assert_eq!(deltas, 3, "call_1 两段 + call_2 一段：{out}");
    }

    /// 「工具调用先到、文本后到」的流：顺序规则仍然是**宣布顺序**（不是「message 永远第一」）。
    #[test]
    fn responses_items_follow_the_announcement_order_when_tool_calls_come_first() {
        let chunks = vec![
            chat_chunk(json!({"role": "assistant", "content": ""}), None, None),
            chat_chunk(
                json!({"tool_calls": [{"index": 0, "id": "call_9", "type": "function",
                    "function": {"name": "f", "arguments": "{}"}}]}),
                None,
                None,
            ),
            chat_chunk(json!({"content": "after"}), None, None),
            chat_chunk(json!({}), Some("stop"), None),
        ];
        let (terminal, out) = streamed_responses_terminal(chunks);

        let announced = announced_items(&out);
        assert_eq!(
            announced
                .iter()
                .map(|(id, index)| (id.as_str(), *index))
                .collect::<Vec<_>>(),
            vec![("fc_call_9", 0), ("msg_c1", 1)],
            "索引顺序＝宣布顺序（工具调用先到 ⇒ 它先拿索引）：{out}"
        );
        assert_eq!(
            output_item_ids(&terminal["output"]),
            vec!["fc_call_9".to_string(), "msg_c1".to_string()],
            "终局顺序跟宣布顺序（不是跟整包的「工具调用在前」）：{terminal}"
        );
        assert_eq!(
            terminal["output"][1]["content"][0]["text"], "after",
            "文本仍必须送达：{terminal}"
        );

        let whole = whole_body_responses(
            "after",
            &[json!({"id": "call_9", "type": "function",
                "function": {"name": "f", "arguments": "{}"}})],
            None,
        );
        if let Some(diff) = responses_parity_diff(&terminal, &whole) {
            panic!("工具调用先到的流也必须与整包同形（按身份比）：{diff}\n流式：{terminal}\n整包：{whole}\n{out}");
        }
    }

    /// 没有文本、没有工具调用的流：**不宣布任何条目**，终局 `output` 也是空的 ——
    /// 「宣布过的条目」与「终局列出的条目」是同一个集合，这条极简流也不例外。
    #[test]
    fn responses_stream_without_content_announces_no_items() {
        let chunks = vec![
            chat_chunk(json!({"role": "assistant", "content": ""}), None, None),
            chat_chunk(json!({}), Some("stop"), None),
        ];
        let (terminal, out) = streamed_responses_terminal(chunks);
        assert!(
            announced_items(&out).is_empty(),
            "无内容 ⇒ 不宣布条目：{out}"
        );
        assert_eq!(
            terminal["output"],
            json!([]),
            "终局 output 与整包路径一致（都为空）：{terminal}"
        );
    }

    /// 阳性对照：parity 比较器必须能拒绝形状不同的对象（否则「一致」是永久免检证）
    #[test]
    fn responses_parity_comparator_rejects_wrong_shapes() {
        let whole = whole_body_responses(
            "Hello",
            &[],
            Some(json!({"prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3})),
        );
        assert!(
            responses_parity_diff(&whole, &whole).is_some(),
            "拿整包对象冒充流式终局必须被拒（无 resp_ 前缀、无 status）"
        );

        // 先造一个「除已记录分叉外处处相同」的流式对象：比较器必须放行
        let mut base = whole.clone();
        base["id"] = json!("resp_c1");
        base["status"] = json!("completed");
        base["output"][0]["id"] = json!("msg_c1");
        assert!(
            responses_parity_diff(&base, &whole).is_none(),
            "修正已记录分叉后必须判为同形：{:?}",
            responses_parity_diff(&base, &whole)
        );

        // 再逐个注入偏差：比较器必须每一处都报出来
        type Mutate = fn(&mut Value);
        let cases: Vec<(&str, Mutate)> = vec![
            ("object", |v| v["object"] = json!("not_a_response")),
            ("model", |v| v["model"] = json!("other-model")),
            ("usage.output_tokens", |v| {
                v["usage"]["output_tokens"] = json!(99)
            }),
            ("status", |v| v["status"] = json!("incomplete")),
            ("output 长度", |v| v["output"] = json!([])),
            ("output[0].content", |v| {
                v["output"][0]["content"][0]["text"] = json!("tampered")
            }),
            ("output[0].id 前缀关系", |v| {
                v["output"][0]["id"] = json!("msg_zzz")
            }),
        ];
        for (label, mutate) in cases {
            let mut broken = base.clone();
            mutate(&mut broken);
            assert!(
                responses_parity_diff(&broken, &whole).is_some(),
                "注入 {label} 偏差后比较器必须报差异：{broken}"
            );
        }
    }
}
