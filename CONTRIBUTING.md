# Contributing to AITokenPool

感谢你愿意参与 AITokenPool！这是一个 Rust 项目，请遵循以下约定提交贡献。

## 开发环境

- **Rust**：稳定版（`rust-version = "1.86"`，见 Cargo.toml）
- 构建与测试：
  ```bash
  cargo build
  cargo test
  cargo fmt --check
  ```

## 分支命名

- 功能：`feat/<描述>`（如 `feat/points-ledger`）
- 修复：`fix/<描述>`（如 `fix/stream-timeout`）
- 文档：`docs/<描述>`（如 `docs/pricing-matrix`）
- 重构：`refactor/<描述>`

## Commit Message

使用 Conventional Commits 格式：

```
<type>(<scope>): <描述>

<可选：说明为什么改、怎么改>
```

- `type`：`feat` / `fix` / `docs` / `refactor` / `test` / `chore` / `perf`
- `scope`：模块名（如 `gateway`、`ledger`、`config`、`db`），可选
- 示例：`feat(config): 支持 TOML 配置加载`

## 代码规范

- 运行 `cargo fmt`（rustfmt）保持格式统一
- 运行 `cargo clippy` 且无新增 warning
- 新功能必须配套单元测试（`#[cfg(test)]`）
- 涉及外部数据/配置的改动，同步更新 `config/config.example.toml` 与示例数据

## 测试要求

- 提交前必须 `cargo test` 全绿
- 流式转发、点数账本等核心逻辑必须有测试覆盖
- 不要提交会破坏既有测试的改动

## PR 规范

- 目标分支：`main`
- 使用 `.github/pull_request_template.md` 模板填写描述
- 一个 PR 只做一件事（单一职责）
- 保持改动小、易 review

## 其它

- 大改动（新模块、架构调整）请先开 Issue 讨论，再动手
- 提交代码即视为同意 MIT License 下分发

## 开发流程（Powered by EMRG）

本项目由 [EMRG](https://emrg.ai)（演化式多实例系统）驱动开发：

1. 需求/反馈以 **rant**（吐槽）形式提交到项目队列
2. EMRG 自动实现 → 本地测试全绿 → 提交 PR（引用 rant 时间戳）
3. 人工评审 → merge → 发版时打 tag（镜像随 tag 发布）

外部贡献者同样欢迎：开 Issue 或直接 PR，遵循上文分支 / commit / 测试规范即可。
