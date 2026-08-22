# AITokenPool — AI Token 共享池

[English](README.en.md) | [简体中文](README.md)

> **不要让 token plan 白白浪费。**
> 订阅了 Claude / ChatGPT / GLM / DeepSeek 的额度用不完？共享出去赚点数，需要时也能用别人的。

AITokenPool 是一个开源的 **AI Token 共享平台 / 多模型网关**：把多家模型提供商的 API 额度汇聚成一个统一入口，按点数公平分配。企业版（内部 key 池 + 员工点数配额）与公共版（用户共享闲置 key 赚点数、消费他人 key）共用同一套核心平台。

## 特性

- **多协议网关互转** — OpenAI Chat / OpenAI Responses / Anthropic Messages 三协议双向转换，非流式 + SSE 流式全支持；客户端只需对接一个 OpenAI 兼容端点
- **透明计费** — 每次调用展示「输入 / 缓存命中 / 输出」三档 token 明细；内置 DeepSeek 官方 CNY 定价，支持高峰时段计价
- **点数体系** — 每日赠送、共享分成（90/10）、管理员加额、自助注册 + 邮箱验证
- **企业管控** — 部门管理、成员点数、用量报表、运营者视图，成本一目了然
- **安全** — 上游 Key AES-256-GCM 加密存储、argon2 密码哈希、邮箱验证、管理员角色隔离
- **轻量易部署** — 单个 Rust 二进制 + SQLite + Docker 一键启动，无外部服务依赖
- **自带中英双语前端** — 完整 Web 界面（市场 / 钱包 / 交易 / 共享 / 管理），无需单独部署

## 适用场景

| 场景 | 说明 |
|------|------|
| 企业 / 团队 | 统一采购多家模型 Plan 放入 Key 池，按点数分配给员工——一个入口用所有模型，成本可控 |
| 个人订阅党 | 闲置的订阅额度共享出去赚点数，需要时消费别人的模型，订阅费不浪费 |
| 开发者 / 极客 | 一个 OpenAI 兼容 API 调全部模型，自带仪表盘、报表与权限体系，开箱即用 |

## 快速上手

### Docker 部署（推荐）

```bash
docker compose up -d --build
# 或直接使用已发布镜像（镜像随版本 tag 发布，latest 指向最新发版）
docker pull ghcr.io/argszero/aitokenpool:latest
docker run -p 8080:8080 -v "$PWD/atp-data:/data" \
  -e ATP_MASTER_KEY=$(openssl rand -hex 32) \
  ghcr.io/argszero/aitokenpool:latest
```

### 源码运行

```bash
cargo run   # 首次自动创建 ./data/ 与配置，打开 http://localhost:8080/
```

### 首次启动

- 自动创建统一数据目录（`<data>/`：`config.toml` + `aitokenpool.db` + `logs/`）
- 自动创建**初始管理员** `admin@aitokenpool.local`，随机 16 位密码打印在启动日志中（仅首次），登录后请立即修改密码
- 普通用户可在登录页**自助注册**（邮箱验证码激活，未验证邮箱不可登录）
- ⚠️ **生产环境必须设置主密钥** `ATP_MASTER_KEY`（`openssl rand -hex 32`）——用于加密上游 Key；未设置时使用随机 dev 密钥，重启后已上架的 Key 将无法解密

完整配置说明见 `config/config.example.toml` 注释。

### 接入方式

OpenAI 兼容网关端点：`POST /v1/chat/completions`、`POST /v1/responses`、`POST /anthropic/v1/messages`、`GET /v1/models`。接入地址在设置页展示，由 `[server].public_url` 配置（生产请设为真实域名）。API 全清单见 [docs/architecture.md](docs/architecture.md)。

## 文档

| 文档 | 说明 |
|------|------|
| [docs/architecture.md](docs/architecture.md) | 架构设计、API 一览、数据库结构 |
| [docs/user-stories.md](docs/user-stories.md) | 用户故事与场景 |
| [docs/plan-api-matrix.md](docs/plan-api-matrix.md) | 套餐与 API 矩阵 |
| [CHANGELOG.md](CHANGELOG.md) | 版本历史 |
| [CONTRIBUTING.md](CONTRIBUTING.md) | 贡献指南 |

## Powered by EMRG

本项目由 [EMRG](https://emrg.ai)（演化式多实例系统）驱动开发——需求以 rant 形式提交，由 EMRG 自动实现、测试并提交 PR，人工评审后合入。

## License

[MIT](LICENSE)
