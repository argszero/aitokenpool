---
id: "d4f8c3a9"
event_at: "2026-08-13T10:18:33Z"
created_at: "2026-08-13T10:18:33Z"
updated_at: "2026-08-13T10:18:33Z"
type: "project"
scope: "project"
status: "active"
---

# GitHub 仓库已创建并推送

用户确认「可以在 github 创建项目了」后，通过 `gh` CLI 在 GitHub 创建了 aitokenpool 仓库并推送完成。

- **仓库**：https://github.com/argszero/aitokenpool （Public）
- **描述**：AI Token 共享池 — 企业 key 池 + 公共共享市场（Rust）
- **提交**：
  - `313795e` init（初始骨架）
  - `a5c5bba` docs：模型定价调研 + 配置/数据种子结构
- **已推送**：`main` 分支已同步到 origin
- **本次提交内容**：`docs/plan-api-matrix.md`、`config/config.example.toml`、`data/models.example.json`、`.gitignore`
- **注意**：`.gitignore` 已排除 `.emrg/`，避免 EMRG 内部会话数据被提交进项目仓库。

后续如需走 EMRG 自演化流程（写 rant 自动改代码提 PR），该仓库已具备条件。
