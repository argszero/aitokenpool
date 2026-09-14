# AITokenPool UI 原型（Static HTML Prototype）

纯静态 HTML + CSS + JS 原型，无框架、无构建工具、无外部 CDN 依赖。
浏览器直接打开即可浏览与演示。

## 产品模型（重要）

**一套产品，两种部署场景，角色区分**：

- **公共版**：部署在公网，任何人都能注册、分享 key、赚点数、消费别人共享的 key；
- **企业版**：部署在企业内网，管理员（IT）把采购的 key 放进池子、给成员分配点数；
- 两者的**功能集合完全相同**（市场 / 共享 / 钱包 / 交易 / 设置），不存在"企业版专属功能"；
- 差异只在**谁会主动分享**（公共版人人可分享；企业版只有管理员有动力采购/分享 key）；
- **角色是权限差异，不是产品差异**：管理员额外拥有"管理视图"（成员点数 / 用量报表 / 组织管理 / 平台运营），普通用户没有；员工就是普通用户，用同一套界面。

## UI 演化约束：尽量少用弹窗（v1.13 定案）

- **优先行内（inline）交互**：表单 / 编辑 / 确认优先行内展开（行内展开表单、可折叠面板、就地编辑），不用弹窗；
- 弹窗仅限真正需要聚焦 / 阻塞的场景（如删除确认）；**禁止弹窗嵌套**；
- 替代形态：行内二次确认（按钮变「确认？」）、toast + 撤销、行内下拉 / 独立页；
- **存量 modal 改造优先级**：
  - **P0 已行内**：上架表单（`#share-form-card` 行内卡片）、部门添加/编辑（`#dept-form-card` 行内卡片，v1.13 由弹层改造）
  - **P1 已行内（v1.14）**：充值（`#topup-card` 行内卡片）、申请加额（`#raise-card` 行内卡片）——钱包页点按钮原地展开、两卡互斥
  - **P2**：模型消费聊天（chat-modal）→ 独立页 / 行内面板
- 本原则与 docs/user-stories.md §4「UI 约定」对齐（原文的 1.1 节，已随文档精简合并到此节）。

本原型据此实现：单一登录入口（角色由账号决定），一套导航（仪表盘 / 模型市场 / 共享管理 / 钱包 / 交易记录 / 设置 + 管理员角色视图）。

## 如何浏览

**方式一（推荐）**：双击 `index.html`，用浏览器打开即可。

**方式二**：本地起一个静态服务器（可选）：

```bash
python3 -m http.server 8000 --directory ui
# 然后访问 http://localhost:8000
```

## 页面清单

登录页：单一入口（邮箱登录占位），不做公共版/企业版二选一；提供「**游客浏览**」入口（US-1）。

**游客模式（US-1）**：未登录可点击「先逛逛市场」免登录进入模型市场——
- 游客可：浏览模型、搜索、厂商筛选、排序、查看点数价格；
- 游客不可：使用/消费模型（提示「请先登录」）、访问钱包/共享/交易/设置/管理视图（导航仅显示「市场」，其余页面点击提示需登录）；
- 登录后退出游客模式恢复完整导航；登出回到登录页（含游客浏览入口）。

同一套界面（角色视图）：

1. 仪表盘 Dashboard — 点数余额、本月用量、共享收益、**本月点数变化（近 1 月按类型汇总收支：赠送/过期/收益/消费/充值/提现 + 净变化，取代静态"点数来源"分组）**
2. 模型市场 Marketplace — 模型浏览、**搜索（输入防抖 ~150ms + 关键词 `<mark>` 高亮 + 清空 × 按钮，v1.18 D）**、厂商筛选、排序（按价格/上下文）；可用性标注「多 key · 自动故障转移」（该模型配置多个上游 key，见 `docs/architecture.md` §3 模块表 `router.rs` 行）；每行「使用 / 消费」入口 → 聊天 Mock 模拟调用（按模型输出参考价扣小数点数，如 -0.38 点，产生消费交易；余额不足时阻断并提示）
3. 共享管理 Sharing — 默认只显示统计 + 我的共享列表（key 脱敏展示，可暂停/恢复/重新上架/彻底删除；列表展示「厂商 · Plan / 模型 · 可用时间段」，如「智谱 · GLM Coding Plan / glm-5.2 · 周一~周五 09:00-18:00」，未设置显示「全天」）；点击"＋ 添加 / 上架新 key"展开上架表单（**三级联动：厂商 → Plan → 模型**——内置国内已知 Plan 清单（阿里云百炼 / 智谱 / 火山方舟 / Kimi / MiniMax / DeepSeek，每家含「API（按量）」= 按量计价的 key），选 Plan 后显示 Plan 类型（按量 / 订阅）；**可用时间段为结构化字段**：星期多选 chips + 起止时间（留空 = 全天不限），备注仅纯文本；须填 API Key，平台加密托管；分享者只填声明额度，单价由平台按模型定价自动计算并展示参考价），提交成功或取消后自动收起
4. 钱包 Wallet — 点数余额、**本月点数变化（近 1 月按类型汇总收支 + 净变化，与仪表盘一致）**、**充值入口（US-4：模拟流程——输入点数 → 余额增加 → topup 交易记录；文案注明"演示，真实支付后续接入"；提现仍 disabled）**、**申请加额（US-20：企业成员余额低时申请更多点数 → 提交后等待管理员审批，默认需审批）**；收支明细已去重，统一到【交易记录】（页内提供跳转提示）
5. 交易记录 Transactions — 消费/收益/充值/提现/赠送（gift）唯一明细入口，Tab 筛选 + MRT 风格表格（列排序/列筛选/分页，与 Tab 叠加生效）
6. 设置 Settings — 账户、**API Key 管理（独占整行全宽，表格字段不挤压；完整增删改查：生成带名字、改名、删除需确认「删除后该 key 立即失效」、按名字搜索（防抖+高亮+清空，v1.18 D）、一键复制完整 key——列表脱敏展示 atk_live_****xxxx，复制为完整值；file:// 下 clipboard API 受限时降级为选中+Ctrl/Cmd+C）**、通知、偏好
7. 管理视图 Admin（管理员 / 运营者角色专属）— 成员管理（改部门：下拉可选部门或"未分配"；任意金额充值，正整数校验；新注册成员默认未分配部门；**加额申请审批（US-20：待审批列表 → 批准=成员余额+申请点数 / 驳回；申请默认需审批）**）、用量报表（按模型/成员）、组织管理（部门列表 + 部门增删改查 + 每月点数分配，含部门汇总统计，未分配成员不计入任何部门；**部门搜索（防抖+高亮+清空，v1.18 D）**；**添加/编辑部门用行内展开表单（`#dept-form-card`，名称+月分配，替代 window.prompt）**，删除有确认；**无「组织设置」表单**——组织名称 / 默认成员配额 / 开关均移除，"关闭外部注册"为部署配置项不在 UI 中）、平台运营（运营者 = 宿主本人，职责最小化：① 运行概览——运行状态 / 用户数 / 共享 key 数 / 交易量 / 点数流入流出；② 用户充值——按用户名 / 邮箱**搜索（防抖+高亮+清空，v1.18 D）**定位，输入点数金额（可为小数）确认后余额增加永久有效点数并产生一条交易记录）。**无独立 Key 池管理**——管理员与普通用户一样通过共享管理页「上架 key」配置上游 key（共享列表即 key 池视图，可暂停/删除）

## 文件结构

```
ui/
├── index.html        # 入口（登录页 + 应用外壳 + 全部视图）
├── css/style.css     # 设计系统（深色主题 · 强调色 #4ecdc4 · 响应式预留 · v1.15 视觉美化）
├── js/data.js        # 内嵌数据（仅游客市场 MARKET + 上架表单兜底 MODELS/PLANS/PROVIDERS/PROVIDER_LABELS；登录态 mock 已清零 v1.22）
├── js/app.js         # 交互逻辑（导航、筛选、表单、分页、Toast；v1.15 内联 SVG 图标 + 空状态组件）
└── README.md         # 本文件
```

## 视觉约定（v1.15，rant 2026-08-17T15:50:05 A 视觉美化）

- 侧边栏导航图标用**统一内联 SVG**（线性风格、同尺寸、currentColor，禁用 emoji 散落）；
- 卡片 hover 阴影/边框轻提升；表格**斑马纹 + 行 hover 高亮**；
- 空状态统一 `empty-state` 组件（图标 + 文案 + 行动按钮），禁用裸文本占位；
- 状态徽章统一带**语义色点**（ok/warn/danger/dim），配色与 `badge.*` 类一致。

## 交互约定（v1.16，rant 2026-08-17T15:50:05 B 交互优化）

- **toast 分级**：`toast(msg, "success" | "error" | "info")`——成功 / 失败校验 / 信息提示不同边框与文字色；
- **按钮 loading**：提交类按钮用 `withLoading(btn, fn)`（转圈 + 禁用，模拟反馈后恢复）；
- **复制反馈**：API Key 复制后按钮短暂变「已复制 ✓」（1.2s 恢复）；降级路径显示「请 Ctrl+C」；
- **键盘可达**：行内表单 Enter 提交、Esc 关闭行内卡片、打开时自动聚焦焦点。

## 行内组件约定（v1.17，rant 2026-08-17T16:57:17 A 清除原生弹窗）

- **禁用原生弹窗**：`grep ui/` 不得出现原生 confirm/prompt 调用；确认走 `confirmInline(btn, onConfirm, text)`（按钮变「确认删除？」红色态 `.confirming`，3 秒无操作或 Esc 还原，再次点击执行），输入走 `inlineForm(cell, opts)`（行内展开 input + 确认/取消，Enter 确认 / Esc 取消，`opts.validate` 返回错误文案时 toast + 重新聚焦）；
- 已覆盖：key 删除、共享下架、部门删除（confirmInline）；API Key 新建/改名、运营者充值、成员充值（inlineForm）；新建 key 行内输入框在 `#ak-new-inline`（Enter/Esc 绑定）。

## 时间显示约定（v1.17，rant 2026-08-17T16:57:17 B 相对时间）

- **相对时间**：`timeAgo(s)` 支持 `MM-DD HH:mm`（默认今年）与 `YYYY-MM-DD[ HH:mm]`，输出 `刚刚 / N 分钟前 / N 小时前 / 昨天 / MM-DD`，非标准格式原样返回；`timeCell(s)` 输出带 `title`（完整绝对时间）的 `.timeago` 单元格，hover 显示；
- **统一使用**：交易列表（时间列）、加额申请列表、API Key 最近使用时间；数据新增 / 生成时间用 `nowTime()`（`MM-DD HH:mm`）写入即可自动相对化。
  ⚠️ **不含共享列表**：本行原写「共享列表（上架时间列）」，但该列**从未存在**（原型 `docs/prototype/aitokenpool-console.html:574` 与实现 `ui/index.html:338` 的 thead 均无时间列，`timeCell` 也只有交易/API Key 两个调用点）。后端 `GET /api/sharings` 自 C2015 起才返回 `created_at`，前端把它收进视图行的 `time` 字段（备用），但**未加列**——若要加列，需同时补 `share.col.time` 键与 8 列表头的 `colspan` 对齐。

## 表格数字列约定（v1.17，rant 2026-08-17T16:57:17 C 数字列对齐）

