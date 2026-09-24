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
5. 交易记录 Transactions — 消费/收益/充值/提现/赠送/过期（gift / expiry）唯一明细入口，Tab 筛选 + MRT 风格表格（列排序/列筛选/分页，与 Tab 叠加生效）
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
  - **调用点的分级必须取自这把尺子**（R92）：`toast()` 体里 `el.className = "toast" + (type ? " " + type : "")` **不校验 `type`** ⇒ 写一个没有 `.toast.<x>` 规则的词不是报错，是**静默退回基类样式**。忘记密码那条成功消息原本写的是 `toast(T("forgot.done"), "ok")` —— `"ok"` 是同一个 `if (r.status === "ok")` 的**判据字面量**被复用成了分级，而 `.toast.ok` 全仓没有规则（它只存在于设计原型），于是全应用唯一一条「成功」提示拿不到成功色。静态门禁 `src/state_gate.rs::every_toast_level_is_a_level_the_sheet_declares` 钉三件事（**每一件都从被测算的代码/样式表/文档里推导**，零手写名册）：① 每个 `toast(...)` 调用点的第二实参是**纯字面量**且落在「`.toast.<x>` 规则集合 − 函数体自己 `classList.add` 的状态类」里；② 该词表与本文档这一行写的契约**双向**相等；③ 反向 —— 每条分级规则都得有调用点用到（无死分级）。
  - ⚠️ **射程**：门禁是**词法**的 —— 它扫 `ui/js/app.js` 的源码文本（括号/字符串/注释/正则字面量感知），证明**词**与**规则**同源；**不证**屏幕上那一刻渲染出来的 class（那半归 jsdom 探针 `r92_probe.js`：真 boot ＋ 真表单 ＋ 读 `#toast-wrap` 的 `className`）。**也不要**用「给样式表补一条 `.toast.ok` 同体规则」（调用点不动）来消掉它 —— 屏幕当场变绿，但词表里多出一个本文档没写的同义词，门禁照样拒（两条仪器都实测：探针 `B2`/`C3` 腿、门禁 `R2`）。
- **按钮 loading**：提交类按钮用 `withLoading(btn, fn)`（转圈 + 禁用，模拟反馈后恢复）；
- **标签所有权（R169）**：带 `data-i18n` 的静态控件，它的标签**归 markup 那个键所有**——临时的忙碌 / 反馈文案可以**另起一个键**，但**收尾必须回到该控件自己的来源**（捕获原值，或引用 markup 声明的键），不得写死另一个键。静态门禁 `state_gate::the_control_that_rests_owns_its_label` 钉三件事（**每一件都从被测算的制品推导**，零手写键名）：① 名册里**会写自己标签**的成员（今天恰一个：`#login-form` 的提交按钮 —— 出厂 `login.enter`，却把标签恢复成另一个键）**最后一次**写标签必须派生自该元素自己的来源；② `ui/index.html` 里**每个「提交按钮带 `data-i18n` 的表单」**（今天 5 个）都要**解得出恰好一个**提交处理器，且不得以别人的键收尾；③ 反向：会写标签的成员对同一按钮的标签写**至少两次**（忙碌 + 收尾），「把忙碌态删掉」不算合格。
  - ⚠️ **射程**：门禁是**词法**的 —— 它证「收尾的表达式与 markup 声明的键同源」；**不证**屏幕上那一刻渲染出来的标签（那半归 jsdom 探针 `r169_probe.js`：真 boot ＋ 真 `submit`，读真渲染出的按钮文字，其中 `C4` 腿在**第一个 `await` 之前**采样，证明忙碌态真的发生过）。句柄只认本仓的主导写法 `querySelector('button[type="submit"]')`（今天 login / register / verify / share 四处）；`#forgot-form` 用的是 `querySelector("button[type=submit]")`（另一种拼法）⇒ 它**不计入**「会写标签的成员」，如实记录、不假装覆盖。**也不要**用「把 markup 改标成 `login.submit`」（两边自洽、门禁全绿）来消掉它 —— 探针 `D2` 腿按设计基线拒（那枚按钮就不再是原型印的那句话）。
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
  - Esc → 关闭消费对话框（`#chat-modal`）；
- **导航提示**：nav-item 补 `title`（"快捷键 N · 名称"）+ 右侧 `.nav-key` 键位角标（管理视图带「管理员」tag 时省略角标）。
- **键位只有一个真源**：角标 = 项在 `NAV_ORDER`（`NAV.flatMap(g => g.items)`）里的下标 +1，数字键处理器按同一数组取项。因此**渲染的每一项都必须取自 `NAV_ORDER`** —— 含游客分支（`NAV_ORDER.filter(it => GUEST_VIEWS.includes(it.id))`）：手搓一个同形字面量会让 `indexOf` 恒 -1，角标印 `0`（死键）、`title` 也跟着印 `Shortcut 0`，而真正生效的键游客看不到（C2141）。静态门禁 `src/state_gate.rs::the_sidebar_advertises_only_digits_that_work` 钉这个形状。

## 行内校验错误约定（v1.17，rant 2026-08-17T16:57:17 E 表单校验）

- **组件**：`setFieldError(input, msg)` 给输入框加 `.input-error`（红边框）并在其后插入 `.field-error`（红字小号行内文案），输入事件自动清除；`clearFieldError(input)` 手动清除；打开表单时重置；
- **覆盖**：充值自定义金额、申请加额（点数/原因）、部门表单（名称/配额/重名）、共享上架表单（API Key/厂商·Plan·模型·额度）；提交校验失败聚焦首个错误字段，不依赖 toast。
- **文案必须描述触发它的条件**：每条字段级错误说的是**它自己那个守卫**判定的条件（「空」与「太短」是两条规则、两句话）；同一把嗓子不得同时服务两类条件（`register.err.pass`＝「请输入密码」只服务「空」）。同一条规则在客户端与服务端必须用**同一句**：口令下限那句就是 `err.weakPassword`（服务端原话经 `I18N.mapErr` 的 `ERR_MAP` 落到它），客户端行内守卫直接引这个键，**不新造同值键**。静态门禁 `src/state_gate.rs::the_forgot_password_length_speaks_the_message_the_same_rule_gets_from_the_server` 钉这个形状（三条规则：至少有一个长度守卫 / 每个长度守卫都用**推导出来的**那个键 / 全文件没有键同时服务两类条件）。⚠️ **射程**：门禁只扫 `ui/js/app.js` 里**行内**写的 `if (<cond>) setFieldError(<field>, T("<key>"))` 这一种形态（跨行写、键经变量传、非 `T(...)` 的都在射程外，另行计数不参与分类）；它证的是**键**与规则同源，**不证**屏幕上那一刻的**值**（值与运行期归 jsdom 探针 `r93_probe.js`）。

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
- **趋势卡四档**（rant 2026-09-23T21:03:32）：`#tx-trend-modes`（`.tabs > .tab`，形状复用 `#tx-tabs`）提供 **总点数（默认）/ 消费 / 收益 / 消费＋收益** 四档，四档**共用同一份载荷**（`/api/transactions/trend` 已同时返回 `net`/`income`/`expense`）⇒ 切档只重绘本卡，**档位不进 `txQuerySig()`、不接 `reloadTransactions()`**（否则每次点档多发一次请求、且四档不再同源）。
  - `net` 档＝**窗口内累计净变化**（自最早桶起逐桶累加 `net`，窗口起点为 0，可为负），故卡片标题/副标题换 `tx.trend.card.net` / `tx.trend.net.sub` **如实写明口径**——加了列筛选后绝对余额不可定义，不得暗示为余额水位；形态用 `sparkline()` **折线**、y 轴按 `|值|` 定标且**包含零基线**；
  - 其余三档保持柱状；归一化的 `max` **只按当前显示的系列**取（`txTrendValues()`），`title` tooltip 与 `#tx-trend-legend` **只陈述显示的系列**；
  - 档位**每次进入交易页复位为默认**，复位点写在 `switchView()` 的**视图入口**（不是 `renderView()`/`renderTxTrend()`——那两个还会被 `atp:langchange` 调用，挂上去切一次语言就静默重置用户档位，#660）；**不做跨会话持久化**。

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
  - ⚠️ C2148 更正：`#prefs-model` 与昵称**都没有消费者**（下拉无监听/无存储键，昵称无后端写路径），
    现按「能力未开放」的仓内成例标成惰性；见下文「设置卡片里的控件」一节。

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
- 格式：**UTF-8 BOM**（`"\uFEFF"` 前缀）+ `\r\n` 换行 + 表头 `时间,类型,模型 / Key,Token 用量,点数,状态`；类型用 `TX_TYPE` 中文映射；点数正负号原值；字段含 `,`/`"`/CR/LF 按 **RFC 4180 §2.6** 双引号转义（`cell()` 助手）——**CR 必须在内**：记录分隔符是 `\r\n`，一个带裸 CR 的字段漏引号就会把一行切成两行（C2167）；
- 下载：`Blob(type="text/csv;charset=utf-8")` → `URL.createObjectURL` → 临时 `<a download>` click → `remove()` → `setTimeout 1s` revoke；文件名 **`aitokenpool-transactions-YYYYMMDD.csv`**（`new Date()` 本地日期）；
- 冒烟测试注意：stub 需给 `document.createElement("a")` 返回带 `click()`/`remove()` 的元素并捕获 `href`/`download`，`URL.createObjectURL` 捕获 Blob（`arrayBuffer()` 首 3 字节 EF BB BF 验证 BOM——`blob.text()` 会按规范剥掉 BOM）；列筛选联动可注入 `#tx-table` 的 `querySelectorAll(".th-filter")`/`querySelector('[data-filter-key=…]')` 假输入并 fire `input`。
- **CI 覆盖**（`src/state_gate.rs::the_csv_cell_escaper_quotes_every_rfc4180_special`，四条规则各有独立的牙）：①a 转义器的字符类**恰**含 `"` `,` CR LF 四元素（漏 CR 的写法在这里红）；①b 那处 `.test(` 的结果确实被当**条件**用（挡「留着字符类、把判定丢掉」这种半修）；② 全站**唯一**一处引号字符类实现（挡第二份 CSV 口径）；③ 反向 —— 不许退化成「恒加引号」（`/[",\r\n]|/` 这种词法上元素齐全、语义上匹配空串的逃逸，只有这条规则有牙）。判别式由 `the_csv_escaper_scanners_have_teeth` 用**合成输入**自证：「取不到 / 取到多处 / 元素不全 / 判定被丢 / 语义退化」五种形态必须报**不同**的错（坑 #291），且元素分词器按元素 token 比、不吃子串匹配的亏（`\r\n` 里含 `\n` —— 坑 #333）。
  ⚠️ **射程**：门禁是**词法**的 —— 它证明字符类的**元素**与三目式的**形状**，**不**求值 JS 正则的语义细节（`-` 的位置、`\r` 在 JS 里确为 CR），也**不**证明导出的文件真的能被解析器读回。后一半归运行期探针：jsdom 驱动真 `#tx-export-btn`、捕获交给 `URL.createObjectURL` 的 Blob、按 RFC 4180 读回 —— 带裸 CR 的字段必须仍落在**同一条**记录里、每条记录**恰 11 列**、单元格原样往返；`/[",\n]/` 的树上是 4 条记录、宽度 `[11,11,3,9]`。

## 交易表分页器窗口约定（R167，2026-09-19）

- **窗口形状**：页数 > 9 时分页器渲染为 `1 … p-1 p p+1 … N` —— 省略号**只在真的有间隙时**出现。
  窗口由 `pagerButtons` 里那行循环定义（左端 `page - 1`、右端 `page + 1`，两端分别夹在 `2` / `pages - 1`）。
- **两个省略号判据必须从窗口边界推导**（不是各自拍的常数）：左侧 `page > 3`、右侧 `page < pages - 2`。
  旧值 `4` / `pages - 3` 比窗口**紧一格**，于是在 `page = 4` 与 `page = pages - 3` 两处**印出相邻页码
  却不放省略号**，吞掉一个真实存在的页码；而省略号是不可点的 `<span>`（`user-select:none`）、全仓也没有
  prev/next ⇒ 用户无从知道那一页还在不在。
- **门禁** `state_gate::the_pager_window_and_its_ellipsis_guards_agree` 是**推导式**（不是快照，见坑 #469）：
  从循环取出窗口半宽 `A`、`B`，要求左判据 == `A + 2`、右判据 == `B + 1` ⇒ 它接受任何**自洽**的窗口，
  拒绝把这一次编辑的字面量写死的断言。

## 数据表格键盘导航约定（v1.20，rant 2026-08-17T20:46:57 F；2026-09-13 改为 DOM 派生，去名册）

