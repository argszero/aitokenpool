---
id: "c7a2f9d1"
event_at: "2026-08-13T09:12:52Z"
created_at: "2026-08-13T09:12:52Z"
updated_at: "2026-08-13T09:14:08Z"
type: "decision"
scope: "session"
status: "active"
---

# 配置文件格式选型：TOML + JSON 分层（已实施）

用户询问模型定价数据用 JSON / YAML / TOML 哪个更合适。助手给出推荐方案，用户未反对，随后已按此方案落地两个种子文件（`config/config.example.toml` + `data/models.example.json`）。具体结构见 project 记忆 `seed-file-structure`。

## 结论：推荐 TOML，但按用途分层

- **配置类**（providers、协议映射、base_url、点数锚定规则）→ **TOML**
  - 理由：Rust 项目；`toml` crate 官方级成熟（Cargo 本身就用 TOML）；支持注释，适合人手工维护的种子数据；类型明确（原生整数/浮点/字符串/数组/布尔）。
- **纯大表**（几百个模型的价格清单，量大且随厂商调价频繁更新）→ **JSON**
  - 理由：机器从 OpenRouter / litellm 同步生成；TOML 里只放「官方价覆盖 + 增量」。
  - TOML 的弱点是超长数组重复字段名会啰嗦，大表不适合手写 TOML。

## 排除 YAML 的硬理由

- `serde_yaml` 已停止维护（archived），Rust 生态长期维护性有风险。
- YAML 缩进敏感，tab/空格易错（emrg 之前踩过坑）；隐式类型可能让价格（如 `0.435`）解析意外。
- JSON 无注释，不适合需要加注释说明（如「缓存命中价」「plan 专属 key 前缀」）的手工种子数据。
