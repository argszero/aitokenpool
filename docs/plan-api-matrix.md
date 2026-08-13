# 国内主流 Token Plan / Code Plan 调研：API 协议与 Base URL

> v0.1（2026-08-13）· browser-harness 实抓官方文档 + Google 官方文档源

## 0. 核心结论

国内主流 Coding Plan / Token Plan **全部走两条协议**：
1. **OpenAI 兼容协议**（Chat Completions，`/chat/completions`）——接 Cursor、Cline、Roo Code、OpenCode 等
2. **Anthropic 兼容协议**（Messages，`/v1/messages`）——接 Claude Code、Goose、OpenClaw 等

**没有一家当前原生支持 OpenAI Responses API**（`/responses`），只有 DeepSeek/智谱按量付费 API 支持。Coding Plan 的专属端点基本只暴露 OpenAI Chat + Anthropic Messages 两种。

**关键设计含义**：AITokenPool 网关层**第一版只需实现两个协议适配器**：
- OpenAI Chat Completions（`/chat/completions`，含 SSE 流式）
- Anthropic Messages（`/v1/messages`，含 SSE 流式）

即可覆盖国内全部主流 Coding Plan 的上游接入，以及大多数下游客户端（Cursor/Claude Code/Cline…）。

---

## 1. 各家 Plan 的协议与 Base URL

### 1.1 阿里云百炼 Token Plan（个人版/团队版）
- **产品**：Token Plan，Credits 统一计量，一份订阅多工具通用（Claude Code/Cursor/Qwen Code/Qoder/OpenClaw/Cline…）
- **Key**：专属 key，前缀 `sk-sp-`（普通按量 `sk-` 会 401）
- **限制**：仅限交互式编程工具，禁止自动化批量调用（后端服务）

| 协议 | Base URL |
|---|---|
| OpenAI 兼容 | `https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1` |
| Anthropic 兼容 | `https://token-plan.cn-beijing.maas.aliyuncs.com/apps/anthropic` |

- 模型：qwen3.8-max、qwen3.7-plus、qwen3.7-flash、deepseek-v4-pro、kimi-k3、glm-5.2、MiniMax-M3 等（多厂商三方直供）

### 1.2 智谱 GLM Coding Plan（个人版/团队版）
- **产品**：GLM Coding Plan，套餐抵扣
- **Key**：智谱开放平台专属 API Key

| 协议 | Base URL |
|---|---|
| OpenAI 兼容 | `https://open.bigmodel.cn/api/coding/paas/v4` |
| Anthropic 兼容 | `https://open.bigmodel.cn/api/anthropic` |

- 模型：glm-5.2、glm-5.2[1m]、glm-4.7 等
- Claude Code 用 `ANTHROPIC_BASE_URL` + `ANTHROPIC_AUTH_TOKEN`

### 1.3 字节火山方舟 Coding Plan / Agent Plan
- **产品**：方舟 Coding Plan（编码专属）、Agent Plan（Agent 开发）
- **Key**：方舟控制台专属 Key

| 协议 | Base URL |
|---|---|
| OpenAI 兼容（Agent Plan） | `https://ark.cn-beijing.volces.com/api/plan/v3` |
| Anthropic 兼容（Agent Plan） | `https://ark.cn-beijing.volces.com/api/plan` |
| OpenAI 兼容（Coding Plan） | `https://ark.cn-beijing.volces.com/api/coding/v3` |
| Anthropic 兼容（Coding Plan） | `https://ark.cn-beijing.volces.com/api/coding` |

- 模型：doubao-seed-2.1-pro/turbo、doubao-seed-2.0-code、Seed-2.1-Turbo 等

### 1.4 Kimi（月之暗面）Kimi Code 会员
- **产品**：Kimi Code 会员（¥39/¥79/¥159 档）
- **Key**：Kimi Code 专属 Key（与开放平台 `api.moonshot.cn` 不通用）

