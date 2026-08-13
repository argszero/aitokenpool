---
id: "626f56c4"
event_at: "2026-08-13T07:47:59Z"
created_at: "2026-08-13T07:47:59Z"
updated_at: "2026-08-13T07:47:59Z"
type: "project"
scope: "project"
status: "active"
---

# emrg-main 主开发会话摘要（s_260727_1103_866d）

用户要求阅读的 emrg 主开发会话（2026-07-27 ~ 08-13，6924 条消息，compact 15 次）。该会话的收尾工作创建了当前工作目录下的 **aitokenpool** 项目。会话内容主线：

- **7/27 基础功能**：排查 `/trigger` 命令不可见、VSCode terminal 闲置输入问题；项目/任务注册进 projects.yml、tasks.yml 的机制。
- **7/28 paper-task 演化**：定时任务自动演化 `paper` 项目（顶会级 AI 论文）；改进 `paper_prompt.md`（实验优先、执行前后 review、browser harness 查证、反思总结常态化）；terminal 标题显示 session/项目名/计时；完善 `open_source_prompt.md`（PR 规范、去 LGTM 式评论）。
- **7/29 架构重构**：自研 yaml 解析改 `yaml.safe_load`（修 TypeError 崩溃）；`_build_system_prompt` 改为 Jinja2 模板（system.j2，每次渲染存 system.md）；rants.jsonl 排序/状态字段/去序号；会话删除（/delete 列表、current 标识）与图片粘贴支持（`[Image #1]` 方案）。
- **7/30–31 图片与任务系统**：图片粘贴 bug 修复、模型 vision 开关、`/image` 指令、TUI 占位符替换；TASK_TEMPLATES 改 Jinja2；open-source 任务支持 GitLab 等多平台 + browser harness 兜底。

参考源会话：`/Users/argszero/.emrg/.emrg/sessions/s_260727_1103_866d`