- **作用表 = 从 DOM 派生，不维护名册**：任何 `<tbody>` 里的数据行都可导航（`mk-body` / `share-body` / `api-keys` / `emp-body` / `dept-body` / `model-body` / `ops-body` / JS 建的 `raise-requests` 表，以及 `tx-table` 内动态建出的 tbody）。新增数据表无需登记，重绘也无需重新绑定——此前是一份手写 id 名册，`model-body` 漏登记即导致重试死键 + 无键盘导航；
- **激活**：① 点击表格行（**document 级**一次性 click 委托，容器 `kbdTbodyOf(e.target)` = `closest("tbody")`，行取 `closest("tr")`，跳过 `.mk-detail`）；② 直接按 ↑/↓——`kbdContainerFrom(t)` 同样按 `closest("tbody")` 解析（键盘事件目标通常是 body ⇒ 沿用 `kbd.c`；`kbd.c` 若已被重建（`isConnected === false`）则视为未激活）；
- **键位**：`ArrowDown/Up` → `kbdMove(dir, c)` 行高亮 `.row-active`（accent 左侧竖条 `inset 3px 0 0` + `--accent-soft` 底），未激活时 ↓ 首行 / ↑ 末行，`scrollIntoView({block:"nearest"})`；`Enter` → `kbdEnter()` 点击行内首个可用 `button.btn:not(.row-expand)`（disabled 不触发）；`Esc` → `kbdClear()`（无高亮时落到原逻辑：关帮助/行内表单/引导）；
- **高亮即身份（R173）**：键盘的「当前行」以**行自己的高亮类** `.row-active` 为身份，不以记住的**下标**为身份 —— `kbdEnter()` 只在目标行**自己**带 `.row-active` 时才执行其主操作（重绘换掉 `innerHTML` ⇒ 高亮随旧元素消失 ⇒ 任何重绘自动失效），`switchView()` 在被接受的角色/访客守卫**之后**调用 `kbdClear()`（被拒绝的切换不得清除）。门禁 `state_gate::the_keyboard_row_action_follows_the_highlight_not_the_index` 按此校验（射程：`kbdEnter` 的目标解析 ＋ `switchView` 的清除调用 ＋ `kbdSet` 仍是唯一装填者）；
- **守卫**：typing（INPUT/TEXTAREA/SELECT/contentEditable）与 meta/ctrl/alt 组合键不拦截；`?`、数字键视图切换、Esc 原有优先级（引导 > 帮助 > 行内新建 Key > 消费对话框 > 表格高亮）均不受影响；
- **每个浮层守卫读该元素**自己**的隐藏机制**（R168，由各自的关闭函数推导）：`#help-panel` / `#chat-modal` 用 **class**（`classList.contains("hidden")`），`#ak-new-inline` / `#dept-form-card` / `#model-form-card` / 充值·加额卡用 **属性**（`.hidden`） ⇒ 判「开没开」必须读对那一个，写反会让守卫恒真、把 Esc 变成无条件捕获。 门禁 `state_gate::the_escape_contract_reaches_every_dismissible_overlay` 按此**推导**校验。
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
- **切换机制**：设置页「偏好 → 界面语言」下拉（`#prefs-lang`，zh/en）→ `I18n.setLang()`：写 `localStorage('atp_lang')` + `document.documentElement.lang` 同步（zh→`zh-CN` / en→`en`）+ 派发 `atp:langchange` → app.js 重渲染 `renderNav()` + `renderView(activeView)` + `document.title`（引导中额外 `renderTourStep()`；帮助面板开着时 `renderHelp()`）；**首载**：localStorage → `navigator.language` 前缀（`zh*`→zh，否则 en）→ 默认 zh；切换即时生效无需刷新；
- **开着的东西都要跟着走（R94）**：切语言时刷新的是**用户此刻可能正开着的**每一块 —— 导航、数据表表头、当前视图、标题、引导、以及**非模态的帮助面板**。`#help-panel` 的标题与关闭按钮是 `[data-i18n]`（`applyStatic()` 换掉），四行快捷键与 `#help-context` 却是 `renderHelp()` 用 JS 建的、**没有钩子**，而它全仓唯一调用点是 `toggleHelp` 的**打开**分支 ⇒ 漏刷新就是「同一块面板两种语言」，且整会话不自愈。**门禁** `state_gate::the_language_switch_refreshes_every_overlay_it_can_show` 从两处**推导**：浮层名册取自 `ui/index.html`（`#app` 之后的顶行元素带 `hidden` 类）；模态与否取自 `ui/css/style.css` 的类规则里有没有 `inset: 0`（模态浮层的全屏遮罩挡住了设置页的语言下拉 ⇒ 用户不可能在它开着时切语言，故不入射程）；写者名册取自「函数体里同时出现该浮层的某个 id 与 `T(`」⇒ **新增浮层或新增写者忘了登记会变红**。**射程**：门禁是**词法**的 —— 它证明刷新名册覆盖了派生出来的每个非模态浮层的每个写者，**不**证明屏幕上那一刻的文案真的是当前语言（那一半归 jsdom 探针），也**看不见**「切语言时干脆把面板关掉」这种竞争修法（探针 `A4` 腿把它拒掉）。
- **静态文案**：`index.html` 内静态中文用 `data-i18n` / `data-i18n-ph`（placeholder）/ `data-i18n-title`（title）标记，`applyStatic()` 启动时与每次切换时批量替换；**容器含表单控件的 `<label>` 用 `<label><span data-i18n="KEY">文本</span><input…></label>` 结构**（避免 innerHTML 替换销毁控件）；
- **动态文案**：`app.js` 面向用户字符串全部走 `t('key')`；**语言敏感常量存 key 而非文案**（NAV/VIEW_TITLE/TOUR_STEPS/HELP_KEYS 存 key，渲染时 `T()` 解析；SHARE_STATUS/RAISE_STATUS 的 `text` 为函数；TX_COLUMNS 的 `title`/`options` 为函数；`DAY_LABELS` 动态 `T("share.day."+n)`）——保证切换语言后重渲染即时生效；
- **数字/时间本地化**：`I18n.fmtNum`（zh→`zh-CN` / en→`en-US` `toLocaleString`）；`I18n.fmtRelTime`（刚刚/N 分钟前/N 小时前/昨天 ↔ just now/N min ago/N hr ago/yesterday）；数量单位（人/个/笔/次）用 `cnt.*` 键（zh 带量词，en 纯数字）；
- **后端错误映射**：`api.js` 抛错前过 `I18n.mapErr()`——en 模式下已知中文错误映射为英文，未知**原样返回**；zh 模式原样透传；`ERR_MAP`（`i18n.js`）按**最长匹配**取值，带运行期插值的消息只登记到插值符之前的稳定前缀（写全模板永远匹配不上）。**后端每写一条用户可见的中文错误，就必须在 `ERR_MAP` 里登记**，并给两个包加对应键——否则英文界面上直接显示中文，而 `cargo test` 全绿（`src/i18n_pack.rs::every_backend_error_message_reaches_the_wordlist` 现在把这条约束变成门禁：它扫 `BACKEND_ERROR_SOURCES` 列出的后端源码，逐条断言「能被 `ERR_MAP` 命中」，并由 `backend_error_sources_cover_the_routes_directory` 用 `src/routes/` 的实际目录项兜住漏登记的文件）；**`api.js` 自己的文案一律按 key 取**（`T("err.network")` / `T("err.http", {n})` 等），文件内不写中文原文（`api_client_error_text_is_key_based`）。
- **单语原则（v1.21.1 去混排）**：zh 词典值一律纯中文（仅保留 API/Key/Plan/tokens/CSV 等专有名词、键盘快捷键与占位符），不再内联英文注释；`index.html` 已移除全部 `<span class="en">` 静态小字（55 处）；`.en` CSS 样式已删除；en 词典保持纯英文；
- 冒烟测试：node 无 DOM 桩跑 i18n.js（t/setLang/mapErr/fmtNum/fmtRelTime 断言，见开发记录）；Key 一致性扫描（`src/i18n_pack.rs` 门禁：app.js 的 `T()` 字面量 431 个、index.html 的 `data-i18n*` 305 个，去重并集 681 键全部存在于 ZH/EN）。
- **整包可达性（C2155）**：语言包是**双份**的，一个没人引用的键不会报错、也不会被上面两条门禁看见——它们只问「**引用了的**键在不在包里」，方向**相反**。而 C2153 证明这类**孤儿键可以是活缺陷的指纹**（`share.toggle.relisted` 两包俱在却无人可达 ⇒ 共享切换的结局少了「重新上架」那一支），C2154 把当时那 59 个逐条裁定为残留/弱项/宿主裁定（零活缺陷）。**不变量**：一个键「可达」当且仅当它以**键 token 边界**（ASCII 字母数字 ＋ `_` `.` `-`）出现在消费语料——`app.js`/`api.js`/`data.js` **剥注释**后的代码 ＋ `index.html` **剥注释**后的标记 ＋ `i18n.js` 语言包区段**之外**的代码——或以**动态前缀**（`T("share.day." + …)`；前缀集合派生自语料本身，不写名册）开头。注释里的键名**不是**消费者（#296）。
- **CI 覆盖**：`src/i18n_pack.rs::every_pack_key_reaches_a_consumer` 断言「计算出的不可达集合 == `UNREACHABLE_PACK_KEYS`」**精确相等**（不是子集）。该清单是**日落清单**不是豁免注册表：新增一个无人用的键变红、从清单删一条而键仍不可达变红、**把某条日落键接上线也变红**（必须**同时**移出清单）。判别式由 `pack_reachability_checker_detects_injected_defects` 用**合成输入**自证：边界规则（`a.b` 不得被 `a.bc` 里的子串命中，坑 #333）、动态前缀、注释不是消费者、空语料下全部不可达。**射程**：门禁钉的是**清单**，**不删键**（缩清单——可安全删的是 `dup-sibling`/`zero-mock`/`composite`/`rename` 四类共 23 条——是独立后续轮）。⚠️ 动态前缀规则在**今天的真语料上是冗余的**（`share.day.1..7` 同时被 `index.html:305-311` 的周几芯片静态绑定 ⇒ 删掉 `app.js` 那段拼接，该家族仍可达、门禁照绿）；它是为**未来**的动态家族准备的，牙齿由合成输入证明。

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
  3. **边界要收拾的不只是 `#app`**（C2171）。`#app` 之外的浮层（帮助面板 / 聊天弹窗 / 首启引导）是它的**兄弟节点** ⇒ `$("#app").classList.add("hidden")` **不会**连带隐藏它们。`exitGuest()` 因此还要调 `resetSessionOverlays()`，把每个浮层交回它自己的关闭器（`toggleHelp(false)` / `closeChat()` / `closeTour()`）。元素集合由 `state_gate::the_identity_boundary_closes_the_panels_outside_the_app` 与 `ui/index.html` **派生比对**（`#app` 起始行之后的**顶行**元素、其 `class` 含独立 token `hidden`），**零豁免清单**。⚠️ **射程**：门禁是**词法**的 —— 它证明「每个派生元素各有一个宣称要隐藏它的关闭器落在边界的调用闭包内」，**不**证明运行期屏幕上真的隐藏了（那是 `c2171-probe.js` 的射程，仓内 CI 无 JS 运行器）；已知盲区＝只认顶行（列 0）元素、只认字面量 `"#<id>"`、`classList.toggle("hidden", false)` 这种带第二布尔实参的隐藏不在判别式内。
  4. **边界清的不只是槽**（C2170）。`Live` 之外还有**模块级**的会话状态：交易视图的 `txTable`（`sort` / `filters` / `page` / `pageSize`）与 `txRange` / `txCustomStart` / `txCustomEnd` 都是模块级的，其中 `txTable.loadedPage` / `loadedPageSize` / `loadedQuerySig` 是载荷的**有效性证据** —— 证据属于载荷：载荷被清空而证据留下，守卫（`txQuerySig()` 比对）就会认一份**不存在**的载荷为「已加载」，下一位用户的首帧因此是**空表**（服务端按 `offset=(page-1)*page_size` 返回 `items: []`，而 `total` 照旧非零）且**不自愈**。`resetSessionCaches()` 因此还要调 `resetTxView()`，复位到**声明处的默认值**（含 `txTable.pageSize = 10` —— 清成 `undefined` 会让 `Math.max(1, txTable.pageSize || 10)` 的兜底把每页退化到 1 行）。名字集合由 `state_gate::the_identity_boundary_resets_the_transaction_view_state` **派生**（`txTable` 字面量的字段 + `txTable.loaded*` + `txQuerySig()` 读到的模块级 `let`），**零手抄名册**；证据的写者只允许装载器与边界闭包——否则在 `renderTransactions()` 里清会把守卫每次渲染都重新武装成**请求风暴**（C2146 同形）。
