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
| 数据库 | **SQLite**（单文件，`data/aitokenpool.db`，幂等迁移，版本见 `src/db.rs` 的 `SCHEMA_VERSION`） |
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
| `transactions` | 交易流水（type 取值以 `src/routes/wallet.rs` 的 `TX_FILTER_TYPES` 为准；含 token 明细列） |
| `transactions_rollup` | 交易明细折叠出的分钟级**可加**汇总（`src/tx_rollup.rs`）；聚合经视图 `tx_facts` 取数，见下 |
| `usage_records` | 调用明细（tokens 拆 input / cached / output） |
| `departments` / `raise_requests` | 部门 + 成员加额申请（企业版） |
| `email_verifications` | 注册邮箱验证码（与 `users.verified` 配套，v6 起） |
| `schema_version` | 迁移记录（当前版本见 `src/db.rs` 的 `SCHEMA_VERSION`） |

> **明细表的保留策略（v0.7.30 起，库不再无限增长）**：`transactions` 与 `usage_records` 只增不减，
> 服务现在**先归档、再删除**。`transactions`：明细 → 归档成 `<数据目录>/archive/transactions-*.jsonl`
> （`src/archive.rs`）→ 折叠成 `transactions_rollup`（`src/tx_rollup.rs`）→ **聚合改从视图 `tx_facts`
> （＝汇总 ∪ 未折叠明细，`src/tx_facts.rs`）取数** → 删除已折叠且已归档、过了 `[rollup] retain_days`
> 的行。分页列表要的是逐行身份（汇总行没有），仍读明细 ⇒ 它与聚合会在保留窗口之外分叉，
> `/api/transactions` 随响应发布这段边界。`usage_records`：读者全是写死的月/日窗口，**不需要折叠** ——
> 归档后按同一套日历窗口删除（`src/usage_retention.rs`）。两表的开关与批次见 `config.example.toml` 的
> `[archive]` / `[rollup]` / `[usage_retention]`；**归档未就绪（水位为 0）时一行不删**。

## 6. API 一览

> 本表按**功能面**归纳，不是逐条路由的完整清单；权威清单以 `src/routes/mod.rs` 的 `router()` 为准。

- `GET /healthz` — 健康检查（返回版本号）
- `POST /api/auth/register` / `login` / `verify` / `resend-code` / `forgot` / `change-password`
- `GET /api/me` — 当前用户
- `POST/GET/DELETE /api/api-keys` — 分发 key 生成 / 列表 / 撤销
- `GET /api/models` — 模型列表（含可用 key 与价格）
- `POST /v1/chat/completions` / `/v1/responses` / `/anthropic/v1/messages` — 网关（非流式 + SSE）
- `GET /api/wallet` / `/api/transactions` / `/api/dashboard` — 钱包 / 交易（summary + 明细）/ 仪表盘
- `POST/GET/PATCH /api/sharings` — key 上架 / 列表 / 编辑（PATCH 部分更新：暂停 · 恢复 · 下线，以及上架时的全部设置）
- `POST /api/admin/credits` / `GET /api/admin/users` / `usage` / `models` CRUD — 管理（role=admin）
- `GET/POST /api/admin/departments` + `PATCH/DELETE /api/admin/departments/:id` — 部门管理（role=admin）
- `POST/GET /api/raise-requests` + `POST /api/admin/raise-requests/:id/{approve,reject}` — 加额申请 / 审批
- `GET /api/ops/runtime` / `users`，`POST /api/ops/credits` — 运营者视图（role=ops）
- `GET /api/config` — 前端动态配置（public_url 等）

## 7. 部署

- Docker：先 `export ATP_MASTER_KEY=$(openssl rand -hex 32)`（compose 缺失即报错退出），再 `docker compose up -d --build`；或镜像 `ghcr.io/argszero/aitokenpool:<tag>`（**镜像随版本 tag 发布**，latest 指向最新发版）
- 数据目录统一在 `ATP_DATA_DIR`（默认 `./data`：config.toml + db + logs/ + archive/）；`archive/` 是明细表的 JSONL 归档（保留窗口之外的明细行**只**在那里），备份与迁移请带上整个目录
- 生产必设 `ATP_MASTER_KEY`（上游 key 加密）；首次启动自动创建初始管理员（随机密码打印在日志）

## 8. Roadmap（规划，未实现）

- **P2**：前端深化（chat-modal 流式接网关、SSE 续传、key 缓存）
- **P3**：公共版共享市场深化（撮合 / 信誉体系）
- **P4**：多地节点 / 地域路由 / PostgreSQL / Redis（当前为单机 SQLite，无外部依赖）

> 注：早期文档中提及的 React / Vite / Tauri 桌面端、PostgreSQL 均**未实现**——前端为原生 JS 静态页，数据库为 SQLite。
