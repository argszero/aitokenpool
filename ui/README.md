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
- 本原则与 docs/user-stories.md §1.1 对齐。

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
2. 模型市场 Marketplace — 模型浏览、**搜索（输入防抖 ~150ms + 关键词 `<mark>` 高亮 + 清空 × 按钮，v1.18 D）**、厂商筛选、排序（按价格/上下文）；可用性标注「多 key · 自动故障转移」（该模型配置多个上游 key，架构 v0.2 路由策略）；每行「使用 / 消费」入口 → 聊天 Mock 模拟调用（按模型输出参考价扣小数点数，如 -0.38 点，产生消费交易；余额不足时阻断并提示）
3. 共享管理 Sharing — 默认只显示统计 + 我的共享列表（key 脱敏展示，可暂停/恢复/重新上架/彻底删除；列表展示「厂商 · Plan / 模型 · 可用时间段」，如「智谱 · GLM Coding Plan / glm-5.2 · 周一~周五 09:00-18:00」，未设置显示「全天」）；点击"＋ 添加 / 上架新 key"展开上架表单（**三级联动：厂商 → Plan → 模型**——内置国内已知 Plan 清单（阿里云百炼 / 智谱 / 火山方舟 / Kimi / MiniMax / DeepSeek，每家含「API（按量）」= 按量计价的 key），选 Plan 后显示 key 前缀 / 专属端点提示；**可用时间段为结构化字段**：星期多选 chips + 起止时间（留空 = 全天不限），备注仅纯文本；须填 API Key，平台加密托管；分享者只填声明额度，单价由平台按模型定价自动计算并展示参考价），提交成功或取消后自动收起
4. 钱包 Wallet — 点数余额、**本月点数变化（近 1 月按类型汇总收支 + 净变化，与仪表盘一致）**、**充值入口（US-4：模拟流程——输入点数 → 余额增加 → topup 交易记录；文案注明"演示，真实支付后续接入"；提现仍 disabled）**、**申请加额（US-20：企业成员余额低时申请更多点数 → 提交后等待管理员审批，默认需审批）**；收支明细已去重，统一到【交易记录】（页内提供跳转提示）
5. 交易记录 Transactions — 消费/收益/充值/提现/赠送（gift）唯一明细入口，Tab 筛选 + MRT 风格表格（列排序/列筛选/分页，与 Tab 叠加生效）
6. 设置 Settings — 账户、**API Key 管理（独占整行全宽，表格字段不挤压；完整增删改查：生成带名字、改名、删除需确认「删除后该 key 立即失效」、按名字搜索（防抖+高亮+清空，v1.18 D）、一键复制完整 key——列表脱敏展示 atk_live_****xxxx，复制为完整值；file:// 下 clipboard API 受限时降级为选中+Ctrl/Cmd+C）**、通知、偏好
7. 管理视图 Admin（管理员 / 运营者角色专属）— 成员管理（改部门：下拉可选部门或"未分配"；任意金额充值，正整数校验；新注册成员默认未分配部门；**加额申请审批（US-20：待审批列表 → 批准=成员余额+申请点数 / 驳回；申请默认需审批）**）、用量报表（按模型/成员）、组织管理（部门列表 + 部门增删改查 + 每月点数分配，含部门汇总统计，未分配成员不计入任何部门；**部门搜索（防抖+高亮+清空，v1.18 D）**；**添加/编辑部门用行内展开表单（`#dept-form-card`，名称+月分配，替代 window.prompt）**，删除有确认；**无「组织设置」表单**——组织名称 / 默认成员配额 / 开关均移除，"关闭外部注册"为部署配置项不在 UI 中）、平台运营（运营者 = 宿主本人，职责最小化：① 运行概览——运行状态 / 用户数 / 共享 key 数 / 交易量 / 点数流入流出；② 用户充值——按用户名 / 邮箱**搜索（防抖+高亮+清空，v1.18 D）**定位，输入点数金额（可为小数）确认后余额增加永久有效点数并产生一条交易记录）。**无独立 Key 池管理**——管理员与普通用户一样通过共享管理页「上架 key」配置上游 key（共享列表即 key 池视图，可暂停/删除）

