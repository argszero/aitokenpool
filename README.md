# AITokenPool — AI Token 共享池

> "共享单车"式通俗命名：把闲置的 AI Token 额度共享出去，需要时也能接入别人的。

AITokenPool 是一个开源的 **AI Token 共享平台**：企业版（内部 key 池 + 员工点数配额）+ 公共版（用户分享闲置 key 赚点数、消费别人 key）。

## 核心价值

- **企业版**：公司统一采购多家模型 plan，放 key 池，IT 给员工分配点数——员工一个入口用所有模型，成本可控。
- **公共版**：把闲置的订阅额度（Claude/ChatGPT/GLM/DeepSeek…）共享出去，别人用了你赚点数，再用点数消费别人的模型。

## 架构定论（详见 docs/architecture.md）

- **中心化（方案 A）**：平台托管 key + 平台执行调用——唯一可信执行者，计量可信、无篡改作弊空间。
- **技术栈**：Rust（axum + hyper + tokio + SQLite），基于 openlocalrouter 的既有积累演进。
- **双模式**：一套核心平台，企业部署 + 公共市场；**角色是权限差异，不是产品差异**——公共版/企业版功能集合相同，管理员/运营者拥有额外管理视图（成员点数 / 用量报表 / 部门 / 运营数据），普通用户没有。

## 状态

后端 + 前端已全部实现，当前版本 **v0.3.3**（70/70 测试全绿）：

- ✅ **P0-A（v0.2.0）**：后端骨架 + TOML 配置（`Config::validate`：points_per_unit>0 / plan→provider 存在 / endpoints≥1 / protocol 枚举）+ SQLite 数据层（幂等迁移 v4 + demo/admin/ops 种子）+ 认证（argon2 + Bearer API Key）+ API Key 端点
- ✅ **P0-B（v0.2.1）**：网关转发（OpenAI Chat Completions + Anthropic Messages）+ 路由故障转移（粘性 / 健康冷却 / 3 次切换上限）+ 计量账本（点数计算、90/10 分成、事务性 settle）
- ✅ **P0-C（v0.2.2）**：SSE 流式转发（流尾 usage 入账、断连不入账）+ 上游 key AES-256-GCM 加密 + 共享 / 钱包 / 交易 API
- ✅ **P1（v0.3.0）**：点数规则细化——新人每日赠送（10 天窗口·当日有效）+ 先扣最早到期赠送再扣永久 + 管理员充值 API（role=admin）
- ✅ **P2-A（v0.3.1）**：前端对接——后端静态托管 `ui/` + API 客户端层 + 登录/会话对接（按 role 显隐管理视图）
- ✅ **P2-B（v0.3.2）**：各页面数据对接真实 API——市场 / 共享 / 交易 / 仪表盘 / API Key 管理全接后端
- ✅ **P2-C（v0.3.3）**：部门/成员管理 + 加额审批 + 用量报表（users/models/departments 三组聚合）+ 运营者视图（runtime/credits/users）

`ui/` 已由纯静态原型升级为**对接真实 API**（登录、钱包、市场、共享、交易、设置、管理、运营全部真实数据），由后端 `ServeDir` 静态托管，无需单独部署前端。

## 快速上手

### ① 本地运行（cargo）

```bash
cp config/config.example.toml config/config.toml   # 首次
cargo run                                         # http://localhost:8080/
```

> ⚠️ **首次启动即应配置主密钥**（用于加密上游 key）：`export ATP_MASTER_KEY=$(openssl rand -hex 32)`
> （或取消 config/config.toml `[server].master_key` 注释）。未配置时使用随机 dev 密钥，
> **重启后已上架的 key 密文无法解密 → 全部 503**（rant 2026-08-18T16:14:21 Bug 3）。

### ② Docker 部署

```bash
docker compose up -d --build                       # 构建 + 启动
open http://localhost:8080/                        # 浏览器访问
```

> ⚠️ **生产必须设置 `ATP_MASTER_KEY`**（32 字节 hex 主密钥，用于加密上游 key）：
>
> ```bash
> export ATP_MASTER_KEY=$(openssl rand -hex 32)
> docker compose up -d
> ```
>
> 未设置时使用 dev 默认值，仅适合本地试用（重启后已加密的上游 key 不可解密）。数据持久化在 `./data/`（SQLite）。

### ③ 登录账号

| 角色 | 账号 | 密码 | 说明 |
|------|------|------|------|
| 普通用户 | `demo@aitokenpool.local` | `demo1234` | 市场/共享/钱包/设置 |
| 管理员 | `admin@aitokenpool.local` | `admin1234` | + 管理视图（成员/充值/用量/部门/加额审批） |
| 运营者 | `ops@aitokenpool.local` | `ops1234` | + 运营视图（运行概览/成员充值） |

## API 端点（Bearer 认证）

- `GET /healthz` → `{"status":"ok","version":"0.3.3"}`
- `POST /api/auth/login` → `{api_key}`；`GET /api/me` → `{id,email,name,role}`
- `POST|GET /api/api-keys`（key 脱敏 `atk_live_****xxxx`）；`DELETE /api/api-keys/:id`（撤销）
- `POST /v1/chat/completions` / `POST /anthropic/v1/messages`（网关，支持 SSE 流式）
- `GET /api/models`（模型市场）
- `POST|GET /api/sharings` + `PATCH /api/sharings/:id`（上架 / 列表 / 暂停 / 恢复 / 删除）
- `GET /api/wallet` / `GET /api/transactions?type=` / `GET /api/dashboard`（钱包 / 交易 / 仪表盘）
- 管理员：`POST /api/admin/credits` / `GET /api/admin/users` + `PATCH /api/admin/users/:id` / `GET /api/admin/usage`
- 部门：`GET|POST /api/admin/departments` + `PATCH|DELETE /api/admin/departments/:id`
- 加额：`POST|GET /api/raise-requests` + `POST /api/admin/raise-requests/:id/approve|reject`
- 运营者：`GET /api/ops/runtime` / `POST /api/ops/credits` / `GET /api/ops/users`

## 环境变量

| 变量 | 说明 |
|------|------|
| `ATP_MASTER_KEY` | 上游 key 主密钥（hex 32 字节），优先级高于 config `[server].master_key`，生产必须设置 |

## License

MIT