- **修前的两张脸**（jsdom 启真 `index.html` + 四脚本、只 stub `fetch`，驱动真导航 / 真登出 / 真登录表单）：甲用完仪表盘登出、乙登录后打开钱包 —— `#wallet-forever`（「永久点数」）显示的是**甲的** `Live.wallet.balance`（实测 `4,242.42424`，乙应为 `7.5`），**且永不自愈**：`renderView("wallet")` 当时是八个分支里**唯一**只 `renderWallet()` 的，缓存不被清就再也没人刷新它。同一根因的**瞬态**脸：乙落在仪表盘时，`renderView` 先**同步**用甲的缓存渲染 `#dash-stats`（冻结乙自己的 `/api/wallet` 即可读到：本月用量是甲的 `12,345.6789`）。
- **改法两半，缺一留红**：① `resetSessionCaches()`（派生式）在 `exitGuest()` 与 `loadSession()` 开头各调一次；② 钱包分支补上自己的 loader `loadWallet()`（镜像 `loadDashboard` 的尾段：`refreshWallet()` → `renderWallet()`）。② 顺带修掉**单会话**下的口径错 —— 此前登录后直接进钱包，`Live.wallet` 永远是 `null`，单元格只能回落到 `D.USER.balance`（= `available` = 永久 + 当日赠送）冒充「永久」。
- **CI 覆盖**：`src/state_gate.rs` 再加两条 —— `the_identity_boundaries_drop_every_session_cache`（清空必须含 `Object.keys(Live)`；体内**不得**出现逐个槽的赋值，否则就是第二份名册；`loadSession` 与 `exitGuest` 都必须调用它）与 `the_view_router_renders_and_loads_in_every_branch`（`renderView` 每个分支行都既含 `render` 又含 `load`）。两条都带**提取器自证**与**合成输入**（手抄名册 / 缺 loader 的分支必须变红）。
- **冒烟测试注意**：① 夹具必须让两个账号的钱包**可区分**，且 **`available ≠ balance`**（有当日赠送）—— 否则「拿到了自己的永久余额」与「回落到 available」不可区分；② 冻结点要**精确**：`loadSession` 自己的 `/api/wallet` 必须放行（否则会话建立就卡住），只扣住**仪表盘刷新**那一次（按序放行第 1 个、冻结第 2 个）；③ 隔离瞬态脸时，乙的**落点**必须是仪表盘 —— 若乙落在钱包，钱包的新 loader 会在登录过程中就把共享的 `Live.wallet` 刷新掉，瞬态脸**看不见**（这正是「只加 loader 不清缓存」这条竞争修法能让探针全绿的原因）；④ 断言按**读到的 DOM 文本**，不要读 `Live` 内部（探针可临时注入 `window.__Live = Live` 仅用于**诊断**）。

## 渲染谁就装载谁：一个视图可能渲染**别的视图的槽**（C2135）

- **分支名不等于数据来源**。`renderView` 的每个分支 `load` 的必须是**它的渲染闭包真正读到的那些槽**——而渲染闭包会读到别人的槽。C2135 实测：`#month-changes`（钱包视图）与 `#dash-month-changes`（仪表盘）由**同一个** `renderMonthChanges()` 绘制，两者都读 `Live.dashboard`，而该槽此前只有仪表盘的 `loadDashboard()` 会写 ⇒ 钱包分支只调 `loadWallet()`（它只刷 `Live.wallet`）时，**会话在钱包视图上建立**（hash `#/wallet` 后登录；在钱包页登出再登录）那一格就永远是空的。
- **不变量**：对每个槽 `S`、每个 `renderView` 分支 `B` —— 若 `B` 的**渲染闭包**（`render…` 的传递调用集）里有人读 `Live.S`，则 `B` 的 **loader 闭包**（`load…` 的传递调用集 ∪ 会话级 `loadSession` 的闭包）里必须有人写 `Live.S`。`models` / `publicUrl` 是会话级数据（`loadSession` 装载、各视图共用）⇒ 把 `loadSession` 的闭包计入写者之后，**无需任何豁免清单**。
- **修法＝共享槽一个写者，装载事由每个渲染它的视图各做一次**：槽的写入收进 `refreshDashboard()`（`Live.dashboard` 的唯一写者，C2131 的纪律），`loadDashboard()` 与 `loadWallet()` 各 `await refreshDashboard();`。**不要让渲染函数自己去拉数据**（渲染保持纯同步；「先同步渲染缓存、再异步拉取」只允许发生在 `renderView` 的分支里）。
- **修前的脸**（jsdom 启真 `index.html` + 四脚本、只 stub `fetch`、驱动**真登录表单**）：`#/wallet` 页面上登录 ⇒ `#month-changes` 印「本月暂无变动」+ 净变化 `0`，而同一份 `/api/dashboard` 载荷在仪表盘上渲染正确（`-7.5` / 各类型行齐全），且**永不自愈**。**带 token 刷新看不到** —— boot 在 `DOMContentLoaded` 里**无条件** `renderView("dashboard")`，顺手就把槽装好了（这正是它长期潜伏的原因）。
- **CI 覆盖**：`src/state_gate.rs::every_view_branch_loads_each_slot_its_renderer_reads`（传递闭包 + 会话级写者；改前树**恰好**红在钱包那一支）。判别式自带牙齿对照：`Live.dashboardTrend` 不得被当成 `Live.dashboard`（标识符边界）、`//` 与 `/* */` 注释里的 `Live.x` 不得造出幻影读点、只写自己槽的 loader 必须判红、经 `refreshShared()` 间接写入必须判绿。**射程**：槽宇宙 = `Live` 字面量 ∪ 代码里出现过的 `Live.<名>`。⚠️ `Live` 字面量**漏登记**的槽（`Live.dashboardTrend` 未在字面量里声明）只是「字面量是不是唯一真源」的可读性问题 —— **不影响清缓存**：`Live.x = v` 会**新建**一个 own enumerable 属性，调用时的 `Object.keys(Live)` 就包含它（C2136 仪器实测：写后登出 ⇒ 该槽为 `null`；本节的旧说法「派生名册清不到它」已被证伪）。

## boot 不渲染视图：数据只在「会话已建立 + 它是当前目的地」时装载（C2136）

- **`renderView` 只有一个合法触发点：`switchView`**（＝当前目的地）。它做的是「先同步渲染缓存、再（登录时）异步装载」（见上节），所以任何**在会话建立之前**触发的 `renderView` 都必然:先按旧的/空的状态渲染一个**不在屏幕上**的视图，并在 `loggedIn()` 已为 true 时把它整套查询发出去。
- **修前的形状**（`app.js` 的 `DOMContentLoaded`）：`renderView("dashboard")` 被**无条件**调用，而它跑在 `restoreSession()` 之前 ⇒ 带 token 刷新时：
  1. **会话还不存在**，仪表盘那套就已经发了 —— 实测 `log[0] = GET /api/wallet`，会话请求 `/api/me` 才排第 2；
  2. 这些响应随后被 `loadSession()` 的 `resetSessionCaches()` **全部作废**；
  3. `enterApp() → switchView(目的地)` 再装一遍 ⇒ 一次 boot 里仪表盘那套查询**各发两次**（实测 14 个请求：`/api/dashboard`、趋势 `type=all&bucket=day`、`page=1&page_size=1`、`/api/sharings` 各 2 次，`/api/wallet` 3 次）；
  4. **目的地不是仪表盘也照拉**（刷新在 `#/transactions` 时仍白拉仪表盘那套 5 次）；
  5. **过期 token 最刺眼**：先发出去的 6 个请求各拿一次 401，`__atpLogout()` 被调用 **6** 次（`TOAST_MAX = 3`，用户看到 3 条一模一样的「登录已过期，请重新登录」）。
- **修法**：boot 只搭外壳（`renderNav()` / `bindEvents()` / 余额占位），**不渲染也不装载任何视图**；视图一律由 `switchView` 按当前目的地渲染装载（登录后由 `enterApp`、游客由 `enterGuest` 触发）。带 token 刷新不会白屏：`restoreSession()` 成功即 `enterApp()`，非 401 失败也照常 `enterApp()`（C2124），401 则 `api.js` 已回登录页。
- **不变量（`src/state_gate.rs::the_boot_handler_touches_no_view`）**：① boot 处理器体内**不得**调用视图层的「渲染器 / 装载器」（视图层名册**派生自** `renderView` 的各分支，不写第二份名册）；② `renderView(...)` 只许以**当前目的地**为实参（全仓唯一合法的一处是语言切换监听器里的 `renderView(activeView)`）；③ `switchView` **必须仍调用** `renderView`（防止矫枉过正 —— 把 boot 那句删掉之后顺手清空 `renderView` 的调用点，就得到一个什么都不渲染的空壳）。
- **为什么必须静态钉**：`renderDashboard()` 单独留在 boot（只渲染不装载）、或 `if (!api.getToken()) renderView("dashboard")`，都能让请求日志／通知计数全绿 —— 三者都只是**症状消失**。C2136 的 A/B：三种「最小改法」变体在探针上**全绿**，而门禁按形状收窄到「boot 不碰视图层」。
- **冒烟测试注意**（`tmp/c2136_probe.js`）：① 判「仪表盘那套」时要连**查询形状**一起比 —— 交易视图自己也拉 `/api/transactions/trend`（`bucket=hour`），只按路径匹配会把别人的请求算进来（假红）；② 参数匹配要锚定（`page_size=1` 不能用裸子串，否则命中 `page_size=10`）；③ 计数不要数 DOM：`TOAST_MAX` 会把 6 条截成 3 条，用 `Object.defineProperty(window, "__atpLogout", { set })` 截住赋值再包装计数；④ 阴性对照腿＝**无 token** 的 boot（必须 0 个视图数据请求）。


## 模型身份：`provider/model`，不是数组下标（C2138）

- **位置不是身份**。`modelsToView()` 曾给每行发 `id: i`（数组下标），而「最近使用」把它**存进 `localStorage`**（`atp-recent-models`）⇒ 这个身份跨了渲染 / 跨了会话 / 跨了**数组**：
  - **跨数组**：游客兜底表 `data.js > MARKET` 是**另一张表**（7 行、id `1..7`、顺序与长度都不同），只是**数字上看起来**是同一个空间。实测：登录态用了 `xai/grok-4.6`（下标 5）→ 登出进游客市场，芯片写成 `google/gemini-3.1-pro`；下标 0（登录态第一行）在 1-based 的游客表里查无此号 ⇒ 芯片**整条消失**。
  - **跨渲染**：`/api/models` 按 `provider, model` 排序 ⇒ 上架 / 下架 / 改名任何一个模型，后面所有下标整体**位移**。实测：管理员加一个排在前面的模型后，芯片写成 `moonshot/kimi-k3`，而**点开那枚芯片打开的对话也是 kimi-k3**（显示上的错升级成动作上的错）。
- **约定**：**跨渲染 / 跨会话 / 跨数组的模型身份一律走 `modelKey(m)`**（= `provider + "/" + model`）；**数组下标只在生成本次渲染的那个数组里有意义**，不得进入视图行（`modelsToView` 不再发 `id`）、不得进 DOM 的 `data-*`、不得进 `localStorage`。三处身份载体（市场行展开 `data-mk-expand`、「使用」`data-use-model`、最近使用芯片 `data-recent-model`）都由 `esc(modelKey(m))` 产出，点击侧**原样传递**（不得再用 `Number(...)` 把身份串转回数字）。
- **存储层只认身份串**：`getRecentKeys()` 只接受 `typeof x === "string" && x.indexOf("/") > 0`；**旧版本存下来的下标无法被诚实地还原成某个模型**，按空处理、一次性丢弃（刻意的 —— 把它「尽力翻译」成某个模型正是本缺陷本身）。解析一律 `find((x) => modelKey(x) === key)`（`renderRecent` / `openChat` / `consumeModel`）。
- **不变量（`src/state_gate.rs::the_model_row_identity_is_the_model_not_its_position`）**：① `modelsToView` 的 `.map(` 回调**只许一个形参**、行对象里不得声明字段 `id`；② 三处 `data-*` 必须由 `modelKey(` 产出、点击侧不得出现 `Number(`；③ `modelKey` 全仓**只有一处定义**，体内同时提到 `provider` 与 `model`、从不提到 `id`；④ `markRecentUsed(...)` 的每个调用点都在写 `modelKey(...)`。
- **判别式注意**：渲染侧与读取侧**长得像**（`data-x="…"` vs `querySelector('[data-x="' + id + '"]')`）⇒ `renders_attr` 必须**同时**要求「属性后跟 `=`」**且**「这一行不是选择器查询」，否则消费者会被算成渲染点（门禁第一版正是这样假红的）。
- **冒烟测试注意**（`tmp/c2138_probe.js`）：① 控制腿要落在**这条路径对该身份本来就成立**的会话里 —— 游客点「最近使用」芯片是被刻意拦住的（`chat.login.need`），把它写成控制腿会恒红（坑 #316）；② 断言必须**逐条目**判「叫的是用过的那个模型」（两个身份空间部分重叠，重叠的那一项看起来是对的，坑 #317）；③ 只有**目录位移**那条腿拒得掉「手工把游客表 id 对齐今日目录」这种竞争修法 —— 它骗得过游客市场那张脸（坑 #318）。