- **数字列**（价格 / 点数 / 已用 / 额度 / 余额 / 用量）统一 `<td class="num">` + 表头 `<th class="num">`：右对齐 + `--mono` 等宽 + `tabular-nums`，便于纵向扫读；时间列 / 状态列 / 文本列保持左对齐；
- 数据表格渲染器 `buildDataTable` 支持列配置 `align: "num"`（交易表 tokens / pts 已启用）；
- 金额一律 `D.fmt()`（整数千分位；小数保留 2 位），禁止裸数字拼接。

## 键盘可达性约定（v1.17，rant 2026-08-17T16:57:17 D 无障碍）

- **焦点环**：全局 `:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }`；输入类控件（`.input` / `.th-filter`）已有边框高亮，`outline: none` 不叠加；
- **全局快捷键**（`document` keydown，输入框内不触发、Cmd/Ctrl/Alt 组合不劫持）：
  - `/` → 聚焦市场搜索 `#mk-search`；
  - 数字 **1-8** → 切换侧边栏视图（键位 = `NAV_ORDER` 下标 +1，范围由该数组长度决定：仪表盘/市场/共享/钱包/交易/管理/运营/设置；游客模式由 `switchView` 拦截提示登录）；
  - Esc → 关闭行内新建 Key（`#ak-new-inline`）；
- **导航提示**：nav-item 补 `title`（"快捷键 N · 名称"）+ 右侧 `.nav-key` 键位角标（管理视图带「管理员」tag 时省略角标）。

## 行内校验错误约定（v1.17，rant 2026-08-17T16:57:17 E 表单校验）

- **组件**：`setFieldError(input, msg)` 给输入框加 `.input-error`（红边框）并在其后插入 `.field-error`（红字小号行内文案），输入事件自动清除；`clearFieldError(input)` 手动清除；打开表单时重置；
- **覆盖**：充值自定义金额、申请加额（点数/原因）、部门表单（名称/配额/重名）、共享上架表单（API Key/厂商·Plan·模型·额度）；提交校验失败聚焦首个错误字段，不依赖 toast。

## 过渡动画约定（v1.17，rant 2026-08-17T16:57:17 F 视图过渡）

- **视图切换**：`.view:not(.hidden)` 播放 `viewIn`（opacity 0→1 + translateY(6px)→0，150ms ease-out）；每次从隐藏变为可见自动重放；
- **表格更新**：`.table tbody` 播放 `tbodyIn`（opacity 0→1，150ms）；整表重建（buildDataTable / 加额申请列表）新 tbody 节点自动播放；静态 tbody（market/sharing/api-keys/emp/dept/ops）在 innerHTML 更新后用 `pulseTbody(el)` 重启动画（style.animation=none → 强制 reflow → 还原）。

## 一致性约定（v1.17，rant 2026-08-17T16:57:17 G 细节一致性）

- **金额**：所有数字/金额一律 `D.fmt()`（整数千分位；小数 2 位），侧边栏余额 / 统计卡 / 表格 / 钱包格式一致；已用/额度统一「已用 X / 额度 Y」；
- **按钮**：表格操作按钮统一 `padding:4px 10px;font-size:12px`，主操作用默认 `.btn` 尺寸；
- **行内编辑校验**：`inlineForm` 的 `opts.validate` 失败走 `setFieldError`（红边框+行内文案），与 E 项表单校验一致，不使用 toast 承载错误；
- 子文本（邮箱/模型等）统一 `font-size:12px`；单价措辞统一「点/1M」。

## 数据可视化约定（v1.18，rant 2026-08-17T18:06:09 A 仪表盘图表）

- **零外部依赖**：手写 SVG（`sparkline(values, opts)`），`--accent` / `--ok` / `--danger` 等现有 CSS 变量着色；每点带 `<title>`（hover 显示日期 + 数值）；
- **使用**：`lastDayLabels(n)` 生成近 n 天 MM-DD 标签；`dailySeries(days, filter)` 按天聚合交易点数；仪表盘「本月点数变化」画近 7 日净变化折线（渐变填充），「我的共享」画收益累计趋势（`--ok` 色，无数据保留空状态）；
- 样式 `.sparkline`（宽 100% / 高 38px）。

## 主题约定（v1.18，rant 2026-08-17T18:06:09 B 亮色主题）

- **双主题**：`:root`（深色默认）与 `:root[data-theme="light"]` 重定义全套颜色变量；**禁止硬编码颜色**——表格行边框/斑马纹、卡片 hover 阴影、确认态、spinner、遮罩均用语义变量（`--table-row-border` / `--table-stripe` / `--card-hover-border` / `--card-shadow` / `--danger-soft` / `--danger-text` / `--spin-track` / `--overlay`）；
- **切换**：侧边栏 `#theme-toggle`（日/月 SVG 图标按主题显隐）→ 设 `document.documentElement.dataset.theme`，`localStorage["atp-theme"]` 记忆；首次加载无记忆时尊重 `prefers-color-scheme`。

## 移动端表格卡片化约定（v1.18，rant 2026-08-17T18:06:09 C）

- **@media (max-width: 560px)**：`.table thead` 隐藏，行 → 卡片（border + radius + 间距），`td` 变 `label: value` 两栏（`td::before { content: attr(data-label) }`），操作按钮整行宽换行；
- **所有表格 td 必须带 `data-label`**（市场/共享/交易/设置 API Key/成员/部门/运营者/加额申请）；`buildDataTable` 动态列自动用 `col.title` 作 label。

## 搜索增强约定（v1.18，rant 2026-08-17T18:06:09 D）

- **统一接线 `wireSearch(input, render)`**：所有搜索框（`#mk-search` / `#ak-search` / `#od-search` / `#emp-search` / `#ops-search` / `#model-search`）走**~150ms 输入防抖**渲染（连续输入只渲染一次，避免整表重绘闪烁）+ **「清空 ×」按钮**（有内容时显示；点击清空立即重绘并聚焦，不走防抖）；HTML 结构为 `.search` 包裹 `<input>` + `<button class="search-clear">`（v1.22 全站 UI 重设计对齐原型类名，原 `.search-box` 已更名）；
- **关键词高亮用 `hl(text, rawQ)`**：先 `esc()` 转义再对查询词**大小写不敏感**包 `<mark>`（正则转义用户输入，`& < >` 等字符与转义正文同构不错位）；无关键词返回转义原样（重置后自动清除）；
- **程序化清空用 `resetSearch(input)`**（值 + × 按钮态同步，不触发渲染；调用方随后自行重绘）——空状态「清除搜索 / 清除筛选」按钮已统一走此路径；
- `mark` 样式：`--accent-soft` 底 + `--accent-text` 字，双主题对比度达标。

## 动效与系统偏好约定（v1.18，rant 2026-08-17T18:06:09 E）

- **按压反馈**：`.btn:active:not(:disabled) { transform: scale(0.98) }`（配合 `.btn` 既有 `transition: all .15s`）；disabled 按钮不触发；
- **统计卡 hover**：`.stat:hover` `translateY(-2px)` + 边框/阴影提升（与 `.card:hover` 语言一致，过渡 0.15–0.18s）；
- **数字跳动**：`bump(el)` 助手（remove `.bump` → reflow → add，重放 `@keyframes numJump`：`translateY(-3px) scale(1.02)`，0.35s）；**接入 4 处余额变化点**——钱包充值（`#side-balance` + `#wallet-balance`）、聊天消费扣款、加额批准、运营者给自己充值（`isMe` 判断）；`#side-balance` 为 inline 元素需 `display:inline-block` 才可 transform；
- **系统偏好**：`@media (prefers-reduced-motion: reduce)` 全局压 `animation-duration`/`transition-duration` 到 0.01ms、`animation-iteration-count: 1`、`scroll-behavior: auto`——**禁用过渡/动画但保留全部功能**；新增加动画时不得绕过此规则。

## 动态文档标题约定（v1.18，rant 2026-08-17T18:06:09 F）

- `document.title` **跟随视图切换**：`switchView` 内统一设置「`VIEW_TITLE[id]` · AITokenPool」（如「模型市场 Marketplace · AITokenPool」）；8 个视图全覆盖，未知视图回退「AITokenPool」；
- **默认「AITokenPool」**：DOMContentLoaded 初始化与登录页/无视图态回默认；HTML `<title>` 即「AITokenPool」；
- 游客受限视图被 `GUEST_VIEWS` 拦截时 `switchView` 提前 return → 标题保持不变。

## 其他细节约定（v1.18，rant 2026-08-17T18:06:09 G 收尾）

- **视图切换滚动复位**：`switchView` 内 `$("#main").scrollTop = 0`（`.main` 为 `overflow-y:auto` 滚动容器，渲染后复位）；新增视图切换入口都必须经过 `switchView` 以保证复位；
- **复制反馈**：`copyKey` 的 `flash(ok)`——复制成功按钮短暂变「已复制 ✓」（禁用 + 1.2s 恢复），降级路径「请 Ctrl+C」（v1.16 起，DOM 冒烟验证）。

## URL hash 路由约定（v1.19，rant 2026-08-17T20:39:30 A）

视图与地址栏 hash 联动，支持**收藏 / 分享 / 刷新恢复**（纯前端，零依赖，`pushState` 实现，不产生真实页面跳转）：

- **hash 格式**：`#/<view-id>`（view id 为 `VIEW_TITLE` 的键：`dashboard` / `marketplace` / `sharing` / `wallet` / `transactions` / `settings` / `admin`）；
- **视图切换同步 URL**：`switchView(id)` 末尾调用 `syncHash(id)` → `history.pushState(null, "", "#/"+id)`；当前 hash 已相同则跳过（不重复入栈）；pushState **不触发 hashchange**，天然避免回环；
- **前进 / 后退跟随**：DOMContentLoaded 注册 `window` 的 `hashchange` → `viewFromHash()` → 与 `activeView` 不同则 `switchView(id)`（浏览器前进/后退时 URL 先变、事件后发）；
- **非法 hash 回仪表盘**：`viewFromHash()` 对 `#/xxx`（非 7 视图）返回 `"dashboard"`；hashchange 处理器以 `{ sync: hashIsValid() }` 调用 `switchView`——**不重写 URL**（避免 pushState 污染历史、用户后退需要两次）；`#` / 空 hash → `null` 不动作；
- **刷新恢复**：DOMContentLoaded 先 `pendingHashView = viewFromHash()`；登录成功恢复 `pendingHashView || "dashboard"`（游客浏览 / 未登录态不冲突）；
- **游客拦截**：游客访问受限 hash（如 `#/settings`）→ `switchView` 内 `GUEST_VIEWS` 拦截（toast 提示），视图与 URL 均保持原状、不入栈新条目。

## 交易汇总卡约定（v1.19 → v1.23 改版，rant 2026-09-11T16:23:43 第 7 节）