## 文件结构

```
ui/
├── index.html        # 入口（登录页 + 应用外壳 + 全部视图）
├── css/style.css     # 设计系统（深色主题 · 强调色 #4ecdc4 · 响应式预留 · v1.15 视觉美化）
├── js/data.js        # 内嵌 mock 数据（模型价格、交易、共享、成员等）
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
- **统一使用**：交易列表（时间列）、加额申请列表、共享列表（上架时间列）、API Key 最近使用时间；数据新增 / 生成时间用 `nowTime()`（`MM-DD HH:mm`）写入即可自动相对化。

## 表格数字列约定（v1.17，rant 2026-08-17T16:57:17 C 数字列对齐）

- **数字列**（价格 / 点数 / 已用 / 额度 / 余额 / 用量）统一 `<td class="num">` + 表头 `<th class="num">`：右对齐 + `--mono` 等宽 + `tabular-nums`，便于纵向扫读；时间列 / 状态列 / 文本列保持左对齐；
- 数据表格渲染器 `buildDataTable` 支持列配置 `align: "num"`（交易表 tokens / pts 已启用）；
- 金额一律 `D.fmt()`（整数千分位；小数保留 2 位），禁止裸数字拼接。

## 键盘可达性约定（v1.17，rant 2026-08-17T16:57:17 D 无障碍）

- **焦点环**：全局 `:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }`；输入类控件（`.input` / `.th-filter`）已有边框高亮，`outline: none` 不叠加；
- **全局快捷键**（`document` keydown，输入框内不触发、Cmd/Ctrl/Alt 组合不劫持）：
  - `/` → 聚焦市场搜索 `#mk-search`；
  - 数字 **1-7** → 切换侧边栏视图（`NAV_ORDER` 顺序：仪表盘/市场/共享/钱包/交易/管理/设置；游客模式由 `switchView` 拦截提示登录）；
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

- **统一接线 `wireSearch(input, render)`**：所有搜索框（`#mk-search` / `#ak-search` / `#od-search` / `#ops-search`）走**~150ms 输入防抖**渲染（连续输入只渲染一次，避免整表重绘闪烁）+ **「清空 ×」按钮**（有内容时显示；点击清空立即重绘并聚焦，不走防抖）；HTML 结构为 `.search-box` 包裹 `<input>` + `<button class="search-clear">`；
- **关键词高亮用 `hl(text, rawQ)`**：先 `esc()` 转义再对查询词**大小写不敏感**包 `<mark>`（正则转义用户输入，`& < >` 等字符与转义正文同构不错位）；无关键词返回转义原样（重置后自动清除）；
- **程序化清空用 `resetSearch(input)`**（值 + × 按钮态同步，不触发渲染；调用方随后自行重绘）——空状态「清除搜索 / 清除筛选」按钮已统一走此路径；
- `mark` 样式：`--accent-soft` 底 + `--accent-text` 字，双主题对比度达标。

## 动效与系统偏好约定（v1.18，rant 2026-08-17T18:06:09 E）

- **按压反馈**：`.btn:active:not(:disabled) { transform: scale(0.98) }`（配合 `.btn` 既有 `transition: all .15s`）；disabled 按钮不触发；
- **统计卡 hover**：`.stat:hover` `translateY(-2px)` + 边框/阴影提升（与 `.card:hover` 语言一致，过渡 0.15–0.18s）；
- **数字跳动**：`bump(el)` 助手（remove `.bump` → reflow → add，重放 `@keyframes numJump`：`translateY(-3px) scale(1.02)`，0.35s）；**接入 4 处余额变化点**——钱包充值（`#side-balance` + `#wallet-balance`）、聊天消费扣款、加额批准、运营者给自己充值（`isMe` 判断）；`#side-balance` 为 inline 元素需 `display:inline-block` 才可 transform；
- **系统偏好**：`@media (prefers-reduced-motion: reduce)` 全局压 `animation-duration`/`transition-duration` 到 0.01ms、`animation-iteration-count: 1`、`scroll-behavior: auto`——**禁用过渡/动画但保留全部功能**；新增加动画时不得绕过此规则。

## 动态文档标题约定（v1.18，rant 2026-08-17T18:06:09 F）

