# AITokenPool 架构设计

> v0.3（2026-08-18）· 落地 P0（A/B/C）与 P1 后端实现：axum 网关（OpenAI/Anthropic 双协议 + SSE 流式）、7 条路由规则落地、计量账本闭环、AES-256-GCM 上游 key 加密、点数规则（每日赠送/有效期/先赠后永扣减）、管理员充值 API；补齐 API 一览与数据库实表结构
> v0.2（2026-08-15）· 新增 4.2.1 路由与故障转移策略（随机初始选择 / 粘性 / 静默故障转移 / 3 次切换上限 / 5 秒健康冷却 / 健康优先）
> v0.1（2026-08-13）· 基于 openlocalrouter 演进

## 1. 定位

开源的 AI Token 共享平台，双模式：
- **企业版**：私有部署，公司 key 池 + 员工点数配额（配额凭证，单向）
- **公共版**：共享市场，用户分享闲置 key 赚点数、消费别人 key（交换媒介，双向）

一套核心平台，两种部署。

## 2. 核心定论（多轮讨论结论）

### 2.1 中心化架构（方案 A）
- **平台托管 key + 平台执行调用**——平台是唯一可信执行者
- 计量可信（平台精确记账）、响应真实（平台直连上游）、无篡改作弊空间
- 分享者一次性上传 key，零负担（可离线）
- **方案 B（边缘代理）否决**：key 在分享者机器 → 篡改作弊无解、伪造响应无解

### 2.2 中心化的工程问题（要解决）
- IP 封禁 → 多地节点部署，某节点被封切流量
- 地域限制（国内 IP 访问 ChatGPT）→ 地域匹配路由（海外 key → 海外节点）
- key 安全 → 加密存储、最小权限、定期轮换、审计
- 平台信任 → 开源代码 + 可审计 + 明确隐私政策

## 3. 技术栈

- **Rust**（宿主有 openlocalrouter 积累）
- axum + hyper + tokio（高吞吐 HTTP 网关/流式转发）
- SQLite（本地/单机）→ PostgreSQL（公共版/多节点）
- Redis（缓存/限流/会话，公共版）
- reqwest + rustls（上游调用 + TLS）
- argon2 + sha2（key 哈希/加密）
- 前端：Web 管理台（React/Vite）+ 可选 Tauri 桌面端（复用 openlocalrouter）

## 4. 模块架构

```
┌──────────────────────────────────────────────────────┐
│                    平台                              │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌──────────┐   │
│  │ 网关层   │ │ 账本层   │ │ 市场层   │ │ 管理台    │   │
│  │ (axum)  │ │ (点数)   │ │ (共享)   │ │ (Web)    │   │
│  └────┬────┘ └────┬────┘ └────┬────┘ └──────────┘   │
│  ┌────▼─────┐ ┌───▼────┐ ┌───▼────┐                 │
│  │ Key 池    │ │ 计量引擎│ │ 撮合路由│                 │
│  │ (加密托管)│ │ (token) │ │ (定价)  │                 │
│  └────┬─────┘ └───┬────┘ └───┬────┘                 │
│  ┌────▼───────────▼──────────▼────┐                 │
│  │        数据库                   │                 │
│  └────────────────────────────────┘                 │
└──────────────────────────────────────────────────────┘
```

### 4.1 网关层（P0-B/C 已实现，Rust axum 自研）
- 统一 API 端点：**OpenAI Chat Completions**（`POST /v1/chat/completions`）+ **Anthropic Messages**（`POST /anthropic/v1/messages`），覆盖国内主流 Coding Plan 与下游工具（Cursor / Claude Code / Cline / OpenCode）
- **流式转发（SSE）**：`stream:true` 分支逐块透传（`text/event-stream`），OpenAI 流尾 usage / Anthropic `message_start`+`message_delta` 计入账本；客户端断开不入账（finalize 随 body drop）
- **余额预检**：上游调用前检查**可用余额**（赠送 + 永久，P1 口径），≤ 0 → 402「余额不足」
- 多 Provider 路由、故障转移（见 4.2.1，`src/router.rs` 落地：粘性 / 健康冷却 / 3 次切换上限）
- **API 一览**（`src/routes/mod.rs`）：
  - `GET /healthz` — 健康检查（返回版本号）
  - `POST /api/auth/login` — 邮箱 + argon2 口令 → 分发 key（`atk_live_` 前缀，get-or-create）
  - `POST/GET /api/api-keys` — 分发 key 生成 / 脱敏列表（Bearer 认证）
  - `POST /v1/chat/completions` / `POST /anthropic/v1/messages` — 网关（非流式 + SSE）
  - `GET /api/models` — 模型市场（含可用性/价格）
  - `POST/GET /api/sharings` + `PATCH /api/sharings/:id` — key 上架 / 列表（脱敏）/ 暂停·下线
  - `GET /api/wallet` / `GET /api/transactions?type=` / `GET /api/dashboard` — 钱包（双余额）/ 交易分页 / 仪表盘聚合
  - `POST /api/admin/credits` / `GET /api/admin/users` / `GET /api/admin/usage` — 管理员充值 / 成员列表 / 用量报表（role=admin）