- 交易记录页顶部为**汇总卡** `#tx-summary`（`.stat-grid` + 5 张 `.stat-card`，改用 PR3 通用组件层）：**消费 / 收益 / 点数变化 / Token 合计 / 记录数**，正数 `var(--ok)`、负数 `var(--danger-text)`、零值中性；
- **口径不变**（数据契约未动）：汇总值来自后端 `summary` 全量 SQL 聚合（无 summary 时兜底用 `filterRows(list, TX_COLUMNS, txTable.filters)`），**不受分页影响**；「记录数」取后端 `total`（真分页下即当前筛选条件的全量条数）；
- 工具栏右侧 `#tx-count` 同步显示 `tx.pager.count`（共 N 条），与汇总卡的记录数同源；
- 图表：`#tx-trend` 改用原型 `.trend` 双色柱状（消费 / 收益两柱 + `.legend`），数据源仍是 `/api/transactions/trend`；**x 轴按请求窗口补零，保证左→右时间递增且柱距恒定**（后端 GROUP BY 只返回有交易的桶，缺行会导致柱子左移——原型同款 bug 的根因）；
- 窄屏（≤560px）`.stat-grid` 两列、`.trend` 高度收紧。

## select 美化约定（v1.19，rant 2026-08-17T20:39:30 C）

- 全站 `select` **移除原生箭头**（`appearance:none` + `-webkit-appearance:none`），改用**自定义 SVG 下拉箭头**：`--select-arrow` CSS 变量（内联 data-URI，深色主题浅色箭头 `#aab6c8`、亮色主题深色箭头 `#55627a`，`url("data:image/svg+xml,…")` 内空格须 `%20` 编码）；
- `padding-right` 预留箭头空间（`select.input` 30px / `.th-filter` 24px / page-size 22px）；**hover / focus 边框同 input**（`var(--accent)`，focus 加 `box-shadow: 0 0 0 1px var(--accent)` 光圈）；`disabled` 态 `opacity:.55` + `not-allowed`；
- **覆盖所有 select 来源**：静态 `.input`（市场筛选、共享表单三级联动、设置页）、动态 `select.th-filter`（表格列筛选）、`select[data-page-size]`（分页器每页条数）；
- ⚠️ 注意：这些选择器的规则**必须用 `background-color` 而非 `background` 简写**（简写会把 `background-image` 置 none 抹掉箭头）；`select.input option { background: var(--bg-card) }`（下拉项深色）与 `select.input.input-error`（错误红边框）保持不动；
- ⚠️ `.th-filter` 自带 `width:100%`（表头列筛选需要撑满单元格），因此**工具栏里的直属 select 必须显式 `width:auto`**（`.toolbar > select`）——否则交易页时间范围下拉会拉伸到整行（C1978 视觉复查发现）。

## toast 队列约定（v1.19，rant 2026-08-17T20:39:30 D）

- 单例 `#toast` 已废弃 → 改为**队列容器** `#toast-wrap`（`index.html` 底部，初始为空）：`position:fixed; bottom:28px; left:50%; translateX(-50%)`，`flex-direction:column` 纵向堆叠，`gap:10px`，`pointer-events:none`（不拦截页面点击）；
- `toast(msg, type)` **每次创建独立 `.toast` 元素**（`document.createElement` + `appendChild`），不再覆盖旧消息；**上限 `TOAST_MAX = 3`**——超限同步移除最旧一条（`wrap.children[0]`）腾位；
- **独立生命周期**：每条到时（`TOAST_MS=2600`）加 `.out` 触发 `toast-out` 淡出动画（`TOAST_OUT_MS=200`）后 `removeChild`；互不影响、不共享定时器；
- **分级样式保留**：`.toast.success/.error/.info` 边框色 + 文字色与 v1.16 一致，75 个 `toast()` 调用点零改动；`.toast` 自身 `pointer-events:auto`（容器 none），为可交互 toast（如按钮）预留；
- 冒烟测试注意：DOM-stub 的 `classList.add` 只更新 `_classes` 集合、不同步 `className` 字符串——断言淡出态用 `_classes.has("out")`。

## 快捷键帮助面板约定（v1.19，rant 2026-08-17T20:39:30 E）

- **触发**：按 `?`（或 `Shift+/`，浏览器会给出 `e.key === "?"`）开合右上角行内卡片 `#help-panel`；**Esc 或再按 `?` 关闭**；关闭按钮 × 同效；
- **形态**：`position:fixed; top:76px; right:24px` 浮层卡片（非 modal、无遮罩、`z-index:950` 低于 toast），入场 `help-in` 动画；窄屏（≤560px）左右 12px 全宽、`top:68px`；
- **内容**：`renderHelp()` 渲染 4 行快捷键（`/` 搜索、`1–8` 视图、`Esc` 关闭/取消、`?` 帮助）+ 底部上下文行（当前视图 `VIEW_TITLE[activeView]` + 亮/深色主题）；
- **优先级**：全局 keydown 里帮助打开时 **Esc 先关帮助**（再关行内新建 Key），`?` 在 typing 守卫之后（输入框内不劫持）；`toggleHelp(force)` 支持强制开/关（close 按钮用 `toggleHelp(false)`）；
- **kbd 键帽**：`.kbd` 样式（等宽、边框、底部 2px 立体），与 `.nav-key` 视觉一致。

## 市场行展开约定（v1.19，rant 2026-08-17T20:39:30 F）

- 市场表格每行首列（厂商）加 **`+`/`−` 展开按钮** `.row-expand`（小号等宽，hover accent）；点击在 **tr 下追加详情行** `.mk-detail`（`colspan=7`，浅底 `--bg-soft`，`tbodyIn` 轻动画，**行内展开不弹窗**）；
- 详情内容（`mkDetailHtml(m)`）：**Max tokens**（查 `D.MODELS` 同名模型的 `max`，未公布显示「未公布」）、**价格换算**（`1M tokens ≈ N 点` 按输出价 + 输入价/1M）、**上下文长度**（`D.ctxFmt`）、**可用性**（可用/繁忙 + 成功率）、**多 key 自动故障转移**（仅 `m.multi` 显示，见 `docs/architecture.md` §3 模块表 `router.rs` 行）；
- **仅展开当前行**：`mkExpanded` 存展开模型 id（**数据态而非 DOM**，搜索/筛选 `renderMarketplace` 整表重建后仍保留）；点其它行自动收起，再点当前行收起；
- 事件在 `#mk-body` 现有 click 委托里扩展 `[data-mk-expand]` 分支（先于 `[data-use-model]` 判断）；
- 移动端卡片模式：详情 td 无 `data-label`（`td::before` 空）→ 整行仅展示详情内容，`mk-detail-grid` `auto-fit` 自适应列数。

## 登录页约定（v1.19，rant 2026-08-17T20:39:30 G）

- **视觉 polish**：`.login-card .logo` 加大（52px，渐变微光 `box-shadow: 0 0 0 1px rgba(78,205,196,.35), 0 0 18px rgba(78,205,196,.35)`）；`.login-brand h1` 22px；`.login-form .input:focus` 加 `0 0 0 3px var(--accent-soft)` 聚焦光晕（深/亮主题通用）；
- **行内校验**：空邮箱 →「请输入邮箱 / 账号」、空密码 →「请输入密码」（复用 `setFieldError`/`field-error` 组件：红边框 + 行内文案 + 聚焦首个错误 + 输入自动清除）；表单 `novalidate` 自管校验；输入框带 id（`#login-email` / `#login-pass`）；
- **记住我**：`#login-remember` checkbox → localStorage `atp-remember`（登录提交时存，DOMContentLoaded 时还原）；`demo-hint` 小字显示演示账号；
- 冒烟测试注意：stub 中 `setFieldError` 依赖 `input.parentNode.querySelector(".field-error")` —— stub 的 parentNode 需实现该查询；`insertAdjacentElement` 记录插入元素供断言。

## 接入端点卡片约定（v1.19，rant 2026-08-17T20:44:18）

- 设置页 **`#endpoint-card`「接入方式 / API 端点」** 卡片，位于 API Key 卡片**上方**（先看端点再生成 key）；
- **三行端点**（v1.22 全站 UI 重设计对齐原型，rant 2026-09-11T16:23:43）：OpenAI Chat `/v1`（POST /v1/chat/completions）· OpenAI Responses `/v1`（POST /v1/responses）· Anthropic Messages `/anthropic`（POST /anthropic/v1/messages）——三条都是后端真实路由（`src/routes/mod.rs`），不是虚标：前两者共用 `/v1` base，协议路径不同；
- 每端点一行 `.endpoint-row`（原型三列栅格 `auto 1fr auto`，行间 1px 分隔线）：`.ep-tag` 协议标签（accent 小药丸）→ `.ep-url`（`--mono` 等宽、`user-select:all` 整段选中、可横向滚动）→ `.ep-copy` 复制按钮 → `.ep-desc` 说明小字跨第 2-3 列（支持的工具与协议路径）；
- **URL 数据**：`apiEndpoints()` 按 `endpointBase()`（`GET /api/config` 的 `public_url`，取不到则同源 origin）实时拼接，无静态域名；`.ep-desc` 文案走 i18n（`settings.ep.*.desc`，`data-ep-desc` 索引在 `applyEndpointUrls()` 里随语言刷新）；卡片内 `.ep-note` 说明端点来源；`.ep-steps` 使用步骤 ①②③（生成 key → 填 Base URL → 填 key）；
- **复制**：`copyEndpoint(i)` 复用 copyKey 的降级链（clipboard API → textarea+execCommand → 提示 Ctrl+C）与「已复制 ✓」flash（1.2s 恢复）；事件绑定 `document.querySelectorAll("[data-ep-copy]")`（bindEvents）；
- 窄屏（≤560px）：`.endpoint-row` 纵向堆叠，`.ep-url` `word-break:break-all` 自动换行。

## 首次引导 tour 约定（v1.20，rant 2026-08-17T20:46:57 A）

- **触发**：登录成功后 `maybeStartTour()`——`localStorage atp-tour-done === "1"` 则不触发；设置页 `#tour-replay-btn`「重新查看引导」随时重放；
- **4 步**（`TOUR_STEPS`，每步 `{view, sel, title, desc}`）：仪表盘 `#dash-stats` → 模型市场 `#view-marketplace` → 共享管理 `#view-sharing` → 钱包/设置 `#endpoint-card`；
- **结构**：`#tour-overlay`（半透明遮罩，点击=关闭）+ `#tour-ring`（目标 accent 高亮环，`getBoundingClientRect` 定位，`tour-pulse` 呼吸动画，reduced-motion 静止）+ `#tour-pop` 气泡（右上「跳过」、底部「上一步/下一步/完成」、计数 `n / 4`；`data-tour-action` 委托）；
- **行为**：步骤切换自动 `switchView(step.view, {sync:false})`（不入历史）；气泡定位视口内钳制；**Esc / 点外 / 跳过 / 完成 均关闭并写 `atp-tour-done=1`**（关闭即标记，之后不再出现）；
- 全局 keydown 优先级：**引导开时 Esc 先关引导**（再处理帮助/行内表单）；冒烟测试注意 stub 需给元素 `getBoundingClientRect`，登录须预填邮箱/密码（item G 行内校验拦截空值）。

## 可交互 toast / 复制后引导约定（v1.20，rant 2026-08-17T20:46:57 B）