## 后端自造的显示文案：分界线在「数据字段 / 文案字段」（C2133）

- **咽喉只覆盖文案字段**：`api.js` 把后端 `error` 字段整串交给 `I18n.mapErr()`（词表 `ERR_MAP`），所以**错误文案**有兜底（C2129）。但**响应数据字段**是前端原样渲染的 —— 后端在数据字段里自造一句中文，`en` 界面上就是中文，而 `cargo test` 全绿。
- **不变量**：**显示文案归语言包，后端只回传数据或语言中性标记**。数据字段里的值要么来自 config / 库 / 用户输入（原值透传），要么是语言中性的机器值（空串 / `null` / 枚举名）。中文标签只允许出现在 `error` 字段（有词表）和邮件正文（无 locale 机制、中英双语）里。
- **修前两处可达**（都在 `en` 界面直接显示中文）：
  1. `src/routes/admin.rs` 的按部门聚合：`COALESCE(d.name, '（未分配）')` → `app.js` 的 `#usage-dept` 只过 `esc()`。任何 `dept_id IS NULL` 且本月有用量的用户都会让这行出现 —— 而**同一张页面**的成员表早就在用 `T("common.unassigned")`（同一个键、零新增）。
  2. `src/gateway.rs` 的 `/api/plans`：config 未写 `name` 时后端按 `type` 自造 `API（按量）` / `Token Plan` / `Coding Plan`；而 `config.example.toml` 与 `config.toml` 的 `[[plans]]` **全都**不写 `name` ⇒ 恒触发，上架表单的 Plan 下拉与上架成功的 toast 都读它。
- **改法**：① 无部门的桶改用**空串**（与同一个 handler 里 `users[].dept_name` 的 `COALESCE(d.name, '')` 同口径），前端 `d.name || T("common.unassigned")` 兜底；② `/api/plans` 的 `name` 改为 config **原值**（未配置即空串），前端新增 `planLabel(pl)`：有 `name` 用原文，否则按 `type` 取新键 `share.planName.paygo|token|coding`（下拉与 toast 共用这一个函数）；③ Plan 下拉框的重建判据从「建过没有」（一次性 `dataset.init`）改成**数据源**（`dataset.plansSrc`）——登录后首次渲染时 `/api/plans` 还在路上，兜底表会先建一次，一次性守卫会让它**赢到底**（实测：真实清单回来后下拉框不再重建，`en` 界面上仍是兜底表里的中文名）；级联填充函数也改成**调用时**读数据源，否则只登记一次的监听器会永远指着那份兜底表。
- **CI 覆盖**：`src/i18n_pack.rs` 三条 —— ① `backend_data_fields_are_language_neutral`：扫全部 `src/**/*.rs`，提取「以 `json!` **数据**字段（key ≠ `error`）交付的中文字面量」，其集合必须**恰好等于**已裁定豁免清单（两侧都有牙：新增一处变红、删掉豁免项也变红；清单只给 1 个名额 —— 它是**日落清单**，不是注册表，扩容要显式改那个数）。提取器必须能穿透 `let name = … "中文" …; json!({ "name": name })` 这层间接（计划名就是这种写法；不穿透就漏掉一半的类），并跳过嵌套 `json!`、跳过 `#[cfg(test)]`；② `backend_neutral_data_labels_are_localized_in_the_client`：两个渲染点必须**有**本地化兜底（`#usage-dept` 的 `barRow` 首参含 `T(`、`planLabel` 体内含 `pl.name` 与 `T("share.planName.…")`），且下拉框必须按**数据源**重建（不得退回一次性守卫）；③ `GET /api/admin/usage` 的运行期契约：无部门用户有用量时，桶名不得含 CJK（`src/routes/mod.rs` 的 router 测试）。
- **豁免清单为什么存在**：`src/routes/mod.rs` 注册接口的 `"name": name` 里那个默认用户名（`email.split('@').next().unwrap_or(...)`）是**用户数据**的默认值（同 `db.rs` 种子里的 `'管理员'`），不是后端自造的显示标签 —— 且 `split().next()` 恒 `Some`，该默认值不可达。豁免项带**理由**、且与提取结果**等价**（`==`，不是 `⊆`），所以它不会腐烂。
- **冒烟测试（`tmp/c2133_probe.js`，jsdom 真 `index.html` + 四脚本、只 stub `fetch`、强制 `atp_lang=en`）**：腿 PRE（`origin/main` 的前端 + 老后端那两份载荷）红 `{B1,B2}`；腿 POST（工作树 + 语言中性载荷）10/10 绿；两条**竞争修法**腿各红在**不相交**的一半 —— 「只加 `planLabel`、保留一次性守卫」红 `{B2b,D2}`（下拉框里仍是兜底表的中文名）、「改前前端 + 老载荷」红 `{B1,B2,D1,E2}`（部门半张脸）。控制腿：真实部门名 `研发` 与 config 里配了 `name` 的 plan **原样透传**（本轴只约束**自造**标签）。
- **同轴未修（记录，勿顺手带上）**：`ui/js/data.js` 的 `PLANS[].name` 是**客户端**自带的一份显示标签（`API（按量）`、`Kimi Code 会员`…）。`/api/plans` 拉取**失败**时前端落回这张表，`en` 界面上就会显示它的中文名（成功路径已由 ③ 的重建判据修好）。它与本 PR 的**生产者**不同（客户端数据表 vs 后端响应字段），且需要先裁定「品牌名放行 / 通用名取语言包」的边界 —— 对照组：`provLabel()` 是**正确**写法（`I18n.lang === "zh"` 才用 `PROVIDER_LABELS`，`en` 直接回 id），`data.js` 的 PLANS 名可以照抄这个形状。
## 数据源由**会话状态**决定，不由「数据到没到」决定（C2140）

`ui/js/data.js` 开头写着它的契约（rant 2026-08-19T15:54:06）：

> 保留对象仅用于：游客市场渲染（`MARKET`）与上架表单兜底（`MODELS`/`PLANS`/`PROVIDERS`/`PROVIDER_LABELS`）；
> **登录态一律使用后端 API，绝不读取下列 mock 数据**。

契约里的判别式是**会话**（`loggedIn()`），不是**数据在不在**。写成 `Live.models ? 活目录 : D.某表`
看着像「有就用真的、没有就用兜底」，实际是「**登录态目录缺席时（请求失败 / 超时 / 还没到）拿游客表
当自己的数据**」。市场面曾有三个消费者这么写（每个都有注释复述着规则）：

- `renderMarketplace()` 的厂商下拉：`… : D.PROVIDERS` ⇒ 下拉里出现**本部署没有的厂商**（游客表 10 个，
  实例目录 6 行 5 个厂商），选了只会筛出 0 行；
- `renderRecent()` 的「最近使用」芯片：`… : D.MARKET` ⇒ 芯片画的是**游客市场**的模型名，而它的唯一
  消费者 `openChat()` 自带 `if (loggedIn() && !Live.models) … return` 守卫 ⇒ 这枚芯片**点开只会提示
  加载失败**（它在屏幕上是个会说谎的按钮）；
- `renderNav()` 的市场徽标：`… : D.MODELS.length` ⇒ 显示 **13**（上架表单的价格镜像行数），而实例目录
  6 行、游客市场 7 行 —— 13 是**第三个数**。

**指纹**：同一个 `renderMarketplace` 里，往下十来行的**兄弟**三元式把规则实现对了
（`let list = … (loggedIn() ? null : D.MARKET)`，并附注释「绝不 fallback D.MARKET」）—— 一行对、一行错、
判别式只差一个 `loggedIn()` ⇒ 漏修，不是取舍（与 C2128/C2139 同型：**先扫兄弟行，再判是不是漂移**）。

**约定**：

1. **市场面的数据源只有一处真源**：`marketRows()`（`null` = 登录态目录缺席）与 `marketProviders()`。
   列表 / 厂商下拉 / 最近使用 / 对话 / 徽标**全部**走它们 —— 表只在这一处读，`loggedIn()` 判别式只在这一处写。
   多一处读取就多一处「按数据到没到」的机会（本轮的三个缺陷正是这么长出来的）。
2. **登录态目录缺席就诚实地空着**：`marketRows()` 返回 `null` ⇒ 列表走 `loadErrorRow` + 重试（C2072 的
   降级态）、芯片不渲染、徽标不显示数字。空着不是缺陷 —— 假数据才是。
3. **`MODELS` / `PLANS` 是「上架表单的兜底表」，不是市场目录**（13 行 vs 7 行、顺序也不同）：
   二者**不得在同一条表达式里互为兜底**。
4. **游客路径是设计的一部分，不许顺手删**：`data.js` 的 `MARKET` / `PROVIDERS` 必须仍然存在且**被读**。
5. 控件按数据源重建时，`dataset` 标记要**区分三种状态**（`live` / `none` / `mock`）—— 把「登录态没数据」
   和「游客」记成同一个值，会让目录到达后控件不再重建（或每次重建）。

**CI 覆盖（`src/state_gate.rs::market_tables_follow_the_session_not_the_data`，三条规则各有独立的牙，
A/B 里各自有互不相交的红集）**：

1. **每条读市场表（`D.MARKET` / `D.PROVIDERS`）的代码行都必须自己按会话状态分支**（行内出现 `loggedIn` /
   `isGuest`）—— 判别式只写在这些读取点上，**零豁免清单**。注释里的提及不算：扫描器剔除 `//` 起始行、
   多行 `/* … */` 块内部的整行、行内成对的 `/* … */` 片段（坑 #309：只剥 `//` 的扫描器会被块注释里的
   字面标识符绊倒 —— 而本门禁的题眼正是「解释性文字会提到表名」）。
2. **同一行不得既读市场表又读上架表单表**（`D.MODELS` / `D.PLANS`）：徽标那一行正是这样把两套数据当成
   彼此的兜底。
3. **每张市场表只有一处读者**（`marketRows` / `marketProviders`）：既防「又多一处按数据分支的读取」，
   也防**过度纠正**——「干脆把 mock 全删掉」（`marketRows()` 返回 `null` 给所有人）会让某张表**没有**读者，
   游客市场随之变空；这条与探针的游客腿是同一件事的两侧（探针钉行为、这条钉形状）。

**为什么必须静态钉**：这三处不是「某一次渲染对不对」，而是**每条消费路径的形状** —— DOM 探针只看得到
当前那一次渲染出来的下拉 / 芯片 / 徽标。A/B 实测（每条腿都跑**两支**仪器）：

| 变体 | 门禁 | 探针 |
|---|---|---|
| 改前树（`git show HEAD:ui/js/app.js`） | 红 · 规则 1 | 红 `{A2,A3,A4}` |
| 竞争修法①：只修探针看得见的两张脸，徽标仍读 `D.MODELS` | 红 · 规则 2 | 红 `{A4}` |
| 竞争修法②：多一处**带会话判别**的读取（行为完全相同） | 红 · 规则 3（两个读者） | **绿** |
| 竞争修法③：过度纠正 —— 兜底表从 helper 里删掉 | 红 · 规则 3（没有读者） | 红 `{C2,C3}` |
| 行内**提到**会话判别、仍然用游客表（`isGuest ? D.PROVIDERS : D.PROVIDERS`） | **绿** | 红 `{A2}` |
| 修复树 | 绿 | 绿 14/14 |

⇒ 两条仪器各管一半：**门禁管形状**（能拒掉探针全绿的第二处读取），**探针管值**（能拒掉「形状对、值仍错」
的撒谎行）。前者比探针更严（实测第 3 行），后者比门禁更严（第 5 行）—— 如实记录，不假装一支够用。

**冒烟测试注意**（`tmp/c2140_probe.js`，jsdom 启真 `index.html` + 四脚本、只 stub 并记账 `fetch`）：

