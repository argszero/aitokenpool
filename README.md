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

后端 + 前端已全部实现，当前版本 **v0.6.0**（102/102 测试全绿）：

- ✅ **P0-A（v0.2.0）**：后端骨架 + TOML 配置（`Config::validate`：points_per_unit>0 / plan→provider 存在 / endpoints≥1 / protocol 枚举）+ SQLite 数据层（幂等迁移，**生产空库不种任何种子数据**）+ 认证（argon2 + Bearer API Key）+ API Key 端点
- ✅ **P0-B（v0.2.1）**：网关转发（OpenAI Chat Completions + Anthropic Messages）+ 路由故障转移（粘性 / 健康冷却 / 3 次切换上限）+ 计量账本（点数计算、90/10 分成、事务性 settle）
- ✅ **P0-C（v0.2.2）**：SSE 流式转发（流尾 usage 入账、断连不入账）+ 上游 key AES-256-GCM 加密 + 共享 / 钱包 / 交易 API
- ✅ **P1（v0.3.0）**：点数规则细化——新人每日赠送（10 天窗口·当日有效）+ 先扣最早到期赠送再扣永久 + 管理员充值 API（role=admin）
- ✅ **P2-A（v0.3.1）**：前端对接——后端静态托管 `ui/` + API 客户端层 + 登录/会话对接（按 role 显隐管理视图）
- ✅ **P2-B（v0.3.2）**：各页面数据对接真实 API——市场 / 共享 / 交易 / 仪表盘 / API Key 管理全接后端
- ✅ **P2-C（v0.3.3）**：部门/成员管理 + 加额审批 + 用量报表（users/models/departments 三组聚合）+ 运营者视图（runtime/credits/users）
- ✅ **P3-A（v0.4.0）**：网关三协议互转——OpenAI Chat / OpenAI Responses / Anthropic 任一端点可调用只暴露其他协议的 plan（自动转换，同协议透传零损耗）+ 新增 `/v1/responses` 端点
- ✅ **P3-A 补充（v0.4.1）**：`GET /v1/models` OpenAI 兼容模型列表（认证可选，带 Bearer 附加 available_keys；`/models` 别名）
- ✅ **P3-B（v0.5.0）**：流式 SSE 跨协议转换——openai/anthropic/responses 任意协议的 `stream:true` 请求可转发到任意协议上游，响应流事件实时互转（openai delta ↔ anthropic content_block_delta ↔ responses output_text.delta，含工具调用/thinking 增量与流内 usage 计量；responses→anthropic 流式暂延后）
- ✅ **发布（v0.5.1）**：GitHub Actions Docker 发布——`main` push / `v*` tag 自动构建推送 GHCR 镜像 `ghcr.io/argszero/aitokenpool`（buildx + gha 缓存）
- ✅ **v0.5.2（bugfix）**：修复 3 个时间敏感测试——测试硬编码日期（2026-08-18）跨天后导致赠送过期断言失败，改为 SQLite 动态日期（`datetime('now')` / `strftime('%Y-%m-%d 23:59:59', 'now')`），测试不再随日期推移周期失败
- ✅ **v0.6.0**：**移除所有 demo 种子数据（rant 2026-08-19T10:41:03）**——首次部署 = 干净空库（只建表），不再预置 demo/admin/ops 假账号、假余额、占位 key；测试改用 `#[cfg(test)]` 专用 `seed_test_users` 辅助（生产构建不含）；UI 登录页/设置页移除演示账号预填与提示
- ✅ **v0.6.1**：**首次启动自动创建初始管理员（rant 2026-08-19T14:35:05）**——空库启动时创建 `admin@aitokenpool.local` + 随机 16 位密码（打印到启动日志，仅首次）+ quotas 账户（balance=0），幂等不重复；新增 `POST /api/auth/change-password` 改密端点（旧密码校验 + argon2 更新）；不再需要手工插库
- ✅ **v0.6.2**：**用户自助注册 + 邮箱验证（rant 2026-08-19T14:36:19 方案 B）**——`POST /api/auth/register` + `verify` + `resend-code`；6 位数字验证码（10 分钟有效、5 次错误失效、60 秒重发限频）；未验证邮箱不可登录（403）；登录页注册表单 + 验证码页（中英 i18n）；SMTP 发信（`[mail]` 配置，未配置时 dev 模式验证码打日志/响应）
- ✅ **v0.6.3**：**接入方式 URL 配置化（rant 2026-08-19T20:37:37）**——设置页「接入方式」端点不再硬编码域名：新增 `[server].public_url` 配置（缺省 `http://localhost:8080`）+ `GET /api/config` 下发；前端从配置拼接 `{public_url}/v1`、`{public_url}/anthropic`，取不到配置时回退同源 origin
- ✅ **v0.6.4**：**管理员模型信息 CRUD（rant 2026-08-19T20:40:29）**——models 表补 context_length / max_output / vision / cache_hit_input_per_m（迁移 v7，幂等）+ seed 从 models.example.json 写入；新增 `GET|POST /api/admin/models` + `PATCH|DELETE /api/admin/models/:id`（admin 权限，唯一冲突 409，删除后按 0 计费）；管理视图「模型管理」tab（搜索/新增/编辑/删除，行内表单 + 二次确认，中英 i18n）；`GET /api/models` 市场列表补新字段（读图/上下文真实值）

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