- **`toast(msg, type, opts)` 扩展**：`opts.action = { label, onClick }` 时渲染为**内嵌按钮**（`.toast-action`，accent 药丸，hover 反色）并延长展示时长至 `TOAST_ACTION_MS=6000`（普通 toast 仍 2600ms）；无 action 时保持 `textContent` 路径（零回归）；
- **复制 API Key 成功**（`copyKey` 的 `okToast` 路径，含 clipboard 成功与 execCommand 降级成功）→ toast「已复制…」+「配置接入端点 →」按钮；
- **`gotoEndpointCard()`**：`switchView("settings")`（幂等，已在设置页保持）→ `#endpoint-card` 加 `.ep-flash`（accent 光圈闪烁 0.8s ×2，重放时 remove→reflow→add）→ `scrollIntoView({behavior:"smooth", block:"center"})`；
- 与 20:44:18 端点卡片联动：复制 key → 跳转端点卡片 → 填 Base URL 闭环。

## 表格密度切换约定（v1.20，rant 2026-08-17T20:46:57 C）

- 设置页「偏好」区**「表格密度」**两档单选：`#density-comfortable`（舒适，默认）/ `#density-compact`（紧凑），name 统一 `density`，`.density-options` 纵向布局；
- v1.22（rant 2026-09-11T16:23:43）：「偏好」新增**主题下拉 `#prefs-theme`**（与右上角/侧边栏切换按钮共用 `applyTheme()`，localStorage `atp-theme` 记忆）与**默认模型下拉 `#prefs-model`**（选项来自真实 `/api/models`，在 `renderPrefModels()` 里填充，未加载时只有「未设置」）；「账户」改为昵称（`/api/me` 真实昵称）+ 只读邮箱（登录账号，无后端改邮箱接口）；三个卡片用 `.card-grid-3`（原型三栏），API Key 卡片用 `.spread` 把「生成新 Key」提到卡片标题右侧；

- **localStorage `atp-density`**（`"comfortable"` / `"compact"`，无值默认 `comfortable`）：启动时 `applyDensity(getDensity())` 还原并勾选对应 radio；`change` 事件 → `applyDensity(value)`（`#app` 加/移 `.density-compact` 类 + 写 localStorage，try/catch 兼容隐私模式）；
- 修饰类**只加在 `#app`**，通过 `.density-compact .table …` 选择器全站生效：紧凑档 `th/td` padding `4px 8px`、`td` 字号 12px、`th`/行内按钮 11px；舒适档不写任何规则（默认样式零回归）；
- 新增表格无需改动——继承 `.table` 即自动响应密度；冒烟测试断言走 `#app` 的 `_classes.has("density-compact")` 与 radio `checked`。

## 市场「最近使用」约定（v1.20，rant 2026-08-17T20:46:57 D）

- 位置：市场页工具栏计数下方 `#mk-recent` 行（`recent-label` + `.chips` 容器 `#mk-recent-chips` + 「清空」按钮 `data-mk-recent-clear`），无记录时 `hidden`；
- 数据：**localStorage `atp-recent-models`** = 最近模型 id 数组（JSON），**最多 5 个**、**去重**（`markRecentUsed(id)`：先滤掉已存在再 `unshift` 置顶，`saveRecentIds` 截断 5）；`getRecentIds()` try/catch 容错（旧数据/隐私模式 → 空数组）；
- 渲染：`renderRecent()` 把 id 映射为 `.chip` 按钮（`data-recent-model`，找不到模型则跳过），写入 chips 容器并同步 `#mk-recent.hidden`；**`renderMarketplace()` 末尾调用**（进市场即还原）+ **`openChat()` 内 markRecentUsed 后立即调用**（使用后即时更新）；
- 交互：`#mk-recent` click 委托——`[data-recent-model]` → 游客 toast「请先登录」/ 否则 `openChat(id)`（复用市场「使用 / 消费」主操作）；`[data-mk-recent-clear]` → `saveRecentIds([])` + `renderRecent()`（行隐藏）；
- 样式：`.recent-row`（flex 换行，label 次要色）+ `.recent-row .chip:hover` accent 高亮（复用 `.chip` 基础药丸）；冒烟测试注意 stub 需给 chip 元素 `closest("[data-recent-model]")` 返回自身。

## 交易记录导出 CSV 约定（v1.20，rant 2026-08-17T20:46:57 E）

- 入口：交易页 page-head 右上 **`#tx-export-btn`「导出 CSV」**（`.btn.btn-secondary.btn-sm`），`bindEvents` 绑 `exportTxCsv`；
- 数据范围：**当前筛选可见行** = 服务端按 tab + 列筛选返回的行（**列筛选只有服务端一个实现**：`TX_COLUMNS` 每个带 `filter` 的列都声明 `serverFilter: true`，见 C2114）；导出与表格共用 `filterRows(list, TX_COLUMNS, txTable.filters)`，而该调用对已声明 `serverFilter` 的列**不生效** ⇒ 导出的行 = 表格显示的行；无数据 → toast info 不导出；
  ⚠️ 不要在客户端再筛一遍：请求侧 `txFilterParams` 会 `trim()`、服务端用 SQL `LIKE`，与本地子串比较的语义不同，重复筛选会把服务端认可的行删掉（表格空、计数与汇总卡却仍有数 —— 见 C2114）；
- **各列与表格单元格同口径**（C2054 点数 / C2111 时间）：时间列走渲染单元格的同一个 `fmtPrecise(t.time)`（本地精确时间），**不**直接写视图行的 `t.time`（库内 UTC 串，`txsToView` 不转换）——否则同一行在表里是 `23:04`、在导出文件里却是 `15:04`（东八区；西半球反向）。`#139`（rant 2026-08-24T12:38:44）把单元格改成当地时间展示时，漏了导出这个消费者；
- 格式：**UTF-8 BOM**（`"\uFEFF"` 前缀）+ `\r\n` 换行 + 表头 `时间,类型,模型 / Key,Token 用量,点数,状态`；类型用 `TX_TYPE` 中文映射；点数正负号原值；字段含 `,`/`"`/换行按 RFC4180 双引号转义（`cell()` 助手）；
- 下载：`Blob(type="text/csv;charset=utf-8")` → `URL.createObjectURL` → 临时 `<a download>` click → `remove()` → `setTimeout 1s` revoke；文件名 **`aitokenpool-transactions-YYYYMMDD.csv`**（`new Date()` 本地日期）；
- 冒烟测试注意：stub 需给 `document.createElement("a")` 返回带 `click()`/`remove()` 的元素并捕获 `href`/`download`，`URL.createObjectURL` 捕获 Blob（`arrayBuffer()` 首 3 字节 EF BB BF 验证 BOM——`blob.text()` 会按规范剥掉 BOM）；列筛选联动可注入 `#tx-table` 的 `querySelectorAll(".th-filter")`/`querySelector('[data-filter-key=…]')` 假输入并 fire `input`。

## 数据表格键盘导航约定（v1.20，rant 2026-08-17T20:46:57 F；2026-09-13 改为 DOM 派生，去名册）

- **作用表 = 从 DOM 派生，不维护名册**：任何 `<tbody>` 里的数据行都可导航（`mk-body` / `share-body` / `api-keys` / `emp-body` / `dept-body` / `model-body` / `ops-body` / JS 建的 `raise-requests` 表，以及 `tx-table` 内动态建出的 tbody）。新增数据表无需登记，重绘也无需重新绑定——此前是一份手写 id 名册，`model-body` 漏登记即导致重试死键 + 无键盘导航；
- **激活**：① 点击表格行（**document 级**一次性 click 委托，容器 `kbdTbodyOf(e.target)` = `closest("tbody")`，行取 `closest("tr")`，跳过 `.mk-detail`）；② 直接按 ↑/↓——`kbdContainerFrom(t)` 同样按 `closest("tbody")` 解析（键盘事件目标通常是 body ⇒ 沿用 `kbd.c`；`kbd.c` 若已被重建（`isConnected === false`）则视为未激活）；
- **键位**：`ArrowDown/Up` → `kbdMove(dir, c)` 行高亮 `.row-active`（accent 左侧竖条 `inset 3px 0 0` + `--accent-soft` 底），未激活时 ↓ 首行 / ↑ 末行，`scrollIntoView({block:"nearest"})`；`Enter` → `kbdEnter()` 点击行内首个可用 `button.btn:not(.row-expand)`（disabled 不触发）；`Esc` → `kbdClear()`（无高亮时落到原逻辑：关帮助/行内表单/引导）；
- **守卫**：typing（INPUT/TEXTAREA/SELECT/contentEditable）与 meta/ctrl/alt 组合键不拦截；`?`、数字键视图切换、Esc 原有优先级（引导 > 帮助 > 行内新建 Key > 表格高亮）均不受影响；
- 冒烟测试注意：导航容器由真实 DOM 的 `closest("tbody")` 解析（不再比对 id）⇒ 旧 stub（`qs(sel)` 返回带 `#` 前缀的假 id）不影响它，但事件目标须是真实 DOM 节点；Esc 分支链依赖 `#ak-new-inline`、`#help-panel` hidden 预置 + `atp-tour-done=1`（防 tour 拦截）。

## 品牌与登录页氛围约定（v1.20，rant 2026-08-17T20:46:57 G）

- **favicon**：`ui/index.html` `<head>` 内 **inline SVG data URI**（`rel="icon" type="image/svg+xml"`）——渐变圆角方块（`#4ecdc4→#2a9d8f`）+ AT 文字，零外部文件；URL 编码（`%23`=#、`%3E`=>、`%3C`=<）；
- **登录页氛围**：`.login-view::before` = 44px 淡色网格（两个 1px `linear-gradient`）+ **径向 mask**（`radial-gradient(ellipse … #000 25%, transparent 72%)`）边缘淡出，`pointer-events:none`；`.login-view::after` = accent 微光圆（`rgba(78,205,196,.16)` 径向渐变）+ `login-float`（10s `translate` + `scale` 交替动画）；`.login-card { position:relative; z-index:1 }` 浮于氛围层之上；
- **logo 质感**：`.logo`（登录页 + 侧边栏共用）加 accent 描边 ring（`0 0 0 1.5px rgba(78,205,196,.45)`）+ 外发光（`0 0 14px`）；`.logo::after` 顶部内高光 `inset 0 1px 0 rgba(255,255,255,.30)` + 底部内阴影 `inset 0 -1px 0 rgba(0,0,0,.14)`（`position:relative` 定位基准）；
- **reduced-motion**：全局规则（`prefers-reduced-motion` 下 `animation-iteration-count:1 !important`）自动让 `login-float` 静止，无需单独规则；
- 冒烟测试以结构断言为主（favicon data URI 含渐变/AT、::before 网格、::after 动画、logo::after 内高光、reduced-motion 覆盖）。

## 数据说明

- 点数规则与机制细节见 `docs/user-stories.md`（v1.8：机制说明不再进入面向用户的界面文案）；UI 只呈现结果（余额数字、模型价格点数、交易金额/类型/状态、可用/繁忙）
- 消费模拟（聊天 Mock）按 输出参考价 × 0.19M tokens 计费，扣减小数点数并产生 consume 交易（US-6）
- 上架单价不由分享者手填：单价是模型×厂商的客观属性，平台按模型价格表自动计算（参考单价 = 该模型输出价 点数/1M）；模型无定价数据时给出"按默认价"兜底，不报错
- 市场模型带 `multi` 标记：表示该模型配置多个上游 key → 标注「多 key · 自动故障转移」（见 `docs/architecture.md` §3 模块表 `router.rs` 行：多 Provider 选择 / 粘性 / 静默故障转移——原文的 v0.2 / 4.2.1 节，已随文档精简合并到此节）；消费模拟（聊天 Mock）按 输出参考价 × 0.19M tokens 计费，扣减小数点数并产生 consume 交易（US-6）
- 上架需提交 API Key（password 输入）：平台加密托管、仅用于代理调用；共享列表只展示脱敏值（如 sk-****1234，前 3 后 4），不展示明文
- 删除 = 彻底下架（key 从平台移除，不可恢复）；暂停 = 临时不接单，可恢复
- 登录态数据全部来自后端 API（`ui/js/api.js`）；`data.js` 内嵌数据仅限游客市场浏览与上架表单兜底（见 v1.22 约定），无后端依赖的纯静态浏览不再成立