- `document.title` **跟随视图切换**：`switchView` 内统一设置「`VIEW_TITLE[id]` · AITokenPool」（如「模型市场 Marketplace · AITokenPool」）；7 个视图全覆盖，未知视图回退「AITokenPool」；
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

## 交易汇总条约定（v1.19，rant 2026-08-17T20:39:30 B）

- 交易记录页卡片顶部（表格上方）有**紧凑汇总条** `#tx-summary`（`.tx-summary`，三列 inline，不引入新布局）：**总收入 / 总支出 / 净变化**，正数 `+` 绿（`var(--ok)`）、负数 `-` 红（`var(--danger-text)`）、零值中性显示 `0`；
- **随 tab 过滤联动**：切换「全部 / 消费 / 收益」tab 即时重算；且与表格**列筛选**一致——汇总基于 `filterRows(list, TX_COLUMNS, txTable.filters)`（`buildDataTable` 的筛选逻辑抽出的公共函数），反映与表格可见行相同的过滤集，**不受分页影响**；
- 窄屏（≤560px）自动收窄间距 / 字号，`flex-wrap` 换行兜底。

## select 美化约定（v1.19，rant 2026-08-17T20:39:30 C）

- 全站 `select` **移除原生箭头**（`appearance:none` + `-webkit-appearance:none`），改用**自定义 SVG 下拉箭头**：`--select-arrow` CSS 变量（内联 data-URI，深色主题浅色箭头 `#aab6c8`、亮色主题深色箭头 `#55627a`，`url("data:image/svg+xml,…")` 内空格须 `%20` 编码）；
- `padding-right` 预留箭头空间（`select.input` 30px / `.th-filter` 24px / page-size 22px）；**hover / focus 边框同 input**（`var(--accent)`，focus 加 `box-shadow: 0 0 0 1px var(--accent)` 光圈）；`disabled` 态 `opacity:.55` + `not-allowed`；
- **覆盖所有 select 来源**：静态 `.input`（市场筛选、共享表单三级联动、设置页）、动态 `select.th-filter`（表格列筛选）、`select[data-page-size]`（分页器每页条数）；
- ⚠️ 注意：这些选择器的规则**必须用 `background-color` 而非 `background` 简写**（简写会把 `background-image` 置 none 抹掉箭头）；`select.input option { background: var(--bg-card) }`（下拉项深色）与 `select.input.input-error`（错误红边框）保持不动。

## toast 队列约定（v1.19，rant 2026-08-17T20:39:30 D）

- 单例 `#toast` 已废弃 → 改为**队列容器** `#toast-wrap`（`index.html` 底部，初始为空）：`position:fixed; bottom:28px; left:50%; translateX(-50%)`，`flex-direction:column` 纵向堆叠，`gap:10px`，`pointer-events:none`（不拦截页面点击）；
- `toast(msg, type)` **每次创建独立 `.toast` 元素**（`document.createElement` + `appendChild`），不再覆盖旧消息；**上限 `TOAST_MAX = 3`**——超限同步移除最旧一条（`wrap.children[0]`）腾位；
- **独立生命周期**：每条到时（`TOAST_MS=2600`）加 `.out` 触发 `toast-out` 淡出动画（`TOAST_OUT_MS=200`）后 `removeChild`；互不影响、不共享定时器；
- **分级样式保留**：`.toast.success/.error/.info` 边框色 + 文字色与 v1.16 一致，39 个 `toast()` 调用点零改动；`.toast` 自身 `pointer-events:auto`（容器 none），为可交互 toast（如按钮）预留；
- 冒烟测试注意：DOM-stub 的 `classList.add` 只更新 `_classes` 集合、不同步 `className` 字符串——断言淡出态用 `_classes.has("out")`。

## 快捷键帮助面板约定（v1.19，rant 2026-08-17T20:39:30 E）