> **对外网关地址（`[server].public_url`）**（rant 2026-08-19T20:37:37）：设置页「接入方式」展示的
> OpenAI/Anthropic 兼容端点由它拼接（`{public_url}/v1`、`{public_url}/anthropic`）。
> 缺省 `http://localhost:8080`（本地 dev 正确）；**生产必须设为真实域名**（如 `https://pool.example.com`），
> 否则用户拿到的 Base URL 是错的。改配置重启后设置页 URL 自动更新。

### ② Docker 部署

```bash
docker compose up -d --build                       # 构建 + 启动
open http://localhost:8080/                        # 浏览器访问
```

> 已发布镜像：`docker pull ghcr.io/argszero/aitokenpool:latest`

> ⚠️ **生产必须设置 `ATP_MASTER_KEY`**（32 字节 hex 主密钥，用于加密上游 key）：
>
> ```bash
> export ATP_MASTER_KEY=$(openssl rand -hex 32)
> docker compose up -d
> ```
>
> 未设置时使用 dev 默认值，仅适合本地试用（重启后已加密的上游 key 不可解密）。数据持久化在 `./data/`（SQLite）。

### ③ 登录账号

首次启动（空库）**自动创建初始管理员账号**（rant 2026-08-19T14:35:05，开源项目惯例）：

1. 启动服务，查看**启动日志**中的初始管理员凭据（仅首次打印）：
   - 账号：`admin@aitokenpool.local`
   - 密码：随机生成的 16 位字母数字（标注「初始管理员密码，请立即修改」）
2. 用该账号登录（登录页邮箱 + 日志中的密码）
3. 登录后**立即修改密码**：`POST /api/auth/change-password`（旧密码 + 新密码，Bearer 认证）

> 初始管理员只创建一次（users 表为空时）；已有用户后重启不会重复创建。
> 若日志丢失，也可直接操作 SQLite 手动建号（备选方案）：
>
> ```bash
> sqlite3 data/aitokenpool.db \
>   "INSERT INTO users (email, password_hash, name, role) VALUES ('admin@example.com', '<argon2-hash>', '管理员', 'admin');
>    INSERT INTO quotas (user_id, balance) VALUES (1, 0);"
> ```

### ④ 注册账号（普通用户自助注册，v0.6.2）

除内置管理员外，普通用户**自助注册**（登录页底部「注册」）：

1. 点击登录页「注册」→ 填昵称（可选）/ 邮箱 / 密码（≥8 位）→ 提交
2. 邮箱收到 **6 位验证码**（10 分钟有效）→ 输入验证码完成激活
3. 激活后回登录页用新账号登录（首次进钱包自动获得每日赠送 1 点）

> **邮件服务配置**（生产必配）：`config/config.example.toml` 的 `[mail]` 段
> （smtp_host / smtp_port / smtp_user / smtp_password / from / from_name）。
> **未配置 SMTP 时为 dev 模式**：验证码直接打印到服务端日志（并随注册响应返回 `dev_code`），仅适合本地试用。

> 后续账号通过注册/邀请流程创建（当前版本未提供注册端点，可直接操作 SQLite）。

## API 端点（Bearer 认证）

- `GET /healthz` → `{"status":"ok","version":"0.6.4"}`
- `POST /api/auth/login` → `{api_key}`；`POST /api/auth/change-password`（改密）；`POST /api/auth/register|verify|resend-code`（注册+邮箱验证）；`GET /api/me` → `{id,email,name,role}`；`GET /api/config` → `{public_url}`（接入端点 base，rant 2026-08-19T20:37:37）
- `POST|GET /api/api-keys`（key 脱敏 `atk_live_****xxxx`）；`DELETE /api/api-keys/:id`（撤销）
- `POST /v1/chat/completions` / `POST /anthropic/v1/messages` / `POST /v1/responses`（网关，三协议互转，非流式 + 流式 SSE 跨协议转换）；`GET /v1/models`（OpenAI 兼容模型列表，认证可选）
- `GET /api/models`（模型市场，含 context_length / max_output / vision / cache_hit_input_per_m）
- `POST|GET /api/sharings` + `PATCH /api/sharings/:id`（上架 / 列表 / 暂停 / 恢复 / 删除）
- `GET /api/wallet` / `GET /api/transactions?type=` / `GET /api/dashboard`（钱包 / 交易 / 仪表盘）
- 管理员：`POST /api/admin/credits` / `GET /api/admin/users` + `PATCH /api/admin/users/:id` / `GET /api/admin/usage`
- 模型管理（rant 2026-08-19T20:40:29）：`GET|POST /api/admin/models`（列表 / 新增）+ `PATCH|DELETE /api/admin/models/:id`（更新 / 删除；删除后该 model 调用按 0 计费）
- 部门：`GET|POST /api/admin/departments` + `PATCH|DELETE /api/admin/departments/:id`
- 加额：`POST|GET /api/raise-requests` + `POST /api/admin/raise-requests/:id/approve|reject`
- 运营者：`GET /api/ops/runtime` / `POST /api/ops/credits` / `GET /api/ops/users`

## 环境变量

| 变量 | 说明 |
|------|------|
| `ATP_MASTER_KEY` | 上游 key 主密钥（hex 32 字节），优先级高于 config `[server].master_key`，生产必须设置 |

## License

MIT