## 共享页「本月新增」卡约定（v1.24，C2015）

- 共享管理页统计区 `#share-stats` 由 3 张卡扩为 **4 张**（对齐原型 `docs/prototype/aitokenpool-console.html:1197`）：在架 Key / 累计收益 / 已用量 / **本月新增**；
- **数据源**：`GET /api/sharings` 自本次起返回每行 `created_at`（UTC ISO，带 `Z`；走既有 `dao::utc_iso`，与 `api_keys.created_at` 同口径）；
  ⚠️ `sharing_row()` **按下标读列**，两个调用点（`list` / `patch`）的 SELECT 列清单必须逐字一致 ⇒ 新增列一律**追加在末尾并同改两处**，否则字段静默错位；
- **月份口径 = UTC，且只在一处计算**：视图行上的 `month` 由 `utcMonth(created_at)` 取 `YYYY-MM`，与后端 SQL 的 `strftime('%Y-%m', …)` 同源（ops/wallet/admin/org 全站聚合口径一致）；当前月键取 `new Date().toISOString().slice(0,7)`。
  **不得**改用 `getMonth()`（本地月份）——那会让每月 1 日 00:00–08:00（UTC+8）的本页数字与同页其它统计不一致；
- **副标题不复刻原型**：原型写死 `deepseek-v4-flash`，而该卡聚合的可能是多个模型（原型自己的表格就渲染多行）⇒ 单个模型时显示该模型名，多个时显示 `share.stats.newthis.sub`（多个模型），为空时显示 `share.stats.newthis.none`；
- 拿不到 `created_at` 的行**不计入**（宁可少报不误报）；`Live.sharings` 未就绪时统计区整体清空（沿用零 mock 降级）；
- 新键（zh/en 各 3 个）：`share.stats.newthis{,.sub,.none}`。

## 登录态零 mock 约定（v1.22，rant 2026-08-19T15:54:06 系统性清理）

- **铁律**：登录态（`loggedIn()`）任何视图**不得渲染 `data.js` 的 `D.*` mock 数据**；mock 仅限游客模式（`GUEST_VIEWS = ["marketplace"]`，且市场模型列表登录态用 `GET /api/models` 真实数据）；
- **统一降级模式**：登录态 `Live.x ? view(Live.x) : loadError(空态+重试)`——绝不 fallback 到 `D.*`；失败渲染 `loadErrorHtml`（div 容器）或 `loadErrorRow`（tbody 容器，`<tr><td class="empty-cell">` 包裹，避免 div 直接进 tbody 被浏览器提升到表外破坏布局与重试委托）；重试按钮 `data-live-retry` 由 `setLiveError(容器, html, loader)` 委托——**渲染方写下降级态时把 loader 一并交出**，容器上一次性的 click 委托（`WeakMap` 存 loader、`WeakSet` 记已绑定，重建不重绑）负责分发。谁渲染降级态谁负责传 loader ⇒ 新增表格无需在名册里登记（漏登记 = 重试按钮点了没反应）；
- **已清零路径**（原 10 处 mock 泄漏）：仪表盘统计（`D.TRANSACTIONS` 聚合）→ 登录用 `/api/wallet` + `/api/dashboard`，未就绪显示 0；月度聚合/sparkline → 登录用 `Live.dashboard.series`，未就绪净 0 空行；交易页/CSV 导出 → 登录失败空态；API Key 设置 → 登录失败空态（生成/改名/删除已走真实 API）；管理视图成员 → 登录失败空态；用量报表 → 登录失败空态；部门管理（CRUD 已真实 API，移除本地 push/splice）；运营者（runtime/users）→ 登录失败空态；加额申请（提交/审批已真实 API）；上架表单（登录已 `POST /api/sharings`，游客无上架权限移除 mock 分支）；
- **`data.js` 保留对象**：`MODELS`（上架表单定价兜底）、`PLANS`（上架表单兜底，登录用 `/api/plans` 单一真源）、`PROVIDERS` / `PROVIDER_LABELS`（厂商枚举/显示名）、`MARKET`（游客市场浏览；`multi`/`success` 虚构字段已移除——登录态 `multi = available_keys >= 2` 真实计算，成功率后端暂无字段不再展示）、`USER`（会话存储：登录后由 `/api/me` + `/api/wallet` 覆盖，初始值不渲染）；
- **已删除对象**：`TRANSACTIONS` / `SHARINGS` / `API_KEYS` / `EMPLOYEES` / `DEPARTMENTS` / `USAGE_MODEL` / `USAGE_EMP` / `OPERATOR_USERS` / `RAISE_REQUESTS`；
- **市场行**：登录态 `ctx` 来自 `/api/models` 新增的 `context_window` 字段（dao `list_models_with_availability` 已补）；厂商筛选下拉按数据源重建（登录=`/api/models` 真实厂商、游客=`D.PROVIDERS`）；
- **兼容防护**：残留的游客分支读取 `D.TRANSACTIONS || []` / `D.SHARINGS || []`（对象已删，防止登录页首帧空引用）；聊天模拟（`sendChat`）为 P2-D 候选（chat-modal 流式网关），不再写 `D.TRANSACTIONS`；
- **验收**：干净库登录 → 仪表盘/市场/共享/交易/设置/管理/运营全部为真实数据或空态，无 mock 数字（如 32,800/100,000、+1,611、51,200/80,000）；`grep -n "D\.[A-Z_]" ui/js/app.js` 仅剩游客/表单兜底与会话存储引用。

## 界面国际化 i18n 约定（v1.21.1，rant 2026-08-18T20:49:22 + 21:40:10 去中英混排）

- **语言包**：`ui/js/i18n.js` 零依赖 IIFE，`I18N = { zh, en }` 双词典（806 键 ×2，覆盖导航/登录/视图标题/通用/仪表盘/市场/共享/钱包/交易/设置/管理/运营/聊天/游客/相对时间/帮助/tour/主题/错误映射）；`window.t(key, vars)` 查当前语言，**缺失回退 zh，再缺回退 key 本身**；`{var}` 占位符插值；
- **切换机制**：设置页「偏好 → 界面语言」下拉（`#prefs-lang`，zh/en）→ `I18n.setLang()`：写 `localStorage('atp_lang')` + `document.documentElement.lang` 同步（zh→`zh-CN` / en→`en`）+ 派发 `atp:langchange` → app.js 重渲染 `renderNav()` + `renderView(activeView)` + `document.title`（引导中额外 `renderTourStep()`）；**首载**：localStorage → `navigator.language` 前缀（`zh*`→zh，否则 en）→ 默认 zh；切换即时生效无需刷新；
- **静态文案**：`index.html` 内静态中文用 `data-i18n` / `data-i18n-ph`（placeholder）/ `data-i18n-title`（title）标记，`applyStatic()` 启动时与每次切换时批量替换；**容器含表单控件的 `<label>` 用 `<label><span data-i18n="KEY">文本</span><input…></label>` 结构**（避免 innerHTML 替换销毁控件）；
- **动态文案**：`app.js` 面向用户字符串全部走 `t('key')`；**语言敏感常量存 key 而非文案**（NAV/VIEW_TITLE/TOUR_STEPS/HELP_KEYS 存 key，渲染时 `T()` 解析；SHARE_STATUS/RAISE_STATUS 的 `text` 为函数；TX_COLUMNS 的 `title`/`options` 为函数；`DAY_LABELS` 动态 `T("share.day."+n)`）——保证切换语言后重渲染即时生效；
- **数字/时间本地化**：`I18n.fmtNum`（zh→`zh-CN` / en→`en-US` `toLocaleString`）；`I18n.fmtRelTime`（刚刚/N 分钟前/N 小时前/昨天 ↔ just now/N min ago/N hr ago/yesterday）；数量单位（人/个/笔/次）用 `cnt.*` 键（zh 带量词，en 纯数字）；
- **后端错误映射**：`api.js` 抛错前过 `I18n.mapErr()`——en 模式下已知中文错误映射为英文，未知**原样返回**；zh 模式原样透传；`ERR_MAP`（`i18n.js`）按**最长匹配**取值，带运行期插值的消息只登记到插值符之前的稳定前缀（写全模板永远匹配不上）。**后端每写一条用户可见的中文错误，就必须在 `ERR_MAP` 里登记**，并给两个包加对应键——否则英文界面上直接显示中文，而 `cargo test` 全绿（`src/i18n_pack.rs::every_backend_error_message_reaches_the_wordlist` 现在把这条约束变成门禁：它扫 `BACKEND_ERROR_SOURCES` 列出的后端源码，逐条断言「能被 `ERR_MAP` 命中」，并由 `backend_error_sources_cover_the_routes_directory` 用 `src/routes/` 的实际目录项兜住漏登记的文件）；**`api.js` 自己的文案一律按 key 取**（`T("err.network")` / `T("err.http", {n})` 等），文件内不写中文原文（`api_client_error_text_is_key_based`）。
- **单语原则（v1.21.1 去混排）**：zh 词典值一律纯中文（仅保留 API/Key/Plan/tokens/CSV 等专有名词、键盘快捷键与占位符），不再内联英文注释；`index.html` 已移除全部 `<span class="en">` 静态小字（55 处）；`.en` CSS 样式已删除；en 词典保持纯英文；
- 冒烟测试：node 无 DOM 桩跑 i18n.js（t/setLang/mapErr/fmtNum/fmtRelTime 断言，见开发记录）；Key 一致性扫描（`src/i18n_pack.rs` 门禁：app.js 的 `T()` 字面量 431 个、index.html 的 `data-i18n*` 305 个，去重并集 681 键全部存在于 ZH/EN）。

## 仪表盘「我的共享」数据源与降级约定（v1.21.2，rant 2026-08-19T15:48:17 BUG）

- **数据源**：登录后 `loadDashboard()` 拉取 `/api/wallet` + `/api/dashboard` + **`/api/sharings`**（此前漏拉 sharings → 仪表盘「我的共享」恒显示 `D.SHARINGS` mock，如 GLM/deepseek 假行）；`Live.sharings` 拉取成功后 `renderDashboard()` 用真实数据渲染「厂商 · Plan / 已用 / 额度 / 累计收益」；
- **降级原则**：**mock 只用于游客模式**——`Live.x ? view(Live.x) : D.x` 的 fallback 在登录态一律不得暴露 mock：
  - 登录态 `Live.sharings` 为空数组（`[]`，上架 0 条）→ 空态「还没有上架的 key」；
  - 登录态拉取失败（`null`）→ `setLiveError($("#dash-sharings"), loadErrorHtml(空态文案, T("err.loadFail")), () => loadDashboard())`：空态 + 「重试」按钮（`data-live-retry` 容器级委托），不白屏、不显示 mock；
  - 游客模式（`loggedIn()` 为假）→ 继续用 `D.SHARINGS` mock 浏览（不变）；
