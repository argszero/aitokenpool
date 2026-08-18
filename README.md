# AITokenPool — AI Token 共享池

> "共享单车"式通俗命名：把闲置的 AI Token 额度共享出去，需要时也能接入别人的。

AITokenPool 是一个开源的 **AI Token 共享平台**：企业版（内部 key 池 + 员工点数配额）+ 公共版（用户分享闲置 key 赚点数、消费别人 key）。

## 核心价值

- **企业版**：公司统一采购多家模型 plan，放 key 池，IT 给员工分配点数——员工一个入口用所有模型，成本可控。
- **公共版**：把闲置的订阅额度（Claude/ChatGPT/GLM/DeepSeek…）共享出去，别人用了你赚点数，再用点数消费别人的模型。

## 架构定论（详见 docs/architecture.md）

- **中心化（方案 A）**：平台托管 key + 平台执行调用——唯一可信执行者，计量可信、无篡改作弊空间。
- **技术栈**：Rust（axum + hyper + tokio + SQLite/PostgreSQL），基于 openlocalrouter 的既有积累演进。
- **双模式**：一套核心平台，企业部署 + 公共市场。

## 状态

后端已实现 **P0（A/B/C）+ P1**，当前版本 **v0.3.0**（61/61 测试全绿）：

- ✅ **P0-A（v0.2.0）**：后端骨架 + TOML 配置（`Config::validate`：points_per_unit>0 / plan→provider 存在 / endpoints≥1 / protocol 枚举）+ SQLite 数据层（幂等迁移 v3 + demo/admin 种子）+ 认证（argon2 + Bearer API Key）+ API Key 端点
- ✅ **P0-B（v0.2.1）**：网关转发（OpenAI Chat Completions + Anthropic Messages）+ 路由故障转移（粘性 / 健康冷却 / 3 次切换上限）+ 计量账本（点数计算、90/10 分成、事务性 settle）
- ✅ **P0-C（v0.2.2）**：SSE 流式转发（流尾 usage 入账、断连不入账）+ 上游 key AES-256-GCM 加密 + 共享 / 钱包 / 交易 API
- ✅ **P1（v0.3.0）**：点数规则细化——新人每日赠送（10 天窗口·当日有效）+ 先扣最早到期赠送再扣永久 + 管理员充值 API（role=admin）

`cargo run` 后可用端点（Bearer 认证；demo@aitokenpool.local / demo1234，admin@aitokenpool.local / admin1234）：
- `GET /healthz` → `{"status":"ok","version":"0.3.0"}`
- `POST /api/auth/login` → `{api_key}`；`POST|GET /api/api-keys`（key 脱敏 `atk_live_****xxxx`）
- `POST /v1/chat/completions` / `POST /anthropic/v1/messages`（网关，支持 SSE 流式）
- `GET /api/models`（模型市场）
- `POST|GET /api/sharings` + `PATCH /api/sharings/:id`（key 上架 / 列表 / 暂停下线）
- `GET /api/wallet` / `GET /api/transactions?type=` / `GET /api/dashboard`（钱包 / 交易 / 仪表盘）
- `POST /api/admin/credits` / `GET /api/admin/users` / `GET /api/admin/usage`（管理员充值 / 成员 / 用量）

UI 原型 v1.20（`ui/` 静态页 + mock 数据）；架构细节见 docs/architecture.md v0.3。

## License

MIT