- 「最近使用」芯片那张脸**需要夹具**：芯片 key 要同时存在于游客表与活目录（`deepseek/deepseek-flash`），
  否则守卫为真、芯片不渲染，缺陷看不见 —— 因此必须配一条**空 store 的对照腿**（`D1`）证明「红是因为夹具，
  不是机制天然如此」。
- 登录态腿里 `Live.models` 要真的**失败**（500），否则走的是另一条路径；且要有 `A1` 这条「列表已是诚实
  降级态」的对照腿 —— 缺陷不在列表，在旁边三张脸。
- 必须有**游客腿**（真点 `#guest-browse-btn`，`C0`–`C3`）作为对照组：游客下拉 = 游客表厂商、芯片 =
  游客市场模型、徽标 = `MARKET.length` —— 「把兜底表删掉/清空」式修法会在这里变红。

## 「总余额」卡：取值必须折进副标题具名的每一项（C2142）

管理视图（`renderAdmin` 的成员支）那张「总余额 / Total balance」卡的副标题本身就是**口径的陈述**：

```
stat(T("admin.emp.stats.total"), D.fmt(total) + " " + T("common.points"), T("admin.emp.stats.total.sub"))
                                          ↑ 合计                    ↑ "余额 + 赠送" / "balance + gift"
```

改前 `total = users.reduce((a, u) => a + (u.balance || 0), 0)` **只加永久余额** ⇒ 卡片比它正下方
「可用」列（`admin.emp.col.avail` = `balance + gift_balance`）的和**少掉全部赠送额**，而屏幕上写着的
公式说它加了。

**哪个方向才对，由产品自己的定义钉死**（不是偏好，所以「把副标题改成『余额』让两边对上」不是修法）：

1. `src/routes/wallet.rs` 的 `available = balance + gift_balance`；赠送点数是**可花的、会过期的真钱**
   （`gift.rs` 的过期清扫真的把它们从账户里划走）；
2. 用户自己看到的那张「余额」（侧栏 `#side-balance` / 仪表盘 `dash.balance`）取的就是 `available`
   （`loadSession` 里 `D.USER.balance = w.available`）⇒ 「总余额」= **Σ 成员的可用额**；
3. 同一张表的表注（`admin.emp.list.sub`）也写着「余额 / 赠送 / 可用（永久点数 + 每日赠送）」。

**约定**：

1. **一个汇总只能有一个口径，且必须与它自己的副标题同源**：叠加了成员合计的卡片，其取值表达式的
   **传递闭包**里必须读到副标题具名的**每一项**（抽成 helper 也行，规则只钉「读到了」）。
2. **副标题是合同，不许为了「对上」而删**：两包都必须同时具名两项；让卡片与一个被削弱的承诺一致，
   等于把这张卡「在说什么」抹掉。
3. **只约束这一条合计**：同函数里另有按月 token 的 reduce（用量报表），它不该被这条规则咬到
   （门禁对「只咬这一条」有对照断言）。

**CI 覆盖**（`src/state_gate.rs::the_admin_total_balance_card_sums_what_its_caption_names`，三条规则
各有独立的牙：卡片唯一且取值是标识符 / 合计闭包必须读到 `balance` **与** `gift_balance` 两项 / 两包副标题仍具名两项），附
`stat_value_argument`（嵌了调用与字符串的取值整段取出）、`assignment_statement`（跨行合计按圆括号配平整段取出，
自证不吞下一条语句）与 `caption_names_both_components`（「余额 + 赠送」「balance + gift」放行，「余额」「balance」判红）
的判别式自证。⚠️ 两项都钉是刻意的：只钉 `gift_balance` 会放行「合计里只剩赠送」这种把轴修反的写法；而用
`contains("balance")` 又会被 `gift_balance` 这个后缀满足（哑子串匹配）⇒ 判别式按**标识符 token** 比。

**为什么 CI 用静态门禁**：CI 里没有 JS 运行器，`cargo test` 是唯一能长期守住的关口；探针
（`tmp/c2142_probe.js`）只用于本地证明**方向**（卡片值 == 同视图「可用」列的和；每行「可用」单元 ==
`balance + gift_balance`），并拒掉竞争修法。

## 会话余额是一个事实：一个来源，且取的是**可花额**那一半（C2145）

`D.USER.balance` 是「我还剩多少可花点数」这个事实在客户端的载体 —— 侧栏 `#side-balance`、钱包页、
聊天余额都印它。它的定义由产品给出：

```
wallet.rs:  "available": balance + gift_balance      // 赠送是可花、会过期的真钱（gift.rs 清扫真的划走）
```

**改前的形状**：这个数字有多个写者，其中 `inlineOpsTopup`（运营者给**自己**充值后的自刷新）
**自己又取了一次钱包载荷**，且只读 `w.balance`（永久额那一半）：

```
try { const w = await api.get("/api/wallet"); if (w) D.USER.balance = w.balance; … } catch (e) {}
```

`gift::ensure_daily_gift` 挂在**每个已认证请求**与 `GET /api/wallet` 上、**与角色无关** ⇒
运营者/管理员恒有 `gift_balance > 0` ⇒ 给自己充值后侧栏**立刻少掉当天赠送额**，且永不自愈
（`loadSession` 只在会话建立时跑）；同一条路径还**绕过缓存槽的唯一写者**，`Live.wallet` 停在充值前的
载荷。实测 `balance=100 + gift=1`、充值 +100 ⇒ 屏幕 **200**、真值 **201**，`Live.wallet.available` 仍是 **101**。

**约定**：

1. **会话余额的绝对值只能取 `available`**：每一处 `D.USER.balance = …` 要么是**相对量**
   （`D.USER.balance ± x`，充值/消费的演示路径）、要么是错误兜底的字面 `0`，要么取 `available`。
2. **取钱包载荷的函数恰为 `{loadSession, refreshWallet}`**：`loadSession` 在 boot/登录时装上会话
   （此时还没有任何视图 loader），`refreshWallet` 是缓存槽 `Live.wallet` 的**唯一写者**。
   任何**第三处**取载荷的代码都同时犯错：自造「同一事实的第二个来源」却不更新缓存 ⇒ 数字与缓存漂移。
   ⇒ 需要「刷新自己的余额」的调用点一律调 **`refreshWallet()`**，不要自己取数。
3. **为「让屏幕上的数字对上」而只改字段不是修法**：保留那次多余的取数、把 `w.balance` 换成
   `w.available`，屏幕上数字对了，缓存仍是旧的（这正是被拒的竞争修法）。

**CI 覆盖**（`src/state_gate.rs::the_session_balance_has_one_source_and_it_is_the_spendable_half`，
三条规则各有独立的牙：赋值必须取可花额 / 取载荷的函数集合恰为那两处 / `Live.wallet` 的唯一写者仍是
`refreshWallet`），附 `session_balance_rhs`（只认重新绑定，`+=` / `==` / `!==` 都不是）、
`session_balance_rhs_is_relative` 与 `fetches_wallet_payload`（须同时有 `api.get(` 与端点 —— `Live`
字面量里那行 `wallet: null, // GET /api/wallet` 是代码 + 尾注释，只按端点匹配会幻影红）的判别式自证。

**为什么 CI 用静态门禁**：规则 2 钉的是「同一事实只有一个来源」这个**形状**，而 DOM 探针只能证明
「屏幕上数字对」—— 竞争修法在探针下全绿（`tmp/c2145_probe.js` 实测：修复树 9/9、改前树恰 `A2`/`A3`
两腿红、竞争者 8/9 被 `A3` 拒绝）。CI 里没有 JS 运行器，`cargo test` 是唯一能长期守住的关口。

## 运营成员表的「余额」列 = 可花额 = 用户自己所见（C2173）

- **一个词一个数**：全站把**不带限定的**「点数余额 / Points balance」绑给 `available = balance + gift_balance`（`common.balance` / `dash.balance` / `wallet.balance` 三处），把「**永久**点数 / Permanent points」绑给 `balance`（`wallet.forever` / `admin.emp.col.perm`）。运营成员表的列头用的正是**前者**（`ops.users.col.balance` zh「余额（点数）」/ en「Balance (pts)」），而取值只印 `u.balance` ⇒ 用户自己看 101、运营者看 100，差的正是「赠送」那一半（可花、会过期、真划走，`gift.rs` 清扫时真扣）。
- **修法**：取值折进 `gift_balance`，**与兄弟表逐字同形** —— `admin.emp.col.avail` 那一格早就是 `D.fmt((u.balance || 0) + (u.gift_balance || 0))`（同一个 PR 家族里写对了的范本）。**零新 i18n 键**。
- **CI 覆盖**：`src/state_gate.rs::the_ops_members_balance_cell_is_the_half_its_caption_names`（三规则：① 取值表达式按**标识符 token** 同时读到 `balance` 与 `gift_balance` ② 两包列头须落在**可花族**（标记由 `wallet.balance` 的包值派生）③ 反向：`admin.emp.col.perm` 仍只读 `balance`）。
- ⚠️ **射程**：门禁是**词法**的 —— 它证明取值**读到**两个字段、名字落在可花族，**不证明算术是 `+`**；行为由仪器 `c2173_probe.js` 钉（19 腿；甲/乙两棵合法树全绿，三条「半个答案 / 化妆 / 自相矛盾」被拒）。


## 表单控件的 property 式 `disabled`：必须有一个在**回收路径**上的清除点（R139）

- **唯一真源**：`ui/js/app.js::resetShareAvail()` —— 它把「七个单日 chip 的 `disabled` 清回
  `false`」收成一处；`showShareForm()`（每次重开表单）与 `afterOk()`（成功上架后回收）各调一次。
- **为什么必须有**：`HTMLFormElement.reset()` 只还原**值 / 勾选态**到默认值，**不清 `disabled`
  property**；而「每天」快捷勾选正是用 **property** 方式禁用单日 chip 的
  （`cb.disabled = allCb.checked`，从没进过 HTML 属性）。于是勾着「每天」成功上架一次之后，
  七枚 chip 是「未勾选 + 禁用」：点任何一天都无反应，表单卡片是静态 HTML ⇒ **整会话不自愈**。
- **写法约定**：任何「用 property 把一个**表单控件**禁成非 `false`」的写点，都必须在
  **回收闭包**里有一处同频道的 `.disabled = false`。回收闭包 = 调用 `form.reset()` 的那个函数的
  调用闭包 ∪ 打开表单卡片那个函数的闭包（两个根都从 `index.html` 派生，不写名册）。
- **门禁**：`src/state_gate.rs::a_form_control_disabled_by_the_property_is_cleared_on_the_recycle_path`
  三规则各有独立的牙（控件写者集合非空且频道能派生出 `<form>` / 卡片 id ／闭包里有同频道清除 ／
  回收宿主仍被调用），配 `the_form_recycle_path_scanners_have_teeth` 四条合成输入自证。
  **形状归门禁，事实归探针**（jsdom 驱动真上架表单：未修树恰 `{B1,B2}` 红、修复树 9/9 绿、
  竞争修法 `m_drop`（取消互斥）被 `{C3,C2,A3}` 拒绝）。

## 载荷签名覆盖所有输入，控件变更只有一个重拉触发器（C2146）

交易列表/趋势的请求体由**两份**状态渲染：列筛选（`txTable.filters`）与时间段
（`txRange` / `txCustomStart` / `txCustomEnd`）。而「缓存还新不新」的判据只有一处 ——
`renderTransactions()` 里的三元比对（`loadedPage` / `loadedPageSize` / **载荷签名**），
所以**签名必须覆盖每一个改变请求体的输入**：

```
function txQuerySig() {                      // 只此一处定义「载荷是什么」
  const f = txTable.filters || {};
  const cols = Object.keys(f).sort().map((k) => k + "=" + String(f[k] == null ? "" : f[k])).join("&");
  return cols + "|" + txRange + "|" + txCustomStart + "|" + txCustomEnd;
}
```

**改前的形状**：签名只哈希列筛选，于是三个时间段控件**各自补一次显式 `loadTransactions()`**。
补丁在「守卫也成立」时会并发第二次请求 —— `#tx-range` 处理器**先把页码重置为 1**，用户只要不在
第 1 页，`loadedPage !== page` 就让守卫自己发一次并 `return`，随后那句显式调用再发一次 ⇒
**两份逐字相同的列表请求（趋势请求也两遍）**。第 1 页上只有一次，所以它潜伏至今。

**约定**：

1. **`txQuerySig()` 是「交易载荷由什么决定」的唯一陈述**：新增任何一个会改变请求体的状态，
   都必须折进它；签名只取**状态值**。
2. **⚠️ 签名不得由 `txRangeParams()` 派生**：后者含「now − 窗口」的毫秒时间戳，每次调用都不同
   ⇒ 签名恒变 ⇒ 守卫每次渲染都重拉，**请求风暴**（比原缺陷更坏；探针实测请求数 3→3→2→4→5 递增）。