- **触发**：按 `?`（或 `Shift+/`，浏览器会给出 `e.key === "?"`）开合右上角行内卡片 `#help-panel`；**Esc 或再按 `?` 关闭**；关闭按钮 × 同效；
- **形态**：`position:fixed; top:76px; right:24px` 浮层卡片（非 modal、无遮罩、`z-index:950` 低于 toast），入场 `help-in` 动画；窄屏（≤560px）左右 12px 全宽、`top:68px`；
- **内容**：`renderHelp()` 渲染 4 行快捷键（`/` 搜索、`1–7` 视图、`Esc` 关闭/取消、`?` 帮助）+ 底部上下文行（当前视图 `VIEW_TITLE[activeView]` + 亮/深色主题）；
- **优先级**：全局 keydown 里帮助打开时 **Esc 先关帮助**（再关行内新建 Key），`?` 在 typing 守卫之后（输入框内不劫持）；`toggleHelp(force)` 支持强制开/关（close 按钮用 `toggleHelp(false)`）；
- **kbd 键帽**：`.kbd` 样式（等宽、边框、底部 2px 立体），与 `.nav-key` 视觉一致。

## 市场行展开约定（v1.19，rant 2026-08-17T20:39:30 F）

- 市场表格每行首列（厂商）加 **`+`/`−` 展开按钮** `.row-expand`（小号等宽，hover accent）；点击在 **tr 下追加详情行** `.mk-detail`（`colspan=7`，浅底 `--bg-soft`，`tbodyIn` 轻动画，**行内展开不弹窗**）；
- 详情内容（`mkDetailHtml(m)`）：**Max tokens**（查 `D.MODELS` 同名模型的 `max`，未公布显示「未公布」）、**价格换算**（`1M tokens ≈ N 点` 按输出价 + 输入价/1M）、**上下文长度**（`D.ctxFmt`）、**可用性**（可用/繁忙 + 成功率）、**多 key 自动故障转移**（仅 `m.multi` 显示，架构 v0.2 路由策略）；
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
- 每端点一行 `.endpoint-row`：`.ep-tag` 协议标签（OpenAI 兼容 / Anthropic 兼容，accent 药丸）→ `.ep-url`（`--mono` 等宽、`user-select:all` 整段选中、可横向滚动）→ `.ep-copy` 复制按钮 → `.ep-desc` 说明小字（支持的工具列表）；
- **URL 数据**：`API_ENDPOINTS` 静态常量（`ui/js/app.js`，注释标明真实值来自部署配置 `config.server.base_url` 之类）；卡片内 `.ep-note`「部署后替换为你的网关域名」（原型占位）；`.ep-steps` 使用步骤 ①②③（生成 key → 填 Base URL → 填 key）；
- **复制**：`copyEndpoint(i)` 复用 copyKey 的降级链（clipboard API → textarea+execCommand → 提示 Ctrl+C）与「已复制 ✓」flash（1.2s 恢复）；事件绑定 `document.querySelectorAll("[data-ep-copy]")`（bindEvents）；
- 窄屏（≤560px）：`.endpoint-row` 纵向堆叠，`.ep-url` `word-break:break-all` 自动换行。

## 数据说明

- 点数规则与机制细节见 `docs/user-stories.md`（v1.8：机制说明不再进入面向用户的界面文案）；UI 只呈现结果（余额数字、模型价格点数、交易金额/类型/状态、可用/繁忙）
- 消费模拟（聊天 Mock）按 输出参考价 × 0.19M tokens 计费，扣减小数点数并产生 consume 交易（US-6）
- 上架单价不由分享者手填：单价是模型×厂商的客观属性，平台按模型价格表自动计算（参考单价 = 该模型输出价 点数/1M）；模型无定价数据时给出"按默认价"兜底，不报错
- 市场模型带 `multi` 标记：表示该模型配置多个上游 key → 标注「多 key · 自动故障转移」（见 `docs/architecture.md` v0.2 路由策略）；消费模拟（聊天 Mock）按 输出参考价 × 0.19M tokens 计费，扣减小数点数并产生 consume 交易（US-6）
- 上架需提交 API Key（password 输入）：平台加密托管、仅用于代理调用；共享列表只展示脱敏值（如 sk-****1234，前 3 后 4），不展示明文
- 删除 = 彻底下架（key 从平台移除，不可恢复）；暂停 = 临时不接单，可恢复
- 全部数据为前端常量（mock），无后端、无真实调用
