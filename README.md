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

🚧 项目初始化中（2026-08-13）

## License

MIT