3. **控件的状态一改只调 `reloadTransactions()`**（它把「页码重置」与「重拉」当作同一个动作）：
   控件的绑定函数体内**不得**出现 `loadTransactions()` —— 守卫已经会按签名/页码决定要不要重拉。
4. **触发器的显式取数只在槽为空时用**（首次进入 / 上次失败时 `renderTransactions()` 只画降级态、不拉取），
   否则它必然与守卫重复拉取。

**CI 覆盖**（`src/state_gate.rs::the_transaction_payload_has_one_signature_and_one_reload_trigger`，
四条规则各有独立的牙：签名读时间段状态且不由 `txRangeParams()` 派生 / 控件绑定函数不自带取数 /
「重置页码 + 直接取数」的函数**恰好一个**且被四个控件共用 / 触发器提到 `!Live.transactions`）。
⚠️ 两个判别式陷阱都已在门禁里自证：`code_body()` **剥注释**（本轮的修法就在控件旁边写着两句提到
`loadTransactions()` 的解释，原文判定会把门禁自己判红）、`mentions_tx_loader()` 按**标识符 token**
比（`reloadTransactions()` **以** `loadTransactions()` 结尾 —— 子串匹配会让规则 2 在每一个修好的树上判红）。
控件归属不写名册：取**文件里第一个**「函数体含该控件字面量」的函数（`bindEvents` 内部还有嵌套函数，
「命中行之前最近声明的函数」会把归属判给那个嵌套函数）。

**为什么 CI 用静态门禁**：请求次数是运行期可观测量，但 CI 里没有 JS 运行器；探针
（`tmp/c2146_probe.js`）只用于本地证明**方向**（改前树恰 `A1`/`B1` 红 = 第 2 页上改时间段发两遍列表 +
两遍趋势），并拒掉最诱人的错修（签名取 `txRangeParams()` ⇒ 请求风暴）。

## 载荷的**收据**由发起它的那次请求写，不由最后落地的响应「问一次现在」（R155）

`loadTransactions()` 在响应落地后写三条**收据**（`txTable.loadedPage` / `loadedPageSize` /
`loadedQuerySig`），`renderTransactions()` 的守卫拿它们跟**现在**的控件比对、不一致就重拉：

```js
const page = Math.max(1, txTable.page || 1);          // ← 发请求前的快照
const pageSize = Math.min(100, …);                    // ← 发请求前的快照
…
const [, trend] = await Promise.all([ … ]);
txTable.loadedPage = page;                            // ← 属于这次请求
txTable.loadedPageSize = pageSize;                    // ← 属于这次请求
txTable.loadedQuerySig = txQuerySig();                // ← 改前：**此刻**的活状态（缺陷所在）
```

1. **收据的右值必须取自发起它的那次请求**：签名在 `await` **之前**捕获（`const reqSig = txQuerySig();`），
   响应落地时**原样盖章**。三条收据描述的是**同一份响应**，其中两条一直是对的写法。
2. **凡「问一次现在」的收据都会自证清白**：守卫比对的也是**现在** ⇒ 印章与投影同源、比对恒成立。
   两次控件变更落在同一个 RTT 内时两个请求同时在飞，被取代的那个不带身份；它若**最后落地**，
   就被采纳并认证为当前 ⇒ 表格**永久**停在用户已经离开的筛选条件的行上，且此后零请求。
   （顺序到达时损害只是短暂的 —— 新的那份落地即纠正；永久的那条需要「被取代者后到」。）
3. **捕获的标识符必须是守卫比较的那个函数的值**：捕获一个常量会让守卫**恒不相等** ⇒ 每次渲染
   都重拉（请求风暴，比原缺陷更坏）；把捕获搬到 `await` **之后**是同形假修（问的仍是「现在的房间」）。

**CI 覆盖**：`src/state_gate.rs::the_receipt_is_written_by_the_request_that_issued_it` 四条规则
（R1 收据右值不得是一次调用 / R2 裸标识符右值须在同函数的第一个 `await` **之前**绑定 /
R3 签名收据捕获的标识符其初始化式必须调用**守卫比较的那个**函数 / R4 反向非空：守卫仍在且仍与
一次调用比较、名册与写入点非空），配 `the_r155_rules_have_teeth`（四个合成变异体各只打翻一条）、
`the_r155_rules_separate_the_variants` 与 `the_r155_extractor_lands_on_real_function_bodies`。

⚠️ **射程**：门禁是**词法**的 —— 它证明收据的右值被捕获、在 `await` 之前、且来自守卫比较的那个
函数，**不**证明屏幕上那一刻的行真的属于当前筛选（那一半归 jsdom 探针的 `A1/A2/A3` 腿）。
两个仪器在**纯代次版**（只丢弃被取代的响应、盖章那行照旧读活状态）上**不同判**：探针接受它
（终态正确、还少发一次请求），门禁**拒**它 —— 「收据的作者」在这条轴上是**同一个主张**，一份收据
要由两个机制分别保证才算真修。这个不对称是**故意**的、可测量的（见上列第三个测试）。

## 交易视图的 Token 数量：一个视图一种拼写，导出的数据列写**精确值**（R158）

一份 token 数量，**修前**有三张脸：

| 面 | 修前实现 | `1500` 的读数 |
|---|---|---|
| 汇总卡（`renderTxSummary`） | `fmtM`：K 档 `Math.round(n / 1000) + "K"` | `"2K"` |
| 表格行（`txsToView`） | `fmtTokens`：K 档 `(n / 1000).toFixed(1)` 去尾零 | `"1.5K"` |
| CSV 导出（`exportTxCsv`） | 直接写**单元格的显示串** | `"1.5K"`（数据列里是文本，不可求和） |

- **唯一真源**：一行数量的拼写只有 `fmtTokens` 一条规则。汇总卡**委派**给它
  （`const fmtM = fmtTokens;`），不再自带 `Math.round` 那条同族实现。
- **导出写精确值**：四个 Token 列读视图模型里的 `<…>Raw` **数字**，换算走单元格悬停
  （`title`）用的 `fmtTokensExact` —— 「悬停所声明的精确值」就是这一层，文件里此前没有任何出口。
- **为什么必须有**：卡片是**它下面那些行**的合计；筛到只剩一行时，卡片与行印的是同一个数却是
  两种拼写（`2K` vs `1.5K`）。而**线上数据永远落在分叉的那一档**（2026-09 实测：47 万行里
  99.92% 的 `tokens` 在 K 档、`max(tokens) = 494 795`，M 档零行）⇒ 「M 档两条规则恰好相同」
  这个唯一的辩解**在真实数据上不可达**。
- **CI 覆盖**：`src/state_gate.rs::the_transactions_view_states_one_token_quantity_one_way`
  四条规则各有独立的牙 —— R1 汇总卡不自带拼写字面量；R2 汇总卡**委派** `fmtTokens`；
  R3 导出四列全部写 `<…>Raw` 且经悬停那层的 helper（一个不漏）；R4（**反向**）四个单元格仍印
  缩写字段（挡「把屏幕降级成精确值、让文件显得对」的过度纠正）。配三条自证：
  `the_r158_roster_is_real`（字段改名即红）、`the_r158_scanners_have_teeth`、
  `the_r158_rules_separate_the_variants`（五个变体，每个变异**只翻它针对的那一条**规则）。
- ⚠️ **射程**：门禁是**词法**的 —— 它证明汇总卡**委派**给 `fmtTokens`（而不是「卡片自己算出来的
  数恰好相等」），也**不**证明 `fmtTokens` 的实现在数值上等于「悬停所声明的那个精确值」；
  后一半由 DOM 仪器在它所跑的树上覆盖（导出 == 悬停值）。**形状归门禁，事实归探针**。

## 设置卡片里的控件：要么接线，要么明确标成惰性（C2148）

「账户 / 通知 / 偏好」三张配置卡里的每个表单控件，只有两种合法状态：

1. **接线**：值真的流进产品代码（或被绑上监听器 —— 按钮没有值可读）；
2. **明确标成惰性**：`readonly` / `disabled`，**且卡里给一句本地化说明**（两包都有该键）。

「看上去能操作、输入却没有任何消费者」不是第三种状态，是缺陷。改前的三张脸：

| 控件 | 它缺的那一半 |
|------|--------------|
| `#settings-nickname` | 可编辑、由真实 `/api/me` 填，但**全仓没有写昵称的后端路径**（`UPDATE users` 只写 `dept_id`/`verified`/`password_hash`）⇒ 输入在下一次 `renderSettings` 被真名抹掉 |
| `#prefs-model` | 由真实 `/api/models` 填、看着能选，但**无监听 / 无存储键 / 全仓无消费者**（`openChat()` 只在模型行与「最近使用」芯片处被调用，都自带模型实参） |
| 三枚通知开关 | 渲染成**已勾选**，**无 id / 无 name / 无人读** —— 仓内没有通知子系统 |

**判据是「同卡兄弟」与「仓内成例」**（⇒ 漏修而非取舍）：同卡的邮箱早已 `readonly` + 一句提示
（`09e4121` / #157 那次重设计**同时**删掉了同卡那个早已无监听的「保存」按钮、给邮箱标了惰性，
**独漏昵称**）；偏好卡的语言 / 主题 / 密度三个控件都持久化并生效；「能力尚未开放」的成例是
`#withdraw-btn`（`disabled` + 「提现暂不支持」）。

**约定**：

1. **没接线的控件必须标惰性并解释**——禁用而不解释，用户分不清「未开放」与「坏了」；
2. **接了线的控件不得标惰性**——「一键全禁用」会把语言 / 主题 / 密度一起打死（门禁规则 1 双向）；
3. **「加个监听器 + 写进 localStorage」不算接线**：值写进存储但**没有任何读取方**，
   只是把静默丢弃换成了静默囤积（这是本轴最诱人的半修，探针与门禁都拒它）；
4. **惰性说明必须是本地化键**，两包俱在——否则 en 界面直接把原始键名显示给用户。

**CI 覆盖**（`src/state_gate.rs::settings_controls_are_either_live_or_marked_inert`）：
射程 = 设置视图里 `settings.account` / `settings.notify` / `settings.prefs` 三张卡的区段
（密钥卡不在射程内，它的接线另有既有门禁）——「三张卡」是**显式登记**的射程，不靠漏扫。
规则 1 双向（`inert ⟺ ¬consumed`）、规则 2 惰性控件 ⇒ 卡内有 `hint` 且键两包俱在、
规则 3 空集守卫。配套
`the_settings_control_extractors_have_teeth` 用合成输入钉住判别式：只填不读 / 只存不读 /
读进局部变量给重渲染用，三者都**不算**消费；值作为非存储调用的实参才算（含单选组的
`name` 句柄）；且**无 id / 无 name 的控件永远算未接线**（那三枚开关就靠这条抓）。
⚠️ HTML 注释先剥掉再断言（修法自己就会在控件旁写解释性注释）。

**为什么 CI 用静态门禁**：探针 `tmp/c2148_probe.js` 能证明屏幕上的事实（jsdom 真启动四脚本、
驱动真控件）—— 改前树恰 7 条轴腿红、修复树 14/14、两条竞争修法分别被 `D1`（过度纠正）与
`A2`（持久化但不消费）拒掉；但 CI 里没有 JS 运行器，长期守住这条约定的是上面那条静态门禁
（原地变异 6/6 如声明：未修树红、过度纠正红、半修红、只标记不解释红、只写一包红、修复树绿）。


## 口令下限以「字符」计（R96）

- **唯一真源**：`src/routes/mod.rs::MIN_PASSWORD_CHARS`（字符数）。三处请求校验（register /
  reset-password / change-password）一律经 `password_too_short()`，**不得**再写 `pw.len() < N`
  —— Rust 的 `String::len()` 是 UTF-8 **字节**数，`密码abc`（5 字符 / 9 字节）会被它放行。
- **四张脸必须同单位**：`ui/index.html` 注册占位符「至少 8 位」（设计基线，`docs/prototype/`
  同款）· i18n 两包 `err.weakPassword` · `i18n.js` 的 ERR_MAP 字面量（服务端返回的原话）·
  客户端守卫。任一处改成「字节」都不算修法：下限是**字符**，改宣告等于把缺陷写进文档。
- **客户端**：`Array.from(pw).length`（Unicode 标量值），**不是** `pw.length`（UTF-16 code
  unit）。emoji 一个字符在 `.length` 里是 2 ⇒ 与后端 `chars().count()` 逐标量对齐。