### 4.2 Key 池
- 上游 key **AES-256-GCM 加密存储**（P0-C 实现，`src/crypto.rs`）：密文格式 `v1:<nonce>:<cipher>`；主密钥来源 env `ATP_MASTER_KEY` → config `server.master_key`（hex 32 字节）→ 缺省 dev 随机密钥并告警（重启后旧密文不可解）；启动时 `migrate_key_encryption` 自动迁移历史明文
- 企业：管理员配 key → 分发子 key 给员工
- 公共：分享者上传 key → 进共享池
- key 状态：可用/失效/限额/撤销；转发前解密，解密失败判 key 不可用
- **管理入口（v1.11 定案）**：无独立 Key 池管理界面——管理员与普通用户一样通过**「上架」**（共享管理页：选厂商 → Plan → 模型、填 key、声明额度、可用时间段）配置上游 key；共享列表即 key 池视图（可暂停 / 删除）。「Key 池」保留为数据层概念（平台持有的上游 key 集合），UI 侧不再单独暴露。

#### 4.2.1 路由与故障转移策略（v0.2，宿主 2026-08-14 定案）

当消费者请求某模型 M 时，网关从多个分享者提供的该模型 key 中选择（公共版共享池；企业版 key 池同规则）。定案规则共 7 条：

1. **初始选择（随机）**：用户首次请求模型 M 时，从 M 的**健康** key 池中**随机**选择一个（如 B1 或 B2）；
2. **粘性（Sticky）**：选定后，该用户**后续请求都复用这个 key**，直到它不可用；
3. **不可用判定**：key 下架（分享者删除 / 暂停）、key 报错（上游错误 / 鉴权失败 / 额度用尽）等；
4. **故障转移（静默）**：当前 key 不可用时，**静默**选择下一个健康 key（用户无感，不打断请求）；
5. **切换上限**：**每次路由最多允许 3 次切换**；3 次都失败 → 报错返回（如「该模型暂无可用 key」）；
6. **健康标记**：任何**在用的 key 报错 → 标记为「非健康」5 秒**（5 秒内不参与选择）；
7. **健康优先**：选择 key 时**优先选择当前健康**的 key（非健康 key 排除，5 秒冷却后可重新进入候选）。

**示例流程**：A 请求模型 M → 随机选 B1（健康）→ A 的后续请求粘性复用 B1 → B1 报错（额度用尽）→ 静默切换 B2（第 1 次）→ B2 也报错 → 切换 B3（第 2 次）→ 成功返回；若 B1/B2/B3 均失败（第 3 次也失败）→ 返回「该模型暂无可用 key」。B1 报错后标记非健康 5 秒，冷却后可重新参与选择。

### 4.3 计量引擎（P0-B 已实现，`src/billing.rs`）
- 每次调用记录：用户、模型、输入/输出 token、成本（`usage_records`）
- **点数计算**：`成本 = 输入token×input_per_m/1e6 + 输出token×output_per_m/1e6`（models 表单价，config.toml `[[models]]` 唯一真源）→ 折算到锚定货币（USD/CNY，`CNY_PER_USD=7.2`）→ × `points_per_unit`（默认 1000 点/USD）
- **入账（事务性）**：`settle` 在成功响应后调用——扣消费者 → 加分享者 90% → 两条 transactions（consume/earn）→ usage_records → keys.used；任一步失败整体回滚（失败不入账）