| 协议 | Base URL |
|---|---|
| OpenAI 兼容 | `https://api.kimi.com/coding/v1` |
| Anthropic 兼容 | `https://api.kimi.com/coding/` |

- 模型：kimi-for-coding（自动升级别名）、kimi-for-coding-highspeed、kimi-k3

### 1.5 MiniMax Coding Plan
- **产品**：MiniMax Coding Plan（$10/月起，M2.5/M2.7/M3）
- **Key**：Coding Plan 专属 Key（`sk-cp...` 前缀，与普通按量不通用）

| 协议 | 国际站 | 国内站 |
|---|---|---|
| Anthropic 兼容 | `https://api.minimax.io/anthropic` | `https://api.minimaxi.com/anthropic` |
| OpenAI 兼容 | `https://api.minimax.io/v1` | `https://api.minimaxi.com/v1` |

### 1.6 DeepSeek（无订阅 Plan，纯按量 API）
- **产品**：无 Coding Plan，只有按量付费 API（充多少用多少）
- **Key**：普通 `sk-` 开放平台 Key

| 协议 | Base URL |
|---|---|
| OpenAI 兼容 | `https://api.deepseek.com`（Chat Completions） |
| Anthropic 兼容 | `https://api.deepseek.com/anthropic` |
| **Responses API** | `https://api.deepseek.com`（`/responses`，唯一支持者） |

- 模型：deepseek-v4-pro[1m]（主模型）、deepseek-v4-flash（子代理）
- 说明：DeepSeek 是少数**原生支持 Responses API** 的国内厂商

---

## 2. API 协议类型清单（本项目需要覆盖的）

| 协议 | 端点路径 | 流式 | 国内 Plan 支持度 |
|---|---|---|---|
| OpenAI Chat Completions | `POST /chat/completions` | SSE | ✅ 全部支持 |
| OpenAI Responses | `POST /responses` | SSE | ⚠️ 仅 DeepSeek/智谱按量 |
| Anthropic Messages | `POST /v1/messages` | SSE | ✅ 全部支持 |

> 注：多数 Coding Plan 的 Anthropic 端点会自动拼 `/v1/messages`，故工具侧只需填 `.../anthropic` 或 `.../coding` 即可。

---

## 3. 下游客户端协议偏好（决定网关要暴露什么）

| 客户端 | 首选协议 | 说明 |
|---|---|---|
| Claude Code | Anthropic | `ANTHROPIC_BASE_URL` + `ANTHROPIC_AUTH_TOKEN` |
| Cursor | OpenAI | Override OpenAI Base URL |
| Cline / Roo Code | OpenAI 兼容 | Provider 设 OpenAI Compatible |
| Qwen Code / Qoder / OpenClaw / Kilo CLI | OpenAI 或 Anthropic | 视配置 |
| Codex | OpenAI Responses | 若接 Codex 需 Responses 适配 |

---

## 4. 对 AITokenPool 网关层的落地结论

1. **第一版网关只做两个协议**：OpenAI Chat + Anthropic Messages（含 SSE 流式透传）。
2. **Base URL 统一由 AITokenPool 提供**，下游客户端只需指向 AITokenPool 的单一端点，由网关按路由规则转发到具体上游 Plan 的端点。
3. **上游协议映射表**（provider × plan 类型 → base url + 协议），作为 `providers`/`plans` 表的配置数据。
4. **Responses API 后置**：除非要接 Codex 客户端，否则 P0 不做。

---

## 5. 参考数据源

- DeepSeek 官方定价页（已实抓）：确认 OpenAI/Anthropic/Responses 三端点
- 阿里云百炼 Token Plan 官方文档（Base URL 总览 / 快速开始）
- 智谱 Coding Plan 官方文档（接入工具）
- 火山方舟官方文档（接入三方工具 / Agent Plan）
- Kimi Code 会员指南 / MiniMax 官方文档
- OpenRouter `/api/v1/models`（409 模型结构化定价，作国际厂商参考）