- **门禁**：`src/state_gate.rs::the_password_minimum_is_counted_in_the_unit_its_message_names`
  四规则（服务端必须走 helper／客户端必须 `Array.from`／四处载体 N 与单位一致且 N == Rust 常量／
  站点形状 3+1）。**形状归门禁，事实归 `src/routes/mod.rs` 的两条边界测试**（都在 `mod tests`
  里读语言包取 N，不写死）：

  - `password_minimum_is_counted_in_characters_not_bytes` —— 钉单位（含 emoji 那一格）；
  - `register_rejects_a_password_short_in_characters_long_in_bytes` —— 钉端点行为。

  屏幕侧那一半由本地 jsdom 仪器钉（仓内 CI 无 JS 运行器）。

## 静态 `data-i18n` 属性归语言层所有，不属于它的子元素（C2150）

`applyStatic()`（`ui/js/i18n.js`）对每个 `[data-i18n]` 元素执行 `innerHTML = t(key)`。
因此**一个已经带文本 `data-i18n` 的元素，不能同时指望它子元素上的语言层钩子生效** ——
祖先那一步会把后代元素连同它自己的 `data-i18n*` 属性一起从文档里摘掉；随后循环再对那个
**已分离**的节点设值，无异常、无效果（jsdom 实测 `isConnected === false`）。

改前的两张脸：

| 位置 | 缺陷 |
|------|------|
| `index.html` 加额申请卡片 | `<h3 data-i18n="admin.raise.title">加额申请 <span data-i18n="admin.raise.sub">（…）</span></h3>` —— 两个包的值都是**纯文本**，子 `span` 被摘掉 ⇒ 提示「（成员申请 → 管理员批准 / 驳回）」**两种语言都不显示**，且 `app.js` 只写 `#raise-requests`、从不重画那个 `h3` ⇒ **永不恢复** |
| `index.html` 登录页底 | `<p data-i18n="login.foot">…<a id="reg-link" data-i18n="login.register">注册</a>…</p>` —— 值**自带**同样的标记（含 `id="reg-link"`），复原后文本正确、点击是 document 委托在 `app.js` 里按 `t.id` 匹配 ⇒ **行为无损**；但那个属性是死的 ⇒ 键 `login.register` 沦为只喂死钩子的孤儿 |

**约定**：

1. 带文本 `data-i18n` 的元素**内部不得**再出现任何 `data-i18n*` 属性。要在一行里放两段文案，
   写成**兄弟**元素（成例：`wallet.hint` + `wallet.tx` + `wallet.hint.suffix` 那个 `<p class="wallet-hint">`，
   父元素不带 `data-i18n`）。
2. **值自带宽标记**是另一种合法形态（`login.brand.headline` 的值含 `<span class="nb">`、
   `ops.users.sub` 含 `<strong>`、`login.foot` 含两个 `<a>`）—— 之所以有效，是因为**值自己把它写了出来**。
   只在该标记必须携带 `id` 等属性时才用它；否则用兄弟元素。
3. **只有文本属性会砸后代**：`data-i18n-title` / `data-i18n-label` / `data-i18n-ph` 走 `setAttribute`，
   各写一个属性，子标记原样保留。故 `select#tx-range`（带 `data-i18n-title`）里的五个
   `<option data-i18n="tx.range.*">`、以及 `div#help-panel`（带 `data-i18n-label`）里的
   `<strong data-i18n="help.title">` 都是**合法**形态。
4. 两种既有门禁都看不见这类缺陷：`every_static_i18n_attribute_resolves` 只问「键在不在两个包里」，
   而「按文本找引用」的死键扫描会看到键的**字面量就写在 `index.html` 里** ⇒ 判「有人用」。

**CI 覆盖**（`src/i18n_pack.rs::no_data_i18n_attribute_nests_inside_a_data_i18n_element`）：
对 `index.html` 做标签栈扫描，`data-i18n*` 属性落在带文本 `data-i18n` 的祖先内部即红；
先剥 HTML 注释（注释里的标记不参与结构），空元素/自闭合不入栈，属性值里的 `>` 不截断标签
（判别式由 `nested_i18n_detector_detects_injected_defects` 用合成输入钉住）。**零豁免清单**。
扫描器另报三个阳性对照（起始标签数、文本载体数、EOF 未闭合栈），避免「0 违规」被误读成「扫描器瞎了」。

⚠️ **射程只到「嵌套」这一条轴**：A/B 里那条竞争修法 `m_drop_parent`（把祖先的 `data-i18n` 整个删掉，
例如让 `<h3>` 不带钩子）**确能让本门禁通过** —— 它真的消掉了嵌套。但它换来的是**另一条轴**上的缺陷：
那位祖先的文案从此不再本地化，键 `admin.raise.title` 沦为**孤儿**（除语言包外零引用，实测孤儿集
恰好 +1）。孤儿键今天无人守（全仓已有 60 余个不可达键）、**本次不入射程**，A/B 如实记录这条腿
按声明为 GREEN，而不是伪装成被拒绝。

⚠️ **本次未覆盖的边界**（已测量、非盲区）：`data-i18n*` 属性若落在**被 JS 整体替换内容的容器**里
（`app.js` 对某 id 做 `innerHTML =`）同样是死的。今日实测为 **0 处**，故未建门禁 ——
该判定的词法近似（按 id 找 `innerHTML =`）比本条脆弱，留待需要时再收。

## 趋势图的聚合粒度：由**实际请求窗口**决定，不由控件值决定（R165）

**契约**：`txTrendBucket()` 的返回值必须是**真正发出去的请求窗口**的函数。窗口真源只有一个 ——
`txRangeParams()`（`ui/js/app.js`），它同时也是请求串的构造者。因此 `txTrendBucket()` 读**它**，
而不是再解释一遍 `#tx-range` 的选项值。

**为什么**：`all`（全部时间）与「自定义 + 两个输入框都空」拼出的**列表请求逐字相同**（都无边界 ——
`txRangeParams()` 在 `custom` 且两个输入框皆空时返回 `""`），而旧实现按控件值分别给出 `week` / `hour`：
原来那张「控件值 → 粒度」的表里，`custom` 分支把跨度**从输入框**算（空 ⇒ 跨度 0 天 ⇒ `hour`），
`all` 分支按窗口（无界 ⇒ `week`）。⇒ 同一份数据、同一个请求，两条粒度。
另一副面孔：`TX_TREND_MAX_COLS = 40` 把 x 轴**锚在右端**，小时粒度下最多画**最近约 40 小时**，
却按 `MM-DD HH:00` 标尺自称。

**射程**（#341）：门禁 `state_gate::the_trend_grain_derives_from_the_query_not_from_the_control`
是**词法**的 —— 它证明该函数体内**没有** `#tx-range` 的选项值字面量、**调用了** `txRangeParams`，
并给返回的粒度字面量划界（`{hour, day, week}`）。它**不**证明阈值（3.5 / 60 天）取得对，
也不证明 `URLSearchParams` 解出的跨度与请求里的 `now` 逐毫秒一致 —— 那些由 jsdom 探针
在它所跑的那棵树上覆盖。

## 交易表的列头箭头：**一个主张、三处载体**（R164）

**契约**：`txTable.sort` 是排序状态的**唯一真源**，而列头的 ▲/▼ 描述的是**整个数据集**的顺序
—— 既然列表由服务端分页（`pageRows = data`），这个主张就必须同时投影到三处，缺一处箭头就在撒谎：

1. **列表请求**带 `&sort=` / `&dir=`（`txSortParams()`）—— 且**只**接在列表请求上：趋势按时间桶
   聚合，行序对它的语义没有意义（对趋势也发排序参数＝又一个「口径被搬走」）；
2. **载荷签名** `txQuerySig()` 覆盖它 —— 少了这一段，点列头只改状态、签名不变 ⇒ 守卫认定载荷没变、
   不重拉 ⇒ **箭头动了而列表不动**（比原缺陷更坏，C2146 同族）；
3. **本地排序为声明了 `serverSort` 的表让路**（`if (state.sort.length && !(serverPaging && serverSort))`）
   —— 开关由**调用点**声明，条件必须**整条**读（`!(a && b)` 自带括号）。

**为什么**：`#135` 把交易列表改成服务端分页后，本地排序只排**传进来的那一页**，而箭头/`tx.sort.title`
说的是整张表 —— 排序停在它出生的地方（静态原型 #7），分页没有跟着走。

**服务端**：用户串**永不**进入 `ORDER BY`。`src/routes/wallet.rs::TX_SORT_KEYS` 是唯一白名单
（11 键，与 `ui/js/app.js::TX_COLUMNS` 的 `key` **逐键相同**），`tx_sort_expr` 翻成表达式，
`tx_order_by` 渲染并**恒**在尾部加 `, t.id DESC`（分页锚：非唯一排序会让相邻页的边界不确定 ——
同一行出现两次、另一行永不出现）；未知键 / 键与方向个数不匹配 / 非法方向 ⇒ 400，且与 `type`
同款「先校验、后取锁」。

**射程**（#341）：门禁 `state_gate::the_sort_indicator_and_the_order_by_share_one_source` 是**词法**的
—— 它证明三处载体都读同一个状态、白名单与列名册**精确相等**、`ORDER BY {…}` 由白名单守卫的构造器
渲染、`q.sort`/`q.dir` **只**在那一处绑定里被采。它**不**证明运行期行真的按全局顺序排（那是 jsdom
探针 `r164_probe.js` 的职责，`cargo test` 里没有 JS 运行器），也不证明 `ORDER BY` 语义本身
（那是 `src/routes/wallet.rs` 的行为测试）。

## 共享行的「动作」与「结局文案」必须由同一条目给出（C2153）

共享 key 有**三个**状态，三个都可达：`PATCH /api/sharings/:id` 接受 `on` / `paused` / `off`
（`off` 是软删），而 `GET /api/sharings` **不做状态过滤** ⇒ 软删过的行仍在列表里。
行内按钮按**当前状态**三值取（暂停 / 恢复 / 重新上架，`SHARE_STATUS` 三状态各有徽标），
而结局消息按**下一状态**取 —— 两半一旦分开写，就会分叉：

```js
// 改前：动作三值，结局两值
(s.status === "on" ? T("share.toggle.pause")
  : s.status === "paused" ? T("share.toggle.resume") : T("share.toggle.relist"))   // 按钮：三值
const next = s.status === "on" ? "paused" : "on";                                   // ← 只有两个值
toast(next === "paused" ? T("share.toggle.paused", …) : T("share.toggle.resumed", …));
```

`off → on`（重新上架）因此被报成「已恢复 …」：按钮写「重新上架 / Re-list」、toast 写
「已恢复 / Resumed sharing of …」。命名那个分支的键 **`share.toggle.relisted` 两个包都在、无人可达** ——
**「不可达的键」正是「丢了一条分支」的指纹**（该键是孤儿集里第一个被证实为**活缺陷**的）。
溯源＝漂移而非取舍：`68f9f70`（#86）起按钮就是三值，`89963f3`（#94「zero mock」）把三分支的 mock
切换换成两值三目式 —— 按钮留下三值、结局塌成两值。
**零账号复现**：共享页 → 删除某条（软删 `off`，行仍在）→ 按钮变「重新上架」→ 点击 → toast 谎报「已恢复」。

**约定**：

1. **一张表一处真源**：`const SHARE_TOGGLE = { 状态: { label, next, outcome } }` —— 按钮的**动作标签**、
   它做完之后的**下一状态**、以及**结局文案键**，由**同一条目**给出。两个消费者都只读这张表
   （`shareToggle(s.status)`），结构上不可能再分叉。
2. **表的键集必须等于 `SHARE_STATUS` 的键集**（两侧都从源码推出来，不写花名册）：徽标认识的状态，
   切换表都必须有对应条目。后端将来加第四个状态时，漏进表的那一个会被门禁看见。
3. **处理器不许自己挑结局**：`toggleSharing` 体内不得出现 `share.toggle.*` 字面量、不得按状态
   **字面量**（`"on"` / `"paused"` / `"off"`）分支，且必须与按钮那一段标记**共用同一个访问器**。
4. **`label` 与 `outcome` 两列逐条目唯一**、`next` 不得指向自己：把三条压成两条 = 又有一个动作
   被报成另一个动作的结局。

