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

#[cfg(test)]
mod tests {
    use super::*;

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