- **覆盖范围**：仪表盘 `renderDashboard()` 与共享管理 `renderSharing()` 两处 `Live.sharings` 消费点均按此约定（共享页为登录态专属视图，原 mock fallback 只在拉取失败时暴露，同属本 bug 类）；
- **冒烟测试注意**：登录态断言「我的共享」不得含 `glm-5.2` / `deepseek-v4-flash` 行；干净库（上架 0 条）应见空态文案；stub `api.get("/api/sharings")` 抛错时断言出现 `[data-live-retry]` 按钮且点击后重新调用 `loadDashboard()`。

## 设置 / 管理 / 运营布局约定（v1.22，rant 2026-09-11T16:23:43 PR6）

- **page-head crumb**：8 个视图统一 `.crumb`（`/ 设置` `/ 管理视图` `/ 运营视图`），与仪表盘/市场/共享/钱包/交易一致；
- **管理视图**（4 tab 不变）：成员管理 pane 增加 `#emp-search` 成员搜索（成员名 / 邮箱 / 部门，`hl()` 高亮）+ 表格「角色 / 部门 / 永久点数 / 赠送点数 / 可用」列；用量报表用 `.card-grid-3` 三栏；组织管理与模型管理的工具栏为「搜索 + `.grow` + 主按钮（`btn-sm`）」，与原型一致；
- **运营视图**（2 tab 不变）：运行概览新增四张卡片——**服务版本**（`/api/ops/runtime` 的 `version`，取自 `env!("CARGO_PKG_VERSION")`，与 `/healthz` 同源；前端**不得**写死版本号）与**运行时长**（`uptime_secs` / `uptime_days` / `uptime_hours` / `uptime_minutes` / `uptime_secs_rest`，进位在后端 `split_uptime` 完成，前端 `fmtUptime` 只挑「最高两个非零位」并取 `ops.uptime.{days,hours,minutes,seconds}` 单位）、**今日调用量（按小时）** `.bar-list`（`today_hours`，服务端 0-23 全量补零；GROUP BY 会省略无调用的小时，不补零会让柱子整体左移，与交易页 `txTrendDays` 同款坑）与**上游 key 健康** `.mini-list`（`key_health` 按厂商聚合 total/on/off，三态 pill：健康 / N 个异常 / 全部失败）；成员充值的搜索框移到卡片标题行右侧（原型 `.spread`）；
- **交易量卡（第 9 张，回归修复）**：`total_txs`（`ops.stats.trades` + `cnt.trades` + `ops.stats.trades.sub`）自 `85982e8`（PR #80）起就在 `/api/ops/runtime` 返回（全库 `COUNT(*)`，累计**全部类型**），但 v1.22 零 mock 重构 `89963f3` 删掉 mock 分支那张卡时漏了重接。**这不是原型对齐**——原型没有这张卡（原型 4 张，实现 9 张，多出的卡是刻意的）。值一律取自响应，**不取** `D.TRANSACTIONS`；
- **零 mock 不破**：以上数据全部来自真实端点，加载失败仍走空态 + 重试（`.mini-item` / `.bar-row` 只在有真实数据时才渲染）。

## 交易类型筛选：一份状态，两套控件（C2112）

- **只有一份状态**：交易页的「类型」筛选器有两个控件 —— 顶部 **`#tx-tabs`**（全部 / 消费 / 收益）与「类型」列表头下的 **`select.th-filter[data-filter-key="type"]`**（6 个库内值）。**状态只存在 `txTable.filters.type`**（空串 = 不限；取值恒为库内值 ⇒ 语言无关，见 C2031：「选项 value 与文案分离」）。
- **两者都读写它**：`txTypeFilter()` 是唯一取值定义（`loadTransactions` 的 `type` 参数、`renderTransactions` 的 tab 高亮都调它）；`setTxTypeFilter(v)` 是唯一写入口，并**同步已渲染的 select**（#148 之后表头不重建 → 控件值不会自己跟上状态）。
- **删掉的第二份状态**：`txTab`。此前顶部 tab 另存一份 `txTab`，而请求按 `filters.type || txTab` 取值 ⇒ 列筛选一旦出值，tab 的写入就被永久盖住：点 tab 只是挪高亮、列表不变（**死控件**），高亮却仍按 `txTab` 画（**说谎的指示器**）。jsdom 实测（改前 5/11）：列筛选选「赠送」后，请求 `type=gift` 而「全部」仍高亮；点「消费」后请求仍是 `gift`、`select` 仍显示 `gift` 而高亮已跳到「消费」——三者各说各话。
- **高亮口径**：生效值不属于 all/consume/earn 时（topup / withdraw / gift / expire）**没有任何 tab 自称生效**（都不高亮），因为把「全部」点亮而列表只有赠送行同样是说谎。改版后不变量：**请求参数 / tab 高亮 / 列筛选 select 三者恒为同一份状态的投影**。
- **冒烟测试注意**：点 tab 后状态已变，重拉由 `renderTransactions` 的筛选签名比对触发 —— **不要再显式 `loadTransactions()`**（会与它并发两次请求，后到者可能把先到者的行覆盖回去）。断言请比对「最近一次 `/api/transactions` 请求的 `type`」与「高亮的 tab」与「`select` 的 value」，三者用**同一个派生**（`wantTabs(type)`），不要写死字面量 —— 字面量会在应用的高亮恰好拼对时误绿。

## 交易表「模型」/「Key」列：单元格文案 = 筛选口径（C2113）

- **两列都是服务端筛选**：`txFilterParams()` 把「模型」列的文字发成 `model`、「Key」列发成 `key_name`，服务端 `tx_where` 用 LIKE 匹配库内值（`transactions.model` / `api_keys.name` → `key_label` 表达式）。**客户端 `filterRows` 只用于导出 CSV**（过滤当前页），表格本身的行由服务端全量过滤 + 分页。
- **不变量**：**单元格里出现的每一段文字，都必须能被该列筛选命中**。因此无值行只能用**语言中性的占位符 `—`**（`user` 列与 Key 列消费分支早已如此），**不能**填本地化类型名 —— 服务端没有语言包，`txType(t.type)`（「赠送」「过期」）在 SQL 里永远匹配不到，按它筛选得 0 行（C2113 修前即此：文案看似可筛选，实际是**说谎的漏斗**）。
- **两侧逐字对应**：`txsToView` 的 `t.model || "—"` / `t.key_name || t.key_label || "—"` ↔ `tx_where` 的 `COALESCE(NULLIF(…, ''), '—')`（Key 列是 `COALESCE(NULLIF(ak.name,''), NULLIF(<key_label 表达式>,''), '—')`，逐层 `NULLIF` 才能对齐 JS `||` 把空串当缺失的语义）。
- **改文案就要同时改两侧**：这是「显示口径 = 筛选口径」类的第 4 处（前 3 处：点数有符号值 C2054、时间列 C2111、Key 列空名兜底 C2101）。`transactions_model_and_key_filters_match_the_displayed_placeholder`（`src/routes/wallet.rs`）钉住服务端半边；阴性对照断言「类型名不再是模型列的可筛值」，防止有人反向把中文标签硬编码进 SQL。
- **冒烟测试注意**：前端半边（单元格文本）用 jsdom 启真 `index.html` + 四脚本、stub `fetch` 喂各类行（consume / gift / topup 哨兵）后**读渲染文本**；服务端半边由 Rust 测试覆盖。两侧的期望值都要**从同一条规则推出**（「库内值，空则 `—`」），不要照抄另一侧的输出 —— 照抄会让两边一起错。

## 会话恢复：非 401 失败不得演成「已登出」（C2124）

- **唯一入口**：boot 的会话恢复（`app.js` `DOMContentLoaded` 里那个 IIFE）只做一件事 —— `if (api.getToken()) await restoreSession();`。判定与降级**全部**写在 `restoreSession()` 里，登录页不再自己接错误。
- **三档判定，只有 401 能判「未登录」**：401 = token 已被服务端作废（`api.js` 已清 token + `__atpLogout` 回登录页），**不重试**（重试不会让失效 token 变有效）；网络错误（`status === 0`）/ 5xx（含网关 504）视为**可重试**，等 1 s 重试**一次**（抖动通常只持续数百毫秒）；其余 4xx 不重试（重试无意义）。
- **不变量**：**`api.getToken()` 非空时，任何非 401 失败都不得把用户摆在登录页**。重试后仍失败 ⇒ **照常 `enterApp()`** 并 `toast(T("login.session.fail"))`。理由：token 仍在却显示登录页 = 谎报「已登出」，而 URL hash 还指向上次视图，用户只会理解为被踢出（宿主 2026-09-14 21:00 实测：`/api/me` 被拖到网关 504 时「停在登录页 + token 仍在 + hash 仍是 `#/sharing`」三特征同现）。
- **两个状态分开表达**：「加载失败」由各视图自己的降级态（`loadErrorHtml` / `loadErrorRow` + 重试，见「登录态零 mock 约定」）承担；「未登录」只由 401 路径承担。若 token 其实已失效，进入 app 后第一次真实请求会拿到 401，由 `api.js` 清 token 回登录页 —— 那条路径给出的才是诚实的「登录已过期」。**不要**为了「稳妥」把非 401 也当作登出。
- **文案**：`login.session.fail` 只说「加载失败」，**不得**写成「请重新登录」（会与 `login.session.expired` 混为一谈）。这条由 `src/i18n_pack.rs::session_failure_copy_does_not_claim_the_user_is_logged_out` 在 CI 里钉住（两档文案必须不同 + 失败档不得要求重新登录）；**视图那一半（非 401 必须进 app）是 JS 控制流，CI 里没有 JS 测试运行器**，只能靠本节约定与下面的冒烟测试。
- **冒烟测试注意**：用 jsdom 启真 `index.html` + 四脚本、只 stub `fetch`，按 leg 脚本化 `/api/me` 的响应：`200` → app 可见 + 登录页隐藏 + `/api/me` **恰好 1 次**；`504` 或 fetch reject → app 可见 + 登录页隐藏 + token 仍在 + toast 是 `login.session.fail`；`504 → 200` → 恰好 **2 次**调用且无错误 toast；`401` → 登录页可见 + token 清空 + **恰好 1 次**（不重试）；无 token → `/api/me` **0 次**。断言期望值一律 `T("login.session.fail")` 现取，**不要**在测试里写死文案字面量。

## 401 的语义由调用方声明：凭据端点不是「会话过期」（C2120）

