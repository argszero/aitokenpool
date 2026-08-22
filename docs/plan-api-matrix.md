# AITokenPool 协议支持现状

> 网关对外暴露的协议、支持的客户端与上游接入现状（历史调研细节见 CHANGELOG / git 历史）。

## 1. 对外协议（网关已实现）

| 协议 | 端点 | 流式 |
|---|---|---|
| OpenAI Chat Completions | `POST /v1/chat/completions` | ✅ SSE |
| OpenAI Responses | `POST /v1/responses` | ✅ SSE |
| Anthropic Messages | `POST /anthropic/v1/messages` | ✅ SSE |

三协议**双向转换**（`src/protocol.rs` + `src/sse.rs`）：客户端只需对接一个 OpenAI 兼容端点，网关按上游实际协议转发。流式场景同样跨协议转换（如 Anthropic → OpenAI SSE）。

## 2. 客户端接入

| 客户端 | 首选协议 | 配置方式 |
|---|---|---|
| Claude Code | Anthropic | `ANTHROPIC_BASE_URL` + `ANTHROPIC_AUTH_TOKEN` |
| Cursor | OpenAI | Override OpenAI Base URL |
| Cline / Roo Code | OpenAI 兼容 | Provider 设为 OpenAI Compatible |
| Codex | OpenAI Responses | 用 Responses 端点 |

Base URL 统一由 AITokenPool 提供（`[server].public_url` 配置），设置页展示；`GET /v1/models` 返回模型与可用 key 列表。

## 3. 上游接入现状

- 上游 key 在「共享管理」页上架（provider + plan + model + key + 额度 + 可用时段）
- 模型价格由 `config/config.example.toml` 的 `[[models]]` 定义（唯一真源，启动 seed 入 `models` 表）
- 内置 DeepSeek 官方 CNY 定价（含高峰时段价）；其余厂商可按同格式配置
- 支持的 provider：deepseek、zhipu(glm)、openai、anthropic、google、bytedance、minimax、aliyun 等（随 `[[models]]` 配置扩展，无需改代码）

## 4. 模型目录

当前内置 13 个模型（DeepSeek / GLM / GPT / Claude / Gemini / 豆包 / MiniMax / 通义等），含 context window、vision 支持、缓存价与高峰价字段。模型列表可经管理端「模型管理」页 CRUD（需 admin 角色）。
