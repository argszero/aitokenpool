# AITokenPool 架构

> 本文说明**现状**：现在是什么、能干什么、将来怎么走（历史变更见 [CHANGELOG.md](../CHANGELOG.md)）。

## 1. 定位

开源的 AI Token 共享平台 / 多模型网关，双模式共用同一套核心：

- **企业版**：私有部署，公司 key 池 + 员工点数配额（配额凭证，单向）
- **公共版**：共享市场，用户分享闲置 key 赚点数、消费他人 key（交换媒介，双向）

**中心化架构**：平台托管 key + 平台执行调用——平台是唯一可信执行者，计量可信、响应真实。

## 2. 技术栈（现状）

| 层 | 选型 |
|---|---|
| 后端 | **Rust**（`rust-version 1.86`）+ axum + tokio + rusqlite |
| 数据库 | **SQLite**（单文件，`data/aitokenpool.db`，迁移 v10） |
| 加密 | AES-256-GCM（上游 key，`src/crypto.rs`）、argon2（密码哈希） |
| 上游调用 | reqwest（非流式）+ SSE 流式转发（`src/sse.rs` 跨协议转换） |
| 前端 | **原生 JS** 静态页（`ui/`，无构建步骤；i18n 中英双语） |
| 部署 | Docker（多阶段构建，非 root）或 `cargo run` |

## 3. 模块（src/）

| 模块 | 职责 |
|---|---|
| `router.rs` | 网关路由：多 Provider 选择、粘性、静默故障转移（3 次上限、5 秒健康冷却） |
| `protocol.rs` | OpenAI Chat / Responses / Anthropic Messages 三协议**双向互转** |
| `sse.rs` | 流式 SSE 跨协议转换 + usage 计量 |
| `billing.rs` | 计量计费：token → 价格 → CNY 锚定点数（1 点 = 1 元，5 位小数）；高峰时段计价 |
| `gift.rs` | 新人每日赠送（注册起 10 天，当日有效，惰性过期清理） |
| `auth.rs` / `mail.rs` | Bearer 认证（API Key）+ argon2；SMTP 验证码（重试 3 次） |
| `db.rs` | SQLite 建表 + 幂等迁移 + seed（仅测试） |
| `dao.rs` | 数据访问层 |
| `routes/` | 认证 / 钱包 / 交易 / 仪表盘 / 共享 / 管理 / 运营者 API |

## 4. 数据流（一次调用）

```
客户端 → POST /v1/chat/completions（或 /v1/responses、/anthropic/v1/messages）
  → auth 校验（Bearer atk_* key → 用户）
  → 余额预检（可用 = gift + permanent，≤0 → 402）
  → 路由选 key（健康优先 → 随机 → 粘性复用）
  → 上游请求（解密 key，非流式或 SSE 转发）
  → 成功后 settle：扣消费者 → 加分享者 90% → 写 transactions + usage_records + keys.used
```

## 5. 数据库（实表）

| 表 | 说明 |
|---|---|
| `users` | 用户（email / password_hash / name / role / verified） |
| `keys` | 上游 key（provider / plan / model / 加密密文 / 额度 / 可用时间段 / note） |
| `api_keys` | 分发 key（`atk_live_` 前缀，绑定用户，可撤销） |
| `models` | 模型价格（input / output / cache_hit，可选高峰价；config `[[models]]` 为唯一真源） |
| `quotas` | 点数账户（balance 永久 + gift_balance 有效赠送） |
| `gift_grants` | 赠送明细（amount / expires_at / status: active\|used\|expired） |
| `transactions` | 交易流水（type: consume\|earn\|topup\|gift；含 token 明细列） |
| `usage_records` | 调用明细（tokens 拆 input / cached / output） |
| `departments` / `raise_requests` | 部门 + 成员加额申请（企业版） |

## 6. API 一览

- `GET /healthz` — 健康检查（返回版本号）
- `POST /api/auth/register` / `login` / `verify` / `resend-code` / `forgot` / `change-password`
- `GET /api/me` — 当前用户
- `POST/GET/DELETE /api/api-keys` — 分发 key 生成 / 列表 / 撤销
- `GET /api/models` — 模型列表（含可用 key 与价格）
- `POST /v1/chat/completions` / `/v1/responses` / `/anthropic/v1/messages` — 网关（非流式 + SSE）
- `GET /api/wallet` / `/api/transactions` / `/api/dashboard` — 钱包 / 交易（summary + 明细）/ 仪表盘
- `POST/GET/PATCH /api/sharings` — key 上架 / 列表 / 暂停下线
- `POST /api/admin/credits` / `GET /api/admin/users` / `usage` / `models` CRUD — 管理（role=admin）
- `GET /api/ops/runtime` / `credits` / `users` — 运营者视图（role=ops）
- `GET /api/config` — 前端动态配置（public_url 等）

## 7. 部署

- Docker：`docker compose up -d --build`，或镜像 `ghcr.io/argszero/aitokenpool:<tag>`（**镜像随版本 tag 发布**，latest 指向最新发版）
- 数据目录统一在 `ATP_DATA_DIR`（默认 `./data`：config.toml + db + logs/）
- 生产必设 `ATP_MASTER_KEY`（上游 key 加密）；首次启动自动创建初始管理员（随机密码打印在日志）

## 8. Roadmap（规划，未实现）

- **P2**：前端深化（chat-modal 流式接网关、SSE 续传、key 缓存）
- **P3**：公共版共享市场深化（撮合 / 信誉体系）
- **P4**：多地节点 / 地域路由 / PostgreSQL / Redis（当前为单机 SQLite，无外部依赖）

> 注：早期文档中提及的 React / Vite / Tauri 桌面端、PostgreSQL 均**未实现**——前端为原生 JS 静态页，数据库为 SQLite。