**CI 覆盖**（`src/state_gate.rs::the_sharing_toggle_outcome_comes_from_the_same_entry_as_its_action`）：
四条断言各有独立的牙 —— ① 全文件 `share.toggle.*` 字面量只在表内（表外出现 = 有人自己挑结局）；
② 表覆盖状态集且两列逐条目唯一、`next` 不自环、键**两包俱在**；③ 处理器不挑结局且与按钮同源；
④ 条目数 == 状态数。判别式自证用**合成输入**：注释里的表不算表（先经 `code_text_by_line` 剥注释）、
字符串里的花括号不参与配对、嵌套对象的键不算外层键、`xlabel` 不是 `label`（坑 #333 的边界），
以及**在缺陷形状上取空集 / 在修复形状上取到那一个访问器**的对照。
处理器是**推**出来的：把状态交给 `/api/sharings/` 的端点里，**恰好一个**是「由状态推出」的
（把状态写成字面量的那些是删除这类直接端点）—— 多于一个即两条各自解释状态的路径。

**为什么 CI 用静态门禁**：`cargo test` 里没有 JS 运行器。仪器 `tmp/c2153_probe.js`（jsdom、真四脚本、
真导航 → 真按钮 → 真 PATCH、读 `#toast-wrap`，`expect` 逐腿声明）实测：改前树 `10/10 as declared`
且恰 `A2`/`A3`/`Z1` 三条轴腿红（en 与 zh 两包都错）、修复树 `10/10` 全绿、竞争修法（把**按钮**也
塌成两值、让两边"一致"）`5/10` 红 —— 仪器钉得住方向；「删键」式逃逸也被拒（断言打在**渲染文本**上）。

⚠️ **射程**：门禁是**词法**的 —— 它证明结局键**来自**那张表，不证明 `entry.next` 与 `entry.outcome`
取的是同一字段（把 `outcome` 写成 `label` 的门禁看不出来，仪器能）。静态门禁挡的是**形状**
（两侧各自解释状态），值是探针在它所跑的那棵树上覆盖的。

## 市场工具栏的计数按**行（模型）**计数（C2172）

- **单位由它汇总的那张表决定**：`#mk-count` 数的是**过滤后的模型行数**（`marketRows()` → `/api/models`，一行一个模型），所以文案必须与同一张表的**行身份列**同单位 —— `ui/index.html` 的 `<th data-i18n="mk.col.providerModel">厂商 / 模型</th>`。旧值 `cnt.on`「{n} 个在售 key」把**模型行数**印成「**key** 数」，而同屏既有「无 key」的行、又有「可用 · 3 key」的行 ⇒ 同一张表里数字与单位互相打脸（夹具 `Σkey = 6 ≠ 4 行`）。改名 `cnt.models`（zh「{n} 个模型」/ en「{n} models」）**净键数不变**（每包改 1 个键名），且不留一个「名字在撒谎」的键。
- **CI 覆盖**：`src/i18n_pack.rs::the_marketplace_count_is_expressed_in_the_unit_of_its_rows`（五条规则：计数键从 `app.js` 派生 / 行身份键从 `index.html` 派生 / 两包都须含行单位词 / 两包都不得含 `share.col.key` 的 key 词 / `mk.avail.` pill 族仍用同一把尺子）。
- ⚠️ **射程**：门禁钉**单位**不钉**数值**；删掉**一个**填充位点门禁看不见（位点数一起降）⇒ 数值与「不许把计数删掉」由仪器 `c2172_probe.js` 的 `F1/F2/P0b/T1` 腿钉。本轴**不新增也不删除**任何包键，也**不动** `UNREACHABLE_PACK_KEYS`。
  （旧值那枚「同值的孤儿键」`mk.count` 已在队列更早的一跳 `pack-key-shrink`（PR #270）里被删掉 ⇒ 本轴落地时这段文案在包里只此一处。）


## 共享行的 plan 显示名：一个来源，四个渲染点（C2157）

共享行（`/api/sharings` 的 `keys.plan`）只存**配置 id**；显示名必须由**语言包感知**的解析器从 id 派生，
而不是把 id 直接印出来。此前同屏两个口径：上架下拉与成功 toast 印标签（`DeepSeek · API（按量）`），
而共享表单元格（`#share-body`）与仪表盘「我的共享」卡（`#dash-sharings`）印 id
（`DeepSeek · deepseek-paygo`）。

**约定（三条，缺一不可）**：

1. **一对 helper 就是全部**：`planList()` 是两张 plan 表（`Live.plans` / `D.PLANS`）的**唯一**合读点，
   `planById(id)` 是**唯一**的 id→plan 解析，`planLabelById(id)` 是**唯一**的「存储 id → 标签」产出；
   渲染点消费**派生值**（`esc(s.plan)`），不得再内联 `s.plan || "API"`。未知 id（config 变更后的陈旧 id）
   由 `planLabelById` 原样返回 —— 语言中性，与旧行为一致。
2. **解析器判别式必须覆盖全部解析器**：`i18n_pack::backend_neutral_data_labels_are_localized_in_the_client`
   的判别式接受 `planLabel(` **或** `planLabelById(`。锚在单个函数名上会把第二个合法入口判成红
   （同族：#347「名字不是唯一载体」）—— 要放宽的是**判别式**，不是把 helper 改名去迁就子串。
3. **跨视图的槽：每个渲染它的分支都要接上写者**：`renderDashboard` 经 `sharingsToView` →
   `planLabelById` 读 `Live.plans`，因此该槽的唯一写者 `refreshPlans()` 必须由 `loadSharing()`
   **和** `loadDashboard()` 各调一次（与 C2135 的 `refreshDashboard()` 同形）。少一个分支，会话若在
   该视图上建立就只拿得到兜底表（卡片显示**类型级**标签而非该 plan 的名字），且**永不自愈**。

⚠️ **射程**：门禁是**词法**的 —— 它证明四个渲染点渲染的是派生值、`sharingsToView` 的 `plan:` 走了
`planLabelById`、`planLabelById` 不读 `name`（兜底表 `D.PLANS[].name` 是中文硬编码，C2133 ⛔ 未修）。
它**不**证明运行期标签取到的是哪一支 —— 那是 jsdom 仪器的职责，而 `cargo test` 里没有 JS 运行器。

## 运营卡「上游 key 状态」：判定语必须说数据说的那件事（C2158）

`keys` 表只有 `status`（`on` / `paused` / `off`），`/api/ops/runtime` 只回 `total` / `on` / `off`
（`off = total − on`，由后端 `SUM(CASE WHEN status='on' …)` 算出）—— **后端不产出任何「健康」信号**。
而这张卡曾把「停用」渲染成「异常 / 全部失败」、并把「全部停用」涂成 `pill-danger`：用户**暂停自己的
key**（正常操作）于是让运营者看到红色故障警报，而屏幕上的句子（「上游 key 健康」「{n} 个异常」）
宣称的是一份**并不存在**的数据。

**约定（三条，缺一不可）**：

1. **键名即语义**：判定语键名**不得**出现 `healthy` / `abnormal` / `failed` / `fail` / `error` /
   `down` / `unhealthy`（比对的是**键名**，不是文案 —— 改文案不改键名仍红）。键名会撒谎，
   门禁就钉不住它：三态键是 `ops.keys.allOn` / `someOff` / `allOff`。
2. **消费到的键必须已登记**：运营卡的 key 状态块只允许用 `{allOn, someOff, allOff, count, empty}`
   —— 另起一个未登记的新判定语键＝偷偷换一套口径。
3. **反向：三个状态臂必须都被渲染**：`allOn` / `someOff` / `allOff` 各至少出现一次。
   删掉 pill 不是修法（判定语要说得更准，不是不说）。
4. **配色是同一个主张**：`off >= total` 用 `pill-muted`（中性），**不是** `pill-danger` ——
   红/警告是「故障」的词法，而数据支持不了故障。`off === 0` 保持 `pill-ok`、部分停用保持 `pill-warn`。
   `pill` 类名另有**两处合法用户**（`PILL_CLS` 表、部门额度耗尽卡）⇒ 判据钉**渲染点**，不是裸类名。

门禁：`i18n_pack::the_ops_key_health_pill_names_the_state_it_counts`（规则 1–3）
＋ `i18n_pack::the_ops_key_state_scanners_have_teeth`（两条判别式的合成自证）。

⚠️ **射程**：门禁是**词法**的 —— 它证明**键名与键集**，**不**证明渲染出来的**句子**与数据一致。
那一半由 jsdom 仪器 `c2158_probe.js` 承接（`A1`–`A4` / `B1` / `C1` 机制腿：共享页真暂停一枚 key →
真 `PATCH` → 回运营视图，该厂商行不得变成红色故障态）。两半**互补**且已实测：把**文案**改对而
**键名**照旧的竞争修法，探针**接受**（屏幕上的句子是对的）、门禁**拒绝**（键名仍在宣称健康）。


## 交易类型的枚举文案：一处枚举 = 具名每一种**生产者真会写**的类型（R99，2026-09-22）

`src/routes/wallet.rs::TX_FILTER_TYPES` 声明 6 个**受理**值
（`consume earn topup gift expire withdraw`），其中 `withdraw` 至今**没有 writer**
（该数组自己的文档注释就写着这点）。UI 有**三处**手抄这份枚举：钱包页脚注
`wallet.hint.suffix`、交易页副标题 `view.transactions.sub`、仪表盘「交易笔数」副标题
`dash.trades.sub`。`expire` 在 C2050/C2051 变成一条**真账本行**（赠送过期真扣余额、
真出现在交易表的类型列与导出 CSV）之后，三处**一处都没跟着走** —— 屏幕上写
「涵盖消费、收益、充值、提现、赠送」，而同屏的交易表里就有一行「过期」。一个事实、三处载体，
抄漏的永远是同一项。

**约定**：把交易类型的集合写进文案时，必须具名**生产者会写出来的每一种**类型；且用词必须与
`tx.type.*` 标签**逐字**一致（尺子就是该包自己的标签值，大小写不敏感的子串）—— 即
「页面上要用表格给这个类型起的那个名字」。所以英文原本的派生词
（`Consumption` / `earnings` / `top-ups` / `withdrawals` / `gifts`）既不能被校验、也和
`tx.type.*` 不是同一个词，随本次一并改回标签词。

门禁：`state_gate::the_transaction_type_prose_names_every_type_the_writers_write`（规则 1–4，
期望值**全部派生**、零手写类型名）
＋ `state_gate::the_r99_rules_have_teeth`（八条变体腿：pre-fix／半修／新 writer／写了未受理的类型／
受理了没标签的类型／单边空标签／两条被声明的盲区）
＋ `state_gate::the_r99_extractors_have_teeth` 与 `state_gate::the_r99_bound_parameter_producer_is_loud`
（提取器自证与「绑定参数必须响亮报错」）。

1. **前置**：语料 ≥ 20 个源文件、生产写入点 ≥ 2、写入类型 ≥ 2（空集上的集合断言会假绿）。
2. **具名**：值里具名**过半**写入类型的键构成「名册」，名册里**每个键 × 每种语言**都必须具名
   **全体**写入类型。
3. **形状**：两个包的键集合相等；`tx.type.<t>` 的标签必须**两个包都非空**。
4. **三明治 `写入 ⊆ 受理 ⊆ 有标签`**：写入集合 ← 每条生产 `INSERT INTO transactions` 在
   **`type` 那一列**写的字面量（不是「这段字面量里任意一个 `'…'`」—— 否则 `'成功'` / `'m'`
   会被算成类型）；受理集合 ← `TX_FILTER_TYPES`（该数组自称「新增类型时只改这里」）。
   这条挡住两类「新类型落地时漏一处」：写了未受理的类型（线上筛不到）、受理了没标签的类型
   （表里印出 `tx.type.refund` 这样的**裸键**）。

⚠️ **射程**：门禁是**词法**的 —— 它证「名册里每处散文都写了那些类型的名字」，**不**证屏幕上
那一刻真的显示这句话（那一半归 jsdom 探针的 A/B 腿：真 boot ＋ 真登录 ＋ 读那三处文案的**文本**）。

⚠️ **三个故意的盲区**（都是**声明**，不是漏判）：
① 「过半」是声明 —— 具名 ≤ 半数写入类型的键**永远**不进名册（`tx.summary.net.sub`
「收益 − 消费」正是这种：它说的是**差**，不是枚举）；
② 名册是**推导**出来的 ⇒ 把某处文案删空、或把键连同绑定一起删掉，本门禁**沉默** ——
那是「让承诺消失」而非「让承诺成真」，挡它的是探针与
`i18n_pack::every_pack_key_reaches_a_consumer`；
③ 写入点必须是**字面量**：将来若有 writer 把类型做成绑定参数（`?1`），门禁**当场报错**，
而不是静默漏掉那个写入点。

⛔ 本轴**刻意不读** `ui/README.md`：名册是从**制品**推导的，若再钉一行手写契约，
同一事实就有了两个来源 —— 那正是本轴要消灭的漂移。本文件「页面清单」第 5 条的枚举是**文档**，
不是门禁的尺子。