- **咽喉的默认行为**：`api.js` 的 `request()` 见到 401 就做全局登出（`clearToken()` + `window.__atpLogout()` → 回登录页 + `toast(T("login.session.expired"))`），并抛 `{status: 401}`。这对**业务端点**是对的：401 只能是我们带上去的会话凭据被服务端否掉了。
- **但有两个端点故意用 401 表示「你刚提交的凭据不对」**：`POST /api/auth/login`（邮箱不存在 / 口令错）与 `POST /api/auth/change-password`（旧密码错）。后者的两个 401 目前**没有前端消费者**（全仓 `ui/` 无该路径调用点），只作为规则的一部分记录在此。
- **声明方式**：`api.post(path, body, { on401: api.CREDENTIAL_401 })`。`api.CREDENTIAL_401` 是 `api.js` 导出的唯一取值；**不声明（或声明未知值）一律按会话失效**——漏声明只会「多登出一次」，不会「少登出一次」，失败方向是安全的。
- **不变量**：**凭据端点的 401 不得清 token、不得回登录页、不得弹 `login.session.expired`**。它按**普通错误**抛给调用方（保留 `status === 401`），由该表单自己的行内错误呈现 —— 登录页是 `setFieldError($("#login-pass"), T("login.err.bad"))`。
- **修前实测**（jsdom 启真 `index.html` + 四脚本、只 stub `fetch`、驱动**真表单**、后端回 401）：行内「邮箱或密码错误」与「登录已过期，请重新登录」**同时**出现在屏幕上 —— 后者把一句「你从未登录过」念给了刚输错密码的人。两层都跑（`handleUnauthorized()` 先截胡，异常随后冒泡到 `catch`），错的是前一层。
- **折入的同轴缺陷**：登录成功后不再 `await loadSession(); enterApp();`，而是走 boot 的**同一个**入口 `restoreSession()`（`if (await restoreSession()) toast(login.welcome)`）。理由与「会话恢复」小节完全相同：**token 已存却停在登录页 = 谎报「已登出」**（改前 `saveToken()` 之后 `loadSession()` 一旦非 401 失败，token 留在 storage 里而人留在登录页）。凭据已被接受之后，401 就不再是「登录失败」了。
- **CI 覆盖**：形状（调用点声明了 + 咽喉的 401 分支受该声明守卫 + 全局登出没被整段删掉）由 `src/i18n_pack.rs::credential_401_is_not_a_session_expiry` 钉住；控制流本身没有 JS 运行器，与上节同理。
- **冒烟测试注意**：① stub `POST /api/auth/login` → 401，主断言是 **toasts 不含** `T("login.session.expired")`（改前为红，有鉴别力），行内错误 **等于** `T("login.err.bad")` 只能作阳性对照（改前已绿）；② 另起一条 leg：带 token boot + stub `GET /api/me` → 401，断言 token 被清 + 回登录页 + toasts **含** `login.session.expired` —— 这条防止有人用「干脆不做全局登出」来让 ① 变绿；③ 再一条：登录成功但 `/api/me` 回 500，断言 app 可见（与「非 401 不得停在登录页」同一条不变量）。

## 时间戳：一律整串交给时间 helper（C2126）

- **唯一的线上格式**：后端用 `dao::utc_iso()` 统一序列化，前端拿到的一律是 `YYYY-MM-DDTHH:MM:SSZ`（`src/dao.rs` 的 `format!("{date}T{time}Z")` 是这条契约的载体）。前端**有**一族现成的本地化 helper：`fmtPrecise`（本地精确到秒）、`timeCell`（单元格：主文本 + 悬停）、`timeAgo`（相对时间）、`localMD`（本地月-日）、`utcMonth`（UTC 月键）。
- **不变量**：**时间戳必须以原始串到达渲染器，格式化只能由 helper 做**。不允许在中间层用 `.slice()` / `.replace()` 自己加工一个线上时间戳 —— 加工 helper 的**输出**可以（`fmtPrecise(k.created_at).slice(0, 10)` 取本地日期就是对的），加工**线上串**不行。理由：那个串是 UTC 且带 `T`，自己切会同时犯两个错 —— 泄出 ISO 的 `T`（屏幕上真的出现 `09-13T16:30`）并按 UTC 显示。要截断/换格式，就加在 helper 之后。
- **修前三处**（都在 `ui/js/app.js`，同一把尺子）：① 管理员加额申请「已处理」行 `(r.created_at || "").slice(5, 16)` → 屏幕上是 `09-13T16:30`；② 设置页 API Key「创建时间」`String(k.created_at || "").slice(0, 10)` → UTC 日，东八区用户在当地 08:00 前看到「昨天」；③ 交易视图行 `time: (t.time || "").replace("T", " ").slice(0, 16)` → **在渲染器之前**就把秒抹掉，而列（`timeCell(t.time, true)`）与 CSV 导出的口径都是 `HH:MM:SS` ⇒ 屏幕上的秒永远是伪造的 `00`（C2111 统一了「导出 = 单元格」，两份口径同源之后，源头的截断就成了口径本身）。
- **改法**：① `timeCell(r.created_at, true)`；② `fmtPrecise(k.created_at).slice(0, 10)`；③ 视图行原样 `time: t.time || ""`。**零新增 i18n 键**（helper 只做数值格式化，文案键与本次无关）。
- **CI 覆盖**：`src/i18n_pack.rs::wire_timestamps_reach_the_renderer_unsliced` 钉两件事 —— `_at` / `last_used` 这类线上字段不得被 `.slice()` / `.replace()` 原地加工（含阴性对照：提取器必须认得出修前的两种形态、并放过 `fmtPrecise(...).slice(...)` 与纯透传 `last: k.last_used || null`），以及 `txsToView` 返回的视图行里 `time` 必须是裸值。
- **冒烟测试注意**：jsdom 启真 `index.html` + 四脚本、只 stub `fetch`，夹具必须带**非零秒**（`2026-09-13T16:30:45Z`）—— 用 `:00` 的夹具看不见第 ③ 面（改前改后都是 `:00`）。三面的期望值都从夹具用 `Date` **独立算出**（本地时间/本地日期），不要抄 helper 的输出。**时区是前提而不是细节**：`TZ=UTC` 下本地 == UTC，本轴整体不可见，探针必须先断言时区。

## 行内卡片的 Enter 提交：容器级委托，不逐字段登记（C2127）

- **两类容器，两条路**：`#share-form` 是**真 `<form>`**（配合 `type="submit"` 按钮，浏览器自己实现隐式提交，所有字段免费获得 Enter）；其余行内卡片是 `<div class="form">` / `<span class="inline-edit">`（`#topup-card` / `#raise-card` / `#dept-form-card` / `#model-form-card` / `#ak-new-inline`），隐式提交不存在，得自己实现。
- **不变量**：**Enter 提交挂在容器上（`wireEnterSubmit($("#某卡片"), "#某确认按钮")`），不得逐字段登记**。逐字段 `$("#某字段").addEventListener("keydown", …)` 等于把「哪些控件能提交」抄成一份**名册**，而名册不会随控件增长 —— 模型表单曾有 10 个可输入控件、只登记了 2 个（厂商 / 模型名），输入价 / 缓存命中价 / 输出价 / 高峰三价 / 上下文窗口 / 最大输出这 8 个按 Enter 毫无反应（点「确认」都能提交）；而同页的部门表单每个字段都登记了（2/2）⇒ 是漏登记，不是取舍。
- **哪类控件算「提交」**：只有文本类输入控件（`NON_TEXT_INPUT_TYPES` = checkbox / radio / button / submit / reset / file / range / color / hidden 之外）。这与真 `<form>` 的隐式提交一致；焦点在「取消」上按 Enter 更不该提交（这正是委托到容器时要早退 `button` 的原因）。
- **请求只走一处**：委托只 `btn.click()`，不自己复制一遍校验与请求 —— 忙碌态（`withLoading`）、字段校验、API 调用都留在确认按钮自己的监听器里。
- **CI 覆盖**：`src/i18n_pack.rs::enter_submit_is_delegated_to_the_card` 钉形状 —— ① 任何让 Enter 去点确认按钮的 `keydown` 登记都不得挂在 `ui/index.html` 里声明于 `input` / `select` / `textarea` 的 id 上（阳性对照：`#chat-input` 的 Enter 是「发送消息」不是「提交表单」，不点按钮 ⇒ 不得被误判）；② `wireEnterSubmit` 必须在容器参数上登记、早退非 `INPUT` 与非文本类型；③ 五张卡片都必须走这个 helper，且 helper 调用数与卡片数相等。
- **冒烟测试注意**：每个控件跑**同一件事两遍** —— 在控件里按 Enter vs 用**相同的字段值**点确认按钮，比较是否发出了同一个请求（点确认那一遍是控件**自带的阳性对照**，排除「这个表单本来就用这些值提交不了」）。⚠️ `withLoading` 会把提交推迟 **320ms**，等待须 > 320ms，且每条腿前清空请求记录，否则会读到上一条腿的请求（结论完全反向）。

## 市场行的「可用性」只有一个事实：`avail`（C2128）

- **一个事实，四处渲染**：一个市场行的可用性由 `avail` 表达 —— 厂商单元格的绿点（`.dot`）、「使用」按钮的 `disabled`、可用性筛选下拉（「仅可用」）、展开详情里的「当前可用 / 当前繁忙」四处都读它。`keys`（key 计数）**不是**第二个事实，它只是**有计数时才存在**的补充说明：登录态由 `modelsToView()` 从 `/api/models` 的 `available_keys` 填，游客兜底表（`data.js > MARKET`）按 rant 2026-08-19T15:54:06「虚构数据已移除」**不携带**任何计数（同源被删的还有 `multi` / `success`）。
- **不变量**：**没有计数时不得把它读成「无 key」** —— 可用性 pill 必须先看计数（≥2 多 key / ==1 紧张），没有计数时回落到 `m.avail`（可用 / 无 key）。违反的后果是同一行自相矛盾：C2128 实测（jsdom 真 `index.html` + 四脚本）游客市场 **7/7 行**的这一格都渲染成「无 key」，而其中 **6 行**的圆点是绿的、「使用」按钮可点。
- **修法**：`availPill(m)` 增加一个**由 `m.avail` 驱动**的回落档（新键 `mk.avail.on`，zh「可用」/ en「Available」）。**不给兜底表补手写计数** —— 那等于把虚构的运营数据展示给游客，正是 v1.22 清理掉 `multi`/`success` 的那件事；游客要看到真实计数，就得让数据来自 `/api/models`。
- **CI 覆盖**：`src/catalog_gate.rs::market_availability_pill_agrees_with_the_row_it_renders` 钉两半 —— ① **消费侧**：`availPill` 体内必须出现 `m.avail`（阳性对照：兜底表每一行都必须有布尔 `avail`，同时证明这个字段名不是拼错的）；② **数据侧**：`MARKET` 行不得出现手写的 `keys`（挡住「给兜底表补计数」这条错修法）；③ 「使用」按钮所在行也必须读 `m.avail`（pill 要与它同源）。`js_function_body_stops_at_the_right_place` 再自证提取器不吞下一个函数，并用**合成输入**（删掉回落分支）证明断言有牙齿。
- **冒烟测试注意**：本轴只在**游客腿**可见（登录态 `keys` 恒存在，三档计数 pill 正常）⇒ 探针必须两条腿都跑，并把登录腿当作**阳性对照**（`keys=3/1/0` 必须渲染成三档），否则「pill 只会渲染一档」与「游客数据缺字段」两种解读都被拒不了。游客腿的行数（7）、圆点、按钮禁用态都要读**渲染后的 DOM**，不要读 `MARKET` 字面量。

## 视图载荷缓存：一个槽一个写者，且写者自己记下有效性证据（C2131）

