//! 前端状态槽门禁（C2131）：**一个缓存槽的写者，必须就是写下它有效性证据的那个函数**。
//!
//! `ui/js/app.js` 的 `Live` 是一组「某个视图自己的载荷缓存」。交易视图那一槽还带**有效性证据**：
//! `loadTransactions` 写完 `Live.transactions` 之后，由**同一个函数**写
//! `txTable.loadedPage / loadedPageSize / loadedFilterSig`，而 `renderTransactions` 的守卫
//! （三项与当前请求参数不一致就重拉）**只比这三项**。
//!
//! 于是「谁写了槽」与「谁写了证据」必须相等。一旦另一个函数也写这个槽 —— 例如仪表盘为了
//! 「交易笔数」卡而发的 `page=1&page_size=1` 查询（它**不带时间范围**、只要 `total`）——
//! 守卫比的就成了**另一个写者的账本**，缓存内容与守卫的证据脱钩。这正是 C2130 实测的缺陷：
//! 再入交易页时 `renderTransactions` 放行仪表盘的载荷，于是表格只剩 1 行（`page_size=1`）、
//! 汇总卡挂着「当前筛选」却显示全时段聚合（那次请求不带范围）、趋势卡谎报「加载失败」
//! （`trend` 只有 `loadTransactions` 会挂上，而它既没发也没失败）。
//!
//! **为什么必须由静态门禁钉，而不是由那支 jsdom 探针钉**（C2128 坑 #287）：竞争修法
//! 「扩守卫」（让守卫也比对载荷的 `page_size`）能让探针的每一条 DOM 断言都变绿 —— 探针只能
//! 证明「屏幕上不再是外来载荷」，证明不了「槽里不再是外来载荷」。本模块断言后者。
//!
//! 设计约束（与 `i18n_pack.rs` / `table_gate.rs` / `catalog_gate.rs` / `deploy_gate.rs` 同型）：
//! - **仅测试期编译**（`#[cfg(test)] mod state_gate`，见 `main.rs`），不进生产二进制；
//! - **零新依赖**：不执行 JS、不起浏览器，只做逐行扫描（仓库**没有** `regex`）；
//! - **归属方式是位置性的**：一行代码属于它之前最近声明的那个 `function NAME(`；
//! - **注释行不参与断言**：行首为 `//` 的行被剔除（本文件的题眼就是那段解释为什么的注释，
//!   它必须能提到 `Live.transactions` 而不触发门禁）；
//! - **阳性对照**：先断言扫到的写者/证据持有者**非空**（空集上的集合断言会假绿，坑 68），
//!   再用**合成输入**自证提取器与判别式都有牙齿。
//!
//! 已知边界（如实的射程，不是承诺）：只认**字面**的 `Live.<slot>` 与 `liveLoad("<slot>"`。
//! 用动态键（`Live["transactions"] = …`）或为 `liveLoad` 另造名字来写槽，本门禁看不见 ——
//! 与兄弟门禁一样，它钉的是**静态调用点**，不是运行期别名。
//!
//! # C2132：缓存的生命周期，以及「每个视图都要有 loader」
//!
//! `Live` 是**按会话**缓存。第二组不变量钉的是它的**生命周期**与**视图路由的形状**：
//!
//! 1. **身份边界必须丢弃每一个槽**。会话建立（`loadSession`：boot / 登录）与会话结束
//!    （`exitGuest`：登出 / 401）两侧都要清空。清空必须**派生自** `Live` 的对象字面量
//!    （`Object.keys(Live)`）—— 手抄名册会在新增槽时静默漏掉。反例正是 C2132：登出不清缓存，
//!    下一位登录者打开钱包时，`#wallet-forever`（「永久点数」）显示的是**上一位用户的**
//!    `Live.wallet.balance`，且永不自愈（见下一条：钱包视图当时没有 loader）。
//! 2. **`renderView` 的每个分支都必须「既渲染又拉取」**。它是唯一允许「先同步渲染缓存、
//!    再异步拉取」的地方 —— 于是「只渲染不拉取」的分支就是**永远显示缓存**的分支。
//!    C2132 实测：八个分支里 `wallet` 是唯一只 `renderWallet()` 的，所以钱包单元格
//!    （乃至一次会话内）从不变新。
//!
//! 这两条与第一条不同：它们**不能用 DOM 探针钉方向**（改前/改后都是「屏幕上对不对」），
//! 必须由静态断言钉住形状（C2128 坑 #287）。

//! # C2135：**渲染谁就装载谁** —— 视图的 loader 必须装载它的 renderer 读到的每个槽
//!
//! 前两条说的是「槽自身的纪律」。这一条说的是**槽与视图的对应关系**：`renderView` 的每个分支
//! 都要 `render` 一个视图、并（登录时）`load` 它自己的数据 —— 但**「它自己的」不是由分支名
//! 决定的，而是由渲染闭包读了哪些槽决定的**。一个视图会读**别的**视图的槽（共享槽），
//! 那时只拉自己那份数据就会让那一格永远空着。
//!
//! C2135 实测的形状：`#month-changes`（钱包视图）与 `#dash-month-changes`（仪表盘）由**同一个**
//! `renderMonthChanges()` 绘制，两者都读 `Live.dashboard`；而该槽的写者只有仪表盘的 `loadDashboard`。
//! 钱包分支只调 `loadWallet()`（它只刷 `Live.wallet`）⇒ **会话在钱包视图上建立时**（hash `#/wallet`
//! 后登录；以及在钱包页登出再登录）没有任何人装载那个槽：净变化印 `0` + 「本月暂无变动」，
//! 而同一份载荷在仪表盘上渲染正确，且**永不自愈**（钱包的 loader 不碰该槽）。
//! ⚠️ 带 token **刷新**看不到它 —— boot 在 `DOMContentLoaded` 里**无条件** `renderView("dashboard")`
//! 顺手把槽装好了（这正是它长期潜伏的原因）。
//!
//! 修法＝**共享槽只能有一个写者**（C2131），装载它的事收进一个函数（`refreshDashboard()`），
//! 由**每个渲染它的视图**各调一次。本门禁钉的就是这条对应关系：
//!
//! > 对每个槽 `S`、每个 `renderView` 分支 `B`：若 `B` 的**渲染闭包**（`render…` 的传递调用集）
//! > 里有人读 `Live.S`，则 `B` 的 **loader 闭包**（`load…` 的传递调用集 ∪ 会话级 `loadSession`
//! > 的闭包）里必须有人写 `Live.S`。
//!
//! 为什么要把 `loadSession` 算进来：`models` / `publicUrl` 是**会话级**数据（`loadSession` 装载，
//! 所有视图共用），它们的「装载者」本来就不是某个视图的 loader。把会话级写者计入后，
//! 全仓**没有任何一处**需要豁免清单（豁免清单＝会腐烂的花名册）。
//!
//! 已知边界（与上一条同型，如实的射程）：槽宇宙 = `Live` 字面量声明 ∪ 代码里出现过的
//! `Live.<名>`。**`Live` 字面量里漏登记的槽**（`Live.dashboardTrend` 只被读写、未在字面量里
//! 声明）本门禁看不见 —— 那是「字面量是不是槽的唯一真源」的可读性问题，属于身份边界那条
//! 不变量的**说明**范畴；⚠️ 它**不影响** `resetSessionCaches()`：`Live.x = v` 会**新建**一个
//! own enumerable 属性，因此调用时的 `Object.keys(Live)` 就包含它（C2136 仪器实测：写后登出 ⇒
//! 该槽为 `null`；C2135 的记账曾断言「派生名册清不到它」，已被该仪器证伪，见下方 §C2136 边界）。

//! # C2136：**boot 不渲染视图** —— 视图数据只在「会话建立之后、且它是当前目的地」时才装载
//!
//! `renderView` 的形状是「先渲染缓存、再（登录时）异步装载」（C2132 起）。它只有一个合法的
//! 触发点：`switchView` —— 即**当前目的地**。`DOMContentLoaded` 里那句无条件的
//! `renderView("dashboard")` 违反了这条：它跑在 `restoreSession()` **之前**，而带 token 时
//! `loggedIn()` 此刻已为 true ⇒ 仪表盘那整套查询在**会话还不存在**时就发了出去
//! （实测 `log[0] = GET /api/wallet`，`/api/me` 才排第 2），随后 `loadSession()` 的
//! `resetSessionCaches()` 把它全部作废，`enterApp() → switchView(目的地)` 又装一遍：
//! - 目的地是仪表盘 → 同一套查询**各发两次**（C2136 实测 1 次 boot 14 个请求：`/api/dashboard`、
//!   趋势 `type=all&bucket=day`、`page=1&page_size=1`、`/api/sharings` 各 2 次，`/api/wallet` 3 次）；
//! - 目的地不是仪表盘（刷新在 `#/transactions`）→ 仍白拉仪表盘那套 5 次；
//! - 过期 token → 先发的 6 个请求各拿 401，`__atpLogout()` 被调用 6 次（`TOAST_MAX = 3`，
//!   用户看到 3 条一模一样的「登录已过期」）。
//!
//! 本门禁钉三条**形状**断言（都是**派生**的，不写名册）：
//!
//! 1. boot 处理器体内**不得**调用视图层的「渲染器 / 装载器」（视图层 = `renderView` 各分支里的
//!    `render…`/`load…` 调用名，由 `view_router_branches` 派生）；`renderView(...)` 只许以
//!    **当前目的地**为实参 —— 全仓唯一合法的一处是语言切换监听器里的 `renderView(activeView)`。
//! 2. **每一个** `renderView(...)` 的实参必须是「当前目的地」（裸标识符 / `activeView` 之类），
//!    不得是字面量视图名 —— 否则又会出现「渲染一个不在屏幕上的视图」。
//! 3. `renderView` 必须仍由 `switchView` 调用（**防止矫枉过正**：把 boot 那句删掉之后，
//!    再顺手把 `renderView` 的调用点也清空，就会得到一个什么都不渲染的空壳）。
//!
//! 为什么必须静态钉（而探针只能钉住值）：`renderDashboard()` 单独留在 boot 里（不装载）也能让
//! 探针的请求日志全绿，`if (!api.getToken()) renderView("dashboard")` 同样能全绿 ——
//! 三者都是「症状消失」。本门禁管的是**形状**：boot 不碰视图层。
//!
//! 已知边界：只认字面调用名。`const f = renderView; f("dashboard")` 这类别名逃得过（与兄弟
//! 不变量同型）；boot 体内的**间接**渲染（调一个自己写的、内部再 `renderView("x")` 的函数）
//! 也看不见 —— 射程是「静态调用点」，不是运行期可达性。

//! # C2138：**位置不是身份** —— 模型行的身份必须是 `provider/model`，不能是数组下标
//!
//! `modelsToView()` 把 `/api/models` 的行适配成视图行时**自己造过一个身份**：`id: i` —— 数组
//! 下标。而那个下标会被 `markRecentUsed()` **存进 `localStorage`**（「最近使用」芯片），于是它
//! 跨了渲染、跨了会话、跨了数组：
//!
//! - **跨数组**：游客兜底表 `data.js > MARKET` 是**另一张表**（7 行、id `1..7`、顺序与长度都
//!   不同），只是**数字上看起来**是同一个空间 —— 实测：登录态用了 `xai/grok-4.6`（下标 5），
//!   登出进游客市场后芯片写成 `google/gemini-3.1-pro`；下标 0（登录态第一行）在 1-based 的游客
//!   表里查无此号 ⇒ 芯片**整条消失**。
//! - **跨渲染**：`/api/models` 是 `ORDER BY provider, model`（`src/dao.rs` 的 `list_models…`）——
//!   上架/下架/改名任何一个模型，后面所有下标整体位移。实测：管理员加一个排在前面的模型后，
//!   芯片写成 `moonshot/kimi-k3`，而**点开那枚芯片打开的对话也是 kimi-k3** —— 用户以为自己在用
//!   用过的那个模型（错误从显示变成了动作）。
//!
//! 四条规则，各有各的牙（A/B 里各自有独立的红集，互不遮蔽）：
//!
//! 1. **位置不得进入行对象** —— `modelsToView` 的 `.map(` 回调只许**一个**形参（第二个通常就是
//!    下标），返回的对象里不得声明字段 `id`；
//! 2. **三处 `data-*` 身份必须由 `modelKey(` 产出**（市场行的展开 / 「使用」、最近使用芯片），
//!    且点击侧必须**原样传递**（不得再用 `Number(` 把身份串转回数字）；
//! 3. **`modelKey` 有且只有一处定义**，体内同时提到 `provider` 与 `model`（单靠 model 名会在
//!    多厂商重名时相撞），且**从不**提到 `id`；
//! 4. **写进「最近使用」的值必须是 `modelKey(...)` 表达式** —— 存储层只接受身份串；旧版本存下来
//!    的**下标**无法被诚实地还原成某个模型，按空处理、一次性丢弃（刻意的，见 `getRecentKeys`）。
//!
//! ⚠️ 存储层与显示层都**不许**再按位置解析：`renderRecent` / `openChat` / `consumeModel` 一律
//! `find((x) => modelKey(x) === key)` —— 规则 2/4 是这两个平面的入口。
//!
//! # C2140：**数据源由会话状态决定，不由「数据到没到」决定**
//!
//! `ui/js/data.js` 开篇写着它的契约：那些表只用于**游客市场**（`MARKET`）与**上架表单兜底**
//! （`MODELS`/`PLANS`/`PROVIDERS`/`PROVIDER_LABELS`），**登录态一律走后端 API**。
//! 契约里的判别式是**会话**（`loggedIn()` / `isGuest`）—— 而市场面有三处把它写成了**数据在不在**：
//!
//! - `renderMarketplace()` 的厂商下拉：`Live.models ? <活厂商> : D.PROVIDERS` —— 登录态目录缺席
//!   （500 / 超时 / 还没到）时，下拉里出现**本部署没有的厂商**（实测游客表 10 个、实例目录 5 个），
//!   选中只会筛出 0 行；
//! - `renderRecent()` 的「最近使用」芯片：`… : D.MARKET` —— 芯片画的是**游客市场**的模型行，
//!   而它的唯一消费者 `openChat()` 自带 `if (loggedIn() && !Live.models) … return` 守卫 ⇒
//!   这枚芯片**点开只会提示加载失败**；
//! - `renderNav()` 的市场徽标：`… : D.MODELS.length` —— 显示 **13**（上架表单的价格镜像行数），
//!   而实例目录 6 行、游客市场 7 行 **13 是第三个数**：`MODELS` 与 `MARKET` 行数、顺序都不同。
//!
//! **指纹**：同一个 `renderMarketplace` 里，往下十来行的**兄弟**三元式把规则实现对了
//! （`let list = Live.models ? modelsToView(Live.models) : (loggedIn() ? null : D.MARKET);`，
//! 并附注释「绝不 fallback D.MARKET」）—— 一行对、一行错、判别式只差一个 `loggedIn()` ⇒ 漏修，
//! 不是取舍（与 C2128/C2139 同型：**先扫兄弟行，再判是不是漂移**）。
//!
//! 三条规则，各有各的牙（A/B 里各自有独立的红集）：
//!
//! 1. **每条读市场表（`D.MARKET` / `D.PROVIDERS`）的代码行都必须自己按会话状态分支**
//!    （行内出现 `loggedIn` / `isGuest`）—— 判别式只写在这些读取点上，**零豁免清单**。
//! 2. **同一行不得既读市场表又读上架表单表**（`D.MODELS` / `D.PLANS`）：两张表描述的是不同的
//!    东西（游客市场 vs 上架表单的价格镜像），互为兜底必然显示错（徽标那一行正是如此）。
//! 3. **每张市场表只有一处读者**（`marketRows()` / `marketProviders()`）：市场面的每个消费者
//!    （列表 / 厂商下拉 / 最近使用 / 对话 / 徽标）都走这两个 helper。多一处读取就是多一处
//!    「按数据到没到」的机会（这正是本轮的三个缺陷），也防「兜底被删掉」式修法 ——
//!    删除会让某张表**没有**读者（游客市场是设计的一部分，探针的游客腿钉它的行为、
//!    这条钉它的形状）。
//!
//! **为什么必须静态钉**：这三处不是「某一次渲染对不对」，而是**每条消费路径的形状** ——
//! DOM 探针只看得到当前那一次渲染出来的下拉 / 芯片 / 徽标（且「把兜底表删掉」在登录态探针上
//! 反而全绿，实测见 A/B）。反过来，探针看得到而本门禁**有意放行**的一类：行内**提到了**会话判别
//! 却仍然用游客表（`isGuest ? D.PROVIDERS : D.PROVIDERS`）—— 那是**撒谎的形状**，门禁只管形状、
//! 值由探针钉；两条仪器各管一半，缺一不可（与 C2128 坑 #287 同型，但方向相反）。
//!
//! # C2141：侧边栏只 advertise「按得响」的键位
//!
//! 侧边栏每个 nav-item 都带一个**角标数字**与 `title`（「快捷键 N · 名称」），而键盘上有**一个**
//! 数字键处理器。两侧都是同一个契约的两半，且**必须取自同一个数组**：
//!
//! ```text
//!   const NAV_ORDER = NAV.flatMap((g) => g.items);      // 登记表（视图增删 ⇒ 这里自动跟着变）
//!   … renderNav 内 …
//!   const short = NAV_ORDER.indexOf(item) + 1;          // 角标 = 项在登记表里的下标 +1
//!   … 全局 keydown …
//!   const item = NAV_ORDER[Number(e.key) - 1];          // 按下的数字 → 同一数组里取项
//! ```
//!
//! `renderNav()` 的**游客**分支曾手搓一个同形字面量
//! （`{ id: "marketplace", icon: "marketplace", label: T("nav.marketplace") }`）——
//! 它不是登记表的成员，于是 `indexOf(item)` 恒 **-1**，角标印 **0**（`NAV_ORDER[-1]` 落空 ⇒
//! **死键**），而真正能打开市场的键是 **2**：一个游客永远看不到的数字。`title` 也跟着撒谎
//! （「Shortcut 0 · Marketplace」）。该形状自 #44（v1.17 D 无障碍快捷键）写下即在，与文档
//! `ui/README.md` §键盘可达性（「数字 1-8 → 切换侧边栏视图（键位 = `NAV_ORDER` 下标 +1）」）
//! 直接冲突 ⇒ **漂移，不是取舍**。
//!
//! 四条规则，各有各的牙：
//!
//! 1. **`renderNav` 只渲染登记表里的项**：体内不得出现导航项字面量（`id: "…"`）。游客分支要从
//!    登记表里**筛**（`NAV_ORDER.filter(…)`），与处理器同源。
//! 2. **角标 = 项在登记表里的位置，且不随会话改变**：算角标的行恰好一处，且该行不得按会话
//!    分支（`isGuest ? 2 : …` 这种「按会话另给一个数字」的修法，那个数字没人负责让它按得响）。
//! 3. **数字键处理器索引同一个登记表**：全仓索引 `NAV_ORDER[…]` 的行恰好一处（就是那个处理器），
//!    且它用**按下的数字**取项（`Number(`）。
//! 4. **登记表是推导出来的**（`NAV.flatMap(…)`），不是手抄的第二份清单。规则 3 与 4 合起来把
//!    「两侧同源」钉成等号：生产者读它、消费者索引它、它自己从 `NAV` 展开。
//!
//! **已知边界（如实的射程）**：规则 2 的「不随会话改变」是**逐行**扫描 —— 把关卡写成跨行的
//! `isGuest\n ? 2\n : NAV_ORDER.indexOf(item) + 1`（角标那一行里看不到 `isGuest`）逃得过；
//! 规则 3/4 只认字面名 `NAV_ORDER`（别名的登记表看不见）。值（角标与生效键是否一致）由探针钉，
//! 形状由本门禁钉，两条仪器各管一半。
//!
//! 已知边界（如实的射程）：扫描器剔除 `//` 起始行、多行 `/* … */` 块内部的整行、以及行内
//! 成对的 `/* … */` 片段，但**不做词法分析** —— 字符串字面量里的 `/*` 会被当成块注释起点、
//! 行尾的 `//` 注释不算注释（`app.js` 当前两者都没有，`is_comment_line` 的兄弟门禁同型）。
//! 只认字面表名：`const T = D; T.MARKET` 这类别名逃得过。
//!
//! # C2170：身份边界要清的不只是 `Live` —— **模块级**的视图状态同样跨不过边界
//!
//! `resetSessionCaches()` 只清 `Live` 的那些槽，而交易视图的状态是**模块级**的：`txTable` 的
//! `sort`/`filters`/`page`/`pageSize`（载荷的**输入**）、`txRange`/`txCustomStart`/`txCustomEnd`
//! （时间窗），以及 `txTable.loadedPage`/`loadedPageSize`/`loadedQuerySig`（载荷的**有效性证据**）。
//! 证据属于载荷 —— 载荷被清空而证据留下，守卫（`txQuerySig()` 的比对）就会认一份**不存在**的
//! 载荷为「已加载」，下一位用户的首帧是空表（服务端按 `offset=(page-1)*page_size` 返回 `items: []`
//! 而 `total` 照旧非零）且**不自愈**。
//!
//! 不变量两条：① 身份边界的**闭包**必须**赋值**每一个派生出来的名字 —— `txTable` 字面量的字段 ＋
//! `txTable.loaded*` ＋ `txQuerySig()` 读到的模块级 `let`，**零手抄名册**；② 这些名字的**证据写者**
//! 只允许装载器与边界闭包（否则在 `renderTransactions()` 里清会把守卫每次渲染都重新武装 ⇒
//! 请求风暴，C2146 同形）。复位必须回到**声明处的字面量**（`txTable.page = 1` /
//! `txTable.pageSize = 10` / `txRange = "24h"`）而不是一键清空：`loadTransactions()` 的
//! `Math.max(1, txTable.pageSize || 10)` 兜底会把 `undefined` 退化成「每页 1 行」。
//!
//! 已知边界（如实的射程）：本门禁是**词法**的 —— 它证明闭包**赋值**了每个名字、且值回到声明处的
//! 字面量，**不**证明这些赋值无条件执行，也不证明屏幕上真的换了数据（那半归 jsdom 探针
//! `c2170-probe.js`，仓内 CI 无 JS 运行器）。闭包只收 `function NAME(` 形式声明的函数（`call_graph`
//! 的键）：把复位写进箭头常量时它看不见那个体的**内容**，规则 1 因此会**红**（诚实失败，不是假绿）。
//!
use std::collections::{BTreeMap, BTreeSet};

/// 前端源码在**编译期**读入：测试不依赖工作目录与文件系统布局。
const APP_JS: &str = include_str!("../ui/js/app.js");
/// 设置视图的静态标记（C2148）：控件在这里被渲染，接线在 `app.js`。
const INDEX_HTML: &str = include_str!("../ui/index.html");

/// 交易视图自己的载荷缓存槽（`Live` 的字段名）。
const TX_SLOT: &str = "transactions";
/// 该槽的有效性证据：`renderTransactions` 的守卫比的就是这三项。
const TX_EVIDENCE: &str = "txTable.loaded";
/// 仪表盘放自己那个数字的槽（C2130 引入；与 `TX_SLOT` 的查询口径不同，不能同住）。
const DASH_SLOT: &str = "tradeCount";
/// 仪表盘那条「只取 total」的查询（阳性对照：修法不得把它删掉，只许换槽）。
const DASH_TX_QUERY: &str = "/api/transactions?page=1&page_size=1";
/// 承载**模型身份**的三处 `data-*`（C2138）：渲染侧必须由 `modelKey(` 产出。
const MODEL_IDENTITY_ATTRS: [&str; 3] = ["data-mk-expand", "data-use-model", "data-recent-model"];
/// 点击侧读回这三处身份的 `dataset` 名（不得再经 `Number(` 转回位置）。
const MODEL_IDENTITY_DATASETS: [&str; 3] = [
    "dataset.mkExpand",
    "dataset.useModel",
    "dataset.recentModel",
];

/// 市场面的**游客**表（`data.js`）：只在游客会话里合法（C2140）。
const MARKET_TABLES: [&str; 2] = ["D.MARKET", "D.PROVIDERS"];
/// 上架**表单**的兜底表：它们是「价格镜像 / plan 清单」，**不是**市场目录（行数与顺序都不同），
/// 不得与市场表在同一条表达式里互为兜底（C2140 规则 2）。
const FORM_TABLES: [&str; 2] = ["D.MODELS", "D.PLANS"];
/// 会话状态的判别式：行内出现其一，才算「按会话分支」而不是「按数据到没到」（C2140 规则 1）。
const SESSION_TESTS: [&str; 2] = ["loggedIn", "isGuest"];

/// 侧边栏**视图登记表**（C2141）：角标位（`indexOf + 1`）与数字键处理器（`[n - 1]`）必须取自它。
const NAV_REGISTRY: &str = "NAV_ORDER";
/// 登记表的定义式：它是从 `NAV` **推导**出来的展开结果，不是一个手抄的第二份清单。
const NAV_REGISTRY_DERIVATION: &str = "NAV.flatMap";

/// 设置视图里**配置卡**的三张卡（C2148）：账户 / 通知 / 偏好。
///
/// 本门禁的**射程**就是这三张卡里的表单控件。密钥卡（`#ak-search` / `#ak-new-*` /
/// `#new-api-key-btn`）不在射程内：它们的接线另有既有门禁（`#ak-search` 的筛选参数名
/// 与后端逐字对齐、`api_keys.name` 的空值由 C2101 钉住），把它们一起收进来会让本门禁
/// 需要一串豁免清单 —— 射程由此显式登记，而不是靠「漏扫」。
const SETTINGS_CARDS: [&str; 3] = ["settings.account", "settings.notify", "settings.prefs"];
/// 设置视图 `<section>` 的身份（射程起点）。
const SETTINGS_VIEW_ID: &str = "id=\"view-settings\"";
/// 卡片的起点（`card` / `card mt16` / `card-grid-3 …` 都算边界；只需前缀）。
const CARD_PREFIX: &str = "<div class=\"card";
/// 惰性控件旁边必须有的本地化说明标记。
const HINT_MARKUP: &str = "<span class=\"hint\"";
/// 「把值存起来」不算消费：值读取落在这些方法上时不计数（坑 #338 的推广 ——
/// 「被填」不是「被消费」，「被持久化」同样不是）。
const STORAGE_METHODS: [&str; 4] = ["setItem", "getItem", "removeItem", "clear"];

/// 行首为 `//` 的行：注释行，不参与断言。
fn is_comment_line(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

/// 只看代码行（剔除 `//` 注释行）。
fn code_only(body: &str) -> String {
    body.lines()
        .filter(|l| !is_comment_line(l))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 去掉一行里**成对**的 `/* … */` 片段，返回 `(剩余代码, 是否出现未闭合的 /*)`。
fn strip_inline_blocks(line: &str) -> (String, bool) {
    let mut out = String::new();
    let mut rest = line;
    loop {
        match rest.find("/*") {
            None => {
                out.push_str(rest);
                return (out, false);
            }
            Some(open) => {
                out.push_str(&rest[..open]);
                let after = &rest[open + 2..];
                match after.find("*/") {
                    Some(close) => rest = &after[close + 2..],
                    None => return (out, true),
                }
            }
        }
    }
}

/// 逐行的**代码文本**（注释已剥离，行号即下标 + 1；纯注释行是空串）。
///
/// 与 `code_only` 的区别只在**块注释**：本门禁的题眼是「读表的那一行有没有按会话分支」，
/// 而解释性文字里正会提到表名 —— 申报「注释不参与」就必须覆盖两种注释形态
/// （坑 #309：只剥 `//` 的扫描器会被块注释里的字面标识符绊倒）。三种剥离：
/// `//` 起始行、多行 `/* … */` 块内部的整行、行内成对的 `/* … */` 片段。
/// **不做词法分析**：字符串字面量里的 `/*` 会被当成块注释起点（`app.js` 当前没有）。
fn code_text_by_line(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in src.lines() {
        let mut rest = line;
        if in_block {
            match rest.find("*/") {
                Some(i) => {
                    rest = &rest[i + 2..];
                    in_block = false;
                }
                None => {
                    out.push(String::new());
                    continue;
                }
            }
        }
        if is_comment_line(rest) {
            out.push(String::new());
            continue;
        }
        let (code, opened) = strip_inline_blocks(rest);
        if opened {
            in_block = true;
        }
        out.push(code.trim().to_string());
    }
    out
}

/// 同 `lines_owned_by`，但用 `code_text_by_line` 的代码文本（注释行不参与归属与命中）。
fn code_lines_owned_by(src: &str, hit: impl Fn(&str) -> bool) -> Vec<(String, usize, String)> {
    let mut out = Vec::new();
    let mut owner = String::from("<top-level>");
    for (i, line) in code_text_by_line(src).into_iter().enumerate() {
        if let Some(name) = function_name(&line) {
            owner = name.to_string();
        }
        if hit(&line) {
            out.push((owner.clone(), i + 1, line));
        }
    }
    out
}

/// 这一行是否读了**市场表**（游客市场：`D.MARKET` / `D.PROVIDERS`）。
fn reads_market_table(line: &str) -> bool {
    MARKET_TABLES.iter().any(|t| mentions_identifier(line, t))
}

/// 这一行是否读了**上架表单的兜底表**（`D.MODELS` / `D.PLANS`）。
fn reads_form_table(line: &str) -> bool {
    FORM_TABLES.iter().any(|t| mentions_identifier(line, t))
}

/// 这一行是否**按会话状态分支**（`loggedIn` / `isGuest`）。
fn branches_on_session(line: &str) -> bool {
    SESSION_TESTS.iter().any(|t| mentions_identifier(line, t))
}

/// 这一行是否声明了一个**导航项字面量** —— 即 `id:` 之后直接跟一个字符串（`{ id: "marketplace", … }`）。
///
/// 判别式只认「`id` 是**标识符** + 后面紧跟 `:` + 值是**字符串字面量**」三件事同时成立：
/// - `b.dataset.view = item.id;`、`item.id === activeView`、`d.itemid:` 都不算（左边不是标识符边界；
///   `item.id` 的 `id` 前面是 `.`，仍然不是标识符边界 —— 但 `item.id` 后面跟的是 `;`/` ` 而不是 `:`）。
/// - `dataset.adminTab`、`data-emp-row` 这类别的字段名不会被 `id` 绊到（`mentions_identifier` 式的边界）。
///
/// 导航项的**身份**就是那个 `id`，所以「手搓一个同形字面量」与「从登记表取项」的差别，正是本门禁
/// 要钉的形状（C2141）。
fn declares_nav_item(line: &str) -> bool {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
    let bytes = line.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = line[from..].find("id:") {
        let at = from + rel;
        let left_ok = at == 0 || !is_word(bytes[at - 1] as char);
        let value = line[at + 3..].trim_start();
        if left_ok && value.starts_with('"') {
            return true;
        }
        from = at + 3;
        if from >= line.len() {
            break;
        }
    }
    false
}

/// 切出 `function <name>(` 之后的**函数体**（含收尾 `}`）。
///
/// 按行收尾：本文件的 JS 函数体一律 2 空格缩进，收尾行恰为 `  }`。
/// 不用括号配对是因为体里有字符串与正则字面量（`/[&<>"']/g` 这种，手写配对会被引号骗到）；
/// 而「首个恰为 `  }` 的行」在这种缩进约定下是稳定的。**调用方必须自证提取器停对了地方**
/// （见 `the_body_extractor_stops_at_the_right_place`），否则它会静默吞掉紧随其后的函数。
fn js_function_body<'a>(src: &'a str, name: &str) -> Option<&'a str> {
    let head = format!("function {name}(");
    let start = src.find(&head)?;
    let rest = &src[start..];
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        offset += line.len();
        if line.trim_end_matches(['\n', '\r']) == "  }" {
            return Some(&rest[..offset]);
        }
    }
    None
}

/// 从一行里取出 `function NAME(` 的 `NAME`（只认行首声明，注释里的 `function` 不算边界）。
fn function_name(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let rest = t
        .strip_prefix("async function ")
        .or_else(|| t.strip_prefix("function "))?;
    let name = rest.split('(').next()?;
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    {
        return None;
    }
    Some(name)
}

/// `line` 里 `needle` 之后是否跟着一个赋值号（`=`，但不是 `==`）。
///
/// `Live.transactions` 的**读取**（`if (Live.transactions)`、`Live.transactions.items`、
/// 甚至就地改字段的 `Live.transactions.trend = t`）都必须判成「不是写」，否则每个消费者
/// 都会被当成写者。因此先跨过标识符剩余字符，再看后面是不是恰好一个赋值。
fn assignment_after(line: &str, needle: &str) -> bool {
    let bytes = line.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = line[from..].find(needle) {
        let at = from + rel;
        let mut end = at + needle.len();
        // 跨过同一个标识符的剩余部分（`Live.transaction` 不该匹配 `Live.transactions`）
        if needle.ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_' || bytes[end] == b'$')
            {
                end += 1;
            }
        }
        let trimmed = line[end..].trim_start();
        if trimmed.starts_with('=') && !trimmed.starts_with("==") {
            return true;
        }
        from = at + 1;
        if from >= line.len() {
            break;
        }
    }
    false
}

/// 这一行是否**写到**了 `Live.<slot>`（重新绑定，或走通用缓存的 `liveLoad("<slot>", …)`）。
fn writes_slot(line: &str, slot: &str) -> bool {
    assignment_after(line, &format!("Live.{slot}"))
        || line.contains(&format!("liveLoad(\"{slot}\""))
}

/// 这一行是否**写到**了有效性证据（`txTable.loaded* = …`）。读取（`!==`）不算。
fn writes_evidence(line: &str) -> bool {
    assignment_after(line, TX_EVIDENCE)
}

/// 一行里 `D.USER.balance = <expr>` 的 `<expr>`（C2145）。
///
/// 只认**重新绑定**：`+=` / `-=`（相对量）与 `==` / `===` / `!==`（读取）都返回 `None`。
/// 返回值保留原文本（可能带尾随 `;` 与同一行的后续语句），由调用方自行判读。
fn session_balance_rhs(line: &str) -> Option<String> {
    let at = line.find(SESSION_BALANCE)?;
    let rest = line[at + SESSION_BALANCE.len()..].trim_start();
    let rest = rest.strip_prefix('=')?;
    if rest.starts_with('=') {
        return None; // `==` / `===`
    }
    Some(rest.trim().to_string())
}

/// 该赋值是不是**相对量**（右值又读了当前值，如 `D.USER.balance + amt`）—— 不是从载荷取的绝对值。
///
/// 只看**第一条语句**（到第一个 `;` 为止）：`session_balance_rhs` 取的是「该行从赋值处到行尾」，
/// 而 UI 里这行的常例是「赋值 + 立刻把它印出来」——
/// `… D.USER.balance = w.balance; $("#side-balance").textContent = D.fmt(D.USER.balance); …`
/// 若按整行判，末尾那次**读取**（`D.fmt(D.USER.balance)`）会把 `w.balance` 伪装成相对量，
/// 规则 1 就此哑掉（实测：改前树上规则 1 不响、只有规则 2 响）。
fn session_balance_rhs_is_relative(rhs: &str) -> bool {
    let stmt = rhs.split(';').next().unwrap_or(rhs);
    stmt.contains(SESSION_BALANCE)
}

/// 一行是否在**取**钱包载荷（`api.get("…/api/wallet")`）。
///
/// 判据必须同时要 `api.get(` 与那个端点：`Live` 对象字面量里那行 `wallet: null,  // GET /api/wallet`
/// 是**代码 + 尾注释**（本模块只剥整行注释），只按端点匹配会把它算成一个读者（坑 #296 的镜像）。
fn fetches_wallet_payload(line: &str) -> bool {
    line.contains("api.get(") && line.contains(WALLET_PAYLOAD)
}

/// 逐行归属到「它之前最近声明的那个函数」，返回命中的 `(归属函数, 行号, 行内容)`。
fn lines_owned_by(src: &str, hit: impl Fn(&str) -> bool) -> Vec<(String, usize, String)> {
    let mut out = Vec::new();
    let mut owner = String::from("<top-level>");
    for (i, line) in src.lines().enumerate() {
        if let Some(name) = function_name(line) {
            owner = name.to_string();
        }
        if hit(line) {
            out.push((owner.clone(), i + 1, line.trim().to_string()));
        }
    }
    out
}

/// 这一行是否在**写** `txTable.page`。
///
/// 判别式有两颗牙：`=`（不是 `==`），且标识符**边界**要收口 —— `txTable.pageSize = 5` 里的
/// `txTable.page` 是**更长标识符的前缀**，不是同一个字段（`assignment_after` 只处理反方向的
/// 前缀，这里必须自己跨过剩余标识符字符）。
fn writes_tx_page(line: &str) -> bool {
    const NEEDLE: &str = "txTable.page";
    let mut from = 0usize;
    while let Some(rel) = line[from..].find(NEEDLE) {
        let at = from + rel;
        let mut end = at + NEEDLE.len();
        while end < line.len()
            && (line.as_bytes()[end].is_ascii_alphanumeric()
                || line.as_bytes()[end] == b'_'
                || line.as_bytes()[end] == b'$')
        {
            end += 1;
        }
        if end > at + NEEDLE.len() {
            from = at + 1; // `txTable.pageSize`：另一个字段，继续往后找
            continue;
        }
        let trimmed = line[end..].trim_start();
        if trimmed.starts_with('=') && !trimmed.starts_with("==") {
            return true;
        }
        from = at + 1;
        if from >= line.len() {
            break;
        }
    }
    false
}

/// 判据（守卫）行里「已加载证据 vs 当前值」比对中**右边**那个签名函数的名字。
///
/// 守卫把三项已加载证据与当前值比对，其中签名那一项的右边是一次**函数调用**（另两项的右边
/// 是 `txTable.page` / `txTable.pageSize` 之类的值）。要在**不假设比对顺序**的前提下取出它：
/// 逐个 `!== ` 的右侧取到第一个 `(` 之间，只有「整段恰好是一个标识符」才算签名函数 ——
/// 前两项的右侧会把后面的比较式一并带进来（含空格），自然被刷掉。
///
/// 找不到调用（守卫被改写成比对一个普通值）⇒ `None`：调用方**必须** `expect` 报错，
/// 否则规则 1 会在空集上假绿。
fn query_sig_name(body: &str) -> Option<String> {
    for line in body.lines() {
        if !line.contains("txTable.loaded") {
            continue;
        }
        let mut from = 0usize;
        while let Some(rel) = line[from..].find("!== ") {
            let at = from + rel + "!== ".len();
            let after = &line[at..];
            if let Some(end) = after.find('(') {
                let name = after[..end].trim();
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
                {
                    return Some(name.to_string());
                }
            }
            from = at;
        }
    }
    None
}

/// 交易控件（`#tx-range` 的绑定）所在的函数名 —— 由**文件顺序**派生，不写名册。
///
/// 不能用「命中行之前最近声明的那个函数」：`bindEvents` 内部还声明了嵌套函数
/// （`showAuthForm`），位置启发式会把归属判给**最后**那个嵌套函数，而控件绑定写在它**之外**、
/// 外层函数体内。改为：取**文件里第一个**「函数体含该控件字面量」的函数 —— 外层函数总在嵌套
/// 函数之前声明，因此拿到的就是属主。
fn tx_control_owner(src: &str) -> Option<String> {
    let mut names: Vec<String> = Vec::new();
    for line in src.lines() {
        if let Some(n) = function_name(line) {
            names.push(n.to_string());
        }
    }
    names
        .into_iter()
        .find(|n| code_body(src, n).contains(TX_RANGE_CONTROL))
}

/// 函数体的**代码文本**（注释已剥离，含 `/* … */` 块）—— 逐行与 [`code_text_by_line`] 对齐。
///
/// 拿 [`function_code`] 的原文判定会被门禁**自己的说明性注释**判红/判绿（坑 #296 的镜像）：
/// 本轮的修法恰好在控件旁边写了两句解释，里面正提到 `loadTransactions()`；而 `code_only` 只剥
/// `//` 行，够不着块注释（坑 #309）。收尾仍按「首个恰为 `  }` 的行」⇒ 只适用于**多行**函数，
/// 调用方须自证提取结果非空且确实是那个函数。
fn code_body(src: &str, name: &str) -> String {
    let text = code_text_by_line(src);
    let mut out: Vec<String> = Vec::new();
    let mut started = false;
    for (i, line) in src.lines().enumerate() {
        if !started {
            if function_name(line) == Some(name) {
                started = true;
            } else {
                continue;
            }
        } else if line == "  }" {
            break;
        }
        if let Some(c) = text.get(i) {
            if !c.is_empty() {
                out.push(c.clone());
            }
        }
    }
    out.join("\n")
}

/// 这段代码是否**提到**交易载荷装载器 `loadTransactions`（以标识符为界）。
///
/// 不能写成 `body.contains("loadTransactions()")`：本轴的两个符号是前缀关系 ——
/// `reloadTransactions()` **以** `loadTransactions()` 结尾（坑 #333 同族：子串匹配会把兄弟
/// 标识符当证据）。调用方传来的文本已由 [`code_body`] 剥离注释。
fn mentions_tx_loader(text: &str) -> bool {
    mentions_identifier(text, "loadTransactions")
}

/// 「重拉触发器」的候选：体内**既重置页码、又直接调用 `loadTransactions()`** 的函数。
///
/// 这就是触发器的语义定义（「重置页码并重拉一次」），因此不需要名册；它必须**唯一** ——
/// 两个候选意味着两条各自独立的触发路径，正是本门禁要挡的形态。判据走 [`code_body`]
/// （注释已剥离），否则解释性注释里提一句 `loadTransactions()` 就会凭空多出一个候选。
fn reload_trigger_candidates(src: &str) -> Vec<String> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    for line in src.lines() {
        if let Some(n) = function_name(line) {
            names.insert(n.to_string());
        }
    }
    names
        .into_iter()
        .filter(|n| {
            let body = code_body(src, n);
            body.lines().any(writes_tx_page) && mentions_tx_loader(&body)
        })
        .collect()
}

fn owners_of(src: &str, hit: impl Fn(&str) -> bool) -> BTreeSet<String> {
    lines_owned_by(src, hit)
        .into_iter()
        .map(|(o, _, _)| o)
        .collect()
}

fn witnesses(src: &str, hit: impl Fn(&str) -> bool) -> Vec<String> {
    lines_owned_by(src, hit)
        .into_iter()
        .map(|(o, n, l)| format!("{o}:{n} {l}"))
        .collect()
}

/// `Live` 的字段名，**从它自己的对象字面量派生**（`const Live = { … };` 之间 `键: 值,` 的键）。
///
/// 派生而非手抄：这份名单是「有哪些槽」的**唯一真源**，新增槽自动出现在这里。
fn live_slots(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in src.lines() {
        if !inside {
            if line.trim_start().starts_with("const Live = {") {
                inside = true;
            }
            continue;
        }
        if line.trim() == "};" {
            break;
        }
        if let Some((name, _)) = line.trim_start().split_once(':') {
            let name = name.trim();
            if !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
            {
                out.push(name.to_string());
            }
        }
    }
    out
}

/// `renderView` 里的分支行（`if (id === "x")` / `else if (id === "x")`）。
fn view_router_branches(body: &str) -> Vec<String> {
    body.lines()
        .filter(|l| l.contains("id === \""))
        .map(|l| l.trim().to_string())
        .collect()
}

// ── C2135：传递闭包（渲染闭包读哪些槽 / loader 闭包写哪些槽）──────────────────────────

/// 函数体里出现的调用名（`ident(` 形状）。用于算**传递闭包**：视图的渲染函数会调用别的渲染
/// 函数（`renderDashboard → renderMonthChanges`），loader 亦然（`loadWallet → refreshDashboard`）。
/// 注释行不参与（否则一段解释性的散文就能造出幻影调用点，坑 #296）。
fn callee_names(body: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in body.lines() {
        let t = line.trim_start();
        if t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') {
            continue;
        }
        let bytes = line.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            let c = bytes[i];
            if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
                let start = i;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$')
                {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b'(' {
                    out.insert(line[start..i].to_string());
                }
            } else {
                i += 1;
            }
        }
    }
    out
}

/// 全仓函数名 → 它的调用名集合（BFS 闭包用）。
fn call_graph(src: &str) -> BTreeMap<String, BTreeSet<String>> {
    let mut graph = BTreeMap::new();
    for line in src.lines() {
        let Some(name) = function_name(line) else {
            continue;
        };
        if graph.contains_key(name) {
            continue;
        }
        if let Some(body) = js_function_body(src, name) {
            graph.insert(name.to_string(), callee_names(body));
        }
    }
    graph
}

/// 从 `roots` 出发能到达的函数集合（含 `roots` 自身）。名字不在图里也保留 —— 外部/未解析的调用
/// 不该让闭包缩水。
fn reachable(graph: &BTreeMap<String, BTreeSet<String>>, roots: &[String]) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut queue: Vec<String> = roots.to_vec();
    while let Some(f) = queue.pop() {
        if !seen.insert(f.clone()) {
            continue;
        }
        if let Some(callees) = graph.get(&f) {
            for c in callees {
                if !seen.contains(c) {
                    queue.push(c.clone());
                }
            }
        }
    }
    seen
}

/// 行里是否出现 `Live.<slot>`（**标识符边界严格**：`Live.dashboardTrend` 不算提到 `dashboard`）。
fn mentions_slot(line: &str, slot: &str) -> bool {
    let needle = format!("Live.{slot}");
    let bytes = line.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = line[from..].find(&needle) {
        let end = from + rel + needle.len();
        let boundary_ok = end >= bytes.len()
            || !(bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_' || bytes[end] == b'$');
        if boundary_ok {
            return true;
        }
        from += rel + 1;
        if from >= line.len() {
            break;
        }
    }
    false
}

/// 这一行是否**写到** `Live.<slot>`。比 [`writes_slot`] 严格：标识符边界必须闭合，
/// 所以 `Live.dashboardTrend = …` **不是**写 `dashboard`（`writes_slot` 会误判为是）。
fn writes_slot_exact(line: &str, slot: &str) -> bool {
    let needle = format!("Live.{slot}");
    let bytes = line.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = line[from..].find(&needle) {
        let at = from + rel;
        let end = at + needle.len();
        let boundary_ok = end >= bytes.len()
            || !(bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_' || bytes[end] == b'$');
        if boundary_ok {
            let trimmed = line[end..].trim_start();
            if trimmed.starts_with('=') && !trimmed.starts_with("==") {
                return true;
            }
        }
        from = at + 1;
        if from >= line.len() {
            break;
        }
    }
    // 通用缓存写入：`liveLoad("<slot>", …)`
    line.contains(&format!("liveLoad(\"{slot}\""))
}

/// 只看代码行（剔除 `//` 行、`/* … */` 块注释行与 `*` 续行）。C2135 的几个闭包判别式用它，
/// 理由与 [`code_only`] 相同，只是块注释也要挡住 —— 否则一段 `/* Live.d */` 就能造出幻影读点。
fn code_lines(src: &str) -> String {
    src.lines()
        .filter(|l| {
            let t = l.trim_start();
            !(t.starts_with("//") || t.starts_with("/*") || t.starts_with('*'))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 某个函数的**代码行**（剔除注释行）。
fn function_code(src: &str, name: &str) -> String {
    js_function_body(src, name)
        .map(code_lines)
        .unwrap_or_default()
}

/// 闭包里是否有人**读** `Live.<slot>`。
fn closure_reads(
    src: &str,
    graph: &BTreeMap<String, BTreeSet<String>>,
    roots: &[String],
    slot: &str,
) -> bool {
    reachable(graph, roots).iter().any(|f| {
        function_code(src, f)
            .lines()
            .any(|l| mentions_slot(l, slot))
    })
}

/// 闭包里是否有人**写** `Live.<slot>`。
fn closure_writes(
    src: &str,
    graph: &BTreeMap<String, BTreeSet<String>>,
    roots: &[String],
    slot: &str,
) -> bool {
    reachable(graph, roots).iter().any(|f| {
        function_code(src, f)
            .lines()
            .any(|l| writes_slot_exact(l, slot))
    })
}

/// 一行里以 `prefix` 开头的调用名（`renderView` 分支里的 `render…` / `load…`）。
fn callee_with_prefix(line: &str, prefix: &str) -> Option<String> {
    callee_names(line)
        .into_iter()
        .find(|c| c.starts_with(prefix))
}

/// 槽宇宙：`Live` 字面量声明的字段 ∪ 代码里出现过的 `Live.<名>`。
///
/// 只用字面量会让**漏登记**的槽静默逃逸（C2135 记账：`dashboardTrend` 只被读写、不在字面量里）。
fn all_live_slots(src: &str) -> Vec<String> {
    let mut out: BTreeSet<String> = live_slots(src).into_iter().collect();
    for line in code_lines(src).lines() {
        let mut from = 0usize;
        while let Some(rel) = line[from..].find("Live.") {
            let start = from + rel + "Live.".len();
            let bytes = line.as_bytes();
            let mut end = start;
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_' || bytes[end] == b'$')
            {
                end += 1;
            }
            if end > start {
                out.insert(line[start..end].to_string());
            }
            from = start;
            if from >= line.len() {
                break;
            }
        }
    }
    out.into_iter().collect()
}

/// boot 处理器（`document.addEventListener("DOMContentLoaded", () => { … });`）的函数体。
///
/// 它**不是** `function NAME(` 声明，所以 [`js_function_body`] 取不到：按行收尾，收尾行恰为
/// `  });`（2 空格缩进 —— 处理器内部的内联箭头函数一律 4 空格缩进收尾）。与兄弟提取器一样，
/// **调用方必须自证它停对了地方**（见 `the_boot_body_extractor_stops_at_the_right_place`）。
fn boot_body(src: &str) -> Option<&str> {
    const HEAD: &str = "document.addEventListener(\"DOMContentLoaded\"";
    let start = src.find(HEAD)?;
    let rest = &src[start..];
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        offset += line.len();
        if line.trim_end_matches(['\n', '\r']) == "  });" {
            return Some(&rest[..offset]);
        }
    }
    None
}

/// 视图层的调用名：`renderView` 每个分支里的 `render…` / `load…`，外加 `renderView` 自己。
///
/// **派生自路由器本身** ⇒ 新增视图自动纳入，不需要在门禁里补一份名册（名册会腐烂）。
fn view_layer_callees(src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    out.insert("renderView".to_string());
    let Some(rv) = js_function_body(src, "renderView") else {
        return out;
    };
    for branch in view_router_branches(rv) {
        if let Some(r) = callee_with_prefix(&branch, "render") {
            out.insert(r);
        }
        if let Some(l) = callee_with_prefix(&branch, "load") {
            out.insert(l);
        }
    }
    out
}

/// 一行里 `renderView(` 的实参（`renderView(` 之后到第一个 `)` 之间的文本）。
///
/// 返回 `None` 表示这一行没调用 `renderView`。注释行由调用方先剔除。
fn render_view_argument(line: &str) -> Option<String> {
    let at = line.find("renderView(")?;
    let after = &line[at + "renderView(".len()..];
    let end = after.find(')').unwrap_or(after.len());
    Some(after[..end].trim().to_string())
}

/// 这一行是否在**渲染** `attr` 这个 HTML 属性（`data-x="…"`）。
///
/// 判别式有两条牙：属性后面跟 `=`（渲染侧写 `data-x=`），且这一行**不是选择器查询**
/// （`querySelector('[data-use-model="' + id + '"]')` 也带 `=`，但它是**读取**，不是产出身份）。
/// 不区分的话，「谁渲染了身份」会被消费者污染（C2138 A/B 实测：`closest("[data-mk-expand]")`
/// 与 `querySelector('[data-use-model="' + id + '"]')` 都被算成渲染点）。
fn renders_attr(line: &str, attr: &str) -> bool {
    line.contains(&format!("{attr}=")) && !is_selector_query(line)
}

/// 这一行是否在做**选择器查询**（读 DOM 里的控件，而不是拼 HTML）。
fn is_selector_query(line: &str) -> bool {
    line.contains("querySelector") || line.contains("closest(") || line.contains("getElementById")
}

/// 这一行是否在**读** `attr` 这个 HTML 属性（`closest("[data-x]")` / `querySelector`）。
fn reads_attr(line: &str, attr: &str) -> bool {
    line.contains(&format!("[{attr}]")) || line.contains(&format!("{attr}]"))
}

/// 取一个函数的源码。**单行函数**（`function f(m) { return …; }`）只取那一行 ——
/// [`js_function_body`] 按「首个恰为 `  }` 的行」收尾，而单行函数的收尾 `}` 在同一行里，
/// 于是它会一路吞到**下一个**多行函数的收尾（模型身份就是这种单行函数，坑 #319）。
/// 与兄弟提取器一样，**调用方必须自证**（见 `the_model_identity_extractors_have_teeth`）。
fn function_source(src: &str, name: &str) -> Option<String> {
    let head = format!("function {name}(");
    let start = src.find(&head)?;
    let line_end = src[start..]
        .find('\n')
        .map(|i| start + i)
        .unwrap_or(src.len());
    let first_line = src[start..line_end].trim_end();
    if first_line.ends_with('}') && first_line.contains('{') {
        return Some(first_line.to_string());
    }
    js_function_body(src, name).map(str::to_string)
}

/// 一个 `.map((…) =>` 回调的形参表（已去空白）。`list.map((m) =>` → `["m"]`、`map((m, i) =>` → `["m","i"]`。
fn map_callback_params(body: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = body[from..].find(".map((") {
        let at = from + rel + ".map((".len();
        let end = body[at..].find(')').map(|i| at + i).unwrap_or(body.len());
        out.push(
            body[at..end]
                .split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect(),
        );
        from = at;
        if from >= body.len() {
            break;
        }
    }
    out
}

/// `text` 里是否把 `name` 当**标识符**提到（前后不是标识符字符）。`id` 不该被 `valid` 之类绊到。
fn mentions_identifier(text: &str, name: &str) -> bool {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
    let bytes = text.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = text[from..].find(name) {
        let at = from + rel;
        let end = at + name.len();
        let before_ok = at == 0 || !is_word(bytes[at - 1] as char);
        let after_ok = end >= bytes.len() || !is_word(bytes[end] as char);
        if before_ok && after_ok {
            return true;
        }
        from = at + 1;
        if from >= text.len() {
            break;
        }
    }
    false
}

/// 这一行是否在**对象字面量里声明了字段** `field`（`id: i,` / `{ id: i }` / `, id: i`）。
///
/// 不认 `x.id:` 这类属性访问（前一个非空白字符既不是行首，也不是 `{`/`,`）。
fn declares_field(line: &str, field: &str) -> bool {
    let needle = format!("{field}:");
    let t = line.trim();
    let mut from = 0usize;
    while let Some(rel) = t[from..].find(&needle) {
        let at = from + rel;
        let before = t[..at].trim_end().chars().last();
        if before.is_none() || matches!(before, Some('{') | Some(',')) {
            return true;
        }
        from = at + 1;
        if from >= t.len() {
            break;
        }
    }
    false
}

// ── C2142：管理页「总余额」卡的**取值**必须折进它副标题具名的每一项 ─────────────────────

/// 「总余额」卡片副标题的语言包键。它是本卡**意义的陈述**（不是装饰）：
/// 「余额 + 赠送」（zh）/ `balance + gift`（en）。
const ADMIN_TOTAL_CARD_SUBTITLE: &str = "admin.emp.stats.total.sub";

/// i18n 语言包源码：`app.js` 只放**键**，这句话的正文在语言包里（本门禁要读两边）。
const I18N_JS: &str = include_str!("../ui/js/i18n.js");

/// 语言包区段标记（口径同 `i18n_pack.rs`：终点取对象字面量自身的收尾，不取 `window.I18N`）。
const ZH_PACK_START: &str = "var ZH = {";
const EN_PACK_START: &str = "var EN = {";
const PACK_END: &str = "\n  };";

// ── C2146：交易载荷的**载荷签名**必须覆盖每一个改变请求体的状态 ──────────────────────────────

/// 决定交易载荷请求体的**时间段**状态（模块级变量，与列筛选是两份状态）。
///
/// 交易列表/趋势请求的 URL 由 `txRangeParams()` 从这三个变量渲染，而「缓存还新不新」的判据是
/// `renderTransactions` 里的签名比对 —— 判据必须覆盖**渲染请求体的全部输入**，否则那个控件
/// 改完没人重拉，只能由调用方补一次显式拉取，而补的那一次会在判据也成立时并发第二次请求。
const TX_RANGE_STATE: [&str; 3] = ["txRange", "txCustomStart", "txCustomEnd"];

/// 时间段 → 查询参数的渲染器。它含「now − 窗口」的毫秒时间戳，**每次调用都不同** ⇒
/// 拿它当签名会让签名恒变（守卫每次渲染都重拉，形成请求风暴）。签名只许取状态值。
const TX_RANGE_RENDERER: &str = "txRangeParams(";

/// 列筛选状态（签名必须覆盖的另一半）。
const TX_FILTER_STATE: &str = "txTable.filters";

/// `#tx-range` 控件所在的绑定函数（交易控件的归属函数由它派生，不写名册）。
const TX_RANGE_CONTROL: &str = "\"#tx-range\"";

// ── C2145：会话余额是一个**事实**，只能读钱包载荷的**可花额**那一半 ───────────────────────

/// 会话余额这个事实在客户端的名字（侧栏 `#side-balance`、钱包页、聊天余额都印它）。
const SESSION_BALANCE: &str = "D.USER.balance";

/// 钱包载荷的字面量端点。
const WALLET_PAYLOAD: &str = "/api/wallet";

/// 允许**取**钱包载荷的函数：会话建立（`loadSession`）与缓存写者（`refreshWallet`）。
///
/// 两者缺一不可、多一不可：`loadSession` 在 boot/登录时装上会话（此时还没有任何视图 loader），
/// `refreshWallet` 是缓存槽 `Live.wallet` 的**唯一写者**。任何**第三处**取载荷的代码都同时
/// 犯了两个错 —— 它自己造了「同一事实的第二个来源」，却又不更新缓存，于是屏幕上的数字与
/// 缓存必然漂移（C2145 实测：运营者给自己充值后侧栏少了赠送额，而 `Live.wallet` 停在旧值）。
const WALLET_READERS: [&str; 2] = ["loadSession", "refreshWallet"];

/// 钱包缓存的槽名（`Live.wallet`）。
const WALLET_SLOT: &str = "wallet";

/// 共享状态的过渡表与它必须覆盖的状态集合（两侧都**从源码推出来**，不写花名册）。
const SHARE_TOGGLE_TABLE: &str = "SHARE_TOGGLE";
const SHARE_STATUS_TABLE: &str = "SHARE_STATUS";

/// 过渡表里**逐条目必须唯一**的两列：动作标签与结局文案。
///
/// 两列共用一个值的两条目 = 两个动作被报成同一个结局 —— 那正是 C2153 的缺陷本身。
const SHARE_TOGGLE_UNIQUE_COLUMNS: [&str; 2] = ["label", "outcome"];

/// 过渡表条目命名的键前缀（`share.toggle.relisted` 一族）。
const SHARE_TOGGLE_KEY_PREFIX: &str = "\"share.toggle.";

/// 行内切换按钮的点击句柄（`data-share-toggle=`）。它同时是「动作」与「结局」的接线点。
const SHARE_TOGGLE_BUTTON: &str = "data-share-toggle=";

/// 按钮标记的收尾 —— 动作标签与 `data-share-toggle=` 不必在同一行（本仓就是分成两行的）。
const SHARE_TOGGLE_BUTTON_END: &str = "</button>";

/// 语言层原语：任何「两侧共用的访问器」判别式都必须先把它排除（它在每一行都会出现）。
const I18N_PRIMITIVE: &str = "T";

/// 去掉一行里**字符串之外**的 `// …` 尾注释（成对 `/* … */` 由 [`code_text_by_line`] 处理）。
///
/// 门禁被自己的说明性注释满足是假绿里最坏的一种（坑 #296 的镜像）：本轮的修法就会在合计旁边
/// 写一句解释，里面正提到 `gift_balance`。
fn strip_trailing_comment(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut in_str: Option<u8> = None;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        match in_str {
            Some(q) => {
                if c == b'\\' {
                    i += 2;
                    continue;
                }
                if c == q {
                    in_str = None;
                }
            }
            None => {
                if c == b'"' || c == b'\'' || c == b'`' {
                    in_str = Some(c);
                } else if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    return line[..i].to_string();
                }
            }
        }
        i += 1;
    }
    line.to_string()
}

/// 代码文本：逐行剔除注释（行首 `//`、`/* … */` 块、行尾尾注释）。C2142 的一切证据文本都过它 ——
/// 一段解释性散文不该满足（也不该破坏）断言（坑 #296 与它的镜像）。
fn code_text(src: &str) -> String {
    code_text_by_line(src)
        .iter()
        .map(|l| strip_trailing_comment(l))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 文本里的标识符 token（`[A-Za-z_$][A-Za-z0-9_$]*`），**字符串字面量里不算**。
fn identifiers(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for raw in text.lines() {
        let line = strip_trailing_comment(raw);
        let bytes = line.as_bytes();
        let mut i = 0usize;
        let mut in_str: Option<u8> = None;
        while i < bytes.len() {
            let c = bytes[i];
            if let Some(q) = in_str {
                if c == b'\\' {
                    i += 2;
                    continue;
                }
                if c == q {
                    in_str = None;
                }
                i += 1;
                continue;
            }
            if c == b'"' || c == b'\'' || c == b'`' {
                in_str = Some(c);
                i += 1;
                continue;
            }
            if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
                let start = i;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$')
                {
                    i += 1;
                }
                out.insert(line[start..i].to_string());
            } else {
                i += 1;
            }
        }
    }
    out
}

/// `prefix` + 标识符 在 `src` 里出现过的**完整名字**（去重、字典序）。
///
/// 左边界也锚：`xtxTable.loaded` 不算（坑 #333 同族：兄弟标识符不是证据）。
fn names_with_prefix(src: &str, prefix: &str) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    let bytes = src.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = src[from..].find(prefix) {
        let at = from + rel;
        from = at + prefix.len();
        if at > 0 && is_ident_byte(bytes[at - 1]) {
            continue;
        }
        let rest = &src[at + prefix.len()..];
        let tail: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !tail.is_empty() {
            out.insert(format!("{prefix}{tail}"));
        }
    }
    out.into_iter().collect()
}

/// `text` 里有没有对 `lhs` 的**赋值**（`lhs = …`）。
///
/// **两侧都锚**：右侧必须是分隔符 —— `txTable.page` 是 `txTable.pageSize` 的前缀、
/// `txTable.loadedPage` 是 `txTable.loadedPageSize` 的前缀，只锚左侧就会把「复位了兄弟字段」
/// 判成「复位了它」（坑 #333）。`==` / `===` / `=>` 都不是赋值。
fn assigns_in(text: &str, lhs: &str) -> bool {
    let mut from = 0usize;
    while let Some(rel) = text[from..].find(lhs) {
        let at = from + rel;
        from = at + lhs.len();
        if at > 0 && is_ident_byte(text.as_bytes()[at - 1]) {
            continue;
        }
        let after = &text[at + lhs.len()..];
        if let Some(c) = after.chars().next() {
            if c.is_ascii_alphanumeric() || c == '_' {
                continue;
            }
        }
        let t = after.trim_start();
        if let Some(rest) = t.strip_prefix('=') {
            if !rest.starts_with('=') && !rest.starts_with('>') {
                return true;
            }
        }
    }
    false
}

/// 模块级 `let`（行首 2 空格缩进）的名字 —— 会话级视图状态就长这样（`app.js:15-22`）。
fn module_level_lets(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in src.lines() {
        if let Some(rest) = line.strip_prefix("  let ") {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                out.push(name);
            }
        }
    }
    out
}

/// `const TABLE = { … }` 字面量里 `field` 声明处的字面量文本（到 `,` / `}` 为止）。
fn declared_literal(src: &str, table: &str, field: &str) -> Option<String> {
    let (open, close) = object_literal_span(src, table)?;
    let body = &src[open + 1..close - 1];
    let needle = format!("{field}:");
    let at = body.find(&needle)? + needle.len();
    let rest = &body[at..];
    // `[',', '}']` (a char-array pattern) rather than a closure: CI runs
    // `cargo clippy --all-targets -- -D warnings`, and the closure form trips
    // `clippy::manual_pattern_char_comparison` (measured: rustc/clippy 1.98.0).
    let end = rest.find([',', '}']).unwrap_or(rest.len());
    Some(rest[..end].trim().to_string())
}

/// `let NAME = <字面量>;` 声明处的字面量文本。
fn declared_let_literal(src: &str, name: &str) -> Option<String> {
    let needle = format!("let {name} = ");
    let at = src.find(&needle)? + needle.len();
    let rest = &src[at..];
    let end = rest.find(';')?;
    Some(rest[..end].trim().to_string())
}

/// 身份边界复位函数的**闭包**：`resetSessionCaches` 可达的函数名集合 + 它们的代码文本。
///
/// 闭包是必需的 —— 把复位抽进 `resetTxView()` 正是推荐写法（「一张表一处真源」），只看
/// `resetSessionCaches` 自己的函数体会把抽出去的复位判成「没复位」。
fn boundary_reset(src: &str) -> (String, Vec<String>) {
    let graph = call_graph(src);
    // ⚠️ **只保留本文件声明过的函数**（第 94 轮实测修）。`call_graph()` 的值是「行里以 `(` 结尾的
    //    标识符」（`callee_names`），所以**方法名**（`Object.keys(...)`、`.forEach(...)`、`.push(...)`）
    //    也会进图；而 `reachable()` 不看对方在不在图里就往 `seen` 里插 ⇒ 闭包混进
    //    `forEach`/`keys`。它们没有函数体 ⇒ 下面那条自证「函数声明数 == 闭包大小」在**每一棵树**上
    //    都为假（实测 live `1≠3`、fix `2≠4`）⇒ 门禁不是「诚实地红」，是**坏的**。
    //    图的键**恰好**是声明集合（`call_graph` 只插 `function_name()` 认出来的名字）。
    let reach: Vec<String> = reachable(&graph, &["resetSessionCaches".to_string()])
        .into_iter()
        .filter(|f| graph.contains_key(f))
        .collect();
    let mut text = String::new();
    for f in &reach {
        text.push_str(&function_code(src, f));
        text.push('\n');
    }
    (text, reach)
}

//

/// `stat(<label>, <value>, <sub>…)` 调用里**第二段实参**（取值表达式）的文本。
///
/// 逗号按圆括号配平切分、字符串里的逗号不算分隔符 ⇒ 取值里可以嵌函数调用
/// （`D.fmt(total) + " " + T("common.points")` 整段取出）。返回 `None` 表示这一行不是一张三段式卡片。
fn stat_value_argument(line: &str) -> Option<String> {
    let at = line.find("stat(")? + "stat(".len();
    let rest = &line[at..];
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut depth = 0isize;
    let mut in_str: Option<char> = None;
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        if let Some(q) = in_str {
            cur.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            } else if c == q {
                in_str = None;
            }
            continue;
        }
        match c {
            '"' | '\'' | '`' => {
                in_str = Some(c);
                cur.push(c);
            }
            '(' | '[' | '{' => {
                depth += 1;
                cur.push(c);
            }
            ')' | ']' | '}' => {
                if c == ')' && depth == 0 {
                    parts.push(cur.clone());
                    break;
                }
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                parts.push(cur.clone());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if parts.len() >= 2 {
        Some(parts[1].trim().to_string())
    } else {
        None
    }
}

/// **取值表达式**（连同它在本函数里用到的变量定义、以及这些文本调用的函数体）是否**每一项**都读到。
///
/// 闭包的意义：等价写法都该放行 —— `D.fmt(total)`（`total` 的定义在别处）、
/// `D.fmt(total + giftTotal)`、`D.fmt(memberSum(users))`。门禁钉的是**读到了**，不是写法。
/// 所有证据文本都剥掉注释（行首 / 成对块 / 行尾尾注释）—— 见 [`strip_trailing_comment`]。
///
/// ⚠️ 两项都要钉：只钉 `gift_balance` 会放行「合计里只剩赠送」这种把轴修反的写法（探针会拒、
/// 门禁不会 ⇒ 门禁比探针松一格）。反过来也**不能**用 `contains("balance")` —— `gift_balance`
/// 本身就以它结尾，哑字符串匹配恒真 ⇒ 必须按标识符 token 比（[`identifiers`] 已排除字符串字面量）。
fn value_reaches_all_fields(src: &str, scope: &str, value: &str, fields: &[&str]) -> bool {
    let mut texts: Vec<String> = vec![code_text(value)];
    // 变量定义：一层层往里展开（`D.fmt(total)` → `const total = …`），循环有界。
    for _ in 0..4 {
        let names: Vec<String> = texts
            .iter()
            .flat_map(|t| identifiers(t).into_iter())
            .collect();
        let mut added = false;
        for n in names {
            if let Some(stmt) = assignment_statement(scope, &n) {
                let stmt = code_text(&stmt);
                if !texts.contains(&stmt) {
                    texts.push(stmt);
                    added = true;
                }
            }
        }
        if !added {
            break;
        }
    }
    // 只有**标识符 token** 算证据（`gift_balance` 含 `balance` ⇒ 子串匹配是哑的）。
    let mut seen_ids: BTreeSet<String> = BTreeSet::new();
    for t in &texts {
        seen_ids.extend(identifiers(t));
    }
    if fields.iter().all(|f| seen_ids.contains(*f)) {
        return true;
    }
    // 函数调用：`memberSum(users)` 这类 helper 的体内也算。
    //
    // ⚠️ 这里的闭包**不能用 `call_graph` + `reachable`**（C2135 那两个 helper 建在
    // `js_function_body` 上）：`js_function_body` 按「首个恰为 `  }` 的行」收尾，**单行函数**
    // （`function memberAvailSum(list) { return …; }` —— 本轴最自然的抽法）的收尾 `}` 就在同一行，
    // 于是它会一路吞到下一个多行函数的收尾（坑 #319）。A/B 实测：M6「抽成 helper 但漏了赠送」
    // 因此假绿 —— 被吞进来的那段区域里有 `(u.gift_balance || 0)`。改用 `function_source`
    // （单行安全）自己走闭包。
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut queue: Vec<String> = texts
        .iter()
        .flat_map(|t| callee_names(t).into_iter())
        .collect();
    let mut reached: BTreeSet<String> = BTreeSet::new();
    while let Some(f) = queue.pop() {
        if !seen.insert(f.clone()) {
            continue;
        }
        let Some(body) = function_source(src, &f) else {
            continue;
        };
        let body = code_text(&body);
        let mut body_ids = identifiers(&body);
        if fields.iter().all(|x| body_ids.contains(*x)) {
            return true;
        }
        // 一个 helper 只覆盖一部分项时（`D.fmt(total + giftSum(users))`），其余项仍须在别处读到
        // ⇒ 累计所有走到过的函数体里的标识符，最后统一判定。
        reached.append(&mut body_ids);
        for c in callee_names(&body) {
            if !seen.contains(&c) {
                queue.push(c);
            }
        }
    }
    fields
        .iter()
        .all(|f| seen_ids.contains(*f) || reached.contains(*f))
}

/// 语言包里某个键的字符串值（`"key": "value",` 的第一处）。取不到 ⇒ `None`。
///
/// 只认「`"key": ` 紧跟一个字符串字面量」这一种形状 —— 值里没有转义引号（真源码如此）。
fn pack_string(region: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\": ");
    let at = region.find(&needle)? + needle.len();
    let rest = region[at..].strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// 切出语言包区段（起点标记 → 终点标记，含起点）。口径同 `i18n_pack::pack_region`。
fn pack_region_strict<'a>(src: &'a str, start_mark: &str, end_mark: &str) -> &'a str {
    let s = src
        .find(start_mark)
        .unwrap_or_else(|| panic!("语言包起点标记 `{start_mark}` 未找到 —— 语言包结构变了？"));
    let rest = &src[s..];
    let e = rest
        .find(end_mark)
        .unwrap_or_else(|| panic!("语言包终点标记 `{end_mark}` 未找到 —— 语言包结构变了？"));
    &rest[..e]
}

/// 副标题是否**同时具名两项**：中文含「余额」+「赠送」，英文含 `balance` + `gift`（大小写不敏感）。
///
/// 这一条挡的是「把承诺删掉让两边对上」那种化妆式修法：卡片与副标题若不一致，两个方向都能
/// 让它们一致，而**哪个方向才对是由产品自己的定义决定的**（见门禁的文档注释）。
fn caption_names_both_components(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    (value.contains("余额") && value.contains("赠送"))
        || (lower.contains("balance") && lower.contains("gift"))
}

/// `ident` 的**赋值语句**文本：从 `const <ident> = ` / `let <ident> = ` / `<ident> = ` 那一行起，
/// 按圆括号配平向后延伸（允许跨行），直到深度回到 0。
///
/// 归属判别式：赋值头前面那个非空白字符必须是**语句边界**（行首 / `{` / `;`）—— 只认行首会漏掉
/// 单行函数（`function f() { const total = … }`），不认边界则 `const total = ` 里那截
/// `total = ` 会把任何提到它的行都算成赋值。
///
/// 只数 `(` / `)`：真源码里 reduce 的字符串字面量不含圆括号（自证测试覆盖该形状）。`ident` 在同一
/// 函数里出现两次赋值 ⇒ 返回 `None`（调用方据此判红：归属必须唯一）。
fn assignment_statement(src: &str, ident: &str) -> Option<String> {
    let heads = [
        format!("const {ident} = "),
        format!("let {ident} = "),
        format!("{ident} = "),
    ];
    let lines: Vec<&str> = src.lines().collect();
    let mut found: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        if t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') {
            continue;
        }
        let hit = heads.iter().any(|h| {
            let mut from = 0usize;
            while let Some(rel) = line[from..].find(h.as_str()) {
                let at = from + rel;
                let before = line[..at].trim_end().chars().last();
                if before.is_none() || matches!(before, Some('{') | Some(';')) {
                    return true;
                }
                from = at + 1;
                if from >= line.len() {
                    break;
                }
            }
            false
        });
        if !hit {
            continue;
        }
        if found.is_some() {
            return None; // 同一函数里两次赋值 ⇒ 归属不唯一
        }
        found = Some(i);
    }
    let start = found?;
    let mut depth: isize = 0;
    let mut out = String::new();
    for line in &lines[start..] {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
        for c in line.chars() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
        }
        if depth <= 0 {
            break;
        }
    }
    Some(out)
}

/// 设置卡片里的一个表单控件（`<input|select|textarea|button>`）。
///
/// `value_bearing` 把「有值的控件」与「按钮」分开：前者必须让**值**流进产品代码才算接线，
/// 后者绑上监听器就算（按钮没有值可读）。
struct HtmlControl {
    tag: String,
    id: Option<String>,
    name: Option<String>,
    value_bearing: bool,
    inert: bool,
}

/// 去掉 `<!-- … -->`（HTML 注释不参与断言 —— 修法自己就会在控件旁边写解释）。
fn strip_html_comments(src: &str) -> String {
    let mut out = String::new();
    let mut rest = src;
    loop {
        match rest.find("<!--") {
            None => {
                out.push_str(rest);
                return out;
            }
            Some(open) => {
                out.push_str(&rest[..open]);
                match rest[open..].find("-->") {
                    Some(close) => rest = &rest[open + close + 3..],
                    None => return out,
                }
            }
        }
    }
}

/// 设置视图的 `<section>` 区段（射程起点；`</section>` 收尾 —— 视图里没有嵌套 section）。
fn settings_view(html: &str) -> Option<&str> {
    let at = html.find(SETTINGS_VIEW_ID)?;
    let rest = &html[at..];
    let end = rest.find("</section>").unwrap_or(rest.len());
    Some(&rest[..end])
}

/// 一张配置卡的区段：从它的 `<h3 data-i18n="KEY">` 起到**下一张卡**（或视图结束）。
///
/// 三张卡是 `settings-grid` 里的兄弟节点，因此「下一处 `<div class="card`」就是下界；
/// 标记里的引号让 `settings.account` **不会**被 `settings.account.nickname` 命中。
fn card_region<'a>(view: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!("data-i18n=\"{key}\"");
    let at = view.find(&marker)?;
    let rest = &view[at..];
    let end = rest.find(CARD_PREFIX).unwrap_or(rest.len());
    Some(&rest[..end])
}

/// `attrs` 里属性 `name` 的引号值（不做词法分析：调用方的属性串只含标签属性）。
fn html_attr_value(attrs: &str, name: &str) -> Option<String> {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let bytes = attrs.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = attrs[from..].find(name) {
        let at = from + rel;
        let before_ok = at == 0 || !is_word(bytes[at - 1] as char);
        if before_ok {
            let t = attrs[at + name.len()..].trim_start();
            if let Some(t) = t.strip_prefix('=') {
                let t = t.trim_start();
                if let Some(q) = t.chars().next() {
                    if q == '"' || q == '\'' {
                        if let Some(end) = t[1..].find(q) {
                            return Some(t[1..1 + end].to_string());
                        }
                    }
                }
            }
        }
        from = at + 1;
        if from >= attrs.len() {
            break;
        }
    }
    None
}

/// `attrs` 里是否出现**独立**的属性 `tok`（`readonly` / `disabled`；`data-readonly` 不算）。
fn html_has_token(attrs: &str, tok: &str) -> bool {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let bytes = attrs.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = attrs[from..].find(tok) {
        let at = from + rel;
        let end = at + tok.len();
        let before_ok = at == 0 || !is_word(bytes[at - 1] as char);
        let after_ok = end >= bytes.len() || !is_word(bytes[end] as char);
        if before_ok && after_ok {
            return true;
        }
        from = at + 1;
        if from >= attrs.len() {
            break;
        }
    }
    false
}

/// 区段里出现的下一个表单控件标签：`(标签名, `<` 处的下标)`。
fn next_control_tag(src: &str, from: usize) -> Option<(&'static str, usize)> {
    let mut best: Option<(&'static str, usize)> = None;
    for tag in ["input", "select", "textarea", "button"] {
        let needle = format!("<{tag}");
        let mut at = from;
        while let Some(rel) = src[at..].find(&needle) {
            let i = at + rel;
            let after = src
                .as_bytes()
                .get(i + needle.len())
                .copied()
                .map(|b| b as char);
            if matches!(after, Some(' ') | Some('\t') | Some('\n') | Some('>')) {
                if best.is_none_or(|(_, b)| i < b) {
                    best = Some((tag, i));
                }
                break;
            }
            at = i + 1;
            if at >= src.len() {
                break;
            }
        }
    }
    best
}

/// 区段里的全部表单控件。
fn parse_controls(region: &str) -> Vec<HtmlControl> {
    let src = strip_html_comments(region);
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some((tag, at)) = next_control_tag(&src, from) {
        let after = at + tag.len() + 1;
        let end = src[after..]
            .find('>')
            .map(|i| after + i)
            .unwrap_or(src.len());
        let attrs = &src[after..end];
        let ty = html_attr_value(attrs, "type");
        let value_bearing = !(tag == "button"
            || matches!(
                ty.as_deref(),
                Some("button") | Some("submit") | Some("reset") | Some("image")
            ));
        out.push(HtmlControl {
            tag: tag.to_string(),
            id: html_attr_value(attrs, "id"),
            name: html_attr_value(attrs, "name"),
            value_bearing,
            inert: html_has_token(attrs, "readonly") || html_has_token(attrs, "disabled"),
        });
        from = end + 1;
    }
    out
}

/// 一个控件能被 `app.js` 认出来的句柄：`#id`，以及同名组的 `name="…"`（单选组靠它接线）。
fn control_handles(c: &HtmlControl) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(id) = &c.id {
        out.push(format!("#{id}"));
    }
    if let Some(name) = &c.name {
        out.push(format!("name=\"{name}\""));
    }
    out
}

/// 值读取处左边那个调用名（`applyTheme(sel.value)` ⇒ `applyTheme`）。
///
/// 只认**紧邻**的左括号：`const cur = sel.value;` 左边是 `=` ⇒ `None`
/// （把当前值读进局部变量只是为了重渲染时还原，不是消费）。
fn enclosing_callee(code: &str, at: usize) -> Option<String> {
    let bytes = code.as_bytes();
    let mut i = at;
    while i > 0 && (bytes[i - 1] as char).is_whitespace() {
        i -= 1;
    }
    if i == 0 || bytes[i - 1] as char != '(' {
        return None;
    }
    let close = i - 1;
    let mut k = close;
    while k > 0 {
        let c = bytes[k - 1] as char;
        if c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '.' {
            k -= 1;
        } else {
            break;
        }
    }
    let name = code[k..close].trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// 读值处的右侧是不是赋值（`sel.value = …` 是；`sel.value === x` 是比较，不是赋值）。
fn is_assignment(code: &str, at: usize) -> bool {
    let rest = code[at..].trim_start();
    if !rest.starts_with('=') {
        return false;
    }
    !rest[1..].trim_start().starts_with('=')
}

fn is_ident_byte(b: u8) -> bool {
    (b as char).is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

/// 这个变量的**值**是否真的流进了产品代码。
///
/// 判据是「值被读、且那次读取是某个**非存储**调用的实参」。三条反例都被判否：
/// ① 赋值给控件本身（`sel.value = cur;`）；② 只写进 `localStorage.setItem(…, sel.value)`；
/// ③ 读进局部变量给重渲染用（`const cur = sel.value;`）。
/// 这就是本门禁与「加个监听器 + 存进 localStorage」的分界线（坑 #338 的推广）。
fn value_reaches_product(code: &str, var: &str) -> bool {
    let bytes = code.as_bytes();
    for prop in ["value", "checked"] {
        let needle = format!("{var}.{prop}");
        let mut from = 0usize;
        while let Some(rel) = code[from..].find(&needle) {
            let at = from + rel;
            let end = at + needle.len();
            // 标识符 token 边界（坑 #333：`themeSel.value` 不是 `sel.value`）
            let before_ok = at == 0 || !is_ident_byte(bytes[at - 1]);
            let after_ok = end >= bytes.len() || !is_ident_byte(bytes[end]);
            if before_ok && after_ok && !is_assignment(code, end) {
                if let Some(callee) = enclosing_callee(code, at) {
                    let last = callee.rsplit('.').next().unwrap_or("");
                    if !STORAGE_METHODS.contains(&last) {
                        return true;
                    }
                }
            }
            from = at + 1;
            if from >= bytes.len() {
                break;
            }
        }
    }
    false
}

/// 控件被绑到哪些变量名上：`X = $("#id")` / `X = document.querySelector(…)` /
/// `querySelectorAll(…).forEach((X) =>`（单选组靠最后一种接线）。
fn bound_vars(code: &str, handle: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut from = 0usize;
    while let Some(rel) = code[from..].find(handle) {
        let at = from + rel;
        if let Some(v) = assign_target_before(code, at) {
            out.insert(v);
        }
        if let Some(v) = foreach_param_after(code, at + handle.len()) {
            out.insert(v);
        }
        from = at + 1;
        if from >= code.len() {
            break;
        }
    }
    out
}

/// 这一处句柄的左边是不是 `X = $(` / `X = document.querySelector(` / `…All(`。
fn assign_target_before(code: &str, at: usize) -> Option<String> {
    let bytes = code.as_bytes();
    let is_sel_char = |c: char| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                '_' | '$' | '[' | ']' | '"' | '\'' | '.' | ':' | '-' | '#' | ' '
            )
    };
    let mut i = at;
    while i > 0 && is_sel_char(bytes[i - 1] as char) {
        i -= 1;
    }
    if i == 0 || bytes[i - 1] as char != '(' {
        return None;
    }
    let open = i - 1;
    let mut k = open;
    while k > 0 {
        let c = bytes[k - 1] as char;
        if c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '.' {
            k -= 1;
        } else {
            break;
        }
    }
    let method = code[k..open].trim();
    if !matches!(
        method,
        "$" | "document.querySelector"
            | "document.querySelectorAll"
            | "querySelector"
            | "querySelectorAll"
    ) {
        return None;
    }
    let head = code[..k].trim_end().strip_suffix('=')?.trim_end();
    let ident: String = head
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '$'))
        .collect::<Vec<char>>()
        .into_iter()
        .rev()
        .collect();
    if ident.is_empty() {
        None
    } else {
        Some(ident)
    }
}

/// 这一处句柄的右边是不是 `…).forEach((X) =>`（多元素查询的回调参数）。
fn foreach_param_after(code: &str, at: usize) -> Option<String> {
    let tail = &code[at..];
    let b = tail.as_bytes();
    let mut k = 0usize;
    while k < b.len() && matches!(b[k] as char, ']' | '"' | '\'' | ' ' | '\t') {
        k += 1;
    }
    if k >= b.len() || b[k] as char != ')' {
        return None;
    }
    k += 1;
    let rest = &tail[k..];
    let p = rest.find(".forEach(")?;
    if !rest[..p].trim().is_empty() {
        return None;
    }
    let after = rest[p + ".forEach(".len()..]
        .trim_start()
        .strip_prefix('(')?;
    let ident: String = after
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '$'))
        .collect();
    if ident.is_empty() {
        None
    } else {
        Some(ident)
    }
}

/// 句柄所在行绑定了监听器（非值控件 —— 按钮 —— 的接线形式）。
fn binds_listener(code: &str, selector: &str) -> bool {
    code.lines()
        .any(|l| l.contains(selector) && l.contains(".addEventListener("))
}

/// 这个控件是否被 `app.js` **消费**（坑 #338：被填 ≠ 被消费）。
///
/// - 有值控件：值必须流进产品代码（见 `value_reaches_product`）——监听器本身不算
///   （一个只把值写进 localStorage 的监听器就是本轴要挡的形状）；
/// - 按钮：绑上监听器即算接线（它没有值可读）。
fn consumed_in(code: &str, c: &HtmlControl) -> bool {
    for h in control_handles(c) {
        if !c.value_bearing && binds_listener(code, &h) {
            return true;
        }
        for v in bound_vars(code, &h) {
            if !c.value_bearing && binds_listener(code, &format!("{v}.addEventListener(")) {
                return true;
            }
            if value_reaches_product(code, &v) {
                return true;
            }
        }
    }
    false
}

/// 卡片里的本地化说明键：`<span class="hint" … data-i18n="KEY">`。
fn hint_keys(region: &str) -> Vec<String> {
    let src = strip_html_comments(region);
    let mut out: Vec<String> = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = src[from..].find(HINT_MARKUP) {
        let at = from + rel;
        let end = src[at..].find('>').map(|i| at + i).unwrap_or(src.len());
        if let Some(k) = html_attr_value(&src[at..end], "data-i18n") {
            out.push(k);
        }
        from = end + 1;
    }
    out.sort();
    out.dedup();
    out
}

/// 语言包区段（与 `i18n_pack.rs` 同标记：终点取对象字面量自身的收尾）。
fn pack_region<'a>(src: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let at = src.find(start)?;
    let rest = &src[at..];
    let stop = rest.find(end)?;
    Some(&rest[..stop])
}

/// `const NAME = { … }` 对象字面量体的字节区间（从 `{` 到配对的 `}` **之后**）。
///
/// 字符串里的花括号不参与配对 —— 语言包与过渡表的值里都有 `{`。传入的应是**已剥注释**的代码
/// 文本（[`code_text_by_line`] 的拼接结果）：说明性注释里也会出现表名与键名（坑 #296 的镜像，
/// 本轮的修法就在表旁边写了一整段解释）。
fn object_literal_span(src: &str, name: &str) -> Option<(usize, usize)> {
    let needle = format!("const {name} = {{");
    let open = src.find(&needle)? + needle.len() - 1;
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    let mut i = open;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => quote = Some(c),
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((open, i + 1));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// 对象字面量里**顶层**的键（`状态:` 形式）。嵌套对象里的键、字符串里的冒号都不算。
fn object_literal_keys(src: &str, name: &str) -> Option<Vec<String>> {
    let (open, close) = object_literal_span(src, name)?;
    let body = &src[open + 1..close - 1];
    let bytes = body.as_bytes();
    let mut keys: Vec<String> = Vec::new();
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if c == b'"' || c == b'\'' || c == b'`' {
            quote = Some(c);
            i += 1;
            continue;
        }
        if c == b'{' || c == b'[' || c == b'(' {
            depth += 1;
        } else if c == b'}' || c == b']' || c == b')' {
            depth = depth.saturating_sub(1);
        } else if depth == 0 && is_ident_byte(c) && (i == 0 || !is_ident_byte(bytes[i - 1])) {
            let start = i;
            let mut end = i;
            while end < bytes.len() && is_ident_byte(bytes[end]) {
                end += 1;
            }
            let mut j = end;
            while j < bytes.len() && bytes[j] == b' ' {
                j += 1;
            }
            // `ident:` 且不是 `::`
            if j < bytes.len() && bytes[j] == b':' && bytes.get(j + 1) != Some(&b':') {
                keys.push(body[start..end].to_string());
            }
            i = end;
            continue;
        }
        i += 1;
    }
    Some(keys)
}

/// 对象字面量里**顶层**的嵌套条目：`状态键 → 该条目自身 `{ … }` 内的文本`（布局无关）。
fn nested_entries(src: &str, name: &str) -> Vec<(String, String)> {
    let Some((open, close)) = object_literal_span(src, name) else {
        return Vec::new();
    };
    let body = &src[open + 1..close - 1];
    let bytes = body.as_bytes();
    let mut out: Vec<(String, String)> = Vec::new();
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    let mut opened: Option<(String, usize)> = None;
    let mut from = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => quote = Some(c),
            b'{' => {
                depth += 1;
                if depth == 1 {
                    // 键 = `from..i` 里**最后一个标识符**（键后面跟的是 `:` 与空格，直接用
                    // `rsplit` 取最后一段会取到空串 —— 那个空键会让 `next` 自环检查恒真）
                    let head = &body[from..i];
                    let hb = head.as_bytes();
                    let mut e = hb.len();
                    while e > 0 && !is_ident_byte(hb[e - 1]) {
                        e -= 1;
                    }
                    let mut s = e;
                    while s > 0 && is_ident_byte(hb[s - 1]) {
                        s -= 1;
                    }
                    opened = Some((head[s..e].to_string(), i + 1));
                }
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if let Some((key, inner)) = opened.take() {
                        out.push((key, body[inner..i].to_string()));
                    }
                    from = i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// 一段代码里 `field: "…"` 的字符串值。字段名必须是**标识符**（`xlabel:` 里的 `label` 不算）。
fn string_field(text: &str, field: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = text[from..].find(field) {
        let at = from + rel;
        let after = at + field.len();
        let before_ok = at == 0 || !is_ident_byte(bytes[at - 1]);
        if before_ok {
            let tail = text[after..].trim_start();
            if let Some(value) = tail.strip_prefix(':') {
                let value = value.trim_start();
                if let Some(rest) = value.strip_prefix('"') {
                    if let Some(close) = rest.find('"') {
                        return Some(rest[..close].to_string());
                    }
                }
            }
        }
        from = after;
    }
    None
}

/// 一行里**被调用**的标识符（`name(` / `a.b(` 取最后一段）。
///
/// **不**过滤 `if(` / `catch(` 这类关键字 —— 判别式只在**两行之间取交集**时使用，关键字出现在
/// 另一行是极不可能的；过滤名单反而是需要维护的花名册。
fn called_identifiers(line: &str) -> BTreeSet<String> {
    let bytes = line.as_bytes();
    let mut out = BTreeSet::new();
    for i in 0..bytes.len() {
        if bytes[i] != b'(' {
            continue;
        }
        let mut j = i;
        while j > 0 && bytes[j - 1] == b' ' {
            j -= 1;
        }
        let mut k = j;
        while k > 0 && is_ident_byte(bytes[k - 1]) {
            k -= 1;
        }
        if k < j {
            out.insert(line[k..j].to_string());
        }
    }
    out
}

/// 两行**共用**的访问器（排除语言层原语）。
///
/// 「按钮渲染那一行」与「切换处理器」共用的、非 `T` 的调用，就是让两侧从**同一张表的同一条目**
/// 取值的那个访问器。缺陷形状（两侧各自解释状态）在这条判别式下交集为空。
fn shared_accessor(button_line: &str, handler: &str) -> Vec<String> {
    let in_handler = called_identifiers(handler);
    let mut out: Vec<String> = called_identifiers(button_line)
        .into_iter()
        .filter(|n| n != I18N_PRIMITIVE && in_handler.contains(n))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// 从含 `needle` 的行起、到含 `end` 的行为止（含两端）的代码文本。
///
/// 一段标记会被写成多行（按钮的 `data-share-toggle=` 与它的动作标签就不在同一行），
/// 只看一行会把「动作」那一半漏掉。
fn code_block_containing(src: &str, needle: &str, end: &str) -> Option<String> {
    let mut started = false;
    let mut out: Vec<&str> = Vec::new();
    for line in src.lines() {
        if !started {
            if line.contains(needle) {
                started = true;
            } else {
                continue;
            }
        }
        out.push(line);
        if line.contains(end) {
            return Some(out.join("\n"));
        }
    }
    if started {
        Some(out.join("\n"))
    } else {
        None
    }
}

/// 把状态值交给 `/api/sharings/` 的端点里，**由状态推出**（而非写成字面量）的那些函数名。
///
/// 直接端点（删除 = `{ status: "off" }`）不算：本轴的处理器是「状态 → 下一状态」那一个，
/// 它由构造就必须是从状态推出来的 —— 所以这里不需要写函数名（推导，不是花名册）。
fn dynamic_status_owners(src: &str) -> Vec<String> {
    code_lines_owned_by(src, |l| {
        l.contains("/api/sharings/") && l.contains("status:")
    })
    .into_iter()
    .filter(|(_, _, line)| !line.contains("status: \"") && !line.contains("status: '"))
    .map(|(owner, _, _)| owner)
    .collect()
}

/// 某个字节位置所在的行（去掉首尾空白）。
fn line_at(src: &str, at: usize) -> String {
    let start = src[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = src[at..].find('\n').map(|i| at + i).unwrap_or(src.len());
    src[start..end].trim().to_string()
}

// ══ PART A ══（模块级；插在 `#[cfg(test)]` 之前）════════════════════════════════════════════
/// C2167 的语料：**i18n 门禁同一语料**（规则②「唯一实现」要扫全站前端源码）。
const DATA_JS: &str = include_str!("../ui/js/data.js");

/// 见 `DATA_JS`。
const API_JS: &str = include_str!("../ui/js/api.js");

/// C2167：规则②的语料（文件名用于报错，源码用于计数）。
const CSV_CORPUS: [(&str, &str); 5] = [
    ("ui/js/app.js", APP_JS),
    ("ui/js/i18n.js", I18N_JS),
    ("ui/js/data.js", DATA_JS),
    ("ui/js/api.js", API_JS),
    ("ui/index.html", INDEX_HTML),
];

/// 从**一行**源码里取出 `/[<class>]/` 的**字面内容**与**尾部**。
///
/// `/[",\n]/.test(s) ? …` ⇒ 字符类 `",\n`、尾部空。
/// `/[",\r\n]|/.test(s) ? …` ⇒ 同样的字符类、尾部 `|`（该正则**匹配空串** ⇒ `.test` 恒真）。
///
/// 尾部是这条门禁的关键：`/[",\r\n]|/` 在**词法上**四元素齐全，规则①抓不到它 ——
/// 只有规则③（把它当正则语义求值、对 `abc` 断言**不**命中）才有牙。
fn csv_class_in_line(line: &str) -> Option<(String, String)> {
    let at = line.find("/[")?;
    let rest = &line[at + 2..];
    let end = rest.find(']')?;
    let class = &rest[..end];
    let after = &rest[end + 1..];
    // 必须是正则字面量的收尾：`]` 之后到下一个 `/` 之间的东西就是「尾部」（正常为空）。
    let slash = after.find('/')?;
    Some((class.to_string(), after[..slash].to_string()))
}

/// 在 `exportTxCsv` 的整段源码里取那个 CSV 单元格转义器：
/// 返回 `(该行源码, 字符类, 尾部)`。
///
/// 判别式：**唯一**一处「`const <ident> = (…) =>` 且同行有 `/[<class>]/`」的行。
/// 返回 `Err` 的两种含义必须**可区分**（坑 #291；自证测试对两者各判一次）：
/// - 「取不到」 = 写法里根本没有 `.test(` 的字符类（例如退化成「永远加引号」）；
/// - 「取到多处」 = 该函数里出现了第二处字符类。
///
/// **取到了但元素不全**由调用方（规则①）判 —— 那是第三种错。
fn csv_escaper_class(body: &str) -> Result<(String, String, String), String> {
    let mut hits: Vec<(String, String, String)> = Vec::new();
    for line in body.lines() {
        if is_comment_line(line) {
            continue;
        }
        if !line.contains("const ") || !line.contains("=>") {
            continue;
        }
        if let Some((class, tail)) = csv_class_in_line(line) {
            hits.push((line.trim().to_string(), class, tail));
        }
    }
    match hits.len() {
        0 => Err(
            "取不到 `.test(` 的字符类：`const <ident> = (…) => … /[…]/.test(…) ? … : …` 这个形状不存在"
                .to_string(),
        ),
        1 => Ok(hits.remove(0)),
        n => Err(format!(
            "`exportTxCsv` 里出现了 {n} 处字符类（单元格转义器只应有一处）：{hits:?}"
        )),
    }
}

/// 字符类的**元素分词器**：单个字符算一个元素，`\`+字符（转义）算一个元素。
///
/// ⛔ **不得做子串匹配**：`\r\n` 里含 `\n`，`contains("\\n")` 恒真 —— 那正是「漏了 CR 也判绿」
/// 的形状（坑 #333 家族）。必须按**元素 token** 比。
fn csv_class_elements(class: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = class.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some(n) => out.push(format!("\\{n}")),
                None => out.push("\\".to_string()),
            }
        } else {
            out.push(c.to_string());
        }
    }
    out
}

/// 某个元素是否匹配某个字符（本轴只用得到这几种；其余按字面单字符处理）。
fn csv_element_matches(elem: &str, c: char) -> bool {
    match elem {
        "\\r" => c == '\r',
        "\\n" => c == '\n',
        "\\t" => c == '\t',
        _ => elem.starts_with(c),
    }
}

/// 那处 `.test(` 的结果是否**驱动分支**：`?` 必须出现在 `.test(` **之后**。
///
/// ⛔ 不能只看「这一行里有 `?`」—— 同一行里 `String(v == null ? "" : v)` 就有一个 `?`，
/// 于是「留着字符类、把 `.test()` 的结果丢掉」这种半修会被判绿（本轮 A/B 首跑当场抓到）。
fn csv_test_drives_the_condition(line: &str) -> bool {
    match line.find(".test(") {
        None => false,
        Some(at) => line[at..].contains('?'),
    }
}

/// 把 `/[<class>]<tail>/` **当正则语义**求值：`s` 会不会被它命中。
///
/// 尾部非空（如 `|` ⇒ 交替了一个空分支）意味着该正则**匹配空串** ⇒ `.test` 恒真 ⇒ 恒加引号。
/// 这是「四元素齐全却仍然退化」的**唯一**判别式 —— 词法层面看不出来（规则①在这里必须绿）。
fn csv_class_matches(elems: &[String], tail: &str, s: &str) -> bool {
    if !tail.is_empty() {
        return true;
    }
    s.chars()
        .any(|c| elems.iter().any(|e| csv_element_matches(e, c)))
}

// ── C2173：运营成员表的「余额」列说的是哪一半 ──────────────────────────────────────

/// 运营成员表余额列的**列头键**（单元格的 `data-label` 与列头用的是同一个键）。
const OPS_BALANCE_CAPTION: &str = "ops.users.col.balance";

/// 管理页「永久点数」列的键 —— 规则 3 的**反向对照**：那一列的名字说它只有永久额。
const ADMIN_PERM_CAPTION: &str = "admin.emp.col.perm";

/// 全站给两个半区起名的那两个键 —— 规则 2 的**尺子**（可花额 / 永久额）。
///
/// 不写死词表：标记从语言包自己的这两个值里推出来（见 [`half_markers`]），两种语言同一条规则。
const SPENDABLE_WORD_KEY: &str = "wallet.balance";
const PERMANENT_WORD_KEY: &str = "wallet.forever";

/// 一个用户的配额被切成的那两项（规则 1 必须**同时**读到）。
const HALF_FIELDS: [&str; 2] = ["balance", "gift_balance"];

/// `'<td … data-label="' + T("<caption>") + '">' + <取值> + "</td>"` 里那截**取值表达式**（C2173）。
///
/// 判别式按 `data-label` 的**收尾标记** `'">'`（引号里就是闭合那个 `td` 的 `>`）与 `"</td>"` 收口，
/// 不写死空格：只要还是「一个 td 的取值段」，重排空白也取得到。取不到 ⇒ `None`，
/// 调用方**必须**报错（空集上的断言会假绿，坑 68）。
fn cell_value_rhs(line: &str, caption_key: &str) -> Option<String> {
    let needle = format!("\"{caption_key}\"");
    let at = line.find(&needle)? + needle.len();
    let rest = &line[at..];
    let close = rest.find("'\">'")? + "'\">'".len();
    let value = rest[close..].trim_start().strip_prefix('+')?.trim_start();
    let end = value.find("\"</td>\"")?;
    let head = value[..end].trim_end();
    let head = head.strip_suffix('+').unwrap_or(head).trim_end();
    if head.is_empty() {
        None
    } else {
        Some(head.to_string())
    }
}

/// `a` / `b` 的**最长公共子串**（按字符，长度相同取先遇到的那条）。
///
/// 尺子用：全站给两个半区起的名字共享的那截「名词」（en: `points`；zh: `点数`）。
fn longest_common_substring(a: &str, b: &str) -> String {
    let ac: Vec<char> = a.chars().collect();
    let mut best = String::new();
    for i in 0..ac.len() {
        for j in (i + 1)..=ac.len() {
            let cand: String = ac[i..j].iter().collect();
            if cand.chars().count() > best.chars().count() && b.contains(cand.as_str()) {
                best = cand;
            }
        }
    }
    best
}

/// 从全站给两个半区起的**名字**里取出各自的**标记**：去掉共同名词后剩下的那截。
///
/// `("点数余额", "永久点数") -> ("余额", "永久")`；`("Points balance", "Permanent points") ->
/// ("balance", "permanent")`。两边都去空白与标点。
///
/// 取不到（没有共同名词、或某一边去掉共同名词后为空）⇒ `None`：调用方必须报错，
/// **不得**退化成空标记（空标记会让 `contains` 恒真 ⇒ 规则 2 静默变哑）。
fn half_markers(spendable: &str, permanent: &str) -> Option<(String, String)> {
    let a = spendable.to_lowercase();
    let b = permanent.to_lowercase();
    let common = longest_common_substring(&a, &b);
    if common.trim().is_empty() {
        return None;
    }
    let strip = |s: &str| {
        s.split(&common)
            .collect::<Vec<_>>()
            .join(" ")
            .trim_matches(|c: char| c.is_whitespace() || "（）()·,、:：".contains(c))
            .trim()
            .to_string()
    };
    let (x, y) = (strip(&a), strip(&b));
    if x.is_empty() || y.is_empty() {
        return None;
    }
    Some((x, y))
}

/// 这一条文案是否落在**可花族**：含可花标记，且**不含**永久标记（C2173 规则 2）。
fn caption_names_spendable_half(
    caption: &str,
    spendable_marker: &str,
    permanent_marker: &str,
) -> bool {
    let c = caption.to_lowercase();
    c.contains(spendable_marker) && !c.contains(permanent_marker)
}

// ============================== PART A: module-level helpers ================================

// ── R139：表单控件的 property 式 `disabled` 必须在回收路径上被清除 ──────────────────────────────
//
// 共享上架表单的「每天」快捷勾选，用 **property** 方式给七个单日 chip 打上 `disabled`
// （`cb.disabled = allCb.checked`），而成功上架后表单走 `e.target.reset()` 回收 ——
// **`HTMLFormElement.reset()` 只还原「值 / 勾选态」到默认值，不清 `disabled` property**。
// 于是勾着「每天」成功上架一次之后，七枚 chip 是「未勾选 + 禁用」：点任何一天都没反应，
// 且**整个会话不自愈**（表单卡片是静态 HTML，不参与重渲染；`showShareForm` / `renderSharing`
// / `fillPlans` 都不碰它）。唯一恢复路径是把「每天」再勾上又取消一次。
//
// 本门禁钉的是**形状**：凡「按 CSS 标签 `input` 取到的表单控件被 property 式禁成非 `false`」
// 的写点，都必须能在**表单回收路径**上找到**同一频道**的 `.disabled = false`。
//
// ⚠️ 浏览器事实（`reset()` 不清 property）与「点一个星期没反应」只有 DOM 仪器能证
// （`r139_probe.js`：未修树恰 `{B1,B2}` 红 / 修复树 9/9 绿 / 竞争修法 `m_drop` 被 `{C3,C2,A3}` 拒绝）。
// 本模块只钉**代码形状** —— 与 C2148 同款分工：**形状归门禁、事实归探针**。
//
// 已知边界（如实的射程，不是承诺）：
// - 频道只认**双引号**选择器字面量（`$("…")` / `$$("…")` / `querySelector(All)("…")`）。仓内的
//   `$` / `$$` 工具一律双引号，而十几处**按钮**瞬时禁用走 `querySelector('button…')`（单引号）
//   或事件目标 ⇒ 天然不在射程内（不是靠名册排除的）。
// - 写点的频道按**向上 80 行内最近的一个**含 `input` 标签的选择器字面量归属；今日全仓只有一个
//   这样的频道（`#sf-days .chip input`），`r139_verify_anchors.py` 的 `D4` 腿把它钉成 1。

/// 从一行里取出 `const NAME = (` / `let NAME = (` / `var NAME = (` 的 `NAME`（只认行首声明）。
fn arrow_name(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let rest = t
        .strip_prefix("const ")
        .or_else(|| t.strip_prefix("let "))
        .or_else(|| t.strip_prefix("var "))?;
    let (name, _) = rest.split_once(" = (")?;
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    {
        return None;
    }
    Some(name)
}

/// 声明（`function NAME(` 或 `const NAME = (`）的**字节区间**。
///
/// 既有的 [`js_function_body`] 靠「首个恰为 `  }` 的行」收尾 ⇒ 只适用于**两空格缩进的多行函数**，
/// 既看不见本轴的三个符号（`afterOk` / `showShareForm` / `resetShareAvail` 都是深缩进的箭头常量），
/// 也会被**单行函数**吞掉一整段（坑 #319 / #332：`const hideShareForm = () => { … };` 会让
/// 「首个恰为 `};` 的行」落到几百行之外）。这里改用**花括号配平**收尾；声明行上没有 `{`
/// （`const shareFormCard = () => $("#share-form-card");`）时退回该行本身。
fn decl_spans(src: &str) -> Vec<(String, usize, usize)> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    for line in src.split_inclusive('\n') {
        if let Some(name) = function_name(line).or_else(|| arrow_name(line)) {
            out.push((name.to_string(), offset, decl_end(src, offset)));
        }
        offset += line.len();
    }
    out
}

/// 声明区间的收尾（花括号配平；声明行上没有 `{` ⇒ 该行末尾）。
fn decl_end(src: &str, start: usize) -> usize {
    let line_end = src[start..]
        .find('\n')
        .map(|i| start + i)
        .unwrap_or(src.len());
    let Some(open) = src[start..line_end].find('{').map(|i| start + i) else {
        return line_end;
    };
    let mut depth = 0i32;
    for (i, c) in src[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return open + i + 1;
                }
            }
            _ => {}
        }
    }
    src.len()
}

/// 包含 `pos` 的**最内层**声明名。
///
/// 不能用「命中行之前最近声明的那个函数」：`showShareForm` / `afterOk` 都声明在 `bindEvents`
/// 体内，那个启发式会把它们的语句全归给 `bindEvents`，而 `bindEvents` 的调用闭包是**整个文件**
/// —— 于是「闭包里有没有一处清 `disabled`」对任何树都恒真（坑 #336 的归属启发式正是这样失效的）。
fn span_owner(spans: &[(String, usize, usize)], pos: usize) -> Option<String> {
    spans
        .iter()
        .filter(|(_, s, e)| *s <= pos && pos < *e)
        .min_by_key(|(_, s, e)| e - s)
        .map(|(n, _, _)| n.clone())
}

/// 某个声明的**区间文本**。
fn span_body<'a>(src: &'a str, spans: &[(String, usize, usize)], name: &str) -> Option<&'a str> {
    spans
        .iter()
        .find(|(n, _, _)| n == name)
        .map(|(_, s, e)| &src[*s..*e])
}

/// 逐行的 `(行首字节偏移, 注释剥离后的代码文本)`。
///
/// 归属判定要**原始偏移**：`code_text_by_line` 剥掉注释后偏移就变了，拿它去查区间会把归属查错
/// （本轮 R139 第一版就是这样把 `e.target.reset()` 归给了 `sendChat`，`reset_fn` 直接判错）。
fn code_lines_at(src: &str) -> Vec<(usize, String)> {
    let text = code_text_by_line(src);
    let mut out = Vec::new();
    let mut offset = 0usize;
    for (i, line) in src.split('\n').enumerate() {
        let body = text.get(i).cloned().unwrap_or_default();
        out.push((offset, strip_trailing_comment(&body).trim().to_string()));
        offset += line.len() + 1;
    }
    out
}

/// 一行里的 `$$("SEL")` / `$("SEL")` / `querySelector(All)("SEL")` 选择器**双引号**字面量。
fn selector_literals(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for pat in ["$$(\"", "$(\"", "querySelectorAll(\"", "querySelector(\""] {
        let mut from = 0usize;
        while let Some(rel) = line[from..].find(pat) {
            let s = from + rel + pat.len();
            let Some(end) = line[s..].find('"') else {
                break;
            };
            out.push(line[s..s + end].to_string());
            from = s + end + 1;
        }
    }
    out
}

/// CSS 标识符字节（`-` 也算 —— `#chat-input` 是一个整体）。
fn is_css_ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b'$'
}

/// `sel` 是否按 **CSS 标签** `input` 取控件。
///
/// 不能写成 `sel.contains("input")`：`#chat-input`（对话输入框）也含 `input`，而 `-` 是 CSS
/// 标识符字符 ⇒ 子串匹配会把兄弟标识符当证据（坑 #333 同族）。本轮实测：首版就是这样把
/// `#chat-input` 判成了表单控件频道。
fn selects_input_tag(sel: &str) -> bool {
    let bytes = sel.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = sel[from..].find("input") {
        let a = from + rel;
        let z = a + "input".len();
        let before_ok = a == 0 || !is_css_ident_byte(bytes[a - 1]);
        let after_ok = z == bytes.len() || !is_css_ident_byte(bytes[z]);
        if before_ok && after_ok {
            return true;
        }
        from = a + 1;
    }
    false
}

/// `#foo .bar` 里的 `foo`（选择器里第一个 id）。
fn css_id_of(selector: &str) -> Option<String> {
    let rest = selector.strip_prefix('#')?;
    let id: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if id.is_empty() {
        None
    } else {
        Some(id)
    }
}

/// `index.html` 里落在 `<form …>…</form>` 内的元素 id（本轴的控件因此确实是**表单控件**）。
fn form_control_ids(html: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut inside = false;
    for line in html.lines() {
        if !inside && line.contains("<form") {
            inside = true;
        }
        if inside {
            let mut from = 0usize;
            while let Some(rel) = line[from..].find("id=\"") {
                let s = from + rel + 4;
                let Some(e) = line[s..].find('"') else { break };
                out.insert(line[s..s + e].to_string());
                from = s + e + 1;
            }
            if line.contains("</form>") {
                inside = false;
            }
        }
    }
    out
}

/// 含有控件 `control` 的那个 `<form …>` 的 id（从 `<form>` 标签自身取，不写名册）。
fn form_of_control(html: &str, control: &str) -> Option<String> {
    let at = html.find(&format!("id=\"{control}\""))?;
    let form_at = html[..at].rfind("<form")?;
    let tag_end = html[form_at..].find('>').map(|i| form_at + i)?;
    let tag = &html[form_at..tag_end];
    let s = tag.find("id=\"")? + 4;
    let e = tag[s..].find('"')? + s;
    Some(tag[s..e].to_string())
}

/// 承载该 `<form …>` 的那张卡片 —— 表单标签**之前**最近的一个 `id="…"`。
fn form_card_id(html: &str, form_id: &str) -> Option<String> {
    let at = html.find(&format!("<form id=\"{form_id}\""))?;
    let prefix = &html[..at];
    let s = prefix.rfind("id=\"")? + 4;
    let e = prefix[s..].find('"')? + s;
    Some(prefix[s..e].to_string())
}

/// R139 的读数（每条规则各取自己那一份，测试里逐条断言）。
#[derive(Debug)]
struct R139Reading {
    /// 规则 1：被 property 式禁用的**表单控件**频道（派生）。
    channels: BTreeSet<String>,
    /// 频道所属的那个 `<form>` 的 id（派生自 `index.html`）。
    lead_form: Option<String>,
    /// 承载该表单的卡片 id（派生自 `index.html`）。
    card: Option<String>,
    /// 回收路径的宿主：调用 `form.reset()` 的那个函数。
    reset_fn: Option<String>,
    /// 把表单卡片打开的函数的集合（派生：自己那层写 `.hidden = false` 且提到卡片 id）。
    open_fns: Vec<String>,
    /// 回收闭包 = `reset_fn` 的闭包 ∪ 每个 `open_fns` 的闭包。
    closure: BTreeSet<String>,
    /// 规则 2 的见证：闭包里那个**同频道**清 `disabled` 的函数。
    witness: Option<String>,
    /// 规则 3：调用 `reset_fn` 的函数集合（除它自己）。
    callers: BTreeSet<String>,
}

fn r139_reading(app: &str, html: &str) -> R139Reading {
    let lines = code_lines_at(app);
    let spans = decl_spans(app);

    // ── 规则 1：property 式禁用的表单控件频道 ────────────────────────────────────────────────
    let mut channels: BTreeSet<String> = BTreeSet::new();
    for (i, (_off, text)) in lines.iter().enumerate() {
        let Some((_, rhs)) = text.split_once(".disabled = ") else {
            continue;
        };
        let value = rhs.split([';', '}', ')']).next().unwrap_or("").trim();
        if value == "false" {
            continue; // 清除，不是禁用
        }
        let mut channel = None;
        for j in (0..=i).rev().take(80) {
            let Some((_, prev)) = lines.get(j) else {
                continue;
            };
            if let Some(s) = selector_literals(prev)
                .into_iter()
                .find(|s| selects_input_tag(s))
            {
                channel = Some(s);
                break;
            }
        }
        if let Some(c) = channel {
            channels.insert(c);
        }
    }

    // ── 派生：频道 → 表单 id → 卡片 id ───────────────────────────────────────────────────────
    let controls = form_control_ids(html);
    let mut lead_form = None;
    for c in &channels {
        let Some(id) = css_id_of(c) else { continue };
        if controls.contains(&id) {
            lead_form = form_of_control(html, &id);
            break;
        }
    }
    let card = lead_form.as_deref().and_then(|f| form_card_id(html, f));

    // ── 规则 3 的宿主：调用 `form.reset()` 的函数 ────────────────────────────────────────────
    let reset_fn = lines
        .iter()
        .find(|(_off, text)| text.contains(".reset()"))
        .and_then(|(off, _)| span_owner(&spans, *off));

    // ── 回收路径的另一半：把表单卡片打开的函数 ──────────────────────────────────────────────
    let mut open_fns: Vec<String> = Vec::new();
    if let Some(card) = &card {
        let needle = format!("#{card}");
        for (name, s, e) in &spans {
            if e <= s {
                continue;
            }
            let body = &app[*s..*e];
            let owners: BTreeSet<String> = body
                .match_indices(".hidden = false")
                .filter_map(|(i, _)| span_owner(&spans, s + i))
                .collect();
            if !owners.contains(name) {
                continue; // `.hidden = false` 全部落在嵌套函数里 ⇒ 这一层不是「打开表单的人」
            }
            let direct = body.contains(&needle);
            let via_callee = callee_names(body).iter().any(|c| {
                span_body(app, &spans, c)
                    .map(|b| b.contains(&needle))
                    .unwrap_or(false)
            });
            if direct || via_callee {
                open_fns.push(name.clone());
            }
        }
    }

    // ── 回收闭包 ────────────────────────────────────────────────────────────────────────────
    let mut closure: BTreeSet<String> = BTreeSet::new();
    let mut queue: Vec<String> = reset_fn
        .iter()
        .cloned()
        .chain(open_fns.iter().cloned())
        .collect();
    while let Some(f) = queue.pop() {
        if !closure.insert(f.clone()) {
            continue;
        }
        let Some(body) = span_body(app, &spans, &f) else {
            continue;
        };
        for c in callee_names(body) {
            if !closure.contains(&c) && span_body(app, &spans, &c).is_some() {
                queue.push(c);
            }
        }
    }

    // ── 规则 2 的见证：闭包里**同频道**的 `.disabled = false` ──────────────────────────────
    let witness = closure
        .iter()
        .find(|f| {
            span_body(app, &spans, f)
                .map(|b| {
                    let text = code_text(b);
                    text.contains(".disabled = false") && channels.iter().any(|c| text.contains(c))
                })
                .unwrap_or(false)
        })
        .cloned();

    // ── 规则 3：`reset_fn` 仍被别人调用 ─────────────────────────────────────────────────────
    let callers: BTreeSet<String> = match &reset_fn {
        None => BTreeSet::new(),
        Some(rf) => lines
            .iter()
            .filter(|(_off, text)| mentions_identifier(text, rf))
            .filter_map(|(off, _)| span_owner(&spans, *off))
            .collect(),
    };

    R139Reading {
        channels,
        lead_form,
        card,
        reset_fn,
        open_fns,
        closure,
        witness,
        callers,
    }
}

// ── R158：同一视图里的同一份 token 数量只许有一种拼写；导出的数字列写**精确值** ──────────────
//
// 汇总卡的 Token 合计/输入/缓存/输出四项跑在 `renderTxSummary` 自带的 `fmtM` 上
// （K 档 `Math.round(n / 1000) + "K"`：1500 → "2K"），而它**正下方那些行**的同一个数由
// `fmtTokens` 渲染（K 档 `(n / 1000).toFixed(1)` 去尾零：1500 → "1.5K"）—— 筛到只剩一行时
// 两处印的是同一个数、两种拼写。CSV 导出再把单元格的**显示串**写进数据文件，而精确值的唯一
// 出口（悬停）在文件里根本不存在（`fmtTokensExact` 只喂 `title`）。
//
// 本门禁钉的是**形状**：卡片不许自带拼写、必须委派；导出必须写数字；单元格必须仍是缩写。
// 「屏幕与文件真的对不对得上」由 DOM 仪器 `r158_probe.js` 证（形状归门禁，事实归探针）。

/// 交易表四个 token 列的**显示字段**（= 视图模型 `txsToView` 返回对象里的键名）。
///
/// 这不是凭空的名单：`the_r158_roster_is_real` 断言每个名字都在那个对象字面量里被声明、
/// 且与它配对的数字字段也在。字段改名时门禁会**响亮地**失败 —— 而不是静默失去射程。
const R158_TOKEN_FIELDS: [&str; 4] = ["inputTokens", "cachedTokens", "outputTokens", "tokens"];

/// 「自带一份紧凑拼写」的指纹：`"K"` / `"M"` 字符串字面量。
const R158_SPELLING_LITERALS: [&str; 2] = ["\"K\"", "\"M\""];

/// 变体的两条锚点（缺陷形态与修好形态），`r158_fix` / `r158_unfix` 是互逆的改写。
///
/// A/B 用例从**修好之后的树**派生，而不是直接拿 `APP_JS` 当「缺陷」那一行：写死
/// `("base", APP_JS, …)` 的话，修复一落地这条测试就**静默反转**（坑 #314）—— 它值得
/// 恒真的地方在于「规则能认出缺陷」，不在于「此刻这棵树有缺陷」。
const FMTM_DEFECTIVE: &str = "const fmtM = (n) => (n >= 1e6 ? (n / 1e6).toFixed(2) + \"M\" : (n >= 1000 ? Math.round(n / 1000) + \"K\" : String(Math.round(n))));";
const FMTM_DELEGATING: &str = "const fmtM = fmtTokens;";
const CSV_DISPLAY_GROUP: &str = "t.inputTokens, t.cachedTokens, t.outputTokens, t.tokens,";
const CSV_RAW_GROUP: &str = "fmtTokensExact(t.inputRaw), fmtTokensExact(t.cachedRaw), fmtTokensExact(t.outputRaw), fmtTokensExact(t.tokensRaw),";
/// 「只修了合计列」的半修形状（竞争修法 m1）。
const CSV_PARTIAL_GROUP: &str =
    "t.inputTokens, t.cachedTokens, t.outputTokens, fmtTokensExact(t.tokensRaw),";

/// 把缺陷形态改写成修好形态（幂等：树已修好时逐字返回原样）。
fn r158_fix(src: &str) -> String {
    src.replace(FMTM_DEFECTIVE, FMTM_DELEGATING)
        .replace(CSV_DISPLAY_GROUP, CSV_RAW_GROUP)
}

/// `r158_fix` 的逆（同样幂等）—— 用来在**任何一棵树**上把缺陷形态构造出来。
fn r158_unfix(src: &str) -> String {
    src.replace(FMTM_DELEGATING, FMTM_DEFECTIVE)
        .replace(CSV_RAW_GROUP, CSV_DISPLAY_GROUP)
}

/// 显示字段 → 同一行的**精确值**字段（`inputTokens` → `inputRaw`、`tokens` → `tokensRaw`）。
///
/// 派生而不是并列第二份名单：`…Tokens` 去掉 `Tokens` 再加 `Raw`；本来就叫 `tokens` 的直接加
/// `Raw`。两份名单一旦各写一遍就会漂移，而漂移的后果正是本轴要消灭的东西。
fn r158_raw_field(display: &str) -> String {
    match display.strip_suffix("Tokens") {
        Some(stem) => format!("{stem}Raw"),
        None => format!("{display}Raw"),
    }
}

/// `const NAME = (…) => …` 的**函数体**，**任意缩进**（不像 `js_function_body` 只认
/// `function NAME(`）。
///
/// 收尾判据：块状箭头停在**去空白后恰为 `};`** 的那一行；单行箭头只取它自己那一行
/// （坑 #319：单行函数不许吞掉紧随其后的多行函数）。缩进不能写死 —— 本轴的
/// tooltip 构造器在 `txsToView` 里缩进 6 格，而只认 2/4 格的提取器会**静默返回空体**，
/// 空体看起来跟「这条规则没什么可抱怨的」一模一样。
fn r158_arrow_body(src: &str, name: &str) -> String {
    let head = format!("const {name} =");
    let mut buf: Vec<String> = Vec::new();
    let mut started = false;
    for line in src.lines() {
        if !started {
            if line.trim_start().starts_with(&head) {
                started = true;
                buf.push(line.to_string());
                if line.contains("=>") && !line.contains("=> {") {
                    break; // 单行箭头
                }
            }
            continue;
        }
        buf.push(line.to_string());
        if line.trim() == "};" {
            break;
        }
    }
    buf.join("\n")
}

/// `function NAME(…)` 的函数体：停在**恰好是 `  }`** 的那一行。
///
/// 不能写成 `trim() == "}"`：函数体里的 `if/else` 等嵌套块收尾是 `    }`，停在第一个这样的行上
/// 会**静默截断**，而截断不长得像错误 —— 它长得像「本该被抓住的那一行不存在」。
/// （实测：用宽松判据时 `renderTxSummary` 在它自己的 `if/else` 处提前结束约 1 000 字符，
/// 正好切掉规则 1 存在的理由 `const fmtM = …`，于是**改前树被判绿**。）
fn r158_fn_body(src: &str, name: &str) -> String {
    let head = format!("function {name}(");
    let mut buf: Vec<String> = Vec::new();
    let mut started = false;
    for line in src.lines() {
        if !started {
            if line.trim_start().starts_with(&head) {
                started = true;
                buf.push(line.to_string());
            }
            continue;
        }
        buf.push(line.to_string());
        if line == "  }" {
            break;
        }
    }
    buf.join("\n")
}

/// 一个声明（箭头 const 或 `function`）的**代码体**：注释已剥离、空行已去掉。
fn r158_code_of(src: &str, name: &str) -> String {
    let body = {
        let a = r158_arrow_body(src, name);
        if a.is_empty() {
            r158_fn_body(src, name)
        } else {
            a
        }
    };
    if body.is_empty() {
        return String::new();
    }
    let text = code_text_by_line(&body);
    text.into_iter()
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 汇总卡那一段（`renderTxSummary`）。
fn r158_card_code(src: &str) -> String {
    r158_code_of(src, "renderTxSummary")
}

/// CSV 的**那一行**：`exportTxCsv` 里逐行拼 11 列的那个 `list.map`。
fn r158_csv_row_line(src: &str) -> String {
    code_text_by_line(src)
        .into_iter()
        .find(|l| l.contains("const lines = list.map((t) => ["))
        .unwrap_or_default()
}

/// 四个 token 单元格的 `render:` 行（在 `TX_COLUMNS` 里，缩进 6 格、带 `t.tokenBrk(`）。
fn r158_token_cell_lines(src: &str) -> Vec<String> {
    code_text_by_line(src)
        .into_iter()
        .filter(|l| l.contains("render:") && l.contains("t.tokenBrk("))
        .collect()
}

/// 悬停那一层用的**精确值 helper** —— 从代码里**派生**，不写名字。
///
/// 语义：单元格的 tooltip 是「缩写」的精确值出口，导出必须写**同一层**的值。所以：取构造
/// `title="` 的那个箭头（`brkTitle`）里、赋值给 `exact` 的那一行，其上被调用的标识符就是它。
/// 派生而非硬编码，是为了不把「这一次编辑」做成快照（坑 #469）：换名/换层写法时门禁跟着变。
fn r158_exact_helpers(src: &str) -> Vec<String> {
    let body = r158_code_of(src, "brkTitle");
    let mut out: Vec<String> = Vec::new();
    for line in body.lines() {
        if !line.starts_with("const exact") || !line.contains('=') {
            continue;
        }
        let bytes = line.as_bytes();
        let is_word = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'$';
        let mut i = 0usize;
        while i < bytes.len() {
            if !(bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' || bytes[i] == b'$') {
                i += 1;
                continue;
            }
            let start = i;
            while i < bytes.len() && is_word(bytes[i]) {
                i += 1;
            }
            let name = &line[start..i];
            if bytes.get(i) == Some(&b'(') && !matches!(name, "typeof" | "String") {
                out.push(name.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// 四条规则的读数与它做判断所依据的证据（诊断要能自证，不能只说「红了」）。
struct R158Reading {
    r1: bool,
    r2: bool,
    r3: bool,
    r4: bool,
    card_len: usize,
    csv_len: usize,
    cells: usize,
    exact_helpers: Vec<String>,
    exact_calls: usize,
    csv_display_used: Vec<&'static str>,
    csv_raw_missing: Vec<String>,
}

fn r158_reading(src: &str) -> R158Reading {
    let card = r158_card_code(src);
    let csv = r158_csv_row_line(src);
    let cells = r158_token_cell_lines(src);

    // 规则 1：卡片**不自带拼写** —— 它的代码里不许出现 K/M 后缀字面量。
    let r1 = !card.is_empty() && !R158_SPELLING_LITERALS.iter().any(|lit| card.contains(lit));

    // 规则 2：卡片**委派**给单元格那条拼写。
    let r2 = mentions_identifier(&card, "fmtTokens");

    // 规则 3：导出的 token 四列写数字（四个 `<field>Raw`），且不读显示串；换算走悬停那一层的
    // helper。判据用标识符边界：`t.tokens` 不得在 `t.tokensRaw` 里命中（坑 #333）。
    let mut csv_display_used: Vec<&'static str> = Vec::new();
    let mut csv_raw_missing: Vec<String> = Vec::new();
    for f in R158_TOKEN_FIELDS {
        if mentions_identifier(&csv, &format!("t.{f}")) {
            csv_display_used.push(f);
        }
        let raw = r158_raw_field(f);
        if !mentions_identifier(&csv, &format!("t.{raw}")) {
            csv_raw_missing.push(raw);
        }
    }
    let exact_helpers = r158_exact_helpers(src);
    let exact_calls: usize = exact_helpers
        .iter()
        .map(|h| csv.matches(&format!("{h}(")).count())
        .sum();
    let r3 = !csv.is_empty()
        && csv_display_used.is_empty()
        && csv_raw_missing.is_empty()
        && exact_calls >= R158_TOKEN_FIELDS.len();

    // 规则 4（反向，防矫枉过正）：四个 token 单元格仍须印**缩写**字段。
    // 「把屏幕改成精确值、让文件显得对」正是这条要挡的过度纠正（m2）。
    let r4 = cells.len() == R158_TOKEN_FIELDS.len()
        && R158_TOKEN_FIELDS.iter().all(|f| {
            let needle = format!("+ t.{f} +");
            cells.iter().any(|l| l.contains(&needle))
        });

    R158Reading {
        r1,
        r2,
        r3,
        r4,
        card_len: card.len(),
        csv_len: csv.len(),
        cells: cells.len(),
        exact_helpers,
        exact_calls,
        csv_display_used,
        csv_raw_missing,
    }
}

impl R158Reading {
    fn verdicts(&self) -> String {
        format!(
            "R1={} R2={} R3={} R4={}",
            self.r1 as u8, self.r2 as u8, self.r3 as u8, self.r4 as u8
        )
    }
}

// ── R165：趋势图的聚合粒度必须由**实际发出的请求窗口**决定，不由控件值决定 ────────────────────
//
// `txTrendBucket()` 按 `#tx-range` 的**控件值**分支（24h/7d/30d/custom/else），而窗口真源是
// `txRangeParams()`（它同时拼出列表与趋势两份请求串）。两个产生**逐字相同的请求**的控件状态
// ——「全部时间」与「自定义 + 两个输入框都空」（后者 `txRangeParams()` 返回 `""`，与 all 同）——
// 在旧实现里得到**不同粒度**：`all` ⇒ week（按窗口：无界 ⇒ 最粗），`custom` ⇒ hour
// （按输入框：空 ⇒ 跨度 0 天）。于是图按 `MM-DD HH:00` 自称，而 `TX_TREND_MAX_COLS = 40`
// 又把 x 轴锚在右端 ⇒ 只画最近约 40 小时，永不与查询对账。
//
// 本门禁钉的是**形状**：粒度函数体内不许再出现控件选项值、必须委派给 `txRangeParams()`、
// 返回的粒度字面量恰为 `{hour, day, week}`、且窗口真源与控件**都还在**。
// 「屏幕上的轴与请求窗口真的对不对得上」由 jsdom 探针 `r165_probe.js` 证。

/// `#tx-range` 这个 `<select>` 的开标签特征 —— 选项集**从 `ui/index.html` 派生**，不写名册。
const R165_CONTROL_SELECT: &str = "id=\"tx-range\"";

/// 派生结果的**阳性对照**（#451）：`ui/index.html` 的选项集一漂移就响亮地失败，
/// 而不是让规则 1 静默失去射程（空集上的「不许出现」恒真 —— 坑 68 同族）。
const R165_OPTIONS: [&str; 5] = ["24h", "7d", "30d", "all", "custom"];

/// 粒度函数名与窗口真源名。
const R165_GRAIN_FN: &str = "txTrendBucket";
const R165_WINDOW_SOURCE: &str = "txRangeParams";

/// 允许被返回的粒度字面量（上层与下层都给界 —— #325）。
const R165_GRAINS: [&str; 3] = ["hour", "day", "week"];

/// 修复后的函数**体**（逐字摘自编辑表 `r165_verify_edits.py` 的 E1 新文本）。
///
/// 它只出现在变体树里（牙齿测试与鉴别力测试的绿基线），**不**参与对真树的断言：真树今天还是
/// 旧实现，轴测试必须因此为红。跨制品对账（这段文本确实是 E1 产物的子串）由
/// `r165_compile_gate.py` 断言 —— 复制粘贴的常量最怕的就是悄悄漂移。
const R165_FIXED_BODY: &str = concat!(
    "    const p = new URLSearchParams(txRangeParams());\n",
    "    const s = p.get(\"start\");\n",
    "    if (!s) return \"week\"; // 全部时间 / 自定义但起点为空：跨度不可知\n",
    "    const from = new Date(s);\n",
    "    const e = p.get(\"end\");\n",
    "    const to = e ? new Date(e) : new Date();\n",
    "    const days = (to.getTime() - from.getTime()) / 86400000;\n",
    "    if (days <= 3.5) return \"hour\";\n",
    "    if (days <= 60) return \"day\";\n",
    "    return \"week\";\n",
);

/// 一次扫描同时产出四条规则的判决**与它们的证据**（逐条可打印 —— #339/#341：判词与取值两列）。
///
/// ⚠️ 结构体里**不**留 `body`：它在 `r165_read` 内部被消费掉了（r2/r3 都读它），字段本身
/// 无人读 ⇒ `cargo clippy --all-targets -- -D warnings` 报 `field is never read`。
/// 这正是 R158 落地轮的坑（#603）：片段预检器用 `rustc --test`，**不读 lint**，
/// 所以「片段绿」不等于「CI 绿」。
struct R165Reading {
    options: Vec<String>,
    control_literals: Vec<String>,
    grains: BTreeSet<String>,
    r1: bool,
    r2: bool,
    r3: bool,
    r4: bool,
}

impl R165Reading {
    fn verdicts(&self) -> (bool, bool, bool, bool) {
        (self.r1, self.r2, self.r3, self.r4)
    }

    fn report(&self) -> String {
        format!(
            "r1={} r2={} r3={} r4={} | control_literals={:?} grains={:?} options={:?}",
            self.r1, self.r2, self.r3, self.r4, self.control_literals, self.grains, self.options
        )
    }
}

/// `ui/index.html` 里 `#tx-range` 的选项值（按出现顺序）。
///
/// 定位方式是「`id="tx-range"` 之前最近的那个 `<select`」—— 不假设同一行、也不数行号。
/// 找不到元素时返回空集，**由规则 4 的「选项集非空」把它变成响亮失败**（而不是静默无射程）。
fn r165_options(html: &str) -> Vec<String> {
    let Some(at) = html.find(R165_CONTROL_SELECT) else {
        return Vec::new();
    };
    let open = html[..at].rfind("<select").unwrap_or(at);
    let end = html[open..]
        .find("</select>")
        .map(|i| open + i)
        .unwrap_or(html.len());
    let block = &html[open..end];
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = block[from..].find("value=\"") {
        let v_at = from + rel + "value=\"".len();
        let Some(close) = block[v_at..].find('"').map(|i| v_at + i) else {
            break;
        };
        let v = block[v_at..close].to_string();
        if !v.is_empty() {
            out.push(v);
        }
        from = close + 1;
        if from >= block.len() {
            break;
        }
    }
    out
}

/// 函数体里 `return "…"` 的字面量集合。
fn r165_grains(body: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut from = 0usize;
    while let Some(rel) = body[from..].find("return \"") {
        let v_at = from + rel + "return \"".len();
        let Some(close) = body[v_at..].find('"').map(|i| v_at + i) else {
            break;
        };
        out.insert(body[v_at..close].to_string());
        from = close + 1;
        if from >= body.len() {
            break;
        }
    }
    out
}

/// 粒度函数的**已剥注释**的函数体；函数不在时返回 `None`（调用方负责响亮地报出来）。
fn r165_body(app: &str) -> Option<String> {
    // `?` 而不是 `if …is_none() { return None; }`：后者被 `clippy::question_mark` 判红，
    // 而片段预检器只跑 `rustc --test`、**不读 lint** ⇒ 这类失败只能在落地轮抓到（#603）。
    function_source(app, R165_GRAIN_FN)?;
    let body = code_body(app, R165_GRAIN_FN);
    if body.trim().is_empty() {
        None
    } else {
        Some(body)
    }
}

/// 把粒度函数的**体**替换成给定文本（造变体用）。锚点漂移即 panic —— 不静默失去射程。
///
/// 尾部锚点是「换行 + 恰好两空格 + `}` + 换行」：函数体内部的闭合括号缩进更深，
/// 所以第一个命中的就是函数自己的收尾（与 `r165_probe.js` 的 `replaceFn` 同一判据）。
fn r165_with_body(app: &str, body: &str) -> String {
    let head = format!("function {R165_GRAIN_FN}() {{");
    let at = app
        .find(&head)
        .unwrap_or_else(|| panic!("锚点 `{head}` 不在给定源码里 —— 变体无从构造"));
    let open = at + head.len();
    let close = app[open..]
        .find("\n  }\n")
        .map(|i| open + i)
        .unwrap_or_else(|| panic!("`{R165_GRAIN_FN}` 的函数尾锚点不在给定源码里"));
    format!("{}\n{}{}", &app[..open], body, &app[close..])
}

/// 登记表 → `Vec<String>`（比较用）。一处定义，三处共用 —— 否则三份写法各自漂移。
fn r165_expected_options() -> Vec<String> {
    R165_OPTIONS.iter().map(|o| o.to_string()).collect()
}

/// 一次读完四条规则（判词 + 证据）。
fn r165_read(app: &str, html: &str) -> R165Reading {
    let body = r165_body(app).unwrap_or_default();
    let options = r165_options(html);
    let control_literals: Vec<String> = options
        .iter()
        .filter(|o| body.contains(&format!("\"{o}\"")))
        .cloned()
        .collect();
    let grains = r165_grains(&body);
    let allowed: BTreeSet<String> = R165_GRAINS.iter().map(|g| g.to_string()).collect();
    let sources = app
        .matches(&format!("function {R165_WINDOW_SOURCE}("))
        .count();
    R165Reading {
        r1: control_literals.is_empty(),
        r2: mentions_identifier(&body, R165_WINDOW_SOURCE),
        r3: grains == allowed,
        r4: sources == 1 && !options.is_empty(),
        options,
        control_literals,
        grains,
    }
}

/// 变体构造器：每个变体**只此一处**定义，牙齿测试与鉴别力测试共用（#325 同族：
/// 同一件事的两个定义迟早会漂移）。它们都从**调用方给的树**派生 —— 因此与「这棵树是
/// 未修还是已修」无关，两腿跑同一套断言（这正是第一版错的地方：把「真树未修」写进了断言）。
fn r165_variant_fix(app: &str) -> String {
    r165_with_body(app, R165_FIXED_BODY)
}

/// 修复体 + 一句控件值分支 ⇒ 规则 1 单独翻红（「既委派又按控件重分支」正是本轴要挡的形状）。
fn r165_variant_control(app: &str) -> String {
    let body = format!("{R165_FIXED_BODY}    if (txRange === \"24h\") return \"hour\";\n");
    r165_with_body(app, &body)
}

/// 修复体但**不委派**给窗口真源 ⇒ 规则 2 单独翻红。
fn r165_variant_nocall(app: &str) -> String {
    r165_with_body(
        app,
        &R165_FIXED_BODY.replace(R165_WINDOW_SOURCE, "someOtherWindow"),
    )
}

/// 修复体 + 多一个粒度字面量 ⇒ 规则 3 单独翻红。
fn r165_variant_grain(app: &str) -> String {
    let body = format!("{R165_FIXED_BODY}    if (days <= 0.5) return \"minute\";\n");
    r165_with_body(app, &body)
}

/// 竞争修法 `m_day`：形状与修复体逐字同类，只把「无下界」那一支的取值从 `week` 改成 `day`。
fn r165_variant_day(app: &str) -> String {
    r165_variant_fix(app).replace("if (!s) return \"week\";", "if (!s) return \"day\";")
}

/// 轴：产品四处宣告「密码至少 8 位」，服务端三处却用 `String::len()`（UTF-8 **字节**）执行
/// 同一条规则 ⇒ 一条规则在同一棵树里活了两个单位：`密码abc`（5 字符 / 9 字节）被服务端放行，
/// 而服务端返回的那句话就是「密码至少 8 位」，且应用自己的找回密码表单（按字符计数）拒同一个密码。
///
/// 四条规则，各有独立的牙：
///
/// 1. **服务端走唯一真源**：三处请求校验一律经 `password_too_short()`；生产区里不得再有口令形状
///    的 `.len() <`；helper 自己的计数表达式必须是**宣告的那个单位**。
/// 2. **客户端同单位**：`ui/js/app.js` 的口令守卫必须是 `Array.from(...).length`；不得再有裸
///    `pw.length <`（UTF-16 code unit：一个星光面字符算 2，`😀😀😀😀` 会被当成 8 个）。
/// 3. **宣告与常量同数同单位**：四张脸（设计基线原型占位符 / zh 包 / en 包 / ERR_MAP 字面量）
///    必须**互相一致**、单位必须是**字符**，且后端常量与客户端常量的值都等于它们报出的 N。
///    N 从源码读出，不写死。
/// 4. **站点形状 == 3 + 1**：服务端三处 + 客户端一处；删掉任一处都不算修法（竞争修法
///    `m_drop_client` 就是删客户端守卫，让两边「不再矛盾」）。
///
/// **射程（诚实边界，已写进 `ui/README.md`）**：本门禁是**词法**的 —— 它证「两处实现数的单位
/// == 四张脸宣告的单位」，**不证**运行期某个样本真的被拒/被收（那半归 `src/routes/mod.rs` 的
/// 两条口令边界测试 `password_minimum_is_counted_in_characters_not_bytes` /
/// `register_rejects_a_password_short_in_characters_long_in_bytes`：**形状归门禁，事实归探针**）。
/// 也不证
/// 「那句字面量真会被服务端返回」（ERR_MAP 由它自己的门禁管）。单位取自**设计基线原型**
/// （`docs/prototype/`）—— 本次修复不碰它（坑 #537：期望值锚在爆炸半径之外）。
/// 「单位是字符」这个**方向**是产品自己的宣告（四处载体、含设计基线），把它改写成「字节」是
/// **改声明**，由规则 3 与 README 约定一起拒。
const PASS_MIN_MOD_RS: &str = include_str!("routes/mod.rs");
/// 设计基线原型：注册密码框的占位符出自这里。
const PASS_MIN_PROTO: &str = include_str!("../docs/prototype/aitokenpool-console.html");
/// 后端唯一真源的常量名。
const PASS_MIN_CONST: &str = "MIN_PASSWORD_CHARS";
/// 客户端同名常量。
const PASS_MIN_JS_CONST: &str = "MIN_PW_CHARS";
/// 后端唯一真源的函数名（三处请求校验必须走它）。
const PASS_MIN_FN: &str = "password_too_short";
/// 后端不得再出现的字节形状（`String::len()` 是 UTF-8 字节数）。
const PASS_MIN_BYTE_SHAPE: &str = "len() <";
/// 客户端必须用的计数层（`Array.from(...)` = Unicode 标量值）。
const PASS_MIN_JS_COUNT: &str = "Array.from(";
/// 四张「宣告」的脸的登记名（规则 3 的集合）。
const PASS_MIN_FACES: [&str; 4] = [
    "prototype placeholder",
    "zh pack",
    "en pack",
    "ERR_MAP literal",
];

/// 一张「宣告」的脸：登记名 / 原话 / 从原话读出的 `(N, 单位)`。
///
/// 用具名别名而不是就地写三元组：`clippy::type_complexity` 在 `-D warnings` 下会把内联的
/// `Vec<(&str, String, Option<(usize, &str)>)>` 判成「非常复杂的类型」而**让整个 crate 红**
/// （`cargo clippy --all-targets -D warnings` 是落地门禁的一部分）。
type PassMinFace = (&'static str, String, Option<(usize, &'static str)>);

/// `mod.rs` 的**生产区**（`#[cfg(test)]` 之前）。
///
/// 射程必须显式声明：本门禁落地时会在同一个文件里追加口令边界测试，那些测试**自己**会调用
/// `password_too_short(...)` —— 把测试区也算进来的话，规则 4 的「三处」会被自己的夹具顶破。
fn pass_min_prod(src: &str) -> &str {
    match src.find("#[cfg(test)]") {
        Some(i) => &src[..i],
        None => src,
    }
}

/// 从一句宣告里读出 `(N, 单位)`。单位只认「字符」与「字节」两种拼法，其余 ⇒ `None`。
fn pass_min_rule(text: &str) -> Option<(usize, &'static str)> {
    let n: usize = text
        .split(|c: char| !c.is_ascii_digit())
        .find(|s| !s.is_empty())?
        .parse()
        .ok()?;
    let unit = if text.contains('位') || text.contains("个字符") || text.contains("character") {
        "chars"
    } else if text.contains("字节") || text.contains("byte") {
        "bytes"
    } else {
        return None;
    };
    Some((n, unit))
}

/// 一个计数表达式数的是哪个单位：`chars().count()` / `Array.from(...)` ⇒ 字符；
/// `String::len()` ⇒ 字节；JS 的裸 `.length` ⇒ UTF-16 code unit（两者都不是）。
fn pass_min_unit_of(expr: &str) -> Option<&'static str> {
    if expr.contains("chars().count()") || expr.contains(PASS_MIN_JS_COUNT) {
        Some("chars")
    } else if expr.contains(".len() <") {
        Some("bytes")
    } else if expr.contains(".length <") {
        Some("utf16")
    } else {
        None
    }
}

/// 原型里注册密码框的 `placeholder`（设计基线的宣告原文）。
fn pass_min_proto_placeholder(src: &str) -> Option<String> {
    let at = src.find("id=\"reg-pass\"")?;
    let rest = &src[at..];
    let p = rest.find("placeholder=\"")? + "placeholder=\"".len();
    let rest = &rest[p..];
    Some(rest[..rest.find('"')?].to_string())
}

/// `ERR_MAP` 里弱口令那条的**字面量**（服务端返回给客户端的就是这句话）。
fn pass_min_err_map_literal(src: &str) -> Option<String> {
    let rest = &src[src.find("var ERR_MAP = [")?..];
    let key = rest.find("\"err.weakPassword\"")?;
    let close = rest[..key].rfind('"')?;
    let open = rest[..close].rfind('"')?;
    Some(rest[open + 1..close].to_string())
}

/// 四张「宣告」的脸（原话，不做断言 —— 期望由它们**互相**推出来，坑 #537）。
fn pass_min_carriers(i18n: &str, proto: &str) -> Vec<(&'static str, String)> {
    let zh = pack_region_strict(i18n, ZH_PACK_START, EN_PACK_START);
    let en = pack_region_strict(i18n, EN_PACK_START, PACK_END);
    vec![
        (
            "prototype placeholder",
            pass_min_proto_placeholder(proto).unwrap_or_default(),
        ),
        (
            "zh pack",
            pack_string(zh, "err.weakPassword").unwrap_or_default(),
        ),
        (
            "en pack",
            pack_string(en, "err.weakPassword").unwrap_or_default(),
        ),
        (
            "ERR_MAP literal",
            pass_min_err_map_literal(i18n).unwrap_or_default(),
        ),
    ]
}

/// 生产区里**提到口令**、又拿它比一个下限的行 —— 无论它是走 helper 还是非法的字节比较。
///
/// 这是规则 4 数的「站点」：半修（只把一处换回 `.len() <`）在这里**仍是三处**，所以它只翻规则 1，
/// 不连坐规则 4（`m_byte_server` 那条腿钉的就是这件事）。
fn pass_min_server_sites(src: &str) -> Vec<String> {
    code_only(src)
        .lines()
        .filter(|l| l.to_ascii_lowercase().contains("password"))
        .filter(|l| l.contains(PASS_MIN_FN) || l.contains(PASS_MIN_BYTE_SHAPE))
        .filter(|l| !l.contains(&format!("fn {PASS_MIN_FN}")))
        .map(|l| l.trim().to_string())
        .collect()
}

/// 生产区里**走 helper** 的那些行（规则 1 的「唯一真源」半边）。
fn pass_min_helper_calls(src: &str) -> Vec<String> {
    code_only(src)
        .lines()
        .filter(|l| l.contains(&format!("{PASS_MIN_FN}(")))
        .filter(|l| !l.contains(&format!("fn {PASS_MIN_FN}")))
        .map(|l| l.trim().to_string())
        .collect()
}

/// 生产区里残留的**字节口径**站点：提到口令、且是 `.len() <`。
fn pass_min_byte_sites(src: &str) -> Vec<String> {
    code_only(src)
        .lines()
        .filter(|l| l.to_ascii_lowercase().contains("password"))
        .filter(|l| l.contains(PASS_MIN_BYTE_SHAPE))
        .map(|l| l.trim().to_string())
        .collect()
}

/// 唯一真源（helper）的函数体，以定义行为界。
fn pass_min_helper_body(src: &str) -> Option<String> {
    let head = format!("fn {PASS_MIN_FN}(pw: &str) -> bool {{");
    let at = src.find(&head)?;
    let rest = &src[at..];
    Some(rest[..rest.find("\n}")?].to_string())
}

/// 后端常量的值（从源码读出，不写死 —— 规则 3 的右半边）。
fn pass_min_const_n(src: &str) -> Option<usize> {
    let head = format!("const {PASS_MIN_CONST}: usize = ");
    let at = src.find(&head)?;
    let rest = &src[at + head.len()..];
    rest[..rest.find(';')?].trim().parse().ok()
}

/// 客户端常量的值。
fn pass_min_js_const_n(src: &str) -> Option<usize> {
    let head = format!("const {PASS_MIN_JS_CONST} = ");
    let at = src.find(&head)?;
    let rest = &src[at + head.len()..];
    rest[..rest.find(';')?].trim().parse().ok()
}

/// `app.js` 里的口令长度守卫站点：既提到 `pw`，又拿它比一个下限（常量名或裸数字）。
///
/// 这个形状对**两种**写法都成立 ⇒ 规则 2（单位）与规则 4（站点数）彼此独立。
fn pass_min_client_sites(src: &str) -> Vec<String> {
    code_only(src)
        .lines()
        .filter(|l| l.contains("pw"))
        .filter(|l| l.contains(PASS_MIN_JS_CONST) || l.contains(".length <"))
        .map(|l| l.trim().to_string())
        .collect()
}

/// `app.js` 里残留的裸 `pw.length <`（UTF-16 code unit 口径）。
fn pass_min_client_bare(src: &str) -> Vec<String> {
    code_only(src)
        .lines()
        .filter(|l| l.contains("pw.length <"))
        .map(|l| l.trim().to_string())
        .collect()
}

/// 门禁读到的一切（四条规则共用同一份读数，`report()` 把它印出来）。
struct PassMinRead {
    /// 四张脸的原话 + 各自读出的 `(N, 单位)`。
    carriers: Vec<PassMinFace>,
    const_n: Option<usize>,
    js_const_n: Option<usize>,
    helper_body: Option<String>,
    helper_calls: Vec<String>,
    server_sites: Vec<String>,
    byte_sites: Vec<String>,
    client_sites: Vec<String>,
    client_bare: Vec<String>,
}

impl PassMinRead {
    /// 四张脸一致时报出的 `(N, 单位)`；不一致 ⇒ `None`。
    fn agreed(&self) -> Option<(usize, &'static str)> {
        let first = self.carriers.first()?.2?;
        if self.carriers.iter().all(|(_, _, r)| *r == Some(first)) {
            Some(first)
        } else {
            None
        }
    }

    /// 单位取自**设计基线原型**那张脸 —— 唯一位于本次修复爆炸半径之外的锚点（坑 #537）。
    fn declared(&self) -> Option<(usize, &'static str)> {
        self.carriers.first().and_then(|(_, _, r)| *r)
    }

    fn verdicts(&self) -> (bool, bool, bool, bool) {
        let declared = self.declared();
        let unit = declared.map(|(_, u)| u);
        // 1. 服务端：没有字节站点 + 每个站点都走 helper + helper 按宣告的单位计数
        let server = self.byte_sites.is_empty()
            && self.helper_calls.len() == self.server_sites.len()
            && !self.server_sites.is_empty()
            && self.helper_body.as_deref().and_then(pass_min_unit_of) == unit;
        // 2. 客户端：没有裸 `.length <`，且每个守卫都按宣告的单位计数
        //    （守卫一个都不剩时这里是恒真 —— 那由规则 4 单独接住，两条规则因此彼此独立）
        let client = self.client_bare.is_empty()
            && self
                .client_sites
                .iter()
                .all(|l| pass_min_unit_of(l) == unit);
        // 3. 宣告：四张脸互相一致、单位是字符、两个常量的值都等于 N
        let n = declared.map(|(n, _)| n);
        let decl = self.agreed().map(|(_, u)| u) == Some("chars")
            && n.is_some()
            && self.const_n == n
            && self.js_const_n == n;
        // 4. 形状：服务端 3 处 + 客户端 1 处
        let shape = self.server_sites.len() == 3 && self.client_sites.len() == 1;
        (server, client, decl, shape)
    }

    fn report(&self) -> String {
        let mut s = String::new();
        for (label, text, rule) in &self.carriers {
            s.push_str(&format!("    {label}: {text:?} -> {rule:?}\n"));
        }
        s.push_str(&format!(
            "    const N={:?} js const N={:?} | helper calls={} server sites={} byte sites={} \
             client sites={} bare={} | helper body unit={:?}",
            self.const_n,
            self.js_const_n,
            self.helper_calls.len(),
            self.server_sites.len(),
            self.byte_sites.len(),
            self.client_sites.len(),
            self.client_bare.len(),
            self.helper_body.as_deref().and_then(pass_min_unit_of),
        ));
        for l in self.byte_sites.iter().chain(self.client_bare.iter()) {
            s.push_str(&format!("\n      offending: {l}"));
        }
        s
    }
}

fn pass_min_read(mod_rs: &str, app_js: &str, i18n: &str, proto: &str) -> PassMinRead {
    let texts = pass_min_carriers(i18n, proto);
    let carriers: Vec<PassMinFace> = texts
        .into_iter()
        .map(|(label, text)| {
            let rule = pass_min_rule(&text);
            (label, text, rule)
        })
        .collect();
    let prod = pass_min_prod(mod_rs);
    PassMinRead {
        carriers,
        const_n: pass_min_const_n(prod),
        js_const_n: pass_min_js_const_n(app_js),
        helper_body: pass_min_helper_body(prod),
        helper_calls: pass_min_helper_calls(prod),
        server_sites: pass_min_server_sites(prod),
        byte_sites: pass_min_byte_sites(prod),
        client_sites: pass_min_client_sites(app_js),
        client_bare: pass_min_client_bare(app_js),
    }
}

// ============================================================================================
// R164 gate fragment -- splice into `src/state_gate.rs`
//
//   PART A (`r164_*` helpers)  -> insert BEFORE the line `mod tests {`
//   PART B (the tests)         -> insert INSIDE `mod tests {`, right after `    use super::*;`
//
// Splice/split is done by `r164_compile_gate.py`, which also COMPILES and RUNS this fragment
// against an explicitly materialized pre-fix tree (`git archive <HEAD>`) and against the landed
// tree -- the discrimination proof travels with the fragment.
//
// WHAT IT PINS (axis: the transactions table's column-header ▲/▼ is a claim about the WHOLE
// dataset; server paging means the ordering has to travel with the request):
//
//   The arrow is ONE claim with THREE carriers. `txTable.sort` is the only sort state, and it
//   must project into all three; any carrier that lags behind makes the arrow describe a口径
//   nobody executes:
//     R1  the LIST request carries it  (a projector that emits `&sort=`/`&dir=`, called from the
//         `/api/transactions?` line and NOT from the trend line -- row order means nothing for
//         time buckets)
//     R2  the payload signature covers it (`txQuerySig`, which the reload guard compares: without
//         it a header click changes the state, the signature does not, nothing refetches, and the
//         arrow moves over a list that does not -- worse than the defect being fixed)
//     R3  the local sort steps aside for a table that declares server sorting (the declared flag
//         travels with the call site; the condition is read WHOLE -- a paren character class
//         cannot see `!(serverPaging && serverSort)`, pitfall #460)
//     R4  the server renders `ORDER BY` from a whitelist that IS the column roster, and the user
//         string never reaches SQL:
//           a. whitelist == the column keys derived from `TX_COLUMNS` (both directions)
//           b. the match arms of `tx_sort_expr` == the whitelist (both directions)
//           c. the declared array length == the number of literals (self-consistency)
//           d. `ORDER BY {ident}` is a placeholder whose `ident` is bound by the whitelist-guarded
//              builder, and every `q.sort` / `q.dir` mention lives in that binding
//     R5  CONTROL (green on the base tree too): the FILTER half of the same road is real --
//         `serverFilter: true` on the filterable columns, the request calls `txFilterParams()`,
//         `TxColFilters` is flattened into the query, and the reload guard compares the
//         signature.  A scanner whose positive reach is empty proves nothing.
//
// SCOPE, the honest half (#341): the rules are LEXICAL. They prove the SHAPE (three carriers
// agree, the whitelist IS the roster). They do NOT prove that the rows on screen really are
// globally ordered -- that belongs to the jsdom probe (`r164_probe.js`, legs S1-S7, re-run on the
// landed bytes), and the ORDER BY semantics themselves to `src/routes/wallet.rs`'s behaviour
// tests. Shape belongs to the gate; facts belong to the instruments.
// ============================================================================================

// ============================= R164 PART A: module-level helpers ==============================

/// 列表端点的查询参数构造器所在的文件（本门禁的另一半：白名单与 `ORDER BY` 的渲染者）。
const R164_WALLET: &str = include_str!("routes/wallet.rs");

/// 排序状态的**唯一真源**（三处载体都必须读它）。
const R164_SORT_STATE: &str = "txTable.sort";

/// 列名册的**阳性对照**（#451）：`TX_COLUMNS` 一漂移就响亮地失败，而不是让「白名单 == 名册」
/// 这条规则在空集上静默恒真（坑 68 同族）。11 键与 `src/routes/wallet.rs::TX_SORT_KEYS` 同源。
const R164_COLUMNS: [&str; 11] = [
    "time", "type", "user", "model", "key", "input", "cached", "output", "tokens", "pts", "status",
];

/// 一次扫描同时产出五条规则的判决**与它们的证据**（判词与取值两列 —— #339/#341：一个
/// 期望藏在脚注里的仪器会报出一个自洽的谎）。
struct R164Reading {
    columns: Vec<String>,
    whitelist: Vec<String>,
    declared_len: usize,
    arms: Vec<String>,
    projectors: Vec<String>,
    guard_flag: Option<String>,
    placeholder: Option<String>,
    binder: Option<String>,
    param_lines: Vec<String>,
    server_filters: usize,
    r1: bool,
    r2: bool,
    r3: bool,
    r4a: bool,
    r4b: bool,
    r4c: bool,
    r4d: bool,
    r5: bool,
}

impl R164Reading {
    /// 三处载体（R1–R4）是否全都就位 —— 轴那一半。
    fn carriers_agree(&self) -> bool {
        self.r1 && self.r2 && self.r3 && self.r4a && self.r4b && self.r4c && self.r4d
    }

    fn report(&self) -> String {
        format!(
            "r1={} r2={} r3={} r4a={} r4b={} r4c={} r4d={} r5={} | columns={:?} whitelist={:?} \
             arms={:?} projectors={:?} guard_flag={:?} placeholder={:?} binder={:?} \
             param_lines={:?} server_filters={}",
            self.r1,
            self.r2,
            self.r3,
            self.r4a,
            self.r4b,
            self.r4c,
            self.r4d,
            self.r5,
            self.columns,
            self.whitelist,
            self.arms,
            self.projectors,
            self.guard_flag,
            self.placeholder,
            self.binder,
            self.param_lines,
            self.server_filters,
        )
    }
}

/// 逐行剥离注释后的整段代码（`code_text_by_line` 会 trim 每行；行数不变）。
///
/// 一切**标识符**判定都必须在这上面做：本轮的解释性注释里正写着 `serverSort` 与
/// `txTable.sort`（坑 #296 的镜像 —— 注释是自己的修法最容易踩的假阳性）。
fn r164_code(src: &str) -> String {
    code_text_by_line(src).join("\n")
}

/// `needle` 之后**第一个** `{` 起、按花括号配平的整段（含两端花括号）。
///
/// 不用 [`decl_spans`]：它只认 JS 的 `function NAME(` / `const NAME = (`，而本轴的另外两半是
/// Rust（`fn tx_sort_expr` / `fn tx_order_by`）；且它的收尾锚在**声明行**上，签名换行的 Rust
/// 函数会被截断。花括号配平对两种语言同样成立（本文件不解析字符串里的括号）。
fn r164_body_after(src: &str, needle: &str) -> Option<String> {
    let at = src.find(needle)?;
    let open = src[at..].find('{').map(|i| at + i)?;
    let mut depth = 0i32;
    for (i, c) in src[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(src[open..open + i + 1].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// `const TX_COLUMNS = [` 起、按**方括号**配平的数组字面量（含两端方括号）。
fn r164_columns_region(app: &str) -> Option<String> {
    let at = app.find("const TX_COLUMNS = [")?;
    let open = app[at..].find('[').map(|i| at + i)?;
    let mut depth = 0i32;
    for (i, c) in app[open..].char_indices() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(app[open..open + i + 1].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// 标识符字符（`[A-Za-z0-9_$]`）—— 左界判定与 `mentions_identifier` 同一把尺子。
fn r164_is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

/// `body` 里所有 `field: "…"` 的字符串值（**标识符左界**：`sortVal:` / `key_name` 不是它）。
fn r164_keyed_literals(body: &str, field: &str) -> Vec<String> {
    let needle = format!("{field}: \"");
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = body[from..].find(&needle) {
        let at = from + rel;
        let before_ok = at == 0 || !r164_is_word(body.as_bytes()[at - 1] as char);
        let v_at = at + needle.len();
        let Some(close) = body[v_at..].find('"').map(|i| v_at + i) else {
            break;
        };
        if before_ok {
            out.push(body[v_at..close].to_string());
        }
        from = close + 1;
        if from >= body.len() {
            break;
        }
    }
    out
}

/// `pub const TX_SORT_KEYS: [&str; N] = [ … ];` ⇒ `(N, 字面量)`。
fn r164_whitelist(wallet: &str) -> Option<(usize, Vec<String>)> {
    let head = "pub const TX_SORT_KEYS: [&str; ";
    let at = wallet.find(head)? + head.len();
    let len_end = at + wallet[at..].find(']')?;
    let declared: usize = wallet[at..len_end].trim().parse().ok()?;
    let arr = wallet[len_end..].find("= [").map(|i| len_end + i)?;
    let body = r164_body_between(wallet, arr, '[', ']')?;
    Some((declared, r164_string_literals(&body)))
}

/// 从 `at` 处的开括号起、按 `open`/`close` 配平的区间文本（含两端）。
fn r164_body_between(src: &str, at: usize, open: char, close: char) -> Option<String> {
    let mut depth = 0i32;
    for (i, c) in src[at..].char_indices() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some(src[at..at + i + 1].to_string());
            }
        }
    }
    None
}

/// 文本里所有 `"…"` 字面量（本轴的两个表达式串里没有转义引号）。
fn r164_string_literals(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = text[from..].find('"') {
        let v_at = from + rel + 1;
        let Some(close) = text[v_at..].find('"').map(|i| v_at + i) else {
            break;
        };
        out.push(text[v_at..close].to_string());
        from = close + 1;
        if from >= text.len() {
            break;
        }
    }
    out
}

/// `fn tx_sort_expr` 的**匹配臂左值**（`"time" => …` 里的 `time`）。
///
/// 判据是「字符串字面量**紧跟** `=>`」：臂体里的表达式串（`"COALESCE(NULLIF(…))"`）后面是
/// `.to_string()`，不会被算进来。
fn r164_arm_keys(wallet: &str) -> Vec<String> {
    let Some(body) = r164_body_after(wallet, "fn tx_sort_expr(") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = body[from..].find('"') {
        let v_at = from + rel + 1;
        let Some(close) = body[v_at..].find('"').map(|i| v_at + i) else {
            break;
        };
        let rest = body[close + 1..].trim_start();
        if rest.starts_with("=>") {
            out.push(body[v_at..close].to_string());
        }
        from = close + 1;
        if from >= body.len() {
            break;
        }
    }
    out
}

/// 「排序投影器」：体内**同时**发出 `&sort=` 与 `&dir=` 的函数名（调用方要求恰好一个）。
fn r164_projectors(app: &str) -> Vec<String> {
    let spans = decl_spans(app);
    let mut out = Vec::new();
    for (name, s, e) in &spans {
        let body = &app[*s..*e];
        if body.contains("\"&sort=\"") && body.contains("\"&dir=\"") {
            out.push(name.clone());
        }
    }
    out
}

/// 含 `needle` 的**代码行**（注释已剥离、逐行 trim）。
fn r164_lines_with(src: &str, needle: &str) -> Vec<String> {
    src.lines()
        .map(|l| l.trim().to_string())
        .filter(|l| l.contains(needle))
        .collect()
}

/// `callee({ … })` 调用的实参对象字面量（花括号配平）。
fn r164_call_region(app: &str, callee: &str) -> Option<String> {
    let at = app.find(&format!("{callee}({{"))?;
    let open = at + callee.len() + 1;
    r164_body_between(app, open, '{', '}')
}

/// 对象字面量里所有 `field: true,` 的字段名（调用点**声明**了哪些开关）。
fn r164_true_flags(region: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = region[from..].find(": true,") {
        let colon = from + rel;
        let name = r164_ident_before(region, colon);
        if !name.is_empty() {
            out.push(name);
        }
        from = colon + ": true,".len();
        if from >= region.len() {
            break;
        }
    }
    out
}

/// `at` 之前紧邻的标识符（左界只吃**一个**标识符的字符，遇非标识符字符即停）。
fn r164_ident_before(text: &str, at: usize) -> String {
    let mut start = at;
    for (i, c) in text[..at].char_indices().rev() {
        if r164_is_word(c) {
            start = i;
        } else {
            break;
        }
    }
    text[start..at].to_string()
}

/// 含 `needle` 的那个 `if (` 的**完整条件文本**（从 `if (` 扫到配平的 `)`）。
///
/// 判据必须是「整条条件」，不是括号字符类的切片：本轴的修复条件**自带括号**
/// （`!(serverPaging && serverSort)`），用 `[^)]*` 取条件的规则会永远取不到它（坑 #460）。
fn r164_condition_around(body: &str, needle: &str) -> Option<String> {
    let hit = body.find(needle)?;
    let start = body[..hit].rfind("if (")?;
    let open = start + 3;
    let mut depth = 0i32;
    for (i, c) in body[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(body[open..open + i + 1].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// `ORDER BY {ident}` 里的 `ident`（字面量 `ORDER BY t.id DESC` ⇒ `None`，那正是旧实现）。
fn r164_order_placeholder(wallet: &str) -> Option<String> {
    let at = wallet.find("ORDER BY {")? + "ORDER BY {".len();
    let close = wallet[at..].find('}').map(|i| at + i)?;
    Some(wallet[at..close].to_string())
}

/// `let IDENT = CALLEE(` 的 `(IDENT, CALLEE)` —— 只认**含 `q.sort`** 的那一行。
fn r164_binder(wallet: &str) -> Option<(String, String)> {
    for line in wallet.lines() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix("let ") else {
            continue;
        };
        if !rest.contains("q.sort") {
            continue;
        }
        let Some((lhs, rhs)) = rest.split_once('=') else {
            continue;
        };
        let Some((callee, _)) = rhs.trim().split_once('(') else {
            continue;
        };
        let ident = lhs.trim();
        let callee = callee.trim();
        if ident.is_empty() || callee.is_empty() {
            continue;
        }
        return Some((ident.to_string(), callee.to_string()));
    }
    None
}

/// 排序后的副本（集合比较：`TX_COLUMNS` 的键序与白名单的声明序不承诺一致）。
fn r164_sorted(v: &[String]) -> Vec<String> {
    let mut out = v.to_vec();
    out.sort();
    out
}

/// 一次读完五条规则（判词 + 证据）。
fn r164_read(app: &str, wallet: &str) -> R164Reading {
    let code = r164_code(app);
    let columns = r164_columns_region(&code)
        .map(|r| r164_keyed_literals(&r, "key"))
        .unwrap_or_default();
    let (declared_len, whitelist) = r164_whitelist(wallet).unwrap_or((0, Vec::new()));
    let arms = r164_arm_keys(wallet);
    let projectors = r164_projectors(&code);
    let projector = match projectors.len() {
        1 => Some(projectors[0].clone()),
        _ => None,
    };
    // ⚠️ 两行请求行必须**在 `loadTransactions` 体内**取：整文件里 `/api/transactions?` 还有
    // 仪表盘的 `page_size=1` 那一处（更早、且不含排序参数）—— 取「第一处」会把列表请求认成仪表盘。
    let load = r164_body_after(&code, "function loadTransactions(").unwrap_or_default();
    let list_req = r164_lines_with(&load, "\"/api/transactions?")
        .into_iter()
        .next()
        .unwrap_or_default();
    let trend_req = r164_lines_with(&load, "\"/api/transactions/trend")
        .into_iter()
        .next()
        .unwrap_or_default();
    let sig = r164_body_after(&code, "function txQuerySig(").unwrap_or_default();
    let guard = r164_body_after(&code, "function buildDataTable(").unwrap_or_default();
    let cond = r164_condition_around(&guard, "state.sort.length").unwrap_or_default();
    let flags = r164_call_region(&code, "buildDataTable")
        .map(|r| r164_true_flags(&r))
        .unwrap_or_default();
    let guard_flag = flags
        .iter()
        .find(|f| mentions_identifier(&cond, f))
        .cloned();
    let placeholder = r164_order_placeholder(wallet);
    let binder = r164_binder(wallet);
    let param_lines = r164_lines_with(wallet, "q.sort");
    let mut param_lines2 = param_lines.clone();
    param_lines2.extend(r164_lines_with(wallet, "q.dir"));
    param_lines2.sort();
    param_lines2.dedup();
    let server_filters = r164_columns_region(&code)
        .map(|r| r164_lines_with(&r, "serverFilter: true").len())
        .unwrap_or(0);
    let reload = r164_body_after(&code, "function renderTransactions(").unwrap_or_default();

    // R1：投影器存在且**唯一**、它读排序状态、列表请求调它、**趋势不调**它。
    let projector_state = projector
        .as_ref()
        .and_then(|p| r164_body_after(&code, &format!("function {p}(")))
        .map(|b| mentions_identifier(&b, R164_SORT_STATE))
        .unwrap_or(false);
    let call = projector.as_ref().map(|p| format!("{p}()"));
    let r1 = projector_state
        && call
            .as_ref()
            .map(|c| list_req.contains(c.as_str()))
            .unwrap_or(false)
        && call
            .as_ref()
            .map(|c| !trend_req.contains(c.as_str()))
            .unwrap_or(false);

    // R2：载荷签名覆盖排序状态（少了它，点列头只改状态、签名不变 ⇒ 守卫不重拉）。
    let r2 = mentions_identifier(&sig, R164_SORT_STATE);

    // R3：本地排序被**调用点声明的**开关豁免（条件读整条）。
    let r3 = guard_flag.is_some() && cond.contains("state.sort.length");

    // R4a/b/c/d。
    let r4a = !columns.is_empty() && r164_sorted(&columns) == r164_sorted(&whitelist);
    let r4b = !arms.is_empty() && r164_sorted(&arms) == r164_sorted(&whitelist);
    let r4c = declared_len == whitelist.len() && !whitelist.is_empty();
    let builder_guarded = binder
        .as_ref()
        .and_then(|(_, callee)| r164_body_after(wallet, &format!("fn {callee}(")))
        .map(|b| b.contains("tx_sort_expr("))
        .unwrap_or(false);
    let r4d = placeholder.is_some()
        && binder
            .as_ref()
            .map(|(id, _)| *id == placeholder.clone().unwrap_or_default())
            .unwrap_or(false)
        && builder_guarded
        && param_lines2.len() == 1;

    // R5（对照）：同一条路的前一半（列筛选后端化）必须**仍然**是真的。
    // ⚠️「请求带列筛选」的证据是**构造器被调用**（`const cols = txFilterParams()` 在请求行上一行），
    // 不是「请求行里出现这六个字」—— 判据必须落在调用上，不能落在某一行的排版上。
    let r5 = server_filters >= 6
        && load.contains("txFilterParams()")
        && list_req.contains("cols")
        && wallet.contains("#[serde(flatten)]")
        && wallet.contains("TxColFilters")
        && reload.contains("txQuerySig()");

    R164Reading {
        columns,
        whitelist,
        declared_len,
        arms,
        projectors,
        guard_flag,
        placeholder,
        binder: binder.map(|(id, callee)| format!("{id} = {callee}")),
        param_lines: param_lines2,
        server_filters,
        r1,
        r2,
        r3,
        r4a,
        r4b,
        r4c,
        r4d,
        r5,
    }
}

// ── 合成夹具：牙齿测试与鉴别力测试都跑在同一份**自足**的迷你源码上（#612：为树 A 写的
// 声明表对树 B 无效 —— 所以变体树不从真树派生，而是自带一份）。真树由轴测试与
// `r164_compile_gate.py` 的物化基线腿覆盖。
//
// 迷你源码的形态与真树**同构**（同样的锚点：`const TX_COLUMNS = [`、`function txSortParams(`、
// `"/api/transactions?`、`let order = tx_order_by(q.sort…`、`ORDER BY {order}`），
// 只是把 11 列压到 6 列；`r164_compile_gate.py` 另有一条腿断言修复体的关键片段确实是真树的子串。

const R164_MINI_APP: &str = concat!(
    "const TX_COLUMNS = [\n",
    "  { key: \"a\", title: () => T(\"a\"), serverFilter: true },\n",
    "  { key: \"b\", title: () => T(\"b\"), serverFilter: true },\n",
    "  { key: \"c\", title: () => T(\"c\"), serverFilter: true },\n",
    "  { key: \"d\", title: () => T(\"d\"), serverFilter: true },\n",
    "  { key: \"e\", title: () => T(\"e\"), serverFilter: true },\n",
    "  { key: \"f\", title: () => T(\"f\"), serverFilter: true }\n",
    "];\n",
    "const txTable = { sort: [], filters: {} };\n",
    "function txFilterParams() {\n  return \"\";\n}\n",
    "function txSortParams() {\n",
    "  if (!txTable.sort || !txTable.sort.length) return \"\";\n",
    "  return \"&sort=\" + txTable.sort.map((s) => s.key) + \"&dir=\" + txTable.sort.map((s) => s.dir);\n",
    "}\n",
    "function txQuerySig() {\n",
    "  const srt = (txTable.sort || []).map((s) => s.key).join(\",\");\n",
    "  return \"x|\" + srt;\n",
    "}\n",
    "function renderTransactions() {\n",
    "  if (txTable.loadedQuerySig !== txQuerySig()) reloadTransactions();\n",
    "}\n",
    "function loadTransactions() {\n",
    "  const cols = txFilterParams();\n",
    "  const q = \"/api/transactions?type=\" + type + (cols ? \"&\" + cols : \"\") + txSortParams();\n",
    "  const tq = \"/api/transactions/trend?type=\" + type + (cols ? \"&\" + cols : \"\");\n",
    "  return [q, tq];\n",
    "}\n",
    "function buildDataTable(cfg) {\n",
    "  const { container, columns, rows, state, onState, serverPaging, serverSort } = cfg;\n",
    "  if (state.sort.length && !(serverPaging && serverSort)) {\n",
    "    data = data.slice().sort(cmp);\n",
    "  }\n",
    "}\n",
    "function viewTx() {\n",
    "  buildDataTable({\n",
    "    container: $(\"#tx-table\"),\n",
    "    columns: TX_COLUMNS,\n",
    "    rows: list,\n",
    "    state: txTable,\n",
    "    onState: renderTransactions,\n",
    "    serverPaging: { total: 1 },\n",
    "    serverSort: true,\n",
    "  });\n",
    "}\n",
);

const R164_MINI_WALLET: &str = concat!(
    "pub const TX_SORT_KEYS: [&str; 6] = [\"a\", \"b\", \"c\", \"d\", \"e\", \"f\"];\n",
    "\n",
    "fn tx_sort_expr(key: &str) -> Option<String> {\n",
    "    Some(match key {\n",
    "        \"a\" => \"t.a\".to_string(),\n",
    "        \"b\" => \"t.b\".to_string(),\n",
    "        \"c\" => \"t.c\".to_string(),\n",
    "        \"d\" => \"t.d\".to_string(),\n",
    "        \"e\" => \"t.e\".to_string(),\n",
    "        \"f\" => \"t.f\".to_string(),\n",
    "        _ => return None,\n",
    "    })\n",
    "}\n",
    "\n",
    "fn tx_order_by(sort: Option<&str>, dir: Option<&str>) -> Result<String, ApiErr> {\n",
    "    let keys = split_keys(sort);\n",
    "    if keys.is_empty() {\n",
    "        return Ok(\"t.id DESC\".to_string());\n",
    "    }\n",
    "    let mut parts: Vec<String> = Vec::new();\n",
    "    for key in keys.iter() {\n",
    "        let expr = tx_sort_expr(key).ok_or_else(bad)?;\n",
    "        parts.push(format!(\"{expr} ASC\"));\n",
    "    }\n",
    "    parts.push(\"t.id DESC\".to_string());\n",
    "    Ok(parts.join(\", \"))\n",
    "}\n",
    "\n",
    "#[derive(Debug, Deserialize)]\n",
    "pub struct TxQuery {\n",
    "    pub sort: Option<String>,\n",
    "    pub dir: Option<String>,\n",
    "    #[serde(flatten)]\n",
    "    pub filters: TxColFilters,\n",
    "}\n",
    "\n",
    "pub async fn transactions(q: TxQuery) -> Result<String, ApiErr> {\n",
    "    let order = tx_order_by(q.sort.as_deref(), q.dir.as_deref())?;\n",
    "    let sql = format!(\"SELECT 1 WHERE {where_sql} ORDER BY {order} LIMIT ?{} OFFSET ?{}\", n + 1, n + 2);\n",
    "    Ok(sql)\n",
    "}\n",
);

/// 迷你**修复体**（阳性基线）：五条规则必须全绿 —— 否则牙齿测试无从谈起。
fn r164_mini_fixed() -> String {
    R164_MINI_APP.to_string()
}

fn r164_mini_fixed_wallet() -> String {
    R164_MINI_WALLET.to_string()
}

/// 把 `needle` 的**第一次**出现换成 `repl`；锚点不在即 panic（变体无从构造时不许静默失去射程）。
fn r164_swap(src: &str, needle: &str, repl: &str) -> String {
    assert!(
        src.contains(needle),
        "变体锚点 {needle:?} 不在给定的迷你源码里"
    );
    src.replacen(needle, repl, 1)
}

// ============================== R167 PART A: module-level helpers ================================

// ── R167：分页器的**省略号判据**必须由窗口边界推导，不能各自拍常数 ───────────────────────────
//
// `pagerButtons(page, pages)`（`ui/js/app.js`）在页数 > 9 时打印紧凑窗口 `1 … p-1 p p+1 … N`：
//
//     out.push(1);
//     if (page > L) out.push("…");
//     for (let i = Math.max(2, page - A); i <= Math.min(pages - 1, page + B); i++) out.push(i);
//     if (page < pages - R) out.push("…");
//     out.push(pages);
//
// 窗口半宽 A / B 由那行循环**定义**；L / R 必须与它自洽：
//   左侧：`1` 之后、窗口左端 `page - A` 之前只差数字 2 ⇒ 有间隙 ⟺ page - A > 2 ⟺ page > A + 2；
//   右侧：窗口右端 `page + B` 之后、`pages` 之前只差 pages - 1 ⇒ 有间隙 ⟺ page < pages - (B + 1)。
// 旧实现（#135 `052b60c`）的 `4` / `pages - 3` 比 `A + 2` / `B + 1` 紧一格 ⇒ 恰有两处
// （page = A + 3 与 page = pages - (B + 2)）会**印出相邻页码却不放省略号**：`1 3 4 5 …`、
// `1 … 8 9 10 12`。而省略号是不可点的 `<span>`（`user-select:none`）、全仓没有 prev/next
// ⇒ 用户无从知道那一页还在不在，只能先点 `1` 绕回去。
//
// 本门禁钉的是**派生关系**（#469：门禁不许把这一次编辑的字面量写死）：它从函数体里读出 A / B，
// 再要求 L == A + 2、R == B + 1 —— 于是它接受**任何自洽的窗口**（含比修复体更宽的那种）；
// 「屏幕上真的没有缺口」「窗口真的够窄」由 jsdom 探针 `r167_pager_harness.js` 证。

/// 分页器函数名。
const R167_FN: &str = "pagerButtons";

/// 窗口左半宽 A 的锚（`Math.max(2, page - A)`）。
const R167_WINDOW_LEFT: &str = "Math.max(2, page - ";
/// 窗口右半宽 B 的锚（`Math.min(pages - 1, page + B)`）。
const R167_WINDOW_RIGHT: &str = "Math.min(pages - 1, page + ";
/// 左侧省略号判据的锚（`if (page > L)`）。
const R167_GUARD_LEFT: &str = "if (page > ";
/// 右侧省略号判据的锚（`if (page < pages - R)`）。
const R167_GUARD_RIGHT: &str = "if (page < pages - ";
/// 「页数少就全量渲染」那一支 —— 紧凑形状的锚。
const R167_SMALL_BRANCH: &str = "if (pages <= 9)";
/// 省略号的压入点（计数用）。
const R167_ELLIPSIS: &str = "out.push(\"…\")";
/// `pagerButtons` 的唯一消费点（R4 的输入）。
const R167_CONSUMER: &str = "pagerButtons(state.page, pages)";

/// 修复后的函数**体**（逐字摘自编辑表 `r167_verify_edits.py` 的 E1 新文本，由生成器切片而非手抄）。
///
/// 它只出现在变体树里（牙齿测试与鉴别力测试的绿基线），**不**参与对真树的断言：真树今天还是
/// 旧实现，轴测试必须因此为红。跨制品对账（这段文本确实是 E1 产物的子串）由
/// `r167_compile_gate.py` 断言 —— 复制粘贴的常量最怕的就是悄悄漂移。
const R167_FIXED_BODY: &str = concat!(
    "    const out = [];\n",
    "    if (pages <= 9) { for (let i = 1; i <= pages; i++) out.push(i); return out; }\n",
    "    out.push(1);\n",
    "    if (page > 3) out.push(\"…\");\n",
    "    for (let i = Math.max(2, page - 1); i <= Math.min(pages - 1, page + 1); i++) out.push(i);\n",
    "    if (page < pages - 2) out.push(\"…\");\n",
    "    out.push(pages);\n",
    "    return out;\n",
);

/// 一次扫描同时产出四条规则的判决**与它们的证据**（逐条可打印 —— #339/#341：判词与取值两列）。
struct R167Reading {
    body: String,
    a: Option<i64>,
    b: Option<i64>,
    left_guard: Option<i64>,
    right_guard: Option<i64>,
    ellipses: usize,
    small_branch: bool,
    definitions: usize,
    consumers: usize,
    r1: bool,
    r2: bool,
    r3: bool,
    r4: bool,
}

impl R167Reading {
    fn verdicts(&self) -> (bool, bool, bool, bool) {
        (self.r1, self.r2, self.r3, self.r4)
    }

    fn report(&self) -> String {
        format!(
            "r1={} r2={} r3={} r4={} | A={:?} B={:?} L={:?} R={:?} ellipses={} small={} defs={} consumers={} body_lines={}",
            self.r1,
            self.r2,
            self.r3,
            self.r4,
            self.a,
            self.b,
            self.left_guard,
            self.right_guard,
            self.ellipses,
            self.small_branch,
            self.definitions,
            self.consumers,
            self.body.lines().count(),
        )
    }
}

/// `needle` **恰好出现一次**时，取它后面紧跟的十进制整数（否则 `None`）。
///
/// 「恰好一次」是判据的一部分：两个候选意味着两条各自独立的路径，读数就不再是那个数了。
/// 找不到元素时返回 `None`，**由规则把它变成响亮失败**（而不是静默取一个默认值）。
fn r167_int_after(text: &str, needle: &str) -> Option<i64> {
    if text.matches(needle).count() != 1 {
        return None;
    }
    let at = text.find(needle)? + needle.len();
    let digits: String = text[at..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

/// 分页器函数的**已剥注释**的函数体；函数不在时返回 `None`（调用方负责响亮地报出来）。
fn r167_body(app: &str) -> Option<String> {
    function_source(app, R167_FN)?; // 函数不在时短路（clippy::question_mark，R167 落地轮的 lint 腿抓到）
    let body = code_body(app, R167_FN);
    if body.trim().is_empty() {
        None
    } else {
        Some(body)
    }
}

/// 把分页器函数的**体**替换成给定文本（造变体用）。锚点漂移即 panic —— 不静默失去射程。
///
/// 尾部锚点是「换行 + 恰好两空格 + `}` + 换行」：函数体内部的闭合括号缩进更深，
/// 所以第一个命中的就是函数自己的收尾（与探针的 `replaceFn` 同一判据）。
fn r167_with_body(app: &str, body: &str) -> String {
    let head = format!("function {R167_FN}(page, pages) {{");
    let at = app
        .find(&head)
        .unwrap_or_else(|| panic!("锚点 `{head}` 不在给定源码里 —— 变体无从构造"));
    let open = at + head.len();
    let close = app[open..]
        .find("\n  }\n")
        .map(|i| open + i)
        .unwrap_or_else(|| panic!("`{R167_FN}` 的函数尾锚点不在给定源码里"));
    format!(
        "{}\n{}{}",
        &app[..open],
        body.trim_end_matches('\n'),
        &app[close..]
    )
}

/// 某个判据所在的**那一行**是否同时压入省略号（形状腿：判据必须真的守着省略号）。
fn r167_guard_line_has_ellipsis(body: &str, guard: &str) -> bool {
    body.lines()
        .any(|l| l.contains(guard) && l.contains(R167_ELLIPSIS))
}

/// 一次读完四条规则（判词 + 证据）。
fn r167_read(app: &str) -> R167Reading {
    let body = r167_body(app).unwrap_or_default();
    let a = r167_int_after(&body, R167_WINDOW_LEFT);
    let b = r167_int_after(&body, R167_WINDOW_RIGHT);
    let left_guard = r167_int_after(&body, R167_GUARD_LEFT);
    let right_guard = r167_int_after(&body, R167_GUARD_RIGHT);
    let ellipses = body.matches(R167_ELLIPSIS).count();
    let small_branch = body.matches(R167_SMALL_BRANCH).count() == 1;
    let definitions = app.matches(&format!("function {R167_FN}(")).count();
    let calls = app.matches(&format!("{R167_FN}(")).count();
    let consumers = calls.saturating_sub(definitions);
    R167Reading {
        r1: matches!((a, left_guard), (Some(a), Some(l)) if l == a + 2),
        r2: matches!((b, right_guard), (Some(b), Some(r)) if r == b + 1),
        r3: small_branch
            && ellipses == 2
            && r167_guard_line_has_ellipsis(&body, R167_GUARD_LEFT)
            && r167_guard_line_has_ellipsis(&body, R167_GUARD_RIGHT),
        r4: definitions == 1 && consumers >= 1,
        a,
        b,
        left_guard,
        right_guard,
        ellipses,
        small_branch,
        definitions,
        consumers,
        body,
    }
}

// ── 变体构造器：每个变体**只此一处**定义，牙齿测试与鉴别力测试共用（#325 同族）────────────
//
// 它们全部从 `R167_FIXED_BODY` 派生（判据的数字也从修复体自己读出的 A / B 推导，不写死 `3` / `2`）
// —— 因此与「这棵树是未修还是已修」无关，两腿跑同一套断言。

/// 修复体的窗口半宽 (A, B) —— 从修复体自己读出，不写死。
fn r167_fixed_half_widths() -> (i64, i64) {
    let a = r167_int_after(R167_FIXED_BODY, R167_WINDOW_LEFT)
        .unwrap_or_else(|| panic!("修复体里读不出窗口左半宽 A"));
    let b = r167_int_after(R167_FIXED_BODY, R167_WINDOW_RIGHT)
        .unwrap_or_else(|| panic!("修复体里读不出窗口右半宽 B"));
    (a, b)
}

/// 把窗口半宽改写成 `(a, b)`（省略号判据不动 —— 调用方负责让它们自洽）。
fn r167_set_window(body: &str, a: i64, b: i64) -> String {
    let cur_a = r167_int_after(body, R167_WINDOW_LEFT)
        .unwrap_or_else(|| panic!("给定体里读不出窗口左半宽"));
    let cur_b = r167_int_after(body, R167_WINDOW_RIGHT)
        .unwrap_or_else(|| panic!("给定体里读不出窗口右半宽"));
    let out = body
        .replace(
            &format!("{R167_WINDOW_LEFT}{cur_a})"),
            &format!("{R167_WINDOW_LEFT}{a})"),
        )
        .replace(
            &format!("{R167_WINDOW_RIGHT}{cur_b})"),
            &format!("{R167_WINDOW_RIGHT}{b})"),
        );
    assert_ne!(out, body, "窗口改写没有落地（锚点漂移了）");
    out
}

/// 把左侧省略号判据改写成给定值。
fn r167_set_guard_left(body: &str, value: i64) -> String {
    let cur =
        r167_int_after(body, R167_GUARD_LEFT).unwrap_or_else(|| panic!("给定体里读不出左侧判据"));
    let out = body.replace(
        &format!("{R167_GUARD_LEFT}{cur})"),
        &format!("{R167_GUARD_LEFT}{value})"),
    );
    assert_ne!(out, body, "左侧判据改写没有落地（锚点漂移了）");
    out
}

/// 把右侧省略号判据改写成给定值。
fn r167_set_guard_right(body: &str, value: i64) -> String {
    let cur =
        r167_int_after(body, R167_GUARD_RIGHT).unwrap_or_else(|| panic!("给定体里读不出右侧判据"));
    let out = body.replace(
        &format!("{R167_GUARD_RIGHT}{cur})"),
        &format!("{R167_GUARD_RIGHT}{value})"),
    );
    assert_ne!(out, body, "右侧判据改写没有落地（锚点漂移了）");
    out
}

/// 把 `app` 的分页器换成**修复体**。
fn r167_variant_fix(app: &str) -> String {
    r167_with_body(app, R167_FIXED_BODY)
}

/// 今日的缺陷形状：窗口不动，两个判据各**紧一格**（`A + 3` / `B + 2`）。
///
/// ⚠️ 这两个数字是**推导**出来的（`A` / `B` 从修复体读出），不是抄的 `4` / `3` —— 否则修复体
/// 一漂移，变体就悄悄变成另一件事（#479 同族）。
fn r167_variant_unfixed_body() -> String {
    let (a, b) = r167_fixed_half_widths();
    let body = r167_set_guard_left(R167_FIXED_BODY, a + 3);
    r167_set_guard_right(&body, b + 2)
}

/// 缺陷形状的函数体装进给定的树。
fn r167_variant_unfixed(app: &str) -> String {
    r167_with_body(app, &r167_variant_unfixed_body())
}

/// 探针的 `m_left_only`：只修了左边半句（右判据仍紧一格）。
fn r167_variant_left_only(app: &str) -> String {
    let (_, b) = r167_fixed_half_widths();
    r167_with_body(app, &r167_set_guard_right(R167_FIXED_BODY, b + 2))
}

/// 只修了右边半句（左判据仍紧一格）—— 左边那半个缺口的单向对照。
fn r167_variant_right_only(app: &str) -> String {
    let (a, _) = r167_fixed_half_widths();
    r167_with_body(app, &r167_set_guard_left(R167_FIXED_BODY, a + 3))
}

/// 探针的 `m_showall`：干脆全画出来 —— 窗口与判据一起消失。
fn r167_variant_show_all(app: &str) -> String {
    r167_with_body(
        app,
        "    const out = [];\n    for (let i = 1; i <= pages; i++) out.push(i);\n    return out;",
    )
}

/// 「紧凑形状」被拆掉（少了 `pages <= 9` 全量渲染那一支）。
fn r167_variant_small_gone(app: &str) -> String {
    let lines: Vec<&str> = R167_FIXED_BODY
        .lines()
        .filter(|l| !l.contains(R167_SMALL_BRANCH))
        .collect();
    let body = lines.join("\n");
    assert!(
        !body.contains(R167_SMALL_BRANCH),
        "`{R167_SMALL_BRANCH}` 那一支没被摘掉（锚点漂移了）"
    );
    assert_ne!(
        body,
        R167_FIXED_BODY.trim_end_matches('\n'),
        "摘掉那一支之后函数体没变 —— 变体与修复体是同一棵树"
    );
    r167_with_body(app, &body)
}

/// **自洽但更宽**的窗口：`A = B = a + 1`，判据按同一推导给出 ⇒ 本门禁**接受**。
///
/// 拒它的是 jsdom 探针的形状腿 C2（`pages > 9` 时 token 数必须 ≤ 7）—— 本门禁如实申报的射程边界。
fn r167_variant_wide_window(app: &str) -> String {
    let (a, b) = r167_fixed_half_widths();
    let body = r167_set_window(R167_FIXED_BODY, a + 1, b + 1);
    let body = r167_set_guard_left(&body, a + 3);
    let body = r167_set_guard_right(&body, b + 2);
    r167_with_body(app, &body)
}

/// 探针的 `m_mark_only`：序列不动，只补一句解释性注释 ⇒ **注释不参与**，
/// 判词必须与缺陷形状逐条相同（否则「加句解释」就能把缺陷说成修好了）。
fn r167_variant_mark_only(app: &str) -> String {
    let body = r167_variant_unfixed_body();
    let marked = body.replace(
        "\n    out.push(1);",
        "\n    out.push(1);\n    // 解释性注释：这里的省略号应当收拢窗口（说得对，但序列没变）",
    );
    assert_ne!(marked, body, "注释没有插进去（锚点漂移了）");
    r167_with_body(app, &marked)
}

// ── R168：快捷键面板宣传的 Esc 契约（`help.k3` = zh「关闭 / 取消」/ en「Close / cancel」）必须
// 真的覆盖 `ui/index.html` 里**每一个可关闭的浮层** ─────────────────────────────────────────────
//
// `ui/index.html` 的快捷键面板第 3 行把 Esc 写成**角色无关、视图无关**的「关闭 / 取消」契约。
// 全局 keydown 处理器（`ui/js/app.js`）里的 Esc 分支按顺序处理：引导 → 帮助 → 行内新建 Key →
// （表格高亮），而 `#chat-modal`（消费对话框，`class="modal-overlay hidden"`）**不在任何一支里**。
//
// 为什么这会真的坏掉：`openChat()` 把焦点放进 `#chat-input` ⇒ 事件目标是 `<input>` ⇒ 紧随其后的
// `if (typing || e.metaKey || …) return;` 会**先把它吞掉**；任何排在那一行之后的浮层分支都不可达。
// `grep -n "Escape" ui/js/app.js` 命中 10 处，`#chat-modal` 零命中。
//
// 三条规则都从代码里**派生**（#469：门禁不许把这一次编辑的字面量写死）。唯一写死的字面量是变体的
// 绿基线 `R168_FIXED_GUARD` —— 与 R167 的 `R167_FIXED_BODY` 同型：它只出现在**自己拼出来的树**里，
// 且由 `r168_compile_gate.py` 断言它确实是编辑表 E1 产物的子串（跨制品对账，#548：导入制品取值，
// 绝不重新抄一遍）。
//
// ⚠️ 关于派生集合：C2171 的浮层集合里还包含 `tour-ring` / `tour-pop` —— 它们是**引导的零件**而不是
// 独立浮层。本门禁的 R1 **不需要**它们各自有自己的 Esc 分支：`closeTour()` 一次把它们全关掉，而
// R1 判的是「有没有某条 Esc 分支（含其传递闭包）会隐藏它」。这正是原样的 C2171 派生能在这里成立
// 的原因（编辑表 §3 把这一点记为**风险**，本轮把它变成实测）。

/// 全局 keydown 处理器的开头（`ui/js/app.js`）。只此一处。
const R168_BLOCK_START: &str = "document.addEventListener(\"keydown\", (e) => {";
/// 该处理器的收尾（模块缩进两格的回调结束 + `);`）。块内任何一行都不会包含它。
const R168_BLOCK_END: &str = "\n    });\n";
/// `typing` 守卫的判据锚 —— R3 的「封锁线」。
const R168_TYPING: &str = "if (typing || e.metaKey";
/// Esc 分支的判据锚。
const R168_ESCAPE: &str = "e.key === \"Escape\"";
/// **class** 机制的读法（`$("#x").classList.contains("hidden")`）—— 判别式用到的唯一机制词。
const R168_CLASS_PROBE: &str = ".classList.contains(\"hidden\")";
/// **property** 机制的读法（`$("#x").hidden`）。
const R168_PROPERTY_PROBE: &str = "\").hidden";

/// 行内新建 Key 的守卫行 —— 修复体的**插入锚点**（逐字来自 `ui/js/app.js`，由编译门禁对账）。
const R168_FIXED_ANCHOR: &str = concat!(
    "      if (e.key === \"Escape\" && !$(\"#ak-new-inline\").hidden) ",
    "{ closeNewKeyInline(); return; }",
);

/// 修复后的守卫行 —— 变体树的**绿基线**（逐字来自编辑表 E1 的新文本，由编译门禁断言子串关系）。
const R168_FIXED_GUARD: &str = concat!(
    "      if (e.key === \"Escape\" && !$(\"#chat-modal\").classList.contains(\"hidden\")) ",
    "{ closeChat(); return; }",
);

/// 一次扫描同时产出三条规则的判决**与它们的证据**（逐条可打印 —— #339/#341：判词与取值两列）。
struct R168Reading {
    block_lines: usize,
    guards: usize,
    derived: BTreeSet<String>,
    hidden_anywhere: BTreeSet<String>,
    hidden_before_bail: BTreeSet<String>,
    missing: BTreeSet<String>,
    probes: BTreeMap<String, BTreeSet<String>>,
    expectations: BTreeMap<String, String>,
    mismatches: Vec<String>,
    typing_at: Option<usize>,
    r1: bool,
    r2: bool,
    r3: bool,
}

impl R168Reading {
    fn verdicts(&self) -> (bool, bool, bool) {
        (self.r1, self.r2, self.r3)
    }

    /// 把已经读到的证据折成三条判词（单独一步，好让 `r168_read` 的构造函数保持一行一句）。
    ///
    /// `R2` 的空集保护在**同一条**判词里：读法一个都没有（或链上无唯一写法）时它必须红，
    /// 而不是在空集上恒真（坑 68 家族；牙齿测试有一条专门喂空期望值）。
    fn finish(mut self) -> R168Reading {
        self.r1 = self.missing.is_empty();
        self.r2 =
            self.mismatches.is_empty() && !self.probes.is_empty() && !self.expectations.is_empty();
        self.r3 = self.hidden_anywhere == self.hidden_before_bail;
        self
    }

    fn report(&self) -> String {
        format!(
            "r1={} r2={} r3={} | lines={} guards={} typing_at={:?} derived={:?} hidden={:?} before_bail={:?} missing={:?} probes={:?} expects={:?} mismatches={:?}",
            self.r1,
            self.r2,
            self.r3,
            self.block_lines,
            self.guards,
            self.typing_at,
            self.derived,
            self.hidden_anywhere,
            self.hidden_before_bail,
            self.missing,
            self.probes,
            self.expectations,
            self.mismatches,
        )
    }
}

/// 全局 keydown 块的**代码行**（已剥注释行）。
///
/// 块的**开头**必须全文件唯一（元素级的 `…addEventListener("keydown", …)` 都不带 `document.`）。
/// 收尾定界（模块缩进两格的回调结束 + `);`）在整份文件里出现很多次（51 次），所以**不**要求它全局
/// 唯一 —— 取开头之后的**第一处**即为本处理器的收尾：块内每一行都比它缩进更深，不可能提前命中。
/// 开头不唯一（或找不到收尾）时返回 `None`（由规则把它变成响亮失败，而不是在空串上「通过」：坑 68）。
fn r168_block(app: &str) -> Option<String> {
    if app.matches(R168_BLOCK_START).count() != 1 {
        return None;
    }
    let at = app.find(R168_BLOCK_START)? + R168_BLOCK_START.len();
    let end = at + app[at..].find(R168_BLOCK_END)?;
    Some(code_lines(&app[at..end]))
}

/// 一行守卫**读**的是哪个隐藏机制（`class` / `property`）；两者都没有 ⇒ `None`（这一行不是在读开合）。
fn r168_guard_mechanism(line: &str) -> Option<&'static str> {
    if line.contains(R168_CLASS_PROBE) {
        return Some("class");
    }
    if line.contains(R168_PROPERTY_PROBE) {
        return Some("property");
    }
    None
}

/// 一个**关闭器**的函数体**写**的是哪个隐藏机制（`class` / `property`）；都认不出 ⇒ `None`
/// （响亮地不作为期望值，而不是猜一个）。
fn r168_closer_mechanism(code: &str) -> Option<&'static str> {
    let adds = code.contains(".classList.add(\"hidden\")");
    let toggles = code.contains(".classList.toggle(\"hidden\"");
    if adds || toggles {
        return Some("class");
    }
    if code.contains("\").hidden = true") {
        return Some("property");
    }
    None
}

/// 从给定函数名集合出发的传递调用闭包。
///
/// ⚠️ **自建**闭包、**不复用** `call_graph` / `reachable`：那两个的每条边都由 `js_function_body`
/// 取体，而后者对**单行**函数会一路吞到下一个 `  }`（C2171 坑 #319/#332）。这里与 `overlay_closure`
/// 同法：用 `function_source`（单行安全），箭头常量没有 `function` 声明头 ⇒ 返回 `None` ⇒ 不参与。
fn r168_chain(app: &str, roots: &BTreeSet<String>) -> BTreeSet<String> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut queue: Vec<String> = roots.iter().cloned().collect();
    while let Some(f) = queue.pop() {
        if !seen.insert(f.clone()) {
            continue;
        }
        let Some(body) = function_source(app, &f) else {
            continue;
        };
        let code = code_lines(&body);
        for c in callee_names(&code) {
            if !seen.contains(&c) {
                queue.push(c);
            }
        }
    }
    seen
}

// ── 变体构造器：每个变体**只此一处**定义，牙齿测试与鉴别力测试共用（#325 同族）────────────
//
// 全部从 `R168_FIXED_GUARD` / `R168_FIXED_ANCHOR` 派生 —— 与「这棵树是未修还是已修」无关，
// 两腿跑同一套断言。

/// 在**包含 `needle` 的那一行**之后插入一行 `text`（插入点由该行自己的换行决定 ⇒ 不钉整行字面量）。
fn r168_insert_after_line(src: &str, needle: &str, text: &str) -> Option<String> {
    if src.matches(needle).count() != 1 {
        return None;
    }
    let at = src.find(needle)?;
    let eol = at + src[at..].find('\n')?;
    Some(format!("{}\n{}{}", &src[..eol], text, &src[eol..]))
}

/// 删掉**包含 `needle` 的那一整行**。
fn r168_remove_line(src: &str, needle: &str) -> Option<String> {
    if src.matches(needle).count() != 1 {
        return None;
    }
    let at = src.find(needle)?;
    let start = src[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = at + src[at..].find('\n')? + 1;
    Some(format!("{}{}", &src[..start], &src[end..]))
}

/// 把给定守卫行装进树里（先回到**未修**形状再插，保证两腿得到同一棵树 —— 幂等）。
fn r168_variant_with_guard(app: &str, guard: &str) -> String {
    let unfixed = r168_variant_unfixed(app);
    let out = r168_insert_after_line(&unfixed, R168_FIXED_ANCHOR, guard)
        .unwrap_or_else(|| panic!("插入锚点 `{R168_FIXED_ANCHOR}` 不是恰好一行 —— 变体无从构造"));
    assert_ne!(out, unfixed, "守卫没有插进去（锚点漂移了）");
    out
}

/// 修复体：唯一那棵判词全绿的树。
fn r168_variant_fix(app: &str) -> String {
    r168_variant_with_guard(app, R168_FIXED_GUARD)
}

/// 未修形状：把守卫行整行摘掉（`R1` 该因此翻红）。
fn r168_variant_unfixed(app: &str) -> String {
    if app.matches(R168_FIXED_GUARD).count() == 1 {
        if let Some(out) = r168_remove_line(app, R168_FIXED_GUARD) {
            assert_ne!(out, app, "摘掉守卫没有改动树");
            return out;
        }
    }
    app.to_string()
}

/// 竞争修法一（探针 `m_after_typing`）：守卫挂到了 `typing` 守卫**之后** ⇒ 从对话框自己的输入框
/// 按 Esc 会被吞掉（`R3` 该因此翻红）。
fn r168_variant_after_bail(app: &str) -> String {
    let detached = r168_remove_line(&r168_variant_fix(app), R168_FIXED_GUARD)
        .unwrap_or_else(|| panic!("修好的树里找不到守卫行 —— 变体无从构造"));
    let out = r168_insert_after_line(&detached, R168_TYPING, R168_FIXED_GUARD)
        .unwrap_or_else(|| panic!("`typing` 守卫行不是恰好一行 —— 变体无从构造"));
    assert_ne!(out, detached, "守卫没有搬到封锁线之后");
    out
}

/// 竞争修法二（探针 `m_property_guard`）：把探针写成**属性**形式 ⇒ 与 `closeChat` 的 class 机制不同源、
/// 守卫恒真（`R2` 该因此翻红）。改写由判别式自己的机制词完成，不写死那半个表达式。
fn r168_property_guard() -> String {
    assert!(
        R168_FIXED_GUARD.matches(R168_CLASS_PROBE).count() == 1,
        "修复体里的机制词不是恰好一个 —— 变体的改写会落错地方"
    );
    let out = R168_FIXED_GUARD.replace(R168_CLASS_PROBE, ".hidden");
    assert_ne!(out, R168_FIXED_GUARD, "属性形式没有替换进去");
    out
}

/// 竞争修法二装进树。
fn r168_variant_property(app: &str) -> String {
    r168_variant_with_guard(app, &r168_property_guard())
}

/// 只加一句**解释性注释**（说得对，但分支没加）⇒ **注释不参与**，判词必须与未修形状逐条相同。
fn r168_variant_mark_only(app: &str) -> String {
    let unfixed = r168_variant_unfixed(app);
    let marked = r168_insert_after_line(
        &unfixed,
        R168_FIXED_ANCHOR,
        "      // 消费对话框也应当在 Esc 上关闭（说得对 —— 但这一行是注释，不是分支）",
    )
    .unwrap_or_else(|| panic!("注释锚点 `{R168_FIXED_ANCHOR}` 漂移了"));
    assert_ne!(marked, unfixed, "注释没有插进去");
    marked
}

//
// R93 轴：行内字段错误的文案必须**描述触发它的条件**，且同一条规则在客户端与服务端必须用
// **同一把嗓子**。
//
// 缺陷（`ui/js/app.js` 的找回密码流程）：守卫是「口令短于下限」，文案却是**空**那句话
// （`register.err.pass` =「请输入密码」）—— 用户刚输了一串密码，红字让他「请输入密码」。
// 姊妹表单（注册）用同一把嗓子说**同一个条件**（`if (!pw)`）是对的 ⇒ 错的是**条件与文案的
// 对应**，不是这把嗓子本身；`err.weakPassword`（＝服务端 `密码至少 8 位` / `新密码至少 8 位`
// 经 `ERR_MAP` 得到的那句）早就在包里，作者写下那一行时它就在**同一个 diff** 里 ⇒ 判为漂移。
//
// 三条规则（各有独立的牙）：
//
// 1. **A-lower（存在性）**：至少有一个守卫里比较 `.length` 的行内字段错误位点 —— 否则
//    「每个这样的位点都用 K」是一句**空转**的全称命题（坑 #324）。
// 2. **A-upper（全称）**：每一个这样的位点都用 **K**。
// 3. **B（类不相交，全文件）**：把每个行内位点按**它自己的守卫**分类为 *空* 或 *非空*；
//    同一个键不得同时服务两类。
//
// **K 是推导出来的，不是写死的**：从 `src/routes/mod.rs` 里**实现这条规则的那个 helper**
// （其体把**字符数**比一个具名常量 ⇒ 由体认定，不按名字认定；R96 把 `(new_)password.len() < 8`
// 换成 `password_too_short()` 时，一切转录了旧拼写的仪器都静默失效）出发，取 enforcing 分支里
// 返回的中文原话，按 `I18n.mapErr` 的**最长子串**规则过 `ERR_MAP`，要求**恰好解析到一个键**。
// 这样门禁抓的是「后端说了什么」，而不是「这次编辑写下了什么」（坑 #469 / #537）。
//
// **射程（诚实边界，已写进 `ui/README.md`）**：本门禁是**词法**的 —— 它证「那个位点用的**键**
// 与规则同源」，**不证**运行期那一刻屏幕上真的出现了那句话、也**不证**两条消息的**值**（字符串）
// 相等。值与运行期归 jsdom 探针（`r93_probe.js`：驱动真表单、逐语言取参照物）与
// `src/routes/mod.rs` 的口令边界测试 —— **形状归门禁，事实归探针**。
//
// 语料：`ui/js/app.js` 复用 `APP_JS`，语言包复用 `I18N_JS`，服务端真源**复用** R78（口令下限）
// 已经引入的 `PASS_MIN_MOD_RS` —— 本轴问的是**同一条规则**「另一个方向」的问题（不是「它怎么
// 数」，而是「客户端的行内文案有没有说这句话」），所以真源只能是同一份文件、同一个 helper。
// ⛔ 不要再 `include_str!` 一次同一个文件。
//
// ⚠️ 这里**不**留「后端今天回哪句原话」的副本：原话是**派生**出来的（见 `r93_branches`），
// 抄一份在这里就会在一行之隔自相矛盾（C2024 的规矩）。

/// 变异体用的**无关键**（A-upper / B 各自的牙）：刻意不是包里的任何一个键 ——
/// 判词与「它在不在包里」无关（那是对**派生**键 K 的检查）。
const R93_MUT_KEY: &str = "r93.mut.only";

/// 位点的**未修**形状：`ui/js/app.js` 里 `#forgot-pass` 长度守卫那一段（逐字来自编辑表 E1 的
/// `old` 文本，由编译门禁对账）。
const R93_SITE_UNFIXED: &str =
    "T(\"register.err.pass\")); firstErr = firstErr || $(\"#forgot-pass\");";

/// 位点的**修复**形状（编辑表 E1 的 `new` 文本）。只差**键那一个 token** ——
/// `the_r93_site_shapes_are_the_edit_sheet_text` 钉住这件事。
const R93_SITE_FIXED: &str =
    "T(\"err.weakPassword\")); firstErr = firstErr || $(\"#forgot-pass\");";

/// 一个行内字段错误位点：`if (<cond>) setFieldError(<field>, T("<key>"))`。
///
/// `line` 是**磁盘上的行号**（剥注释时块注释换成等量换行，见 `r93_code_lines` —— 报告里的行号
/// 是要被人 `sed -n` 复核的，坑 #532）。
struct R93Site {
    line: usize,
    field: String,
    key: String,
    cond: String,
    /// 守卫说的是「这个字段是**空**的」吗（形状判定，见 `r93_empty_guard`）。
    empty_guard: bool,
}

/// R93 的剥注释扫描器：`//` 行、**行内** `//` 之后的文本、`/* … */`（含跨行块）。
///
/// 与 `code_text_by_line` 的分工：那个只管整行注释与成对块片段（它服务的判别式不看行内 `//`
/// 之后的东西），而本轴的位点是**行内**写的 —— 一句 `x = 1; // setFieldError(...)` 在它那里
/// 会造出一个幻影位点（`the_r93_readers_are_real` 的 C4 腿钉的就是这件事）。
///
/// ⚠️ 块注释**保住换行**（这里＝「这一行留空」）：行号由返回向量的下标 + 1 给出，必须与磁盘一致
/// （坑 #532）。
fn r93_code_lines(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in src.lines() {
        let mut rest = line;
        if in_block {
            match rest.find("*/") {
                Some(i) => {
                    rest = &rest[i + 2..];
                    in_block = false;
                }
                None => {
                    out.push(String::new());
                    continue;
                }
            }
        }
        let (code, opened) = strip_inline_blocks(rest);
        if opened {
            in_block = true;
        }
        let code = match code.find("//") {
            Some(i) => code[..i].to_string(),
            None => code,
        };
        out.push(code.trim().to_string());
    }
    out
}

/// 语料的前 `n` 个**字符**（按字节切 `&str` 会 panic —— 这是必须显式处理的切片规约）。
fn r93_window(src: &str, chars: usize) -> &str {
    match src.char_indices().nth(chars) {
        Some((i, _)) => &src[..i],
        None => src,
    }
}

/// Rust 的 `\`+换行续行先合上（坑 #301：不合并的话，一个跨行写下的字面量会被静默漏掉）。
fn r93_join_continuations(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() && (chars[i + 1] == '\n' || chars[i + 1] == '\r')
        {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            i = j;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// 找一个**独立关键字** `kw` 的位置：前面不是标识符字符、后面（允许空白）跟 `(`。
fn r93_kw_at(text: &str, kw: &str, from: usize) -> Option<usize> {
    let mut at = from;
    while let Some(i) = text[at..].find(kw) {
        let p = at + i;
        let before_ok = match text[..p].chars().next_back() {
            None => true,
            Some(c) => !(c.is_alphanumeric() || c == '_' || c == '$' || c == '.'),
        };
        let after_ok = text[p + kw.len()..].trim_start().starts_with('(');
        if before_ok && after_ok {
            return Some(p);
        }
        at = p + kw.len();
    }
    None
}

/// 从 `open`（`(` 的字节位置）取出**配对**的括号内容（不含两端括号）。
fn r93_paren(text: &str, open: usize) -> Option<String> {
    let mut depth = 0usize;
    let mut out = String::new();
    for c in text[open..].chars() {
        match c {
            '(' => {
                depth += 1;
                if depth == 1 {
                    continue;
                }
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(out);
                }
            }
            _ => {}
        }
        out.push(c);
    }
    None
}

/// 一个「标识符路径」（`a` / `a.b` / `a.b.c`），不含运算符与空白。
fn r93_is_path(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '$' || c == '.')
}

/// 守卫属于哪一类：`true` =「这个字段是**空**的」；`false` = 其他（长度/相等/业务…）。
///
/// 两类认法是**形状**的，不是内容的：`!<路径>`（`!pw` / `!$("#login-email")`）与
/// `<路径> === ""`（含 `.trim()`）。认不出的一律算「其他」—— 这一侧保守是有意的：漏判只会让
/// B 更**宽**（更难变红），不会凭空造出一类。
fn r93_empty_guard(cond: &str) -> bool {
    let c = cond.trim();
    if let Some(rest) = c.strip_prefix('!') {
        let r = rest.trim();
        if r.starts_with("$(") || r93_is_path(r) {
            return true;
        }
    }
    let Some(head) = c.strip_suffix("\"\"") else {
        return false;
    };
    let h = head.trim().trim_end_matches('=').trim();
    let h = h.strip_suffix(".trim()").unwrap_or(h).trim();
    r93_is_path(h)
}

/// 守卫是否把某个 `.length` 与一个界限比较（`<` / `>`）。
fn r93_bounds_length(cond: &str) -> bool {
    let mut rest = cond;
    while let Some(i) = rest.find(".length") {
        let after = rest[i + ".length".len()..].trim_start();
        if after.starts_with('<') || after.starts_with('>') {
            return true;
        }
        rest = &rest[i + 1..];
    }
    false
}

/// 实现这条规则的那个 helper：**从它的体里**认（`.chars().count() < <具名常量>`），不按名字认。
/// 候选不是**恰好一个** ⇒ `None`（响亮失败，绝不猜一个）。
fn r93_helper(rust: &str) -> Option<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    let mut from = 0usize;
    while let Some(i) = rust[from..].find("-> bool {") {
        let head_end = from + i;
        let body_start = head_end + "-> bool {".len();
        if let Some(f) = rust[..head_end].rfind("fn ") {
            let name: String = rust[f + 3..head_end]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let rest = &rust[body_start..];
            let body = match rest.find("\n}") {
                Some(e) => &rest[..e],
                None => rest,
            };
            if !name.is_empty() && body.contains(".chars().count() <") {
                found.push((name, body.to_string()));
            }
        }
        from = body_start;
    }
    if found.len() == 1 {
        found.pop()
    } else {
        None
    }
}

/// 分支里 `"error": "<原话>"` 的原话。同一个臂里**必须**有；没有 ⇒ `None`（由规则响亮失败，
/// 而不是把这条分支静默跳过 —— 静默跳过会让「每条分支都被检查过」变成一句空话）。
fn r93_error_literal(win: &str) -> Option<String> {
    let at = win.find("\"error\"")?;
    let rest = &win[at..];
    let colon = rest.find(':')?;
    let rest = rest[colon + 1..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// 强制这条规则的分支：`(被限定的参数, 那句原话)`，按出现顺序。任何一条分支读不出原话 ⇒ `None`。
fn r93_branches(rust: &str, helper: &str) -> Option<Vec<(String, String)>> {
    let needle = format!("if {helper}(");
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(i) = rust[from..].find(&needle) {
        let at = from + i + needle.len();
        from = at;
        // 窗口在**字符**上取（按字节切会 panic），且刻意宽松：一个懒惰窗口会停在实参表之后、
        // 把分支自己的原话留在窗外（两个仪器犯同一个错，就是它们互相漂开的路径）。
        let win = r93_window(&rust[at..], 400);
        let arg_end = win.find(')')?;
        let arg: String = win[..arg_end]
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '&')
            .collect();
        let lit = r93_error_literal(win)?;
        out.push((arg, lit));
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// 一段文本里的双引号字面量（本词表的字面量自身不含 `"`；否则这条解析必须先声明）。
fn r93_string_literals(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('"') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('"') else { break };
        out.push(after[..close].to_string());
        rest = &after[close + 1..];
    }
    out
}

/// `ERR_MAP` 的 `(原话, 键)` 条目 —— 从语言包本身解析，**不写死任何一条**。
fn r93_err_map(i18n: &str) -> Vec<(String, String)> {
    let Some(start) = i18n.find("var ERR_MAP = [") else {
        return Vec::new();
    };
    let tail = &i18n[start..];
    let end = tail.find("\n  ];").unwrap_or(tail.len());
    let mut out = Vec::new();
    for line in tail[..end].lines() {
        let l = line.trim();
        let Some(rest) = l.strip_prefix('[') else {
            continue;
        };
        let body = rest.strip_suffix(',').unwrap_or(rest);
        let Some(inner) = body.strip_suffix(']') else {
            continue;
        };
        let toks = r93_string_literals(inner);
        if toks.len() == 2 {
            out.push((toks[0].clone(), toks[1].clone()));
        }
    }
    out
}

/// `I18n.mapErr` 的规则：**最长**匹配的原话胜出（`msg.indexOf(lit) !== -1` 且最长）。
/// 长度比较用**字符数** —— 与 JS 的 UTF-16 长度在这张全中文词表上逐条一致。
fn r93_resolve(msg: &str, pairs: &[(String, String)]) -> Option<String> {
    let mut best: Option<(&str, usize)> = None;
    for (lit, key) in pairs {
        if msg.contains(lit.as_str()) {
            let n = lit.chars().count();
            if best.map(|(_, b)| n > b).unwrap_or(true) {
                best = Some((key.as_str(), n));
            }
        }
    }
    best.map(|(k, _)| k.to_string())
}

/// A-upper（全称那条规则）：每一个「守卫里比较 `.length`」的位点都必须用**推导出来的键** `key`。
///
/// ⚠️ 单独成函数、且**调用点写作一行**：编译门禁的阴性对照要能**一行**拆掉这条规则
/// （`let a_upper = true || r93_all_upper(...)`），从而证明「轴测试在 base 腿上红**是因为**
/// 这条规则」—— 那是这类门禁最容易搞错的方向（拆掉规则会让未修树**通过**轴测试）。
fn r93_all_upper(sites: &[R93Site], key: Option<&str>) -> bool {
    sites
        .iter()
        .filter(|s| r93_bounds_length(&s.cond))
        .all(|s| key == Some(s.key.as_str()))
}

/// 一次扫出 `ui/js/app.js` 的**行内**字段错误位点，以及**没有**守卫的那些（射程之外，单独
/// 计数：它们不属于「按守卫分类」，静默丢掉会让「全文件」成为一句空话）。
fn r93_sites(app: &str) -> (Vec<R93Site>, usize) {
    let mut sites = Vec::new();
    let mut unguarded = 0usize;
    for (i, raw) in r93_code_lines(app).iter().enumerate() {
        if raw.starts_with("function setFieldError(") {
            continue;
        }
        let Some(at) = raw.find("setFieldError(") else {
            continue;
        };
        let head = &raw[..at];
        let rest = &raw[at + "setFieldError(".len()..];
        let Some(comma) = rest.find(',') else {
            continue;
        };
        let field = rest[..comma].trim().to_string();
        let after = rest[comma + 1..].trim_start();
        let Some(after) = after.strip_prefix("T(\"") else {
            continue;
        };
        let Some(q) = after.find('"') else { continue };
        let key = after[..q].to_string();
        let Some(if_at) = r93_kw_at(head, "if", 0) else {
            unguarded += 1;
            continue;
        };
        let Some(open) = head[if_at..].find('(').map(|o| if_at + o) else {
            unguarded += 1;
            continue;
        };
        let Some(cond) = r93_paren(head, open) else {
            unguarded += 1;
            continue;
        };
        let cond = cond.trim().to_string();
        let empty_guard = r93_empty_guard(&cond);
        sites.push(R93Site {
            line: i + 1,
            field,
            key,
            cond,
            empty_guard,
        });
    }
    (sites, unguarded)
}

/// 一次读出的三条规则**与它们的证据**（#339/#341：判词与取值两列，红的时候要能印出「为什么」）。
struct R93Reading {
    key: Option<String>,
    branches: usize,
    by_arg: BTreeMap<String, BTreeSet<String>>,
    literals: BTreeSet<String>,
    /// 后端 enforcing 分支里**在 `ERR_MAP` 里没有落点**的原话 —— 推不出来的那些必须被数出来
    /// （静默丢掉它们会让「同一条规则一把嗓子」变成「能解析的那几条恰好同声」）。
    unresolved: BTreeSet<String>,
    sites: Vec<R93Site>,
    unguarded: usize,
    shared_keys: BTreeSet<String>,
    a_lower: bool,
    a_upper: bool,
    b: bool,
}

impl R93Reading {
    fn new(app: &str, rust: &str, i18n: &str) -> Self {
        let rust = r93_join_continuations(pass_min_prod(rust));
        let helper = r93_helper(&rust);
        let branches = helper
            .as_ref()
            .and_then(|(name, _)| r93_branches(&rust, name));
        let mut by_arg: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut literals: BTreeSet<String> = BTreeSet::new();
        for (arg, lit) in branches.iter().flatten() {
            by_arg.entry(arg.clone()).or_default().insert(lit.clone());
            literals.insert(lit.clone());
        }
        let pairs = r93_err_map(i18n);
        let resolved: BTreeMap<&String, String> = literals
            .iter()
            .filter_map(|l| r93_resolve(l, &pairs).map(|k| (l, k)))
            .collect();
        let unresolved: BTreeSet<String> = literals
            .iter()
            .filter(|l| !resolved.contains_key(l))
            .cloned()
            .collect();
        let keys: BTreeSet<&String> = resolved.values().collect();
        let key = if keys.len() == 1 {
            keys.into_iter().next().cloned()
        } else {
            None
        };

        let (sites, unguarded) = r93_sites(app);
        let mut empty_keys: BTreeSet<String> = BTreeSet::new();
        let mut other_keys: BTreeSet<String> = BTreeSet::new();
        for s in &sites {
            if s.empty_guard {
                empty_keys.insert(s.key.clone());
            } else {
                other_keys.insert(s.key.clone());
            }
        }
        let a_lower = sites.iter().any(|s| r93_bounds_length(&s.cond));
        let a_upper = r93_all_upper(&sites, key.as_deref());
        let shared_keys: BTreeSet<String> = empty_keys.intersection(&other_keys).cloned().collect();

        R93Reading {
            key,
            branches: branches.map(|b| b.len()).unwrap_or(0),
            by_arg,
            literals,
            unresolved,
            sites,
            unguarded,
            b: shared_keys.is_empty(),
            shared_keys,
            a_lower,
            a_upper,
        }
    }

    fn verdicts(&self) -> (bool, bool, bool) {
        (self.a_lower, self.a_upper, self.b)
    }

    fn length_sites(&self) -> Vec<&R93Site> {
        self.sites
            .iter()
            .filter(|s| r93_bounds_length(&s.cond))
            .collect()
    }

    fn report(&self) -> String {
        let sites: Vec<String> = self
            .sites
            .iter()
            .map(|s| {
                format!(
                    "L{} {} {} {}",
                    s.line,
                    s.field,
                    if s.empty_guard { "empty" } else { "other" },
                    s.key
                )
            })
            .collect();
        format!(
            "key={:?} branches={} literals={:?} unresolved={:?} by_arg={:?} length_sites={:?} shared={:?} sites={} unguarded={} verdicts={:?} {}",
            self.key,
            self.branches,
            self.literals,
            self.unresolved,
            self.by_arg,
            self.length_sites()
                .iter()
                .map(|s| (s.line, s.field.clone(), s.key.clone()))
                .collect::<Vec<_>>(),
            self.shared_keys,
            sites.len(),
            self.unguarded,
            self.verdicts(),
            sites.join(" | ")
        )
    }
}

// ── R94：`atp:langchange` 处理器的刷新名册必须覆盖「用户可能正开着的每个浮层」 ────────────────────
//
// 题眼：`ui/js/app.js` 的 `atp:langchange` 处理器（#86 装）刷新 `{nav, 数据表表头, 当前视图, 标题,
// 引导}` —— 而 `ui/index.html` 里处境**完全相同**的浮层 `#help-panel` 无人刷新。面板标题与关闭按钮是
// `[data-i18n]`（被 `applyStatic()` 换掉），四行 `HELP_KEYS → T(d)` 与底部 `#help-context` 却是
// `renderHelp()` 用 JS 建的、**没有钩子**，且 `renderHelp()` 全仓唯一调用点是 `toggleHelp` 的**打开**
// 分支 ⇒ 同一块面板两种语言，只有关掉再开才重画（且整会话不自愈）。它是**非模态**浮动面板：
// `ui/css/style.css` 的 `.help-panel` 是 `position: fixed` 且没有 `inset: 0`（不盖住设置页的语言
// 下拉）⇒ 用户真的能在它开着时去切语言。同一提交里作者既给 `renderHelp` 加了 `T(...)`、又给这个
// 处理器加了引导刷新 ⇒ 看过这个渲染器、只漏了这一块（漂移，不是取舍）。
//
// 三条规则（都从代码**派生**，#469：门禁不许把这一次编辑的字面量写死）：
//
//   R1  每个「**非模态**（按 CSS 推导：该元素的类规则里都没有 `inset: 0`）＋ 内容由 **JS 写**
//       （某函数体里同时出现该浮层的后代 id 与 `T(`）」的浮层，其**每一个**这样的写者都必须从
//       处理器体内被调用。
//   R2  处理器不得把刷新清空（挡「把名册删掉」式假修）：派生出来的 `render*` 刷新名 ≥ 3。
//   R3  **分界为真**：派生集合里确实既有模态（类规则含 `inset: 0`）又有非模态 —— 否则 R1 的
//       「非模态」这一半恒真/恒假，规则就退化成空集上的关系式（坑 68 家族）。
//
// 射程（如实）：本门禁是**词法**的。它证明「处理器的刷新名册覆盖了派生出来的每个非模态浮层的每个
// 写者」，**不**证明屏幕上那一刻的文案真的是当前语言（那一半归 jsdom 探针 `r94_probe.js` 的
// A1/A2/Z1 腿）。竞争修法「干脆把用户自己开的面板关掉」**两边都拒、但理由不同**：探针（A4）按
// DOM 实况拒它（面板被夺走），本门禁因为**写者没被调用**而拒它 —— 两个仪器各自能看见对方看不见的
// 东西（C2148 的分工：形状归门禁，事实归探针），这条边界实测于
// `the_r94_rules_separate_the_variants` 的声明表。

/// `ui/css/style.css`（编译期读入）：模态/非模态的判据是**类规则里有没有 `inset: 0`**，
/// 所以样式表必须是本门禁的输入，而不是一条手抄的豁免清单。
const STYLE_CSS: &str = include_str!("../ui/css/style.css");

/// `atp:langchange` 处理器的开头。全仓只此一处（`document.addEventListener(...)` 的元素级兄弟
/// 都不带这个事件名）。
const R94_HANDLER_START: &str = "addEventListener(\"atp:langchange\"";

/// 紧邻的、**已经被刷新**的兄弟行（引导）—— 修复体的插入锚点（逐字来自 `ui/js/app.js`）。
const R94_TOUR_GUARD: &str = "if (tourStep >= 0) renderTourStep();";

/// 修复体新增的调用（代码部分，不含行尾注释）—— 变体树的**绿基线**。
///
/// 与 R167 的 `R167_FIXED_BODY` / R168 的 `R168_FIXED_GUARD` 同型：它只出现在**自己拼出来的树**
/// 里，且由 `r94_compile_gate.py` 断言它确实是编辑表 E1 产物的子串（#548：导入制品，绝不重抄）。
const R94_FIXED_GUARD: &str = concat!(
    "if (!$(\"#help-panel\").classList.contains(\"hidden\")) ",
    "renderHelp();",
);

/// 竞争修法：切线时把用户自己开的面板**关掉**（屏幕上没有面板，就没有陈旧的行）。
/// 本门禁**也**拒它 —— 但理由与探针不同：探针（`A4` 腿）按 DOM 实况拒它（用户的面板被夺走了），
/// 本门禁拒它是因为**写者 `renderHelp` 没被调用**（R1 问的是「刷新了没」，不是「藏起来了没」）。
/// 两个仪器在这一点上同判、不同因 —— 列在这里是为了让这条射程边界**可测量**（实测见
/// `the_r94_rules_separate_the_variants` 的声明表），而不是只写在注释里。
const R94_CLOSE_GUARD: &str =
    "if (!$(\"#help-panel\").classList.contains(\"hidden\")) toggleHelp(false);";

/// 「模态」的判据：类规则里的整屏覆盖（挡住设置页的语言下拉 ⇒ 用户不可能在它开着时切语言）。
const R94_MODAL_RULE: &str = "inset: 0";

/// 通用工具类（`display: none` 的开关）：它说的是「现在藏着」，不是「这个浮层长什么样」，
/// 所以不参与模态/非模态的推导。
const R94_GENERIC_CLASS: &str = "hidden";

/// R2 的下界：处理器里派生出来的 `render*` 刷新名的个数。阈值只承担「不许清空」这一件事，
/// 真正承重的是 R1 的逐浮层关系。
const R94_MIN_REFRESHES: usize = 3;

/// 一个浮层：`ui/index.html` 里 `<body>` 顶层的元素（`#app` 之后、带 `hidden` 类）。
#[derive(Debug, Clone)]
struct R94Overlay {
    id: String,
    classes: Vec<String>,
    /// 浮层自己的 id ＋ 它内部出现的每个 `id="…"`（写者就是「往这些东西里写」的函数）。
    descendants: Vec<String>,
}

/// 从一行标签里取属性值（`id="…"` / `class="…"`）。
fn r94_attr(tag: &str, name: &str) -> Option<String> {
    let pat = format!("{name}=\"");
    let at = tag.find(&pat)? + pat.len();
    let rest = &tag[at..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// 顶层元素（列 0 起的 `<div id="…">`）的身体：单行自闭合的取该行，多行的取到**列 0 的 `</div>`**。
///
/// 只适用于顶层元素 —— 这正是本门禁的射程（浮层都是 `<body>` 顶层的兄弟）。
fn r94_overlay_span<'a>(html: &'a str, id: &str) -> Option<&'a str> {
    let pat = format!("<div id=\"{id}\"");
    let start = html.find(&pat)?;
    let rest = &html[start..];
    let line_end = rest.find('\n').unwrap_or(rest.len());
    if rest[..line_end].contains("</div>") {
        return Some(&rest[..line_end]);
    }
    let end = rest[line_end..].find("\n</div>")? + line_end;
    Some(&rest[..end])
}

/// 一段标记里出现的每个 `id="…"` 的值。
fn r94_ids_in(span: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = span[from..].find("id=\"") {
        let at = from + rel + 4;
        let Some(end) = span[at..].find('"') else {
            break;
        };
        out.push(span[at..at + end].to_string());
        from = at + end;
    }
    out
}

/// 浮层名册：`<body>` 里 `#app` **之后**、带 `hidden` **类**的顶层元素。
///
/// 零手写豁免清单 —— `#toast-wrap`（`<div id="toast-wrap"></div>`）自然落选，因为它的实例是**即时**
/// 生成的、没有 `hidden` 类；`#app` 自己是射程的起点而不是浮层。
fn r94_overlays(html: &str) -> Vec<R94Overlay> {
    let mut out = Vec::new();
    let mut seen_app = false;
    for line in html.lines() {
        if !line.starts_with("<div id=\"") {
            continue;
        }
        let Some(id) = r94_attr(line, "id") else {
            continue;
        };
        if id == "app" {
            seen_app = true;
            continue;
        }
        if !seen_app {
            continue;
        }
        let classes: Vec<String> = r94_attr(line, "class")
            .map(|c| c.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default();
        if !classes.iter().any(|c| c == R94_GENERIC_CLASS) {
            continue;
        }
        let mut descendants = r94_overlay_span(html, &id)
            .map(r94_ids_in)
            .unwrap_or_default();
        descendants.push(id.clone());
        out.push(R94Overlay {
            id,
            classes,
            descendants,
        });
    }
    out
}

/// 类 `class` 的**规则体**（`.<class> { … }` 里大括号之间的原文）。
///
/// 只认「选择器就是它」的写法：`.modal` 不会被 `.modal-overlay` 命中（右边界必须是分隔符），
/// `.help-panel .foo` 也不算（`.help-panel` 之后得直接跟 `{`）。
fn r94_class_rule(css: &str, class: &str) -> Option<String> {
    let pat = format!(".{class}");
    let mut from = 0usize;
    while let Some(rel) = css[from..].find(&pat) {
        let at = from + rel;
        let after = &css[at + pat.len()..];
        let boundary = after
            .chars()
            .next()
            .map(|c| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .unwrap_or(true);
        if boundary {
            if let Some(brace) = after.trim_start().strip_prefix('{') {
                if let Some(end) = brace.find('}') {
                    return Some(brace[..end].to_string());
                }
            }
        }
        from = at + pat.len();
    }
    None
}

/// 这个浮层是不是**模态**：它的某个非通用类规则里有 `inset: 0`。
///
/// 没有类规则的浮层（例如只有 `hidden` 的 `#tour-*` 零件）判为**非模态** —— 保守方向：留在射程内。
fn r94_is_modal(css: &str, classes: &[String]) -> bool {
    classes
        .iter()
        .filter(|c| c.as_str() != R94_GENERIC_CLASS)
        .any(|c| {
            r94_class_rule(css, c)
                .map(|rule| rule.contains(R94_MODAL_RULE))
                .unwrap_or(false)
        })
}

/// 一行代码里是否**提到**这个 id（`$("#help-body")` 或 `"help-body"` 两种拼法）。
fn r94_mentions(body: &str, id: &str) -> bool {
    body.contains(&format!("#{id}\"")) || body.contains(&format!("\"{id}\""))
}

/// 「往浮层里写文案」的函数名册：函数体里同时有 `T(` 与该浮层的某个 id。
fn r94_writers(app: &str, overlays: &[R94Overlay]) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for line in app.lines() {
        let Some(name) = function_name(line) else {
            continue;
        };
        let Some(body) = js_function_body(app, name) else {
            continue;
        };
        let code = code_only(body);
        if !code.contains("T(") {
            continue;
        }
        for o in overlays {
            if o.descendants.iter().any(|d| r94_mentions(&code, d)) {
                out.entry(o.id.clone())
                    .or_default()
                    .insert(name.to_string());
            }
        }
    }
    out
}

/// `atp:langchange` 处理器体（注释已剥；按代码的括号配对切，不用行号 —— 坑 #458）。
fn r94_handler_body(app: &str) -> Option<String> {
    let code = code_text_by_line(app).join("\n");
    let head = code.find(R94_HANDLER_START)? + R94_HANDLER_START.len();
    let open = head + code[head..].find('{')?;
    let mut depth = 0i32;
    for (i, ch) in code[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(code[open..=open + i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// 一段代码里**以 `name(` 形式被调用**的标识符集合（标识符边界：`item.id(` 不算 `id(`）。
fn r94_calls(text: &str) -> BTreeSet<String> {
    let is_word = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'$';
    let bytes = text.as_bytes();
    let mut out = BTreeSet::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if !is_word(bytes[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && is_word(bytes[i]) {
            i += 1;
        }
        let left_ok = start == 0 || !is_word(bytes[start - 1]);
        let mut k = i;
        while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
            k += 1;
        }
        if left_ok && k < bytes.len() && bytes[k] == b'(' {
            out.insert(text[start..i].to_string());
        }
    }
    out
}

/// 一次扫描同时产出三条规则的判决**与它们的证据**（逐条可打印 —— #339/#341：判词与取值两列）。
struct R94Reading {
    /// 处理器体本身有没有被找到（找不到时三条规则会静默地在空集上「通过」）。
    handler_found: bool,
    overlays: BTreeSet<String>,
    modal: BTreeSet<String>,
    non_modal: BTreeSet<String>,
    scope: BTreeSet<String>,
    writers: BTreeMap<String, BTreeSet<String>>,
    refreshes: BTreeSet<String>,
    missing: BTreeSet<String>,
    r1: bool,
    r2: bool,
    r3: bool,
}

impl R94Reading {
    fn verdicts(&self) -> (bool, bool, bool) {
        (self.r1, self.r2, self.r3)
    }

    /// 把已经读到的证据折成三条判词。
    ///
    /// R1 的**空集保护**在同一条判词里：射程为空（没有非模态浮层，或没有 JS 写者）时必须红，
    /// 而不是在空集上恒真（坑 68 家族；牙齿测试有一条专门喂空名册）。
    fn finish(mut self) -> R94Reading {
        let in_scope = !self.scope.is_empty();
        self.r1 = self.missing.is_empty() && in_scope;
        self.r2 = self.refreshes.len() >= R94_MIN_REFRESHES;
        self.r3 = !self.modal.is_empty() && !self.non_modal.is_empty();
        self
    }

    fn report(&self) -> String {
        format!(
            "r1={} r2={} r3={} handler={} | overlays={:?} modal={:?} scope={:?} refreshes={:?} writers={:?} missing={:?}",
            self.r1,
            self.r2,
            self.r3,
            self.handler_found,
            self.overlays,
            self.modal,
            self.scope,
            self.refreshes,
            self.writers,
            self.missing,
        )
    }
}

/// 读一棵树（index.html / app.js / style.css）得出三条规则的判决。
fn r94_read(html: &str, app: &str, css: &str) -> R94Reading {
    let overlays = r94_overlays(html);
    let mut modal = BTreeSet::new();
    let mut non_modal = BTreeSet::new();
    for o in &overlays {
        if r94_is_modal(css, &o.classes) {
            modal.insert(o.id.clone());
        } else {
            non_modal.insert(o.id.clone());
        }
    }
    let writers = r94_writers(app, &overlays);
    let scope: BTreeSet<String> = non_modal
        .iter()
        .filter(|id| writers.contains_key(*id))
        .cloned()
        .collect();
    let handler = r94_handler_body(app).unwrap_or_default();
    let calls = r94_calls(&handler);
    let refreshes: BTreeSet<String> = calls
        .iter()
        .filter(|c| c.starts_with("render"))
        .cloned()
        .collect();
    let mut missing = BTreeSet::new();
    for id in &scope {
        for w in writers.get(id).into_iter().flatten() {
            if !calls.contains(w) {
                missing.insert(format!("{id}/{w}"));
            }
        }
    }
    R94Reading {
        handler_found: r94_handler_body(app).is_some(),
        overlays: overlays.iter().map(|o| o.id.clone()).collect(),
        modal,
        non_modal,
        scope,
        writers,
        refreshes,
        missing,
        r1: false,
        r2: false,
        r3: false,
    }
    .finish()
}

/// 在**兄弟守卫行**之后插一行（幂等：已经在了就不动）—— 变体树与修复体共用同一个构造器。
fn r94_insert_after_tour_guard(app: &str, text: &str) -> String {
    if app.contains(text) {
        return app.to_string();
    }
    let mut out = Vec::new();
    let mut done = false;
    for line in app.lines() {
        out.push(line.to_string());
        if !done && line.contains(R94_TOUR_GUARD) {
            let indent = &line[..line.len() - line.trim_start().len()];
            out.push(format!("{indent}{text}"));
            done = true;
        }
    }
    assert!(done, "找不引导守卫行 `{R94_TOUR_GUARD}`（锚点漂移）");
    let mut text = out.join("\n");
    text.push('\n');
    text
}

/// 修复体：处理器里补上「面板开着就重画」。
fn r94_variant_fix(app: &str) -> String {
    r94_insert_after_tour_guard(app, R94_FIXED_GUARD)
}

/// 竞争修法：切线时把面板**关掉**（本门禁与探针**同判不同因**：这里因为写者没被调用而红，
/// 探针按 DOM 实况因为「面板被夺走」而红 —— 射程边界，实测）。
fn r94_variant_close(app: &str) -> String {
    r94_insert_after_tour_guard(app, R94_CLOSE_GUARD)
}

/// 未修形状：把修复体那一行摘掉（**只**摘这一行，别的刷新名册不动）。
fn r94_variant_unfixed(app: &str) -> String {
    let before = app.to_string();
    let kept: Vec<&str> = app
        .lines()
        .filter(|l| !l.contains(R94_FIXED_GUARD))
        .collect();
    let out = kept.join("\n") + "\n";
    assert!(
        out != before || !before.contains(R94_FIXED_GUARD),
        "修复体那一行没被摘掉（锚点漂移）"
    );
    out
}

/// 假修：把刷新名册**削到恰好满足 R1 的最小集** —— 只留浮层写者（`renderHelp` / `renderTourStep`），
/// 把导航与当前视图那两次刷新删掉。R1 仍绿（浮层写者都在），而 R2 的下界必须红 —— 挡住
/// 「名册越删越干净」这条路径。
fn r94_variant_minimal_roster(app: &str) -> String {
    let mut out = Vec::new();
    let mut inside = false;
    let mut dropped = 0usize;
    for line in app.lines() {
        if line.contains(R94_HANDLER_START) {
            inside = true;
        }
        let trims = line.trim_start();
        if inside && (trims.contains("renderNav()") || trims.contains("renderView(activeView)")) {
            dropped += 1;
            continue;
        }
        if inside && line.trim_end().ends_with("});") {
            inside = false;
        }
        out.push(line.to_string());
    }
    assert_eq!(
        dropped, 2,
        "刷新名册的锚点漂了（摘掉 {dropped} 行，期望 2）"
    );
    out.join("\n") + "\n"
}

/// 合成的**新浮层**（`ui/index.html`）：验证「将来新加的浮层忘了登记时会被抓到」。
fn r94_variant_new_overlay(html: &str) -> String {
    let inject = concat!(
        "<div id=\"new-panel\" class=\"panel hidden\">\n",
        "  <div id=\"new-panel-body\"></div>\n",
        "</div>\n",
    );
    let at = html
        .find("<div id=\"help-panel\"")
        .expect("找不到 `#help-panel`（名册的起点漂了）");
    format!("{}{}{}", &html[..at], inject, &html[at..])
}

/// 合成的**新写者**（`ui/js/app.js`）：给上面那个新浮层的 JS 内容。
fn r94_variant_new_writer(app: &str) -> String {
    let writer = concat!(
        "  function renderNewPanel() {\n",
        "    $(\"#new-panel-body\").innerHTML = T(\"nav.dashboard\");\n",
        "  }\n",
    );
    let at = app
        .find("  function renderHelp() {")
        .expect("找不到 `renderHelp`（写者名册的锚点漂了）");
    format!("{}{}{}", &app[..at], writer, &app[at..])
}

/// 合成的**假修**：把 `#chat-modal` 的 `hidden` 类摘掉 ⇒ 它不再是名册里的浮层（R3 的「模态」那一半
/// 变空，而 R1 仍然覆盖得了剩下的非模态浮层）—— 专门测量 R3 的牙。
fn r94_variant_no_modal(html: &str) -> String {
    let from = "<div id=\"chat-modal\" class=\"modal-overlay hidden\">";
    let to = "<div id=\"chat-modal\" class=\"modal-overlay\">";
    assert_eq!(
        html.matches(from).count(),
        1,
        "找不到 `#chat-modal` 的类属性（名册的锚点漂了）"
    );
    html.replace(from, to)
}

/// 空名册的极端形状：把浮层全部删掉 —— R1 的射程变空，必须**响亮地红**（坑 68）。
fn r94_variant_no_overlays(html: &str) -> String {
    let kept: Vec<&str> = html
        .lines()
        .filter(|l| {
            !l.starts_with("<div id=\"help-panel\"")
                && !l.starts_with("<div id=\"chat-modal\"")
                && !l.starts_with("<div id=\"tour-")
        })
        .collect();
    let out = kept.join("\n") + "\n";
    assert!(
        r94_overlays(&out).is_empty(),
        "浮层没删干净（构造器坏了）: {:?}",
        r94_overlays(&out)
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 本轴的读取入口：浮层名册与模态判据永远取自 live 的 `ui/index.html` / `ui/css/style.css`，
    /// 只有被测的 `app.js` 由调用方给（两条腿喂不同的树）。
    fn r94_live(app: &str) -> R94Reading {
        r94_read(INDEX_HTML, app, STYLE_CSS)
    }

    /// 本轴的派生名册（`ui/index.html` 中 `#app` 之后、带 `hidden` 类的顶行元素）。
    fn r94_roster() -> BTreeSet<String> {
        r94_overlays(INDEX_HTML).into_iter().map(|o| o.id).collect()
    }

    /// 轴：切语言时被刷新的名册必须覆盖**用户此刻可能正开着的每个非模态浮层**（R94）。
    ///
    /// 三条规则各自的含义见文件头。本测试只断言「三条同时成立」；每条规则的**牙**由
    /// [`the_r94_rules_have_teeth`] 逐条测量，与竞争修法的**关系**由
    /// [`the_r94_rules_separate_the_variants`] 逐腿声明。
    #[test]
    fn the_language_switch_refreshes_every_overlay_it_can_show() {
        let read = r94_live(APP_JS);
        assert!(
        read.handler_found,
        "取不到 `{R94_HANDLER_START}` 的处理器体（开头/收尾定界不唯一）—— 三条规则的射程会静默变空：{}",
        read.report()
    );
        assert!(
            read.scope.len() >= 2,
            "射程里的非模态浮层少于两个 —— R1 退化成空集上的关系式：{}",
            read.report()
        );
        assert!(
            !read.modal.is_empty() && !read.non_modal.is_empty(),
            "派生名册里模态/非模态有一边是空的 —— R3 的分界恒真，规则失去意义：{}",
            read.report()
        );
        assert!(
            read.refreshes.len() >= R94_MIN_REFRESHES,
            "处理器派生出来的 `render*` 刷新名少于 {R94_MIN_REFRESHES} 个 —— R2 的下界不成立：{}",
            read.report()
        );
        assert!(
            read.verdicts() == (true, true, true),
            "切语言必须刷新用户可能正开着的每个非模态浮层（R94）：{}",
            read.report()
        );
    }

    /// 阳性对照：派生器/写者名册/读取器在**已知为绿**的树上读到东西，在「什么都没有」的树上读到空。
    ///
    /// 这是三条规则的射程保险：读取器若只会返回常量，规则就成了恒真断言（空集上的关系式会静默
    /// 通过）。⚠️ 绝对判词只打在自己拼出来的树上 —— 真树在两条腿上形状不同（#314）。
    #[test]
    fn the_r94_roster_is_real() {
        // ① 名册的阳性对照（解析器变了 / index.html 结构变了都会在这里响）。
        let roster = r94_roster();
        assert!(
            roster.contains("help-panel")
                && roster.contains("chat-modal")
                && roster.contains("tour-overlay"),
            "派生名册少了浮层（解析器坏了，还是 index.html 结构变了？）：{roster:?}"
        );
        assert!(
        !roster.contains("app") && !roster.contains("login-view") && !roster.contains("toast-wrap"),
        "派生名册混进了射程外的元素（`#app` 是起点，实例由 JS 即时生成的 `#toast-wrap` 不是浮层）：{roster:?}"
    );

        // ② 模态判据的阳性对照（R3 的分界真的能分开两类，而不是恒真/恒假）。
        let modal: BTreeSet<String> = roster
            .iter()
            .filter(|id| {
                let classes = r94_overlays(INDEX_HTML)
                    .into_iter()
                    .find(|o| o.id == **id)
                    .map(|o| o.classes)
                    .unwrap_or_default();
                r94_is_modal(STYLE_CSS, &classes)
            })
            .cloned()
            .collect();
        assert!(
        modal.contains("chat-modal") && !modal.contains("help-panel"),
        "模态判据没有把 `#chat-modal`（`.modal-overlay` 有 `inset: 0`）与 `#help-panel`（无）分开：{modal:?}"
    );

        // ③ 写者名册的阳性对照：`#help-panel` 的 JS 写者**恰好**是 `renderHelp`（派生出来的）。
        let live = r94_live(APP_JS);
        assert_eq!(
            live.writers.get("help-panel"),
            Some(
                &[String::from("renderHelp")]
                    .into_iter()
                    .collect::<BTreeSet<_>>()
            ),
            "`#help-panel` 的写者名册不是恰好 `renderHelp` —— 写者判别式变了：{}",
            live.report()
        );

        // ④ 修复树上三条规则全绿，且证据全都读到。
        let fixed_tree = r94_variant_fix(APP_JS);
        let fix = r94_live(&fixed_tree);
        assert!(
            fix.verdicts() == (true, true, true),
            "修复树上三条规则不是全绿 —— 变体构造器坏了：{}",
            fix.report()
        );
        assert!(
            fix.missing.is_empty() && !fix.refreshes.is_empty(),
            "修复树上 R1 的输入或证据是空的 —— 那条规则会恒真：{}",
            fix.report()
        );

        // ⑤ 反向：未修形状上缺的必须**恰好**是帮助面板（派生出来的，不是抄的）。
        let gone = r94_live(&r94_variant_unfixed(APP_JS));
        assert_eq!(
            gone.missing,
            [String::from("help-panel/renderHelp")]
                .into_iter()
                .collect::<BTreeSet<_>>(),
            "未修形状上「没被刷新的浮层」不是恰好帮助面板 —— R1 的判别式变了：{}",
            gone.report()
        );
        assert!(
            !gone.r1 && gone.r2 && gone.r3,
            "未修形状只该翻 R1 一条：{}",
            gone.report()
        );

        // ⑥ 空名册的极端形状：射程变空时必须**响亮地红**，不许在空集上通过（坑 68）。
        let empty = r94_read(&r94_variant_no_overlays(INDEX_HTML), APP_JS, STYLE_CSS);
        assert!(
            empty.overlays.is_empty() && !empty.r1,
            "浮层全删掉之后 R1 仍然绿 —— 覆盖率规则在空集上会「通过」：{}",
            empty.report()
        );
    }

    /// 三条规则**各有独立的牙**：合成变异体逐个喂给规则自己的判别式，每个恰好打翻一条。
    ///
    /// 判据是「恰好一条翻转」而不是「至少一条红」—— 否则一条从别处借来红的规则也能自称有牙
    /// （#454：牙齿必须长在该规则的判别式上）。基线是**已知为绿的**修复树（#458）。
    #[test]
    fn the_r94_rules_have_teeth() {
        let fixed_tree = r94_variant_fix(APP_JS);
        let base = r94_live(&fixed_tree);
        assert_eq!(
            base.verdicts(),
            (true, true, true),
            "自证基线不绿，牙齿测试没有意义：{}",
            base.report()
        );

        // 每个变异体只动一处，期望**恰好一条**翻转（#454）。顺序 = (r1, r2, r3)。
        let unfixed = r94_variant_unfixed(&fixed_tree);
        let trimmed = r94_variant_minimal_roster(&fixed_tree);
        let new_overlay_tree = r94_variant_new_writer(&r94_variant_fix(APP_JS));
        let mutants: [(&str, R94Reading, (bool, bool, bool)); 4] = [
            ("guard gone", r94_live(&unfixed), (false, true, true)),
            (
                "the refresh roster is trimmed to the writers only",
                r94_live(&trimmed),
                (true, false, true),
            ),
            (
                "a future overlay's writer is not registered",
                r94_read(
                    &r94_variant_new_overlay(INDEX_HTML),
                    &new_overlay_tree,
                    STYLE_CSS,
                ),
                (false, true, true),
            ),
            (
                "no modal overlay left in the roster",
                r94_read(&r94_variant_no_modal(INDEX_HTML), &fixed_tree, STYLE_CSS),
                (true, true, false),
            ),
        ];
        for (label, read, expected) in mutants {
            assert_eq!(
                read.verdicts(),
                expected,
                "规则 `{label}` 的牙不成立（期望 {expected:?}）：{}",
                read.report()
            );
        }

        // R1 的第二半：新浮层的缺项必须是**它自己**（证明规则不是只认 `help-panel` 一个元素）。
        let new_overlay = r94_read(
            &r94_variant_new_overlay(INDEX_HTML),
            &new_overlay_tree,
            STYLE_CSS,
        );
        assert_eq!(
            new_overlay.missing,
            [String::from("new-panel/renderNewPanel")]
                .into_iter()
                .collect::<BTreeSet<_>>(),
            "新浮层的缺项不是它自己 —— 覆盖率的判别式变了：{}",
            new_overlay.report()
        );

        // R2 的第二半：**刷新的名字少于下界**也必须响亮失败（不许把「名册越删越干净」当成功）。
        let numpty = r94_live(&r94_variant_minimal_roster(&fixed_tree));
        assert!(
            !numpty.r2 && numpty.refreshes.len() < R94_MIN_REFRESHES,
            "刷新名册被削到最小集后 R2 仍绿 —— 那条规则可以静默失明：{}",
            numpty.report()
        );
        assert!(
            numpty.r1 && numpty.r3,
            "削名册只该翻 R2 一条：{}",
            numpty.report()
        );

        // 读取器本身的牙（合成输入：不碰真文件）—— 「提不到 id」和「提到了但没写」必须都不算写者。
        assert!(r94_mentions(
            "$(\"#help-body\").innerHTML = T(\"a\");",
            "help-body"
        ));
        assert!(!r94_mentions(
            "$(\"#help-bodyx\").innerHTML = \"x\";",
            "help-body"
        ));
        assert!(!r94_mentions(
            "$(\"#help-body\").innerHTML = \"x\";",
            "help-context"
        ));
        assert!(r94_calls("if (a) renderHelp();").contains("renderHelp"));
        assert!(!r94_calls("const x = o.renderHelp;").contains("renderHelp"));
    }

    /// 规则与**竞争修法**的关系，逐腿声明（#339/#341：声明的期望与实际各印一列）。
    ///
    /// 竞争修法来自 jsdom 探针 `r94_probe.js`（它按 DOM 实况逐条裁定）：
    /// - `m_close`：切线时把用户自己开的面板**关掉**（「屏幕上没有面板，就没有陈旧的行」）。
    ///   本门禁**也**拒它 —— 但理由是「写者 `renderHelp` 没被调用」（R1 问的是刷新了没），而探针的
    ///   `A4` 腿拒它的理由是「用户的面板被夺走」。**同判不同因**：这条边界实测在这里，不是只写在注释里
    ///   （C2148 的分工：两个仪器各自能看见对方看不见的东西）。
    /// - `m_minimal_roster`：把刷新名册削到只留浮层写者（R1 仍绿、R2 收口）。
    ///
    /// 探针另有一条 `m_datai18n`（把四行快捷键改写成静态 `data-i18n`），它要同时改写 `index.html` 与
    /// `app.js` 两侧，本门禁的构造器不做它 —— 它的判官是探针的 `A2`/`Z1` 腿。
    #[test]
    fn the_r94_rules_separate_the_variants() {
        // ⚠️ 本测试必须在**两腿**都绿（编译门禁分别把真树与 E1 修复树当作 `APP_JS` 来编译）
        // ⇒ 绝对判词只能打在**它自己拼出来的树**上（#314），而每棵变体树都必须从**同一个**起点拼
        // （`r94_variant_fix(APP_JS)` 幂等 ⇒ 两条腿上逐字相同，#612）；从 `APP_JS` 直接拼会在 base 腿上
        // 拼出另一棵树（那条腿的 `APP_JS` 就是未修形状），声明的判词当场失真。
        let all_green = (true, true, true);

        let tree_fix = r94_variant_fix(APP_JS);
        let tree_unfixed = r94_variant_unfixed(&tree_fix);
        let tree_close = r94_variant_close(&tree_unfixed);
        let tree_trimmed = r94_variant_minimal_roster(&tree_fix);

        // 先证明这些树互不相同，否则「判词不同」可能只是同一棵树的两张脸。
        for (label, other) in [
            ("unfixed", &tree_unfixed),
            ("m_close", &tree_close),
            ("m_minimal_roster", &tree_trimmed),
        ] {
            assert_ne!(tree_fix, *other, "`{label}` 与修复树逐字相同 —— 变体没落地");
        }

        let declared = [
            ("fix (spliced)", tree_fix.as_str(), all_green),
            (
                "unfixed (the defect)",
                tree_unfixed.as_str(),
                (false, true, true),
            ),
            (
                "m_close (probe rejects on UX, gate rejects on the writer)",
                tree_close.as_str(),
                (false, true, true),
            ),
            (
                "m_minimal_roster",
                tree_trimmed.as_str(),
                (true, false, true),
            ),
        ];
        for (name, app, expected) in declared {
            let read = r94_live(app);
            assert_eq!(
                read.verdicts(),
                expected,
                "变体 `{name}` 的判词与声明不符（声明 {expected:?}）：{}",
                read.report()
            );
        }

        // `m_close` 那条边界要可测量：它**不是**修复体（逐字不同），却与**未修形状**缺同一项
        // （`help-panel/renderHelp`）—— 因为「把面板关掉」并没有让写者被调用。两个仪器在这一点上
        // 同判（都拒）、不同因，这里钉的就是那个「因」。
        assert_ne!(
            tree_close, tree_fix,
            "`m_close` 与修复体逐字相同 —— 竞争修法没落地"
        );
        assert_eq!(
            r94_live(&tree_close).missing,
            r94_live(&tree_unfixed).missing,
            "`m_close` 的缺项与未修形状不同 —— 「关掉面板 ≠ 刷新面板」这条判据变了"
        );
    }

    /// 修复体常量与插入锚点的自洽（跨制品那一腿在 `r94_compile_gate.py` 里，真值取自编辑表）。
    #[test]
    fn the_r94_fixed_guard_is_the_edit_sheet_text() {
        assert!(
            !R94_FIXED_GUARD.contains(R94_MODAL_RULE)
                && R94_FIXED_GUARD.contains("classList.contains(\"hidden\")"),
            "修复体不是「读 class 机制」那一行：{R94_FIXED_GUARD:?}"
        );
        assert!(
            R94_FIXED_GUARD.contains("renderHelp();") && R94_FIXED_GUARD.contains("#help-panel"),
            "修复体不是「帮助面板开着就重画」那一行：{R94_FIXED_GUARD:?}"
        );
        assert!(
            R94_TOUR_GUARD.contains("renderTourStep();") && !R94_TOUR_GUARD.contains("help-panel"),
            "插入锚点不是引导守卫行：{R94_TOUR_GUARD:?}"
        );
        // 幂等：两腿拿到的必须是**同一棵**修复树（否则声明表在另一条腿上就不成立了，#612）。
        let once = r94_variant_fix(APP_JS);
        let twice = r94_variant_fix(&once);
        assert_eq!(once, twice, "修复体构造器不幂等 —— 两腿会得到不同的树");
        assert_eq!(
            once.matches(R94_FIXED_GUARD).count(),
            1,
            "修复树里那一行不是恰好一个"
        );
        assert_ne!(
            r94_variant_unfixed(APP_JS),
            once,
            "未修形状与修复树逐字相同 —— 变体构造器失效了"
        );
        assert_ne!(
            R94_CLOSE_GUARD, R94_FIXED_GUARD,
            "关面板式竞争修法与修复体逐字相同 —— 竞争修法没落地"
        );
    }

    /// R93 的**基线树** = 位点的未修形状。
    ///
    /// 两条腿（编译门禁的**真树**与 **E1 修复树**）都先归一到它，变体测试才是在**同一棵树**上做的
    /// （坑 #314：真树在两条腿上形状不同，直接读 `APP_JS` 会让「未修形状上 B 翻红」那条在修复腿上
    /// 测了另一棵树）。幂等：已是未修形状时原样返回。
    fn r93_base_tree(app: &str) -> String {
        app.replace(R93_SITE_FIXED, R93_SITE_UNFIXED)
    }

    /// R93 的**修复树**。
    fn r93_fix_tree(app: &str) -> String {
        r93_base_tree(app).replace(R93_SITE_UNFIXED, R93_SITE_FIXED)
    }

    /// 把一棵树读一遍（三条规则 + 证据）。
    fn r93_read(app: &str) -> R93Reading {
        R93Reading::new(app, PASS_MIN_MOD_RS, I18N_JS)
    }

    /// 删掉**包含 `needle` 的那一整行**（行尾换行保持原样）。
    fn r93_remove_line(src: &str, needle: &str) -> String {
        let kept: Vec<&str> = src.lines().filter(|l| !l.contains(needle)).collect();
        let mut out = kept.join("\n");
        if src.ends_with('\n') {
            out.push('\n');
        }
        out
    }

    /// 变异：**拿掉**长度守卫整行（竞争修法「不做行内长度校验，交给服务端」）⇒ A-lower 该翻红，
    /// 另两条该保持绿。
    fn r93_variant_drop(app: &str) -> String {
        r93_remove_line(&r93_fix_tree(app), R93_SITE_FIXED)
    }

    /// 变异（A-upper 自己的牙）：长度守卫改说**另一把嗓子**（一个只在「其他」类出现的键）⇒
    /// 只有 A-upper 该翻红。
    fn r93_variant_wrong_key(app: &str) -> String {
        let site = R93_SITE_FIXED.replace("err.weakPassword", R93_MUT_KEY);
        r93_fix_tree(app).replace(R93_SITE_FIXED, &site)
    }

    /// 变异（B 自己的牙）：在修复树上再加**一个别的键**同时服务两类 ⇒ 只有 B 该翻红。
    ///
    /// 两条新守卫刻意**都不比较 `.length`**（否则 A-upper 会连坐，「恰好一条翻红」就废了）：
    /// 一条是空类（`!$("#…")`），一条是其他类里的比较（`count < 2`）。
    fn r93_variant_cross_class(app: &str) -> String {
        let add = format!(
        "\n        if (!$(\"#r93-cross\")) {{ setFieldError($(\"#r93-cross\"), T(\"{k}\")); }}\n        \
         if (r93CrossCount < 2) {{ setFieldError($(\"#r93-cross\"), T(\"{k}\")); }}\n",
        k = R93_MUT_KEY
    );
        r93_fix_tree(app) + &add
    }

    /// 变异（注释不是证据，坑 #345）：加一行**注释**，说的正是未修形状那句话 ⇒ 判词必须与修复树
    /// **逐条相同**（并且位点数一个不差）。
    fn r93_variant_mark_only(app: &str) -> String {
        format!("{}\n        // {}\n", r93_fix_tree(app), R93_SITE_UNFIXED)
    }

    /// 轴：找回密码表单的长度守卫必须说**长度**那条规则 —— 与同一规则在服务端拿到的那句话同源。
    ///
    /// 三条规则各自的牙见 [`the_r93_rules_have_teeth`]，读取器的阳性对照见
    /// [`the_r93_readers_are_real`]，两个位点形状与编辑表的关系见
    /// [`the_r93_site_shapes_are_the_edit_sheet_text`]。
    #[test]
    fn the_forgot_password_length_speaks_the_message_the_same_rule_gets_from_the_server() {
        let read = r93_read(APP_JS);
        assert!(
            read.branches > 0,
            "后端没有分支强制这条规则 —— 推导链是空的，三条规则会一起空转：{}",
            read.report()
        );
        assert!(
            read.key.is_some(),
            "推导链断了（helper 候选不唯一 / 原话过不了 ERR_MAP / 解析到不止一个键）：{}",
            read.report()
        );
        assert!(
            read.unresolved.is_empty(),
            "后端某条分支的原话在 ERR_MAP 里没有落点 —— 客户端拿不到「同一把嗓子」的那句话：{}",
            read.report()
        );
        assert!(
            read.sites.len() >= 10,
            "全文件的行内位点少得不正常（扫描器读空了？）—— B 会退化成恒真：{}",
            read.report()
        );
        assert!(
            !read.length_sites().is_empty(),
            "一个长度守卫都没有 —— A-upper 是一句空转的全称命题（坑 #324）：{}",
            read.report()
        );
        assert!(
            read.verdicts() == (true, true, true),
            "行内字段错误的文案必须描述**触发它的条件**（R93 三条规则）：{}",
            read.report()
        );
    }

    /// 两个位点形状就是编辑表那一段文本，且只差**键那一个 token**。
    #[test]
    fn the_r93_site_shapes_are_the_edit_sheet_text() {
        let base = r93_base_tree(APP_JS);
        let fixed = r93_fix_tree(APP_JS);
        assert_eq!(
            base.matches(R93_SITE_UNFIXED).count(),
            1,
            "未修形状在基线树上不是恰好一处"
        );
        assert_eq!(
            base.matches(R93_SITE_FIXED).count(),
            0,
            "基线树上已经有修复体（变体构造器的方向反了？）"
        );
        assert_eq!(
            fixed.matches(R93_SITE_FIXED).count(),
            1,
            "修复体在修复树上不是恰好一处"
        );
        assert_eq!(fixed.matches(R93_SITE_UNFIXED).count(), 0);
        // 两棵树只差那一处：归一回未修形状后逐字节相同（变体构造器的幂等性）。
        assert_eq!(r93_base_tree(&fixed), base, "两个形状互逆失败");
        // 两个形状之间差的**只是键那一个 token**：把两种键拼法换成同一个记号后逐字相同。
        let blank_keys = |s: &str| {
            s.replace("register.err.pass", "@k@")
                .replace("err.weakPassword", "@k@")
        };
        assert_eq!(
            blank_keys(R93_SITE_UNFIXED),
            blank_keys(R93_SITE_FIXED),
            "两个形状差的不是那个键 —— 编辑表改了别的东西"
        );
        assert_ne!(R93_SITE_UNFIXED, R93_SITE_FIXED);
    }

    /// 阳性对照：读取器在**已知为绿**的树上读到东西、在未修形状上**恰好多翻两条**；派生链在合成
    /// 输入上有牙齿；剥注释有牙齿。
    ///
    /// ⚠️ 断言只打在自己拼出来的树上（真树在两条腿上形状不同 —— 所以基线一律先走
    /// [`r93_base_tree`]）。
    #[test]
    fn the_r93_readers_are_real() {
        let base = r93_base_tree(APP_JS);
        let fixed = r93_fix_tree(APP_JS);

        // 正向：修复树上三条规则全绿，且证据（分支数、键、位点、长度位点）全都读到。
        let fix = r93_read(&fixed);
        assert_eq!(
            fix.verdicts(),
            (true, true, true),
            "修复树上三条规则不是全绿 —— 变体构造器或派生链坏了：{}",
            fix.report()
        );
        assert!(
            fix.branches >= 1 && fix.key.is_some() && !fix.literals.is_empty(),
            "修复树上的推导链是空的：{}",
            fix.report()
        );
        assert!(
            fix.sites.len() >= 10,
            "修复树上的位点数少得不正常（扫描器读空了？）：{}",
            fix.report()
        );
        assert!(
            !fix.length_sites().is_empty() && fix.shared_keys.is_empty(),
            "修复树上的证据不对：{}",
            fix.report()
        );

        // 反向：未修形状上**恰好**两条翻红（A-upper 与 B），A-lower 仍绿；跨两类的键恰好是那把
        // 「空」的嗓子。名字写在这里是**判词的投影**（本轴全仓只有这一个成员，侦察 §6），
        // 它红了就是要人回来看——不是「通过」。
        let bad = r93_read(&base);
        assert_eq!(
            bad.verdicts(),
            (true, false, false),
            "未修形状的投影不是 (T,F,F)：{}",
            bad.report()
        );
        let want: BTreeSet<String> = [String::from("register.err.pass")].into_iter().collect();
        assert_eq!(
            bad.shared_keys,
            want,
            "跨两类的键不是恰好那把「空」的嗓子：{}",
            bad.report()
        );
        // 这条轴**存在**的判据：未修形状上，长度位点说的话**不是**规则推导出来的那个键。
        // 那把键从 `bad.key` 取（**推导**来的，不写死「今天那句错话」）—— 判词说的是「位点与规则
        // 不同源」，不是「位点恰好写了某个字符串」；写死错话会让这条腿在别人改那句话时静默失效。
        let k = bad.key.clone();
        assert!(
            bad.length_sites()
                .iter()
                .all(|s| Some(s.key.as_str()) != k.as_deref()),
            "未修形状上长度位点已经用对了嗓子 —— 这条轴不存在了：{}",
            bad.report()
        );

        // ---- 派生链的牙齿（合成输入：不碰真文件）----
        let pairs = r93_err_map(I18N_JS);
        assert!(
            pairs.len() >= 40,
            "ERR_MAP 解析器读空了（{} 条）—— 推导会静默退化",
            pairs.len()
        );
        // 最长子串规则端到端：注册那句是重置那句的**子串** ⇒ 两条原话必须落到同一个键。
        let ks: BTreeSet<String> = bad
            .literals
            .iter()
            .filter_map(|l| r93_resolve(l, &pairs))
            .collect();
        assert_eq!(
            ks.len(),
            1,
            "同一规则的两句原话解析到不止一个键（最长匹配没生效？）：{:?} / {:?}",
            bad.literals,
            ks
        );
        // 字节口径**不是**这条规则的形状：候选必须为空（响亮失败，而不是猜一个）。
        let byte_shape = "fn f(x: &str) -> bool {\n    x.len() < MIN\n}\n";
        assert!(
            r93_helper(byte_shape).is_none(),
            "字节形状被当成了字符形状的 helper —— 推导会在错的规则上「通过」"
        );
        // 两个候选也必须是响亮失败（不唯一 ⇒ 不猜）。
        let two = "fn a(x: &str) -> bool {\n    x.chars().count() < MIN\n}\n\
               fn b(y: &str) -> bool {\n    y.chars().count() < MIN\n}\n";
        assert!(r93_helper(two).is_none(), "两个候选必须响亮失败");
        // 正对照：字符形状的 helper 必须**读得到**（否则上面两条是空的）。
        let one = "fn a(x: &str) -> bool {\n    x.chars().count() < MIN\n}\n";
        assert_eq!(
            r93_helper(one).map(|(n, _)| n),
            Some(String::from("a")),
            "字符形状的 helper 都读不到 —— 判别式坏了"
        );

        // ---- 剥注释的牙齿（合成输入：注释里的位点不算证据）----
        let synth = "var x = 1; // setFieldError($(\"#zzz\"), T(\"zzz.key\"))\n\
                 /* setFieldError($(\"#yyy\"), T(\"yyy.key\")) */\n\
                 if (!a) { setFieldError($(\"#www\"), T(\"www.key\")); }\n";
        let (s, u) = r93_sites(synth);
        assert_eq!(
            s.iter().map(|x| x.key.clone()).collect::<Vec<_>>(),
            vec![String::from("www.key")],
            "剥注释后仍有假位点（行内 `//` 或块注释漏了）"
        );
        assert_eq!(u, 0, "合成输入里没有无守卫的位点");
        // 行号必须对得上磁盘（坑 #532）：真位点在第 3 行。
        assert_eq!(s[0].line, 3, "报出的行号不是磁盘上的行号");

        // ---- 两条分类器的正/反向（合成输入）----
        assert!(
            r93_empty_guard("!pw")
                && r93_empty_guard("!$(\"#a\")")
                && r93_empty_guard("x.trim() === \"\"")
                && r93_empty_guard("pw === \"\""),
            "空类判别式漏了合法形状"
        );
        assert!(
            !r93_empty_guard("Array.from(pw).length < MIN_PW_CHARS")
                && !r93_empty_guard("pw !== pw2")
                && !r93_empty_guard("a <= 0"),
            "非空类形状被误判成空类（B 会因此无意义地翻红）"
        );
        assert!(
            r93_bounds_length("Array.from(pw).length < MIN_PW_CHARS")
                && r93_bounds_length("a.length > 3"),
            "长度判别式漏了合法形状"
        );
        assert!(
            !r93_bounds_length("a.count < 2") && !r93_bounds_length("a.value"),
            "长度判别式误吞了非长度比较（A-upper 会因此管辖过宽）"
        );
    }

    /// 三条规则**各有独立的牙**：每个变异恰好打翻一条，且基线是**已知为绿**的修复树。
    ///
    /// 判据是「恰好一条翻转」而不是「至少一条红」—— 否则一条从别处借来红的规则也能自称有牙。
    #[test]
    fn the_r93_rules_have_teeth() {
        let baseline = r93_read(&r93_fix_tree(APP_JS));
        assert_eq!(
            baseline.verdicts(),
            (true, true, true),
            "自证基线不绿，牙齿测试没有意义：{}",
            baseline.report()
        );

        // 顺序 = (A-lower, A-upper, B)。
        let mutants = [
            (
                "length guard removed",
                r93_variant_drop(APP_JS),
                (false, true, true),
            ),
            (
                "length guard speaks another key",
                r93_variant_wrong_key(APP_JS),
                (true, false, true),
            ),
            (
                "one key serves both classes",
                r93_variant_cross_class(APP_JS),
                (true, true, false),
            ),
        ];
        for (name, tree, want) in mutants {
            let got = r93_read(&tree).verdicts();
            assert_eq!(
                got, want,
                "变异 `{name}` 的投影不是「恰好一条翻红」（want {want:?}）"
            );
        }

        // 注释不算证据：加一句说着未修形状的注释，判词与位点数必须**逐条相同**（坑 #345）。
        let marked = r93_read(&r93_variant_mark_only(APP_JS));
        assert_eq!(
            marked.verdicts(),
            baseline.verdicts(),
            "注释被当成了证据：{}",
            marked.report()
        );
        assert_eq!(
            marked.sites.len(),
            baseline.sites.len(),
            "注释造出了幻影位点：{}",
            marked.report()
        );
    }

    /// 闭包里各函数**宣称隐藏**的 `#<id>` 集合（判别式与 C2171 同一把：字面量 `"#<id>"` **且**把
    /// `hidden` 加上去的操作，两者在本元素的**同一个函数体**里）。
    fn r168_ids_hidden_by(app: &str, fns: &BTreeSet<String>) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for f in fns {
            let Some(body) = function_source(app, f) else {
                continue;
            };
            let code = code_lines(&body);
            for id in quoted_hash_ids(&code) {
                if hides_element(&code, &id) {
                    out.insert(id);
                }
            }
        }
        out
    }

    /// 某条 Esc 分支（它的调用闭包）能关掉的 `#<id>` 集合。
    fn r168_line_hidden_ids(app: &str, line: &str) -> BTreeSet<String> {
        let mut roots: BTreeSet<String> = BTreeSet::new();
        roots.extend(callee_names(line));
        r168_ids_hidden_by(app, &r168_chain(app, &roots))
    }

    /// 一次读完三条规则（判词 + 证据）。
    ///
    /// `derived` 由调用方传入（`overlays_outside_app(INDEX_HTML)` 的集合）—— 本函数不自己去派生它，
    /// 这样阳性对照可以把**别的**集合喂进来，证明规则不是在常量上「通过」。
    fn r168_read(app: &str, derived: &BTreeSet<String>) -> R168Reading {
        let block = r168_block(app).unwrap_or_default();
        let lines: Vec<&str> = block.lines().collect();
        let typing_at = lines.iter().position(|l| l.contains(R168_TYPING));
        let guard_idx: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.contains(R168_ESCAPE))
            .map(|(i, _)| i)
            .collect();

        // Esc 链能关掉的元素：逐条 Esc 分支各自算一遍，再并起来（这样 R3 可以只取「封锁线之前」那一半）。
        let mut hidden_anywhere: BTreeSet<String> = BTreeSet::new();
        let mut hidden_before_bail: BTreeSet<String> = BTreeSet::new();
        for i in &guard_idx {
            let local = r168_line_hidden_ids(app, lines[*i]);
            hidden_anywhere.extend(local.iter().cloned());
            if typing_at.map(|t| *i < t).unwrap_or(true) {
                hidden_before_bail.extend(local);
            }
        }

        // R1 的期望值来自 index.html 的派生；缺的记下来（判词之外还要能印出「缺了谁」）。
        let missing: BTreeSet<String> = derived.difference(&hidden_anywhere).cloned().collect();

        // R2：块里对**派生元素**下的机制探针（读法）与它们各自关闭器的机制（写法）必须同源。
        let mut probes: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for l in &lines {
            for id in quoted_hash_ids(l) {
                if !derived.contains(&id) {
                    continue;
                }
                if let Some(m) = r168_guard_mechanism(l) {
                    probes.entry(id).or_default().insert(m.to_string());
                }
            }
        }
        // 期望值 = Esc 链上那些「宣称隐藏该元素」的函数的机制；**必须唯一**（多个不同写法 ⇒ 无期望值，
        // 由 R2 的空集保护把它变成红，而不是静默放行）。
        let mut expectations: BTreeMap<String, String> = BTreeMap::new();
        let mut chain_roots: BTreeSet<String> = BTreeSet::new();
        for i in &guard_idx {
            chain_roots.extend(callee_names(lines[*i]));
        }
        let chain = r168_chain(app, &chain_roots);
        for id in derived {
            let mut mechs: BTreeSet<String> = BTreeSet::new();
            for f in &chain {
                let Some(body) = function_source(app, f) else {
                    continue;
                };
                let code = code_lines(&body);
                if hides_element(&code, id) {
                    if let Some(m) = r168_closer_mechanism(&code) {
                        mechs.insert(m.to_string());
                    }
                }
            }
            if mechs.len() == 1 {
                expectations.insert(id.clone(), mechs.into_iter().next().unwrap_or_default());
            }
        }
        let mut mismatches: Vec<String> = Vec::new();
        for (id, read) in &probes {
            let Some(want) = expectations.get(id) else {
                mismatches.push(format!("{id}: 读法存在但链上无唯一写法"));
                continue;
            };
            for m in read {
                if m != want {
                    mismatches.push(format!("{id}: closer writes {want}, guard reads {m}"));
                }
            }
        }

        R168Reading {
            block_lines: lines.len(),
            guards: guard_idx.len(),
            derived: derived.clone(),
            hidden_anywhere,
            hidden_before_bail,
            missing,
            probes,
            expectations,
            mismatches,
            typing_at,
            r1: false,
            r2: false,
            r3: false,
        }
        .finish()
    }

    /// 本轴的派生集合：`ui/index.html` 中 `#app` 之外、带独立 token `hidden` 的顶行元素。
    ///
    /// 与 C2171 同一个派生器（`overlays_outside_app`）—— R1 的对象就是它。
    fn r168_derived() -> BTreeSet<String> {
        overlays_outside_app(INDEX_HTML).into_iter().collect()
    }

    /// 轴：快捷键面板宣传的 Esc 契约必须真的覆盖每个可关闭浮层（R168）。
    ///
    /// 三条规则各自的含义见文件头。本测试只断言「三条同时成立」；每条规则的**牙**由
    /// [`the_r168_rules_have_teeth`] 逐条测量，规则与竞争修法的**关系**由
    /// [`the_r168_rules_separate_the_variants`] 声明。
    #[test]
    fn the_escape_contract_reaches_every_dismissible_overlay() {
        let derived = r168_derived();
        assert!(
            r168_block(APP_JS).is_some(),
            "取不到全局 keydown 块（开头/收尾定界不唯一）—— 三条规则的射程会静默变空"
        );
        let read = r168_read(APP_JS, &derived);
        assert!(
            read.typing_at.is_some(),
            "块里读不到 `{R168_TYPING}` 那一行 —— R3 的封锁线不见了：{}",
            read.report()
        );
        assert!(
            read.guards >= 2,
            "块里数不到 Esc 分支 —— R1 会退化成恒真：{}",
            read.report()
        );
        assert!(
            read.verdicts() == (true, true, true),
            "Esc 契约必须覆盖每个可关闭浮层（R168）：{}",
            read.report()
        );
    }

    /// 阳性对照：三个读取器在**已知为绿**的树上读到东西，在「什么都没有」的树上读到空。
    ///
    /// 这是规则 1/2/3 的射程保险：读取器若只会返回常量，规则就成了恒真断言（空集上的关系式
    /// 会静默通过）。⚠️ 断言只打在**自己拼出来的树**上 —— 真树在两条腿上形状不同（#314）。
    #[test]
    fn the_r168_roster_is_real() {
        let derived = r168_derived();
        // 派生器本身要有阳性对照（与 C2171 同源，这里再钉一次，避免「解析器坏了」被当成修好了）。
        assert!(
            !derived.is_empty()
                && !derived.contains("login-view")
                && !derived.contains("toast-wrap")
                && derived.contains("chat-modal")
                && derived.contains("help-panel"),
            "派生集合的阳性对照失败（解析器变了，还是 index.html 结构变了？）：{derived:?}"
        );

        // 正向：修复树上三条规则全绿，且证据（分支数、探针、期望值、封锁线）全都读到。
        let fixed_tree = r168_variant_fix(APP_JS);
        let fix = r168_read(&fixed_tree, &derived);
        assert!(
            fix.verdicts() == (true, true, true),
            "修复树上三条规则不是全绿 —— 变体构造器坏了：{}",
            fix.report()
        );
        assert!(
            fix.typing_at.is_some() && fix.guards >= 4,
            "修复树上读不到封锁线/Esc 分支：{}",
            fix.report()
        );
        assert!(
            !fix.probes.is_empty() && !fix.expectations.is_empty(),
            "修复树上 R2 的输入是空的 —— 那条规则会恒真：{}",
            fix.report()
        );
        assert!(
            fix.missing.is_empty(),
            "修复树上居然还有关不掉的浮层：{}",
            fix.report()
        );

        // 反向：未修形状上，缺的必须**恰好**是消费对话框（派生出来的，不是抄的）。
        let gone = r168_read(&r168_variant_unfixed(APP_JS), &derived);
        assert_eq!(
            gone.missing,
            [String::from("chat-modal")]
                .into_iter()
                .collect::<BTreeSet<_>>(),
            "未修形状上「关不掉的浮层」不是恰好 `chat-modal` —— R1 的判别式变了：{}",
            gone.report()
        );
        assert!(
            !gone.r1 && gone.r2 && gone.r3,
            "未修形状只该翻 R1 一条：{}",
            gone.report()
        );

        // 机制判别式的正向/反向（合成输入：不碰真文件）。
        assert_eq!(r168_guard_mechanism("!$(\"#x\").hidden"), Some("property"));
        assert_eq!(
            r168_guard_mechanism("!$(\"#x\").classList.contains(\"hidden\")"),
            Some("class")
        );
        assert_eq!(r168_guard_mechanism("if (helpOpen) return;"), None);
        assert_eq!(
            r168_closer_mechanism("$(\"#x\").hidden = true;"),
            Some("property")
        );
        assert_eq!(
            r168_closer_mechanism("$(\"#x\").classList.add(\"hidden\");"),
            Some("class")
        );
        assert_eq!(r168_closer_mechanism("x.textContent = \"hi\";"), None);

        // 闭包不许被单行函数带跑（C2171 坑 #319/#332 的回归面）：Esc 链很短，走不到视图切换/引导渲染器。
        // ⚠️ 基准必须是**自己拼出来的变体树**（`r168_variant_unfixed` 幂等 ⇒ 两条腿同形）。直接读
        // `APP_JS` 会让「未修形状上链里没有 `closeChat`」这条在**修复腿**上测另一棵树 —— 真树在两条腿
        // 上形状不同（#314），而那正是这条断言要说的事。
        let unfixed_tree = r168_variant_unfixed(APP_JS);
        // 每条腿从**自己那棵树**的分支取根（未修树上没有那一行 ⇒ 它的根集合里当然没有 `closeChat`；
        // 拿未修树的根去走修复树，是让这条断言测另一件事）。
        let chain_of = |app: &str| -> BTreeSet<String> {
            let block = r168_block(app).unwrap_or_default();
            let mut roots: BTreeSet<String> = BTreeSet::new();
            for l in block.lines().filter(|l| l.contains(R168_ESCAPE)) {
                roots.extend(callee_names(l));
            }
            r168_chain(app, &roots)
        };
        let chain = chain_of(&unfixed_tree);
        assert!(
            !chain.contains("switchView")
                && !chain.contains("renderTourStep")
                && !chain.contains("startTour"),
            "Esc 链被单行函数带跑（`markTourDone` 那条吞并路径）：{chain:?}"
        );
        // 未修形状上链里没有 `closeChat`（正是缺陷）；修复树上有（闭包跨过那一行 —— 名字不是
        // 标识符的唯一载体，所以这里既断言「有」也断言「不是靠常量碰巧命中」）。
        assert!(
            !chain.contains("closeChat"),
            "未修形状的 Esc 链上居然已经有关闭器 —— 采集器在看别的地方：{chain:?}"
        );
        let fixed_chain = chain_of(&r168_variant_fix(&unfixed_tree));
        assert!(
            fixed_chain.contains("closeChat"),
            "修复树的 Esc 链上没有 `closeChat` —— R1 的射程不成立：{fixed_chain:?}"
        );
    }

    /// 三条规则**各有独立的牙**：合成变异体逐个喂给规则自己的判别式，每个恰好打翻一条。
    ///
    /// 判据是「恰好一条翻转」而不是「至少一条红」—— 否则一条从别处借来红的规则也能自称有牙
    /// （#454：牙齿必须长在该规则的判别式上）。基线是**已知为绿的**修复树（#458）。
    #[test]
    fn the_r168_rules_have_teeth() {
        let derived = r168_derived();
        let fixed_tree = r168_variant_fix(APP_JS);
        let base_read = r168_read(&fixed_tree, &derived);
        assert_eq!(
            base_read.verdicts(),
            (true, true, true),
            "自证基线不绿，牙齿测试没有意义：{}",
            base_read.report()
        );

        // 每个变异体只动一处，期望**恰好一条**翻转（#454）。顺序 = (r1, r2, r3)。
        let mutants = [
            (
                "guard gone",
                r168_variant_unfixed(APP_JS),
                (false, true, true),
            ),
            (
                "guard after the typing bail",
                r168_variant_after_bail(APP_JS),
                (true, true, false),
            ),
            (
                "property probe",
                r168_variant_property(APP_JS),
                (true, false, true),
            ),
        ];
        for (label, tree, expected) in mutants {
            assert_ne!(tree, fixed_tree, "变异体 `{label}` 没有改动树");
            let read = r168_read(&tree, &derived);
            assert_eq!(
                read.verdicts(),
                expected,
                "规则 `{label}` 的牙不成立（期望 {expected:?}）：{}",
                read.report()
            );
        }

        // R1 的第二半：整条「关不掉的浮层」不是只有一个 —— 把**帮助**那一支也摘掉，R1 必须仍然红，
        // 且缺的必须是被摘掉的那一个（证明规则不是只认 `chat-modal` 一个元素）。
        let help_gone = r168_remove_line(&fixed_tree, "&& helpOpen")
            .unwrap_or_else(|| panic!("摘掉帮助分支的锚点漂移了"));
        let help_read = r168_read(&help_gone, &derived);
        assert_eq!(
            help_read.missing,
            [String::from("help-panel")]
                .into_iter()
                .collect::<BTreeSet<_>>(),
            "摘掉帮助分支后，缺的不是恰好 `help-panel` —— 覆盖率的判别式变了：{}",
            help_read.report()
        );
        assert!(
            !help_read.r1,
            "摘掉帮助分支后 R1 仍绿 —— 覆盖率规则在空集上会「通过」：{}",
            help_read.report()
        );

        // R2 的第二半：**读法存在但链上无唯一写法**也必须响亮失败（不许把「没有期望值」当成功）。
        let ambiguous = R168Reading {
            expectations: BTreeMap::new(),
            ..r168_read(&fixed_tree, &derived)
        };
        assert!(
            !ambiguous.finish().r2,
            "期望值为空时 R2 仍绿 —— 那条规则可以静默失明"
        );
    }

    /// 规则与**竞争修法**的关系，逐腿声明（#339/#341：声明的期望与实际各印一列）。
    ///
    /// 竞争修法出自 jsdom 探针 `r168_probe.js`（它按 DOM 实况把它们全部拒掉）：
    /// - `m_after_typing`：守卫挂到 `typing` 守卫**之后** —— 本门禁的 `R3` 拒掉它（探针的 `B1` 也是）。
    /// - `m_property_guard`：把探针写成属性形式（守卫恒真）—— 本门禁的 `R2` 拒掉它（探针的 `D2` 也是）。
    /// - `m_mark_only`：只加一句解释性注释 —— **注释不参与**，判词必须与未修形状逐条相同。
    #[test]
    fn the_r168_rules_separate_the_variants() {
        // ⚠️ 本测试必须在**两腿**都绿（编译门禁分别把真树与 E1 修复树当作 `APP_JS` 来编译）
        // ⇒ 绝对判词只能打在**它自己拼出来的树**上（#314）。
        let derived = r168_derived();
        let all_green = (true, true, true);

        let tree_fix = r168_variant_fix(APP_JS);
        let tree_unfixed = r168_variant_unfixed(APP_JS);
        let tree_after = r168_variant_after_bail(APP_JS);
        let tree_property = r168_variant_property(APP_JS);
        let tree_mark = r168_variant_mark_only(APP_JS);

        // 先证明这些树互不相同，否则「判词不同」可能只是同一棵树的两张脸。
        for (label, other) in [
            ("unfixed", &tree_unfixed),
            ("m_after_typing", &tree_after),
            ("m_property_guard", &tree_property),
            ("m_mark_only", &tree_mark),
        ] {
            assert_ne!(tree_fix, *other, "`{label}` 与修复树逐字相同 —— 变体没落地");
        }
        assert_ne!(
            tree_unfixed, tree_property,
            "两个「缺分支」的变体是同一棵树"
        );

        let declared = [
            ("fix (spliced)", tree_fix.as_str(), all_green),
            ("unfixed", tree_unfixed.as_str(), (false, true, true)),
            ("m_after_typing", tree_after.as_str(), (true, true, false)),
            (
                "m_property_guard",
                tree_property.as_str(),
                (true, false, true),
            ),
        ];
        let mut reports = Vec::new();
        for (name, app, expected) in declared {
            let read = r168_read(app, &derived);
            reports.push(format!("{name}: {}", read.report()));
            assert_eq!(
                read.verdicts(),
                expected,
                "变体 `{name}` 的判词与声明不符（声明 {expected:?}）—— 门禁的鉴别力变了"
            );
        }

        // 注释不参与：`m_mark_only` 的判词必须与未修形状**逐条相同**。
        let marked = r168_read(&tree_mark, &derived);
        let plain = r168_read(&tree_unfixed, &derived);
        reports.push(format!("m_mark_only: {}", marked.report()));
        assert_eq!(
            marked.verdicts(),
            plain.verdicts(),
            "解释性注释改变了判词 —— 注释参与了断言（坑 #296/#309 同族）"
        );

        // 与腿无关的一条关系：真树必须是门禁认识的**两种形状之一**（未修 / 已修）。
        // ⚠️ 不能写成 `== unfixed_v`：本测试在**两腿**都要绿，修好之后那句话会静默反转（#314）。
        let real = r168_read(APP_JS, &derived);
        reports.push(format!("real tree: {}", real.report()));
        assert!(
            real.verdicts() == all_green || real.verdicts() == (false, true, true),
            "真树的形状既不是「未修」也不是「已修」—— 门禁不认识它了：{}",
            real.report()
        );
        // ⚠️ 落地轮必做（#314：默认期望必须钉在**显式基线**上）：真树修好之后，
        // `r168_compile_gate.py` 的 `DECLARED_RED` 表必须从 `base: [AXIS]` 改成 `base: []`，
        // 否则轴测试会为红而仪器仍宣称「未修」。两处一起改。
        println!("{}", reports.join("\n"));
    }

    /// 修复体文本**逐字**来自编辑表 E1；`r168_compile_gate.py` 另外断言它与 E1 的产物是子串关系
    /// （跨制品对账）。这里钉「常量非空、形状齐全、且两个变体构造器是幂等的」。
    #[test]
    fn the_r168_fixed_guard_is_the_edit_sheet_text() {
        assert!(
            R168_FIXED_GUARD.contains(R168_CLASS_PROBE) && R168_FIXED_GUARD.contains("closeChat()"),
            "修复体不是「查 class 机制 + 调 closeChat」那一行：{R168_FIXED_GUARD:?}"
        );
        assert!(
            R168_FIXED_GUARD.contains(R168_ESCAPE),
            "修复体不是一条 Esc 分支：{R168_FIXED_GUARD:?}"
        );
        assert!(
            R168_FIXED_ANCHOR.contains(R168_ESCAPE) && !R168_FIXED_ANCHOR.contains("chat-modal"),
            "插入锚点不是行内新建 Key 的守卫行：{R168_FIXED_ANCHOR:?}"
        );
        // 幂等：两腿拿到的必须是**同一棵**修复树（否则声明表在另一条腿上就不成立了，#612）。
        let once = r168_variant_fix(APP_JS);
        let twice = r168_variant_fix(&once);
        assert_eq!(once, twice, "修复体构造器不幂等 —— 两腿会得到不同的树");
        assert_eq!(
            once.matches(R168_FIXED_GUARD).count(),
            1,
            "修复树里守卫行不是恰好一个"
        );
        assert_ne!(
            r168_variant_unfixed(APP_JS),
            once,
            "未修形状与修复树逐字相同 —— 变体构造器失效了"
        );
        assert_ne!(
            r168_property_guard(),
            R168_FIXED_GUARD,
            "属性形式与修复体逐字相同 —— 竞争修法没落地"
        );
    }

    // ============================ R167 PART B: tests (inside `mod tests`) ============================

    /// 轴：分页器的两个省略号判据必须与它守护的窗口**自洽**（R167）。
    ///
    /// 四条规则各自的含义见文件头。本测试只断言「四条同时成立」；每条规则的**牙**由
    /// [`the_r167_rules_have_teeth`] 逐条测量，规则与竞争修法的**关系**由
    /// [`the_r167_rules_separate_the_variants`] 声明。
    #[test]
    fn the_pager_window_and_its_ellipsis_guards_agree() {
        let read = r167_read(APP_JS);
        assert!(
            r167_body(APP_JS).is_some(),
            "找不到 `{R167_FN}` —— 规则 1/2/3 的射程会静默变空"
        );
        assert!(
            read.a.is_some() && read.b.is_some(),
            "从函数体里读不出窗口半宽 A / B —— 规则 1/2 会退化成恒真（坑 68）：{}",
            read.report()
        );
        assert!(
            read.verdicts() == (true, true, true, true),
            "分页器的省略号判据必须由窗口边界推导（R167）：{}",
            read.report()
        );
    }

    /// 阳性对照：四个读取器在**已知为绿**的树上读到东西，在「什么都没有」的树上读到 `None`。
    ///
    /// 这条测试是规则 1/2 的射程保险：读取器若只会返回常量，规则 1/2 就成了恒真断言
    /// （空集/`None` 上的关系式会静默通过）。
    #[test]
    fn the_r167_roster_is_real() {
        // 正向：修复体上，窗口、判据、省略号、形状、定义点、消费点全都必须读到。
        let fixed_tree = r167_variant_fix(APP_JS);
        let read = r167_read(&fixed_tree);
        assert!(
            r167_body(&fixed_tree).is_some(),
            "修复树上读不出 `{R167_FN}` 的函数体"
        );
        assert!(
            read.a.is_some() && read.b.is_some(),
            "修复体上读不出窗口半宽：{}",
            read.report()
        );
        assert!(
            read.left_guard.is_some() && read.right_guard.is_some(),
            "修复体上读不出省略号判据：{}",
            read.report()
        );
        assert_eq!(
            read.ellipses,
            2,
            "修复体上省略号的压入点不是两个：{}",
            read.report()
        );
        assert!(
            read.small_branch,
            "修复体上少了全量渲染那一支：{}",
            read.report()
        );
        assert_eq!(
            read.definitions,
            1,
            "修复体上 `{R167_FN}` 的定义点不是一个：{}",
            read.report()
        );
        assert!(
            read.consumers >= 1,
            "修复体上 `{R167_FN}` 没有消费点：{}",
            read.report()
        );

        // 反向：窗口与判据一起拿掉，读取器必须**读不到**（否则它们只是常量，不是读数）。
        let gone = r167_read(&r167_variant_show_all(APP_JS));
        assert!(
            gone.a.is_none() && gone.b.is_none(),
            "全量渲染的树里居然读出了窗口半宽 —— 读取器在看别的地方：{}",
            gone.report()
        );
        assert!(
            gone.left_guard.is_none() && gone.right_guard.is_none(),
            "全量渲染的树里居然读出了省略号判据 —— 读取器在看别的地方：{}",
            gone.report()
        );
        assert_eq!(
            gone.ellipses,
            0,
            "全量渲染的树里居然有省略号：{}",
            gone.report()
        );

        // R4 的输入在**真树**上也必须存在，否则那条规则在空集上恒真。
        assert!(
            APP_JS.contains(R167_CONSUMER),
            "唯一消费点 `{R167_CONSUMER}` 不在 `app.js` 里 —— 规则 4 会退化成恒真（坑 68）"
        );
    }

    /// 四条规则**各有独立的牙**：合成变异体逐个喂给规则自己的判别式，每个恰好打翻一条。
    ///
    /// 判据是「恰好一条翻转」而不是「至少一条红」—— 否则一条从别处借来红的规则也能自称有牙
    /// （#454：牙齿必须长在该规则的判别式上）。基线是**已知为绿的**修复体（#458）。
    #[test]
    fn the_r167_rules_have_teeth() {
        let fixed_tree = r167_variant_fix(APP_JS);
        let base_read = r167_read(&fixed_tree);
        assert_eq!(
            base_read.verdicts(),
            (true, true, true, true),
            "自证基线不绿，牙齿测试没有意义：{}",
            base_read.report()
        );

        // 每个变异体只动一处，期望**恰好一条**翻转（#454）。
        let consumer_gone = fixed_tree.replace(R167_CONSUMER, "[]");
        assert_ne!(
            consumer_gone, fixed_tree,
            "变异体 `consumer gone` 没有改动树（锚点 `{R167_CONSUMER}` 漂移了）"
        );
        let mutants = [
            (
                "left guard too tight",
                r167_variant_right_only(APP_JS),
                (false, true, true, true),
            ),
            (
                "right guard too tight",
                r167_variant_left_only(APP_JS),
                (true, false, true, true),
            ),
            (
                "compact branch gone",
                r167_variant_small_gone(APP_JS),
                (true, true, false, true),
            ),
            ("consumer gone", consumer_gone, (true, true, true, false)),
        ];
        for (label, tree, expected) in mutants {
            assert_ne!(tree, fixed_tree, "变异体 `{label}` 没有改动树");
            let read = r167_read(&tree);
            assert_eq!(
                read.verdicts(),
                expected,
                "规则 `{label}` 的牙不成立（期望 {expected:?}）：{}",
                read.report()
            );
        }

        // 规则 3 的第二半：判据还在、窗口还在，但**判据那行不再压省略号**（形状被拆散）。
        // 锚点与替换文本都由 A 推导（不写死 `3`）—— 变体是**推导**出来的，不是抄的。
        let (a, _) = r167_fixed_half_widths();
        let push_line = format!("{R167_GUARD_LEFT}{}) {R167_ELLIPSIS};", a + 2);
        let bare_line = format!("{R167_GUARD_LEFT}{});", a + 2);
        let no_push_body = R167_FIXED_BODY.replace(&push_line, &bare_line);
        assert_ne!(
            no_push_body, R167_FIXED_BODY,
            "变异体 `guard without push` 没有改动函数体（锚点 `{push_line}` 漂移了）"
        );
        let no_push = r167_with_body(APP_JS, &no_push_body);
        assert_ne!(
            no_push, fixed_tree,
            "变异体 `guard without push` 没有改动树"
        );
        let read = r167_read(&no_push);
        assert!(
            !read.r3 && read.r1 && read.r2 && read.r4,
            "判据那行不再压省略号时，只有规则 3 该翻红：{}",
            read.report()
        );

        // 规则 4 的第二半：定义点被改名 ⇒ 读取器连函数体都找不到，四条一起翻红。
        // 本条不是「恰好一条」的牙齿，而是**响亮**的证明：改名不会被静默当成通过。
        let renamed = APP_JS.replace(
            &format!("function {R167_FN}("),
            &format!("function {R167_FN}X("),
        );
        assert_ne!(renamed, APP_JS, "变异体 `renamed definition` 没有改动源码");
        let read = r167_read(&renamed);
        assert_eq!(
            read.verdicts(),
            (false, false, false, false),
            "定义点改名后四条规则必须一起翻红：{}",
            read.report()
        );
    }

    /// 规则与**竞争修法**的关系，逐腿声明（#339/#341：声明的期望与实际各印一列）。
    ///
    /// 竞争修法出自 jsdom 探针 `r167_pager_harness.js`（它按值把它们全部拒掉）：
    /// - `m_wide_window`：把窗口放宽到 `A = B = 2` 并按**同一推导**给出判据 —— **自洽**，故本门禁
    ///   **接受**它，探针的形状腿 `C2`（token 数 ≤ 7）拒掉它。这一格不是漏，是本门禁的射程边界：
    ///   推导关系归门禁，窗口大小归探针。
    /// - `m_left_only` / `m_right_only`：只修一半 —— 各被它没修的那条规则拒掉。
    /// - `m_showall`：窗口与判据一起消失 —— 被规则 1/2/3 拒掉。
    /// - `m_mark_only`：序列不动、只加一句解释性注释 —— **注释不参与**，判词必须与缺陷形状逐条相同。
    #[test]
    fn the_r167_rules_separate_the_variants() {
        // ⚠️ 本测试必须在**两腿**都绿（编译门禁分别把真树与 E1 修复树当作 `APP_JS` 来编译）
        // ⇒ 绝对判词只能打在**它自己拼出来的树**上；对**真树**只能断言「它必须是门禁认识的
        // 两种形状之一」这种与腿无关的关系。
        let unfixed_v = (false, false, true, true);
        let fixed_v = (true, true, true, true);

        let tree_fix = r167_variant_fix(APP_JS);
        let tree_unfixed = r167_variant_unfixed(APP_JS);
        let tree_wide = r167_variant_wide_window(APP_JS);
        let tree_left_only = r167_variant_left_only(APP_JS);
        let tree_right_only = r167_variant_right_only(APP_JS);
        let tree_show_all = r167_variant_show_all(APP_JS);
        let tree_mark = r167_variant_mark_only(APP_JS);

        // 先证明这些树互不相同，否则「判词不同」可能只是同一棵树的两张脸。
        assert_ne!(
            tree_fix, tree_unfixed,
            "修复体与缺陷形状逐字相同 —— 变体没落地"
        );
        assert_ne!(
            tree_fix, tree_wide,
            "`m_wide_window` 没有落地（锚点漂移了）"
        );
        assert_ne!(tree_fix, tree_left_only, "`m_left_only` 没有落地");
        assert_ne!(tree_fix, tree_right_only, "`m_right_only` 没有落地");
        assert_ne!(tree_fix, tree_show_all, "`m_showall` 没有落地");
        assert_ne!(
            tree_left_only, tree_right_only,
            "两个「只修一半」的变体是同一棵树"
        );
        assert_ne!(tree_mark, tree_unfixed, "注释变形体与缺陷形状是同一棵树");

        let declared = [
            ("fix (spliced)", tree_fix.as_str(), fixed_v),
            // 自洽但更宽：门禁接受，探针 C2 拒掉 —— 本门禁的射程边界，不是漏。
            ("m_wide_window", tree_wide.as_str(), fixed_v),
            ("unfixed", tree_unfixed.as_str(), unfixed_v),
            (
                "m_left_only",
                tree_left_only.as_str(),
                (true, false, true, true),
            ),
            (
                "m_right_only",
                tree_right_only.as_str(),
                (false, true, true, true),
            ),
            (
                "m_showall",
                tree_show_all.as_str(),
                (false, false, false, true),
            ),
        ];
        let mut reports = Vec::new();
        for (name, app, expected) in declared {
            let read = r167_read(app);
            reports.push(format!("{name}: {}", read.report()));
            assert_eq!(
                read.verdicts(),
                expected,
                "变体 `{name}` 的判词与声明不符（声明 {expected:?}）—— 门禁的鉴别力变了"
            );
        }

        // 注释不参与：`m_mark_only` 的判词必须与缺陷形状**逐条相同**。
        let marked = r167_read(&tree_mark);
        let plain = r167_read(&tree_unfixed);
        reports.push(format!("m_mark_only: {}", marked.report()));
        assert_eq!(
            marked.verdicts(),
            plain.verdicts(),
            "解释性注释改变了判词 —— 注释参与了断言（坑 #296/#309 同族）"
        );

        // 与腿无关的一条关系：真树必须是门禁认识的**两种形状之一**（未修 / 已修）。
        // ⚠️ 不能写成 `== unfixed_v`：本测试在**两腿**都要绿，修好之后那句话会静默反转（#314）。
        let real = r167_read(APP_JS);
        reports.push(format!("real tree: {}", real.report()));
        assert!(
            real.verdicts() == unfixed_v || real.verdicts() == fixed_v,
            "真树的形状既不是「未修」也不是「已修」—— 门禁不认识它了：{}",
            real.report()
        );
        // ⚠️ 落地轮必做（#314：默认期望必须钉在**显式基线**上）：真树修好之后，
        // `r167_compile_gate.py` 的 `DECLARED_RED` 表必须从 `base: [AXIS]` 改成 `base: []`，
        // 否则轴测试会为红而仪器仍宣称「未修」。两处一起改，否则门禁与仪器会各说一套。
        println!("{}", reports.join("\n"));
    }

    /// 修复体文本**逐字**来自编辑表 E1；`r167_compile_gate.py` 另外断言它与 E1 的产物是子串
    /// 关系（跨制品对账）。这里钉「常量非空、形状齐全、且判据确实由窗口推导」。
    #[test]
    fn the_r167_fixed_body_is_the_edit_sheet_text() {
        assert!(
            R167_FIXED_BODY.contains(R167_SMALL_BRANCH),
            "修复体少了全量渲染那一支：{R167_FIXED_BODY:?}"
        );
        assert!(
            R167_FIXED_BODY.contains(R167_WINDOW_LEFT)
                && R167_FIXED_BODY.contains(R167_WINDOW_RIGHT),
            "修复体里没有那行窗口循环 —— 规则 1/2 无从推导"
        );
        assert_eq!(
            R167_FIXED_BODY.matches(R167_ELLIPSIS).count(),
            2,
            "修复体里省略号的压入点不是两个"
        );
        let (a, b) = r167_fixed_half_widths();
        let lg = r167_int_after(R167_FIXED_BODY, R167_GUARD_LEFT)
            .unwrap_or_else(|| panic!("修复体里读不出左侧判据"));
        let rg = r167_int_after(R167_FIXED_BODY, R167_GUARD_RIGHT)
            .unwrap_or_else(|| panic!("修复体里读不出右侧判据"));
        assert_eq!(
            (lg, rg),
            (a + 2, b + 1),
            "修复体的两个判据必须从窗口推导（A={a} B={b}）"
        );
        assert_ne!(
            R167_FIXED_BODY.trim_end_matches('\n'),
            r167_variant_unfixed_body(),
            "缺陷形状与修复体逐字相同 —— 变体构造器失效了"
        );
    }

    // =============================== END OF R167 GATE FRAGMENT ==================================

    /// R164 轴：三处载体（请求 / 载荷签名 / 本地排序守卫）＋ 服务端白名单必须**同源**，
    /// 且同一条路的前一半（列筛选后端化）必须仍然是真的。
    #[test]
    fn the_sort_indicator_and_the_order_by_share_one_source() {
        let r = r164_read(APP_JS, R164_WALLET);
        println!("R164 real tree: {}", r.report());
        assert!(
            r.r1,
            "列表请求没有携带排序状态（或趋势请求也带了）：projectors={:?}",
            r.projectors
        );
        assert!(
            r.r2,
            "载荷签名没覆盖排序状态 —— 点列头只改状态、守卫不重拉，箭头动了而列表不动"
        );
        assert!(
            r.r3,
            "本地排序没有为「声明了服务端排序的表」让路：guard_flag={:?}",
            r.guard_flag
        );
        assert!(
            r.r4a,
            "白名单与列名册不是同一份名单：columns={:?} whitelist={:?}",
            r.columns, r.whitelist
        );
        assert!(
            r.r4b,
            "`tx_sort_expr` 的臂与白名单不同源：arms={:?} whitelist={:?}",
            r.arms, r.whitelist
        );
        assert!(
            r.r4c,
            "`TX_SORT_KEYS` 声明的长度（{}）与字面量个数（{}）不一致",
            r.declared_len,
            r.whitelist.len()
        );
        assert!(
            r.r4d,
            "`ORDER BY` 不是由白名单守卫的构造器渲染：placeholder={:?} binder={:?} param_lines={:?}",
            r.placeholder, r.binder, r.param_lines
        );
        assert!(
            r.r5,
            "阳性对照失效：列筛选那一半（serverFilter / txFilterParams / TxColFilters / 签名守卫）不见了 \
             —— 扫描器的正射程为空时，四条规则都在说没有"
        );
    }

    /// R164 阳性对照：列名册与白名单都必须是**真的**（声明的地面真值 —— 漂移时响亮失败，
    /// 而不是让「白名单 == 名册」在空集上静默恒真）。
    #[test]
    fn the_r164_roster_is_real() {
        let app = r164_code(APP_JS);
        let cols = r164_columns_region(&app).expect("`TX_COLUMNS = [` 必须还在（名册无从派生）");
        let keys = r164_keyed_literals(&cols, "key");
        let want: Vec<String> = R164_COLUMNS.iter().map(|s| s.to_string()).collect();
        assert_eq!(
            keys, want,
            "`TX_COLUMNS` 的可排序列漂移了 —— 白名单与名册必须一起改（不是只改一边）"
        );
        let (declared, list) = r164_whitelist(R164_WALLET).expect("`TX_SORT_KEYS` 必须还在");
        assert_eq!(declared, list.len(), "声明的长度必须等于字面量个数");
        assert_eq!(
            r164_sorted(&list),
            r164_sorted(&keys),
            "白名单必须与列名册逐键相同"
        );
        let arms = r164_arm_keys(R164_WALLET);
        assert_eq!(
            r164_sorted(&arms),
            r164_sorted(&keys),
            "匹配臂必须覆盖同一份白名单"
        );
    }

    /// R164 牙齿：每一条规则都有一副**只翻它自己**的牙（合成迷你源码，自足于真树）。
    #[test]
    fn the_r164_rules_have_teeth() {
        // (0) 阳性基线：迷你修复体必须五条全绿，否则下面的「变红」什么也证明不了。
        let base = r164_read(&r164_mini_fixed(), &r164_mini_fixed_wallet());
        assert!(
            base.carriers_agree() && base.r5,
            "迷你修复体本身就不绿：{}",
            base.report()
        );

        // (1) R1 —— 列表请求不再带排序参数。
        let app = r164_swap(&r164_mini_fixed(), " + txSortParams();", ";");
        let r = r164_read(&app, &r164_mini_fixed_wallet());
        assert!(
            !r.r1 && !r.carriers_agree() && r.r2 && r.r3 && r.r4d && r.r5,
            "R1 的牙不独立：{}",
            r.report()
        );

        // (2) R1 —— 反向：趋势请求也带上了排序参数（行序对时间桶没有语义）。
        let app = r164_swap(
            &r164_mini_fixed(),
            "(cols ? \"&\" + cols : \"\");\n  return [q, tq];",
            "(cols ? \"&\" + cols : \"\") + txSortParams();\n  return [q, tq];",
        );
        let r = r164_read(&app, &r164_mini_fixed_wallet());
        assert!(
            !r.r1 && r.r2 && r.r3 && r.r4a && r.r4d && r.r5,
            "R1 没挡住「趋势也带排序」：{}",
            r.report()
        );

        // (3) R2 —— 载荷签名丢掉排序状态。
        let app = r164_swap(
            &r164_mini_fixed(),
            "  const srt = (txTable.sort || []).map((s) => s.key).join(\",\");\n  return \"x|\" + srt;\n",
            "  return \"x\";\n",
        );
        let r = r164_read(&app, &r164_mini_fixed_wallet());
        assert!(
            !r.r2 && r.r1 && r.r3 && r.r4a && r.r4b && r.r4c && r.r4d && r.r5,
            "R2 的牙不独立：{}",
            r.report()
        );

        // (4) R3 —— 守卫回到「一律本地排序」。
        let app = r164_swap(
            &r164_mini_fixed(),
            "if (state.sort.length && !(serverPaging && serverSort)) {",
            "if (state.sort.length) {",
        );
        let r = r164_read(&app, &r164_mini_fixed_wallet());
        assert!(
            !r.r3 && r.r1 && r.r2 && r.r4a && r.r4b && r.r4c && r.r4d && r.r5,
            "R3 没读到完整条件：{}",
            r.report()
        );

        // (5) R4a —— 只把**列名册**改掉（白名单与臂仍彼此一致）。
        let app = r164_swap(&r164_mini_fixed(), "{ key: \"f\"", "{ key: \"z\"");
        let r = r164_read(&app, &r164_mini_fixed_wallet());
        assert!(
            !r.r4a && r.r4b && r.r4c && r.r4d && r.r1 && r.r2 && r.r3 && r.r5,
            "R4a 的牙不独立：{}",
            r.report()
        );

        // (6) R4b —— 只给 `tx_sort_expr` 加一个白名单外的臂。
        let wallet = r164_swap(
            &r164_mini_fixed_wallet(),
            "        _ => return None,",
            "        \"z\" => \"t.z\".to_string(),\n        _ => return None,",
        );
        let r = r164_read(&r164_mini_fixed(), &wallet);
        assert!(
            !r.r4b && r.r4a && r.r4c && r.r4d && r.r1 && r.r5,
            "R4b 的牙不独立：{}",
            r.report()
        );

        // (7) R4c —— 只把声明的长度改错。
        let wallet = r164_swap(&r164_mini_fixed_wallet(), "[&str; 6]", "[&str; 7]");
        let r = r164_read(&r164_mini_fixed(), &wallet);
        assert!(
            !r.r4c && r.r4a && r.r4b && r.r4d && r.r1 && r.r5,
            "R4c 的牙不独立：{}",
            r.report()
        );

        // (8) R4d —— `ORDER BY` 回到字面量（用户串不再经白名单守卫）。
        let wallet = r164_swap(
            &r164_mini_fixed_wallet(),
            "ORDER BY {order} LIMIT",
            "ORDER BY t.id DESC LIMIT",
        );
        let r = r164_read(&r164_mini_fixed(), &wallet);
        assert!(
            !r.r4d && r.r4a && r.r4b && r.r4c && r.r1 && r.r2 && r.r3 && r.r5,
            "R4d 的牙不独立：{}",
            r.report()
        );

        // (9) R4d —— 反向：查询参数在**绑定之外**又被采一次（白名单被绕过）。
        let wallet = r164_swap(
            &r164_mini_fixed_wallet(),
            "    let sql = format!(\"SELECT 1 WHERE {where_sql} ORDER BY {order} LIMIT ?{} OFFSET ?{}\", n + 1, n + 2);",
            "    let sql = format!(\"SELECT 1 WHERE {where_sql} ORDER BY {order} LIMIT ?{} OFFSET ?{}\", n + 1, n + 2);\n    let raw = format!(\"{}\", q.dir.as_deref().unwrap_or(\"\"));",
        );
        let r = r164_read(&r164_mini_fixed(), &wallet);
        assert!(
            !r.r4d && r.r4a && r.r4b && r.r4c && r.r5,
            "R4d 没挡住「参数在绑定之外又被采一次」：{}",
            r.report()
        );

        // (10) R5（对照）—— 丢掉 `#[serde(flatten)]`：只有对照翻红，四条规则不受影响
        //      （证明对照是**独立**的一条腿，不是四条规则的同义反复）。
        let wallet = r164_swap(&r164_mini_fixed_wallet(), "    #[serde(flatten)]\n", "");
        let r = r164_read(&r164_mini_fixed(), &wallet);
        assert!(!r.r5 && r.carriers_agree(), "R5 的牙不独立：{}", r.report());

        // (11) 提取器的形态牙：`sortVal:`/`key_name` 不是 `key: "…"`，兄弟标识符不算证据。
        assert_eq!(
            r164_keyed_literals("  { sortVal: (t) => t.x, key: \"k\" }", "key"),
            vec!["k".to_string()],
            "标识符左界失效（`sortVal` 被当成了 `key`）"
        );
        assert!(
            !mentions_identifier("  txTable.sortBy = [];", R164_SORT_STATE),
            "兄弟标识符被当成了排序状态（#333）"
        );
    }

    // =============================== END OF R164 GATE FRAGMENT ==================================

    /// 轴：口令下限**用「位」宣告、用「字节」执行**（R96）。
    ///
    /// 四条规则的含义见 `PassMinRead` 的文档注释。本测试只断言「四条同时成立」；
    /// 每条规则的**牙**由 [`the_pass_min_rules_have_teeth`] 逐条测量。
    #[test]
    fn the_password_minimum_is_counted_in_the_unit_its_message_names() {
        let read = pass_min_read(PASS_MIN_MOD_RS, APP_JS, I18N_JS, PASS_MIN_PROTO);
        assert_eq!(
            read.carriers.len(),
            4,
            "「宣告」那族必须恰好四张脸（原型 / zh / en / ERR_MAP）—— 少一张会让规则 3 假绿：{}",
            read.report()
        );
        assert!(
            read.declared().is_some(),
            "从设计基线原型里读不出注册占位符 ⇒ 「宣告」的锚点没了（坑 #537）：{}",
            read.report()
        );
        assert_eq!(
            read.verdicts(),
            (true, true, true, true),
            "口令下限必须按它自己宣告的单位来数（R96）：{}",
            read.report()
        );
    }

    /// 阳性对照：读数不是空的。
    ///
    /// 空集会把「不许出现」变成恒真（坑 68 家族）；一个从不返回绿的判据等于没有判据。
    /// ⚠️ 本测试**刻意只断言在两棵树上都成立的事**（四张脸可读、站点集合非空、切片正确），
    /// 不碰「修复体才有的形状」（常量 / helper）—— 那些是轴测试的题眼，写在这里只会让
    /// 「未修树红在哪条腿上」不可判（R96 记录 §E 的 A/B 声明）。
    #[test]
    fn the_pass_min_roster_is_real() {
        let read = pass_min_read(PASS_MIN_MOD_RS, APP_JS, I18N_JS, PASS_MIN_PROTO);
        assert_eq!(
            read.carriers.iter().map(|(l, _, _)| *l).collect::<Vec<_>>(),
            PASS_MIN_FACES.to_vec(),
            "四张脸的登记名与派生结果不一致（名册是快照，派生才是真源）"
        );
        for (label, text, rule) in &read.carriers {
            assert!(!text.is_empty(), "「{label}」读出来是空的 —— 那条腿恒真");
            assert!(rule.is_some(), "「{label}」读不出 N/单位：{text:?}");
        }
        assert!(
            read.agreed().is_some(),
            "四张脸读不出一个共同的 (N, 单位) —— 规则 3 会因另一个理由红：{}",
            read.report()
        );
        assert!(!read.server_sites.is_empty(), "一个服务端站点都读不到");
        assert!(!read.client_sites.is_empty(), "一个客户端守卫都读不到");
        // 射程保险：`pass_min_prod` 真的截掉了测试区（否则本门禁的夹具自己会顶破规则 4）
        assert!(
            !pass_min_prod(PASS_MIN_MOD_RS).contains("#[cfg(test)]"),
            "生产区切片没截到 `#[cfg(test)]` —— 规则 1/4 会把测试里的调用点也数进去"
        );
        assert!(
            pass_min_prod("no test module here\n").contains("no test module"),
            "没有测试区时切片必须原样返回全文（不是空串）"
        );
    }

    /// 四条规则**各有独立的牙**：每个变异体只动一处，期望**恰好翻掉它那一条**。
    ///
    /// 基线是**已知为绿的**落地体（坑 #458：在已知为红的基线上测牙齿没有意义）。
    #[test]
    fn the_pass_min_rules_have_teeth() {
        let base = pass_min_read(PASS_MIN_MOD_RS, APP_JS, I18N_JS, PASS_MIN_PROTO);
        assert_eq!(
            base.verdicts(),
            (true, true, true, true),
            "自证基线不绿，牙齿测试没有意义：{}",
            base.report()
        );

        // (a) 服务端半修：把注册那一处换回字节口径 —— 只翻规则 1（站点仍是三处）
        let mod_byte = PASS_MIN_MOD_RS.replace(
            "    if password_too_short(&req.password) {",
            "    if req.password.len() < 8 {",
        );
        let r = pass_min_read(&mod_byte, APP_JS, I18N_JS, PASS_MIN_PROTO);
        assert_eq!(
            r.verdicts(),
            (false, true, true, true),
            "字节站点必须只翻规则 1：{}",
            r.report()
        );

        // (b) 客户端退回 UTF-16 口径（星号面 / emoji 一个字符算 2）—— 只翻规则 2
        let app_bare = APP_JS.replace(
            "if (Array.from(pw).length < MIN_PW_CHARS)",
            "if (pw.length < 8)",
        );
        let r = pass_min_read(PASS_MIN_MOD_RS, &app_bare, I18N_JS, PASS_MIN_PROTO);
        assert_eq!(
            r.verdicts(),
            (true, false, true, true),
            "裸 `pw.length <` 必须只翻规则 2：{}",
            r.report()
        );

        // (c) 竞争修法 `m_reword`：保住字节口径，把 zh 包改说「字节」—— 只翻规则 3
        let i18n_reword = I18N_JS.replace(
            "\"err.weakPassword\": \"密码至少 8 位\"",
            "\"err.weakPassword\": \"密码至少 8 字节\"",
        );
        assert_ne!(
            i18n_reword, I18N_JS,
            "zh 包的弱口令条目没被改到 —— 这条腿会假绿"
        );
        let r = pass_min_read(PASS_MIN_MOD_RS, APP_JS, &i18n_reword, PASS_MIN_PROTO);
        assert_eq!(
            r.verdicts(),
            (true, true, false, true),
            "改宣告（保住字节口径）必须只翻规则 3：{}",
            r.report()
        );

        // (d) 竞争修法 `m_drop_client`：删掉客户端守卫，让两边「不再矛盾」—— 只翻规则 4
        //
        // ⚠️ 针**不得包含那一行说的话**（键）：R93 的 E1 把这一行的 `register.err.pass` 换成
        // `err.weakPassword`，含键的针在修复树上就成了**不动的** replace —— 本腿当场假绿，而下一行
        // 的 `assert_ne!` 正是为这种事准备的。所以按**守卫**整行删除：针里只有条件，没有那句话。
        let app_drop = {
            let guard = "if (Array.from(pw).length < MIN_PW_CHARS)";
            let kept: Vec<&str> = APP_JS.lines().filter(|l| !l.contains(guard)).collect();
            kept.join("\n")
        };
        assert_ne!(app_drop, APP_JS, "客户端守卫那一行没被删掉 —— 这条腿会假绿");
        let r = pass_min_read(PASS_MIN_MOD_RS, &app_drop, I18N_JS, PASS_MIN_PROTO);
        assert_eq!(
            r.verdicts(),
            (true, true, true, false),
            "删站点必须只翻规则 4：{}",
            r.report()
        );
    }

    /// 轴：趋势图的聚合粒度由**实际请求窗口**决定，不由控件值决定（R165）。
    ///
    /// 四条规则各自的含义见文件头。本测试只断言「四条同时成立」；每条规则的**牙**由
    /// [`the_r165_rules_have_teeth`] 逐条测量，规则与竞争修法的**关系**由
    /// [`the_r165_rules_separate_the_variants`] 声明。
    #[test]
    fn the_trend_grain_derives_from_the_query_not_from_the_control() {
        let read = r165_read(APP_JS, INDEX_HTML);
        assert!(
            r165_body(APP_JS).is_some(),
            "找不到 `{R165_GRAIN_FN}` —— 规则 1/2/3 的射程会静默变空"
        );
        assert!(
            !read.options.is_empty(),
            "从 `ui/index.html` 派生不出 `#tx-range` 的选项集 —— 规则 1 会退化成恒真（坑 68）"
        );
        assert!(
            read.verdicts() == (true, true, true, true),
            "趋势聚合粒度必须由请求窗口决定，不由控件值决定（R165）：{}",
            read.report()
        );
    }

    /// 阳性对照：选项集是**派生**的，且与登记表逐字相同。
    ///
    /// 这条测试是规则 1 的射程保险：`ui/index.html` 的选项集一变，它先红 —— 否则规则 1
    /// 的命令集过期而「不许出现」照样通过（空集恒真）。
    #[test]
    fn the_r165_roster_is_real() {
        let options = r165_options(INDEX_HTML);
        assert_eq!(
            options,
            r165_expected_options(),
            "`#tx-range` 的选项集与登记表不一致：派生的是 {options:?}"
        );
        for o in R165_OPTIONS {
            assert!(
                INDEX_HTML.contains(&format!("value=\"{o}\"")),
                "登记表里的 {o:?} 在 `ui/index.html` 里找不到 —— 名册是快照，派生才是真源"
            );
        }
        let body = r165_body(APP_JS).expect("找不到粒度函数");
        assert!(
            !body.trim().is_empty(),
            "粒度函数体读出来是空的 —— 提取器的证据就没了（#332 同族）"
        );
        assert!(
            !r165_grains(&body).is_empty(),
            "这个函数体里一个 `return \"…\"` 都没有 ⇒ 规则 3 的集合断言会假绿"
        );
    }

    /// 四条规则**各有独立的牙**：合成变异体逐个喂给规则自己的判别式，每个恰好打翻一条。
    ///
    /// 判据是「恰好一条翻转」而不是「至少一条红」—— 否则一条从别处借来红的规则也能自称有牙
    /// （#454：牙齿必须长在该规则的判别式上）。基线是**已知为绿的**修复体（#458）。
    #[test]
    fn the_r165_rules_have_teeth() {
        let fixed = r165_variant_fix(APP_JS);
        let base_read = r165_read(&fixed, INDEX_HTML);
        assert_eq!(
            base_read.verdicts(),
            (true, true, true, true),
            "自证基线不绿，牙齿测试没有意义：{}",
            base_read.report()
        );

        // 每个变异体只动一处，期望**恰好一条**翻转（#454：牙齿必须长在该规则的判别式上）。
        let mutants = [
            (
                "control literal",
                r165_variant_control(APP_JS),
                (false, true, true, true),
            ),
            (
                "no delegation",
                r165_variant_nocall(APP_JS),
                (true, false, true, true),
            ),
            (
                "extra grain",
                r165_variant_grain(APP_JS),
                (true, true, false, true),
            ),
        ];
        for (label, tree, expected) in mutants {
            assert_ne!(tree, fixed, "变异体 `{label}` 没有改动树");
            let read = r165_read(&tree, INDEX_HTML);
            assert_eq!(
                read.verdicts(),
                expected,
                "规则 `{label}` 的牙不成立（期望 {expected:?}）：{}",
                read.report()
            );
        }

        // 规则 4 的牙在**源码级**：窗口真源被改名 ⇒ 只有规则 4 翻红
        // （规则 2 看的是体内**提到**这个名字，仍在）。
        let renamed = APP_JS.replace(
            &format!("function {R165_WINDOW_SOURCE}("),
            &format!("function {R165_WINDOW_SOURCE}X("),
        );
        assert_ne!(renamed, APP_JS, "变异体 `renamed source` 没有改动源码");
        let read = r165_read(&r165_variant_fix(&renamed), INDEX_HTML);
        assert_eq!(
            read.verdicts(),
            (true, true, true, false),
            "规则 4（窗口真源恰一处 + 控件仍在）的牙不成立：{}",
            read.report()
        );

        // 规则 4 的第二半：控件没了 —— 规则 1 会退化成恒真，所以必须由规则 4 拦下。
        let no_control = INDEX_HTML.replace("<select id=\"tx-range\"", "<select id=\"tx-rangeX\"");
        assert_ne!(
            no_control, INDEX_HTML,
            "变异体 `control removed` 没有改动 HTML"
        );
        let read = r165_read(&fixed, &no_control);
        assert!(
            read.options.is_empty() && !read.r4,
            "控件消失时规则 4 必须翻红（否则规则 1 在空集上恒真 —— 坑 68）：{}",
            read.report()
        );
        assert!(
            read.r2,
            "控件消失不该影响规则 2（它只看函数体）：{}",
            read.report()
        );
    }

    /// 规则与**竞争修法**的关系，逐腿声明（#339/#341：声明的期望与实际各印一列）。
    ///
    /// 三条竞争修法都出自 jsdom 探针 `r165_probe.js`（它按值把它们全部拒掉）：
    /// - `m_day`：把「无下界」那一支改回 `day` —— **形状与修复体逐字同类**（仍委派、仍无控件
    ///   选项值、粒度集合不变）⇒ 本门禁**接受**它，探针的 `K3`（`all` 控制腿）拒掉它。
    ///   这一格不是漏，是本门禁的射程边界：形状归门禁，取值归探针。
    /// - `m_hour_start` / `m_hide`：改的是**别处**（窗口真源 / 趋势请求），粒度函数的形状没动
    ///   ⇒ 与未修树同判，被规则 1、2 拒掉。
    #[test]
    fn the_r165_rules_separate_the_variants() {
        // ⚠️ 本测试必须在**两腿**都绿（编译门禁分别把真树与 E1 修复树当作 `APP_JS` 来编译）
        // ⇒ 绝对判词只能打在**它自己拼出来的树**上；对**真树**只能断言「它必须是门禁认识的
        // 两种形状之一」这种与腿无关的关系。第一版把「真树未修」写成了断言，于是修复腿一编译
        // 就红 —— 门禁在量「这棵树是谁」，而不是「形状对不对」。
        let unfixed = (false, false, true, true);
        let fixed = (true, true, true, true);

        let tree_fix = r165_variant_fix(APP_JS);
        let tree_day = r165_variant_day(APP_JS);
        assert_ne!(
            tree_day, tree_fix,
            "`m_day` 没有落地（锚点 `if (!s) return \"week\";` 漂移了）"
        );

        // 两条「改别处」的竞争修法：粒度函数的形状没动 ⇒ 判词必须与**真树**逐条相同。
        let tree_hour_start = APP_JS.replace(
            "if (txCustomStart) start = new Date(txCustomStart);",
            "if (txCustomStart) start = new Date(txCustomStart);\n      \
             if (!start && !end) start = new Date(now - MS(24));",
        );
        assert_ne!(
            tree_hour_start, APP_JS,
            "`m_hour_start` 没有落地（锚点漂移了）"
        );
        let tree_hide = APP_JS.replace(
            "api.get(tq).catch(() => null),",
            "(txRange === \"custom\" ? Promise.resolve(null) \
             : api.get(tq).catch(() => null)),",
        );
        assert_ne!(tree_hide, APP_JS, "`m_hide` 没有落地（锚点漂移了）");

        // 先证明这些树互不相同，否则「判词不同」可能只是同一棵树的两张脸。
        assert_ne!(tree_fix, tree_day);
        assert_ne!(APP_JS, tree_fix);
        assert_ne!(tree_hour_start, tree_hide);
        assert_ne!(tree_hour_start, APP_JS);
        assert_ne!(tree_hide, APP_JS);

        // 绝对判词（全部打在自造树上，与腿无关）。
        let tree_control = r165_variant_control(APP_JS);
        let tree_nocall = r165_variant_nocall(APP_JS);
        let tree_grain = r165_variant_grain(APP_JS);
        assert_ne!(tree_control, tree_fix);
        assert_ne!(tree_nocall, tree_fix);
        assert_ne!(tree_grain, tree_fix);
        let declared = [
            ("fix (spliced)", tree_fix.as_str(), fixed),
            // 形状同类、取值不同：门禁接受，探针 K3 拒掉 —— 本门禁的射程边界，不是漏。
            ("m_day", tree_day.as_str(), fixed),
            (
                "m_control",
                tree_control.as_str(),
                (false, true, true, true),
            ),
            ("m_nocall", tree_nocall.as_str(), (true, false, true, true)),
            ("m_grain", tree_grain.as_str(), (true, true, false, true)),
        ];
        let mut reports = Vec::new();
        for (name, app, expected) in declared {
            let read = r165_read(app, INDEX_HTML);
            reports.push(format!("{name}: {}", read.report()));
            assert_eq!(
                read.verdicts(),
                expected,
                "变体 `{name}` 的判词与声明不符（声明 {expected:?}）—— 门禁的鉴别力变了"
            );
            assert_eq!(
                read.options,
                r165_expected_options(),
                "变体 `{name}` 的选项集不该变：{:?}",
                read.options
            );
        }

        // 与腿无关的两条关系。
        let real = r165_read(APP_JS, INDEX_HTML);
        assert!(
            real.verdicts() == unfixed || real.verdicts() == fixed,
            "真树的形状既不是「未修」也不是「已修」—— 门禁不认识它了：{}",
            real.report()
        );
        for (name, tree) in [("m_hour_start", &tree_hour_start), ("m_hide", &tree_hide)] {
            let read = r165_read(tree, INDEX_HTML);
            reports.push(format!("{name}: {}", read.report()));
            assert_eq!(
                read.verdicts(),
                real.verdicts(),
                "`{name}` 改的是别处，粒度函数的形状没动 ⇒ 必须与真树同判"
            );
        }
        // ⚠️ 落地轮已做（#314：默认期望必须钉在**显式基线**上）：真树修好之后，「未修 ⇒ 轴测试红」
        // 这条期望会**静默反转** —— 本文件不再在任何腿上观察它，而是由落地轮的编译器门禁在
        // 一棵**显式基线树**（`git archive <落地前的 HEAD>` 物化，同 R158 / #588）上观察，
        // 否则「基线腿」在已修的真树上会变成同义反复。表的声明与本节脚注一起改，否则各说一套。
        // （仪器住在仓外，故此处不写文件名 —— #606。）
        println!("real tree: {}\n{}", real.report(), reports.join("\n"));
    }

    /// 修复体文本**逐字**来自编辑表 E1；`r165_compile_gate.py` 另外断言它与 E1 的产物
    /// 是子串关系（跨制品对账）。这里只钉「常量非空且不是占位符」。
    #[test]
    fn the_r165_fixed_body_is_the_edit_sheet_text() {
        assert!(
            R165_FIXED_BODY.contains(R165_WINDOW_SOURCE),
            "修复体必须委派给窗口真源：{R165_FIXED_BODY:?}"
        );
        assert!(
            R165_FIXED_BODY.contains("return \"week\";"),
            "修复体的「无下界 ⇒ 最粗粒度」那一支不见了"
        );
        assert!(
            !R165_FIXED_BODY.contains("txRange === "),
            "修复体里还留着控件值比较 —— 那不是修复"
        );
    }

    /// R158：交易视图里**同一份 token 数量只许有一种拼写**，且导出写精确值。
    ///
    /// 四条规则各有独立的牙（`the_r158_rules_separate_the_variants` 逐条证）：
    /// - R1 卡片不自带拼写；R2 卡片委派给 `fmtTokens`（改前树两条都红）
    /// - R3 导出四列写 `<…>Raw` 数字、走悬停那层的 helper（`m1`「只修合计列」红在这里）
    /// - R4 四个单元格仍是缩写（`m2`「把屏幕降级成精确值」红在这里）
    #[test]
    fn the_transactions_view_states_one_token_quantity_one_way() {
        let r = r158_reading(APP_JS);

        // 规则 1/2：卡片那一半。诊断里带上卡片代码的长度，是为了让「提取器没找到卡片」
        // 与「卡片真的合规」在输出上可区分（空体 = 提取失效，而不是通过）。
        assert!(
            r.card_len > 0,
            "没提取到汇总卡 `renderTxSummary` 的代码体（长度 0）⇒ 下面两条断言会在空体上通过。\
             提取器或函数改名了。{}",
            r.verdicts()
        );
        assert!(
            r.r1,
            "汇总卡自带了一份紧凑拼写（代码里出现 \"K\"/\"M\" 后缀字面量，卡片代码 {} 字符）⇒\
             卡片的 K 档与它正下方那些行不是同一条规则（1500 → 卡片 \"2K\"、行 \"1.5K\"）。\
             卡片的 token 取值必须**委派**给单元格那条拼写（`fmtTokens`）。{}",
            r.card_len,
            r.verdicts()
        );
        assert!(
            r.r2,
            "汇总卡的代码里没有出现 `fmtTokens` ⇒ 它没有委派给单元格那条拼写。{}",
            r.verdicts()
        );

        // 规则 3：导出那一半。
        assert!(
            r.csv_len > 0,
            "没找到 `exportTxCsv` 里逐行拼列的那一行（长度 0）⇒ R3 会在空串上通过。{}",
            r.verdicts()
        );
        assert!(
            r.csv_display_used.is_empty(),
            "CSV 的 token 列仍在写**显示串** {:?} —— 那是给人看的缩写（\"1.5K\"），\
             写进数据列后 Excel 既不能求和也不能透视，而精确值在文件里没有任何出口。\
             必须改读同一行的数字字段并走悬停那一层的 helper。{}",
            r.csv_display_used,
            r.verdicts()
        );
        assert!(
            r.csv_raw_missing.is_empty(),
            "CSV 的 token 列没有读这些数字字段 {:?}（字段名由 `…Tokens` → `…Raw` 派生）。{}",
            r.csv_raw_missing,
            r.verdicts()
        );
        assert!(
            !r.exact_helpers.is_empty(),
            "没能从悬停构造器（`title=\"` 那个箭头里赋值给 `exact` 的一行）派生出一致的精确值 helper ⇒\
             规则 3 的第三半个判据没有尺子。{}",
            r.verdicts()
        );
        assert!(
            r.exact_calls >= R158_TOKEN_FIELDS.len(),
            "CSV 只调用了精确值 helper {:?} {} 次，而 token 列有 {} 列 ⇒ 漏掉了某几列（这正是\
             「只修合计列」那类半修的形状）。{}",
            r.exact_helpers,
            r.exact_calls,
            R158_TOKEN_FIELDS.len(),
            r.verdicts()
        );

        // 规则 4（反向）：别把屏幕降级成精确值来让文件显得对。rant 2026-08-22T08:58:54 要的
        // 正是屏幕上的 K/M 缩写 + 悬停取精确值。
        assert_eq!(
            r.cells,
            R158_TOKEN_FIELDS.len(),
            "token 单元格的 `render:` 行数不是 {}（找到 {}）⇒ 表格形状变了或扫描器失效。{}",
            R158_TOKEN_FIELDS.len(),
            r.cells,
            r.verdicts()
        );
        assert!(
            r.r4,
            "四个 token 单元格不再印缩写的显示字段 ⇒ 屏幕被降级成了精确值（「让导出显得对」的\
             过度纠正）。屏幕口径是 rant 2026-08-22T08:58:54 明确要的 K/M 缩写。{}",
            r.verdicts()
        );
    }

    /// 阳性对照（坑 68 家族）：名册里的四个显示字段**真的**是视图模型的字段。
    ///
    /// 门禁的名册是最容易腐烂的载体：字段一改名，R3/R4 会在**空集**上通过。这条断言把
    /// 名册钉在被测代码上 —— 每个显示字段及其派生的数字字段都必须作为**对象字面量的键**
    /// 出现在 `txsToView` 的返回里。
    #[test]
    fn the_r158_roster_is_real() {
        let body = r158_fn_body(APP_JS, "txsToView");
        assert!(!body.is_empty(), "没提取到 `txsToView`（改名了？）");
        for f in R158_TOKEN_FIELDS {
            let raw = r158_raw_field(f);
            let declares = |name: &str| {
                body.lines().any(|l| {
                    let t = l.trim();
                    t.starts_with(&format!("{name}: ")) || t == format!("{name},")
                })
            };
            assert!(
                declares(f),
                "名册里的显示字段 `{f}` 不在 `txsToView` 的返回对象里 ⇒ R3/R4 的射程已空"
            );
            assert!(
                declares(&raw),
                "显示字段 `{f}` 派生的数字字段 `{raw}` 不在 `txsToView` 的返回对象里 ⇒\
                 「导出写数字」这条规则读不到东西"
            );
        }
    }

    /// 扫描器自证（合成语料，不依赖仓库当前内容）：每条判别式都得能**翻面**。
    ///
    /// 覆盖四类真实踩过的坑：①只剥 `//` 的注释剥离会被解释性文字绊倒；②单行箭头不许吞掉
    /// 下一个函数；③函数体不许停在嵌套块的 `}` 上（会静默截断掉证据）；④`t.tokens` 不许在
    /// `t.tokensRaw` 里命中。
    #[test]
    fn the_r158_scanners_have_teeth() {
        // ① 注释里提到 fmtTokens 不算「委派」：注释剥离必须覆盖注释行。
        let commented = "  function renderTxSummary(list) {\n    // 卡片这里本该用 fmtTokens\n    const fmtM = (n) => (n >= 1000 ? Math.round(n / 1000) + \"K\" : String(n));\n  }\n";
        let r = r158_reading(commented);
        assert!(
            !r.r2,
            "注释里的 `fmtTokens` 被当成了委派证据（注释剥离失效）"
        );

        // ③ 函数体必须在**恰好 `  }`** 处收尾：嵌套块先收尾时截断会切掉证据。
        let nested = "  function renderTxSummary(list) {\n    if (list) {\n      x();\n    } else {\n      y();\n    }\n    const fmtM = (n) => (n >= 1000 ? Math.round(n / 1000) + \"K\" : String(n));\n  }\n";
        assert!(
            !r158_reading(nested).r1,
            "函数体停在了嵌套块的收尾括号上、把自带的拼写切掉了（截断不长得像错误）"
        );

        // ② 单行箭头不许吞掉紧随其后的多行函数。
        let single = "  const fmtCtx = (n) => String(n);\n  function renderTxSummary(list) {\n    const fmtM = (n) => (n >= 1000 ? Math.round(n / 1000) + \"K\" : String(n));\n  }\n";
        assert_eq!(
            r158_arrow_body(single, "fmtCtx").lines().count(),
            1,
            "单行箭头吞掉了下一个函数"
        );
        // 任意缩进都要能找到（tooltip 构造器在 `txsToView` 里缩进 6 格）。
        let deep = "  function outer() {\n      const brkTitle = (l, v) => {\n        const exact = typeof v === \"number\" ? fmtTokensExact(v) : \"0\";\n        return \"x\";\n      };\n  }\n";
        assert_eq!(
            r158_exact_helpers(deep),
            vec!["fmtTokensExact".to_string()],
            "深缩进的 tooltip 构造器没被找到（提取器写死了缩进）"
        );

        // ④ 标识符边界：`t.tokens` 不得在 `t.tokensRaw` 里命中。
        let boundary = "    const lines = list.map((t) => [fmtTokensExact(t.tokensRaw)].map(cell).join(\",\"));";
        assert!(
            !mentions_identifier(boundary, "t.tokens"),
            "`t.tokens` 在 `t.tokensRaw` 里命中了（子串被当成了兄弟标识符，坑 #333）"
        );
    }

    /// A/B：四条规则必须**各自**有独立的牙 —— 每个变异只许翻面**一条**规则。
    ///
    /// 变体从**修好之后的树**派生（`r158_fix` 幂等），所以在改前树与修复树上都成立：它证的是
    /// 「规则能认出缺陷」，不是「此刻这棵树有缺陷」。
    ///
    /// 形状取自 DOM 仪器 `r158_probe.js` 的四条竞争修法（它那边已被 60 条腿拒绝）：
    ///   defect           卡片自带拼写 + 导出写显示串（本轴）→ R1 R2 R3 红
    ///   fixed            卡片委派 + 导出写 `<…>Raw`           → 全绿
    ///   m1_partial_csv   只修合计列                           → 只 R3 红
    ///   m2_degrade_screen 把屏幕降级成精确值                   → 只 R4 红
    ///   m3_card_plain_int 卡片改成纯整数（不是同一条拼写）     → 只 R2 红
    #[test]
    fn the_r158_rules_separate_the_variants() {
        let fixed = r158_fix(APP_JS);
        let defect = r158_unfix(&fixed);
        // 锚点自证：两条锚点在两棵树上至少要各命中一条，否则 `fix`/`unfix` 是空转，
        // 下面的变体会**退化成同一个字符串**而这条测试仍然「通过」。
        assert_ne!(
            defect, fixed,
            "`fix`/`unfix` 没有真正改写任何东西（锚点漂移了）"
        );
        assert!(
            APP_JS.contains(FMTM_DEFECTIVE) || APP_JS.contains(FMTM_DELEGATING),
            "卡片上的 `fmtM` 定义两条锚点都不命中"
        );
        assert!(
            APP_JS.contains(CSV_DISPLAY_GROUP) || APP_JS.contains(CSV_RAW_GROUP),
            "CSV 的 token 列组两条锚点都不命中"
        );

        // 每个变异只动一处，且只该翻它针对的那一条规则。
        let m1 = fixed.replace(CSV_RAW_GROUP, CSV_PARTIAL_GROUP);
        let m2 = R158_TOKEN_FIELDS.iter().fold(fixed.clone(), |acc, f| {
            let raw = r158_raw_field(f);
            acc.replace(
                &format!("+ t.{f} +"),
                &format!("+ fmtTokensExact(t.{raw}) +"),
            )
        });
        let m3 = fixed.replace(
            FMTM_DELEGATING,
            "const fmtM = (n) => String(Math.round(n));",
        );

        let cases: [(&str, &str, [bool; 4]); 5] = [
            ("defect", &defect, [false, false, false, true]),
            ("fixed", &fixed, [true, true, true, true]),
            ("m1_partial_csv", &m1, [true, true, false, true]),
            ("m2_degrade_screen", &m2, [true, true, true, false]),
            ("m3_card_plain_int", &m3, [true, false, true, true]),
        ];
        for (name, src, want) in cases {
            let r = r158_reading(src);
            let got = [r.r1, r.r2, r.r3, r.r4];
            assert_eq!(
                got,
                want,
                "变体 `{name}` 的读数与声明不符：{}（声明 R1={} R2={} R3={} R4={}）\
                 —— 四条规则若不能各自翻面，它们就只是同一句话的四种说法",
                r.verdicts(),
                want[0] as u8,
                want[1] as u8,
                want[2] as u8,
                want[3] as u8
            );
        }
    }

    // ============================== PART B: tests (inside `mod tests`) ==========================

    /// 表单控件的 property 式 `disabled` 必须在**回收路径**上被清除（R139）。
    ///
    /// 三条规则各有独立的牙：
    /// - 规则 1（**阳性对照**）：控件写者集合非空。空集上的集合断言会假绿（坑 68），而且「把
    ///   `cb.disabled = allCb.checked` 整句删掉」这种**取消互斥**的竞争修法正是先让这里变空
    ///   —— 探针实测它被 `{C3, C2, A3}` 拒绝（「每天」还勾着而周三已被取消 ⇒ 快捷勾选开始说谎）。
    /// - 规则 2：每个频道都能在**回收闭包**里找到同频道的清除。改前树在这里红（缺陷就在）。
    /// - 规则 3（**防矫枉过正**）：回收路径的宿主仍然被调用 —— 否则「把 `reset()` 一起删掉」
    ///   也能让规则 2 变绿。
    #[test]
    fn a_form_control_disabled_by_the_property_is_cleared_on_the_recycle_path() {
        let r = r139_reading(APP_JS, INDEX_HTML);

        // ── 规则 1（阳性对照 + 派生自证）────────────────────────────────────────────────────
        assert!(
            !r.channels.is_empty(),
            "没扫到任何「表单控件被 property 式禁用」的写点 ⇒ 下面的断言会在空集上通过。\
         本轴靠 `$(\"… input\")` 的双引号选择器派生频道；改动控件写法时扫描器要一起改。"
        );
        assert_eq!(
            r.lead_form.as_deref(),
            Some("share-form"),
            "频道 {:?} 没有派生出一个 `<form>` id ⇒ 控件归属/卡片派生都断了（今天应为 share-form）",
            r.channels
        );
        assert_eq!(
            r.card.as_deref(),
            Some("share-form-card"),
            "没有从 `<form id=\"share-form\">` 之前派生出卡片 id ⇒ 开表单那一半的射程会静默变空"
        );
        assert!(
            r.reset_fn.is_some(),
            "找不到承载 `form.reset()` 的函数 ⇒ 表单回收路径不存在（或声明区间提取器失效）"
        );
        assert!(
        !r.open_fns.is_empty(),
        "找不到「把表单卡片打开」的函数 ⇒ 轨道派生失效（它由 index.html 的卡片 id 推出，不写名册）"
    );

        // ── 规则 2（不变量）──────────────────────────────────────────────────────────────────
        assert!(
            r.witness.is_some(),
            "表单控件被 property 式禁用（{:?}），但**回收闭包**里没有任何一处把它清回 `false`。\
         `form.reset()` 只还原「值 / 勾选态」、不清这个 property ⇒ 成功上架一次之后这些控件\
         永久冻死（R139 实测：勾着「每天」上架成功后七枚星期 chip 点不动，整会话不自愈）。\
         回收闭包 = `{}` 的闭包 ∪ 开表单函数 {:?} 的闭包 = {:?}",
            r.channels,
            r.reset_fn.as_deref().unwrap_or("?"),
            r.open_fns,
            r.closure
        );

        // ── 规则 3（反向：别把回收路径一起删掉）─────────────────────────────────────────────
        let reset_fn = r.reset_fn.clone().expect("规则 2 的前置已断言它存在");
        assert!(
            r.callers.iter().any(|f| f != &reset_fn),
            "`{reset_fn}` 不再被任何函数调用（callers={:?}）⇒ 表单回收路径被删掉了",
            r.callers
        );
    }

    /// 合成输入自证：四条判别式各有各的牙（R139 门禁的**可杀性**）。
    ///
    /// ⚠️ 语料用 `r##"…"##` 而不能用 `r#"…"#`：样本里有 `$("#f-days .chip input")`，
    /// `"#` 会把 `r#"…"#` 提前闭合（坑 #342）。
    #[test]
    fn the_form_recycle_path_scanners_have_teeth() {
        // 语料自带一个 `<form id="f">` 与承载它的 `<div … id="f-card">`。
        let html = r##"<div class="card" id="f-card"><form id="f"><span id="f-days"><input value="1"></span><input id="f-all"></form></div>"##;

        // (a) 未修：控件被 property 禁用，回收路径上**没有**清除 ⇒ 规则 2 开口
        let base = r##"
function bind() {
  const showForm = () => { $("#f-card").hidden = false; };
  const allCb = $("#f-all input");
  if (allCb) allCb.addEventListener("change", () => {
    $$("#f-days .chip input").forEach((cb) => {
      if (cb !== allCb) { cb.checked = allCb.checked; cb.disabled = allCb.checked; }
    });
  });
  $("#f").addEventListener("submit", (e) => {
    const afterOk = () => { e.target.reset(); };
    afterOk();
  });
}
"##;
        let r = r139_reading(base, html);
        assert!(
            !r.channels.is_empty(),
            "合成语料：频道派生失败（{:?}），后面的断言会在空集上通过",
            r.channels
        );
        assert_eq!(
            r.reset_fn.as_deref(),
            Some("afterOk"),
            "合成语料：回收宿主判错"
        );
        assert_eq!(
            r.open_fns,
            vec!["showForm".to_string()],
            "合成语料：开表单函数判错"
        );
        assert!(
            r.witness.is_none(),
            "合成语料是**未修**形状，却找到了清除见证 {witness:?} ⇒ 规则 2 没有牙",
            witness = r.witness
        );

        // (b) 修在回收闭包上（helper 被 `afterOk` 调用）⇒ 规则 2 闭嘴
        let fixed = r##"
function bind() {
  const showForm = () => { $("#f-card").hidden = false; };
  const resetAvail = () => { $$("#f-days .chip input").forEach((cb) => { cb.disabled = false; }); };
  const allCb = $("#f-all input");
  if (allCb) allCb.addEventListener("change", () => {
    $$("#f-days .chip input").forEach((cb) => {
      if (cb !== allCb) { cb.checked = allCb.checked; cb.disabled = allCb.checked; }
    });
  });
  $("#f").addEventListener("submit", (e) => {
    const afterOk = () => { e.target.reset(); resetAvail(); };
    afterOk();
  });
}
"##;
        let rf = r139_reading(fixed, html);
        assert_eq!(
            rf.witness.as_deref(),
            Some("resetAvail"),
            "合成语料：修在回收路径上却找不到见证 ⇒ 规则 2 会把正确修法判红"
        );

        // (c) 只清在「每天」勾选处理器里（闭包外）⇒ 规则 2 仍红（最诱人的半修）
        let handler_only = r##"
function bind() {
  const showForm = () => { $("#f-card").hidden = false; };
  const allCb = $("#f-all input");
  if (allCb) allCb.addEventListener("change", () => {
    $$("#f-days .chip input").forEach((cb) => {
      if (cb !== allCb) { cb.checked = allCb.checked; cb.disabled = allCb.checked; }
      else { $$("#f-days .chip input").forEach((c2) => { c2.disabled = false; }); }
    });
  });
  $("#f").addEventListener("submit", (e) => {
    const afterOk = () => { e.target.reset(); };
    afterOk();
  });
}
"##;
        assert!(
            r139_reading(handler_only, html).witness.is_none(),
            "把清除放在「每天」处理器里（不在回收闭包上）却被判合格 ⇒ 规则 2 认错了地方"
        );

        // (d) 删掉互斥写入（竞争修法 `m_drop`）⇒ 规则 1 开口
        let dropped = base.replace(
            "if (cb !== allCb) { cb.checked = allCb.checked; cb.disabled = allCb.checked; }",
            "if (cb !== allCb) { cb.checked = allCb.checked; }",
        );
        assert!(
            r139_reading(&dropped, html).channels.is_empty(),
            "删掉 property 式禁用之后频道集合仍非空 ⇒ 规则 1 的阳性对照认错了东西"
        );

        // (e) `input` 必须按 **CSS 标签**匹配：`#chat-input` 是兄弟标识符，不是表单控件
        assert!(
            !selects_input_tag("#chat-input") && selects_input_tag("#f-days .chip input"),
            "`input` 的子串匹配把 `#chat-input` 判成了控件频道（坑 #333 同族）"
        );

        // (f) 归属必须取**最内层**声明（否则 `bind` 的闭包是整个语料，规则 2 恒真）
        let spans = decl_spans(base);
        let reset_at = base.find(".reset()").expect("语料里有 e.target.reset()");
        assert_eq!(
            span_owner(&spans, reset_at).as_deref(),
            Some("afterOk"),
            "`form.reset()` 的归属不是最内层声明 ⇒ 闭包会缩水/膨胀（坑 #336）"
        );
        let single_line = "  const showForm = () => { $(\"#f-card\").hidden = false; };\n  const after = () => { other(); };\n";
        assert_eq!(
            span_body(single_line, &decl_spans(single_line), "showForm")
                .map(|b| b.contains("other()")),
            Some(false),
            "**单行**箭头函数被吞进了下一个函数（坑 #319 / #332）"
        );
    }

    // ============================ END OF R139 GATE FRAGMENT ============================

    /// 交易视图的缓存槽只有一个写者，且它就是写下该槽有效性证据的那个函数。
    ///
    /// 两条断言各有各的牙：
    /// - `writers == {loadTransactions}`：挡住「仪表盘为自己一个数字也来写这个槽」（C2130 的形状）；
    /// - `writers == holders`：挡住「换了写者，却仍让守卫拿旧写者的账本当证据」
    ///   （即使把写者搬去新函数，只要证据不跟着搬，这条依然红）。
    #[test]
    fn the_transaction_cache_slot_has_one_writer_and_that_writer_records_the_evidence() {
        let writers = owners_of(APP_JS, |l| writes_slot(l, TX_SLOT));
        let holders = owners_of(APP_JS, writes_evidence);

        // ── 前置：扫描器必须真的看得见东西（空集上的集合断言会假绿，坑 68）───────────────
        assert!(
            !writers.is_empty() && !holders.is_empty(),
            "扫描器返回空集 ⇒ 后面的集合断言会在空集上「通过」：writers={writers:?} holders={holders:?}"
        );

        // 写者名册（改动缓存归属时，这里与守卫的证据要一起改）
        assert_eq!(
            writers,
            BTreeSet::from(["loadTransactions".to_string()]),
            "`Live.{TX_SLOT}` 的写者不止交易视图的加载器。守卫（{TX_EVIDENCE}*）只认 \
             `loadTransactions` 那本账 —— 别的写者的载荷会被当成「本视图的数据」渲染。\
             实测写点：{:?}",
            witnesses(APP_JS, |l| writes_slot(l, TX_SLOT))
        );

        // ── 不变量：写槽的人 == 记证据的人（守卫比的就是那三项）───────────────────────
        //
        // C2170 起，身份边界（会话结束 / 建立）是**复位者**，不是第二个生产者：它清掉证据
        // （`txTable.loaded*`）——证据是守卫信任的东西；槽本身由 `resetSessionCaches()` 的通用擦除负责
        // （`Object.keys(Live).forEach(k => Live[k] = null)`，`transactions` 是 `Live` 字面量的字段）。
        // 所以证据的写者合法地比槽的写者多一个；多出来的那个必须在**边界闭包里** ——
        // 名册**派生**自边界闭包，不维护第二份手写名单。上面那条名册（槽的生产者）**逐字节不动**。
        let boundary: BTreeSet<String> = boundary_reset(APP_JS).1.into_iter().collect();
        assert!(
            writers.is_subset(&holders),
            "写了 `Live.{TX_SLOT}` 却不记 `{TX_EVIDENCE}*`：缓存内容与守卫手上的证据脱钩。             writers={writers:?} holders={holders:?}"
        );
        assert!(
            holders
                .difference(&writers)
                .all(|h| boundary.contains(h)),
            "`{TX_EVIDENCE}*` 的写者既不是装载器、也不在身份边界闭包里：             {:?} —— 在 `renderTransactions()` 里清会把守卫每次渲染都重新武装 ⇒ 请求风暴（C2146 同形）。             实测证据写点：{:?}",
            holders.difference(&writers).collect::<Vec<_>>(),
            witnesses(APP_JS, writes_evidence)
        );
    }

    /// 仪表盘把「交易笔数」留在自己的槽里：那一查是**另一种查询**，不能住进交易视图的缓存。
    ///
    /// 两半都断言：**不得**读写交易视图的槽；**必须**写自己的槽、且那条查询还在
    /// （否则「把这次请求整段删掉」这种「修法」能让「仪表盘不再污染缓存」变绿，
    /// 而卡片上的笔数永远是 0）。
    #[test]
    fn the_dashboard_keeps_the_transaction_count_in_its_own_slot() {
        let dash =
            code_only(js_function_body(APP_JS, "loadDashboard").expect("找不到 loadDashboard()"));
        assert!(
            dash.contains(DASH_TX_QUERY),
            "阳性对照失败：仪表盘不再拉那条「只取 total」的查询了？\n{dash}"
        );
        assert!(
            dash.contains(&format!("Live.{DASH_SLOT}")),
            "仪表盘必须把笔数写进自己的槽（`Live.{DASH_SLOT}`）\n{dash}"
        );
        assert!(
            !writes_slot(&dash, TX_SLOT),
            "仪表盘的加载器**写**了交易视图的缓存槽 `Live.{TX_SLOT}` —— C2130 的缺陷形状\n{dash}"
        );

        let render = code_only(
            js_function_body(APP_JS, "renderDashboard").expect("找不到 renderDashboard()"),
        );
        assert!(
            render.contains(&format!("Live.{DASH_SLOT}")),
            "仪表盘渲染必须读自己的槽（`Live.{DASH_SLOT}`）\n{render}"
        );
        assert!(
            !render.contains(&format!("Live.{TX_SLOT}")),
            "仪表盘渲染又去读交易视图的缓存槽了 —— 那是「另一种查询」的载荷\n{render}"
        );

        // 槽必须先被声明（`Live` 的字段表就是它的名片）
        assert!(
            APP_JS.contains(&format!("{DASH_SLOT}: null,")),
            "`Live` 字段表里没有 `{DASH_SLOT}` —— 槽位没有单一的声明处"
        );
        // 且声明处那条注释要点明它不是交易视图的缓存
        let decl = APP_JS
            .lines()
            .find(|l| l.contains(&format!("{DASH_SLOT}: null,")))
            .unwrap_or_default();
        assert!(
            decl.contains("loadTransactions") || decl.contains("仪表盘"),
            "`Live.{DASH_SLOT}` 的声明没说清它属于仪表盘、与交易视图的缓存不同：{decl}"
        );
    }

    /// `renderView` 的每个分支都必须「既渲染又拉取」—— 它是唯一允许「先同步渲染缓存、再异步
    /// 拉取」的地方，于是「只渲染不拉取」的分支就是**永远显示缓存**的分支。
    /// C2132 实测：八个分支里 `wallet` 是唯一只 `renderWallet()` 的 ⇒ 钱包单元格从不变新。
    #[test]
    fn the_view_router_renders_and_loads_in_every_branch() {
        let body = code_only(js_function_body(APP_JS, "renderView").expect("找不到 renderView()"));
        let branches = view_router_branches(&body);

        // 提取器自证：必须停在 `renderView` 自己的收尾处（`switchView` 里也有 `id === "…"`
        // 形状的守卫；吞进它会把非分支行算成分支）
        assert!(
            !body.contains("function renderDashboard("),
            "提取器吞掉了紧随其后的 `function renderDashboard(`：\n{body}"
        );

        // ── 前置：分支必须扫到（空集上循环体不会执行 ⇒ 假绿，坑 68）─────────────────
        assert!(
            branches.len() >= 5,
            "只扫到 {} 个视图分支：{branches:?} —— 提取器或分支判别式坏了",
            branches.len()
        );

        for b in &branches {
            assert!(
                b.contains("render"),
                "视图分支没有渲染（首帧会空白）？\n{b}"
            );
            assert!(
                b.contains("load"),
                "视图分支只渲染不拉取 —— 它会永远显示缓存（C2132 的钱包缺陷形状）；\
                 每个分支都要有 `if (loggedIn()) load…()`。\n{b}"
            );
        }

        // ── 合成输入：缺 loader 的分支必须变红 ────────────────────────────────────
        let no_loader = concat!(
            "    if (id === \"dashboard\") { renderDashboard(); if (loggedIn()) loadDashboard(); }\n",
            "    else if (id === \"wallet\") renderWallet();\n"
        );
        let scanned = view_router_branches(no_loader);
        assert_eq!(scanned.len(), 2, "分支扫描在合成输入上不对：{scanned:?}");
        assert!(
            scanned[0].contains("render") && scanned[0].contains("load"),
            "阳性对照失败：正常分支的两半没被认出：{}",
            scanned[0]
        );
        assert!(
            !scanned[1].contains("load"),
            "合成对照失败：缺 loader 的分支竟被认成有 loader：{}",
            scanned[1]
        );
    }

    /// 提取器自证：两个函数体都必须停在**本函数**的收尾处，不能吞掉紧随其后的函数。
    #[test]
    fn the_body_extractor_stops_at_the_right_place() {
        let dash = js_function_body(APP_JS, "loadDashboard").expect("找不到 loadDashboard()");
        assert!(
            dash.contains("renderDashboard()"),
            "提取到的 loadDashboard 体不含它自己调用的 renderDashboard()：\n{dash}"
        );
        assert!(
            !dash.contains("function stat("),
            "提取器吞掉了紧随其后的 `function stat(`（收尾行判定错了）：\n{dash}"
        );

        let render = js_function_body(APP_JS, "renderDashboard").expect("找不到 renderDashboard()");
        assert!(
            render.contains("dash-sharings"),
            "提取到的 renderDashboard 体不含它渲染的 `#dash-sharings`：\n{render}"
        );
        assert!(
            !render.contains("function dashTrendDays("),
            "提取器吞掉了紧随其后的 `function dashTrendDays(`：\n{render}"
        );
    }

    /// 合成输入对照：换一种写者形状时两条断言必须**变红**（否则它们测的不是宣称的东西），
    /// 且「读」不得被误判成「写」。
    #[test]
    fn the_scanners_have_teeth_on_a_second_writer() {
        // 合成的「仪表盘也来写这个槽」形状（即 C2130 改前的代码；参数与真源码逐字同形）
        let two_writers = concat!(
            "  async function loadTransactions() {\n",
            "    Live.transactions = await liveLoad(\"transactions\", q);\n",
            "    txTable.loadedPage = txTable.page;\n",
            "  }\n\n",
            "  async function loadDashboard() {\n",
            "    Live.transactions = await api.get(\"/api/transactions?page=1&page_size=1\");\n",
            "    Live.tradeCount = 7;\n",
            "  }\n"
        );
        let w = owners_of(two_writers, |l| writes_slot(l, TX_SLOT));
        let e = owners_of(two_writers, writes_evidence);
        assert_eq!(
            w,
            BTreeSet::from(["loadTransactions".to_string(), "loadDashboard".to_string()]),
            "归属错了：每一行必须归到它**之前最近声明**的函数"
        );
        assert_eq!(
            e,
            BTreeSet::from(["loadTransactions".to_string()]),
            "证据持有者归属错：{e:?}"
        );
        assert_ne!(w, e, "合成输入上第一条不变量未被触发 —— 那条断言没有牙齿");

        // 阴性对照：`Live.transactions` 的**读取**（含就地改字段、比较）都不是写
        let readers = concat!(
            "  function renderTransactions() {\n",
            "    if (Live.transactions) Live.transactions.trend = null;\n",
            "    let list = Live.transactions.items || [];\n",
            "    let same = Live.transactions === null;\n",
            "  }\n"
        );
        assert!(
            !writes_slot(readers, TX_SLOT),
            "阴性对照失败：把读取当成写者了"
        );
        // 阴性对照：守卫对证据的**比较**不是「持有证据」
        let guard = "    if (Live.transactions && (txTable.loadedPage !== txTable.page)) {\n";
        assert!(
            !writes_evidence(guard),
            "阴性对照失败：把守卫的证据**比较**当成证据**写入**了 —— \
             那会让 `renderTransactions` 也算证据持有者，不变量永远为假"
        );
        // 阳性对照：真正的证据写入必须被认出（否则上面的阴性对照可能只是判别式全假）
        let write = "      txTable.loadedFilterSig = txFilterSig();\n";
        assert!(
            writes_evidence(write),
            "阳性对照失败：证据写入没被认出 —— 判别式坏成恒假"
        );
        // 阳性对照：注释行提到标识符不算命中
        let comment = "    // 它曾写进 `Live.transactions` —— 坏的形状\n";
        assert!(
            !writes_slot(comment, TX_SLOT),
            "注释行被当成代码了（`code_only` 或判别式串了）"
        );
    }

    /// 身份边界（会话建立 / 会话结束）必须丢弃**每一个** `Live` 槽，且清空方式必须**派生自**
    /// 对象字面量，而不是第二份手抄名册。
    ///
    /// 三条断言各有各的牙：
    /// - `Object.keys(Live)`：挡住手抄名册（漏一个槽就静默留下一份上一位用户的载荷）；
    /// - 名册检测（体里不得出现 `Live.<slot> =`）：挡住「派生了但顺手又抄了一份」；
    /// - 两个边界都调用：挡住「清了缓存但忘了在某条身份路径上清」（登出与 boot/登录是两条路）。
    #[test]
    fn the_identity_boundaries_drop_every_session_cache() {
        let slots = live_slots(APP_JS);

        // ── 前置：派生出的槽名必须非空，否则后面的断言都在空集上假绿（坑 68）─────────────
        assert!(
            slots.len() >= 5,
            "从 `const Live = {{ … }}` 派生的槽名太少（{}）：{slots:?} —— 派生器可能没停对地方",
            slots.len()
        );

        let reset = code_only(
            js_function_body(APP_JS, "resetSessionCaches").expect("找不到 resetSessionCaches()"),
        );
        // 提取器自证：必须停在本函数的收尾处（否则下面「不得逐个槽赋值」的断言会被后面的
        // 函数绊出假红/假绿）
        assert!(
            !reset.contains("function loggedIn("),
            "提取器吞掉了紧随其后的 `function loggedIn(`：\n{reset}"
        );

        // ── 不变量 A：清空方式是派生的（自动覆盖每一个槽）─────────────────────────────
        assert!(
            reset.contains("Object.keys(Live)"),
            "清空缓存的函数没有从 `Live` 字面量派生槽名 —— 手抄的名册会在新增槽时静默漏掉。\n{reset}"
        );

        // ── 不变量 B：不得在手抄名册（体里不许出现逐个槽的赋值）──────────────────────
        let roster: Vec<String> = slots
            .iter()
            .filter(|s| writes_slot(&reset, s))
            .cloned()
            .collect();
        assert!(
            roster.is_empty(),
            "清空缓存的函数里出现了逐个槽的赋值（{roster:?}）—— 那就是第二份名册，会腐烂。\n{reset}"
        );

        // ── 不变量 C：两条身份路径都必须清（会话建立 + 会话结束）──────────────────────
        for boundary in ["loadSession", "exitGuest"] {
            let body = code_only(
                js_function_body(APP_JS, boundary).unwrap_or_else(|| panic!("找不到 {boundary}()")),
            );
            assert!(
                body.contains("resetSessionCaches()"),
                "身份边界 `{boundary}()` 没有清空 `Live` 缓存 —— 它会把上一位用户的载荷留在槽里。\n{body}"
            );
        }

        // ── 合成输入：判别式有牙齿 ────────────────────────────────────────────────
        // (a) 手抄名册：不含派生式 ⇒ 不变量 A 会红
        let roster_reset = concat!(
            "  function resetSessionCaches() {\n",
            "    Live.wallet = null;\n",
            "    Live.models = null;\n",
            "  }\n"
        );
        assert!(
            !roster_reset.contains("Object.keys(Live)"),
            "合成对照失败：手抄名册被认成了派生式"
        );
        // (b) 名册检测器必须真的认得出逐个槽的赋值（否则不变量 B 恒真）
        assert!(
            writes_slot(roster_reset, "wallet") && writes_slot(roster_reset, "models"),
            "阳性对照失败：名册检测器认不出 `Live.wallet = null;`（不变量 B 没有牙齿）"
        );
        // (c) 派生式不得被误判成名册（`Live[k]` 是动态键，不该命中字面槽）
        let derived_reset =
            "  function resetSessionCaches() {\n    Object.keys(Live).forEach((k) => { Live[k] = null; });\n  }\n";
        assert!(
            derived_reset.contains("Object.keys(Live)"),
            "阳性对照失败：派生式没被认出"
        );
        assert!(
            !writes_slot(derived_reset, "wallet"),
            "阴性对照失败：`Live[k] = null` 被当成了 `Live.wallet = …`"
        );
        // (d) `live_slots` 要能从合成字面量里派生字段
        let synthetic_literal = "  const Live = {\n    a: null,\n    b: null,\n  };\n";
        assert_eq!(
            live_slots(synthetic_literal),
            vec!["a".to_string(), "b".to_string()],
            "合成输入上 `live_slots` 的派生结果不对"
        );
    }

    /// C2135：**渲染谁就装载谁** —— 每个视图分支的 loader 闭包必须装载该分支渲染闭包读到的每个槽。
    ///
    /// 反例（实测）：钱包视图渲染 `#month-changes`（读 `Live.dashboard`），而它的 loader 只刷
    /// `Live.wallet` ⇒ 会话在钱包视图上建立时那一格永远是空的（详见本文件头部）。
    ///
    /// `loadSession` 的闭包算**所有**分支的写者：`models` / `publicUrl` 是会话级数据，
    /// 由它装载、各视图共用。有了这一条，全仓**零豁免清单**。
    #[test]
    fn every_view_branch_loads_each_slot_its_renderer_reads() {
        let src = code_only(APP_JS);
        let slots = all_live_slots(&src);
        let graph = call_graph(&src);
        let rv = js_function_body(&src, "renderView").expect("找不到 renderView()");
        let branches = view_router_branches(rv);

        // ── 前置：提取器必须真的看见东西（空集上的断言会假绿，坑 68）───────────────────
        assert!(
            slots.len() >= 8 && graph.len() >= 50,
            "槽宇宙/调用图太小（slots={} funcs={}）—— 提取器坏了",
            slots.len(),
            graph.len()
        );
        assert!(
            branches.len() >= 5,
            "只扫到 {} 个视图分支：{branches:?}",
            branches.len()
        );
        let session = reachable(&graph, &["loadSession".to_string()]);
        assert!(
            session.len() >= 2,
            "`loadSession` 的闭包只算出 {} 个函数 —— 会话级写者认不出来",
            session.len()
        );

        let mut checked = 0usize;
        for b in &branches {
            let renderer = callee_with_prefix(b, "render")
                .unwrap_or_else(|| panic!("分支行里找不到 render… 调用：{b}"));
            let loader = callee_with_prefix(b, "load")
                .unwrap_or_else(|| panic!("分支行里找不到 load… 调用：{b}"));
            let mut loader_roots = reachable(&graph, std::slice::from_ref(&loader));
            loader_roots.extend(session.iter().cloned());
            let loader_roots: Vec<String> = loader_roots.into_iter().collect();

            for slot in &slots {
                if !closure_reads(&src, &graph, std::slice::from_ref(&renderer), slot) {
                    continue;
                }
                checked += 1;
                assert!(
                    closure_writes(&src, &graph, &loader_roots, slot),
                    "`{renderer}()` 读了 `Live.{slot}`，但分支的 loader 闭包 \
                     （`{loader}()` ∪ `loadSession()`）里没有任何人写它 ⇒ 会话在该视图上建立时\
                     这一格永远空着（C2135 的钱包缺陷形状）。共享槽的正解是「一个写者 \
                     （如 `refreshDashboard()`）+ 每个渲染它的视图各调一次」。分支：{b}"
                );
            }
        }
        // 前置：闭合检查的次数必须够多，否则判别式可能什么都没比
        assert!(
            checked >= 10,
            "只做了 {checked} 次「读了 ⇒ 被装载」检查 —— 判别式太弱"
        );
    }

    /// 提取器/判别式自证：闭包、注释剥离、标识符边界，都要在**合成输入**上有牙齿。
    #[test]
    fn the_slot_closure_scanners_have_teeth() {
        // (a) 传递闭包必须跨函数：renderer 自己只调用，真正读槽的是它调用的那个函数
        let src = concat!(
            "  const Live = {\n    shared: null,\n    own: null,\n  };\n",
            "  function renderA() {\n",
            "    paintA();\n",
            "  }\n\n",
            "  function paintA() {\n",
            "    if (Live.shared) body();\n",
            "  }\n\n",
            "  function loadA() {\n",
            "    Live.own = 1;\n",
            "  }\n\n",
            "  function refreshShared() {\n",
            "    Live.shared = api.get(\"/x\");\n",
            "  }\n\n",
            "  function loadB() {\n",
            "    refreshShared();\n",
            "  }\n"
        );
        let graph = call_graph(src);
        assert_eq!(
            callee_with_prefix(
                "if (id === \"a\") { renderA(); if (loggedIn()) loadA(); }",
                "render"
            ),
            Some("renderA".to_string()),
            "分支行里的 render… 调用没被取出"
        );
        assert!(
            closure_reads(src, &graph, &["renderA".to_string()], "shared"),
            "闭包没跨函数：renderA → paintA 读到 Live.shared 应被认出"
        );
        assert!(
            !closure_reads(src, &graph, &["renderA".to_string()], "own"),
            "阴性对照失败：renderA 的闭包不该「读」Live.own"
        );
        // 竞争修法形状：loader 只写自己的槽 ⇒ 必须红
        assert!(
            !closure_writes(
                src,
                &graph,
                &reachable(&graph, &["loadA".to_string()])
                    .into_iter()
                    .collect::<Vec<_>>(),
                "shared"
            ),
            "判别式没有牙齿：只写 `Live.own` 的 loader 竟被判成装载了 `Live.shared`"
        );
        // 正解形状：loader 调的那个写者写了共享槽 ⇒ 绿
        assert!(
            closure_writes(
                src,
                &graph,
                &reachable(&graph, &["loadB".to_string()])
                    .into_iter()
                    .collect::<Vec<_>>(),
                "shared"
            ),
            "闭包没跨函数：loadB → refreshShared 写 Live.shared 应被认出"
        );

        // (b) 标识符边界：`Live.dashboardTrend` 不等于 `Live.dashboard`
        assert!(
            mentions_slot(
                "    const tr = Live.dashboardTrend || null;",
                "dashboardTrend"
            ),
            "阳性对照失败：Live.dashboardTrend 本身没被认出"
        );
        assert!(
            !mentions_slot("    const tr = Live.dashboardTrend || null;", "dashboard"),
            "阴性对照失败：`Live.dashboardTrend` 被当成了 `Live.dashboard`"
        );
        assert!(
            !writes_slot_exact(
                "    Live.dashboardTrend = await api.get(\"/t\");",
                "dashboard"
            ),
            "阴性对照失败：写 `Live.dashboardTrend` 被当成了写 `Live.dashboard`"
        );
        assert!(
            writes_slot_exact("    Live.dashboard = await api.get(\"/d\");", "dashboard")
                && writes_slot_exact("  } catch (e) { Live.dashboard = null; }", "dashboard"),
            "阳性对照失败：`Live.dashboard = …` 的两种形态（赋值 / catch 兜底）没被认出"
        );
        assert!(
            writes_slot_exact("    await liveLoad(\"models\", \"/api/models\");", "models"),
            "阳性对照失败：通用缓存写入 `liveLoad(\"models\", …)` 没被认出"
        );

        // (c) 注释不参与：解释性的散文里出现 `Live.dashboard` 不得造出「读」
        let commented = concat!(
            "  const Live = {\n    d: null,\n  };\n",
            "  function renderC() {\n",
            "    // 这里必须能提到 Live.d 而不触发门禁（本文件的题眼就是这种注释）\n",
            "    return 1;\n",
            "  }\n"
        );
        let g2 = call_graph(commented);
        assert!(
            !closure_reads(commented, &g2, &["renderC".to_string()], "d"),
            "阴性对照失败：`//` 注释里的 `Live.d` 被当成了读（坑 #296）"
        );
        let block_commented = "  function renderD() {\n    /* Live.d */\n    return 1;\n  }\n";
        let g3 = call_graph(block_commented);
        assert!(
            !closure_reads(block_commented, &g3, &["renderD".to_string()], "d"),
            "阴性对照失败：`/* Live.d */` 块注释被当成了读"
        );

        // (d) 槽宇宙包含「未在字面量里登记但被读写过」的槽
        let undeclared = concat!(
            "  const Live = {\n    a: null,\n  };\n",
            "  function f() {\n    Live.hidden = 1;\n  }\n"
        );
        assert_eq!(
            all_live_slots(undeclared),
            vec!["a".to_string(), "hidden".to_string()],
            "槽宇宙没纳入「代码里出现过但字面量漏登记」的槽"
        );
    }

    /// C2136：**boot 不渲染视图** —— 视图数据只在会话建立之后、且它是当前目的地时才装载。
    ///
    /// 反例（实测，见文件头部）：`DOMContentLoaded` 里那句无条件的 `renderView("dashboard")`
    /// 跑在 `restoreSession()` 之前 ⇒ 仪表盘那套查询在会话不存在时就发出、被
    /// `resetSessionCaches()` 作废、再被 `switchView(目的地)` 重发一遍；目的地不是仪表盘时
    /// 白拉一屏；过期 token 时每个先发的请求都各报一次「登录已过期」。
    #[test]
    fn the_boot_handler_touches_no_view() {
        let src = code_only(APP_JS);
        let boot = boot_body(&src).expect("找不到 DOMContentLoaded 处理器");
        let boot_code = code_lines(boot);
        let view_layer = view_layer_callees(&src);

        // ── 前置：提取器必须真的看见东西（空集上的断言会假绿，坑 68）───────────────────
        assert!(
            view_layer.len() >= 8,
            "视图层调用名只算出 {} 个：{view_layer:?} —— 派生器坏了",
            view_layer.len()
        );
        for expected in ["renderView", "renderDashboard", "loadDashboard"] {
            assert!(
                view_layer.contains(expected),
                "视图层派生漏了 `{expected}`：{view_layer:?}"
            );
        }
        assert!(
            boot_code.lines().count() >= 40,
            "boot 处理器体只切出 {} 行 —— 提取器停早了",
            boot_code.lines().count()
        );

        // ── 不变量 A：boot 不得调用视图层的「渲染器 / 装载器」；`renderView` 只许以当前
        //             目的地为实参（语言切换监听器里的 `renderView(activeView)` 是合法的一处：
        //             它渲染的就是当前目的地，且只在切语言时触发）────────────────────────
        let mut offenders: Vec<(String, String)> = Vec::new();
        for line in boot_code.lines() {
            for callee in callee_names(line) {
                if !view_layer.contains(&callee) {
                    continue;
                }
                if callee == "renderView" {
                    let arg = render_view_argument(line).unwrap_or_default();
                    if arg.contains('"') || arg.contains('\'') {
                        offenders.push((callee, line.trim().to_string()));
                    }
                } else {
                    offenders.push((callee, line.trim().to_string()));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "boot 处理器碰了视图层 —— 会话还没有，视图数据不该在此时装载；\
             `renderView` 只许渲染**当前目的地**。命中：{offenders:?}"
        );

        // ── 不变量 B：每个 `renderView(...)` 的实参都是「当前目的地」，不是字面量视图名 ──
        let mut call_sites = 0usize;
        for line in src.lines() {
            let t = line.trim_start();
            if t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') {
                continue;
            }
            let Some(arg) = render_view_argument(line) else {
                continue;
            };
            call_sites += 1; // `function renderView(id) {` 自身的签名行
            if line.contains("function renderView(") {
                continue;
            }
            assert!(
                !arg.contains('"') && !arg.contains('\''),
                "`renderView({arg})` 传了字面量视图名 —— 只许渲染**当前目的地**（形参 / \
                 `activeView`），否则又会出现「渲染一个不在屏幕上的视图」（C2136）：{t}"
            );
        }
        assert!(
            call_sites >= 2,
            "只找到 {call_sites} 处 `renderView(` —— 提取器坏了"
        );

        // ── 不变量 C：`renderView` 仍由 `switchView` 触发（防止矫枉过正：删成了一个空壳）──
        let router = function_code(&src, "switchView");
        assert!(
            callee_names(&router).contains("renderView"),
            "`switchView` 不再调用 `renderView()` —— 删掉 boot 那句之后，再也没有人渲染目的地视图了"
        );
    }

    /// 提取器自证：`boot_body` 必须停在 boot 处理器自己的收尾行，而不是紧随其后的内容。
    #[test]
    fn the_boot_body_extractor_stops_at_the_right_place() {
        // 处理器内部的内联箭头函数以 4 空格缩进收尾；只有处理器自己以 `  });` 收尾。
        let synthetic = concat!(
            "  document.addEventListener(\"DOMContentLoaded\", () => {\n",
            "    window.addEventListener(\"hashchange\", () => {\n",
            "      switchView(\"a\");\n",
            "    });\n",
            "    renderView(\"b\");\n",
            "  });\n",
            "  function after() {\n",
            "    renderView(\"c\");\n",
            "    return 1;\n",
            "  }\n"
        );
        let body = boot_body(synthetic).expect("合成输入上找不到 boot 处理器");
        assert!(
            body.contains("switchView(\"a\")") && body.contains("renderView(\"b\")"),
            "提取器停早了：处理器内部的行没被切进来"
        );
        assert!(
            !body.contains("function after()"),
            "提取器停晚了：把紧随其后的函数也吞进来了"
        );
        assert!(
            boot_body("  function f() {\n    return 1;\n  }\n").is_none(),
            "阴性对照失败：没有 boot 处理器时不该返回函数体"
        );

        // `render_view_argument` 取的是实参本身（含字面量的引号，供断言判形态）
        assert_eq!(
            render_view_argument("    renderView(\"dashboard\");").as_deref(),
            Some("\"dashboard\"")
        );
        assert_eq!(
            render_view_argument("    renderView(activeView);").as_deref(),
            Some("activeView")
        );
        assert_eq!(
            render_view_argument("  function renderView(id) {").as_deref(),
            Some("id")
        );
        assert_eq!(
            render_view_argument("    const x = 1;"),
            None,
            "阴性对照失败：没调用 `renderView(` 的行被判成调用"
        );

        // 视图层必须**派生自**路由器：给一组合成分支，它就该产出对应的 render/load 名
        let synthetic_router = concat!(
            "  function renderView(id) {\n",
            "    if (id === \"a\") { renderA(); if (loggedIn()) loadA(); }\n",
            "    else if (id === \"b\") { renderB(); if (loggedIn()) loadB(); }\n",
            "  }\n"
        );
        let derived = view_layer_callees(synthetic_router);
        for expected in ["renderView", "renderA", "loadA", "renderB", "loadB"] {
            assert!(
                derived.contains(expected),
                "合成路由器上漏掉了 `{expected}`：{derived:?}"
            );
        }
        assert!(
            !derived.contains("renderC"),
            "阴性对照失败：没出现过的名字被凭空派生了出来"
        );
    }

    /// 模型行的身份是**模型本身**，不是它在某个数组里的位置（C2138）。四条规则各有各的牙。
    #[test]
    fn the_model_row_identity_is_the_model_not_its_position() {
        // ── 规则 1：位置不得进入行对象 ──────────────────────────────────────────────────
        let row = function_code(APP_JS, "modelsToView");
        assert!(
            !row.trim().is_empty(),
            "提取器没取到 `modelsToView` 的函数体（后面几条断言会在空集上假绿）"
        );
        let params = map_callback_params(&row);
        assert_eq!(
            params.len(),
            1,
            "`modelsToView` 里应当有且只有一个 `.map(` 回调：{params:?}"
        );
        assert_eq!(
            params[0].len(),
            1,
            "`modelsToView` 的 `.map(` 回调声明了 {} 个形参 —— 第二个通常就是数组下标，\
             而位置不是身份：它会随目录位移、随表换人（C2138）：{:?}",
            params[0].len(),
            params[0]
        );
        let id_fields: Vec<&str> = row.lines().filter(|l| declares_field(l, "id")).collect();
        assert!(
            id_fields.is_empty(),
            "视图行里声明了字段 `id` —— 那是「模型在本数组里的位置」，一旦被存进 \
             localStorage（最近使用）就跨了渲染/会话/数组（C2138）：{id_fields:?}"
        );

        // ── 规则 2：三处 `data-*` 身份来自 `modelKey(`，点击侧原样传递 ────────────────────
        let mut rendered = 0usize;
        for attr in MODEL_IDENTITY_ATTRS {
            let hits: Vec<String> =
                lines_owned_by(APP_JS, |l| !is_comment_line(l) && renders_attr(l, attr))
                    .into_iter()
                    .map(|(_, _, l)| l)
                    .collect();
            assert!(
                !hits.is_empty(),
                "找不到渲染 `{attr}` 的地方 —— 属性被改名或提取器坏了（空集断言会假绿）"
            );
            for h in &hits {
                assert!(
                    h.contains("modelKey("),
                    "`{attr}` 的值不是由 `modelKey(` 产出的 —— 位置（下标 / `id`）不是身份（C2138）：{h}"
                );
            }
            rendered += hits.len();
        }
        assert!(
            rendered >= MODEL_IDENTITY_ATTRS.len(),
            "承载模型身份的 `data-*` 只找到 {rendered} 处（应 ≥ {}），扫描器没看全",
            MODEL_IDENTITY_ATTRS.len()
        );

        let mut consumed = 0usize;
        for ds in MODEL_IDENTITY_DATASETS {
            let hits: Vec<String> =
                lines_owned_by(APP_JS, |l| !is_comment_line(l) && l.contains(ds))
                    .into_iter()
                    .map(|(_, _, l)| l)
                    .collect();
            assert!(
                !hits.is_empty(),
                "找不到读 `{ds}` 的点击侧 —— 提取器坏了（空集断言会假绿）"
            );
            for h in &hits {
                assert!(
                    !h.contains("Number("),
                    "`{ds}` 被 `Number(` 转回了数字 —— 身份串又被当成位置用（C2138）：{h}"
                );
            }
            consumed += hits.len();
        }
        assert!(
            consumed >= MODEL_IDENTITY_DATASETS.len(),
            "读模型身份的点击侧只找到 {consumed} 处（应 ≥ {}）",
            MODEL_IDENTITY_DATASETS.len()
        );

        // ── 规则 3：`modelKey` 只有一处定义，且身份由 provider+model 构成 ────────────────
        let defs = APP_JS.matches("function modelKey(").count();
        assert_eq!(
            defs, 1,
            "`modelKey` 应当全仓只有一处定义（两份定义会各漂各的）：找到 {defs} 处"
        );
        let key = function_source(APP_JS, "modelKey").expect("取不到 `modelKey` 的定义");
        assert!(
            mentions_identifier(&key, "provider") && mentions_identifier(&key, "model"),
            "`modelKey` 必须同时用 `provider` 与 `model` 构成身份 —— 只用 model 名会在多厂商\
             重名时把两个模型认成同一个：{key}"
        );
        assert!(
            !mentions_identifier(&key, "id"),
            "`modelKey` 体内提到了标识符 `id` —— 位置不得进入身份（C2138）：{key}"
        );

        // ── 规则 4：写进「最近使用」的值必须是 `modelKey(...)` 表达式 ────────────────────
        let writes: Vec<String> = lines_owned_by(APP_JS, |l| {
            !is_comment_line(l) && l.contains("markRecentUsed(")
        })
        .into_iter()
        .map(|(_, _, l)| l)
        .filter(|l| !l.contains("function markRecentUsed("))
        .collect();
        assert!(
            writes.len() >= 2,
            "`markRecentUsed(` 的调用点少于 2 处（应有 openChat 与 consumeModel 两个）—— \
             提取器坏了或调用点被删：{writes:?}"
        );
        for h in &writes {
            assert!(
                h.contains("modelKey("),
                "写进「最近使用」的值不是 `modelKey(...)` —— 存下来的位置活不过一次目录变更\
                 （C2138）：{h}"
            );
        }
    }

    /// C2138 的提取器与判别式自证：合成输入（含阴性对照）必须让每条牙都能单独咬合。
    #[test]
    fn the_model_identity_extractors_have_teeth() {
        // `function_source`：单行函数只取那一行（否则会一路吞到下一个多行函数的收尾，坑 #319）
        let synthetic = concat!(
            "  function modelKey(m) { return m.provider + \"/\" + m.model; }\n",
            "  function modelsToView(list) {\n",
            "    return list.map((m, i) => {\n",
            "      return { id: i, provider: m.provider };\n",
            "    });\n",
            "  }\n"
        );
        let one = function_source(synthetic, "modelKey").expect("取不到单行函数");
        assert!(
            !one.contains("modelsToView"),
            "`function_source` 把紧随其后的函数吞进来了（判别式会读到别人的 `id`）：{one}"
        );
        assert!(
            mentions_identifier(&one, "provider") && !mentions_identifier(&one, "id"),
            "单行函数体读数不对：{one}"
        );
        let multi = function_source(synthetic, "modelsToView").expect("取不到多行函数");
        assert!(multi.contains("id: i"), "多行函数体没被取到：{multi}");
        assert!(
            !multi.contains("function modelKey"),
            "多行函数体取过头了：{multi}"
        );

        // `map_callback_params`：形参个数就是判别式
        assert_eq!(
            map_callback_params(&code_only(&multi)),
            vec![vec!["m".to_string(), "i".to_string()]],
            "形参表读数不对"
        );
        assert_eq!(
            map_callback_params("    return list.map((m) => m);"),
            vec![vec!["m".to_string()]],
            "单形参回调被读错了"
        );
        assert!(
            map_callback_params("  const x = list.map(f);").is_empty(),
            "阴性对照失败：没有 `.map((` 的行被读出了形参"
        );

        // `declares_field`：只认对象字面量里的字段声明，不认属性访问
        assert!(declares_field("        id: i,", "id"), "行首字段没被认出");
        assert!(
            declares_field("      return { id: i,", "id"),
            "`{{` 后字段没被认出"
        );
        assert!(
            declares_field("      return { a: 1, id: i };", "id"),
            "`,` 后字段没被认出"
        );
        assert!(
            !declares_field("      const x = m.id;", "id"),
            "阴性对照失败：属性访问被误判成字段声明"
        );
        assert!(
            !declares_field("      const idx = 1;", "id"),
            "阴性对照失败：`idx` 被误判成 `id`"
        );
        assert!(
            !declares_field("      api.del(\"/api/admin/models/\" + m.id);", "id"),
            "阴性对照失败：URL 里的 `models/\" + m.id` 被误判成字段声明"
        );

        // `renders_attr` / `reads_attr`：渲染侧写 `data-x=`，读取侧写 `[data-x]` —— 判别式是那个 `=`
        assert!(
            renders_attr(
                "      '<button data-mk-expand=\"' + esc(modelKey(m)) + '\">'",
                "data-mk-expand"
            ),
            "合成渲染行没被判成渲染"
        );
        assert!(
            !renders_attr(
                "      const ex = e.target.closest(\"[data-mk-expand]\");",
                "data-mk-expand"
            ),
            "阴性对照失败：读取侧被误判成渲染侧（会把消费者也算成渲染点）"
        );
        assert!(
            !renders_attr(
                "      const btn = document.querySelector('[data-use-model=\"' + id + '\"]');",
                "data-use-model"
            ),
            "阴性对照失败：带值的选择器查询被误判成渲染侧（消费者的属性选择器带 `=`）"
        );
        assert!(
            reads_attr(
                "      const ex = e.target.closest(\"[data-mk-expand]\");",
                "data-mk-expand"
            ),
            "合成读取行没被判成读取"
        );
        assert!(
            !reads_attr(
                "      '<button data-mk-expand=\"' + esc(modelKey(m)) + '\">'",
                "data-mk-expand"
            ),
            "阴性对照失败：渲染侧被误判成读取侧"
        );

        // `mentions_identifier`：词边界（`modelKey` 里的 `model` 不是标识符 `model`）        assert!(mentions_identifier("return m.model;", "model"));
        assert!(
            !mentions_identifier("function modelKey(m) {", "model"),
            "阴性对照失败：`modelKey` 里的 `model` 被当成标识符"
        );
        assert!(
            !mentions_identifier("return m.valid;", "id"),
            "阴性对照失败：`valid` 里的 `id` 被当成标识符"
        );
    }

    /// 市场面的数据源由**会话状态**决定，不由「数据到没到」决定（C2140）。三条规则各有各的牙。
    #[test]
    fn market_tables_follow_the_session_not_the_data() {
        let reads = code_lines_owned_by(APP_JS, reads_market_table);

        // ── 规则 1：每条读市场表的代码行都必须自己按会话状态分支 ─────────────────────────
        assert!(
            !reads.is_empty(),
            "扫描器一条读市场表（`D.MARKET` / `D.PROVIDERS`）的代码行都没扫到 —— 底下三条断言\
             会在空集上假绿，先查扫描器与注释剥离"
        );
        for (owner, line_no, line) in &reads {
            assert!(
                branches_on_session(line),
                "`{owner}` 第 {line_no} 行读了市场表，却没有按**会话状态**分支 —— 它按「数据到没到」\
                 分支，于是登录态目录缺席时把游客表当自己的数据（C2140）：{line}"
            );
        }

        // ── 规则 2：同一行不得既读市场表又读上架表单表 ──────────────────────────────────
        let mixed: Vec<String> =
            code_lines_owned_by(APP_JS, |l| reads_market_table(l) && reads_form_table(l))
                .into_iter()
                .map(|(o, n, l)| format!("{o}:{n}  {l}"))
                .collect();
        assert!(
            mixed.is_empty(),
            "有代码行把**市场表**与**上架表单的兜底表**当成彼此的兜底 —— 两者行数与顺序都不同\
             （`MARKET` 是游客市场、`MODELS` 是价格镜像），互为兜底必然显示错（C2140）：{mixed:?}"
        );

        // ── 规则 3：每张市场表只有一处读者（市场数据源的唯一真源） ───────────────────────
        for table in MARKET_TABLES {
            let owners: BTreeSet<&str> = reads
                .iter()
                .filter(|(_, _, l)| mentions_identifier(l, table))
                .map(|(o, _, _)| o.as_str())
                .collect();
            assert!(
                !owners.is_empty(),
                "全仓找不到任何读 `{table}` 的代码行 —— 游客市场的兜底被删了（那是设计的一部分），\
                 或者扫描器坏了：{reads:?}"
            );
            assert_eq!(
                owners.len(),
                1,
                "`{table}` 被 {} 个函数读（{owners:?}）—— 市场数据源只许有一处真源\
                 （`marketRows` / `marketProviders`），市场的每个消费者都走它：多一处读取就多一处\
                 「按数据到没到」的机会（C2140 的三个缺陷正是这么长出来的）",
                owners.len()
            );
        }
    }

    /// C2140 的扫描器与三条判别式自证：合成输入（含阴性对照）必须让每条牙都能单独咬合。
    #[test]
    fn the_market_source_scanners_have_teeth() {
        // 注释不参与：`//` 起始行、多行块内部整行、行内成对片段都不该被当成「读表」。
        let synthetic = concat!(
            "  // D.PROVIDERS 写在行注释里不算读\n",
            "  /* 多行块注释里的 D.MARKET 也不算\n",
            "     连 D.MODELS 一起 */\n",
            "  const inline = 1; /* D.MARKET */\n",
            "  function renderNav() {\n",
            "    const n = Live.models ? Live.models.length : D.MODELS.length;\n",
            "  }\n"
        );
        let text = code_text_by_line(synthetic);
        assert_eq!(text.len(), 7, "逐行读数不对：{text:?}");
        assert_eq!(text[0], "", "行注释没被剥掉");
        assert_eq!(text[1], "", "块注释首行没被剥掉");
        assert_eq!(text[2], "", "块注释内部行没被剥掉");
        assert_eq!(text[3], "const inline = 1;", "行内成对 `/* … */` 没被剥掉");
        assert_eq!(text[4], "function renderNav() {", "函数声明行被误剥");
        assert!(
            text[5].contains("D.MODELS"),
            "代码行里的表名被误剥：{}",
            text[5]
        );
        assert!(
            !text.iter().any(|l| reads_market_table(l)),
            "注释里的市场表名被当成了读表：{text:?}"
        );
        assert!(
            !text[..5].iter().any(|l| reads_form_table(l)),
            "注释里的表单表名被当成了读表：{text:?}"
        );

        // `reads_*` / `branches_on_session`：标识符边界与判别式
        assert!(
            reads_market_table("      const providers = Live.models ? a : D.PROVIDERS;"),
            "市场表读取行没被认出"
        );
        assert!(
            reads_market_table("          const n = isGuest ? (D.MARKET || []).length : 0;"),
            "同一行里带括号的市场表没被认出"
        );
        assert!(
            !reads_market_table("      const x = D.MODELS_OLD.length;"),
            "阴性对照失败：`D.MODELS_OLD` 被当成 `D.MODELS`（表名互为前缀）"
        );
        assert!(
            !branches_on_session("      const providers = Live.models ? a : D.PROVIDERS;"),
            "阴性对照失败：没有会话判别的行被判成了「按会话分支」"
        );
        assert!(
            branches_on_session(
                "      const p = Live.models ? a : (loggedIn() ? [] : D.PROVIDERS);"
            ),
            "`loggedIn()` 判别没被认出"
        );
        assert!(
            branches_on_session("        const n = isGuest ? (D.MARKET || []).length : 0;"),
            "`isGuest` 判别没被认出"
        );

        // 规则 2 的判别式：同现才算错（徽标那一行的形状 vs 修好后的形状）
        let mixed_line = "          const n = Live.models ? Live.models.length : (isGuest ? (D.MARKET || []).length : (D.MODELS || []).length);";
        assert!(
            reads_market_table(mixed_line) && reads_form_table(mixed_line),
            "规则 2 的判别式没认出徽标那一行"
        );
        let fixed_line =
            "          const n = Live.models ? Live.models.length : (isGuest ? (D.MARKET || []).length : 0);";
        assert!(
            reads_market_table(fixed_line) && !reads_form_table(fixed_line),
            "规则 2 把修好后的徽标行也判成了「两套数据互为兜底」"
        );

        // 规则 3 的判别式：归属（`code_lines_owned_by`）与「读者集合」的读数
        let one = concat!(
            "  function marketRows() { return Live.models ? a : (loggedIn() ? null : D.MARKET); }\n",
            "  function renderRecent() { return marketRows(); }\n"
        );
        let two = concat!(
            "  function marketRows() { return Live.models ? a : (loggedIn() ? null : D.MARKET); }\n",
            "  function renderRecent() { return (Live.models ? a : (loggedIn() ? [] : D.MARKET)); }\n"
        );
        let owners = |src: &str| -> BTreeSet<String> {
            code_lines_owned_by(src, reads_market_table)
                .into_iter()
                .map(|(o, _, _)| o)
                .collect()
        };
        assert!(
            reads_market_table("  function marketRows() { return Live.models ? a : (loggedIn() ? null : D.MARKET); }"),
            "单行 helper 里的市场表没被认出（规则 3 会在空集上假绿）"
        );
        assert_eq!(
            owners(one).len(),
            1,
            "单一真源被读成了多个读者：{:?}",
            owners(one)
        );
        assert_eq!(
            owners(two).len(),
            2,
            "第二处读取没被算成第二个读者（规则 3 会假绿）：{:?}",
            owners(two)
        );
        assert!(
            code_lines_owned_by(two, reads_market_table)
                .iter()
                .all(|(_, _, l)| branches_on_session(l)),
            "合成输入里两处读取都带会话判别，规则 1 不该判红"
        );
    }

    /// 侧边栏只 advertise「按得响」的键位（C2141）。四条规则各有各的牙。
    ///
    /// 契约（`ui/README.md` §键盘可达性）说：nav-item 的角标 = 项在 `NAV_ORDER` 里的下标 +1，
    /// 数字键处理器按**同一个数组**取项，「视图增删后两者自动保持一致」。本门禁钉的就是这个
    /// 「同一个数组」—— 任一侧另立一份清单，角标与生效键就会脱钩（C2141：游客角标 0、死键，
    /// 真正生效的是 2）。
    #[test]
    fn the_sidebar_advertises_only_digits_that_work() {
        let nav = function_source(APP_JS, "renderNav").expect("找不到 renderNav");
        assert!(
            nav.contains("nav-item"),
            "renderNav 提取错了地方（正文里没有 nav-item），下面四条断言会在空集上假绿：{nav}"
        );
        let nav_text = code_text_by_line(&nav).join("\n");
        let all_text = code_text_by_line(APP_JS);

        // ── 规则 1：侧边栏只渲染登记表里的项，不手搓导航项字面量 ─────────────────────────
        let literals: Vec<&str> = nav_text.lines().filter(|l| declares_nav_item(l)).collect();
        assert!(
            literals.is_empty(),
            "`renderNav` 里手搓了导航项字面量 —— 它不在登记表 {NAV_REGISTRY} 里，于是 \
             `indexOf(item)` 恒 -1、角标印 0（`{NAV_REGISTRY}[-1]` 落空 ⇒ 死键），而真正能打开\
             这一页的键游客永远看不到（C2141）。游客分支要从登记表里**筛**（`{NAV_REGISTRY}.filter(…)`），\
             与数字键处理器同源：{literals:?}"
        );

        // ── 规则 2：角标是「项在登记表里的位置」，且不随会话改变 ─────────────────────────
        let badges: Vec<&str> = nav_text
            .lines()
            .filter(|l| l.contains(&format!("{NAV_REGISTRY}.indexOf(")))
            .collect();
        assert_eq!(
            badges.len(),
            1,
            "`renderNav` 里算角标的行有 {} 处（须恰好 1 处）—— 多一处就是多一种「这个数字从哪来」\
             的说法，两侧就会再次脱钩：{badges:?}",
            badges.len()
        );
        assert!(
            !branches_on_session(badges[0]),
            "角标按**会话**分支了（`isGuest ? … : {NAV_REGISTRY}.indexOf(…)`）—— 快捷键是项在登记表里的\
             位置，不是会话的属性；按会话另给一个数字，那个数字没人负责让它按得响：{}",
            badges[0]
        );

        // ── 规则 3：数字键处理器**索引同一个登记表**，且用按下的数字取项 ─────────────────
        let indexes: Vec<(usize, &str)> = all_text
            .iter()
            .enumerate()
            .filter(|(_, l)| l.contains(&format!("{NAV_REGISTRY}[")))
            .map(|(i, l)| (i + 1, l.as_str()))
            .collect();
        assert_eq!(
            indexes.len(),
            1,
            "全仓索引登记表（`{NAV_REGISTRY}[…]`）的行有 {} 处（须恰好 1 处）：它就是数字键处理器。\
             换成另一份清单，角标与生效键就分家了：{indexes:?}",
            indexes.len()
        );
        assert!(
            indexes[0].1.contains("Number("),
            "索引登记表的那一行不是用**按下的数字**取项（看不到 `Number(`）—— 处理器得按角标印的\
             那个数字去取项：{}:{}",
            indexes[0].0,
            indexes[0].1
        );
        assert!(
            !nav_text.contains(&format!("{NAV_REGISTRY}[")),
            "角标这一侧也在**索引**登记表 —— `indexOf` 的 +1 才是角标，索引只该出现在数字键处理器里"
        );

        // ── 规则 4：登记表是**推导**出来的展开结果，不是手抄的第二份清单 ─────────────────
        let decls: Vec<&str> = all_text
            .iter()
            .filter(|l| l.contains(&format!("const {NAV_REGISTRY} =")))
            .map(String::as_str)
            .collect();
        assert_eq!(
            decls.len(),
            1,
            "`const {NAV_REGISTRY} =` 有 {} 处（须恰好 1 处）—— 两份登记表必然各自漂移：{decls:?}",
            decls.len()
        );
        assert!(
            decls[0].contains(NAV_REGISTRY_DERIVATION),
            "登记表 `{NAV_REGISTRY}` 不是从 `NAV` 推导（`{NAV_REGISTRY_DERIVATION}`）而是另抄的一份\
             清单 —— 它与侧边栏的分组立刻会不一致：{}",
            decls[0]
        );
    }

    /// C2141 的扫描器与四条判别式自证：合成输入（含阴性对照）必须让每条牙都能单独咬合。
    #[test]
    fn the_nav_shortcut_scanners_have_teeth() {
        // 注释不参与：注释里的导航项字面量不算「手搓」（坑 #296：门禁会被自己的说明性注释判红）。
        let synthetic = concat!(
            "  function renderNav() {\n",
            "    // { id: \"marketplace\", icon: \"marketplace\" } 写在行注释里不算\n",
            "    /* { id: \"wallet\" }\n",
            "       多行块注释里的也不算 */\n",
            "    const groups = isGuest ? [{ g: \"nav.guest\", items: NAV_ORDER.filter((it) => GUEST_VIEWS.includes(it.id)) }] : roleNav;\n",
            "  }\n"
        );
        let text = code_text_by_line(synthetic);
        assert_eq!(text.len(), 6, "逐行读数不对：{text:?}");
        assert_eq!(text[1], "", "行注释没被剥掉");
        assert_eq!(text[2], "", "块注释首行没被剥掉");
        assert_eq!(text[3], "", "块注释内部行没被剥掉");
        assert!(
            text[4].contains("NAV_ORDER.filter("),
            "代码行被误剥：{}",
            text[4]
        );
        assert!(
            !text.iter().any(|l| declares_nav_item(l)),
            "注释里的导航项字面量被当成了手搓项：{text:?}"
        );

        // 判别式 `declares_nav_item`：阳性（登记表里的项 / 老的那条游客字面量）与阴性对照
        assert!(
            declares_nav_item(
                "      { id: \"dashboard\", icon: \"dashboard\", label: \"nav.dashboard\" },"
            ),
            "登记表里的项没被认出（规则 1 会在空集上假绿）"
        );
        assert!(
            declares_nav_item("          { id: \"marketplace\", icon: \"marketplace\", label: T(\"nav.marketplace\") },"),
            "C2141 那条手搓的游客项没被认出 —— 这正是规则 1 要咬的地方"
        );
        assert!(
            !declares_nav_item("        b.dataset.view = item.id;"),
            "阴性对照失败：读 `item.id` 被当成了声明导航项"
        );
        assert!(
            !declares_nav_item(
                "        b.className = \"nav-item\" + (item.id === activeView ? \" active\" : \"\");"
            ),
            "阴性对照失败：`item.id === …` 被当成了声明导航项"
        );
        assert!(
            !declares_nav_item("        const ok = D.valid ? 1 : 2;"),
            "阴性对照失败：`valid` 里的 `id:`（不，是 `valid`）被绊到了"
        );
        assert!(
            !declares_nav_item("        const key = obj.id + \"x\";"),
            "阴性对照失败：`obj.id` 后面不是字符串值"
        );

        // 提取器：`renderNav` 停在正确的地方（真源码里没有字面量；而 `NAV` 登记表里有）
        let nav = function_source(APP_JS, "renderNav").expect("找不到 renderNav");
        let nav_text = code_text_by_line(&nav).join("\n");
        assert!(
            nav_text.contains("marketRows()"),
            "renderNav 提取短了（市场的 badge 那一行不在里面）：{nav_text}"
        );
        assert_eq!(
            nav_text.matches("function ").count(),
            1,
            "renderNav 提取长了（正文里出现了第二个函数声明，说明它吞进了下一个函数）：{nav_text}"
        );
        assert!(
            !nav_text.lines().any(declares_nav_item),
            "真源码里 renderNav 仍含导航项字面量（规则 1 的判别式或提取器有误）"
        );
        assert!(
            code_text_by_line(APP_JS)
                .iter()
                .filter(|l| declares_nav_item(l))
                .count()
                >= 3,
            "登记表 `NAV` 里的项没被认出来（规则 1 的判别式坏了）"
        );

        // 规则 2 的分支判别式：按会话给角标必须判红，按登记表位置必须放行
        assert!(
            branches_on_session("        const short = isGuest ? 2 : NAV_ORDER.indexOf(item) + 1;"),
            "规则 2 没认出「角标按会话分支」这条竞争修法"
        );
        assert!(
            !branches_on_session("        const short = NAV_ORDER.indexOf(item) + 1;"),
            "规则 2 把修好后的角标行判成了按会话分支"
        );

        // 规则 3：`.filter(` 不算索引（修好后的游客行必须放行）
        let fixed_guest = "      ? [{ g: \"nav.guest\", items: NAV_ORDER.filter((it) => GUEST_VIEWS.includes(it.id)) }]";
        assert!(
            !fixed_guest.contains(&format!("{NAV_REGISTRY}[")),
            "规则 3 把 `{NAV_REGISTRY}.filter(` 当成了索引登记表"
        );
        assert!(
            "        const item = NAV_ORDER[Number(e.key) - 1];"
                .contains(&format!("{NAV_REGISTRY}["))
                && "        const item = NAV_ORDER[Number(e.key) - 1];".contains("Number("),
            "规则 3 认不出真正的数字键处理器"
        );
    }

    /// 管理页「总余额」卡片的**取值**必须折进它副标题具名的每一项（C2142）。
    ///
    /// 轴：卡片副标题写的是「余额 + 赠送」（两包同款），同视图表格的列口径也一样
    /// （`admin.emp.list.sub`：「余额 / 赠送 / 可用（永久点数 + 每日赠送）」），而改前的合计
    /// 只加 `balance` —— 卡片因此比它正下方「可用」列的和少掉**全部赠送额**，屏幕上写着的公式
    /// 却声称加了。
    ///
    /// 哪个方向才是对的，由**产品自己的定义**钉死（不是偏好）：
    /// - `wallet.rs` 的 `available = balance + gift_balance`；赠送点数是可花的、会过期的真钱
    ///   （`gift.rs` 的过期清扫真的把它们从账户里划走）；
    /// - 用户自己看到的那张「余额」（侧栏 / 仪表盘）取的就是 `available`（`loadSession` 里
    ///   `D.USER.balance = w.available`）—— 所以「总余额」= Σ 成员的可用额。
    ///
    /// 三条规则，各有各的牙：
    /// 1. 卡片唯一（`renderAdmin` 里提到副标题键的行恰好一条）。
    /// 2. 卡片**取值表达式**的闭包（表达式 ∪ 其中变量在本函数里的定义 ∪ 它们调用的函数体）必须
    ///    读到 `gift_balance`。只钉「读到了」，不钉写法 —— 抽成 helper、换个变量名都放行。
    /// 3. 副标题**仍须具名两项**（两包都要）：把承诺删掉、让卡片与它对上，不是修法。
    #[test]
    fn the_admin_total_balance_card_sums_what_its_caption_names() {
        let admin = function_source(APP_JS, "renderAdmin").expect("找不到 renderAdmin");
        let admin_code = code_text_by_line(&admin).join("\n");

        // 提取器自证：停在 renderAdmin 里（下一站是 renderAdminModels，本函数不该含它的标记）
        assert!(
            admin_code.contains("emp-stats") && !admin_code.contains("admin-models"),
            "renderAdmin 提取错了地方（规则 1/2 会在空集上假绿）：{admin_code}"
        );
        assert_eq!(
            admin_code.matches("function ").count(),
            1,
            "renderAdmin 提取长了（吞进了下一个函数）：{admin_code}"
        );

        // 规则 1：卡片唯一
        let admin_lines = code_text_by_line(&admin);
        let cards: Vec<&String> = admin_lines
            .iter()
            .filter(|l| l.contains(ADMIN_TOTAL_CARD_SUBTITLE))
            .collect();
        assert_eq!(
            cards.len(),
            1,
            "副标题键 `{ADMIN_TOTAL_CARD_SUBTITLE}` 在 renderAdmin 里出现了 {} 次（卡片要么没了要么重复）：{cards:?}",
            cards.len()
        );

        // 规则 2：取值表达式的闭包必须把副标题具名的**两项**都读进来
        let value = stat_value_argument(cards[0]).unwrap_or_else(|| {
            panic!("总余额卡不再是「标签 / 取值 / 副标题」三段式：{}", cards[0])
        });
        assert!(
            value_reaches_all_fields(APP_JS, &admin, &value, &["balance", "gift_balance"]),
            "总余额卡的取值没有同时读到 `balance` 与 `gift_balance` —— 它副标题（{ADMIN_TOTAL_CARD_SUBTITLE}）\
             写的就是「余额 + 赠送」，可卡片比正下方「可用」列的和少掉全部赠送额：取值 = `{value}`"
        );

        // 规则 3：两包的同一条文案都仍须具名两项
        let zh = pack_region_strict(I18N_JS, ZH_PACK_START, EN_PACK_START);
        let en = pack_region_strict(I18N_JS, EN_PACK_START, PACK_END);
        for (pack, region) in [("zh", zh), ("en", en)] {
            let caption = pack_string(region, ADMIN_TOTAL_CARD_SUBTITLE)
                .unwrap_or_else(|| panic!("{pack} 包里没有键 `{ADMIN_TOTAL_CARD_SUBTITLE}`"));
            assert!(
                caption_names_both_components(&caption),
                "{pack} 包的 `{ADMIN_TOTAL_CARD_SUBTITLE}`（\"{caption}\"）不再同时具名两项 —— \
                 删掉承诺让卡片与它对上不是修法（这张卡说的就是成员的可用额 = 永久 + 赠送）"
            );
        }

        // ── 判别式自证（合成输入）─────────────────────────────────────────────────────
        // 规则 2：等价写法都放行，缺任一项判红
        let both = &["balance", "gift_balance"];
        let direct = "  function renderAdmin() { const total = users.reduce((a, u) => a + (u.balance || 0) + (u.gift_balance || 0), 0); }";
        assert!(
            value_reaches_all_fields(
                direct,
                direct,
                "D.fmt(total) + \" \" + T(\"common.points\")",
                both
            ),
            "规则 2 把「合计里直接两项都加」判红了"
        );
        let bare = "  function renderAdmin() { const total = users.reduce((a, u) => a + (u.balance || 0), 0); }";
        assert!(
            !value_reaches_all_fields(
                bare,
                bare,
                "D.fmt(total) + \" \" + T(\"common.points\")",
                both
            ),
            "规则 2 的判别式坏了：只加 balance 的合计被当成了「两项都读到」"
        );
        // 轴修反了：合计里只剩赠送 —— 探针会拒（卡片 ≠ Σ可用），门禁也必须拒，否则两者松紧不一
        let gift_only = "  function renderAdmin() { const total = users.reduce((a, u) => a + (u.gift_balance || 0), 0); }";
        assert!(
            !value_reaches_all_fields(gift_only, gift_only, "D.fmt(total)", both),
            "规则 2 放行了「合计里只剩赠送」—— 钉单项时 `gift_balance` 会把它判绿（哑子串匹配）"
        );
        // 换个变量名、或把两项分在两条语句里 —— 都还是「两项都读到了」
        let split = "  function renderAdmin() { const total = users.reduce((a, u) => a + (u.balance || 0), 0); const gift = users.reduce((a, u) => a + (u.gift_balance || 0), 0); }";
        assert!(
            value_reaches_all_fields(split, split, "D.fmt(total + gift)", both),
            "规则 2 把「取值里另加一条赠送合计」判红了（等价写法）"
        );
        // 抽成 helper：体内读到也算
        let helper = concat!(
            "  function memberSum(list) { return list.reduce((a, u) => a + (u.balance || 0) + (u.gift_balance || 0), 0); }\n",
            "  function renderAdmin() { const total = memberSum(users); }\n"
        );
        assert!(
            value_reaches_all_fields(helper, helper, "D.fmt(total)", both),
            "规则 2 把「抽成 helper」这种等价写法判红了（只该钉读到了，不该钉写法）"
        );
        // 两个 helper 各读一项（`D.fmt(balSum(users) + giftSum(users))`）—— 也是两项都读到了
        let two_helpers = concat!(
            "  function balSum(list) { return list.reduce((a, u) => a + (u.balance || 0), 0); }\n",
            "  function giftSum(list) { return list.reduce((a, u) => a + (u.gift_balance || 0), 0); }\n",
            "  function renderAdmin() { const total = balSum(users) + giftSum(users); }\n"
        );
        assert!(
            value_reaches_all_fields(two_helpers, two_helpers, "D.fmt(total)", both),
            "规则 2 把「两项各抽一个 helper」判红了（等价写法）"
        );
        // 单行 helper 的闭包不许「吞掉」它下面那段区域（坑 #319 的本轴复发）：
        // helper 自己只含 balance，而它下面十几行外有 gift_balance ⇒ 必须判红
        let swallow = concat!(
            "  function memberSum(list) { return list.reduce((a, u) => a + (u.balance || 0), 0); }\n",
            "  function renderAdmin() {\n",
            "    const total = memberSum(users);\n",
            "    const cell = D.fmt((u.balance || 0) + (u.gift_balance || 0));\n",
            "  }\n"
        );
        assert!(
            !value_reaches_all_fields(swallow, swallow, "D.fmt(total)", both),
            "单行 helper 的闭包吞掉了下面的区域（假绿：坑 #319 的本轴复发）"
        );

        // 注释里的 gift_balance 不算证据（坑 #296 的镜像：被自己的说明性注释满足）
        let commented = "  function renderAdmin() { const total = users.reduce((a, u) => a + (u.balance || 0), 0); // sums gift_balance elsewhere\n  }";
        assert!(
            !value_reaches_all_fields(commented, commented, "D.fmt(total)", both),
            "规则 2 被行尾注释里的 `gift_balance` 满足了（假绿）"
        );

        // `stat_value_argument`：嵌了调用与字符串的取值整段取出；两段式调用取不到
        let card_line = "        stat(T(\"admin.emp.stats.total\"), D.fmt(total) + \" \" + T(\"common.points\"), T(\"admin.emp.stats.total.sub\")),";
        assert_eq!(
            stat_value_argument(card_line).as_deref(),
            Some("D.fmt(total) + \" \" + T(\"common.points\")"),
            "规则 2 的取值提取器取错了段"
        );
        assert_eq!(
            stat_value_argument("        stat(T(\"a\"), D.fmt(x)),").as_deref(),
            Some("D.fmt(x)"),
            "两段式卡片（无副标题）的取值没取出来"
        );
        assert_eq!(
            stat_value_argument("        stat(T(\"a\"),").as_deref(),
            None,
            "一段式调用被当成了卡片"
        );

        // 规则 2 **只咬这一张卡**（对照）：同函数里还有一条按 token 的 `reduce` 合计，它走 `barRow`
        // 不走 `stat` ⇒ 取值提取器对它取不到东西，这条规则不会越界去咬它。
        assert!(
            admin_code.contains("month_tokens"),
            "renderAdmin 里那条用量合计不见了 —— 「只咬一张卡」的对照失去意义"
        );
        let usage_line = admin_lines
            .iter()
            .find(|l| l.contains("month_tokens"))
            .expect("找不到用量合计行");
        assert!(
            stat_value_argument(usage_line).is_none(),
            "用量行被当成了三段式卡片（规则 2 会越界咬到别的合计）：{usage_line}"
        );

        // `assignment_statement`：跨行合计整段取出，且不吞下一条语句
        let multi = concat!(
            "      const users = x;\n",
            "      const total = users.reduce((a, u) =>\n",
            "        a + (u.balance || 0) + (u.gift_balance || 0), 0);\n",
            "      const other = 1;\n"
        );
        let stmt_multi = assignment_statement(multi, "total").expect("跨行合计没被取出");
        assert!(
            stmt_multi.contains("gift_balance") && stmt_multi.contains("(u.balance || 0)"),
            "跨行合计被截短了（规则 2 会假红）：{stmt_multi}"
        );
        assert!(
            !stmt_multi.contains("const other"),
            "跨行合计吞进了下一条语句：{stmt_multi}"
        );
        assert!(
            assignment_statement("      const a = 1;\n      const a = 2;\n", "a").is_none(),
            "同一函数里两次赋值没被判成「归属不清」"
        );

        // 规则 3 的判别式：阳性 / 阴性
        assert!(caption_names_both_components("余额 + 赠送"));
        assert!(caption_names_both_components("balance + gift"));
        assert!(caption_names_both_components("Balances and Gifts"));
        assert!(
            !caption_names_both_components("余额") && !caption_names_both_components("balance"),
            "规则 3 认不出「把承诺删掉」的化妆式修法"
        );
        assert!(
            !caption_names_both_components("赠送"),
            "规则 3 认不出只有赠送、没有余额的半个承诺"
        );

        // `strip_trailing_comment`：字符串里的 `//` 不是注释
        assert_eq!(strip_trailing_comment("  a = 1; // note").trim(), "a = 1;");
        assert_eq!(
            strip_trailing_comment("  x = \"http://a/b\";").trim(),
            "x = \"http://a/b\";",
            "字符串里的 `//` 被当成了注释"
        );
    }

    /// 会话余额（`D.USER.balance`）是一个**事实**：它的绝对值只能来自钱包载荷的**可花额**
    /// （`available = balance + gift_balance`），而且只能由取载荷的那**一处**写（C2145）。
    ///
    /// 轴：这个数字在客户端有**一个定义、多个写者**，其中 `inlineOpsTopup`（运营者给**自己**
    /// 充值后的自刷新）自己又取了一次 `/api/wallet`，且只读 `w.balance`（永久额那一半）。
    /// 赠送点数是**可花、会过期**的真钱（`gift.rs` 的清扫真的把它划走），而
    /// `gift::ensure_daily_gift` 挂在**每个已认证请求**与 `GET /api/wallet` 上、
    /// **与角色无关** ⇒ 运营者/管理员恒有 `gift_balance > 0`。于是给自己充值后侧栏
    /// **立刻少掉当天赠送额**，且永不自愈（`loadSession` 只在会话建立时跑）；同一条路径
    /// 还绕过了缓存槽的唯一写者 ⇒ `Live.wallet` 停在充值前的载荷。实测 `balance=100 +
    /// gift=1`、充值 +100 ⇒ 屏幕 **200**、真值 **201**；`Live.wallet.available` 仍是 **101**。
    ///
    /// 三条规则，各有各的牙：
    /// 1. 每一处 `D.USER.balance = …` 要么是**相对量**、要么是字面 `0`（错误兜底），
    ///    要么取的是 `available` —— 取 `w.balance` 就是拿走永久额那一半。
    /// 2. 取钱包载荷的函数**恰为** `{loadSession, refreshWallet}` —— 多的那一处就是
    ///    「同一事实的第二个来源」（它取载荷却不更新缓存）。这一条拒掉「保留多余取数、
    ///    只把字段换成 `w.available`」的化妆式修法：屏幕上数字对了，缓存仍是旧的。
    /// 3. 缓存槽 `Live.wallet` 的唯一写者仍必须是 `refreshWallet`（否则证据与事实再次脱钩）。
    #[test]
    fn the_session_balance_has_one_source_and_it_is_the_spendable_half() {
        let code = code_text_by_line(APP_JS).join("\n");

        // 阳性对照：赋值点与读者都非空（空集上的集合断言会假绿，坑 68）
        let assignments = code_lines_owned_by(APP_JS, |l| session_balance_rhs(l).is_some());
        assert!(
            assignments.len() >= 3,
            "`{SESSION_BALANCE} = …` 只找到 {} 处 —— 提取器坏了，下面三条规则会在空集上假绿",
            assignments.len()
        );
        let readers = owners_of(&code, fetches_wallet_payload);
        assert!(
            !readers.is_empty(),
            "找不到任何取钱包载荷（`{WALLET_PAYLOAD}`）的函数 —— 规则 2 会假绿"
        );

        // 规则 1：绝对值必须取**可花额**
        for (owner, line_no, line) in &assignments {
            let rhs = session_balance_rhs(line).expect("session_balance_rhs 与命中判据不一致");
            if session_balance_rhs_is_relative(&rhs) || rhs.trim_end_matches(';').trim() == "0" {
                continue; // 相对量（充值/消费演示路径）或错误兜底
            }
            assert!(
                rhs.contains("available"),
                "`{owner}`（第 {line_no} 行）把 `{SESSION_BALANCE}` 从载荷的非可花额字段取了值：`{rhs}`\n\
                 产品定义是 `available = balance + gift_balance`（`wallet.rs`），赠送与永久一样是可花的真钱；\
                 取 `w.balance` 会让这个数字比真值**少掉赠送额**，且永不自愈"
            );
        }

        // 规则 2：取载荷的函数恰为这两处
        let want: BTreeSet<String> = WALLET_READERS.iter().map(|s| s.to_string()).collect();
        assert_eq!(
            readers, want,
            "取钱包载荷（`{WALLET_PAYLOAD}`）的函数集合应恰为 {want:?}，实际 {readers:?} —— \
             多出来的那一处就是「同一事实的第二个来源」：它自己取载荷、却不更新缓存 \
             (`Live.{WALLET_SLOT}`)，于是屏幕上的数字与缓存必然漂移（C2145）"
        );

        // 规则 3：缓存槽的唯一写者
        let cache_writers = owners_of(&code, |l| writes_slot(l, WALLET_SLOT));
        assert_eq!(
            cache_writers,
            BTreeSet::from(["refreshWallet".to_string()]),
            "`Live.{WALLET_SLOT}` 的写者应恰为 `refreshWallet`，实际 {cache_writers:?}"
        );

        // ── 判别式自证（合成输入）─────────────────────────────────────────────────────
        // 只认重新绑定：`+=` / `==` / `!==` 都不是 `D.USER.balance = …`
        assert_eq!(
            session_balance_rhs("  D.USER.balance += amt;").as_deref(),
            None,
            "相对量 `+=` 被当成了重新绑定"
        );
        assert_eq!(
            session_balance_rhs(
                "      D.USER.balance = Math.round((D.USER.balance - cost) * 100) / 100;"
            )
            .as_deref()
            .map(session_balance_rhs_is_relative),
            Some(true),
            "相对量没被判成相对量"
        );
        assert_eq!(
            session_balance_rhs("        if (D.USER.balance === 0) return;"),
            None,
            "读取（`===`）被当成了赋值"
        );
        assert_eq!(
            session_balance_rhs("        if (w) D.USER.balance = w.balance;").as_deref(),
            Some("w.balance;"),
            "真正的赋值没取出来"
        );

        // 规则 1 的判别式：`w.available` 放行，`w.balance` 判红
        assert!(
            session_balance_rhs("  D.USER.balance = w.available;")
                .unwrap()
                .contains("available"),
            "规则 1 把「取可花额」判红了"
        );
        assert!(
            !session_balance_rhs("  D.USER.balance = w.balance;")
                .unwrap()
                .contains("available"),
            "规则 1 的判别式坏了：`w.balance` 被当成了可花额"
        );
        // 同一条语句后面又**读**了一次同一个标识符（UI 的常例：赋值后立刻印出来）——
        // 末尾那次读取不得把 `w.balance` 伪装成相对量（改前树上规则 1 就是这样哑掉的）
        let same_line = session_balance_rhs(
            "            try { const w = await api.get(\"/api/wallet\"); if (w) D.USER.balance = w.balance; \
             $(\"#side-balance\").textContent = D.fmt(D.USER.balance); } catch (e) {}",
        )
        .unwrap();
        assert!(
            !session_balance_rhs_is_relative(&same_line) && !same_line.contains("available"),
            "规则 1 被同一行末尾的**读取**骗过了（`w.balance` 被当成相对量）：`{same_line}`"
        );

        // 规则 2 的判别式：第三处取载荷必须被点名（这正是被拒的化妆式修法）
        let third = concat!(
            "  function loadSession() { const w = await api.get(\"/api/wallet\"); D.USER.balance = w.available; }\n",
            "  function refreshWallet() { Live.wallet = await api.get(\"/api/wallet\"); D.USER.balance = Live.wallet.available; }\n",
            "  function inlineOpsTopup() { const w = await api.get(\"/api/wallet\"); D.USER.balance = w.available; }\n"
        );
        let third_readers = owners_of(&code_text_by_line(third).join("\n"), fetches_wallet_payload);
        assert_ne!(
            third_readers, want,
            "规则 2 认不出第三处取载荷 —— 保留多余取数、只换字段的化妆式修法会全绿"
        );
        assert!(
            third_readers.contains("inlineOpsTopup"),
            "规则 2 的读者集合里没有那一处多余的取数：{third_readers:?}"
        );

        // `fetch` 判据不能把 `Live` 字面量里那行「代码 + 尾注释」算成读者
        assert!(
            !fetches_wallet_payload("    wallet: null,        // GET /api/wallet"),
            "尾注释里的端点被当成了真正的取数（规则 2 会幻影红）"
        );
        assert!(fetches_wallet_payload(
            "      Live.wallet = await api.get(\"/api/wallet\");"
        ));
    }

    /// 交易载荷有两件事必须**同源**：① 「缓存还新不新」的判据（载荷签名）必须覆盖每一个改变
    /// 请求体的输入；② 任何一次「控件改了查询状态」只许有**一个**重拉触发器（C2146）。
    ///
    /// 轴：交易列表/趋势的 URL 由 `txRangeParams()` 从**时间段状态**渲染，而守卫比对的
    /// `txFilterSig()` 只哈希**列筛选**。两份状态、一份判据 ⇒ 时间段控件改完没人重拉，于是三个
    /// 时间段处理器各自**补一次显式 `loadTransactions()`**。补丁在「守卫也成立」时会并发第二次
    /// 请求：`#tx-range` 处理器**先把页码重置为 1**，再渲染 —— 只要用户不在第 1 页，
    /// `loadedPage !== page` 就让守卫成立、`renderTransactions()` 自己发一次并 return，随后那句
    /// 显式调用再发一次 ⇒ **两份逐字相同的列表请求（外加趋势请求也两遍）**。第 1 页上只有一次，
    /// 所以它潜伏至今（要复现必须先翻页）。
    ///
    /// 四条规则，各有各的牙：
    /// 1. **判据覆盖全部输入**：守卫比对的签名函数必须同时读列筛选与三个时间段状态变量，且
    ///    **不得**由 `txRangeParams()` 派生（它含「now − 窗口」的时间戳，每次调用都不同 ⇒
    ///    签名恒变 ⇒ 守卫每次渲染都重拉，形成请求风暴 —— 比原缺陷更坏）。
    /// 2. **控件不许自带取数**：控件绑定所在函数体内不得出现 `loadTransactions()`。
    /// 3. **触发器唯一且被所有控件使用**：候选（体内既重置页码、又直接调 `loadTransactions()`）
    ///    必须恰好一个；两个候选就是两条各自独立的触发路径。
    /// 4. **触发器的显式取数只在槽为空时用**：它必须提到 `!Live.transactions` —— 槽非空时
    ///    `renderTransactions()` 的守卫会自己决定要不要重拉，触发器再无条件拉一次就又是两次。
    #[test]
    fn the_transaction_payload_has_one_signature_and_one_reload_trigger() {
        // 提取器自证（坑 #296 的镜像：本轮的修法**就在控件旁边**写着两句提到
        // `loadTransactions()` 的解释，原文判定会把门禁自己判红）
        let noisy = "  function demo() {\n    // loadTransactions() 出现在注释里\n    /* txTable.page = 1; */\n    return 1;\n  }\n";
        assert_eq!(
            code_body(noisy, "demo"),
            "function demo() {\nreturn 1;",
            "`code_body` 没剥掉注释（含块注释）—— 规则 2/3 会被说明性注释判红"
        );
        assert_eq!(
            reload_trigger_candidates(noisy),
            Vec::<String>::new(),
            "注释里的 `loadTransactions()` / `txTable.page = 1` 被当成了证据"
        );
        assert!(
            mentions_tx_loader("      loadTransactions();"),
            "装载器调用没认出来"
        );
        assert!(
            !mentions_tx_loader("      reloadTransactions();"),
            "`reloadTransactions()` 被当成了 `loadTransactions()` 的证据 —— 两个符号是前缀关系，\
             子串匹配会让规则 2 在**每一个**修好的树上判红（坑 #333）"
        );
        assert_eq!(
            tx_control_owner(
                "  function outer() {\n    const el = $(\"#tx-range\");\n    function inner() {}\n    el.addEventListener(\"change\", () => {});\n  }\n"
            )
            .as_deref(),
            Some("outer"),
            "归属被判给了后声明的嵌套函数（位置启发式的老毛病）"
        );

        let guard = code_body(APP_JS, "renderTransactions");
        assert!(
            mentions_tx_loader(&guard),
            "提取到的 `renderTransactions` 体里没有守卫的重拉调用 —— 提取器坏了，规则 1 会假绿"
        );

        // 阳性对照：签名函数必须被提取到（空集上的断言会假绿）。**不写函数名** ——
        // 写死名字会让改前树因为「名字对不上」而红，红在错的那条规则上。
        let sig = query_sig_name(&guard)
            .expect("守卫里找不到「已加载签名 !== <函数>()」的比对 —— 判别式坏了（或守卫被改写）");
        assert!(
            js_function_body(APP_JS, &sig).is_some(),
            "守卫比对的 `{sig}` 不是本文件里的函数"
        );
        let sig_body = code_body(APP_JS, &sig);
        assert!(
            sig_body.lines().count() >= 2,
            "签名函数 `{sig}` 的体只提取到 {} 行 —— `code_body` 只适用多行函数，先自证再说规则",
            sig_body.lines().count()
        );

        // 规则 1：签名覆盖**列筛选 + 时间段**，且只取状态值
        assert!(
            sig_body.contains(TX_FILTER_STATE),
            "签名函数 `{sig}` 没读列筛选状态（`{TX_FILTER_STATE}`）—— 列筛选改完不会重拉"
        );
        for state in TX_RANGE_STATE {
            assert!(
                mentions_identifier(&sig_body, state),
                "签名函数 `{sig}` 没读时间段状态 `{state}` —— 那个控件改完守卫看不见，\
                 只能靠调用方补一次显式拉取，而补的那一次会在守卫也成立时变成第二次请求（C2146）"
            );
        }
        assert!(
            !sig_body.contains(TX_RANGE_RENDERER),
            "签名函数 `{sig}` 由 `{TX_RANGE_RENDERER}` 派生 —— 它含「now − 窗口」的毫秒时间戳，\
             每次调用都不同 ⇒ 签名恒变 ⇒ 守卫每次渲染都重拉（请求风暴，比原缺陷更坏）"
        );

        // 规则 2：控件绑定所在函数不得自己取数
        let wiring = tx_control_owner(APP_JS)
            .expect("找不到注册 `#tx-range` 的函数 —— 规则 2 的射程会静默变空");
        let wiring_body = code_body(APP_JS, &wiring);
        assert!(
            wiring_body.contains(TX_RANGE_CONTROL),
            "`{wiring}` 的体里没有 `{TX_RANGE_CONTROL}` —— 提取错了函数（规则 2 会假绿）"
        );
        assert!(
            !mentions_tx_loader(&wiring_body),
            "`{wiring}`（交易控件的绑定函数）自己调了 `loadTransactions()` —— 控件必须只调唯一的\
             重拉触发器：`renderTransactions()` 的守卫已经会按签名/页码决定重拉，再补一次会在守卫\
             也成立时并发第二次请求（C2146：第 2 页起改一次时间段发两遍列表 + 两遍趋势）"
        );

        // 规则 3：触发器唯一，且被控件使用
        let triggers = reload_trigger_candidates(APP_JS);
        assert_eq!(
            triggers.len(),
            1,
            "「体内重置页码、又直接调 `loadTransactions()`」的函数应恰好一个，实际 {triggers:?} —— \
             多于一个即两条各自独立的触发路径（本轴要消掉的就是这种形状）"
        );
        let trigger = &triggers[0];
        let trigger_body = code_body(APP_JS, trigger);
        assert!(
            trigger_body.contains("renderTransactions()"),
            "触发器 `{trigger}` 没走 `renderTransactions()` —— 那就绕过了签名守卫，\
             槽非空时它必然与守卫重复拉取"
        );
        let uses = wiring_body.matches(&format!("{trigger}()")).count();
        assert!(
            uses >= 4,
            "`{wiring}` 里只调了 `{trigger}()` {uses} 次 —— 四个控件（顶部 tab + 时间段快捷/起/止）\
             都必须走这一个触发器（漏掉的那个就只能自带取数）"
        );

        // 规则 4：触发器里的显式取数只在槽为空时用
        assert!(
            trigger_body.contains("!Live.transactions"),
            "触发器 `{trigger}` 里的显式 `loadTransactions()` 没有被「槽为空」守住 —— \
             槽非空时守卫会自己重拉，这里再拉一次就是两次请求"
        );

        // ── 判别式自证（合成输入）─────────────────────────────────────────────────────
        // 规则 1 的判别式：`!==` 右侧取到的是**签名函数**，不是页码/每页行数那些值
        let real_guard = "    if (Live.transactions && (txTable.loadedPage !== txTable.page \
                          || txTable.loadedPageSize !== txTable.pageSize \
                          || txTable.loadedQuerySig !== txQuerySig())) {";
        assert_eq!(
            query_sig_name(real_guard).as_deref(),
            Some("txQuerySig"),
            "判别式没从守卫里取出签名函数名（前两项的右侧把后面的比较式一起带进来，必须被刷掉）"
        );
        // 顺序无关：签名比对写在最前面也一样取得到
        assert_eq!(
            query_sig_name(
                "    if (txTable.loadedQuerySig !== txQuerySig() || txTable.loadedPage !== txTable.page) {"
            )
            .as_deref(),
            Some("txQuerySig"),
            "判别式假设了签名比对排在最后 —— 换个顺序就取不到（规则 1 会假绿）"
        );
        // 只比页码的守卫：右侧不是调用 ⇒ `None`（调用方 `expect` 会当场报错，而不是静默假绿）
        assert_eq!(
            query_sig_name("    if (txTable.loadedPage !== txTable.page) {"),
            None,
            "「右侧不是函数调用」的守卫被当成了签名比对"
        );

        // 规则 1 的牙：`txRangeParams()` 派生的签名必须被这条断言看见
        let storm_sig = function_source(
            "  function txQuerySig() { return txTable.filters + txRangeParams(); }",
            "txQuerySig",
        )
        .expect("`function_source` 取不到单行函数（自证提取器）");
        assert!(
            storm_sig.contains(TX_RANGE_RENDERER),
            "自证失败：由 `txRangeParams()` 派生的签名本该被 `contains({TX_RANGE_RENDERER:?})` 命中 —— \
             规则 1 的第三条断言没有牙"
        );

        // 规则 2/3 的判别式
        assert!(
            !writes_tx_page("    const page = Math.max(1, txTable.page || 1);"),
            "读 `txTable.page` 被当成了写"
        );
        assert!(
            !writes_tx_page("    txTable.pageSize = Math.min(100, x);"),
            "`txTable.pageSize` 被当成了 `txTable.page`（前缀没收到标识符边界）"
        );
        assert!(writes_tx_page("    txTable.page = 1;"), "真正的写没认出来");

        // 规则 3：合成输入 —— 两条触发路径必须被点名
        let two_triggers = concat!(
            "  function reloadTransactions() { txTable.page = 1; loadTransactions(); renderTransactions(); }\n",
            "  function otherReload() { txTable.page = 1; loadTransactions(); }\n"
        );
        assert_eq!(
            reload_trigger_candidates(two_triggers),
            vec!["otherReload".to_string(), "reloadTransactions".to_string()],
            "规则 3 认不出第二条触发路径（半修会全绿）"
        );
        assert!(
            reload_trigger_candidates(
                "  function reloadTransactions() { txTable.page = 1; renderTransactions(); }\n"
            )
            .is_empty(),
            "不含显式取数的函数被当成了触发器"
        );
    }

    /// 设置卡片里的每个控件要么**接线**、要么**明确标成惰性**（C2148）。
    ///
    /// 轴：设置视图渲染了三族「看着能操作、输入却无人消费」的控件 ——
    /// ① 昵称输入框（可编辑、由真实 `/api/me` 填，但全仓没有写昵称的后端路径：
    ///    `UPDATE users` 只写 `dept_id`/`verified`/`password_hash` ⇒ 输入会在下一次
    ///    `renderSettings` 被真名抹掉）；
    /// ② 「默认模型」下拉（由真实目录填、看着能选，但无监听 / 无存储键 / 全仓无消费者）；
    /// ③ 三枚渲染成**已勾选**的通知开关（无 id / 无 name / 无人读 —— 仓内没有通知子系统）。
    /// 三族的每一族都能在**同卡兄弟**或仓内成例里指出它缺的那一半：同卡的邮箱早已
    /// `readonly` + 一句提示（#157 那次重设计**同时**删掉了同卡那个死「保存」按钮、
    /// 给邮箱标了惰性，独漏昵称 ⇒ 漂移而非取舍），卡内三个偏好控件（语言 / 主题 / 密度）
    /// 都持久化并生效，而「能力尚未开放」的仓内成例是 `#withdraw-btn`（`disabled` + 说明）。
    ///
    /// 三条规则：
    /// 1. **`inert ⟺ ¬consumed`**（双向）：没接线的控件必须标惰性（挡住本轴的三张脸），
    ///    接了线的控件不得标惰性（挡住「一键全禁用」式的过度纠正 —— 那会把语言 / 主题 /
    ///    密度三个真控件一起打死）。
    /// 2. 卡内有惰性控件 ⇒ 卡内必须有一句 `hint` 说明，且键**两包俱在**（否则 en 界面
    ///    显示原始键名，等于没解释）。
    /// 3. 空集守卫：解析出的惰性 / 接线控件都非空 —— 否则上面两条会在空集上假绿。
    #[test]
    fn settings_controls_are_either_live_or_marked_inert() {
        let code = code_text_by_line(APP_JS).join("\n");
        let view = settings_view(INDEX_HTML).expect("设置视图 `view-settings` 不在 index.html 里");
        let zh = pack_region(I18N_JS, ZH_PACK_START, PACK_END).expect("zh 语言包区段");
        let en = pack_region(I18N_JS, EN_PACK_START, PACK_END).expect("en 语言包区段");
        let mut inerts: Vec<String> = Vec::new();
        let mut lives: Vec<String> = Vec::new();

        for key in SETTINGS_CARDS {
            let region =
                card_region(view, key).unwrap_or_else(|| panic!("卡片 `{key}` 不在设置视图里"));
            let controls = parse_controls(region);
            assert!(
                !controls.is_empty(),
                "卡片 `{key}` 里一个表单控件都没解析出来 —— 提取器坏了，下面的断言会假绿"
            );
            for c in &controls {
                let consumed = consumed_in(&code, c);
                let label =
                    c.id.clone()
                        .or_else(|| c.name.clone())
                        .unwrap_or_else(|| format!("<{}>（无 id / 无 name）", c.tag));
                if c.inert {
                    assert!(
                        !consumed,
                        "卡片 `{key}` 的控件 `{label}` 被标成了惰性，但 `app.js` 确实在**读它的值** \
                         并送进产品代码 —— 惰性标记必须与接线状态一致，否则标记本身在说谎"
                    );
                    inerts.push(label);
                } else {
                    assert!(
                        consumed,
                        "卡片 `{key}` 的控件 `{label}` 既没有接线、也没有标惰性（缺 \
                         `readonly` / `disabled`）：它看上去可以操作，但输入没有任何消费者；\
                         没接线的控件必须明确标成惰性并在卡里给一句本地化说明（C2148）"
                    );
                    lives.push(label);
                }
            }
            if controls.iter().any(|c| c.inert) {
                let hints = hint_keys(region);
                assert!(
                    !hints.is_empty(),
                    "卡片 `{key}` 里有惰性控件，却没有一句 `hint` 说明 —— 用户只看到控件被禁用，\
                     不知道是「能力未开放」还是「坏了」（仓内成例：`#withdraw-btn` + 说明）"
                );
                for h in &hints {
                    let needle = format!("\"{h}\":");
                    assert!(
                        zh.contains(&needle) && en.contains(&needle),
                        "卡片 `{key}` 的说明键 `{h}` 没有两包俱在（zh={} en={}）—— 缺的那一包会把\
                         原始键名直接显示给用户",
                        zh.contains(&needle),
                        en.contains(&needle)
                    );
                }
            }
        }

        assert!(
            inerts.len() >= 3 && lives.len() >= 3,
            "解析出的惰性控件 {} 个、接线控件 {} 个 —— 提取器或射程坏了（空集守卫）",
            inerts.len(),
            lives.len()
        );
        assert!(
            inerts.iter().any(|l| l == "settings-nickname")
                && lives.iter().any(|l| l == "prefs-lang"),
            "射程里没同时看到「账户卡的昵称」与「偏好卡的语言」—— 卡片区段提取可能串了\
             （inerts={inerts:?} lives={lives:?}）"
        );
    }

    /// 上面那张门禁的**提取器自证**（合成输入）：判别式没牙的话，规则会在空集或错集合上假绿。
    #[test]
    fn the_settings_control_extractors_have_teeth() {
        // ── 控件解析 ──────────────────────────────────────────────────────────────
        let fake = concat!(
            "<section class=\"view hidden\" id=\"view-settings\">\n",
            "  <div class=\"card\">\n",
            "    <h3 data-i18n=\"settings.account\">账户</h3>\n",
            "    <div class=\"form\">\n",
            "      <input class=\"input\" id=\"a-live\" hidden value=\"\">\n",
            "      <!-- <input id=\"a-in-comment\"> -->\n",
            "      <input type=\"checkbox\" disabled>\n",
            "      <select id=\"a-model\" disabled><option value=\"\">—</option></select>\n",
            "      <button id=\"a-btn\" type=\"button\">确定</button>\n",
            "    </div>\n",
            "  </div>\n",
            "  <div class=\"card\">\n",
            "    <h3 data-i18n=\"settings.prefs\">偏好</h3>\n",
            "    <div class=\"form\"><input type=\"radio\" name=\"density\" id=\"d-1\"></div>\n",
            "  </div>\n",
            "</section>\n",
            "<section id=\"view-admin\"><input id=\"not-mine\"></section>\n",
        );
        let view = settings_view(fake).expect("设置视图");
        assert!(
            !view.contains("not-mine"),
            "射程越过了 `</section>`，把后面的视图也扫进来了"
        );
        let account = card_region(view, "settings.account").expect("账户卡");
        assert!(
            !account.contains("settings.prefs"),
            "卡片区段没有在下一张卡处收口"
        );
        let controls = parse_controls(account);
        assert_eq!(
            controls.len(),
            4,
            "控件解析漏了或多了：{:?}",
            controls.iter().map(|c| c.id.clone()).collect::<Vec<_>>()
        );
        assert_eq!(controls[0].id.as_deref(), Some("a-live"));
        assert!(
            controls[0].value_bearing && !controls[0].inert,
            "`hidden` 被当成了惰性标记，或文本输入框被判成按钮"
        );
        assert_eq!(
            controls[1].id, None,
            "HTML 注释里的控件参与了断言（修法自己就会在控件旁写注释）"
        );
        assert!(
            controls[1].inert && controls[1].value_bearing,
            "`disabled` 没被认出来"
        );
        assert!(
            controls[2].inert && controls[2].value_bearing,
            "禁用的 select 必须仍是值控件"
        );
        assert!(!controls[3].value_bearing, "按钮被当成了值控件");
        let radios = parse_controls(card_region(view, "settings.prefs").expect("偏好卡"));
        assert_eq!(
            control_handles(&radios[0]),
            vec!["#d-1".to_string(), "name=\"density\"".to_string()],
            "单选组的 `name` 句柄没生成 —— 单选组靠它接线"
        );

        // ── 消费判别式 ────────────────────────────────────────────────────────────
        let value_ctrl = |id: &str| HtmlControl {
            tag: "input".into(),
            id: Some(id.into()),
            name: None,
            value_bearing: true,
            inert: false,
        };
        let button = HtmlControl {
            tag: "button".into(),
            id: Some("b-go".into()),
            name: None,
            value_bearing: false,
            inert: false,
        };
        let radio = HtmlControl {
            tag: "input".into(),
            id: Some("d-1".into()),
            name: Some("density".into()),
            value_bearing: true,
            inert: false,
        };

        // ① 只填不读 ⇒ 未接线
        assert!(
            !consumed_in(
                "  const a = $(\"#a\");\n  if (a) a.value = 1;",
                &value_ctrl("a")
            ),
            "「只被填过」被当成了接线（坑 #338）"
        );
        // ② 只把值存起来 ⇒ 未接线（这才是「加个监听器 + 写 localStorage」的分界）
        assert!(
            !consumed_in(
                "  const a = $(\"#a\");\n  a.addEventListener(\"change\", () => localStorage.setItem(\"k\", a.value));",
                &value_ctrl("a")
            ),
            "「把值写进 localStorage」被当成了消费 —— 那么半修（持久化但不消费）就会全绿"
        );
        // ③ 读进局部变量给重渲染还原 ⇒ 未接线
        assert!(
            !consumed_in(
                "  const a = $(\"#a\");\n  const cur = a.value;\n  a.value = cur;",
                &value_ctrl("a")
            ),
            "「把当前值读进局部变量」被当成了消费"
        );
        // ④ 值作为非存储调用的实参 ⇒ 接线
        assert!(
            consumed_in(
                "  const a = $(\"#a\");\n  a.addEventListener(\"change\", () => applyTheme(a.value));",
                &value_ctrl("a")
            ),
            "值流进产品代码却没被判成接线"
        );
        // ⑤ 标识符 token 边界：`themeSel.value` 不是 `sel.value`（坑 #333）
        assert!(
            !consumed_in(
                "  const sel = $(\"#a\");\n  applyTheme(themeSel.value);",
                &value_ctrl("a")
            ),
            "兄弟标识符把证据送进了集合（`themeSel.value` 被算成 `sel.value`）"
        );
        // ⑥ 单选组：`querySelectorAll(...).forEach((r) =>` + 值读取 ⇒ 接线
        assert!(
            consumed_in(
                "  document.querySelectorAll('input[name=\"density\"]').forEach((r) => {\n    r.addEventListener(\"change\", () => applyDensity(r.value));\n  });",
                &radio
            ),
            "单选组的 `name` 句柄接不上线"
        );
        // ⑦ 按钮：绑监听器即接线；同形写法放在值控件上不算（那正是 ② 的形状）
        assert!(
            consumed_in("  $(\"#b-go\").addEventListener(\"click\", go);", &button),
            "按钮绑了监听器却没被判成接线"
        );
        assert!(
            !consumed_in(
                "  $(\"#a\").addEventListener(\"change\", () => save(a.value));",
                &value_ctrl("a")
            ),
            "按钮那套判据被用到了值控件上 —— 监听器本身不算消费"
        );
        // ⑧ 没有 id / name 的控件：任何代码都认不出它 ⇒ 未接线
        assert!(
            !consumed_in(
                "  document.querySelectorAll(\"input[type=checkbox]\").forEach((c) => use(c.checked));",
                &HtmlControl {
                    tag: "input".into(),
                    id: None,
                    name: None,
                    value_bearing: true,
                    inert: false,
                }
            ),
            "无 id / 无 name 的控件被判成了接线 —— 那三枚通知开关就再也抓不住了"
        );

        // ── 说明键：两包俱在 ──────────────────────────────────────────────────────
        let zh = pack_region(I18N_JS, ZH_PACK_START, PACK_END).expect("zh 包");
        let en = pack_region(I18N_JS, EN_PACK_START, PACK_END).expect("en 包");
        assert!(
            zh.contains("\"settings.notify.hint\":") && en.contains("\"settings.notify.hint\":"),
            "惰性说明键没有两包俱在 —— 规则 2 会在错集合上假绿"
        );
        assert!(
            !zh.contains("\"settings.notify.hint.zzz\":"),
            "`contains` 判别式认出了不存在的键（自证：阳性对照必须是真键）"
        );
        assert_eq!(
            hint_keys("<span class=\"hint\" data-i18n=\"a.b\">x</span>"),
            vec!["a.b".to_string()],
            "说明键提取器"
        );
        assert!(
            hint_keys("<span class=\"hintish\" data-i18n=\"a.b\">x</span>").is_empty(),
            "`hint` 前缀被当成了说明元素"
        );
    }

    /// 共享行的「切换动作」与它的「结局文案」必须由**同一条目**给出（C2153）。
    ///
    /// 轴：共享 key 有三个状态（`on` / `paused` / `off`，三个都可达 —— `PATCH /api/sharings/:id`
    /// 接受这三个值，而 `GET /api/sharings` 不做状态过滤 ⇒ 软删过的行仍在列表里）。行内按钮按
    /// **当前状态**三值取（暂停 / 恢复 / 重新上架），而**结局消息**按**下一状态**取两值
    /// （`const next = s.status === "on" ? "paused" : "on"`）⇒ `off → on`（重新上架）被报成
    /// 「已恢复」。命名那个分支的键 `share.toggle.relisted` 两个包都在、无人可达 ——
    /// 「不可达的键」正是「丢了一条分支」的指纹（orphan 60 → 59）。
    ///
    /// 三条规则各有各的牙：
    /// 1. 全文件里 `share.toggle.*` 的**字面量**只许出现在过渡表里（表外出现 = 有人自己挑结局）；
    ///    表里承担「动作」与「结局」的两列**逐条目互不相同**（把三条压成两条 = 又有一个动作被
    ///    报成另一个动作的结局），`next` 不得指向自己（切换必然改状态）。
    /// 2. 表的键集**恰好等于** `SHARE_STATUS` 的键集 —— 两侧都是**从源码推出来的**，不写花名册：
    ///    徽标认识的状态，切换表都必须有对应条目。
    /// 3. 处理器（＝那个把**非字面量**状态交给 `/api/sharings/` 的函数，推导得出）**不许自己挑
    ///    结局**：体内不得出现 `share.toggle.*` 字面量、不得按状态字面量分支，且必须与**按钮渲染
    ///    那一行**共用同一个访问器（两侧同源）。
    #[test]
    fn the_sharing_toggle_outcome_comes_from_the_same_entry_as_its_action() {
        let code = code_text_by_line(APP_JS).join("\n");
        let zh = pack_region(I18N_JS, ZH_PACK_START, PACK_END).expect("zh 语言包区段");
        let en = pack_region(I18N_JS, EN_PACK_START, PACK_END).expect("en 语言包区段");

        // ── 前置：右侧的表必须被真的解析出来（空集上的断言会假绿，坑 68）───────────────
        let status_keys = object_literal_keys(&code, SHARE_STATUS_TABLE)
            .expect("app.js 里没有 `const SHARE_STATUS = { … }` —— 状态徽标表不见了");
        assert!(
            status_keys.len() >= 3,
            "`{SHARE_STATUS_TABLE}` 只解析出 {} 个键 —— 提取器坏了（规则 2 的右侧会是空集）",
            status_keys.len()
        );

        // ── 规则 1：键字面量只许出现在过渡表里 ─────────────────────────────────────
        let span = object_literal_span(&code, SHARE_TOGGLE_TABLE);
        let mut outside: Vec<String> = Vec::new();
        let mut from = 0usize;
        while let Some(rel) = code[from..].find(SHARE_TOGGLE_KEY_PREFIX) {
            let at = from + rel;
            if !matches!(span, Some((s, e)) if at >= s && at < e) {
                outside.push(line_at(&code, at));
            }
            from = at + SHARE_TOGGLE_KEY_PREFIX.len();
        }
        assert!(
            outside.is_empty(),
            "`share.toggle.*` 的字面量出现在过渡表之外（{} 处）：{outside:?} —— 结局文案必须取自\
             过渡表的条目；自己挑结局正是 C2153 的形状（`off → on` 重新上架被报成「已恢复」）",
            outside.len()
        );

        let entries = nested_entries(&code, SHARE_TOGGLE_TABLE);
        assert_eq!(
            entries.len(),
            status_keys.len(),
            "过渡表 `{SHARE_TOGGLE_TABLE}` 有 {} 个条目，而 `{SHARE_STATUS_TABLE}` 有 {} 个状态 —— \
             每个可达状态都必须在表里有一条目（C2153 就是 `off` 那一条被漏掉）",
            entries.len(),
            status_keys.len()
        );
        // 两个独立提取器必须给出同一串键（坑 #291：读数不一致时**先查规则**）——
        // 条目键若是空串，`next` 自环检查会恒真，规则 1 的后半就没有牙。
        let table_keys = object_literal_keys(&code, SHARE_TOGGLE_TABLE).unwrap_or_default();
        let entry_keys: Vec<String> = entries.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(
            entry_keys, table_keys,
            "条目键与顶层键两个提取器的读数不一致 —— 先查提取规则，别改数据"
        );

        // ── 规则 1（后半）：动作 / 结局两列逐条目唯一，`next` 不自环，键两包俱在 ──────
        for (key, inner) in &entries {
            let next = string_field(inner, "next")
                .unwrap_or_else(|| panic!("过渡表条目 `{key}` 没有 `next` 列"));
            assert_ne!(
                next.as_str(),
                key.as_str(),
                "过渡表条目 `{key}` 的 `next` 指向它自己 —— 切换不会产生状态变化"
            );
            for column in SHARE_TOGGLE_UNIQUE_COLUMNS {
                let value = string_field(inner, column)
                    .unwrap_or_else(|| panic!("过渡表条目 `{key}` 没有 `{column}` 列"));
                assert!(
                    value.starts_with("share.toggle."),
                    "过渡表条目 `{key}` 的 `{column}` 是 `{value}` —— 不是 `share.toggle.*` 键"
                );
                for (pack, name) in [(zh, "zh"), (en, "en")] {
                    assert!(
                        pack.contains(&format!("\"{value}\":")),
                        "过渡表引用了 `{value}`，而 {name} 语言包里没有这个键 —— \
                         界面上会显示原始键名"
                    );
                }
            }
        }
        for column in SHARE_TOGGLE_UNIQUE_COLUMNS {
            let mut seen: Vec<(String, String)> = Vec::new();
            for (key, inner) in &entries {
                let value = string_field(inner, column).unwrap_or_default();
                if let Some((_, other)) = seen.iter().find(|(v, _)| v == &value) {
                    panic!(
                        "过渡表的 `{column}` 列在 `{other}` 与 `{key}` 上重复（都取 `{value}`）—— \
                         两个不同的动作被报成同一个结局：这正是 C2153 的缺陷本身"
                    );
                }
                seen.push((value, key.clone()));
            }
        }

        // ── 规则 2：键集相等（两侧都从源码推出来）──────────────────────────────────
        let mut toggle_keys = table_keys;
        toggle_keys.sort();
        toggle_keys.dedup();
        let mut status_sorted = status_keys.clone();
        status_sorted.sort();
        status_sorted.dedup();
        assert_eq!(
            toggle_keys, status_sorted,
            "过渡表的键集与 `{SHARE_STATUS_TABLE}` 的状态集不等 —— 徽标认识的状态，切换表都必须有\
             对应的动作与结局（漏一个 = 那个状态的切换会被报成另一个动作的结局，C2153）"
        );

        // ── 规则 3：处理器不许自己挑结局 ───────────────────────────────────────────
        let handlers = dynamic_status_owners(&code);
        assert_eq!(
            handlers.len(),
            1,
            "把状态交给 `/api/sharings/` 的端点里，应**恰好一个**是由状态推出的切换（把状态写成\
             字面量的是删除这类直接端点），实际 {handlers:?} —— 多于一个即两条各自解释状态的路径，\
             那正是本轴要消掉的形状"
        );
        let handler_name = handlers[0].clone();
        let handler = code_body(APP_JS, &handler_name);
        assert!(
            handler.contains("s.status"),
            "提取到的 `{handler_name}` 体里没有行状态 —— 提取器坏了（规则 3 会假绿）"
        );
        assert!(
            !handler.contains(SHARE_TOGGLE_KEY_PREFIX),
            "`{handler_name}` 体内出现 `share.toggle.*` 字面量 —— 结局必须由过渡表的条目给出"
        );
        for status in &status_keys {
            for lit in [format!("\"{status}\""), format!("'{status}'")] {
                assert!(
                    !handler.contains(&lit),
                    "`{handler_name}` 体内按状态字面量 `{lit}` 分支 —— 处理器必须把当前状态交给\
                     过渡表；自己用两值判别式挑 `next` / 结局正是 C2153 的形状（三个状态被压成两个）"
                );
            }
        }
        let button_line =
            code_block_containing(&code, SHARE_TOGGLE_BUTTON, SHARE_TOGGLE_BUTTON_END)
                .expect("找不到行内切换按钮那一段标记 —— 规则 3 的射程会静默变空");
        assert!(
            button_line.contains(SHARE_TOGGLE_BUTTON_END),
            "按钮那一段没有收尾（`{SHARE_TOGGLE_BUTTON_END}`）—— 取到的不是完整按钮"
        );
        let shared = shared_accessor(&button_line, &handler);
        assert_eq!(
            shared.len(),
            1,
            "按钮那一行与 `{handler_name}` 之间没有**唯一**的共同访问器（实际 {shared:?}）—— \
             动作与结局必须取自同一张表的同一条目（两边各自解释状态时，三个状态的切换必然有一边\
             少一个分支）"
        );
        assert!(
            mentions_identifier(&handler, &shared[0]),
            "共同访问器 `{}` 没出现在处理器体里",
            shared[0]
        );

        // ── 判别式自证（合成输入）──────────────────────────────────────────────────
        // ① 注释里的表名/键名不成证据（坑 #296 的镜像：本轮的修法就在表旁边写了一整段解释）
        let commented = concat!(
            "  // const SHARE_TOGGLE = { a: { label: \"l\" }, b: { label: \"m\" } };\n",
            "  const SHARE_TOGGLE = {\n",
            "    on: { label: \"share.toggle.pause\", next: \"paused\", outcome: \"share.toggle.paused\" },\n",
            "  };\n"
        );
        assert_eq!(
            nested_entries(commented, SHARE_TOGGLE_TABLE).len(),
            2,
            "自证失效：未剥注释的文本本该把注释里那张表也读出来"
        );
        assert_eq!(
            nested_entries(&code_text_by_line(commented).join("\n"), SHARE_TOGGLE_TABLE).len(),
            1,
            "注释里的过渡表被当成了真表 —— 规则 1/2 的射程会被解释性注释撑大"
        );

        // ② 字符串里的花括号不参与配对（表里两处都有）
        let braces = concat!(
            "  const SHARE_TOGGLE = {\n",
            "    a: { label: \"}\", next: \"b\", outcome: \"{\" },\n",
            "    b: { label: \"x\", next: \"a\", outcome: \"y\" },\n",
            "  };\n"
        );
        assert_eq!(
            object_literal_keys(braces, SHARE_TOGGLE_TABLE),
            Some(vec!["a".to_string(), "b".to_string()]),
            "字符串里的花括号参与了配对（规则 1 的射程会偏）"
        );
        assert_eq!(
            nested_entries(braces, SHARE_TOGGLE_TABLE).len(),
            2,
            "字符串里的花括号让条目提取提前收尾"
        );
        assert_eq!(
            string_field("{ label: \"}\", next: \"b\" }", "next").as_deref(),
            Some("b"),
            "字段值被字符串里的花括号截断"
        );

        // ③ 嵌套对象的键不算外层键；`xlabel` 不是 `label`（坑 #333 的判别式边界）
        assert_eq!(
            object_literal_keys("  const X = {\n    a: { b: { c: 1 } },\n  };\n", "X"),
            Some(vec!["a".to_string()]),
            "嵌套对象里的键被当成了外层键"
        );
        // 条目键必须真的取出来 —— 键后跟的是 `:` 与空格，`rsplit` 直接取最后一段会拿到空串，
        // 而空键会让「`next` 不自环」恒真（A/B 的 M2b 腿当场把这条缺陷印了出来）
        assert_eq!(
            nested_entries(
                "  const X = {\n    a: { label: \"l\" },\n    b: { label: \"m\" },\n  };\n",
                "X"
            )
            .into_iter()
            .map(|(k, _)| k)
            .collect::<Vec<_>>(),
            vec!["a".to_string(), "b".to_string()],
            "条目键没被取出来（空键让 `next` 自环检查恒真）"
        );
        assert_eq!(
            string_field("{ xlabel: \"nope\", label: \"yes\" }", "label").as_deref(),
            Some("yes"),
            "`label` 的前缀匹配把 `xlabel` 当成了证据"
        );

        // ④ 规则 3 的判别式必须**在缺陷形状上**取空集、在修复形状上取到那一个访问器
        let base_button = "      (s.status === \"on\" ? T(\"share.toggle.pause\") : \
                            s.status === \"paused\" ? T(\"share.toggle.resume\") : \
                            T(\"share.toggle.relist\")) + \"</button> \";";
        let base_handler = concat!(
            "  async function toggleSharing(i) {\n",
            "    const next = s.status === \"on\" ? \"paused\" : \"on\";\n",
            "    await api.patch(\"/api/sharings/\" + s.id, { status: next });\n",
            "    toast(next === \"paused\" ? T(\"share.toggle.paused\") : T(\"share.toggle.resumed\"));\n",
            "  }"
        );
        assert!(
            shared_accessor(base_button, base_handler).is_empty(),
            "缺陷形状的那两段被认成了「有共同访问器」—— 规则 3 没有牙"
        );
        assert!(
            base_handler.contains("\"paused\""),
            "自证失效：缺陷的处理器本该按状态字面量 `\"paused\"` 分支"
        );
        assert_eq!(
            shared_accessor(
                "      T(shareToggle(s.status).label) + \"</button> \";",
                "  toast(T(shareToggle(s.status).outcome));"
            ),
            vec!["shareToggle".to_string()],
            "两侧共用访问器时判别式没认出来（规则 3 会假红）"
        );
        assert!(
            called_identifiers("  toast(T(x));").contains("toast"),
            "调用识别漏了普通调用"
        );

        // ⑤ 处理器是**推**出来的：把状态写成字面量的直接端点不算切换
        assert!(
            dynamic_status_owners(
                "  async function d() {\n    await api.patch(\"/api/sharings/\" + s.id, \
                 { status: \"off\" });\n  }\n"
            )
            .is_empty(),
            "把状态写成字面量的端点被当成了切换处理器"
        );
        assert_eq!(
            dynamic_status_owners(
                "  async function t() {\n    await api.patch(\"/api/sharings/\" + s.id, \
                 { status: entry.next });\n  }\n"
            ),
            vec!["t".to_string()],
            "由状态推出的切换端点没被认出来（规则 3 的射程会静默变空）"
        );
    }

    // ══ PART B ══（`#[cfg(test)] mod tests` 内；插在文件收尾 `}` 之前）═══════════════════════════
    /// C2167：CSV 单元格转义器必须把 **RFC 4180 §2.6** 的四个特殊字符**都**放进字符类，并真的拿
    /// 那个 `.test()` 当条件。
    ///
    /// 记录分隔符是 `\r\n`（同一函数里 `…join("\r\n")`），漏掉 CR ⇒ 带**裸 CR** 的字段不加引号
    /// ⇒ 一个字段把一行切成两行（数据完整性/互操作性缺陷；`user` 与 `key` 两列都有存活路径，
    /// 见编辑表 §五 —— 只能由直接调 API 的客户端触发，与 C2043 同一性质）。
    ///
    /// 四条断言各有独立的牙（A/B 每条各有一只隔离臂）：
    /// ①a 元素齐全且**恰**四个 —— 缺 CR 的现役写法在这里红；
    /// ①b 那处 `.test(` 的结果确实被当**条件**用（`?` 在同一条语句里）—— 挡「留着字符类、
    ///    却把 `.test()` 的结果丢掉」这种半修（编辑表 §一 明令保留三目式形状）；
    /// ②  全站**唯一**一处引号字符类实现 —— 挡「第二份 CSV 口径」（同 #230 的「两份实现」族）；
    /// ③  反向：不许退化成「恒加引号」（`/[",\r\n]|/` 这种词法上齐全的逃逸唯一的牙）。
    #[test]
    fn the_csv_cell_escaper_quotes_every_rfc4180_special() {
        let body = function_source(APP_JS, "exportTxCsv").expect("找不到 `exportTxCsv`");
        // 提取器自证：必须停在该函数**自己的**收尾处（吞进邻居会把邻居的字符类也算进来）
        assert!(
            !body.contains("function kbdRows("),
            "提取器吞掉了紧随其后的函数（`function kbdRows(`）：\n{body}"
        );
        assert!(
            body.contains(".join(\"\\r\\n\")"),
            "提取过短：`exportTxCsv` 的记录分隔符那一行不在体内：\n{body}"
        );

        // ── 规则①a：四个元素齐全，且**恰**为四个 ─────────────────────────────────────
        let (line, class, tail) = csv_escaper_class(&body).unwrap_or_else(|e| panic!("{e}"));
        let elems = csv_class_elements(&class);
        for want in ["\"", ",", "\\r", "\\n"] {
            assert!(
                elems.iter().any(|e| e == want),
                "CSV 单元格转义器的字符类 `/{class}/` 少了元素 `{want}`（RFC 4180 §2.6 要求 , \" CR LF 四者）\n这一行是：{line}"
            );
        }
        assert_eq!(
            elems.len(),
            4,
            "字符类 `/{class}/` 应恰有 4 个元素，实际 {} 个：{elems:?}\n这一行是：{line}",
            elems.len()
        );

        // ── 规则①b：`.test(` 的结果被当条件用（不许「留着字符类、丢掉判定」）───────────
        assert!(
            csv_test_drives_the_condition(&line),
            "转义器那一行把 `.test(` 的结果丢了（`.test(` 之后没有 `?`）—— 字符类还在，但加不加引号已不由它决定：\n{line}"
        );

        // ── 规则②：全站唯一实现（`[",` 是引号字符类的开启形状）─────────────────────
        let mut hits: Vec<String> = Vec::new();
        for (name, src) in CSV_CORPUS {
            for text in code_text_by_line(src) {
                if text.contains("[\",") {
                    hits.push(format!("{name}: {text}"));
                }
            }
        }
        assert_eq!(
            hits.len(),
            1,
            "引号字符类的实现必须**恰有一处**，实际 {} 处（出现第二份 CSV 口径就会分叉）：{hits:#?}",
            hits.len()
        );

        // ── 规则③：反向 —— 不许退化成「恒加引号」────────────────────────────────────
        for plain in ["abc", "a b", "中文", ""] {
            assert!(
                !csv_class_matches(&elems, &tail, plain),
                "字符类 `/{class}/{tail}/` 命中了**不需要引号**的 {plain:?} —— 退化成「永远加引号」了"
            );
        }
        for special in ["a,b", "a\"b", "a\rb", "a\nb"] {
            assert!(
                csv_class_matches(&elems, &tail, special),
                "字符类 `/{class}/{tail}/` 漏了特殊字符 {special:?} —— 该字段不会被加引号"
            );
        }
    }

    /// 判别式自证：五种合成形态各判一次，钉住「取不到 / 取到多处 / 元素不全 / 判定被丢 / 语义退化」
    /// **报不同的错**（坑 #291），以及元素分词器不吃子串匹配的亏（坑 #333）。
    ///
    /// ⚠️ 这是**合成输入**，与活树无关 —— 它证明的是判别式有牙，不是缺陷存在。
    #[test]
    fn the_csv_escaper_scanners_have_teeth() {
        // ① 现役写法（漏 CR）⇒ 取得字符类，但只有 3 个元素
        let live = synthetic_escaper("[\",\\n]");
        let (line, class, _) = csv_escaper_class(&live).expect("现役写法应当取得字符类");
        assert!(
            !csv_class_elements(&class).contains(&"\\r".to_string()),
            "现役写法被判成含 CR"
        );
        assert_eq!(
            csv_class_elements(&class).len(),
            3,
            "现役写法的元素个数读错了"
        );
        assert!(
            csv_test_drives_the_condition(&line),
            "现役写法应当过规则①b（它确实用了条件）"
        );

        // ② 候选写法（四元素齐全）⇒ 四条全绿
        let fixed = synthetic_escaper("[\",\\r\\n]");
        let (_, class, tail) = csv_escaper_class(&fixed).expect("候选写法应当取得字符类");
        assert_eq!(tail, "", "候选写法的尾部应当为空");
        assert_eq!(
            csv_class_elements(&class),
            vec![
                "\"".to_string(),
                ",".to_string(),
                "\\r".to_string(),
                "\\n".to_string()
            ],
            "元素分词器读错了候选写法"
        );
        let elems = csv_class_elements(&class);
        assert!(
            !csv_class_matches(&elems, "", "abc"),
            "候选写法命中了普通文本"
        );
        assert!(csv_class_matches(&elems, "", "a\rb"), "候选写法漏了 CR");
        assert!(csv_class_matches(&elems, "", "a\nb"), "候选写法漏了 LF");

        // ③ 「永远加引号」（根本没有字符类）⇒ 报「取不到」，且**不**含「元素」字样
        let always = "  const cell = (v) => { return '\"' + String(v) + '\"'; };\n";
        let err = csv_escaper_class(always).expect_err("没有字符类的写法不该被当成合规");
        assert!(
            err.contains("取不到"),
            "错误文本没区分「取不到」这一种：{err}"
        );
        assert!(
            !err.contains("元素"),
            "「取不到」与「元素不全」必须报**不同**的错（坑 #291）：{err}"
        );

        // ④ 一处以上 ⇒ 报「多处」（判别式不许「找到一个就收工」）
        let two = format!(
            "{}{}",
            synthetic_escaper("[\",\\r\\n]"),
            synthetic_escaper("[\",\\n]")
        );
        let err = csv_escaper_class(&two).expect_err("两处字符类应当被拒");
        assert!(err.contains("2 处"), "「多处」没被点名：{err}");

        // ⑤ 四元素齐全但判定被丢（`.test()` 结果不用）⇒ 规则①a 绿、规则①b 红
        let discarded = "  const cell = (v) => { const s = String(v == null ? \"\" : v); /[\",\\r\\n]/.test(s); return '\"' + s + '\"'; };\n";
        let (line, class, _) = csv_escaper_class(discarded).expect("丢判定的写法仍能取得字符类");
        assert_eq!(
            csv_class_elements(&class).len(),
            4,
            "丢判定的写法本应有四个元素"
        );
        assert!(
            !csv_test_drives_the_condition(&line),
            "`.test()` 结果被丢掉的写法躲过了规则①b"
        );

        // ⑥ 四元素齐全但恒真 ⇒ 规则①绿、规则③红（规则③的独立价值）
        let escape = synthetic_escaper("[\",\\r\\n]|");
        let (line, class, tail) =
            csv_escaper_class(&escape).expect("逃逸写法在词法上仍能取得字符类");
        assert_eq!(
            csv_class_elements(&class).len(),
            4,
            "逃逸写法的四个元素应当被读出"
        );
        assert_eq!(tail, "|", "逃逸写法的尾部应当被读出");
        assert!(
            csv_test_drives_the_condition(&line),
            "逃逸写法仍然是个三目式（所以只有规则③能拒它）"
        );
        assert!(
            csv_class_matches(&csv_class_elements(&class), &tail, "abc"),
            "`|` 尾部（匹配空串 ⇒ 恒加引号）没被规则③抓住"
        );
    }

    /// 造一行「转义器」合成源码（`class` 是**已转义好的**字符类内容，如 `[\",\\n]`）。
    fn synthetic_escaper(class: &str) -> String {
        format!(
            "  const cell = (v) => {{ const s = String(v == null ? \"\" : v); return /{class}/.test(s) ? '\"' + s.replace(/\"/g, '\"\"') + '\"' : s; }};\n"
        )
    }
    /// 运营成员表的「余额」列说的必须是它列头那个词所指的那一半 —— **可花额**（C2173）。
    ///
    /// 产品定义：`available = balance + gift_balance`（`src/routes/wallet.rs`），而全站把**不带限定**的
    /// 「点数余额 / Points balance」（`wallet.balance` / `common.balance` / `dash.balance`）绑给
    /// `available`、把「永久点数 / Permanent points」（`wallet.forever` / `admin.emp.col.perm`）绑给
    /// `balance`。`/api/ops/users` 两个半区**都**返回，前端却只印了 `u.balance` ⇒ 同一张屏幕上，
    /// 用户自己看到 101、运营者在成员表里看到 100，差的就是**可花、会过期**的赠送那一半。
    ///
    /// 三条规则各有各的牙：
    /// - 规则 1（取值）：单元格的取值必须**同时**读到两个半区 —— 只读到永久额＝本缺陷；
    ///   只读到赠送＝把轴修反。
    /// - 规则 2（词）：两包的列头文案必须仍落在**可花族**。把列头改名成「永久点数」也能让两边对上，
    ///   但那是**改这一列声明的口径**（设计变更），不是修 —— 列头用的就是全站给可花额起的那个名字。
    /// - 规则 3（反向）：管理页的「永久点数」列必须**仍然只**读永久额 —— 挡「一键全改成 available」
    ///   那种过度纠正（同屏那份兄弟表把两半各自起对了名，正确范例就是从它那里来的）。
    #[test]
    fn the_ops_members_balance_cell_is_the_half_its_caption_names() {
        let ops = function_source(APP_JS, "renderOps").expect("找不到 renderOps");
        let ops_lines = code_text_by_line(&ops);

        // ── 前置：提取器停对了地方，且填充位点唯一（空集上的断言会假绿，坑 68）──────────────
        assert!(
            ops_lines.iter().any(|l| l.contains("ops-body"))
                && !ops_lines.iter().any(|l| l.contains("function loadOps(")),
            "renderOps 提取错了地方（规则 1 会在空集上假绿）：{}",
            ops_lines.join("\n")
        );
        assert_eq!(
            ops_lines.iter().filter(|l| l.contains("function ")).count(),
            1,
            "renderOps 提取长了（吞进了下一个函数）：{}",
            ops_lines.join("\n")
        );
        let fills: Vec<&String> = ops_lines
            .iter()
            .filter(|l| l.contains(OPS_BALANCE_CAPTION))
            .collect();
        assert_eq!(
            fills.len(),
            1,
            "`{OPS_BALANCE_CAPTION}` 在 renderOps 里出现了 {} 次（单元格要么没了要么重复）",
            fills.len()
        );
        let value = cell_value_rhs(fills[0], OPS_BALANCE_CAPTION)
            .unwrap_or_else(|| panic!("取不到余额单元格的取值表达式：{}", fills[0]));
        assert!(
            value.contains("D.fmt"),
            "取值表达式取错了地方（规则 1 可能判在别的文本上）：`{value}`"
        );

        // ── 规则 1：取值同时读到两个半区 ────────────────────────────────────────────────
        assert!(
            value_reaches_all_fields(APP_JS, &ops, &value, &HALF_FIELDS),
            "运营成员表的余额列没有同时读到 `balance` 与 `gift_balance` —— 列头（`{OPS_BALANCE_CAPTION}`）\
             用的是全站给**可花额**起的那个名字，而这一列只印了永久额，比用户自己在侧栏/钱包里看到的\
             余额少掉全部赠送额：取值 = `{value}`"
        );

        // ── 规则 2：两包的列头文案仍须落在可花族 ────────────────────────────────────────
        let zh = pack_region_strict(I18N_JS, ZH_PACK_START, EN_PACK_START);
        let en = pack_region_strict(I18N_JS, EN_PACK_START, PACK_END);
        for (pack, region) in [("zh", zh), ("en", en)] {
            let spendable = pack_string(region, SPENDABLE_WORD_KEY)
                .unwrap_or_else(|| panic!("{pack} 包里没有键 `{SPENDABLE_WORD_KEY}`"));
            let permanent = pack_string(region, PERMANENT_WORD_KEY)
                .unwrap_or_else(|| panic!("{pack} 包里没有键 `{PERMANENT_WORD_KEY}`"));
            let (s_mark, p_mark) = half_markers(&spendable, &permanent).unwrap_or_else(|| {
                panic!(
                    "{pack} 包的两个半区名推不出各自的标记（尺子坏了）：\"{spendable}\" / \"{permanent}\""
                )
            });
            let caption = pack_string(region, OPS_BALANCE_CAPTION)
                .unwrap_or_else(|| panic!("{pack} 包里没有键 `{OPS_BALANCE_CAPTION}`"));
            assert!(
                caption_names_spendable_half(&caption, &s_mark, &p_mark),
                "{pack} 包的 `{OPS_BALANCE_CAPTION}`（\"{caption}\"）不再落在**可花族**\
                 （标记：可花 `{s_mark}` / 永久 `{p_mark}`）—— 把列头改名成永久族也能让两边对上，\
                 但这一列的头用的就是全站给可花额起的那个名字，改它就是改这一列声明的口径"
            );
        }

        // ── 规则 3（反向）：管理页的「永久点数」列仍只读永久额 ──────────────────────────
        let admin = function_source(APP_JS, "renderAdmin").expect("找不到 renderAdmin");
        let admin_lines = code_text_by_line(&admin);
        let perm: Vec<&String> = admin_lines
            .iter()
            .filter(|l| l.contains(ADMIN_PERM_CAPTION))
            .collect();
        assert_eq!(
            perm.len(),
            1,
            "`{ADMIN_PERM_CAPTION}` 在 renderAdmin 里出现了 {} 次（反向对照的位点不唯一）",
            perm.len()
        );
        let perm_value = cell_value_rhs(perm[0], ADMIN_PERM_CAPTION)
            .unwrap_or_else(|| panic!("取不到「永久点数」列的取值表达式：{}", perm[0]));
        assert!(
            mentions_identifier(&perm_value, "balance"),
            "管理页的「永久点数」列不再读永久额（反向对照失去意义）：取值 = `{perm_value}`"
        );
        assert!(
            !mentions_identifier(&perm_value, "gift_balance"),
            "管理页的「永久点数」列读到了赠送额 —— 那一列的名字说它只有永久额；\
             同一张表本来就另有两列（赠送 / 可用）。这条挡的是「一键全改 available」式过度纠正：\
             取值 = `{perm_value}`"
        );
    }

    /// 规则 1/2/3 的三把尺子各有各的牙（合成输入，坑 68 与 #348）。
    #[test]
    fn the_ops_balance_half_checkers_have_teeth() {
        // 单元格取值提取器：真形状取得到、且**停在取值段**；没有该单元格 ⇒ None
        let line = "      '<td class=\"num\" data-label=\"' + T(\"ops.users.col.balance\") + '\">' + D.fmt(u.balance || 0) + \" \" + T(\"common.points\") + \"</td>\" +";
        assert_eq!(
            cell_value_rhs(line, OPS_BALANCE_CAPTION).as_deref(),
            Some("D.fmt(u.balance || 0) + \" \" + T(\"common.points\")"),
            "取值提取器取错了（规则 1 会判在别的文本上）"
        );
        let admin_line = "        '<td class=\"num\" data-label=\"' + T(\"admin.emp.col.perm\") + '\">' + D.fmt(u.balance || 0) + \"</td>\" +";
        assert_eq!(
            cell_value_rhs(admin_line, ADMIN_PERM_CAPTION).as_deref(),
            Some("D.fmt(u.balance || 0)"),
            "同一个提取器在反向对照那一列上取错了"
        );
        assert!(
            cell_value_rhs("      \"<td></td>\"", OPS_BALANCE_CAPTION).is_none(),
            "没有该单元格时提取器凭空取到了值（规则 1 会在空集上假绿）"
        );
        // 少了 `data-label` 的收尾标记（`'">'`）⇒ 取不到，而不是把后面的东西当取值
        assert!(
            cell_value_rhs(
                "      '<td data-label=\"' + T(\"ops.users.col.balance\") + D.fmt(u.balance || 0) + \"</td>\"",
                OPS_BALANCE_CAPTION
            )
            .is_none(),
            "少了收尾标记时提取器仍取到值（形状变了却静默取值）"
        );

        // 尺子：在**真语言包**上推出的标记
        let zh = pack_region_strict(I18N_JS, ZH_PACK_START, EN_PACK_START);
        let en = pack_region_strict(I18N_JS, EN_PACK_START, PACK_END);
        let (zs, zp) = half_markers(
            &pack_string(zh, SPENDABLE_WORD_KEY).expect("zh 包缺 wallet.balance"),
            &pack_string(zh, PERMANENT_WORD_KEY).expect("zh 包缺 wallet.forever"),
        )
        .expect("zh 半区标记取不到");
        assert_eq!(
            (zs.as_str(), zp.as_str()),
            ("余额", "永久"),
            "zh 半区标记取错了"
        );
        let (es, ep) = half_markers(
            &pack_string(en, SPENDABLE_WORD_KEY).expect("en 包缺 wallet.balance"),
            &pack_string(en, PERMANENT_WORD_KEY).expect("en 包缺 wallet.forever"),
        )
        .expect("en 半区标记取不到");
        assert_eq!(
            (es.as_str(), ep.as_str()),
            ("balance", "permanent"),
            "en 半区标记取错了"
        );
        // 两个名字完全相同 ⇒ 取不到标记（**不得**静默退化成空标记：空标记让 contains 恒真、规则 2 变哑）
        assert!(
            half_markers("points", "points").is_none(),
            "两个名字相同时尺子给出了空标记（规则 2 会静默变哑）"
        );

        // 分类判别式：可花族、永久族、谁都不指（化妆）三态都要判对
        assert!(caption_names_spendable_half("余额（点数）", &zs, &zp));
        assert!(caption_names_spendable_half("Balance (pts)", &es, &ep));
        assert!(
            !caption_names_spendable_half("永久点数（点数）", &zs, &zp),
            "改名成永久族没有被拒（那一列口径被换了却判绿）"
        );
        assert!(
            !caption_names_spendable_half("Permanent points (pts)", &es, &ep),
            "改名成永久族没有被拒（en 侧）"
        );
        assert!(
            !caption_names_spendable_half("点数", &zs, &zp),
            "谁都不指的化妆文案没有被拒（它既不是可花族也不是永久族）"
        );
        assert!(
            !caption_names_spendable_half("Points", &es, &ep),
            "谁都不指的化妆文案没有被拒（en 侧）"
        );

        // 规则 1 的牙：只读永久额 ⇒ 判红；只读赠送 ⇒ 判红；两项都读（含抽 helper 的等价写法）⇒ 判绿
        let bare =
            "  function renderOps() { const row = \"<td>\" + D.fmt(u.balance || 0) + \"</td>\"; }";
        assert!(
            !value_reaches_all_fields(bare, bare, "D.fmt(u.balance || 0)", &HALF_FIELDS),
            "规则 1 放行了只读永久额的取值（本缺陷的形状）"
        );
        let gift_only =
            "  function renderOps() { const row = \"<td>\" + D.fmt(u.gift_balance || 0) + \"</td>\"; }";
        assert!(
            !value_reaches_all_fields(
                gift_only,
                gift_only,
                "D.fmt(u.gift_balance || 0)",
                &HALF_FIELDS
            ),
            "规则 1 放行了只读赠送的取值（轴修反）"
        );
        let both = "  function renderOps() { const row = \"<td>\" + D.fmt((u.balance || 0) + (u.gift_balance || 0)) + \"</td>\"; }";
        assert!(
            value_reaches_all_fields(
                both,
                both,
                "D.fmt((u.balance || 0) + (u.gift_balance || 0))",
                &HALF_FIELDS
            ),
            "规则 1 把两项都读到的取值判红了"
        );
        // 说明性注释里提到 `gift_balance` 不算证据（#296 的镜像：被自己的解释满足）
        let commented = "  function renderOps() { const row = \"<td>\" + D.fmt(u.balance || 0) + \"</td>\"; // 另一半是 u.gift_balance\n  }";
        assert!(
            !value_reaches_all_fields(commented, commented, "D.fmt(u.balance || 0)", &HALF_FIELDS),
            "规则 1 被注释里的 `gift_balance` 满足了（假绿）"
        );
    }

    // =============================================================================================
    // C2171 — 身份边界只隐藏 `#app`，而浮层是它的**兄弟节点**
    // =============================================================================================
    //
    // `exitGuest()` 是 UI 的身份边界。它隐藏 `#app`；但 `ui/index.html` 里还有若干「登录后才存在」
    // 的浮层是 `#app` 的**兄弟节点** ⇒ 隐藏 `#app` **不会**连带隐藏它们。不清它们，上一个会话的
    // 面板会浮在登录页上，并在**下一个人登录后被继承**：`renderHelp()` 只在打开时渲染 ⇒
    // `#help-context` 还印着上一位用户的视图，且没有任何渲染路径会重画它（永不自愈）。
    //
    // 元素集合是**派生**的 —— `ui/index.html` 中 `#app` 起始行之后的**顶行**（列 0）元素，其
    // `class` 属性含独立 token `hidden`。**零豁免清单**：往 index.html 加一个登录态浮层而不收它，
    // 这里必红。`resetSessionOverlays()` 只负责把每个元素交给它自己的关闭器。
    //
    // ⚠️ 射程（词法）：本门禁证明「派生集合里每个元素，各有一个**宣称要隐藏它**的关闭器落在
    // `resetSessionOverlays()` 的调用闭包内」，**不**证明运行期屏幕上真的隐藏了 —— 后者是
    // `c2171-probe.js` 的射程（仓内 CI 无 JS 运行器）。已知盲区：① 只认顶行元素（缩进看不见）；
    // ② 只认字面量 `"#<id>"`（动态选择器看不见）；③ `classList.toggle("hidden", false)` 这种
    // 带第二布尔实参的隐藏不在判别式内。

    /// `class` 属性里是否含**独立** token `hidden`（`hidden-x` / `unhidden` 不算）。
    fn has_hidden_class_token(cls: &str) -> bool {
        cls.split_whitespace().any(|t| t == "hidden")
    }

    /// 文本里所有 `"#<id>"` 字面量的 `<id>`（只收 `[A-Za-z0-9_-]`，动态选择器看不见）。
    fn quoted_hash_ids(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let bytes = text.as_bytes();
        let mut i = 0usize;
        while i + 2 < bytes.len() {
            if bytes[i] == b'"' && bytes[i + 1] == b'#' {
                let start = i + 2;
                let mut j = start;
                while j < bytes.len() && bytes[j] != b'"' {
                    j += 1;
                }
                if j < bytes.len() {
                    let id = &text[start..j];
                    if !id.is_empty()
                        && id
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                    {
                        out.push(id.to_string());
                    }
                    i = j + 1;
                    continue;
                }
            }
            i += 1;
        }
        out
    }

    /// `ui/index.html` 中 **`#app` 起始行之后**的顶行（列 0）元素，其 `class` 含独立 token
    /// `hidden` ⇒ 这就是「登录后才存在、且不随 `#app` 一起被隐藏」的浮层集合。
    fn overlays_outside_app(html: &str) -> Vec<String> {
        let clean = strip_html_comments(html);
        let mut lines = clean.lines();
        let mut found_app = false;
        for l in lines.by_ref() {
            if l.starts_with("<div id=\"app\"") {
                found_app = true;
                break;
            }
        }
        if !found_app {
            return Vec::new();
        }
        let mut out = Vec::new();
        for line in lines {
            let Some(rest) = line.strip_prefix("<div id=\"") else {
                continue;
            };
            let Some(q) = rest.find('"') else { continue };
            let id = &rest[..q];
            let attrs = &rest[q + 1..];
            let Some(c) = attrs.find("class=\"") else {
                continue;
            };
            let after = &attrs[c + "class=\"".len()..];
            let Some(e) = after.find('"') else { continue };
            if has_hidden_class_token(&after[..e]) {
                out.push(id.to_string());
            }
        }
        out
    }

    /// 该函数的**代码体**（调用方须已剥注释）是否**宣称**把 `#<id>` 隐藏：既出现字面量
    /// `"#<id>"`，又出现把 `hidden` **加**上去的操作。
    ///
    /// 两个条件都必须按**字面量**判 —— 裸 `contains(id)` 会被兄弟标识符骗到（坑 #333 同族：
    /// `renderTourStep` 也提到 `#tour-ring` 且含 `ring.classList.add("hidden")`，它是个**渲染器**）。
    fn hides_element(body_code: &str, id: &str) -> bool {
        if !body_code.contains(&format!("\"#{id}\"")) {
            return false;
        }
        body_code.contains(".classList.add(\"hidden\")")
            || body_code.contains(".classList.toggle(\"hidden\"")
    }

    /// 从 `resetSessionOverlays()` 出发的传递调用闭包（函数名集合）。
    ///
    /// ⚠️ **自建**闭包，**不复用** `call_graph` / `reachable`：那两个的每条边都由 `js_function_body`
    /// 取体，而后者对**单行**函数会一路吞到下一个 `  }`。`ui/js/app.js` 里的 `markTourDone`
    /// 正是单行 ⇒ 旧写法会把紧随其后的 `startTour` / `renderTourStep` / `switchView` 拉进闭包，
    /// 于是 `C` 溢出成整份文件（22 个 view 元素 id）。这是「编译＋实跑」才逮到的真缺陷
    /// （坑 #319/#332：`function_source` 就是为这个坑写的）。射程局限记在本节顶部。
    fn overlay_closure(src: &str) -> BTreeSet<String> {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut queue: Vec<String> = vec!["resetSessionOverlays".to_string()];
        while let Some(f) = queue.pop() {
            if !seen.insert(f.clone()) {
                continue;
            }
            // 箭头函数常量（如 `esc`）没有 `function` 声明头 ⇒ 返回 None，不参与（不是空串）
            let Some(body) = function_source(src, &f) else {
                continue;
            };
            let code = code_lines(&body);
            for c in callee_names(&code) {
                if !seen.contains(&c) {
                    queue.push(c);
                }
            }
        }
        seen
    }

    /// 闭包里各函数的代码体宣称隐藏的 `#<id>` 集合。
    fn closure_hidden_elements(src: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for f in overlay_closure(src) {
            let Some(body) = function_source(src, &f) else {
                continue;
            };
            let code = code_lines(&body);
            for id in quoted_hash_ids(&code) {
                if hides_element(&code, &id) {
                    out.insert(id);
                }
            }
        }
        out
    }

    /// 身份边界必须把 `#app` **之外**的浮层也收起（集合派生自 index.html，零豁免清单）。
    #[test]
    fn the_identity_boundary_closes_the_panels_outside_the_app() {
        // ── 规则 4：派生必须有阳性对照（非空、且不含登录视图 / 常驻容器）────────────────
        let derived: BTreeSet<String> = overlays_outside_app(INDEX_HTML).into_iter().collect();
        let expected: BTreeSet<String> = [
            "help-panel",
            "chat-modal",
            "tour-overlay",
            "tour-ring",
            "tour-pop",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
        assert!(
            !derived.is_empty()
                && !derived.contains("login-view")
                && !derived.contains("toast-wrap"),
            "派生集合的阳性对照失败（解析器变了，还是 index.html 结构变了？）：{derived:?}"
        );
        assert_eq!(
            derived,
            expected,
            "`ui/index.html` 里 `#app` 之后的浮层集合变了 —— 若是有意新增，请一并让 resetSessionOverlays 收它"
        );

        // ── 规则 1（主牙）：闭包收起集合 == 派生集合 ────────────────────────────────────
        let closed = closure_hidden_elements(APP_JS);
        assert_eq!(
            closed,
            derived,
            "resetSessionOverlays 的调用闭包没有恰好覆盖 `#app` 之外的每个浮层（少收 ⇒ 面板会留下来）"
        );

        // ── 规则 2：身份边界必须调用该复位 ──────────────────────────────────────────────
        let boundary = code_body(APP_JS, "exitGuest");
        assert!(
            !boundary.is_empty(),
            "提取器没取到 exitGuest 的代码体（后面的断言会在空串上「通过」）"
        );
        assert!(
            boundary.contains("resetSessionOverlays()"),
            "exitGuest（身份边界）没有收起 `#app` 之外的浮层"
        );

        // ── 规则 3（反向）：边界不许被掏空 ─────────────────────────────────────────────
        assert!(
            boundary.contains("resetSessionCaches()"),
            "边界不再清空会话缓存"
        );
        assert!(
            boundary.contains("(\"#app\").classList.add(\"hidden\")"),
            "边界不再隐藏 `#app`"
        );
        assert!(
            boundary.contains("(\"#login-view\").classList.remove(\"hidden\")"),
            "边界不再显示登录视图"
        );
    }

    /// 判别式的牙：两个条件都必须按**字面量**判；闭包限定必须排除「渲染器」形状的假阳性；
    /// 提取器必须**单行安全**（本轴被这条咬过一次）。
    #[test]
    fn the_c2171_overlay_extractors_have_teeth() {
        // 1) 有 add("hidden") 但不提该 id ⇒ 不算收起
        assert!(!hides_element("x.classList.add(\"hidden\");", "help-panel"));
        // 2) 提了 id 但只有 remove ⇒ **渲染器**形状，不算收起（`renderTourStep` 对 `#tour-ring` 正是如此）
        assert!(!hides_element(
            "$(\"#tour-ring\").classList.remove(\"hidden\");",
            "tour-ring"
        ));
        // 3) 真形状 ⇒ 算（两种写法都要认）
        assert!(hides_element(
            "$(\"#chat-modal\").classList.add(\"hidden\");",
            "chat-modal"
        ));
        assert!(hides_element(
            "const panel = $(\"#help-panel\");\npanel.classList.toggle(\"hidden\", !open);",
            "help-panel"
        ));
        // 4) 派生：`#toast-wrap` 无 class ⇒ 不入集合；`#app` 自己与它**之前**的元素也不算
        let synth = "<div id=\"login-view\" class=\"login-view\"></div>\n\
                 <div id=\"app\" class=\"app hidden\"></div>\n\
                 <div id=\"toast-wrap\"></div>\n\
                 <div id=\"help-panel\" class=\"help-panel hidden\"></div>\n";
        assert_eq!(
            overlays_outside_app(synth),
            vec!["help-panel".to_string()],
            "派生规则：只收 `#app` 之后的、带 hidden token 的顶行元素"
        );
        // 5) 字面量 id 提取器（token 边界：`#a-b` 与 `#c` 是两个 id；动态选择器看不见）
        assert_eq!(
            quoted_hash_ids("$(\"#a-b\")+\"#c\""),
            vec!["a-b".to_string(), "c".to_string()]
        );
        assert!(quoted_hash_ids("`#${x}`").is_empty());
        // 6) 实际文件里的**单行**函数：体必须只有一行，且不许吞到下一个函数
        //    （`js_function_body` 在这里会一路吞到下一个 `  }` —— 闭包溢出成整份文件就是这个原因）
        let single = function_source(APP_JS, "markTourDone").unwrap_or_default();
        assert_eq!(
            single.lines().count(),
            1,
            "单行函数被吞了（须用 function_source，不是 js_function_body）：{single}"
        );
        assert!(
            !single.contains("startTour"),
            "单行函数吞到了它后面的函数：{single}"
        );
        // 7) 箭头函数常量没有 `function` 声明头 ⇒ 提取器返回 None（不是空串），调用方要 `continue`。
        //    ⚠️ **合成输入，不用真实文件** —— 真实文件里 `esc` **同时**还有一个**具名函数表达式**
        //    （`app.js:323` `document.addEventListener("keydown", function esc(e) {`）⇒ 提取器**该**
        //    返回 Some。踩过：本条断言原写作 `function_source(APP_JS, "esc").is_none()`，在**所有**树上
        //    恒红（自检自红 ⇒ 自检不成立）。教训：**名字不是标识符的唯一载体** —— 同一名字可以有
        //    箭头常量、具名函数声明、具名函数表达式三种载体，且可以同时存在。
        let arrow_only = "  const foo = (x) => x;\n  function bar() {\n    foo(1);\n  }\n";
        assert!(
            function_source(arrow_only, "foo").is_none(),
            "箭头常量不该被当成函数声明体"
        );
        assert!(
            function_source(arrow_only, "bar").is_some(),
            "普通函数声明必须被取到"
        );
        // 7b) 真实文件上的**同款碰撞**（记录性钉）：`esc` 的第二次出现是具名函数表达式 ⇒ 会命中。
        //     这不是缺陷 —— 闭包走的四个名字（resetSessionOverlays/closeTour/closeChat/toggleHelp）
        //     全部是唯一声明（见下一条），所以碰撞不影响本轴。若日后给闭包添一个与箭头常量同名的
        //     具名函数表达式，那才是真问题，故把它钉住。
        assert!(
            function_source(APP_JS, "esc").is_some(),
            "`esc` 的具名函数表达式消失了？闭包对名字碰撞的容忍度需要重估"
        );
        // 8) 闭包**不许**被单行函数带跑：真实文件上的闭包里不得出现 `startTour` 之后的东西。
        //    合成输入里把 `closeTour` 写成**单行**，正是复现那条吞并路径。
        let synth_src = "  function resetSessionOverlays() {\n    closeTour();\n  }\n\
                     \x20 function closeTour() { $(\"#tour-ring\").classList.add(\"hidden\"); }\n\
                     \x20 function renderTourStep() {\n    $(\"#tour-ring\").classList.remove(\"hidden\");\n    ring.classList.add(\"hidden\");\n  }\n";
        let syn = overlay_closure(synth_src);
        assert!(
            syn.contains("closeTour") && !syn.contains("renderTourStep"),
            "单行函数把后面的函数带进了闭包：{syn:?}"
        );
        let real = overlay_closure(APP_JS);
        assert!(
            !real.contains("switchView")
                && !real.contains("renderTourStep")
                && !real.contains("startTour"),
            "闭包被单行函数带跑（`markTourDone` 那条吞并路径）：{real:?}"
        );
        // 9) **真实反例**：`renderTourStep` 确实会被裸判别式当成收起器 —— 排除它的是**闭包限定**。
        //    （用 `function_source`：`function_code` 走的是会吞单行函数的 `js_function_body`。）
        let body = function_source(APP_JS, "renderTourStep")
            .map(|b| code_lines(&b))
            .unwrap_or_default();
        assert!(
            hides_element(&body, "tour-ring"),
            "假阳性必须先存在，本测试才有意义（renderTourStep 的形状变了？）"
        );
        // 10) 闭包走到的每个名字都必须是**唯一声明** —— 否则 `function_source` 可能取到**另一个**载体
        //     （`esc` 那样：箭头常量 + 具名函数表达式），闭包就会凭一个名字窜到无关代码里。
        for f in overlay_closure(APP_JS) {
            if function_source(APP_JS, &f).is_none() {
                continue; // 不存在的名字（如未修树上的 resetSessionOverlays）：无体可走
            }
            let decls = APP_JS.matches(&format!("function {f}(")).count();
            assert_eq!(
            decls, 1,
            "闭包成员 `{f}` 有 {decls} 处 `function {f}(` 声明 —— 名字不是标识符的唯一载体，闭包可能窜到别处"
        );
        }
    }
}

/// C2170：身份边界必须连**模块级**视图状态一起清 —— `Live` 之外的会话状态同样跨不过边界。
///
/// 反例（实测，`c2170-probe.js`）：`resetSessionCaches()` 只清 `Object.keys(Live)`，而交易视图的
/// 分页/筛选/时间段是模块级的 —— 尤其 `txTable.loaded*` 是**载荷的有效性证据**。载荷
/// （`Live.transactions`）被清空而证据留下 ⇒ 下一位用户进入交易视图时守卫按 `txTable.page`
/// （上一位用户的第 5 页）发请求；服务端 `offset=(page-1)*page_size` + `LIMIT ? OFFSET ?`
/// （`wallet.rs:389/396/410`）回 `items: []` 而 `total: 3` ⇒ 屏幕上「没有匹配的记录」旁边写着
/// 「共 3 条」，且**不自愈**（`buildDataTable` 把 `state.page` 夹到 1 发生在渲染**内部**，守卫
/// 不会因此重跑；实测 2.5s 内零次纠正请求）。同一跳里 `type=consume` + 7 天 `start=` 也从
/// 上一位用户手里带过来，新用户的视图被静默收窄。
#[test]
fn the_identity_boundary_resets_the_transaction_view_state() {
    let src = code_only(APP_JS);

    // ── 派生：载荷守卫**真正依赖**的状态（不手抄名册，坑 #293 的两侧牙齿）────────────
    let fields = object_literal_keys(&src, "txTable").expect("找不到 `txTable` 对象字面量");
    let inputs: Vec<String> = fields.iter().map(|f| format!("txTable.{f}")).collect();
    let evidence = names_with_prefix(&src, "txTable.loaded");
    let sig = code_body(&src, "txQuerySig");
    let lets = module_level_lets(&src);
    let mut ranges: Vec<String> = identifiers(&sig)
        .into_iter()
        .filter(|n| lets.contains(n))
        .collect();
    ranges.sort();

    // ── 前置：提取器必须真的看见东西（空集上的断言会假绿，坑 68）─────────────────────
    assert!(inputs.len() >= 4, "`txTable` 只派生出 {inputs:?}");
    assert!(
        evidence.len() >= 3,
        "只派生出 {evidence:?} 个 `txTable.loaded*`"
    );
    assert!(
        !sig.is_empty(),
        "找不到 `txQuerySig()` 的函数体 —— 提取器坏了"
    );
    assert!(
        ranges.len() >= 3,
        "`txQuerySig()` 读到的模块级状态只有 {ranges:?} —— 判别式太弱"
    );
    assert_eq!(
        declared_literal(&src, "txTable", "page").as_deref(),
        Some("1"),
        "`txTable.page` 的声明默认值变了 —— 规则 3 的判别式要跟着改"
    );

    let (boundary, fns) = boundary_reset(&src);
    assert!(!boundary.is_empty(), "找不到 `resetSessionCaches()` 的闭包");
    // 反-吞并守卫（坑 #319/#332）：`js_function_body` 对**单行**函数会吞进下一个多行函数 ——
    // 抽到的「证据」可能来自别的函数。多行函数下声明行数应恰等于闭包大小
    // （若边界闭包里出现了嵌套 `function` 声明，请把这条守卫改成按名字逐个断言）。
    assert_eq!(
        boundary.matches("function ").count(),
        fns.len(),
        "边界闭包提取可疑（函数声明数 != 闭包大小）：{fns:?} —— 提取吞了别的函数，或边界里有嵌套声明"
    );

    // ── 规则 1：闭包必须复位每一个被守卫依赖的名字 ────────────────────────────────
    for lhs in inputs.iter().chain(evidence.iter()).chain(ranges.iter()) {
        assert!(
            assigns_in(&boundary, lhs),
            "身份边界没有复位 `{lhs}`（C2170）：`Live` 之外的模块级视图状态同样跨不过边界。\
             载荷的有效性证据留下 ⇒ 守卫认一份「不存在的载荷」为已加载 ⇒ 下一位用户首帧即空表\
             且不自愈。边界闭包 = {fns:?}"
        );
    }

    // ── 规则 3：复位到**声明处的默认值**，不是一键清空 ────────────────────────────
    let page = declared_literal(&src, "txTable", "page").expect("`txTable.page` 没有声明默认值");
    let size =
        declared_literal(&src, "txTable", "pageSize").expect("`txTable.pageSize` 没有默认值");
    let range = declared_let_literal(&src, "txRange").expect("`txRange` 没有声明默认值");
    for (lhs, lit) in [
        ("txTable.page", page.as_str()),
        ("txTable.pageSize", size.as_str()),
        ("txRange", range.as_str()),
    ] {
        let want = format!("{lhs} = {lit}");
        assert!(
            boundary.contains(&want),
            "复位没有回到声明处的默认值（期望 `{want}`）：清成 `undefined` 只会让 \
             `loadTransactions()` 的 `Math.max(1, txTable.pageSize || 10)` 把每页退化到 1 行"
        );
    }

    // ── 规则 2：有效性证据的写者只有装载器与身份边界 ──────────────────────────────
    let graph = call_graph(&src);
    let writers: Vec<String> = graph
        .keys()
        .filter(|f| {
            let code = function_code(&src, f);
            evidence.iter().any(|e| assigns_in(&code, e))
        })
        .cloned()
        .collect();
    let allowed: BTreeSet<String> = std::iter::once("loadTransactions".to_string())
        .chain(fns.iter().cloned())
        .collect();
    let strays: Vec<&String> = writers.iter().filter(|w| !allowed.contains(*w)).collect();
    assert!(
        strays.is_empty(),
        "`txTable.loaded*` 被 {strays:?} 写 —— 除装载器与身份边界外谁都不许写它：在 \
         `renderTransactions()` 里清会把守卫每次渲染都重新武装 ⇒ 请求风暴（C2146 同形）"
    );
}

/// C2170 提取器自证：两侧锚定、注释剥离、闭包跨函数，都要在**合成输入**上有牙齿。
#[test]
fn the_session_state_scanners_have_teeth() {
    // (a) 右侧锚定：`txTable.page` 是 `txTable.pageSize` 的前缀 —— 复位了 pageSize 不算复位 page
    assert!(
        !assigns_in("    txTable.pageSize = 10;\n", "txTable.page"),
        "兄弟字段被当成了证据（坑 #333）"
    );
    assert!(assigns_in("    txTable.page = 1;\n", "txTable.page"));
    assert!(
        !assigns_in("    if (txTable.page === 1) x();\n", "txTable.page"),
        "比较被当成了赋值"
    );
    assert!(
        !assigns_in("    const y = txTable.page;\n", "txTable.page"),
        "读取被当成了赋值"
    );
    // (b) 左侧锚定
    assert!(!assigns_in("    xtxTable.page = 1;\n", "txTable.page"));
    // (c) 注释不是证据（坑 #296 的镜像：修法自己就在复位旁写了提到字段名的解释）
    let commented = code_only(
        "  function resetTxView() {\n    // txTable.page = 1;\n    txTable.pageSize = 10;\n  }\n",
    );
    assert!(!assigns_in(&commented, "txTable.page"), "注释被当成了赋值");
    // (d) 前缀扫描不吞兄弟
    let synth = "  a = txTable.loadedPage;\n  b = txTable.loadedPageSize;\n";
    assert_eq!(
        names_with_prefix(synth, "txTable.loaded"),
        vec![
            "txTable.loadedPage".to_string(),
            "txTable.loadedPageSize".to_string()
        ]
    );
    // (e) 闭包必须跨函数：复位写在被调用者里也算（抽 helper 是推荐写法）
    let synth2 = concat!(
        "  function resetSessionCaches() {\n",
        "    Object.keys(Live).forEach((k) => { Live[k] = null; });\n",
        "    resetTxView();\n",
        "  }\n\n",
        "  function resetTxView() {\n",
        "    txTable.page = 1;\n",
        "  }\n"
    );
    let (text, fns) = boundary_reset(synth2);
    assert!(
        fns.contains(&"resetTxView".to_string()),
        "闭包没跨函数：{fns:?}"
    );
    assert!(
        assigns_in(&text, "txTable.page"),
        "闭包文本里读不到被调用者的赋值"
    );
    // (f) 模块级 `let` 只认 2 空格缩进的声明
    assert_eq!(
        module_level_lets("  let txRange = \"24h\";\n    let nested = 1;\n"),
        vec!["txRange".to_string()]
    );
}

// ===================================================================================================
// R155：**载荷的收据必须由发起它的那次请求写**，不是由最后落地的响应「问一次现在」。
// ===================================================================================================
//
// 题眼（`ui/js/app.js` `loadTransactions()`）：一连三行收据描述**同一份响应** ——
// `loadedPage` / `loadedPageSize` 在 `await` **之前**取好（`const page = …` 自己的快照），
// 第三条 `loadedQuerySig` 却在响应落地后读**活状态** `txQuerySig()`；而 `renderTransactions()`
// 的守卫拿它跟**现在**的控件比对（`txTable.loadedQuerySig !== txQuerySig()`）⇒ 印章与投影同源，
// 比对恒成立。两次控件变更落在同一个 RTT 内时两个请求同时在飞，被取代的那个不带身份；它若最后
// 落地就会被**采纳并认证为当前**，守卫随即满足 ⇒ 表格永久显示用户已经离开的筛选条件的行。
//
// 错的不是**写错的值**，而是**问错了人**（所以它能自证清白）⇒ 修法＝把签名**在发请求时就捕获**。
//
// 四条规则（全部从代码**派生**，#469：门禁不许把这一次编辑的字面量写死作判据）：
//
//   R1  每一条收据赋值的右值**不得是一次调用**（`(`）—— 收据必须是「发请求那一刻」的值，
//       不是「响应落地那一刻」才求值的东西。
//   R2  右值是**裸标识符**时，该标识符必须在**同一个函数体内**、且在**第一个 `await` 之前**
//       被绑定（挡「await 之后再捕获」这种同形假修）。
//   R3  签名那条收据（由守卫行派生：与一次**调用**比较的那一项）捕获的标识符，其初始化式必须
//       调用**守卫自己比较的那个函数**（由守卫派生）—— 挡「捕获一个常量」式假修（那会让守卫恒
//       不相等 ⇒ 请求风暴，比原缺陷更坏）。
//   R4  反向／非空：守卫仍然存在且仍与一次**调用**比较（派生出的函数名非空）、收据名册非空、
//       至少两条收据写入 —— 否则 R1–R3 会在空集上「通过」（坑 68 家族）。
//
// 射程（如实，并已写进 `ui/README.md`）：本门禁是**词法**的。它证明「收据的右值是被捕获的、
// 且在 `await` 之前、且是守卫比较的那个函数」，**不**证明屏幕上那一刻的行真的属于当前筛选
// （那一半归 jsdom 探针的 A1/A2/A3 腿）。两个仪器在**纯代次版**（`fix_generation`：只丢弃被
// 取代的响应、盖章那行照旧读活状态）上**不同判**：探针接受它（终态正确、还少发一次请求），
// 本门禁**拒**它（R1）—— 因为「收据的作者」在这条轴上是**同一个主张**，一份收据要由两个机制
// 分别保证才是真缺陷未修。这个不对称是**故意**的、可测量的（见 `the_r155_rules_separate_the_variants`）。

// ⛔ 不在这里再 `include_str!` 一次 `ui/js/app.js` —— 本模块顶部（`APP_JS`）已经有了。

/// 收据字段的前缀（`txTable.loaded*`）。
const R155_PREFIX: &str = "txTable.loaded";

/// 修复体的两半（逐字来自 `ui/js/app.js`；两处必须同时出现，缺一即未修）。
///
/// `R155_CAPTURE` 连它上方那段解释性注释一起收进来 —— 这样 `r155_variant_unfixed` 还原
/// 出的树与修复前的文件**逐字相同**（留下一段解释「已被撤掉的修法」的散文是另一种谎）。
const R155_CAPTURE: &str = concat!(
    "    // R155：收据（`txTable.loadedQuerySig`）必须取自**这次请求自己**的签名，在发请求前捕获。\n",
    "    // 响应落地后再调 `txQuerySig()`，读到的是**此刻**的控件；两次控件变更落在同一个 RTT 内时，\n",
    "    // 被取代的那份响应会拿它盖章，而守卫比较的正是**现在**（`!== txQuerySig()`）⇒ 比对恒成立、\n",
    "    // 表格永久停在用户已经离开的筛选条件的行上（同块的 `page`/`pageSize` 一直是对的写法）。\n",
    "    const reqSig = txQuerySig(); // R155：本请求自己的载荷签名（收据的右值不读活状态）\n",
);
const R155_STAMP: &str = "      txTable.loadedQuerySig = reqSig; // 记录已加载的载荷签名（取自本请求），变化时 renderTransactions 重拉\n";

/// 未修形状（原树那一行）。
const R155_STAMP_BASE: &str = "      txTable.loadedQuerySig = txQuerySig(); // 记录已加载的载荷签名，变化时 renderTransactions 重拉\n";

/// 未修形状里那一行**之后**紧邻的锚（插入捕获用的位置）—— 逐字来自 `ui/js/app.js`。
const R155_ANCHOR_AFTER_COLS: &str =
    "    const cols = txFilterParams(); // rant 2026-08-25T10:33:26：列筛选随请求发出，后端全量过滤\n";

/// `await Promise.all([` 那一行（R2 的分界线也是**派生**的：取函数体里第一条含 `await` 的行，
/// 这里只用于**构造**变异体，不用于判定）。
const R155_AWAIT_LINE: &str = "      const [, trend] = await Promise.all([";

// --------------------------------------------------------------------------------- helpers ---

/// 每个函数的 `(名字, 起始行, 结束行)`（1 基；结束行＝首个恰为 `  }` 的行）。
///
/// 与 `state_gate.rs` 的 `js_function_body` 同一约定：本文件函数体一律 2 空格缩进、收尾行恰为
/// `  }`。调用方须自证提取结果非空（见 `the_r155_extractor_lands_on_real_function_bodies`）。
fn r155_function_ranges(src: &str) -> Vec<(String, usize, usize)> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        if let Some(name) = function_name(lines[i]) {
            let mut end = None;
            for (j, l) in lines.iter().enumerate().skip(i + 1) {
                if *l == "  }" {
                    end = Some(j);
                    break;
                }
            }
            if let Some(e) = end {
                out.push((name.to_string(), i, e));
                i = e + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// 收据名册：全文件里出现过的 `txTable.loaded<Field>` 字段名（去重，**派生**）。
fn r155_receipt_roster(src: &str) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    for line in code_text_by_line(src) {
        let mut from = 0usize;
        while let Some(rel) = line[from..].find(R155_PREFIX) {
            let at = from + rel + R155_PREFIX.len();
            let rest = &line[at..];
            let field: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
                .collect();
            if !field.is_empty() {
                out.insert(format!("{R155_PREFIX}{field}"));
            }
            from = at;
            if from >= line.len() {
                break;
            }
        }
    }
    out
}

/// 一条收据写入：`(行号(1 基), 字段全名, 右值, 所属函数)`。
///
/// 「写」＝该行在字段名之后（跨过标识符剩余字符）紧跟一个赋值号（不是 `==`，同
/// `state_gate.rs::assignment_after`）。右值取 `=` 之后、行尾注释之前，两端去空白。
fn r155_receipt_writes(src: &str) -> Vec<(usize, String, String, String)> {
    let text = code_text_by_line(src);
    let ranges = r155_function_ranges(src);
    let mut out = Vec::new();
    for (i, line) in text.iter().enumerate() {
        let lineno = i + 1;
        let mut from = 0usize;
        while let Some(rel) = line[from..].find(R155_PREFIX) {
            let at = from + rel + R155_PREFIX.len();
            let rest = &line[at..];
            let fld: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
                .collect();
            if fld.is_empty() {
                from = at;
                continue;
            }
            let after = &rest[fld.len()..];
            let trimmed = after.trim_start();
            if !trimmed.starts_with('=') || trimmed.starts_with("==") {
                from = at;
                if from >= line.len() {
                    break;
                }
                continue;
            }
            // 行尾 `//` 注释（本仓写法的形态是 `…; // 说明`）不属于右值；注释**行**与
            // `/* … */` 已由 `code_text_by_line` 剥掉（与 `state_gate.rs` 同一约定）。
            let rhs = trimmed[1..]
                .split(" //")
                .next()
                .unwrap_or("")
                .trim()
                .trim_end_matches(';')
                .trim()
                .to_string();
            let owner = ranges
                .iter()
                .find(|(_, s, e)| lineno > *s && lineno <= *e)
                .map(|(n, _, _)| n.clone())
                .unwrap_or_default();
            out.push((lineno, format!("{R155_PREFIX}{fld}"), rhs, owner));
            from = at;
            if from >= line.len() {
                break;
            }
        }
    }
    out
}

/// 守卫行：`txTable.loaded<Field> !== <RHS>`，且 `RHS` 到第一个 `(` 之间恰好是一个标识符
/// ⇒ `(字段, 函数名)`。找不到（守卫被删掉，或改成与普通值比较）⇒ `None`。
fn r155_guard_sig(src: &str) -> Option<(String, String)> {
    for line in code_text_by_line(src) {
        if !line.contains(R155_PREFIX) || !line.contains("!==") {
            continue;
        }
        let mut from = 0usize;
        while let Some(rel) = line[from..].find(R155_PREFIX) {
            let at = from + rel + R155_PREFIX.len();
            let rest = &line[at..];
            let fld: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
                .collect();
            if fld.is_empty() {
                from = at;
                continue;
            }
            let after = rest[fld.len()..].trim_start();
            if let Some(rhs) = after.strip_prefix("!==") {
                let rhs = rhs.trim_start();
                if let Some(paren) = rhs.find('(') {
                    let name = rhs[..paren].trim();
                    if !name.is_empty()
                        && name
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
                    {
                        return Some((format!("{R155_PREFIX}{fld}"), name.to_string()));
                    }
                }
            }
            from = at;
            if from >= line.len() {
                break;
            }
        }
    }
    None
}

/// JS 字面量右值（`undefined` / `null` / 布尔 / 数字 / 字符串）：**不需要**捕获来源，
/// 因此 R2/R3 不适用（`resetTxView()` 的三行 `= undefined` 就是这种形状）。
fn r155_is_literal_rhs(rhs: &str) -> bool {
    let r = rhs.trim();
    if r.is_empty() {
        return false;
    }
    if matches!(
        r,
        "undefined" | "null" | "true" | "false" | "NaN" | "Infinity"
    ) {
        return true;
    }
    if r.starts_with('"') || r.starts_with('\'') || r.starts_with('`') {
        return true;
    }
    r.chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
}

/// 裸标识符右值。
fn r155_is_bare_identifier(rhs: &str) -> bool {
    let r = rhs.trim();
    !r.is_empty()
        && !r.starts_with(|c: char| c.is_ascii_digit())
        && r.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// 在函数体内找 `const|let|var <id> =` 的声明行（1 基行号）。
fn r155_declaration_line(src: &str, func: &str, id: &str) -> Option<usize> {
    let text = code_text_by_line(src);
    let (_, s, e) = r155_function_ranges(src)
        .into_iter()
        .find(|(n, _, _)| n == func)?;
    for i in s..e {
        let line = match text.get(i) {
            Some(l) => l,
            None => continue,
        };
        for kw in ["const ", "let ", "var "] {
            if let Some(pos) = line.find(kw) {
                let after = line[pos + kw.len()..].trim_start();
                let name: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
                    .collect();
                if name == id {
                    let rest = after[name.len()..].trim_start();
                    if rest.starts_with('=') && !rest.starts_with("==") {
                        return Some(i + 1);
                    }
                }
            }
        }
    }
    None
}

/// 函数体内**第一条含 `await` 的行**（1 基行号）。
fn r155_first_await_line(src: &str, func: &str) -> Option<usize> {
    let text = code_text_by_line(src);
    let (_, s, e) = r155_function_ranges(src)
        .into_iter()
        .find(|(n, _, _)| n == func)?;
    (s..e)
        .find(|i| text.get(*i).map(|l| l.contains("await")).unwrap_or(false))
        .map(|i| i + 1)
}

/// 某个标识符的初始化右值（`const <id> = <RHS>`）。
fn r155_initializer_rhs(src: &str, func: &str, id: &str) -> Option<String> {
    let text = code_text_by_line(src);
    let (_, s, e) = r155_function_ranges(src)
        .into_iter()
        .find(|(n, _, _)| n == func)?;
    for i in s..e {
        let line = match text.get(i) {
            Some(l) => l,
            None => continue,
        };
        for kw in ["const ", "let ", "var "] {
            if let Some(pos) = line.find(kw) {
                let after = line[pos + kw.len()..].trim_start();
                let name: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
                    .collect();
                if name == id {
                    let rest = after[name.len()..].trim_start();
                    if let Some(eq) = rest.strip_prefix('=') {
                        if !eq.starts_with('=') {
                            return Some(eq.trim().to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

// --------------------------------------------------------------------------------- reading ---

/// 四条规则的判词（clippy::type_complexity 的别名）。
type R155Verdicts = (bool, bool, bool, bool);

#[derive(Default)]
struct R155Reading {
    roster: std::collections::BTreeSet<String>,
    writes: Vec<(usize, String, String, String)>,
    sig_field: Option<String>,
    sig_fn: Option<String>,
    /// R1：全部收据右值都不是调用。
    r1: bool,
    /// R2：裸标识符右值都在同函数、第一个 `await` 之前绑定。
    r2: bool,
    /// R3：签名收据捕获的标识符，其初始化式调用守卫比较的那个函数。
    r3: bool,
    /// R4：守卫仍在、名册/写入非空（非空集）。
    r4: bool,
    /// 触犯者的可读清单（判词用）。
    r1_bad: Vec<String>,
    r2_bad: Vec<String>,
    r3_bad: Vec<String>,
    r4_bad: Vec<String>,
}

/// 触犯清单的**内容**（抹掉行号）：跨树比较必须只看「触犯了什么」，不能看「在第几行」——
/// 一个只多插了两行的变体就会让行号整体位移，那份比较会变成假红（#635）。
fn r155_offenses(list: &[String]) -> Vec<String> {
    list.iter()
        .map(|s| match s.split_once(": ") {
            Some((_, rest)) => rest.to_string(),
            None => s.clone(),
        })
        .collect()
}

impl R155Reading {
    fn verdicts(&self) -> R155Verdicts {
        (self.r1, self.r2, self.r3, self.r4)
    }
    fn report(&self) -> String {
        format!(
            "roster={:?} sig=({:?},{:?}) writes={:?} r1bad={:?} r2bad={:?} r3bad={:?} r4bad={:?}",
            self.roster.iter().collect::<Vec<_>>(),
            self.sig_field,
            self.sig_fn,
            self.writes
                .iter()
                .map(|(l, f, r, o)| format!("{l}:{f}={r}@{o}"))
                .collect::<Vec<_>>(),
            self.r1_bad,
            self.r2_bad,
            self.r3_bad,
            self.r4_bad,
        )
    }
}

fn r155_read(src: &str) -> R155Reading {
    let mut rd = R155Reading {
        roster: r155_receipt_roster(src),
        writes: r155_receipt_writes(src),
        ..Default::default()
    };
    let (sig_field, sig_fn) = r155_guard_sig(src)
        .map(|(f, n)| (Some(f), Some(n)))
        .unwrap_or((None, None));
    rd.sig_field = sig_field;
    rd.sig_fn = sig_fn;

    // R1：右值不得是一次调用。
    for (l, f, rhs, _) in &rd.writes {
        if rhs.contains('(') {
            rd.r1_bad.push(format!("line {l}: {f} = {rhs}"));
        }
    }
    rd.r1 = rd.r1_bad.is_empty();

    // R2：裸标识符右值必须在同函数、第一个 await 之前绑定。
    for (l, f, rhs, owner) in &rd.writes {
        if r155_is_literal_rhs(rhs) || !r155_is_bare_identifier(rhs) {
            continue;
        }
        let decl = r155_declaration_line(src, owner, rhs.trim());
        let aw = r155_first_await_line(src, owner);
        match (decl, aw) {
            (Some(d), Some(a)) if d < a && d < *l => {}
            (Some(d), Some(a)) => rd.r2_bad.push(format!(
                "line {l}: {f} = {rhs} -> declared line {d} but first await line {a}"
            )),
            (Some(_), None) => {}
            (None, _) => rd.r2_bad.push(format!(
                "line {l}: {f} = {rhs} -> no `const|let|var {rhs}` in `{owner}`"
            )),
        }
    }
    rd.r2 = rd.r2_bad.is_empty();

    // R3：签名收据捕获的标识符，其初始化式必须调用守卫比较的那个函数。
    match (&rd.sig_field, &rd.sig_fn) {
        (Some(field), Some(fname)) => {
            for (l, f, rhs, owner) in &rd.writes {
                if f != field || r155_is_literal_rhs(rhs) || !r155_is_bare_identifier(rhs) {
                    continue;
                }
                match r155_initializer_rhs(src, owner, rhs.trim()) {
                    Some(init) if init.contains(&format!("{fname}(")) => {}
                    other => rd.r3_bad.push(format!(
                        "line {l}: {f} = {rhs} -> initializer {other:?} does not call {fname}()"
                    )),
                }
            }
        }
        _ => rd
            .r3_bad
            .push("no signature receipt derived from a comparison with a call".into()),
    }
    rd.r3 = rd.r3_bad.is_empty();

    // R4：非空集 + 守卫仍在。
    if rd.sig_fn.is_none() {
        rd.r4_bad
            .push("the guard no longer compares the signature against a call".into());
    }
    if rd.roster.is_empty() {
        rd.r4_bad.push("no receipt field left in the file".into());
    }
    if rd.writes.len() < 2 {
        rd.r4_bad
            .push(format!("only {} receipt write(s)", rd.writes.len()));
    }
    rd.r4 = rd.r4_bad.is_empty();
    rd
}

// --------------------------------------------------------------------- variant constructors ---

/// 未修形状（幂等）：把两半修复体换回原形。
fn r155_variant_unfixed(src: &str) -> String {
    let mut s = src.to_string();
    if s.contains(R155_CAPTURE) {
        s = s.replace(R155_CAPTURE, "");
    }
    if s.contains(R155_STAMP) {
        s = s.replace(R155_STAMP, R155_STAMP_BASE);
    }
    s
}

/// 修复形状（幂等）：在 `await` 之前捕获签名，并把它原样盖章。
fn r155_variant_fix(src: &str) -> String {
    let mut s = r155_variant_unfixed(src);
    if !s.contains(R155_CAPTURE) {
        s = s.replace(
            R155_ANCHOR_AFTER_COLS,
            &format!("{R155_ANCHOR_AFTER_COLS}{R155_CAPTURE}"),
        );
    }
    if !s.contains(R155_STAMP) {
        s = s.replace(R155_STAMP_BASE, R155_STAMP);
    }
    s
}

/// 假修①：捕获仍在，但被搬到 `await` **之后**（同形，问的仍是「现在的房间」）。
fn r155_variant_capture_after_await(fixed: &str) -> String {
    let moved = format!("{R155_AWAIT_LINE}\n{R155_CAPTURE}");
    let s = fixed.replace(R155_CAPTURE, "");
    s.replace(R155_AWAIT_LINE, &moved)
}

/// 假修②：捕获一个常量（守卫恒不相等 ⇒ 请求风暴）。
fn r155_variant_const_capture(fixed: &str) -> String {
    fixed.replace(
        R155_CAPTURE,
        "    const reqSig = 42; // R155 mutation: capture a constant\n",
    )
}

/// 竞争修法（探针侧 `fix_generation`）：只在落地时丢弃被取代的响应，盖章那行照旧读活状态。
fn r155_variant_generation_only(fixed: &str) -> String {
    let mut s = r155_variant_unfixed(fixed);
    let head = "  async function loadTransactions() {\n    if (!loggedIn()) return;\n";
    s = s.replace(head, "  let txReqSeq = 0;\n  async function loadTransactions() {\n    if (!loggedIn()) return;\n    const mySeq = ++txReqSeq;\n");
    s.replace(
        "        liveLoad(\"transactions\", q).catch(() => { Live.transactions = null; return null; }),\n",
        "        api.get(q).then((d) => { if (mySeq !== txReqSeq) return null; Live.transactions = d; return d; }).catch(() => { Live.transactions = null; return null; }),\n",
    )
}

/// 把盖章那行删掉（收据名册仍在、写入少一条 ⇒ R4）。
fn r155_variant_drop_guard(fixed: &str) -> String {
    fixed.replace(
        "txTable.loadedQuerySig !== txQuerySig()",
        "txTable.loadedQuerySig !== txTable.loadedPage",
    )
}

// ------------------------------------------------------------------------------------ tests ---

/// 提取器落在真函数体上（自证：范围非空、恰好覆盖到 `loadTransactions`/`renderTransactions`）。
#[test]
fn the_r155_extractor_lands_on_real_function_bodies() {
    let ranges = r155_function_ranges(APP_JS);
    let names: Vec<&str> = ranges.iter().map(|(n, _, _)| n.as_str()).collect();
    for want in ["loadTransactions", "renderTransactions", "resetTxView"] {
        assert!(
            names.contains(&want),
            "函数范围提取器没找到 `{want}`（找到了 {} 个函数）",
            ranges.len()
        );
    }
    let lines: Vec<&str> = APP_JS.lines().collect();
    for (n, s, e) in &ranges {
        assert!(e > s, "函数 `{n}` 的范围为空：{s}..{e}");
        // 收尾行必须**恰是** `  }`（提取器的约定，不是「看起来像」）。
        assert_eq!(
            lines[*e], "  }",
            "函数 `{n}` 的收尾行不是 `  }}`：{:?}",
            lines[*e]
        );
    }
    // 轴上的两个函数（+ 清空收据的那个）都必须是**多行**函数，且体量正常。
    for want in ["loadTransactions", "renderTransactions", "resetTxView"] {
        let (n, s, e) = ranges.iter().find(|(n, _, _)| n == want).unwrap();
        assert!(
            e - s < 200,
            "函数 `{n}` 的范围跨了 {} 行 —— 提取器停错了地方",
            e - s
        );
        assert!(*s > 0);
    }
    // 名册是派生的（不是手抄）：三枚收据字段都在。
    let roster = r155_receipt_roster(APP_JS);
    assert_eq!(roster.len(), 3, "收据名册不是 3 个字段：{roster:?}");
    assert!(
        roster.iter().any(|f| f.ends_with("loadedQuerySig")),
        "名册里没有签名收据：{roster:?}"
    );
    // 写入点：两处函数各三条。
    let writes = r155_receipt_writes(APP_JS);
    assert!(writes.len() >= 2, "收据写入点少于两条：{writes:?}");
}

/// 轴：收据由发起它的那次请求写（本测试在**未修树**上红，#314 的「轴腿」）。
#[test]
fn the_receipt_is_written_by_the_request_that_issued_it() {
    let rd = r155_read(APP_JS);
    assert!(
        rd.verdicts() == (true, true, true, true),
        "R155 未修：收据在响应落地后读活状态 —— 守卫会与自己的投影比较、恒成立。\n  {}",
        rd.report()
    );
}

/// 四条规则**各有独立的牙**：合成变异体各只打翻一条（基线＝已知为绿的修复树，#458）。
#[test]
fn the_r155_rules_have_teeth() {
    let fixed = r155_variant_fix(APP_JS);
    let base = r155_read(&fixed);
    assert_eq!(
        base.verdicts(),
        (true, true, true, true),
        "自证基线不绿，牙齿测试没有意义：{}",
        base.report()
    );

    let unfixed = r155_variant_unfixed(&fixed);
    let after_await = r155_variant_capture_after_await(&fixed);
    let const_capture = r155_variant_const_capture(&fixed);
    let no_guard = r155_variant_drop_guard(&fixed);

    let mutants: [(&str, &str, R155Verdicts); 4] = [
        (
            "未修形状（盖章读活状态）",
            unfixed.as_str(),
            (false, true, true, true),
        ),
        (
            "捕获搬到 await 之后",
            after_await.as_str(),
            (true, false, true, true),
        ),
        (
            "捕获一个常量",
            const_capture.as_str(),
            (true, true, false, true),
        ),
        (
            "守卫不再与调用比较",
            no_guard.as_str(),
            (true, true, false, false),
        ),
    ];
    for (label, tree, expected) in mutants {
        let rd = r155_read(tree);
        assert_eq!(
            rd.verdicts(),
            expected,
            "规则 `{label}` 的牙不成立（期望 {expected:?}）：{}",
            rd.report()
        );
    }

    // 每条变异体都必须**真的改了树**（否则「判词相同」可能只是同一棵树的两张脸）。
    for (label, tree) in [
        ("unfixed", &unfixed),
        ("after_await", &after_await),
        ("const_capture", &const_capture),
        ("no_guard", &no_guard),
    ] {
        assert_ne!(
            tree, &fixed,
            "变异体 `{label}` 与修复树逐字相同 —— 变异没落地"
        );
    }
    // 修复构造器幂等（两腿拿到同一棵树，#612）。
    assert_eq!(fixed, r155_variant_fix(&fixed), "修复构造器不幂等");
    assert_eq!(
        fixed.matches(R155_CAPTURE).count(),
        1,
        "修复树里捕获行不是恰好一条"
    );
    assert_eq!(
        fixed.matches(R155_STAMP).count(),
        1,
        "修复树里盖章行不是恰好一条"
    );
}

/// 规则与**竞争修法**的关系，逐腿声明（#339/#341：期望与实际各印一列）。
#[test]
fn the_r155_rules_separate_the_variants() {
    // 判词只打在**自己拼出来的树**上（#314），每棵树都从同一个起点拼（#612）。
    let all_green = (true, true, true, true);
    let tree_fix = r155_variant_fix(APP_JS);
    let tree_unfixed = r155_variant_unfixed(&tree_fix);
    let tree_generation = r155_variant_generation_only(&tree_fix);

    assert_ne!(tree_fix, tree_unfixed, "未修形状与修复树逐字相同");
    assert_ne!(tree_fix, tree_generation, "竞争修法与修复树逐字相同");

    let declared = [
        (
            "fix (capture before the await)",
            tree_fix.as_str(),
            all_green,
        ),
        (
            "unfixed (the defect)",
            tree_unfixed.as_str(),
            (false, true, true, true),
        ),
        (
            // ⚠️ 故意比探针严一档：探针接受它（终态正确、少发一次请求），本门禁拒它 —— 收据的作者
            // 是同一个主张，一份收据要两个机制分别保证才算真修（不对称可测量，见测试名）。
            "fix_generation only (probe accepts, gate rejects)",
            tree_generation.as_str(),
            (false, true, true, true),
        ),
    ];
    for (name, app, expected) in declared {
        let rd = r155_read(app);
        assert_eq!(
            rd.verdicts(),
            expected,
            "变体 `{name}` 的判词与声明不符（声明 {expected:?}）：{}",
            rd.report()
        );
    }

    // 竞争修法那条边界的「因」要可测量：它**不是**修复体，却与未修形状在 R1 上同样红。
    assert_eq!(
        r155_offenses(&r155_read(&tree_generation).r1_bad),
        r155_offenses(&r155_read(&tree_unfixed).r1_bad),
        "`fix_generation` 与未修形状的 R1 触犯**内容**不同 —— 那条判据变了"
    );
    assert_ne!(
        tree_generation, tree_unfixed,
        "`fix_generation` 与未修形状逐字相同 —— 竞争修法没落地"
    );
}

// ═══════════════════════ R92：toast 的分级词表只有一个（PART A：helpers）═══════════════════════
//
// 轴：**调用点写下的那个分级词，必须是样式表为 `.toast` 声明过的词；而样式表声明的词又必须
// 正是 `ui/README.md` 把它当契约写出来的那一组；反过来，样式表里也不许有待用的分级。**
//
// 一件事、三处载体：
//   ① 机制（`ui/js/app.js` 的 `function toast(msg, type, opts)`）把 `type` **无校验**地拼进
//      `className`（`el.className = "toast" + (type ? " " + type : "")`）⇒ 拼错的词**不会报错**，
//      只是永远拿不到分级样式。词表因此只能靠「调用点」与「样式表」两处对齐来保证。
//   ② 样式表（`ui/css/style.css`）声明 `.toast.success` / `.toast.error` / `.toast.info` 三条
//      **分级**规则；`.toast.out` 是**状态类**（由 `toast()` 函数体自己 `classList.add("out")`），
//      不是分级 ⇒ 分级集合**推导**为「规则集合 − 函数体自己加的状态类」，零手写豁免清单
//      （#357 家族：豁免要能从制品里推出来）。
//   ③ 文档（`ui/README.md` §交互约定）把 `toast(msg, "success" | "error" | "info")` 写成契约。
//
// 缺陷（R92）：`ui/js/app.js` 的忘记密码成功分支
// `if (r && r.status === "ok") { toast(T("forgot.done"), "ok"); … }` —— 第二个实参**不是选词**，
// 而是**同一个布尔分支的判据字面量**被复用了（`"ok"` 是 API 响应状态、也是 pill 颜色词表的词，
// 不是 toast 分级）⇒ 全应用唯一一条拿不到成功色的成功消息，且整会话不自愈。
//
// 规则：
//   R1  每个 `toast()` **调用点**写下的**字面量**分级，都必须 ∈ 由样式表推导出的分级集合；
//       调用点的第二实参**不得**是非字面量（变量 / 拼接 / 模板串）—— 那会让静态对齐失效，
//       要么写进词表、要么走派生映射（#322：先扫兄弟行 —— 27 条 success 本来就写对了）。
//       非空前置（坑 68：空集上的集合断言会假绿）：调用点 ≥ 2、样式表分级集合非空。
//   R2  推导出的分级集合必须**恰好等于** `ui/README.md` 契约行里**解析**出来的那组词
//       —— 样式表与文档必须说同一件事（#469：期望值从被测量的制品里推导，不写死字面量）。
//       这条同时挡住竞争修法「不动调用点、给 `.toast.ok` 补一条与 `.toast.success` 同体的规则」
//       —— 那样样式表的词表就多了一个文档没写的同义词（探针把这种叫 `m_css` / `m_css2`）。
//   R3  反向：样式表里每条**分级**规则都要被至少一个调用点用到（无死分级）。只加规则会触犯 R2、
//       只改文档也会触犯 R2 ⇒ R3 单独负责「两边都改了、但那条规则没人用」这种死分级。
//
// 射程（如实，并已写进 `ui/README.md`）：本门禁是**词法**的 —— 它证「词表的三个载体说的是同一
// 组词」，**不**证屏幕上那一刻这条消息真的染上了成功色（那一半归 jsdom 探针 `r92_probe.js`
// 的 B1/B3/C3 腿）。扫描器跳过字符串／模板串／注释；**正则字面量不被识别**（当时的 75 个调用点
// 里没有正则；自证测试把这一点钉在「每个调用点的括号都能配对、实参数都是 1..3」上）。

/// `ui/README.md`：契约的第三个载体（把分级词表写成文档）。
const UI_README: &str = include_str!("../ui/README.md");

/// 修复前 / 修复后那一行的**唯一区别**（两个片段都逐字来自 `ui/js/app.js`）。
/// 变异体靠这两个常量在**同一棵树**上来回走（#612：每个变异体都从同一个起点拼）。
const R92_CALL_UNFIXED: &str = "toast(T(\"forgot.done\"), \"ok\")";
const R92_CALL_FIXED: &str = "toast(T(\"forgot.done\"), \"success\")";

// ------------------------------------------------------------------ 词法扫描（括号/串/注释感知）---

/// 标识符字符（JS 的 `$` 与 `_` 也算）。
fn r92_ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$'
}

/// `i` 指向引号；返回该字符串字面量之后的偏移。
fn r92_skip_string(b: &[u8], mut i: usize) -> usize {
    let q = b[i];
    i += 1;
    while i < b.len() {
        if b[i] == b'\\' {
            i += 2;
            continue;
        }
        if b[i] == q {
            return i + 1;
        }
        i += 1;
    }
    i
}

/// `i` 指向 `//`；返回到行尾之后（含换行）。
fn r92_skip_line_comment(b: &[u8], i: usize) -> usize {
    match b[i..].iter().position(|&c| c == b'\n') {
        Some(p) => i + p + 1,
        None => b.len(),
    }
}

/// `i` 指向 `/*`；返回注释之后。
fn r92_skip_block_comment(b: &[u8], i: usize) -> usize {
    let mut j = i + 2;
    while j + 1 < b.len() {
        if b[j] == b'*' && b[j + 1] == b'/' {
            return j + 2;
        }
        j += 1;
    }
    b.len()
}

/// 这个 `/` 是否**开始一个正则字面量**（而不是除号）。
///
/// 判据＝前一个有意义字符是不是运算符/开括号一类：`( , = : [ ! & | ? { } ; + - * % ~ ^ < >`。
/// 除号前面的字符总是标识符字符 / `)` / `]` / 数字 / 引号，落不在这个集合里。
/// 必须做这一步：`/[&<>"']/g`（`esc()`）与 `/[",\r\n]/`（CSV 单元格转义）里都带着**引号**，
/// 把它们当字符串起始会吞掉大段代码（本轮就是这么被咬的 —— 76 个位点只扫到 16 个）。
fn r92_regex_starts(b: &[u8], i: usize) -> bool {
    let mut k = i;
    while k > 0 {
        let c = b[k - 1];
        if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
            k -= 1;
            continue;
        }
        return matches!(
            c,
            b'(' | b','
                | b'='
                | b':'
                | b'['
                | b'!'
                | b'&'
                | b'|'
                | b'?'
                | b'{'
                | b'}'
                | b';'
                | b'+'
                | b'-'
                | b'*'
                | b'%'
                | b'~'
                | b'^'
                | b'<'
                | b'>'
        );
    }
    true
}

/// `i` 指向正则字面量的起始 `/`；返回到它之后（含 flags）。字符类 `[...]` 里的 `/` 不算收尾；
/// 换行即放弃（正则不能跨行 ⇒ 那说明这里其实是除号，别吞代码）。
fn r92_skip_regex(b: &[u8], i: usize) -> usize {
    let mut j = i + 1;
    let mut in_class = false;
    while j < b.len() {
        let c = b[j];
        if c == b'\\' {
            j += 2;
            continue;
        }
        if c == b'\n' {
            return j;
        }
        if c == b'[' {
            in_class = true;
        } else if c == b']' {
            in_class = false;
        } else if c == b'/' && !in_class {
            return j + 1;
        }
        j += 1;
    }
    j
}

/// `i` 指向 `(`；返回与之配对的 `)` 的偏移。字符串／模板串／注释里的括号不算结构。
fn r92_matching_paren(b: &[u8], i: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut j = i;
    while j < b.len() {
        let c = b[j];
        if c == b'"' || c == b'\'' || c == b'`' {
            j = r92_skip_string(b, j);
            continue;
        }
        if c == b'/' && j + 1 < b.len() && b[j + 1] == b'/' {
            j = r92_skip_line_comment(b, j);
            continue;
        }
        if c == b'/' && j + 1 < b.len() && b[j + 1] == b'*' {
            j = r92_skip_block_comment(b, j);
            continue;
        }
        if c == b'/' && r92_regex_starts(b, j) {
            j = r92_skip_regex(b, j);
            continue;
        }
        if c == b'(' {
            depth += 1;
        } else if c == b')' {
            depth -= 1;
            if depth == 0 {
                return Some(j);
            }
        }
        j += 1;
    }
    None
}

/// 括号／串／注释感知地把 `[open, close)` 之间的实参切成顶层若干段（不含定界逗号），去首尾空白。
/// ⛔ 不用「按行切」：一行可以有好几处调用，实参里也常有 `T(...)` 这类嵌套调用。
fn r92_split_args(b: &[u8], open: usize, close: usize) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut depth = 0i32;
    let mut j = open + 1;
    while j < close {
        let c = b[j];
        if c == b'"' || c == b'\'' || c == b'`' {
            let e = r92_skip_string(b, j).min(close);
            cur.extend_from_slice(&b[j..e]);
            j = e;
            continue;
        }
        if c == b'/' && j + 1 < close && b[j + 1] == b'/' {
            j = r92_skip_line_comment(b, j);
            cur.push(b' ');
            continue;
        }
        if c == b'/' && j + 1 < close && b[j + 1] == b'*' {
            j = r92_skip_block_comment(b, j);
            cur.push(b' ');
            continue;
        }
        if c == b'/' && r92_regex_starts(b, j) {
            let e = r92_skip_regex(b, j).min(close);
            cur.extend_from_slice(&b[j..e]);
            j = e;
            continue;
        }
        if c == b'(' || c == b'[' || c == b'{' {
            depth += 1;
        } else if c == b')' || c == b']' || c == b'}' {
            depth -= 1;
        } else if c == b',' && depth == 0 {
            args.push(String::from_utf8_lossy(&cur).trim().to_string());
            cur.clear();
            j += 1;
            continue;
        }
        cur.push(c);
        j += 1;
    }
    let tail = String::from_utf8_lossy(&cur).trim().to_string();
    if !tail.is_empty() || !args.is_empty() {
        args.push(tail);
    }
    args
}

/// `i` 之前（跳过空白）是否恰好是 `function` 这个词 —— 用来把函数**定义**处从调用点名册里剔除。
/// 按**字节**取，不切 `&str`（游标可能落在多字节字符中间）。
fn r92_preceded_by_function(b: &[u8], i: usize) -> bool {
    let mut k = i;
    while k > 0 && (b[k - 1] == b' ' || b[k - 1] == b'\t') {
        k -= 1;
    }
    let word = b"function";
    k >= word.len() && &b[k - word.len()..k] == word
}

/// 整个字面量（首尾同一种引号、且长度 ≥ 2）。
fn r92_is_string_literal(a: &str) -> bool {
    let bs = a.as_bytes();
    bs.len() >= 2 && (bs[0] == b'"' || bs[0] == b'\'') && bs[bs.len() - 1] == bs[0]
}

/// 一个 `toast(` 位点。
#[derive(Debug, Clone)]
struct R92Site {
    /// 行号（1 起）。
    line: usize,
    /// 顶层实参个数。
    args: usize,
    /// 第二实参是**纯字面量**时的值（去掉引号）。
    level: Option<String>,
    /// 第二实参**不是**字面量时的原文（诊断用；`None` 表示缺参）。
    dynamic: Option<String>,
    /// 是否是 `function toast(...)` 的定义处（定义处不进调用点名册）。
    definition: bool,
}

/// 扫出文件里每个 `toast(` 位点（字符串／注释里的 `toast(` 不算 —— 它们不是代码）。
fn r92_sites(src: &str) -> Vec<R92Site> {
    let b = src.as_bytes();
    let mut out: Vec<R92Site> = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        if c == b'"' || c == b'\'' || c == b'`' {
            i = r92_skip_string(b, i);
            continue;
        }
        if c == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            i = r92_skip_line_comment(b, i);
            continue;
        }
        if c == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            i = r92_skip_block_comment(b, i);
            continue;
        }
        if c == b'/' && r92_regex_starts(b, i) {
            i = r92_skip_regex(b, i);
            continue;
        }
        if !b[i..].starts_with(b"toast") {
            i += 1;
            continue;
        }
        let after = i + "toast".len();
        let prev_ok = i == 0 || !r92_ident_byte(b[i - 1]);
        // 后面必须是可选空白 + `(`（`okToast(` / `toastAll(` 之类的名字不匹配）。
        let tail_ok = b.get(after) == Some(&b'(')
            || (matches!(b.get(after), Some(&x) if x == b' ' || x == b'\t')
                && b.get(after + 1) == Some(&b'('));
        if !prev_ok || !tail_ok {
            i = after;
            continue;
        }
        let mut open = after;
        while open < b.len() && b[open] != b'(' {
            open += 1;
        }
        let Some(close) = r92_matching_paren(b, open) else {
            i = after;
            continue;
        };
        let args = r92_split_args(b, open, close);
        let line = b[..i].iter().filter(|&&x| x == b'\n').count() + 1;
        let (level, dynamic) = match args.get(1) {
            None => (None, None),
            Some(a) if r92_is_string_literal(a) => (Some(a[1..a.len() - 1].to_string()), None),
            Some(a) => (None, Some(a.replace('\n', " "))),
        };
        out.push(R92Site {
            line,
            args: args.len(),
            level,
            dynamic,
            definition: r92_preceded_by_function(b, i),
        });
        i = close + 1;
    }
    out
}

/// 样式表里行首 `.toast.<level> {` 规则的分级名（去重排序）。
/// ⚠️ `.toast-action {`（连字符）与 `.toast {`（基类）都不匹配。
fn r92_rule_levels(css: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in css.lines() {
        let Some(rest) = line.trim_end().strip_prefix(".toast.") else {
            continue;
        };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        if !name.is_empty() && rest[name.len()..].trim_start().starts_with('{') {
            out.push(name);
        }
    }
    out.sort();
    out.dedup();
    out
}

/// `toast()` 函数体自己 `classList.add` 的字面量 —— 那些是**状态类**，不是分级。
/// 从函数体**推导** ⇒ 将来它再加或换状态类，分级集合自动跟着走（零手写豁免清单）。
fn r92_state_classes(src: &str) -> Vec<String> {
    let Some(body) = js_function_body(src, "toast") else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for (idx, _) in body.match_indices("classList.add(") {
        let rest = body[idx + "classList.add(".len()..].trim_start();
        let Some(q) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            continue;
        };
        if let Some(end) = rest[1..].find(q) {
            let name = &rest[1..1 + end];
            if !name.is_empty() {
                out.push(name.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// 从 `ui/README.md` 的契约行**解析**分级枚举（不写死字面量，#469）。
/// 契约行形状：`` `toast(msg, "success" | "error" | "info")` ``。
fn r92_readme_levels(readme: &str) -> Option<Vec<String>> {
    for line in readme.lines() {
        let Some(at) = line.find("`toast(msg,") else {
            continue;
        };
        let rest = &line[at..];
        let Some(close) = rest.find(')') else {
            continue;
        };
        let mut out: Vec<String> = Vec::new();
        for (idx, _) in rest[..close].match_indices('"') {
            let seg = &rest[idx + 1..close];
            let Some(end) = seg.find('"') else {
                continue;
            };
            let name = &seg[..end];
            if !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                out.push(name.to_string());
            }
        }
        out.sort();
        out.dedup();
        if !out.is_empty() {
            return Some(out);
        }
    }
    None
}

// ----------------------------------------------------------------------------------- 判读 ---

/// 本轴的判读结果（三条规则各带自己的触犯清单）。
struct R92Reading {
    sites: Vec<R92Site>,
    rules: Vec<String>,
    state: Vec<String>,
    styled: Vec<String>,
    readme: Option<Vec<String>>,
    used: Vec<String>,
    r1_bad: Vec<String>,
    r2_bad: Vec<String>,
    r3_bad: Vec<String>,
}

impl R92Reading {
    fn verdicts(&self) -> (bool, bool, bool) {
        (
            self.r1_bad.is_empty(),
            self.r2_bad.is_empty(),
            self.r3_bad.is_empty(),
        )
    }

    fn report(&self) -> String {
        let calls: Vec<&R92Site> = self.sites.iter().filter(|s| !s.definition).collect();
        format!(
            "调用点 {} 条 · 用到的字面量分级 {:?} · 样式表规则 {:?} · 状态类(函数体推导) {:?} \
             · 分级集合 {:?} · 文档契约 {:?}\n  R1: {:?}\n  R2: {:?}\n  R3: {:?}",
            calls.len(),
            self.used,
            self.rules,
            self.state,
            self.styled,
            self.readme,
            self.r1_bad,
            self.r2_bad,
            self.r3_bad,
        )
    }
}

/// 三条规则的唯一读取入口。`app` 是被测的那棵树（两条腿喂不同的树）；
/// 样式表与文档永远是 live 的 `ui/css/style.css` / `ui/README.md`。
fn r92_read(app: &str, css: &str, readme: &str) -> R92Reading {
    let sites = r92_sites(app);
    let rules = r92_rule_levels(css);
    let state = r92_state_classes(app);
    let styled: Vec<String> = rules
        .iter()
        .filter(|r| !state.contains(r))
        .cloned()
        .collect();
    let readme_levels = r92_readme_levels(readme);
    let mut used: Vec<String> = sites
        .iter()
        .filter(|s| !s.definition)
        .filter_map(|s| s.level.clone())
        .collect();
    used.sort();
    used.dedup();

    // R1：调用点的分级词必须在样式表声明过；调用点不得拿非字面量当分级；两个非空前置。
    let mut r1_bad: Vec<String> = Vec::new();
    let calls: Vec<&R92Site> = sites.iter().filter(|s| !s.definition).collect();
    for s in &calls {
        match (&s.level, &s.dynamic) {
            (Some(lv), _) if !styled.contains(lv) => r1_bad.push(format!(
                "第 {} 行：toast(…, \"{}\") —— `{}` 不在样式表声明的分级里",
                s.line, lv, lv
            )),
            (None, Some(dyn_src)) => r1_bad.push(format!(
                "第 {} 行：toast(…, {}) —— 调用点的分级不是字面量，静态对齐失效",
                s.line, dyn_src
            )),
            _ => {}
        }
    }
    if calls.len() < 2 {
        r1_bad.push(format!(
            "toast() 调用点只有 {} 条 —— 扫描器或树不对",
            calls.len()
        ));
    }
    if styled.is_empty() {
        r1_bad.push("样式表里一条 `.toast.<level>` 分级规则都没有".into());
    }

    // R2：样式表的分级词表 == 文档契约里解析出来的词表（双向）。
    let mut r2_bad: Vec<String> = Vec::new();
    match &readme_levels {
        None => r2_bad.push("`ui/README.md` 里找不到 `toast(msg, …)` 契约行".into()),
        Some(rm) => {
            for lv in &styled {
                if !rm.contains(lv) {
                    r2_bad.push(format!(
                        "样式表声明了 `.toast.{}`，而文档契约里没有这个词",
                        lv
                    ));
                }
            }
            for lv in rm {
                if !styled.contains(lv) {
                    r2_bad.push(format!(
                        "文档契约写了 `{}`，而样式表没有 `.toast.{}` 规则",
                        lv, lv
                    ));
                }
            }
        }
    }

    // R3：反向 —— 每条分级规则都要有人用（无死分级）。
    let mut r3_bad: Vec<String> = Vec::new();
    for lv in &styled {
        if !used.contains(lv) {
            r3_bad.push(format!("`.toast.{}` 没有任何调用点用到 —— 死分级", lv));
        }
    }

    R92Reading {
        sites,
        rules,
        state,
        styled,
        readme: readme_levels,
        used,
        r1_bad,
        r2_bad,
        r3_bad,
    }
}

// ------------------------------------------------------------------------------ 变异体构造 ---

/// 未修形状（幂等）：把那处分级换回它出生时写下的那个词（＝判据字面量的复用）。
fn r92_variant_unfixed(app: &str) -> String {
    app.replace(R92_CALL_FIXED, R92_CALL_UNFIXED)
}

/// 修复形状（幂等）：把那一处换成词表里的词。
fn r92_variant_fix(app: &str) -> String {
    app.replace(R92_CALL_UNFIXED, R92_CALL_FIXED)
}

/// 竞争修法 `m_css`（幂等）：不动调用点，给 `"ok"` 补一条与 `.toast.success` **同体**的规则。
/// 这是本轴最像样的竞争修法 —— 屏幕上的颜色当场就对了，但词表多了一个文档没写的同义词。
fn r92_variant_css_synonym(css: &str) -> String {
    if css.contains(".toast.ok {") {
        return css.to_string();
    }
    let add = ".toast.ok { border-color: var(--ok); color: var(--ok); }\n";
    css.replacen(".toast.out {", &format!("{add}.toast.out {{"), 1)
}

/// 死分级（幂等，R3 的牙）：样式表里加一条 `.toast.warn` **并**把 `warn` 写进文档契约
/// —— 于是 R2 两边一致，只剩 R3 能开口（那条规则没有任何调用点用到）。
/// 返回 `(新样式表, 新文档)`。
fn r92_variant_dead_level(css: &str, readme: &str) -> (String, String) {
    let css2 = if css.contains(".toast.warn {") {
        css.to_string()
    } else {
        css.replacen(
            ".toast.out {",
            ".toast.warn { border-color: var(--warn); color: var(--warn); }\n.toast.out {",
            1,
        )
    };
    let enum_src = "\"success\" | \"error\" | \"info\"";
    let readme2 = if readme.contains("\"info\" | \"warn\"") {
        readme.to_string()
    } else {
        readme.replacen(enum_src, "\"success\" | \"error\" | \"info\" | \"warn\"", 1)
    };
    (css2, readme2)
}

/// 文档漂移（幂等，R2 的牙之二）：把 `info` 从文档契约里删掉 —— 样式表没动。
fn r92_variant_readme_desync(readme: &str) -> String {
    readme.replacen(
        "\"success\" | \"error\" | \"info\"",
        "\"success\" | \"error\"",
        1,
    )
}

// ------------------------------------------------------------------------------------ tests ---

/// 一条变异腿的声明：(标签, app 树, 样式表, 文档契约, 期望判词 `(R1, R2, R3)`)。
/// 取别名是因为 CI 跑 `clippy --all-targets -- -D warnings`，裸元组会触发 `type_complexity`。
type R92Leg<'a> = (&'a str, &'a str, &'a str, &'a str, (bool, bool, bool));

/// 提取器落在真位点上（自证：定义处恰好一处、调用点 75 条、实参数都是 1..3、词表三者非空）。
#[test]
fn the_r92_scanner_lands_on_the_real_toast_sites() {
    let sites = r92_sites(APP_JS);
    let defs: Vec<&R92Site> = sites.iter().filter(|s| s.definition).collect();
    let calls: Vec<&R92Site> = sites.iter().filter(|s| !s.definition).collect();

    assert_eq!(
        defs.len(),
        1,
        "`function toast(` 定义处不是恰好一处：{defs:?}"
    );
    assert_eq!(
        defs[0].dynamic.as_deref(),
        Some("type"),
        "定义处的第二实参应当是这个形参 `type`：{defs:?}"
    );
    assert!(
        calls.len() >= 70,
        "调用点只有 {} 条 —— 扫描器停错了地方或树不对",
        calls.len()
    );
    // 括号能配对、实参数在 1..3 之间（正则字面量若骗到了扫描器，这里会先炸）。
    for s in &calls {
        assert!(
            (1..=3).contains(&s.args),
            "第 {} 行的 toast() 实参数是 {} —— 扫描器被字符串/正则骗到了",
            s.line,
            s.args
        );
    }
    // 第二实参的三种形状都必须在名册里出现过（缺参是**受支持的中性态**，见 ui/README.md）。
    assert!(
        calls.iter().any(|s| s.level.is_some()),
        "没有任何调用点带字面量分级 —— 名册是空的"
    );
    assert!(
        calls.iter().any(|s| s.args == 1),
        "没有一条「缺参」调用点 —— 与本仓的实际形状不符（#322：先看兄弟行）"
    );

    // 状态类由函数体推导，不是手写豁免清单。
    assert_eq!(
        r92_state_classes(APP_JS),
        vec!["out".to_string()],
        "`toast()` 函数体里 `classList.add` 的字面量集合变了"
    );
    // 样式表：三条分级 + 一个状态类。
    assert_eq!(
        r92_rule_levels(STYLE_CSS),
        vec![
            "error".to_string(),
            "info".to_string(),
            "out".to_string(),
            "success".to_string()
        ],
        "`ui/css/style.css` 的 `.toast.<x>` 规则集合变了"
    );
    // 文档契约能解析出三个词。
    assert_eq!(
        r92_readme_levels(UI_README),
        Some(vec![
            "error".to_string(),
            "info".to_string(),
            "success".to_string()
        ]),
        "`ui/README.md` 的 toast 契约行解析不出三个分级"
    );
}

/// 轴：**调用点的分级词、样式表的规则、文档的契约，三者说同一组词**（本测试在**未修树**上红）。
#[test]
fn every_toast_level_is_a_level_the_sheet_declares() {
    let rd = r92_read(APP_JS, STYLE_CSS, UI_README);
    assert!(
        rd.verdicts() == (true, true, true),
        "R92 未修：有一个调用点写的分级词不在 `.toast` 的规则集里（或被文档漏写）。\n  {}",
        rd.report()
    );
}

/// 三条规则**各有独立的牙**：合成变异体各只打翻它针对的那一条（基线＝已知为绿的修复树，#458）。
#[test]
fn the_r92_rules_have_teeth() {
    let fixed = r92_variant_fix(APP_JS);
    let base = r92_read(&fixed, STYLE_CSS, UI_README);
    assert_eq!(
        base.verdicts(),
        (true, true, true),
        "自证基线不绿，牙齿测试没有意义：{}",
        base.report()
    );

    // R1：把那一处换回 `"ok"`（判据字面量被当成分级）。
    let unfixed = r92_variant_unfixed(&fixed);
    // R1 的另一条牙：把分级改成非字面量（静态对齐失效）。
    let dynamic = r92_variant_fix(APP_JS).replace(R92_CALL_FIXED, "toast(T(\"forgot.done\"), lv)");
    // R2：竞争修法（给 `.toast.ok` 补规则，调用点不动）。
    let css_synonym = r92_variant_css_synonym(STYLE_CSS);
    let (css_dead, readme_dead) = r92_variant_dead_level(STYLE_CSS, UI_README);
    let readme_desync = r92_variant_readme_desync(UI_README);

    let mutants: [R92Leg; 5] = [
        (
            "未修形状（分级＝判据字面量）",
            unfixed.as_str(),
            STYLE_CSS,
            UI_README,
            (false, true, true),
        ),
        (
            "调用点的分级不是字面量",
            dynamic.as_str(),
            STYLE_CSS,
            UI_README,
            (false, true, true),
        ),
        (
            "竞争修法：给 `.toast.ok` 补同体规则",
            unfixed.as_str(),
            css_synonym.as_str(),
            UI_README,
            (true, false, true),
        ),
        (
            "文档漂移：契约里删掉 `info`",
            fixed.as_str(),
            STYLE_CSS,
            readme_desync.as_str(),
            (true, false, true),
        ),
        (
            "死分级：加 `.toast.warn` 并写进文档",
            fixed.as_str(),
            css_dead.as_str(),
            readme_dead.as_str(),
            (true, true, false),
        ),
    ];
    for (label, app, css, readme, expected) in mutants {
        let rd = r92_read(app, css, readme);
        assert_eq!(
            rd.verdicts(),
            expected,
            "规则 `{label}` 的牙不成立（期望 {expected:?}）：{}",
            rd.report()
        );
    }

    // 每条变异体都必须**真的改了它那一处**（否则「判词不同」可能只是同一棵树的两张脸）。
    assert_ne!(unfixed, fixed, "变异体 `unfixed` 没落地");
    assert_ne!(dynamic, fixed, "变异体 `dynamic` 没落地");
    assert_ne!(css_synonym, STYLE_CSS, "变异体 `css_synonym` 没落地");
    assert_ne!(readme_desync, UI_README, "变异体 `readme_desync` 没落地");
    assert_ne!(css_dead, STYLE_CSS, "变异体 `css_dead` 没落地");
    assert_ne!(readme_dead, UI_README, "变异体 `readme_dead` 没落地");
    // 构造器幂等（两腿拿到同一棵树，#612）。
    assert_eq!(fixed, r92_variant_fix(&fixed), "修复构造器不幂等");
    assert_eq!(unfixed, r92_variant_unfixed(&unfixed), "未修构造器不幂等");
    assert_eq!(
        css_synonym,
        r92_variant_css_synonym(&css_synonym),
        "竞争修法构造器不幂等"
    );
    assert_eq!(
        (css_dead.clone(), readme_dead.clone()),
        r92_variant_dead_level(&css_dead, &readme_dead),
        "死分级构造器不幂等"
    );
    assert_eq!(
        readme_desync,
        r92_variant_readme_desync(&readme_desync),
        "文档漂移构造器不幂等"
    );
}

/// 规则与各棵树的**逐腿声明**（#339/#341：期望与实际各印一列；判词只打在自己拼出来的树上）。
#[test]
fn the_r92_rules_separate_the_variants() {
    let all_green = (true, true, true);
    let tree_fix = r92_variant_fix(APP_JS);
    let tree_unfixed = r92_variant_unfixed(&tree_fix);
    let css_synonym = r92_variant_css_synonym(STYLE_CSS);

    assert_ne!(tree_fix, tree_unfixed, "未修形状与修复树逐字相同");
    assert_ne!(css_synonym, STYLE_CSS, "竞争修法与样式表逐字相同");

    let declared = [
        (
            "fix (level from the vocabulary)",
            tree_fix.as_str(),
            STYLE_CSS,
            all_green,
        ),
        (
            "unfixed (the defect)",
            tree_unfixed.as_str(),
            STYLE_CSS,
            (false, true, true),
        ),
        (
            // 竞争修法（**调用点不动**、只给样式表补 `.toast.ok`）：屏幕上的颜色当场就对了，
            // 探针的 B1/C3 也会变绿，但词表多了一个文档没写的同义词 ⇒ 本门禁拒它（R2）。
            // 这条不对称是**故意**的、可测量的；⛔ 必须挂在**未修树**上（调用点仍写 `"ok"`）——
            // 挂在修复树上时那条新规则没人用，R2 之外还会多触犯 R3，就不是这个竞争修法了。
            "m_css (add a synonym rule)",
            tree_unfixed.as_str(),
            css_synonym.as_str(),
            (true, false, true),
        ),
    ];
    for (name, app, css, expected) in declared {
        let rd = r92_read(app, css, UI_README);
        assert_eq!(
            rd.verdicts(),
            expected,
            "变体 `{name}` 的判词与声明不符（声明 {expected:?}）：{}",
            rd.report()
        );
    }

    // 竞争修法在 R1 上**没有**触犯（它确实让每个调用点的词都有规则），只在 R2 上被拒 ——
    // 「为什么拒它」要可测量，否则这条边界会退化成「判词恰好不同」。
    let rd_m = r92_read(&tree_unfixed, &css_synonym, UI_README);
    assert!(
        rd_m.r1_bad.is_empty(),
        "竞争修法不该在 R1 上触犯：{}",
        rd_m.report()
    );
    assert!(
        rd_m.r2_bad.iter().any(|b| b.contains(".toast.ok")),
        "竞争修法的 R2 触犯没有点名那条新规则：{}",
        rd_m.report()
    );
    // 而它在 R3 上也**没有**触犯（那条新规则「有人用」—— 用它的正是那个写错词的调用点）。
    assert!(
        rd_m.r3_bad.is_empty(),
        "竞争修法不该在 R3 上触犯：{}",
        rd_m.report()
    );
}

// ⚠️ FRAGMENT — 追加到 `src/state_gate.rs` 的**文件末尾**（模块级，与 R92 lane 同层：
//    `fn r92_read` ≈:12836、`#[test] fn the_r92_rules_separate_the_variants` ≈:13206，
//    文件最后一行 `}` 就是那个 test fn 的收尾 ⇒ 追加物与它们是兄弟，不缩进。）
// ⚠️ `PathBuf` 在本文件里**没有** module-level import ⇒ 本 lane 一律写全路径
//    `std::path::PathBuf`（别加 `use`，那样会动到 13268 行文件的中段）。
// ⚠️ 状态：**已用等价的 Python 模拟在真语料上逐腿验证**（见 `r99-landing-kit.md` 的矩阵），
//    并已由 `r99_probe.js`（jsdom，5 变体 × 12 腿）在**真 DOM** 上坐实另一半射程；
//    另已做静态自检（本 lane 的 28 个顶层标识符在 `state_gate.rs` 里各 0 次；依赖的
//    `pack_region_strict`/`I18N_JS`/`ZH_PACK_START`/`EN_PACK_START`/`PACK_END`/`R164_WALLET`
//    全部已在；`use std::collections::{BTreeMap, BTreeSet};` 覆盖全部用法）。
//    但**仍未编译**（R99/R100/R101 三轮的沙箱都是 read-only，`cargo` 写不了 `target/`）。
//    落地时必须先跑 rustfmt / cargo test / clippy 三件套。

// ------------------------------------------------------------------ R99 ---
//
// 轴（R99）：交易类型的**枚举散文**必须具名「生产者真的会写出来的每一种类型」。
//
// `src/routes/wallet.rs::TX_FILTER_TYPES` 声明 6 个**受理**值
// （`consume earn topup gift expire withdraw`），其中 `withdraw` 至今**没有 writer**
// （该数组自己的文档注释写着「`withdraw` 受理但尚无 writer」）。UI 另有**三处**手抄的
// 类型枚举：钱包页脚注 `wallet.hint.suffix`、交易页副标题 `view.transactions.sub`、
// 仪表盘「交易笔数」副标题 `dash.trades.sub`。`expire` 在 C2050/C2051 变成**一条真账本行**
// （赠送过期真扣余额、真出现在交易表的类型列与导出 CSV）之后，三处**一处都没跟着走** ——
// 屏幕上写「涵盖消费、收益、充值、提现、赠送」，而同屏的交易表里就有一行「过期」。
// 一个事实、三处载体，抄漏的永远是同一项。
//
// 规则（期望值**全部**从制品推导，零手写类型名）：
//   R1  前置（空集上的集合断言会假绿，坑 #68）：语料 ≥ 20 个源文件、生产写入点 ≥ 2、
//       写入类型 ≥ 2、名册 ≥ 2、两个包的**键集合相等**、每个已知类型两个包都有非空标签。
//   R2  具名：值里具名**过半**写入类型的包键构成「名册」（两个包取并集）；名册里
//       **每个键 × 每个语言**都必须具名**全体**已具名的写入类型。尺子是该包**自己的**
//       `tx.type.<t>` 值（大小写不敏感的字面子串）—— `Consumption` 不算具名 `consume`，
//       `expiry` 也不该冒名顶替 `Expired`。
//   R3  形状：两个包的键集合必须相等；`tx.type.<t>` 的标签必须**两个包都非空**
//       （只有一边有标签 ⇒ 那一包的散文无从具名）。
//   R4  三明治：`写入 ⊆ 受理 ⊆ 有标签`。写入集合 ← 每个 `INSERT INTO transactions` 的
//       SQL 字面量在 **`type` 那一列**上写的字面量；受理集合 ← `wallet.rs::TX_FILTER_TYPES`
//       （该数组自称「新增类型时只改这里」的唯一真源）。这条挡住两类「新类型落地时漏一处」：
//       写了未受理的类型（线上筛不到）、受理了没标签的类型（表里印出 `tx.type.refund` 这种裸键）。
//
// 推导链（无一处手写类型名）：标签 ← 两包 `tx.type.*` 键的**后缀即类型值**、值是标签；
// 写入 ← 每条生产 `INSERT INTO transactions` 的**列名清单里 `type` 的下标**对应的那个值字面量
// （不是「字面量里任意一个 `'…'`」—— 那样会把 `'成功'`/`'m'` 也算进来）；
// 受理 ← `TX_FILTER_TYPES` 数组字面量；名册 ← 两个包里具名过半写入类型的键。
//
// 射程（如实）：本门禁是**词法**的 —— 它证「名册里每处散文都写了那 5 个类型的名字」，
// **不**证屏幕上那一刻真的显示这句话（那一半归 jsdom 探针 `r99_probe.js` 的 A/B 腿：
// 5 变体 × 12 腿，`landed` 12/12 绿；`A1_*`/`A3_*` 读**真 DOM**、`A2_*` 经 `t()` ＋
// 派生腿 `B3` 读〔该键没有 `[data-i18n]` 钩子，需活会话才渲染〕）。
// 三个**故意**的盲区（已写进 `ui/README.md`）：
//   ① 「过半」是一条**声明**：具名 ≤ 半数写入类型的键永不进名册（`tx.summary.net.sub`
//      「收益 − 消费」正是这种 —— 它说的是**差**，不是枚举）；
//   ② 名册是**推导**出来的 ⇒ 把某处文案删空、或把键连同 `index.html` 的绑定一起删掉，
//      本门禁不会变红 —— 那是「让承诺消失」而非「让承诺成真」，挡它的是探针与
//      `i18n_pack::every_pack_key_reaches_a_consumer`；
//   ③ 写入点必须是**字面量**：若将来某个 writer 把类型做成绑定参数（`?1`），本门禁**当场红**
//      （报「读不出字面量」而不是静默漏掉）—— 宁可要一次显式的施工，也不要一条静默的盲肠。

/// 生产写入点的锚（一条 SQL 字面量以它开头）。
const R99_ANCHOR: &str = "INSERT INTO transactions";

/// 类型名册的**键前缀** —— 后缀就是类型值本身，因此本轴不需要任何手写类型名。
const R99_TYPE_PREFIX: &str = "tx.type.";

/// 受理集合的锚（`wallet.rs::TX_FILTER_TYPES` 的声明处）。
const R99_ACCEPTED_ANCHOR: &str = "TX_FILTER_TYPES: [";

/// `INSERT INTO transactions` 的列名清单里，承载类型值的列名。
const R99_TYPE_COLUMN: &str = "type";

/// 「过半」：具名数 × 2 > 写入类型数。写成乘法而不是除以 2 —— 5 种与 6 种（`withdraw`
/// 有了 writer 之后）共用同一条判据，无浮点、无取整。
fn r99_over_half(named: usize, total: usize) -> bool {
    named * 2 > total
}

/// 生产区：第一个 `#[cfg(test)]` 之前。
fn r99_prod(src: &str) -> &str {
    match src.find("#[cfg(test)]") {
        Some(i) => &src[..i],
        None => src,
    }
}

/// 逐字节扫 `"key": "value"` 形态 —— 口径同 `i18n_pack::scan_object_keys`：**不能用行首锚定的
/// 正则**（`share.day.1`…`.6` 挤在同一行 ⇒ 会漏键，坑 #69）。本轴的语料里没有转义引号。
fn r99_pairs(region: &str) -> Vec<(String, String)> {
    let b = region.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        let Some(k0) = region[i..].find('"').map(|r| i + r) else {
            break;
        };
        let Some(k1) = region[k0 + 1..].find('"').map(|r| k0 + 1 + r) else {
            break;
        };
        let mut j = k1 + 1;
        while j < b.len() && matches!(b[j], b' ' | b'\t' | b'\n' | b'\r') {
            j += 1;
        }
        if !region[j..].starts_with(": \"") {
            i = k1 + 1;
            continue;
        }
        let v0 = j + 3;
        let Some(v1) = region[v0..].find('"').map(|r| v0 + r) else {
            break;
        };
        out.push((region[k0 + 1..k1].to_string(), region[v0..v1].to_string()));
        i = v1 + 1;
    }
    out
}

/// 从 `open` 处的 `(` 找到配对的 `)`；找不到 ⇒ `None`。
fn r99_match_paren(text: &str, open: usize) -> Option<usize> {
    let b = text.as_bytes();
    let mut depth = 0i32;
    let mut i = open;
    while i < b.len() {
        match b[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// 按**顶层**逗号切分（跳过括号内与引号内的逗号）。SQL 的值清单里会出现
/// `datetime('now')` 这种带括号项，所以不能裸 `split(',')`。
fn r99_split_top(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for ch in text.chars() {
        if let Some(q) = quote {
            cur.push(ch);
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => {
                quote = Some(ch);
                cur.push(ch);
            }
            '(' => {
                depth += 1;
                cur.push(ch);
            }
            ')' => {
                depth -= 1;
                cur.push(ch);
            }
            ',' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// 一条 `INSERT INTO transactions …` 在 **`type` 列**上写的字面量 ⇒ `Some("consume")`。
///
/// 读不出（没有列名清单 / 列名里没有 `type` / `type` 那一位不是字面量）⇒ `None`，
/// 由调用方**报错**（静默漏掉一个写入点比没有门禁更危险 —— 那正是「少数比没有更坏」）。
fn r99_type_of_insert(text: &str, at: usize) -> Option<String> {
    let open = at + text[at..].find('(')?;
    let close = r99_match_paren(text, open)?;
    let cols: Vec<String> = r99_split_top(&text[open + 1..close])
        .iter()
        .map(|c| {
            c.rsplit('.')
                .next()
                .unwrap_or(c)
                .trim()
                .trim_matches('"')
                .to_string()
        })
        .collect();
    let idx = cols.iter().position(|c| c == R99_TYPE_COLUMN)?;
    let vfrom = text[close..]
        .to_uppercase()
        .find("VALUES")
        .map(|r| close + r)?;
    let vopen = vfrom + text[vfrom..].find('(')?;
    let vclose = r99_match_paren(text, vopen)?;
    let vals = r99_split_top(&text[vopen + 1..vclose]);
    let tok = vals.get(idx)?.trim();
    if tok.len() > 2 && tok.starts_with('\'') && tok.ends_with('\'') {
        Some(tok[1..tok.len() - 1].to_string())
    } else {
        None
    }
}

/// 语料边界：门禁模块把别的源码当语料内嵌（`include_str!`），扫它们会把**夹具**里的写入点
/// 当成生产写入点；`i18n_pack.rs` 本身就是语言包语料。这条谓词是**语料边界**的唯一载体 ——
/// 它活在**走盘**那一层（`r99_source_files`），`r99_written` 不做排除，所以对它的断言必须
/// 打在这里，不能把「兄弟门禁文件」塞进 `r99_written` 的合成语料里（那样断言的是错的那一层，
/// 见 `the_r99_extractors_have_teeth`）。
fn r99_is_corpus_file(name: &str) -> bool {
    !(name.ends_with("_gate.rs") || name == "i18n_pack.rs")
}

/// `src/**/*.rs` 的生产区语料。运行时走盘（`CARGO_MANIFEST_DIR`）：生产写入点散在
/// `billing.rs` / `gift.rs` / `routes/*.rs`，只 `include_str!` 某几个文件会漏掉一种类型。
/// 形状与 `body_limit_gate.rs::source_files` 一致（同一套排除规则）。
fn r99_source_files() -> Vec<(String, String)> {
    fn walk(dir: &std::path::PathBuf, root: &std::path::PathBuf, out: &mut Vec<(String, String)>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, root, out);
            } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
                let name = p.file_name().unwrap().to_string_lossy().to_string();
                if !r99_is_corpus_file(&name) {
                    continue;
                }
                let rel = p
                    .strip_prefix(root)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .replace('\\', "/");
                if let Ok(text) = std::fs::read_to_string(&p) {
                    out.push((rel, text));
                }
            }
        }
    }
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    walk(&root, &root, &mut out);
    out.sort();
    out
}

/// 生产区每个 `INSERT INTO transactions` 在 `type` 列写的值 ⇒ `(类型 → 出处, 锚点数)`。
///
/// 整行注释里的锚点不算写入点（`routes/sharing.rs` 有一段讲这句 SQL 的说明）；行号取自
/// **原始**生产区文本（不是在剥过注释的副本上数），所以报出来的位置就是文件里那一行。
fn r99_written(files: &[(String, String)]) -> (BTreeMap<String, BTreeSet<String>>, usize) {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut anchors = 0usize;
    for (rel, src) in files {
        let text = r99_prod(src);
        let mut from = 0usize;
        while let Some(at) = text[from..].find(R99_ANCHOR).map(|r| from + r) {
            from = at + R99_ANCHOR.len();
            let ls = text[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
            if text[ls..at].trim_start().starts_with("//") {
                continue;
            }
            anchors += 1;
            let line = text[..at].matches('\n').count() + 1;
            let ty = r99_type_of_insert(text, at).unwrap_or_else(|| {
                panic!(
                    "{rel}:{line} 的 `{R99_ANCHOR}` 没能在 `{R99_TYPE_COLUMN}` 列上读出字面量 —— \
                     若类型改成了绑定参数，请扩展本门禁（不要静默跳过这个写入点）"
                )
            });
            out.entry(ty).or_default().insert(format!("{rel}:{line}"));
        }
    }
    (out, anchors)
}

/// `wallet.rs::TX_FILTER_TYPES` 声明的受理集合（该数组自称「新增类型时只改这里」）。
fn r99_accepted(wallet: &str) -> BTreeSet<String> {
    let at = wallet
        .find(R99_ACCEPTED_ANCHOR)
        .unwrap_or_else(|| panic!("`{R99_ACCEPTED_ANCHOR}` 未找到 —— 受理集合搬走了？"));
    // ⚠️ `R99_ACCEPTED_ANCHOR` **自己以 `[` 结尾**，而声明里紧随其后的是长度标注
    // （`[&str; 6]`）⇒ 从 `at` 直接找下一个 `[` 会命中**长度标注**那个，切出 `[&str; 6`
    // ——  零个字面量、受理集合**静默变空**，于是 R4 在**修好的**树上必红（R102 实测，见
    // `r102-landing-preflight-readonly.md` §7）。取值清单在 `=` 之后。
    let eq = at + wallet[at..].find('=').expect("常量声明缺 `=`");
    let open = eq + wallet[eq..].find('[').expect("数组字面量");
    let close = wallet[open..]
        .find(']')
        .map(|r| open + r)
        .expect("数组字面量收尾");
    r99_pairs_over_quotes(&wallet[open..close])
}

/// 只取一串 `"x"` 字面量（数组字面量里没有 `: `，所以不能直接用 `r99_pairs`）。
fn r99_pairs_over_quotes(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = text;
    while let Some(i) = rest.find('"') {
        let Some(j) = rest[i + 1..].find('"').map(|r| i + 1 + r) else {
            break;
        };
        out.insert(rest[i + 1..j].to_string());
        rest = &rest[j + 1..];
    }
    out
}

/// 某类型在某个包里的标签「在且非空」。R3 与 `labeled` 共用这一条判据（空串不算标签）。
fn r99_label(v: &[Option<String>; 2], lang: usize) -> Option<&str> {
    v[lang].as_deref().filter(|s| !s.is_empty())
}

/// 一个文案**具名**的类型（尺子是该包自己的标签值，大小写不敏感的字面子串）。
fn r99_named(
    value: &str,
    labels: &BTreeMap<String, [Option<String>; 2]>,
    lang: usize,
) -> BTreeSet<String> {
    let low = value.to_lowercase();
    labels
        .iter()
        .filter(|(_, v)| match r99_label(v, lang) {
            Some(l) => low.contains(&l.to_lowercase()),
            None => false,
        })
        .map(|(t, _)| t.clone())
        .collect()
}

/// 名册里某条键**未具名**的类型。
struct R99Miss {
    lang: &'static str,
    key: String,
    types: Vec<String>,
}

/// 本轴的读数。
struct R99Reading {
    files: usize,
    anchors: usize,
    written: BTreeMap<String, BTreeSet<String>>,
    labeled: BTreeSet<String>,
    accepted: BTreeSet<String>,
    roster: BTreeSet<String>,
    missing: Vec<R99Miss>,
    shape: Vec<String>,
    r4_bad: Vec<String>,
    keys_zh: usize,
    keys_en: usize,
}

impl R99Reading {
    /// 判红清单（结构化）：`"<lang>`<key>`"` ＋ 形状类判词 ＋ 三明治判词。
    fn red(&self) -> BTreeSet<String> {
        let mut out: BTreeSet<String> = self
            .missing
            .iter()
            .map(|m| format!("{}`{}`", m.lang, m.key))
            .collect();
        for s in self.shape.iter().chain(self.r4_bad.iter()) {
            out.insert(s.clone());
        }
        out
    }

    fn report(&self) -> String {
        let written: Vec<&String> = self.written.keys().collect();
        let mut s = format!(
            "语料 {} 文件 / {} 个 `{R99_ANCHOR}` 锚点；写入={written:?}；受理={:?}；有标签={:?}；\
             名册={:?}；包键 zh={} en={}",
            self.files,
            self.anchors,
            self.accepted,
            self.labeled,
            self.roster,
            self.keys_zh,
            self.keys_en
        );
        for x in self.shape.iter().chain(self.r4_bad.iter()) {
            s.push_str(&format!("\n  - {x}"));
        }
        for m in &self.missing {
            s.push_str(&format!("\n  - {}`{}` 未具名 {:?}", m.lang, m.key, m.types));
        }
        s
    }
}

/// 读一棵树：`files` = 生产语料，`i18n` = 语言包源码，`wallet` = `routes/wallet.rs`。
/// 两条腿（修前 / 修后）喂不同的语言包源码。
fn r99_read(files: &[(String, String)], i18n: &str, wallet: &str) -> R99Reading {
    let zh = pack_region_strict(i18n, ZH_PACK_START, PACK_END);
    let en = pack_region_strict(i18n, EN_PACK_START, PACK_END);
    let zp = r99_pairs(zh);
    let ep = r99_pairs(en);

    // 类型名册 = `tx.type.*` 键的后缀；标签 = 同名键的值（两个包各一把尺子）。
    let mut labels: BTreeMap<String, [Option<String>; 2]> = BTreeMap::new();
    for (lang, pack) in [(0usize, &zp), (1usize, &ep)] {
        for (k, v) in pack.iter() {
            if let Some(t) = k.strip_prefix(R99_TYPE_PREFIX) {
                labels.entry(t.to_string()).or_insert_with(|| [None, None])[lang] = Some(v.clone());
            }
        }
    }

    let keys_zh: BTreeSet<String> = zp.iter().map(|(k, _)| k.clone()).collect();
    let keys_en: BTreeSet<String> = ep.iter().map(|(k, _)| k.clone()).collect();
    let mut shape: Vec<String> = Vec::new();
    if keys_zh != keys_en {
        shape.push(format!(
            "两个包的键集合不等：zh 独有 {:?}，en 独有 {:?}",
            keys_zh.difference(&keys_en).collect::<Vec<_>>(),
            keys_en.difference(&keys_zh).collect::<Vec<_>>()
        ));
    }
    for (t, v) in labels.iter() {
        if r99_label(v, 0).is_none() || r99_label(v, 1).is_none() {
            shape.push(format!(
                "`{R99_TYPE_PREFIX}{t}` 的标签不是两个包都非空：zh={:?} en={:?}",
                v[0], v[1]
            ));
        }
    }

    let labeled: BTreeSet<String> = labels
        .iter()
        .filter(|(_, v)| r99_label(v, 0).is_some() && r99_label(v, 1).is_some())
        .map(|(t, _)| t.clone())
        .collect();

    let (written, anchors) = r99_written(files);
    let accepted = r99_accepted(wallet);

    // R4 三明治：写入 ⊆ 受理 ⊆ 有标签。
    let mut r4_bad: Vec<String> = Vec::new();
    let unaccepted: Vec<&String> = written.keys().filter(|t| !accepted.contains(*t)).collect();
    if !unaccepted.is_empty() {
        r4_bad.push(format!(
            "有写入点写了未受理的类型：{unaccepted:?}（受理集合来自 `{R99_ACCEPTED_ANCHOR}`）"
        ));
    }
    let unlabeled: Vec<&String> = accepted.iter().filter(|t| !labeled.contains(*t)).collect();
    if !unlabeled.is_empty() {
        r4_bad.push(format!("受理集合里有类型没有标签：{unlabeled:?}"));
    }

    // R2 的判据只落在「有名字的写入类型」上；R4 保证正常情况下两者相等
    // （写入点写了没有标签的类型时，R4 报错，而 R2 不会连带报一堆无从具名的键）。
    let named_targets: BTreeSet<String> = written
        .keys()
        .filter(|t| labeled.contains(*t))
        .cloned()
        .collect();

    // 名册：值里具名过半写入类型的包键（两个包取并集）。
    let mut roster: BTreeSet<String> = BTreeSet::new();
    for (lang, pack) in [(0usize, &zp), (1usize, &ep)] {
        for (k, v) in pack.iter() {
            if r99_over_half(r99_named(v, &labels, lang).len(), named_targets.len()) {
                roster.insert(k.clone());
            }
        }
    }

    // R2：名册里每个键 × 每个语言都必须具名**全体**写入类型。
    let mut missing: Vec<R99Miss> = Vec::new();
    for key in &roster {
        for (lang, name, pack) in [(0usize, "zh", &zp), (1usize, "en", &ep)] {
            let Some((_, value)) = pack.iter().find(|(k, _)| k == key) else {
                continue;
            };
            let named = r99_named(value, &labels, lang);
            let types: Vec<String> = named_targets
                .iter()
                .filter(|t| !named.contains(*t))
                .cloned()
                .collect();
            if !types.is_empty() {
                missing.push(R99Miss {
                    lang: name,
                    key: key.clone(),
                    types,
                });
            }
        }
    }

    R99Reading {
        files: files.len(),
        anchors,
        written,
        labeled,
        accepted,
        roster,
        missing,
        shape,
        r4_bad,
        keys_zh: keys_zh.len(),
        keys_en: keys_en.len(),
    }
}

/// 把 `key` 在**一个包**里的值换成 `new`。找不到要改的键 ⇒ panic（变体必须是真变异 ——
/// 空操作换来的绿是假的，R96 的教训）。
fn r99_set_value_in(src: &str, key: &str, zh: bool, new: &str) -> String {
    let cut = src
        .find(EN_PACK_START)
        .expect("语言包起点标记 `var EN = {` 未找到 —— 语言包结构变了？");
    let (head, tail) = src.split_at(cut);
    let region = if zh { head } else { tail };
    let needle = format!("\"{key}\": \"");
    let at = region
        .find(&needle)
        .unwrap_or_else(|| panic!("变体要改的键 `{key}` 不在这个包里 —— 变体是空操作"));
    let v0 = at + needle.len();
    let v1 = v0 + region[v0..].find('"').expect("值没有收尾引号");
    let mut replaced = String::new();
    replaced.push_str(&region[..v0]);
    replaced.push_str(new);
    replaced.push_str(&region[v1..]);
    if zh {
        format!("{replaced}{tail}")
    } else {
        format!("{head}{replaced}")
    }
}

/// 把某个键**整行**从**一个包**里删掉（键集合/标签两用）。
fn r99_drop_key_in(src: &str, key: &str, zh: bool) -> String {
    let cut = src
        .find(EN_PACK_START)
        .expect("语言包起点标记 `var EN = {` 未找到 —— 语言包结构变了？");
    let (head, tail) = src.split_at(cut);
    let region = if zh { head } else { tail };
    let needle = format!("\"{key}\": ");
    let mut out = String::new();
    let mut hits = 0usize;
    for line in region.split_inclusive('\n') {
        if line.contains(&needle) {
            hits += 1;
            continue;
        }
        out.push_str(line);
    }
    assert_eq!(
        hits, 1,
        "变体要删的键 `{key}` 在这个包里出现 {hits} 次 —— 变体不是「删掉一条」"
    );
    if zh {
        format!("{out}{tail}")
    } else {
        format!("{head}{out}")
    }
}

/// 多出的写入点（合成文件）：`withdraw` 受理但没 writer。
fn r99_probe_withdraw() -> (String, String) {
    (
        "src/withdraw_probe.rs".to_string(),
        "fn w() { tx.execute(\"INSERT INTO transactions (user_id, type) \
         VALUES (?1, 'withdraw')\", []); }\n"
            .to_string(),
    )
}

/// 多出的写入点（合成文件）：`refund` 既未受理也无标签。
fn r99_probe_refund() -> (String, String) {
    (
        "src/refund_probe.rs".to_string(),
        "fn r() { tx.execute(\"INSERT INTO transactions (user_id, type) \
         VALUES (?1, 'refund')\", []); }\n"
            .to_string(),
    )
}

/// 修复前那三句的**逐字原文**（两个包各三处）—— 用来在真语料上构造 pre-fix 变体。
const R99_PREFIX: [(&str, bool, &str); 6] = [
    (
        "view.transactions.sub",
        true,
        "涵盖消费、收益、充值、提现、赠送",
    ),
    ("dash.trades.sub", true, "含充值 / 消费 / 收益 / 赠送"),
    (
        "wallet.hint.suffix",
        true,
        "——涵盖消费、收益、充值、提现、赠送",
    ),
    (
        "view.transactions.sub",
        false,
        "Consumption, earnings, top-ups, withdrawals, gifts",
    ),
    ("dash.trades.sub", false, "top-up / consume / earn / gift"),
    (
        "wallet.hint.suffix",
        false,
        "— consumption, earnings, top-ups, withdrawals, gifts",
    ),
];

/// 轴：名册里每处类型枚举都必须具名生产者真会写出来的每一种类型。
#[test]
fn the_transaction_type_prose_names_every_type_the_writers_write() {
    let files = r99_source_files();
    assert!(
        files.len() >= 20,
        "只走到 {} 个源文件 —— 扫描器本身坏了（空集上通过是假绿）",
        files.len()
    );
    let rd = r99_read(&files, I18N_JS, R164_WALLET);
    // R1 阳性对照：空集上的集合断言会假绿（坑 #68）。
    assert!(rd.anchors >= 2, "生产写入点太少：{}", rd.report());
    assert!(rd.written.len() >= 2, "写入类型太少：{}", rd.report());
    assert!(
        rd.keys_zh >= 100 && rd.keys_zh == rd.keys_en,
        "语言包语料失真或两包键数不等：{}",
        rd.report()
    );
    assert!(rd.roster.len() >= 2, "名册派生失真：{}", rd.report());
    assert!(rd.shape.is_empty(), "形状类判词：{}", rd.report());
    assert!(rd.r4_bad.is_empty(), "三明治判词：{}", rd.report());
    assert!(rd.missing.is_empty(), "{}", rd.report());
}

/// 提取器自证（合成输入）：整行注释里的锚点、`#[cfg(test)]` 之后的锚点、**非 `type` 列**的
/// 字面量（`'成功'`/`'m'`）都不得被算成写入点；`type` 列传绑定参数时必须**报错**而不是静默跳过。
/// ⚠️ 语料边界（兄弟门禁文件 / `i18n_pack.rs`）**不在**这里验 —— 它活在走盘那一层，
/// `r99_written` 不做排除（本测试的合成语料绕过了走盘）⇒ 断言打在本测试的
/// `r99_is_corpus_file` 那几条上，而不是把夹具塞进 `r99_written`（R115 实测：那样断言的是
/// **错的那一层**，兄弟门禁文件里的写入点照样被算成生产写入点）。
#[test]
fn the_r99_extractors_have_teeth() {
    // `type` 在列名清单里的下标是第 3 个 ⇒ 值清单里第 3 个才是类型；其余字面量是干扰项。
    // ⚠️ 生产语句必须在 `#[cfg(test)]` **之前**（真实 Rust 文件的形状）—— 否则被
    // `r99_prod` 一起截掉，看起来像「提取器漏读」，其实是夹具不真实。
    let files = vec![(
        "src/probe.rs".to_string(),
        concat!(
            "// INSERT INTO transactions (user_id, type) VALUES (?1, 'earn')\n",
            "fn a() { tx.execute(\"INSERT INTO transactions (user_id, counterpart, type, status) ",
            "VALUES (?1, '2', 'consume', '成功')\", []); }\n",
            "fn c() { tx.execute(\"INSERT INTO transactions (user_id, type) ",
            "VALUES (?1, 'withdraw')\", []); }\n",
            "#[cfg(test)]\n",
            "mod tests { fn b() { tx.execute(\"INSERT INTO transactions ",
            "(user_id, type) VALUES (?1, 'earn')\", []); } }\n",
        )
        .to_string(),
    )];
    let (written, anchors) = r99_written(&files);
    let got: BTreeSet<String> = written.keys().cloned().collect();
    assert_eq!(
        got,
        BTreeSet::from(["consume".to_string(), "withdraw".to_string()]),
        "提取器失真（整行注释 / 测试区 / 非 type 列的字面量都不该被算）：{written:?}"
    );
    assert_eq!(anchors, 2, "锚点计数失真：{written:?}");

    // 语料边界（`_gate.rs` / `i18n_pack.rs`）：谓词本体 + 两个方向的牙。缺了它，门禁模块
    // 自己内嵌的夹具（`'consume'`/`'withdraw'`/`'refund'`）就会被当成生产写入点。
    assert!(r99_is_corpus_file("wallet.rs"), "真源文件被误排除");
    assert!(r99_is_corpus_file("gift.rs"), "真源文件被误排除");
    assert!(
        r99_is_corpus_file("admin_models.rs"),
        "不带 `_gate` 后缀的文件不得被误排除"
    );
    assert!(
        !r99_is_corpus_file("state_gate.rs"),
        "本门禁模块必须被排除（它的夹具里有写入点）"
    );
    assert!(
        !r99_is_corpus_file("body_limit_gate.rs"),
        "兄弟门禁模块必须被排除"
    );
    assert!(!r99_is_corpus_file("i18n_pack.rs"), "语言包语料必须被排除");

    // 受理集合的提取器同样要有牙 —— 而且夹具必须是**真实声明形状**（带长度标注 `[&str; N]`）。
    // 这条自证原先只覆盖 `r99_written`，于是不受测的 `r99_accepted` 在真形状上静默读空（R102）：
    // 锚点 `TX_FILTER_TYPES: [` **自己以 `[` 结尾** ⇒ 它找到的第一个 `[` 是长度标注那个。
    let wallet_shape = concat!(
        "/// 新增类型时只改这里。\n",
        "pub const TX_FILTER_TYPES: [&str; 3] = [\"consume\", \"earn\", \"topup\"];\n",
        "pub const TX_INCOME_TYPES: [&str; 2] = [\"topup\", \"gift\"];\n",
    );
    let want_accepted = BTreeSet::from([
        "consume".to_string(),
        "earn".to_string(),
        "topup".to_string(),
    ]);
    assert_eq!(
        r99_accepted(wallet_shape),
        want_accepted,
        "受理集合提取器失真（必须读 `=` 之后的取值清单，而不是长度标注 `[&str; N]`）"
    );
}

/// `type` 列写成**绑定参数**（`?1`）时，提取器必须**当场报错** —— 静默漏掉一个写入点
/// 比没有门禁更危险（「少数比没有更坏」）。这条腿就是那条断言的载体。
#[test]
#[should_panic(expected = "没能在 `type` 列上读出字面量")]
fn the_r99_bound_parameter_producer_is_loud() {
    let bound = vec![(
        "src/bound.rs".to_string(),
        "fn e() { tx.execute(\"INSERT INTO transactions (user_id, type) VALUES (?1, ?2)\", []); }\n"
            .to_string(),
    )];
    let _ = r99_written(&bound);
}

/// 每条规则的**牙**：变体各只翻它针对的那条判词，且都声明在**它真正描述的那棵树**上。
#[test]
fn the_r99_rules_have_teeth() {
    let files = r99_source_files();

    // 落地树必须先绿 —— 否则「变体的红」可能只是这棵树的红（R96 的教训）。
    let landed = r99_read(&files, I18N_JS, R164_WALLET);
    assert!(
        landed.missing.is_empty() && landed.shape.is_empty() && landed.r4_bad.is_empty(),
        "landed 树必须先绿：{}",
        landed.report()
    );

    // ① 修复前那三句（六个位点）：六个 (语言, 键) 全红 —— 这就是本轮实测的 pre-fix 红集。
    let mut pre = I18N_JS.to_string();
    for (key, zh, old) in R99_PREFIX {
        pre = r99_set_value_in(&pre, key, zh, old);
    }
    let rd_pre = r99_read(&files, &pre, R164_WALLET);
    let expect: BTreeSet<String> = R99_PREFIX
        .iter()
        .map(|(k, zh, _)| format!("{}`{k}`", if *zh { "zh" } else { "en" }))
        .collect();
    assert_eq!(
        rd_pre.red(),
        expect,
        "pre-fix 变体的红集与声明不符：{}",
        rd_pre.report()
    );

    // ② 只修了一半（中文那三句还是旧的）：zh 三处红、en 绿 —— 半修不能过闸。
    let mut half = I18N_JS.to_string();
    for (key, zh, old) in R99_PREFIX {
        if zh {
            half = r99_set_value_in(&half, key, zh, old);
        }
    }
    let rd_half = r99_read(&files, &half, R164_WALLET);
    let expect_half: BTreeSet<String> = R99_PREFIX
        .iter()
        .filter(|(_, zh, _)| *zh)
        .map(|(k, _, _)| format!("zh`{k}`"))
        .collect();
    assert_eq!(
        rd_half.red(),
        expect_half,
        "半修变体的红集与声明不符：{}",
        rd_half.report()
    );

    // ③ 新增一个 writer（`withdraw` 有 writer 的那一天）：散文必须跟着走 ——
    //    名册里没具名 `提现`/`withdraw` 的那条键当场变红（两个包各一处）。
    let mut with_writer = files.clone();
    with_writer.push(r99_probe_withdraw());
    let rd_new = r99_read(&with_writer, I18N_JS, R164_WALLET);
    assert!(
        rd_new.written.contains_key("withdraw"),
        "变体没把 `withdraw` 变成「生产者真会写」：{}",
        rd_new.report()
    );
    assert_eq!(
        rd_new.red(),
        BTreeSet::from([
            "zh`dash.trades.sub`".to_string(),
            "en`dash.trades.sub`".to_string()
        ]),
        "新 writer 变体的红集与声明不符：{}",
        rd_new.report()
    );

    // ④ R4 前半（写入 ⊆ 受理）：写了未受理的类型 ⇒ 只有这一条判词（R2 不连带报）。
    let mut refund = files.clone();
    refund.push(r99_probe_refund());
    let rd_refund = r99_read(&refund, I18N_JS, R164_WALLET);
    assert_eq!(
        rd_refund.r4_bad.len(),
        1,
        "未受理写入点没被点名：{}",
        rd_refund.report()
    );
    assert!(
        rd_refund.r4_bad[0].contains("refund"),
        "R4 判词没有点名那个类型：{}",
        rd_refund.report()
    );
    assert!(
        rd_refund.missing.is_empty() && rd_refund.shape.is_empty(),
        "R4 的牙不该连带翻别的规则：{}",
        rd_refund.report()
    );

    // ⑤ R4 后半（受理 ⊆ 有标签）：把 `expire` 的标签从**两个包**都删掉 ⇒ 键集合仍相等、
    //    也没有单边标签 ⇒ 只有 R4 报「受理了没标签的类型」。
    let dropped = r99_drop_key_in(
        &r99_drop_key_in(I18N_JS, "tx.type.expire", true),
        "tx.type.expire",
        false,
    );
    let rd_drop = r99_read(&files, &dropped, R164_WALLET);
    assert_eq!(
        rd_drop.r4_bad.len(),
        1,
        "两个包都缺标签时该只有 R4 一条：{}",
        rd_drop.report()
    );
    assert!(
        rd_drop.r4_bad[0].contains("expire") && rd_drop.shape.is_empty(),
        "R4 后半的牙没打中 / 连带翻了形状规则：{}",
        rd_drop.report()
    );

    // ⑥ R3 的牙：标签只在一个包里**非空**（另一边留空串）⇒ 形状判词点名该类型，
    //    并且 R4 也报「受理了没标签的类型」（空串不算标签）。
    let emptied = r99_set_value_in(I18N_JS, "tx.type.expire", false, "");
    let rd_empty = r99_read(&files, &emptied, R164_WALLET);
    assert!(
        rd_empty.shape.iter().any(|s| s.contains("expire")),
        "单边空标签没被形状规则点名：{}",
        rd_empty.report()
    );
    assert!(
        rd_empty.r4_bad.iter().any(|s| s.contains("expire")),
        "单边空标签没被 R4 点名：{}",
        rd_empty.report()
    );

    // ⑦ 盲区①（**刻意**放行，写进 `ui/README.md`）：「过半」是一条声明 —— 把某条枚举
    //    在两个包里都削弱到「具名 2 种」（< 半数）就离开了名册，本门禁不再管它。
    //    这不是漏判：它已经不再是**枚举**，而挡「让承诺消失」的是探针与 i18n_pack 的可达性门禁。
    let weakened = r99_set_value_in(
        &r99_set_value_in(I18N_JS, "dash.trades.sub", true, "含充值 / 消费"),
        "dash.trades.sub",
        false,
        "top-up / consume",
    );
    let rd_weak = r99_read(&files, &weakened, R164_WALLET);
    assert!(
        !rd_weak.roster.contains("dash.trades.sub"),
        "削弱到半数以下应离开名册：{}",
        rd_weak.report()
    );
    assert!(
        rd_weak.red().is_empty(),
        "这条盲区是**声明**：本门禁对它必须沉默（挡它的是探针）：{}",
        rd_weak.report()
    );

    // ⑧ 盲区②（同上，刻意放行）：把某条枚举的键从两个包都删掉 ⇒ 名册少一员、全绿。
    let gone = r99_drop_key_in(
        &r99_drop_key_in(I18N_JS, "dash.trades.sub", true),
        "dash.trades.sub",
        false,
    );
    let rd_gone = r99_read(&files, &gone, R164_WALLET);
    assert!(
        !rd_gone.roster.contains("dash.trades.sub") && rd_gone.red().is_empty(),
        "键被整体删掉时本门禁必须沉默（挡它的是 `every_pack_key_reaches_a_consumer`）：{}",
        rd_gone.report()
    );
}

// ══════════════ R169：带 `data-i18n` 的静态控件，标签归 markup 那个键所有 ══════════════
//
// 一个静态控件的标签只有一个来源：`ui/index.html` 的 `data-i18n`，由 `applyStatic()` 填充
// （`innerHTML = t(key)`；只在 `setLang()` 与 `DOMContentLoaded` 两处被调）。交互态可以**临时**
// 换文案（忙碌态），但**收尾必须回到该控件自己的来源**：要么先捕获它原来的值再写回（仓库成例是
// `withLoading`：`const orig = btn.innerHTML` → `btn.innerHTML = orig`，另有四处同形），要么写回
// markup 声明的那个键。
//
// 破在最显眼的地方：`#login-form` 的提交按钮出厂是 `login.enter`（zh「进入平台」/ en「Enter」），
// 而它的提交处理器在 `finally` 里**恢复**成 `T("login.submit")`（zh「登 录」/ en「Sign in」）——
// 即**别人的键**。于是输错一次密码之后按钮改口说另一句话，而登录页没有语言切换器 ⇒
// **整会话不自愈**（只有切语言或刷新才会被 `applyStatic()` 纠正）。溯源是漂移不是取舍：
// `8bd1063`（P2-A 登录对接）在同一个提交里既挂了 markup 的「进入平台」又手写
// `btn.textContent = "登 录"`，`68f9f70`（i18n 语言包）把两句各翻成一个键，从此一个控件两个键。
//
// 三条规则全部**派生**（#469：门禁不许把这次编辑的字面量写死）：
//   R1（轴）  —— 名册里**会写自己标签**的那些成员（今天恰一个：`#login-form`）：母集不为空，且每个
//                成员的处理器里**最后一次**写标签的表达式必须派生自该元素自己的来源 ——
//                ①对该元素的**捕获读**（`const x = btn.textContent;` 之后再写回 `x`），或
//                ②`T(k)` 且 `k ==` `ui/index.html` 给这个控件声明的键（键**从 markup 读**）。
//   R2（类）  —— `ui/index.html` 里**每个「提交按钮带 `data-i18n` 的表单」**都是成员（今日 5 个，
//                下界 2）：每个成员都必须能在 `ui/js/app.js` 里**解出恰好一个**提交处理器
//                （零个或两个以上＝响亮地红：解不出 ≠ 干净，#477），且不得以别人的键收尾。
//   R3（反向）—— 那些会写标签的成员，处理器里对同一按钮的标签写必须**至少两次**（先忙碌态、再
//                收尾）——「把忙碌态删掉」不能算合格。运行时证据是探针 `r169_probe.js` 的 `C4` 腿
//                （它在**第一个 `await` 之前**采样，证明忙碌态真的发生过）。
//
// 射程（如实，并写进 `ui/README.md`）：
//   · 门禁是**词法**的 —— 它证明「收尾的表达式与 markup 声明的键同源」；`ui/index.html` 与
//     `ui/js/app.js` 是它读的两个制品。它**不证**屏幕上那一刻渲染出的标签（那半归 jsdom 探针）。
//   · 句柄只认本仓的主导写法 `querySelector('button[type="submit"]')`（今天四处：login / register /
//     verify / share）。`#forgot-form` 用的是 `querySelector("button[type=submit]")`（另一种拼法），
//     不在词法射程内 —— 它因此**不计入**「会写标签的成员」，如实记录，不假装覆盖。
//   · 刻意放行：把 markup 改标成 `login.submit`（两边自洽）也能过 R1–R3 —— 那枚按钮就不再是设计
//     原型印的那句话，挡它的是探针的 `D2` 腿（设计基线），如同 R92 的「同义词」不对称。

/// 绑到「表单的提交按钮」的句柄写法（本门禁引用的唯一一个选择器字面量）。
const R169_SUBMIT_SELECTOR: &str = "querySelector('button[type=\"submit\"]')";
/// 提交处理器的绑定后缀（`$("#id")` 直绑与 `const formEl = $("#id")` 别名共用）。
const R169_BIND_SUFFIX: &str = ".addEventListener(\"submit\"";
/// 表单的射程边界（名册从这里派生）。
const R169_FORM_OPEN: &str = "<form id=\"";
const R169_FORM_CLOSE: &str = "</form>";
/// 表单里的提交按钮与它自己的键。
const R169_SUBMIT_TYPE: &str = "type=\"submit\"";
const R169_I18N_ATTR: &str = "data-i18n=\"";
/// 名册下界：**至少两个**成员（今日 5 个）—— 防止类规则在提取器坏掉时静默退化成单例。
const R169_MIN_MEMBERS: usize = 2;
/// 会写标签的成员**至少**要写两次（忙碌 + 收尾）—— R3 的下界。
const R169_MIN_LABEL_WRITES: usize = 2;

/// 字节等长的「代码化」文本：注释、字符串/模板串、正则字面量的**内容**换成空格，长度不变
/// ⇒ 偏移可以直接拿来切**原始**源码（偏移与文本必须同一把尺子，否则会静默切错地方）。
/// 正则那一步是必须的：`/[&<>"']/g`（`esc()`）与 `/[",\r\n]/`（CSV 转义）里都带**引号**，
/// 把它们当字符串起始会吞掉大段代码（R92 就是被这一条咬过的）。
fn r169_code_mask(src: &str) -> Vec<u8> {
    let b = src.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        let (end, lexeme) = if c == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            (r92_skip_line_comment(b, i), true)
        } else if c == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            (r92_skip_block_comment(b, i), true)
        } else if c == b'/' && r92_regex_starts(b, i) {
            (r92_skip_regex(b, i), true)
        } else if c == b'"' || c == b'\'' || c == b'`' {
            (r92_skip_string(b, i), true)
        } else {
            (i + 1, false)
        };
        if lexeme {
            for k in i..end.min(out.len()) {
                if out[k] != b'\n' && out[k] != b'\r' {
                    out[k] = b' ';
                }
            }
        }
        i = end;
    }
    out
}

/// `prefix="…"` 的属性值。
fn r169_attr(tag: &str, prefix: &str) -> Option<String> {
    let i = tag.find(prefix)? + prefix.len();
    let rest = &tag[i..];
    let j = rest.find('"')?;
    Some(rest[..j].to_string())
}

/// 表单名册：`ui/index.html` 里**提交按钮带 `data-i18n`** 的每个表单 ⇒ `(表单 id, 按钮自己的键)`。
/// 键**从 markup 读** —— R1/R2 的期望值全部长在这里，门禁里没有第二个键名。
fn r169_forms(html: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = html[from..].find(R169_FORM_OPEN) {
        let at = from + rel + R169_FORM_OPEN.len();
        let Some(id_end) = html[at..].find('"').map(|p| at + p) else {
            break;
        };
        let id = html[at..id_end].to_string();
        let close = html[at..]
            .find(R169_FORM_CLOSE)
            .map(|p| at + p)
            .unwrap_or(html.len());
        let body = &html[at..close];
        let mut scan = 0usize;
        while let Some(b) = body[scan..].find("<button") {
            let tag_start = scan + b;
            let tag_end = body[tag_start..]
                .find('>')
                .map(|p| tag_start + p + 1)
                .unwrap_or(body.len());
            let tag = &body[tag_start..tag_end];
            scan = tag_end.max(tag_start + 1);
            if !tag.contains(R169_SUBMIT_TYPE) {
                continue;
            }
            if let Some(k) = r169_attr(tag, R169_I18N_ATTR) {
                out.push((id.clone(), k));
                break;
            }
        }
        if close <= at {
            break;
        }
        from = close;
    }
    out
}

/// 某个表单的提交处理器在 `ui/js/app.js` 里的**候选起点**（偏移 + 命中的绑定片段）。
/// 两种绑定写法都算：`$("#id").addEventListener("submit"`，或先 `const formEl = $("#id");`
/// 再 `formEl.addEventListener("submit"`（别名从声明行读出来，不写死）。
fn r169_binding_starts(app: &str, form_id: &str) -> Vec<(usize, String)> {
    let mut found: Vec<(usize, String)> = Vec::new();
    let direct = format!("$(\"#{form_id}\"){R169_BIND_SUFFIX}");
    let mut from = 0usize;
    while let Some(rel) = app[from..].find(&direct) {
        let at = from + rel;
        found.push((at, direct.clone()));
        from = at + 1;
    }
    let decl = format!("= $(\"#{form_id}\")");
    let mut from = 0usize;
    while let Some(rel) = app[from..].find(&decl) {
        let at = from + rel;
        from = at + 1;
        let ls = app[..at].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let Some(alias) = app[ls..at].trim().strip_prefix("const ") else {
            continue;
        };
        let alias = alias.trim();
        if alias.is_empty() {
            continue;
        }
        let bind = format!("{alias}{R169_BIND_SUFFIX}");
        let mut f2 = 0usize;
        while let Some(r2) = app[f2..].find(&bind) {
            let a2 = f2 + r2;
            found.push((a2, bind.clone()));
            f2 = a2 + 1;
        }
    }
    found.sort_by_key(|(at, _)| *at);
    found.dedup_by_key(|(at, _)| *at);
    found
}

/// 处理器体：从**唯一**的绑定起点出发，在代码化文本上做花括号配对（掩码保长度 ⇒ 偏移同尺），
/// 再把该区间从**原始**源码里切出来。候选不是恰好一个、或括号不闭合 ⇒ `None`（规则把它变成
/// 响亮失败，而不是在空串上「通过」：解不出 ≠ 干净）。
fn r169_handler_body(app: &str, form_id: &str) -> Option<String> {
    let starts = r169_binding_starts(app, form_id);
    if starts.len() != 1 {
        return None;
    }
    let start = starts[0].0;
    let masked = r169_code_mask(app);
    let open = masked[start..].iter().position(|&c| c == b'{')? + start;
    let mut depth = 0i32;
    for k in open..masked.len() {
        match masked[k] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(app[start..=k].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// 处理器里**绑到该表单提交按钮**的句柄（`const x = …querySelector('button[type="submit"]')`）。
fn r169_handles(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in body.lines() {
        let Some(at) = line.find(R169_SUBMIT_SELECTOR) else {
            continue;
        };
        let Some(eq) = line[..at].rfind('=') else {
            continue;
        };
        let name = line[..eq].split_whitespace().last().unwrap_or("");
        if !name.is_empty() && name.bytes().all(r92_ident_byte) && !out.iter().any(|h| h == name) {
            out.push(name.to_string());
        }
    }
    out
}

/// 一行里对某句柄标签的**写** ⇒ 右值（`handle.textContent = <rhs>`；`==` 与捕获读都不算写）。
fn r169_write_rhs(line: &str, handle: &str) -> Option<String> {
    for prop in ["textContent", "innerHTML", "innerText"] {
        let needle = format!("{handle}.{prop}");
        let mut from = 0usize;
        while let Some(rel) = line[from..].find(&needle) {
            let at = from + rel + needle.len();
            from = at;
            let rest = line[at..].trim_start();
            let Some(after_eq) = rest.strip_prefix('=') else {
                continue;
            };
            if after_eq.starts_with('=') {
                continue;
            }
            let rhs = after_eq.trim_start();
            let end = rhs.find([';', '}']).unwrap_or(rhs.len());
            return Some(rhs[..end].trim().to_string());
        }
    }
    None
}

/// 处理器对同一按钮标签的**写**，按出现顺序 ⇒ `(句柄, 右值)`。
fn r169_label_writes(body: &str, handles: &[String]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in body.lines() {
        for h in handles {
            if let Some(rhs) = r169_write_rhs(line, h) {
                out.push((h.clone(), rhs));
            }
        }
    }
    out
}

/// 处理器里对该元素的**捕获读**得到的标识符（`const orig = btn.textContent;` —— 含守卫式
/// `const orig = btn ? btn.textContent : "";`；判据是「这一行**读**了这个元素的标签」）。
fn r169_captures(body: &str, handles: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in body.lines() {
        for h in handles {
            if r169_write_rhs(line, h).is_some() {
                continue;
            }
            if !["textContent", "innerHTML", "innerText"]
                .iter()
                .any(|p| line.contains(&format!("{h}.{p}")))
            {
                continue;
            }
            let Some(eq) = line.find('=') else {
                continue;
            };
            let lhs = line[..eq].trim();
            let name = ["const ", "let ", "var "]
                .iter()
                .find_map(|kw| lhs.strip_prefix(*kw))
                .unwrap_or(lhs)
                .trim();
            if !name.is_empty()
                && name.bytes().all(r92_ident_byte)
                && !out.iter().any(|x| x == name)
            {
                out.push(name.to_string());
            }
        }
    }
    out
}

/// 这次写是否**派生自该元素自己的来源**（R1 的两条合法形状；其余一律 `None` ＝ 别人的键）。
fn r169_derives_from_element(
    rhs: &str,
    captures: &[String],
    own_key: &str,
) -> Option<&'static str> {
    if captures.iter().any(|c| c == rhs) {
        return Some("capture (read from the element)");
    }
    if let Some(k) = rhs.strip_prefix("T(\"") {
        if let Some(k) = k.strip_suffix("\")") {
            if k == own_key {
                return Some("literal naming the markup key");
            }
        }
    }
    None
}

/// 一个名册成员的全部派生事实。
#[derive(Debug, Clone, PartialEq, Eq)]
struct R169Member {
    id: String,
    own_key: String,
    binding: Option<String>,
    resolved: bool,
    handles: Vec<String>,
    captures: Vec<String>,
    writes: Vec<(String, String)>,
    last_how: Option<String>,
    delegated: bool,
}

/// 一次读取的全部证据与三条判词。
struct R169Reading {
    members: Vec<R169Member>,
    unresolved: BTreeSet<String>,
    foreign: BTreeMap<String, String>,
    /// 会写自己标签的成员（R1/R3 的母集）。
    axis: BTreeSet<String>,
    /// 写标签但**少于**两次的成员（R3 的牙）。
    thin: BTreeSet<String>,
    r1: bool,
    r2: bool,
    r3: bool,
}

impl R169Reading {
    fn verdicts(&self) -> (bool, bool, bool) {
        (self.r1, self.r2, self.r3)
    }

    fn report(&self) -> String {
        let rows: Vec<String> = self
            .members
            .iter()
            .map(|m| {
                format!(
                    "{}[key={} resolved={} delegated={} handles={:?} captures={:?} writes={} last={:?}]",
                    m.id,
                    m.own_key,
                    m.resolved,
                    m.delegated,
                    m.handles,
                    m.captures,
                    m.writes.len(),
                    m.last_how
                )
            })
            .collect();
        format!(
            "r1={} r2={} r3={} | members={} axis={:?} thin={:?} unresolved={:?} foreign={:?}\n    {}",
            self.r1,
            self.r2,
            self.r3,
            self.members.len(),
            self.axis,
            self.thin,
            self.unresolved,
            self.foreign,
            rows.join("\n    ")
        )
    }
}

/// 读一棵树：名册与键从 `html` 派生、处理器事实从 `app` 派生（两条腿喂不同的树）。
fn r169_read(html: &str, app: &str) -> R169Reading {
    let mut members: Vec<R169Member> = Vec::new();
    for (id, own_key) in r169_forms(html) {
        let starts = r169_binding_starts(app, &id);
        let binding = if starts.len() == 1 {
            Some(starts[0].1.clone())
        } else {
            None
        };
        let resolved_body = r169_handler_body(app, &id);
        let resolved = resolved_body.is_some();
        let body = resolved_body.unwrap_or_default();
        let handles = r169_handles(&body);
        let captures = r169_captures(&body, &handles);
        let writes = r169_label_writes(&body, &handles);
        let last_how = writes.last().map(|(_, rhs)| {
            r169_derives_from_element(rhs, &captures, &own_key)
                .map(str::to_string)
                .unwrap_or_else(|| format!("FOREIGN: {rhs:?}"))
        });
        members.push(R169Member {
            id,
            own_key,
            binding,
            resolved,
            handles,
            captures,
            writes,
            last_how,
            delegated: body.contains("withLoading("),
        });
    }
    let unresolved: BTreeSet<String> = members
        .iter()
        .filter(|m| !m.resolved)
        .map(|m| m.id.clone())
        .collect();
    let foreign: BTreeMap<String, String> = members
        .iter()
        .filter(|m| m.resolved)
        .filter_map(|m| match &m.last_how {
            Some(h) if h.starts_with("FOREIGN") => Some((m.id.clone(), h.clone())),
            _ => None,
        })
        .collect();
    let axis: BTreeSet<String> = members
        .iter()
        .filter(|m| !m.writes.is_empty())
        .map(|m| m.id.clone())
        .collect();
    let thin: BTreeSet<String> = members
        .iter()
        .filter(|m| !m.writes.is_empty() && m.writes.len() < R169_MIN_LABEL_WRITES)
        .map(|m| m.id.clone())
        .collect();
    // 三条都带**空集保护**：母集为空时规则红，而不是在空集上恒真（坑 68 家族）。
    let r1 = !axis.is_empty() && axis.iter().all(|m| !foreign.contains_key(m));
    let r2 = unresolved.is_empty() && members.len() >= R169_MIN_MEMBERS && foreign.is_empty();
    let r3 = !axis.is_empty() && thin.is_empty();
    R169Reading {
        members,
        unresolved,
        foreign,
        axis,
        thin,
        r1,
        r2,
        r3,
    }
}

/// 名册里**会写自己标签**的那个成员（今日：`#login-form`）—— 变体构造的锚。
fn r169_axis_or_panic(html: &str, app: &str) -> R169Member {
    let rd = r169_read(html, app);
    assert_eq!(
        rd.axis.len(),
        1,
        "写自己标签的成员不是恰好一个 —— 变体无从构造：{}",
        rd.report()
    );
    rd.members
        .iter()
        .find(|m| !m.writes.is_empty())
        .cloned()
        .unwrap_or_else(|| panic!("名册里没有写标签的成员：{}", rd.report()))
}

/// 变异体①：轴的**收尾**改成**别人的键**（键从 `ui/index.html` 派生 —— 原缺陷的形状）。
fn r169_variant_foreign(html: &str, app: &str) -> String {
    let m = r169_axis_or_panic(html, app);
    let (h, rhs) = m.writes.last().cloned().expect("轴成员没有收尾写");
    let other = r169_forms(html)
        .into_iter()
        .map(|(_, k)| k)
        .find(|k| *k != m.own_key)
        .expect("markup 里没有第二个键");
    let old = format!("{h}.textContent = {rhs};");
    let new = format!("{h}.textContent = T(\"{other}\");");
    assert_eq!(app.matches(&old).count(), 1, "收尾写的锚点不唯一：{old:?}");
    app.replacen(&old, &new, 1)
}

/// 变异体②：整行删掉**忙碌态**写（忙碌 + 禁用在一起 —— 「把忙碌态删掉」式修法；只该翻 R3）。
fn r169_variant_drop_busy(html: &str, app: &str) -> String {
    let m = r169_axis_or_panic(html, app);
    let (h, rhs) = m.writes.first().cloned().expect("轴成员没有忙碌写");
    let needle = format!("{h}.textContent = {rhs};");
    assert_eq!(
        app.matches(&needle).count(),
        1,
        "忙碌写的锚点不唯一：{needle:?}"
    );
    let out = app
        .lines()
        .filter(|l| !l.contains(&needle))
        .collect::<Vec<_>>()
        .join("\n");
    assert_ne!(out, app, "忙碌写那一行没有被删掉");
    out
}

/// 变异体③：收尾写成 `T(自己的键)` —— R1 接受的**另一条形状**（把 markup 的键在 JS 里复述一遍）。
fn r169_variant_own_key(html: &str, app: &str) -> String {
    let m = r169_axis_or_panic(html, app);
    let (h, rhs) = m.writes.last().cloned().expect("轴成员没有收尾写");
    let old = format!("{h}.textContent = {rhs};");
    let new = format!("{h}.textContent = T(\"{}\");", m.own_key);
    assert_eq!(app.matches(&old).count(), 1, "收尾写的锚点不唯一：{old:?}");
    app.replacen(&old, &new, 1)
}

/// 变异体④：把一个**非轴**成员的绑定改成扫描器认不出的写法 —— 只该翻 R2（解不出 ≠ 干净）。
fn r169_variant_unresolved(html: &str, app: &str) -> String {
    let rd = r169_read(html, app);
    let m = rd
        .members
        .iter()
        .find(|m| m.resolved && m.writes.is_empty() && !rd.axis.contains(&m.id))
        .cloned()
        .expect("没有可用的非轴成员");
    let binding = m.binding.clone().expect("已解析的成员应当带绑定片段");
    let new = format!("document.querySelector(\"#{}\"){}", m.id, R169_BIND_SUFFIX);
    assert_eq!(
        app.matches(&binding).count(),
        1,
        "绑定片段不唯一：{binding:?}"
    );
    let out = app.replacen(&binding, &new, 1);
    assert!(
        r169_handler_body(&out, &m.id).is_none(),
        "改完之后那个成员仍能被解出 —— 变体没生效"
    );
    out
}

/// 轴：带 `data-i18n` 的静态控件，收尾必须回到**该控件自己的来源**（R169）。
///
/// 三条规则各自的含义见文件头。本测试只断言「三条同时成立」；每条规则的**牙**由
/// [`the_r169_rules_have_teeth`] 用合成变异体逐条测量（基线＝已知为绿的活树）。
#[test]
fn the_control_that_rests_owns_its_label() {
    let rd = r169_read(INDEX_HTML, APP_JS);
    assert!(
        rd.members.len() >= R169_MIN_MEMBERS,
        "名册少于 {R169_MIN_MEMBERS} 个成员 —— R2 退化成单例上的关系式：{}",
        rd.report()
    );
    assert!(
        !rd.axis.is_empty(),
        "没有任何成员写自己的标签 —— R1/R3 的母集是空的（规则会在空集上恒真）：{}",
        rd.report()
    );
    assert!(
        rd.verdicts() == (true, true, true),
        "R169 未修：某个带 `data-i18n` 的提交按钮被处理器用**别人的键**收尾（或处理器解不出、\
         或收尾只写了一次）。\n  {}",
        rd.report()
    );
}

/// 阳性对照：派生器在**已知为绿**的活树上读到东西（名册、键、句柄、捕获、写次数）。
///
/// 这是三条规则的射程保险：提取器若只会返回常量/空集，规则就成了恒真断言。⚠️ 绝对判词只打在
/// 自己读出来的事实上，且每一项都点名活树（#314：真树在两条腿上形状不同）。
#[test]
fn the_r169_roster_is_real() {
    // ① 名册：从 `ui/index.html` 派生的五个表单（多一个少一个都在这里响）。
    let roster: Vec<String> = r169_forms(INDEX_HTML)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(
        roster,
        vec![
            "login-form",
            "register-form",
            "verify-form",
            "forgot-form",
            "share-form"
        ],
        "从 markup 派生的名册变了"
    );

    // ② 键**从 markup 读**（不是门禁里写死的）：`#login-form` 的提交按钮声明的是 `login.enter`。
    let own = r169_forms(INDEX_HTML)
        .into_iter()
        .find(|(id, _)| id == "login-form")
        .map(|(_, k)| k)
        .expect("`#login-form` 不在名册里");
    assert_eq!(own, "login.enter", "markup 提取器读到的键变了");

    // ③ 提取器自证：五个处理器体都切对了地方（停在收尾花括号上、体里有 `preventDefault`）。
    for (id, _) in r169_forms(INDEX_HTML) {
        let b =
            r169_handler_body(APP_JS, &id).unwrap_or_else(|| panic!("`#{id}` 的处理器体切不出来"));
        assert!(
            b.trim_end().ends_with('}'),
            "`#{id}` 的处理器体没有停在收尾花括号上：{:?}",
            b.trim_end().chars().last()
        );
        assert!(
            b.contains("preventDefault"),
            "`#{id}` 的处理器体里没有 `preventDefault` —— 切错地方了"
        );
    }

    // ④ 轴上的事实（句柄 / 捕获 / 两次写），全部从活树派生。
    let rd = r169_read(INDEX_HTML, APP_JS);
    let m = rd
        .members
        .iter()
        .find(|m| m.id == "login-form")
        .expect("轴成员不在名册里");
    assert_eq!(m.handles, vec!["btn".to_string()], "提交按钮的句柄名册变了");
    assert_eq!(
        m.captures,
        vec!["origLabel".to_string()],
        "捕获名册变了（收尾据此写回）"
    );
    assert_eq!(m.writes.len(), 2, "标签写次数不是两次（忙碌 + 收尾）");
    assert_eq!(
        rd.axis,
        ["login-form"]
            .into_iter()
            .map(String::from)
            .collect::<BTreeSet<_>>(),
        "会写自己标签的成员不是恰好 `#login-form`"
    );
    assert!(
        m.last_how.as_deref() == Some("capture (read from the element)"),
        "轴的收尾判词不是「捕获」：{:?}",
        m.last_how
    );
    // ⑤ 另外三个带句柄的成员只切 `disabled`（不写标签）⇒ 不进 R1/R3 的母集；上架表单委托给
    //    `withLoading`（那个工具的收尾本身就是捕获）—— 两者都是**合法形状**，如实钉住。
    for id in ["register-form", "verify-form", "share-form"] {
        let m = rd
            .members
            .iter()
            .find(|m| m.id == id)
            .expect("成员不在名册里");
        assert!(m.writes.is_empty(), "`#{id}` 不该写标签：{:?}", m.writes);
    }
    let share = rd
        .members
        .iter()
        .find(|m| m.id == "share-form")
        .expect("成员不在名册里");
    assert!(
        share.delegated,
        "上架表单没有委托给 `withLoading` —— 射程说明过期了"
    );
}

/// 三条规则**各有独立的牙**：合成变异体各只打翻它针对的那条（基线＝已知为绿的活树）。
///
/// ⚠️ 变异体一律从**活树**上构造（不写第二棵基线），且每个锚点都断言唯一（#612 家族）：
/// 变体若没生效，构造器里的断言先响，而不是让某条腿因错误的原因变绿（#605）。
#[test]
fn the_r169_rules_have_teeth() {
    // ① 原缺陷的形状：收尾写**别人的键** ⇒ R1（轴）与 R2（类）翻红，R3（反向）不动。
    let foreign = r169_variant_foreign(INDEX_HTML, APP_JS);
    let rd = r169_read(INDEX_HTML, &foreign);
    assert_eq!(
        rd.verdicts(),
        (false, false, true),
        "把收尾改回别人的键，只应翻掉 R1 与 R2：{}",
        rd.report()
    );

    // ② 删掉忙碌态写 ⇒ 只翻 R3。
    let dropped = r169_variant_drop_busy(INDEX_HTML, APP_JS);
    let rd = r169_read(INDEX_HTML, &dropped);
    assert_eq!(
        rd.verdicts(),
        (true, true, false),
        "删掉忙碌态只应翻掉 R3：{}",
        rd.report()
    );

    // ③ 收尾写成 `T(自己的键)` —— R1 接受的另一条形状 ⇒ 三条全绿。
    let own_key = r169_variant_own_key(INDEX_HTML, APP_JS);
    let rd = r169_read(INDEX_HTML, &own_key);
    assert_eq!(
        rd.verdicts(),
        (true, true, true),
        "写回 markup 自己的键是 R1 接受的形状：{}",
        rd.report()
    );

    // ④ 一个非轴成员的绑定认不出来了 ⇒ 只翻 R2（解不出 ≠ 干净）；且该成员必须真的进了未解析集。
    let unresolved = r169_variant_unresolved(INDEX_HTML, APP_JS);
    let rd = r169_read(INDEX_HTML, &unresolved);
    assert_eq!(
        rd.verdicts(),
        (true, false, true),
        "把一个非轴成员的绑定改成认不出的写法只应翻掉 R2：{}",
        rd.report()
    );
    assert_eq!(
        rd.unresolved,
        ["register-form"]
            .into_iter()
            .map(String::from)
            .collect::<BTreeSet<_>>(),
        "未解析集不是恰好被改动的那个成员：{}",
        rd.report()
    );
}