### 4.4 账本层（P1 点数规则已落地）
- **点数账户拆分**：`quotas.balance` = 永久点数（分享收益 / 管理员充值）；`quotas.gift_balance` = 当前有效赠送点数总额
- **新人每日赠送**（`src/gift.rs`）：注册（users.created_at）起**连续 10 天**每天 1 点，**当日有效**（expires_at = 当天 23:59:59）；懒加载 `ensure_daily_gift`（wallet/dashboard/网关预检前）补发当日点数；**惰性过期清理**——查询时把过期 active 记录标 expired 并从 gift_balance 扣减（无需定时任务），余额自愈对齐
- **扣减顺序**：消费**先扣最早到期的赠送点数**（gift_grants 按 expires_at ASC 逐条扣，标 used），不足再扣永久 balance；可用余额 = gift + permanent（网关预检与 settle 同口径）
- **分享收益**：分享者得 **90%**（平台抽成 10%，`SHARE_RATIO=0.9`），永久有效
- **管理员充值**：`POST /api/admin/credits` → 永久 balance 增加 + 写 transactions（type=topup，counterpart=管理员）
- 交易流水（可对账）：`transactions` 全量记录 consume / earn / topup

### 4.5 市场层（公共版）
- 分享者上架闲置 key（声明额度/价格）
- 消费者按点数购买使用
- 撮合/路由（价格/可用性/地域）
- 信誉体系（滥用惩罚、成功率）

### 4.6 管理台
- 企业：key 管理、员工点数、用量报表
- 公共：市场浏览、钱包（点数余额）、交易记录
- **管理员 API（P1 已实现，`src/routes/admin.rs`）**：角色经 `AuthUser.role` 判定（Bearer key 关联 users.role），非 admin → 403；**v0.6.0 起生产库不预置任何种子账号**（首次部署 = 干净空库，测试用 demo/admin/ops 账号仅存在于 `#[cfg(test)]` 的 `seed_test_users`）
- role=ops 端点（平台运营者）留 P2，暂与 admin 合并权限位

## 5. 数据库设计（实表，`src/db.rs` 迁移 v3）

```
users        — 用户（email/password_hash[argon2]/name/role/created_at）
keys         — 上游 key（provider/plan/model/状态/属主/加密密文/额度/已用/可用时间段/备注）
api_keys     — 分发 key（绑定用户、atk_live_ 前缀、状态、last_used）
models       — 模型价格（provider/model/currency/input_per_m/output_per_m）
quotas       — 点数账户（balance 永久 + gift_balance 有效赠送）
gift_grants  — 赠送明细（amount/granted_at/expires_at/status: active|used|expired）
transactions — 交易（counterpart/key_id/model/tokens/pts/type: consume|earn|topup/status/time）
usage_records— 调用明细（api_key_id/key_id/model/tokens/cost/time）
schema_version— 迁移版本（SCHEMA_VERSION=3，幂等迁移：ensure_column 补列 + CREATE TABLE IF NOT EXISTS）
```

## 6. 部署

- 企业版：Docker 单机（网关+账本+管理台），内网
- 公共版：云部署 + 多地节点（地域路由）+ 前端市场
- 配置：`config/config.toml`（server addr/db_path/master_key、points 锚定货币与粒度、providers/plans 端点、价格覆盖）；`cargo run -- --config <path>`

## 7. 路线

1. ~~**P0**：从 openlocalrouter 复用核心~~ → **已完成（v0.1.0→v0.2.2，PR #71–#74）**：Rust axum 自研骨架 + 认证 + API Key + 网关双协议 + 路由故障转移 + 计量账本 + SSE 流式 + key 加密 + 共享/钱包/交易 API
2. ~~**P1**：点数/账本系统~~ → **已完成（v0.3.0，PR #75）**：每日赠送/有效期/先赠后永扣减/管理员充值
3. **P2**：Web 管理台（UI 原型已 v1.20，前端静态页先行走在前面；后端角色 ops 端点）
4. **P3**：公共版共享市场深化（上架/撮合/结算）
5. **P4**：多地节点/地域路由

## 8. 竞品（2026-08-13 调研）

| 项目 | 模式 | 架构 | 结算 | 状态 |
|---|---|---|---|---|
| one-api (36K⭐) | B2C 分发 | 中心化 | 卡密/订阅 | 成熟 |
| new-api (45K⭐) | B2C 分发 | 中心化 | 卡密/订阅 | 成熟 |
| coai (9.3K⭐) | B2C 多租户 | 中心化 | 计费/卡密 | 成熟 |
| **Asale** | **C2C 共享** | **边缘代理(B)** | **USDT** | alpha/用户少 |
| **AITokenPool** | **企业+公共** | **中心化(A)** | **点数** | 构想→立项 |

差异化：中心化（无正文暴露风险，Asale 有）+ 企业版先行（Asale 无）+ 点数（无币合规简单）。