- **`Live` 是「某个视图自己的载荷」的槽表**：`Live.transactions` 是**交易视图自己的**载荷（`loadTransactions` 写），`Live.apiKeys` / `Live.adminModels` / `Live.departments` / `Live.sharings` 同理。槽是缓存，缓存**必须**能回答「里面这份数据还算不算数」。
- **证据与载荷同源**：交易槽的有效性证据是 `txTable.loadedPage / loadedPageSize / loadedFilterSig`，由 `loadTransactions` 在写槽的**同一个函数**里写下；`renderTransactions` 的守卫只比这三项，不一致就重拉。因此**写槽的人必须就是写证据的人** —— 换了写者而证据不跟着搬，守卫比的就是**另一个写者的账本**，缓存内容与守卫手上的证据脱钩。
- **不变量**：**一个槽只能有一个写者**（`writes(Live.transactions) == {loadTransactions}`，且等于写 `txTable.loaded*` 的函数集合）。仪表盘要一个「交易笔数」数字时，它发的是**另一种查询**（`/api/transactions?page=1&page_size=1`，不带时间范围，只为读 `total`）—— 这类载荷只能进自己的槽（`Live.tradeCount`），**不得**住进交易视图的缓存。
- **修前实测**（jsdom 启真 `index.html` + 四脚本、只 stub `fetch`、冻结交易视图自己的请求以固定中间态）：`loadDashboard` 把那个 1 行载荷写进 `Live.transactions`，而守卫只比 `txTable.loaded*`（仪表盘一项都不碰）⇒ **放行外来载荷**。再入交易页（`renderView` 先同步 `renderTransactions()` 再异步拉取）三张脸同时说谎：表格 **1 行**（`page_size=1`）、汇总卡挂着「当前筛选」却显示**全时段**聚合（那次请求不带范围）、趋势卡「**趋势数据加载失败**」（`trend` 只有 `loadTransactions` 会挂上，而它既没发也没失败）。首次进入交易页不触发（`txTable.loadedPage` 仍是 `undefined` ⇒ 守卫为真 ⇒ 走重拉直接返回）。
- **为什么这条必须由静态门禁钉，而不是由 DOM 探针钉**：竞争修法「扩守卫」（让守卫也比对载荷的 `page_size`）能让探针的**全部缺陷断言**（B1/B2/B3）变绿 —— 探针只能证明「屏幕上不再是外来载荷」，证明不了「槽里不再是外来载荷」。而 `exportTxCsv` 直接读 `Live.transactions.items`、**不经那道守卫**，槽被污染它照样导出外来的那 1 行。A/B 实测：改前树门禁红、探针红 `{B1,B2,B3}`；扩守卫腿门禁**仍红**、探针轴断言**全绿** ⇒ 方向由门禁钉。
- **CI 覆盖**：`src/state_gate.rs` 钉两件事 —— ① 交易槽的写者名册**恰好**是 `{loadTransactions}`；② 写槽者集合 **==** 写 `txTable.loaded*` 者集合（两者都不为空，空集上的集合断言会假绿）。另有仪表盘那条：必须**仍拉**那个只取 `total` 的查询、必须写/读自己的 `Live.tradeCount`、**不得**读写交易槽（否则「把请求整段删掉」这种「修法」会让断言变绿而笔数永远是 0）。`the_body_extractor_stops_at_the_right_place` 自证函数体提取器不吞下一个函数，`the_scanners_have_teeth_on_a_second_writer` 用**合成输入**证明判别式有牙齿（含阴性对照：`if (Live.transactions)` / `Live.transactions.items` / `=== null` / 守卫里的 `txTable.loadedPage !== …` 都**不是**写）。
- **冒烟测试注意**：夹具必须让两种查询口径**真的不同**（带范围 vs 不带范围的 `summary` / `total`），否则「渲染了外来载荷」与「正常状态」在屏幕上无法区分。冻结交易视图自己的请求以固定中间态，并另起一条**阳性对照腿**（同样冻结、但把「先去仪表盘」换成「先去市场」）—— 红必须由那次仪表盘访问造成。射程：门禁只认**字面**的 `Live.<槽>` 与 `liveLoad("<槽>"`；动态键（`Live["transactions"] = …`）看不见。

## 按会话缓存的生命周期：身份边界必须清空，且每个视图都要有自己的 loader（C2132）

- **`Live` 是按会话的缓存**（上节讲它的写者），所以它的**生命周期**与「谁登录了」绑定。两条不变量：
  1. **身份边界必须丢弃每一个槽**。会话建立（`loadSession`：boot / 登录，两者都经 `restoreSession`）与会话结束（`exitGuest`：登出 / 401）两侧都要清空。清空必须**派生自** `Live` 的对象字面量（`Object.keys(Live).forEach((k) => { Live[k] = null; })`）—— 手抄名册会在新增槽时静默漏掉。
  2. **`renderView` 的每个分支都必须「既渲染又拉取」**。它是唯一允许「先同步渲染缓存、再异步拉取」的地方，于是「只渲染不拉取」的分支就是**永远显示缓存**的分支。
- **修前的两张脸**（jsdom 启真 `index.html` + 四脚本、只 stub `fetch`，驱动真导航 / 真登出 / 真登录表单）：甲用完仪表盘登出、乙登录后打开钱包 —— `#wallet-forever`（「永久点数」）显示的是**甲的** `Live.wallet.balance`（实测 `4,242.42424`，乙应为 `7.5`），**且永不自愈**：`renderView("wallet")` 当时是八个分支里**唯一**只 `renderWallet()` 的，缓存不被清就再也没人刷新它。同一根因的**瞬态**脸：乙落在仪表盘时，`renderView` 先**同步**用甲的缓存渲染 `#dash-stats`（冻结乙自己的 `/api/wallet` 即可读到：本月用量是甲的 `12,345.6789`）。
- **改法两半，缺一留红**：① `resetSessionCaches()`（派生式）在 `exitGuest()` 与 `loadSession()` 开头各调一次；② 钱包分支补上自己的 loader `loadWallet()`（镜像 `loadDashboard` 的尾段：`refreshWallet()` → `renderWallet()`）。② 顺带修掉**单会话**下的口径错 —— 此前登录后直接进钱包，`Live.wallet` 永远是 `null`，单元格只能回落到 `D.USER.balance`（= `available` = 永久 + 当日赠送）冒充「永久」。
- **CI 覆盖**：`src/state_gate.rs` 再加两条 —— `the_identity_boundaries_drop_every_session_cache`（清空必须含 `Object.keys(Live)`；体内**不得**出现逐个槽的赋值，否则就是第二份名册；`loadSession` 与 `exitGuest` 都必须调用它）与 `the_view_router_renders_and_loads_in_every_branch`（`renderView` 每个分支行都既含 `render` 又含 `load`）。两条都带**提取器自证**与**合成输入**（手抄名册 / 缺 loader 的分支必须变红）。
- **冒烟测试注意**：① 夹具必须让两个账号的钱包**可区分**，且 **`available ≠ balance`**（有当日赠送）—— 否则「拿到了自己的永久余额」与「回落到 available」不可区分；② 冻结点要**精确**：`loadSession` 自己的 `/api/wallet` 必须放行（否则会话建立就卡住），只扣住**仪表盘刷新**那一次（按序放行第 1 个、冻结第 2 个）；③ 隔离瞬态脸时，乙的**落点**必须是仪表盘 —— 若乙落在钱包，钱包的新 loader 会在登录过程中就把共享的 `Live.wallet` 刷新掉，瞬态脸**看不见**（这正是「只加 loader 不清缓存」这条竞争修法能让探针全绿的原因）；④ 断言按**读到的 DOM 文本**，不要读 `Live` 内部（探针可临时注入 `window.__Live = Live` 仅用于**诊断**）。


## 后端自造的显示文案：分界线在「数据字段 / 文案字段」（C2133）

- **咽喉只覆盖文案字段**：`api.js` 把后端 `error` 字段整串交给 `I18n.mapErr()`（词表 `ERR_MAP`），所以**错误文案**有兜底（C2129）。但**响应数据字段**是前端原样渲染的 —— 后端在数据字段里自造一句中文，`en` 界面上就是中文，而 `cargo test` 全绿。
- **不变量**：**显示文案归语言包，后端只回传数据或语言中性标记**。数据字段里的值要么来自 config / 库 / 用户输入（原值透传），要么是语言中性的机器值（空串 / `null` / 枚举名）。中文标签只允许出现在 `error` 字段（有词表）和邮件正文（无 locale 机制、中英双语）里。
- **修前两处可达**（都在 `en` 界面直接显示中文）：
  1. `src/routes/admin.rs` 的按部门聚合：`COALESCE(d.name, '（未分配）')` → `app.js` 的 `#usage-dept` 只过 `esc()`。任何 `dept_id IS NULL` 且本月有用量的用户都会让这行出现 —— 而**同一张页面**的成员表早就在用 `T("common.unassigned")`（同一个键、零新增）。
  2. `src/gateway.rs` 的 `/api/plans`：config 未写 `name` 时后端按 `type` 自造 `API（按量）` / `Token Plan` / `Coding Plan`；而 `config.example.toml` 与 `config.toml` 的 `[[plans]]` **全都**不写 `name` ⇒ 恒触发，上架表单的 Plan 下拉与上架成功的 toast 都读它。
- **改法**：① 无部门的桶改用**空串**（与同一个 handler 里 `users[].dept_name` 的 `COALESCE(d.name, '')` 同口径），前端 `d.name || T("common.unassigned")` 兜底；② `/api/plans` 的 `name` 改为 config **原值**（未配置即空串），前端新增 `planLabel(pl)`：有 `name` 用原文，否则按 `type` 取新键 `share.planName.paygo|token|coding`（下拉与 toast 共用这一个函数）。
- **CI 覆盖**：`src/i18n_pack.rs` 三条 —— ① `backend_data_fields_are_language_neutral`：扫全部 `src/**/*.rs`，提取「以 `json!` **数据**字段（key ≠ `error`）交付的中文字面量」，其集合必须**恰好等于**已裁定豁免清单（两侧都有牙：新增一处变红、删掉豁免项也变红）。提取器必须能穿透 `let name = … "中文" …; json!({ "name": name })` 这层间接（计划名就是这种写法；不穿透就漏掉一半的类），并跳过嵌套 `json!`；② 这两个渲染点必须**有**本地化兜底（`#usage-dept` 的 `barRow` 首参含 `T(`、`planLabel` 体内含 `pl.name` 与 `T("share.planName.…")`）；③ `GET /api/admin/usage` 的运行期契约：无部门用户有用量时，桶名不得含 CJK（`src/routes/mod.rs` 的 router 测试）。
- **豁免清单为什么存在**：`src/routes/mod.rs` 注册接口的 `"name": name` 里那个默认用户名（`email.split('@').next().unwrap_or(...)`）是**用户数据**的默认值（同 `db.rs` 种子里的 `'管理员'`），不是后端自造的显示标签 —— 且 `split().next()` 恒 `Some`，该默认值不可达。豁免项带**理由**、且与提取结果**等价**（`==`，不是 `⊆`），所以它不会腐烂。
- **冒烟测试注意**：`en` 语言包下断言 `#usage-dept` / `#sf-plan` 的**渲染后文本**不含 CJK（改前红）。夹具要让 `departments` 桶真的出现（`dept_id IS NULL` + 本月 `usage_records`），并**独立构造**期望值（`T("common.unassigned")` / `T("share.planName.paygo")` 现取，不要抄后端回传的串）；阴性对照腿用**配了 `name` 的 plan**（此时必须原样显示 config 的名字）。
