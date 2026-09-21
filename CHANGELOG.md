# Changelog

All notable changes are recorded here. Versions follow [SemVer](https://semver.org/).

## v0.7.27 (2026-09-21)

自 v0.7.26 起 1 个 PR（#272）。本版修的是**同一族缺陷的第二次现身**：请求体上限分布在**两层**上、而两层是两个数。

- **请求体上限只有一个数，并抬到 277 MiB（#272 87c8777）** — 上限同时存在于 axum 提取器那层（`DefaultBodyLimit::max`，`String` / `Json` 的 2 MiB 默认值只认它写的 `DefaultBodyLimitKind` 扩展）与 tower-http 的外层粗闸（`RequestBodyLimitLayer::new`，只看 `Content-Length`、超限时**不读体直接 413**）。外层抬不动内层，但**只要它更小就赢** —— 把常量从 8 MiB 抬到 277 MiB 时外层仍停在 `70 * 1024 * 1024`，实测 71 MiB 的体返回 `413 length limit exceeded`（tower-http 自己的正文，不是提取器那句前缀）⇒ 抬了个寂寞。修法：`GATEWAY_BODY_LIMIT` 8 MiB → **277 MiB**，外层改为引用同一常量（不再自带字面量），并把外层从 `main.rs` 移进 `routes::router()`（测试构造的正是这个 router ⇒ 被测的栈就是生产的栈）。新门禁 `src/body_limit_gate.rs`（仅测试期编译、零新依赖）：**每处层构造的实参必须是 `GATEWAY_BODY_LIMIT`**，且全树不得出现第二个以 `BODY_LIMIT` 结尾的常量。探针 3 MiB → 9 MiB（跨过旧的 8 MiB）＋新增 71 MiB 一条（在改前的外层上必红）；未认证端点的 2 MiB 默认**刻意保留**作负对照。`cargo test` 302 → 308。⚠️ **内存**：提取器整个缓冲请求体、转发前还 `clone()` 一次 ⇒ 单请求峰值约为该值的 2~3 倍；prod 主机 1.8 GiB / 无 swap / 容器未设 `mem_limit`，**在把公网入口放宽到同一量级之前应先处理这件事**（公网目前仍受 nginx 内置 1 MB 限制，且该主机只对外开 80/443）。**无 schema 变更、无 config 变更。**

## v0.7.26 (2026-09-21)

自 v0.7.25 起累计 10 个 PR（#261–#270）。本版两条主线：**「一个事实只有一个真源 / 显示口径必须等于消费口径」的第 2 批**（前端 6 处），以及 **i18n 的可达性收口**（先上门禁、再删死键）。另含**网关请求体上限真正生效的那一层**（rant `2026-09-18T09:14:18` 的应用侧一半）。**无 schema 变更、无 config 变更**（`config.toml` 无需同步）。

- **请求体上限改挂在 axum 真正读取的那一层（#267 2ccbc76）** — v0.7.10 起的那句 `RequestBodyLimitLayer::new(70MB)` 从未生效：tower-http 那层只把**外层流**包成 `Limited`，**不往请求扩展里写任何东西**；而 axum 提取器读的是 `DefaultBodyLimitKind` 扩展，缺失时回落到 `DEFAULT_LIMIT = 2MB` ⇒ 实际上限 = min(70MB, 2MB) = **2MB**（直连实测 2.02MB → `413 Failed to buffer the request body`）。改为三条网关路由（`/v1/chat/completions`、`/anthropic/v1/messages`、`/v1/responses`）各挂 `.layer(DefaultBodyLimit::max(GATEWAY_BODY_LIMIT))`（`GATEWAY_BODY_LIMIT = 8 MiB`），未认证端点保持 2MB 默认；A/B 在真 crate 内跑（还原路由层 ⇒ 正向测试在 `/v1/chat/completions` 上 413、负对照仍绿）。⚠️ 本 PR 落在 `v0.7.25` **之后** ⇒ v0.7.24 / v0.7.25 都不含它；**且公网仍受 prod nginx 的 1MB 内置默认限制**（给反代加 per-domain `client_max_body_size` 是宿主的部署动作，尚未做）。同 PR 更正了下方 v0.7.10 那条被实测证伪的「raised to 70MB」。
- **侧栏「永久点数」改读可花的那一半（#261 13898d6）** — 运营者给自己充值后，`inlineOpsTopup` 的自刷新分支自己又取一次 `/api/wallet` 且只读 `w.balance`（永久额），而产品把可用余额定义为 `available = balance + gift_balance`、赠送与角色无关（`gift::ensure_daily_gift` 挂每个已认证请求）⇒ 实测「100 + 1、充 100」屏幕显示 **200**、真值 **201**，且永不自愈。改为调用唯一写者 `refreshWallet()`（只把字段换成 `w.available` 是被探针拒掉的竞争修法）；门禁 `the_session_balance_has_one_source_and_it_is_the_spendable_half`。
- **交易载荷签名必须覆盖时间段（#262 03b6be7）** — 缓存签名只含列筛选、不含三个时间段状态值 ⇒ 从第 2 页起改一次时间段**不发新请求**（屏幕停在旧数据）。改为 `txQuerySig()`（**不可**由 `txRangeParams()` 派生 —— 其中的毫秒时间戳会让签名恒变、退化成自喂请求风暴）＋新增**唯一**重拉触发器 `reloadTransactions()`，四个控件全走它；门禁 `the_transaction_payload_has_one_signature_and_one_reload_trigger`。
- **设置页三族控件：要么接线、要么显式惰性（#263 ad72020）** — 昵称可编辑却无人消费（后端无 name 写路径）、`#prefs-model` 看着能选但全仓无消费者（设计稿那个 `<select>` 本就无引用）、三枚通知开关渲染成**已勾选**却无 id/无 name/无通知子系统 ⇒ 输入在下次渲染时被抹掉。按仓内惰性成例（`readonly`/`disabled` ＋ 卡片级提示）处理并摘掉误导性的 `checked`；门禁 `settings_controls_are_either_live_or_marked_inert`（`inert ⟺ ¬consumed` **双向**，反向挡「一键全禁用」）。
- **语言层拥有元素内容（#264 ba486b2）** — `applyStatic()` 对每个 `[data-i18n]` 元素做 `innerHTML = t(key)` ⇒ **一个带文本 `data-i18n` 的元素，其后代上的语言层钩子永不生效**（祖先那步把后代连同属性从文档摘掉，随后对已分离节点设值：无异常、无效果）。恰 2 处：加额申请卡的提示**两种语言都不显示**、登录页底部那个键成孤儿。改为**兄弟 span**；门禁 `no_data_i18n_attribute_nests_inside_a_data_i18n_element`。
- **「重新上架」不再被报成「已恢复」（#265 6936973）** — 共享动作按钮按状态**三值**取（`on→pause` / `paused→resume` / `off→relist`），而处理器用**两值** `next` 报结局 ⇒ `off → on`（重新上架）的 toast 是「已恢复」，正确键 `share.toggle.relisted` 两包俱在却**无人可达**。新增 `SHARE_TOGGLE`（状态 → {标签, 下一状态, 结局}）作唯一真源；门禁 `the_sharing_toggle_outcome_comes_from_the_same_entry_as_its_action`（表键集 == 徽标状态集）。
- **共享行 / 仪表盘印 plan 的标签而非配置 id（#268 7b1929a）** — `keys.plan` 只存配置 id（如 `deepseek-paygo`），上架下拉与成功 toast 一直经 `planLabel()` 印**标签**，而共享表单元格与仪表盘卡把 **id 原样印出** ⇒ 同屏两个口径、中文界面印出未翻译 token。新增 `planList()` / `planById()` / `planLabelById()`，渲染点只消费派生值。同 PR 必须带走两个阻断：`i18n_pack` 的判别式把**解析器**认成一个函数名（改判两个入口之一）＋ `state_gate` 槽闭合门禁（抽 `refreshPlans()` 作唯一写者）。
- **运营卡不再把「禁用计数」当「健康判定」念（#269 e5ee178）** — 数据里**没有**任何健康信息（`/api/ops/runtime` 只回 `total`/`on`/`off`）⇒ 用户**暂停自己的 key**（正常操作）会让运营者看到红色「全部失败」。三个判定键**重命名且改写**（`healthy/abnormal/failed` → `allOn/someOff/allOff`，标题「健康」→「状态」），全停用改中性色 —— **会撒谎的键名没法被门禁钉住**，故键名也改。**零新 i18n 键**。
- **整包可达性门禁、并删掉 23 个无人可达的键（#266 a4cb622 / #270 77b2a82）** — 先建门禁 `every_pack_key_reaches_a_consumer`（标识符边界 ＋ 唯一动态前缀 ＋ **精确相等**的日落清单：新增孤儿红、砍清单条目红、把日落键接上线也红），它算出 **59** 对不可达；再把其中 **23 对确定死键删掉**（两个包各 −23 行、清单 59 → **36**、`ZH/EN_KEY_COUNT` 811 → **788**）。⚠️ 门禁**不是**这次的安全网（它只证「清单 == 计算出的不可达集合」⇒ 删一个**其实可达**的键它照常绿）——真正的判据是**零消费者**复核：独立跑边界精确谓词（语料 `index.html` ＋ `app.js` ＋ `api.js` ＋ `ui/README.md`，**不含语言包本身**），实测 **23/23 命中 0**。剩下的 36 条已逐条判负（一次更大的产品裁定 / 值同时是静态文本 / 该接线 / 宿主裁定）。

## v0.7.25 (2026-09-15)

自 v0.7.24 起累计 18 个 PR（#242–#259）。⚠️ **数据库 schema 14 → 15**：新增两条覆盖索引，迁移在启动时执行。本版两条主线：dev 上 NFS 的**查询/写入性能**，以及前端一批「**显示口径必须等于筛选口径、一个事实只有一个真源**」的缺陷。**无 config 变更**（`config.toml` 无需同步改动）。

- **NFS 上不再映射数据库、不再每请求真写（#259 a28ad3e）** — dev 上 `/api/transactions` 1.7–2.3s、`/api/ops/runtime` 2.1s，根因是**两个独立的 NFS 机制**叠加。① `PRAGMA mmap_size=64MB` 在这台 NFS 上是**慢路径**（映射页进不了客户端缓存 ⇒ 每次查询都真读文件）：真库同一条覆盖索引 `COUNT(*)`，mmap=64MB **每次 1.7–3.1s、进程实读 250 MiB / 5 次**，mmap=0 **~10.5ms、实读 80 KiB**（同副本放本地盘两者都 7ms ⇒ 不是存储慢）；尺寸扫描 0→13ms / 16MB→334–407ms / 64MB→1.7–2.2s / 128MB→2.2–2.8s ⇒ **任何非零值都更差**，故取 0。② **每一次真写都会打掉 NFS 客户端对该文件的缓存** ⇒ 只改 ① 只能 1.9s→~1.1s，到不了目标；而**每个已认证请求**都经过的 `dao::touch_api_key` 会写 `last_used`，加 60 秒时间守卫后（命中 0 行 ⇒ SQLite 不落页、不打掉缓存；实测提交 7ms vs 真写 34ms）把「每请求一次真写」降为「每分钟至多一次」。同 PR 更正了 `db.rs` 里被实测证伪的 v12 mmap 注释（原注释称「整库常驻进程内存、远端只首读一次」）。prod（本地盘、2.26MB）实测中性：200 次 COUNT 11ms（64MB）vs 9ms（0）。
- **应急索引进迁移 + 条件 JOIN 门禁（#242 5d4e6f4）** — v0.7.24 的按需 JOIN 与两条应急索引此前只存在于 dev/prod 的**手工操作**里，新部署仍会踩 22s 慢查询。新增 **v15 迁移**建两条覆盖索引（`transactions(user_id,time,type,pts,tokens,…)` 与 `transactions(key_id,type,pts)`，后者此前在该查询上零索引），并补**计划级验收**：`EXPLAIN QUERY PLAN` 必须报 `COVERING INDEX`（名字断言 ≠ 覆盖性证明），以及第二条 `perf_gate` 不变量（`tx_joins()` 的调用点只在包装器内，且包装器必须保留无 JOIN 分支）。
- **共享页收益改为批量聚合（#243 199566c8）** — `sharing_row` 曾对每行跑一次 `SELECT SUM(pts) … WHERE key_id=?` ⇒ 共享页 N 行 = N 次额外语句；改为一条 13 列 `SELECT` + `LEFT JOIN` 一次性聚合（同文件 `list`/`patch` 两份逐字重复的 SELECT 也随之合并为一处）。实测（20 万行）8 次子查询 1.54ms → 一次批聚合 0.02ms；门禁 `perf_gate::the_sharing_row_builder_runs_no_sql`（行构造器体内零 SQL）。
- **交易页缓存单一写者（#250 a81839b）** — `Live.transactions` 曾有两个写者：`loadTransactions`（`page_size=10`+筛选，并记录 `txTable.loaded*` 作为缓存有效性证据）与 `loadDashboard`（只为「交易笔数」卡拉 `page_size=1`）；后者的载荷借守卫通过 ⇒ 再入交易页时**表只 1 行、汇总卡挂「当前筛选」却显示全时段数字、趋势卡谎报「加载失败」**。改为仪表盘用**自己的槽** `Live.tradeCount`；门禁钉 `writes(Live.transactions) == {loadTransactions} == writers(txTable.loaded*)`。
- **会话边界清空缓存 + 钱包视图补 loader（#251 0e4acc2）** — `Live` 缓存活得比填它的身份更久 ⇒ 换账号后侧栏「永久点数」显示**上一个人的钱**且永不自愈。`resetSessionCaches()` 改为**槽名派生自 `Live` 字面量**（不维护名册），并在登出/401 与 boot/登录两个边界各调一次；同时给钱包视图补上此前缺失的 loader。门禁：身份边界必须清空每个槽、`renderView` 每个分支必须既渲染又装载。
- **钱包页月度净变动改用共享写者（#253 f83df40）** — `#month-changes`（钱包）与 `#dash-month-changes`（仪表盘）由同一 `renderMonthChanges()` 画、都读 `Live.dashboard`，而该槽唯一写者是**仪表盘的** `loadDashboard` ⇒ **会话在钱包视图上建立**时（含在钱包页登出再登入）净变化恒印 `0` + 「本月暂无变动」，永不自愈（带 token 刷新看不到，因为 boot 的无条件 `renderView("dashboard")` 顺手装好了槽）。新增 `refreshDashboard()` 作该槽唯一写者，两个 loader 各调一次；门禁升级为**传递闭包**版（渲染闭包读的槽，其 loader 闭包必须写它）。
- **boot 只装载当前目的地视图（#254 6be548a）** — `DOMContentLoaded` 无条件 `renderView("dashboard")` 且跑在 `restoreSession()` **之前** ⇒ 带 token 时仪表盘那套查询在会话还不存在时就发出（`log[0]` 是 `/api/wallet`），随后被 `resetSessionCaches()` 作废、再装一遍；落在 `#/transactions` 也照拉仪表盘那套 5 次；过期 token 时同一条「Session expired」toast 发 6 次（用户看到 3 条）。删掉那句渲染，视图只由 `switchView` 按**当前目的地**装载；门禁 `the_boot_handler_touches_no_view`。
- **模型行身份 = 模型而非下标（#255 e64ae6c）** — `modelsToView()` 曾用**数组下标** `id: i` 当身份，还被持久化进 `localStorage` ⇒ 身份跨渲染/跨会话/跨数组（游客表 7 行、id 从 **1** 起；后端按 `provider, model` 排）⇒「最近使用」芯片指向**另一个模型**、或整条消失、或目录一变改写别的模型，点开的对话也一样。改为 `modelKey(m)=provider/model` 作唯一身份（三处 `data-*` 载体与点击侧全改用它，旧下标一次性丢弃）；门禁 `the_model_row_identity_is_the_model_not_its_position`。
- **市场数据源由会话决定（#256 afc9c6f）** — 市场面写成 `Live.models ? 活目录 : 游客表`（按「数据到没到」而非「会话在不在」分支）⇒ 登录态 `/api/models` 失败/超时时：厂商下拉列出 **10** 个厂商（5 个本部署没有）、「最近使用」芯片画游客市场模型（点开只提示加载失败）、徽标显示 **13**（活目录 6、游客市场 7 ⇒ 13 是第三个数）。改为 `marketRows()`/`marketProviders()` 一处真源，判别式为 `loggedIn()`；门禁 `market_tables_follow_the_session_not_the_data`（零豁免清单）。
- **侧栏只 advertise 能按响的键位（#257 9982063）** — 每个导航项印数字角标 + 「快捷键 N」提示，而键盘上只有**一个**数字键处理器；两半是同一契约、必须取自同一数组 `NAV_ORDER`。`renderNav()` 的**游客**分支手搓了一个同形字面量（不属于登记表）⇒ `indexOf` 恒 **-1** ⇒ 角标印 **`0`**（死键）、提示写「Shortcut 0 · Marketplace」，而真正能打开市场的键是 **`2`**（游客永远看不到）。改为游客组从登记表里筛；门禁 `the_sidebar_advertises_only_digits_that_work`（四规则各有独立的牙）。
- **管理员「总余额」卡折进赠送额（#258 4af7cb7）** — 该卡**副标题**写「余额 + 赠送」、正下方「可用」列也是 `balance + gift_balance`，而取值只加永久余额 ⇒ **少掉全部赠送额**。赠送是**可花会过期**的真钱（`gift.rs` 真划走），产品把可用余额定义为 `available = balance + gift_balance`（用户自己看到的「余额」取的就是它）⇒ 方向由产品钉死（「把副标题改窄」不是修法）；修法一行＝合计折进 `gift_balance`；门禁 `the_admin_total_balance_card_sums_what_its_caption_names`。
- **后端错误全量登记进词表（#249 5183e41）** — `api.js` 把后端 `error` 整串交给 `ERR_MAP`，未登记即**原样返回** ⇒ en 界面显示中文而 CI 全绿（45 条里 20 条泄漏，`删除部门` 409 由可达控件触发）。20 条逐字派生登记 + 名册改为**派生**（`BACKEND_ERROR_SOURCES` 必须覆盖每个产出 `"error"` 字面量的文件）。同 PR 修掉一个**静默三轮**的失真：剥注释函数用 `byte as char` 搬 UTF-8 ⇒ 中文词表被撕成 Latin-1 乱码、**永远比不中**（旧消费者只问 `is_ascii()`，乱码同样非 ASCII），改为 `Vec<u8>` + `String::from_utf8` 并加保真断言。
- **后端不再自造中文展示标签（#252 91a2da1）** — 两处把**自己编的中文标签**放进响应**数据**字段、前端原样渲染（`admin.rs` 的 `'（未分配）'` 部门桶名 → `#usage-dept`；`gateway.rs` 的 plan 兜底名 `'API（按量）'` → 上架表单/`#sf-plan`，config 的 `[[plans]]` 全不写 `name` ⇒ 恒触发），而 `ERR_MAP` 只扫 `"error"` 字面量 ⇒ **这一类零门禁**。同屏/同 payload 早有本地化机制（`T("common.unassigned")`、`showPlanHint`）⇒ 是漏用而非缺功能；改为后端只发**语言中性**标记、前端本地化，并加门禁（中文**数据**字面量集合须恰等于一份**日落清单**）。
- **凭据 401 不再当作会话过期（#245 301ca4d）** — `api.js` 曾把一切 401 当「会话过期」（清 token + 登出 + toast）⇒ 登录失败时会发一条**误报**的「会话已过期」并把用户登出。改为 `opts.on401` 逃生口（401 语义由调用方声明，默认仍是会话失效 ⇒ 业务端点零改动），登录调用点声明为凭据错误；门禁 `credential_401_is_not_a_session_expired`。
- **boot 非 401 失败不再演成「被登出」（#244 3199731）** — boot 的 `catch` 对任何**非 401** 失败都停在登录页（token 仍在、hash 仍指向上次视图）⇒ 与「被踢出」视觉不可区分。改为三档：401 回登录页（不重试）；网络错误/5xx 重试一次；其余 4xx 不重试但**仍进 app**、由各视图的降级态（加载失败 + 重试）承接 —— 「加载失败」交各视图，「未登录」只由 401 承担。
- **时间戳不再被手工切片（#246 6e022e1）** — 三个消费者自己切线上时间串：管理员加额行 `.slice(5,16)` 把 ISO 的 `T` 泄进 UI（屏幕上是 `09-13T16:30`、且是 UTC）、设置页 Key「创建时间」`.slice(0,10)` 取 **UTC 日**（东八区 08:00 前显示「昨天」）、交易行 `.replace("T"," ").slice(0,16)` 在渲染器**之前**把秒抹掉 ⇒ 屏幕上的秒永远是伪造的 `00`。改为整串交给现成 helper（`fmtPrecise`/`timeCell`/`timeAgo`）；门禁 `wire_timestamps_reach_the_renderer_unsliced`。
- **行内卡片 Enter 从每个字段提交（#247 ecc654c）** — Enter 提交是逐字段手工登记的，模型表单 10 个可输入控件里 **8 个**按 Enter 没反应（另三张卡片是 2/2、1/1、1/1，`#share-form` 本身是真 `<form>`+`type=submit`）。改为 `wireEnterSubmit(卡片, 确认按钮)` 委托到容器（当前与以后的文本控件自动生效；焦点在「取消」上按 Enter 仍只取消；处理体只 `btn.click()` ⇒ 忙碌态/校验/请求仍只有一处）；门禁 `enter_submit_is_delegated_to_the_card`（不变量**从 markup 派生**而非人写名册）。
- **市场可用性标签取自该行自身（#248 818ed88）** — `availPill()` 只认 `keys` 计数，而游客兜底表按「虚构数据已移除」**不携带**该字段 ⇒ 游客 **7/7** 行的绿点/按钮说「可用」而这一格写「无 key」（自相矛盾）。改为有计数时按计数分档（≥2 / ==1 / 0）、无计数时落到该行的 `avail` 布尔；门禁同时钉**消费侧**（须出现 `m.avail`）与**数据侧**（`MARKET` 不得携带手写 `keys`，否则等于把虚构运营数据摆给游客）。

## v0.7.24 (2026-09-14)

- **交易页慢查询导致「刷新跳回登录页」（#240 95d69ba）** — 宿主实测：dev 上登录后进 `#/sharing` 刷新会**停在登录页**（URL 仍 `#/sharing`、token 仍在 sessionStorage）。根因是 `/api/transactions` 的 summary / COUNT / trend 三条语句**无条件**拼上 keys/users/api_keys 三个 LEFT JOIN，而这几个 JOIN 一出现 SQLite 就**弃用覆盖索引**、退回逐行回表：同一条聚合语句、同一个库（18.6 万行，NAS/NFS），**不带 JOIN 0.21s、带 JOIN 22.6s（约 100×）**。实际只有 `user_name` / `key_name` 两个筛选会引用 JOIN 表（`model` / `status` / pts 区间全在 `transactions` 自身），故改为**按需 JOIN**（新增单一判定谓词 `needs_joins()`，避免 JOIN 与 `tx_where` 的引用再次分叉）；列表查询保留 JOIN（要渲染 user_name / key_label / key_name，且有 `LIMIT` 兜底）。故障链：慢查询长时间占住共享 DB 锁 → `/api/me`、`/api/models` 被排队（实测单次 `/api/me` 被卡 35s、并发下出现网关 504）→ 前端 boot 串行等 4 个请求（其中 `/api/models` 非必需）才 `enterApp()` ⇒ 任一卡住则登录页无限期停留（实测 3 次刷新 2 次卡住）。
- **README 标语重写 + 线上实例入口（#239 cefd589）** — 原文案「共享出去赚点数，需要时也能用别人的」主语省略、宾语含糊、只讲省钱不讲收益；改为「把闲置额度共享出去赚点数，再用点数兑换别人共享的模型 —— 一份订阅，换来整个模型池」，并同步「适用场景」表格里的同句复述。新增线上实例 `https://aitokenpool.args.fun/`（语言切换行下方 + 快速上手「不想自己部署」路径）。中英两版同步。

## v0.7.23 (2026-09-14)

自 v0.7.22 起累计 76 个 PR（#161–#237），是本项目迄今最大的一次版本。⚠️ **数据库 schema 12 → 14**：含一次性数据修复（`keys.used` 按账本重算）与 4 条新索引（迁移在启动时执行；dev 库 79.6 MB / 36.6 万行，升级前请先备份 `data/aitokenpool.db`）。

- **OpenAI 流式「无 usage ⇒ 计费 0」（#187 ec158bd）** — 该协议下上游 usage 是 **opt-in**（`stream_options.include_usage`），而网关把客户端请求体原样转发、从不设该开关 ⇒ 整条流按 0 token 结算：余额不动、`usage_records` 无行、`transactions` 无行；`month_calls = COUNT(*) FROM usage_records` 使同一次调用在运营视图里也一并消失。修法：同一 helper 为 `openai_chat` 出站体注入 `stream_options.include_usage`（客户端自己设过则保留），anthropic（`message_start`）/ responses（`response.completed`）本就必报、不动；**仍无 usage 时 fail closed 报错**，且区分「从未上报」与「上报 0」。**刻意不估算 token** —— 缺数据不等于数据为 0。
- **DeepSeek 缓存命中 token 的三种拼写（#188 73fae8c / #189 9bd2dec）** — 同一规则在四处实现，流式认 3 种拼写而非流式只认 2 种 ⇒ 计费面超收（未配命中价 5.5×、已配 2.9×）、高峰价下命中的单位价错 30×；同时非流式 openai→anthropic 的响应翻译漏掉命中 token。两处补齐后四个读者一致（余下只认 1 种者皆为协议原生、流式/非流式同口径）。
- **模型价格校验漏掉 `cache_hit_input_per_m`（#190 5fcc68e）** — `validate_common` 查了 6 个价格字段里的 5 个，独漏命中价的非高峰兄弟；该字段可为负则一路无钳制进结算（`balance-pts` 负数变加法、`earn` 负数变扣减，实测 -8 点）。补齐 ≥0 校验。
- **流式响应不再被总超时切断（#210 acc6f49）** — 出站客户端 `.timeout(120s)` 是**整个请求含响应体**的截止时间，而流式转发共用它 ⇒ 持续出数据的流到 120 s 被硬切（实测每 30 s 一帧的假上游只收到 3 帧、`[DONE]` 永不到达），长回答与推理模型常态超过 120 s。改为两条明确策略：非流式保留 `.timeout(120s)`；流式用 `.connect_timeout(120s).read_timeout(120s)` 无总截止 —— 有进展就不罚，静默 120 s 仍判死。`read_timeout` 此前全树从未设置（流式只有总上限、没有本该有的停滞检测）。
- **上游响应体读取失败不再伪造 200（#211 e885cd6）** — `resp.bytes().await.unwrap_or_default()` 把读失败折叠成空 body，再让状态行单独决定结果 ⇒ 2xx 而 body 从未到达的请求被当作 `200 OK` + `0` 字节交给客户端。
- **跨协议翻译五连修（#205 45a26bd / #206 b46f113 / #207 254eacf / #208 03a9aca / #209 bbc74ea）** — ① 完成信号在流式与整体两个翻译器里各推一次且结论不同，改为同源；② `openai_sse_to_openai_responses` 在 `[DONE]` 发 `event: response.completed` + `data: {}`（既无 `type` 也无 `response`），且 `finish_reason` 分支也会发 ⇒ 正常上游产生两次终态，改为恰好一次完整终态；③ 流式 `openai_chat → responses` 把消息项硬编码 `output_index: 0`、工具项也从 0 起 ⇒ 文本 + 工具调用两项同号，按 `output_index` 取键的客户端必然错位，改为每项唯一序号且保序；④ Responses 客户端的工具往返由 `function_call` / `function_call_output` 两个**无 `role`** 的项承载，而转换器只认 `role` + `content` ⇒ 工具项被直接丢弃；⑤ `tool_choice` 只在 `anthropic → openai_chat` 一条路径翻译，其余 4 条跨协议路径全部丢失。
- **认证路径不再占住异步 worker 与共享 DB 锁（#212 ce68a9f / #213 fa6a794 / #214 a2d5fe1 / #223 174879a）** — ① 验证码写入分两步且绑定字面量 `"+10 minutes"` 作 `expires_at`（列里就存了这串字），重发限流也在另一把锁里另查，三条语句改为同一临界区内一次写清；② 密码 KDF（argon2 ~0.24 s）在进程级 DB 互斥锁内执行，一个未认证请求即可卡住所有 DB 路径（改后锁可用次数 0/0/1/0 → 158/159/525/670）；③ 邮件发送是阻塞 I/O（3 次重试 × 15 s 超时），却在三个未认证 handler 里就地调用；④ 哈希/校验已挪出锁，但仍**内联跑在异步 worker 上**，改为阻塞池。
- **认证边界两处（#185 223ad55 / #186 f16fe47）** — 登录邮箱大小写归一（`Admin@X` 与 `admin@x` 曾是两个账号）；`resend_code` 曾对**任意**地址发信用码，改为镜像 `forgot_password` 的守卫、按存在性判断（带阳性对照）。
- **共享上架校验补齐（#199 ad100dd / #201 928dd24 / #202 ad817cd）** — ① `POST /api/sharings` 曾原样落库调用者给的 `provider`/`model`，而每次调用都按 `(provider, model)` 查价、查不到即回落 `(0.0, 0.0)` ⇒ 不可计价的组合被平台**免费代理**；② 同理补上「能路由」（plan 必须在 config `[[plans]]` 中可解析出端点），否则上架即失败；③ `mask_upstream_key` 按**字节**切片（`&key[..2]` / `&key[len-4..]`），多字节字符跨边界即 panic。
- **`keys.used` 单位修正 + 一次性数据修复（#217 164b8d3，schema 12 → 13）** — `keys.quota` 是点数（表单标题即「声明额度（点数）」），而 `used` 累计的是 **token** 数，共享页把两者相除画进度条并都标「点」⇒ 「已用 1000000 / 5000 点」、条常年钉在 100%。改为累计 `pts`（同单位），并**从账本一次性重算历史值**（`used = SUM(transactions.pts WHERE type='consume')`，门禁 `schema_version < 13`）。同提交把 `schema_version` 读数改为 `MAX(version)`：该表无唯一约束，真库里已堆 22 行，单行读会让每次启动都追加一行并使每个 `v < N` 门禁**永远为真**。
- **月聚合查询性能（#234 93b2ebd / #236 83ecf4e，schema 13 → 14）** — 12 处 `strftime('%Y-%m', time) = strftime('%Y-%m','now')` 把索引列包进函数 ⇒ 索引不可用、退化为全表扫（dev NAS 实测单查询 3.8 s / 4.5 s，表现为「刷新页面十几秒像被登出」）。根因**不是** NAS 吞吐（4 KB 热读 1.9 µs，与本地盘同级）而是「一次查询摸多少页」。改为**闭**区间范围谓词（闭上界必须写，否则会带进未来月行——已加断言）+ 4 条新索引：`transactions(user_id,time,type,pts)`、`transactions(time,type,pts)`（前者给带 user_id 的聚合，后者给全库聚合）与 `usage_records(time)`、`usage_records(user_id,time)`（该表此前**零索引**）。实测 3845 ms → 119 ms、4525 ms → 84 ms。
- **部署产物不再提供非法主密钥默认值（#235 7852277 / #237 bc4b111）** — `docker-compose.yml` 的 `ATP_MASTER_KEY` 默认值是 `dev-master-key-请替换`（18 字符、含 `-` 与中文）：`crypto::parse_master_key` 拒绝它，`Crypto::from_config` 只记一行日志后**落穿到随机主密钥** ⇒ 重启后已加密的上游 key 全部不可解密、所有上游调用 503 —— 一次**静默全量故障**，且看起来像 key 问题而不是配置问题。改为 `${ATP_MASTER_KEY:?…}`：缺失即 compose 报错退出并打印生成命令（**刻意不给合法默认值** —— 那是把「重启即坏」换成「所有部署共用一把公开主密钥」），并修正同文件那句「未设置时使用示例默认值」的注释（该值根本解析不了，注释承诺的行为并不存在）。新增 `cargo test` 门禁：随包产物里的主密钥默认值必须「要么不存在、要么恰好 64 位 hex」，按形状（`Required` / `Default` / `Literal` / `NotALiteral`）分别判定，因此文件自己的快速开始示例不会被误读为默认值。
- **模型目录按官方改名（#233 11c5324）** — DeepSeek 官方现名 `deepseek-flash`（V4.1-Flash），旧名 `deepseek-v4-flash` / `deepseek-v4-flash-vision-exp` 已退役（请求仍应答，但上游按 Flash 现行价承接计费）；`deepseek-v4-pro` 不变。影响面不只是展示：`POST /api/sharings` 以「`models` 表里存在该 (provider, model)`」判定可计价，旧名不在表里会让上架被拒。示例目录 14 → 13 条、vision 并入新名。**⚠️ 部署侧须同步改自己的 `config.toml`** —— `seed_models` 是**全量同步**（config 为准，缺席即删行）。
- **钱包 / 交易口径统一（#191 562c8f6 / #192 0c25117 / #193 9fead29 / #195 f55cfd2 / #196 f6eed10 / #197 6997241 / #215 1febe59 / #218 a9596f9 / #219 0feae18 / #220 1d5fcdd）** — 交易方向**只由 `type` 决定**（6 个 writer 全存正数 `pts`，不再从符号推方向；后端、前端、运营计数器三处各自的副本同时收口）；仪表盘 `net` 改按**当月**（此前是近 7 天合计，却显示在当月分类型行之上）；「点数」列的筛选/排序/CSV 导出改按**渲染出的带符号值**（其余消费者仍按原值）；Key 列筛选与显示取同一个值；周趋势的「周」定义去重（周一起、闭区间）；7 天序列补齐空日（sparkline 的 x 轴此前是「序号」而非时间）；四个 token 列可按真实值排序（此前表头按钮点了不动）。
- **赠送过期进入账本（#194 e3f85d2）** — `gift.rs::expire_past_gifts` 把过期点数标掉并重算 `gift_balance`：**点数真离账却没有任何 `transactions` 行**，是唯一「移动余额而无账本行」的路径 ⇒ 账本与账户不可对账。改为清扫前先 `SUM` 即将过期额、仅 >0 时插**一行** `expire`（幂等、只记真正过期的部分），钱包汇总/趋势、运营 `month_out`、前端类型与筛选项同步补上（`expire` 计入支出）。
- **交易表与表格交互（#226 9ecfc67 / #227 26d448b / #228 6673389 / #229 d1faab6 / #230 2be0d54 / #231 dfa7318 / #232 e7bfc6f）** — ① 列数（`<thead>` / 空行 colspan / 错误行 colspan / 行模板 `<td>`）在四处手写、失配无声（colspan 短了只是画窄），新增门禁；② 行内按钮按**筛选后**的下标标记、处理器却按缓存记录取数 ⇒ 搜索后操作错行；③ 管理员模型表单的提交按钮被**嵌套 `withLoading`** 吃掉（外层守卫直接返回，请求从不发出）；④ CSV 导出的时间列与单元格不同口径（一处本地精确时间、一处库内 UTC 串，时区与精度两张脸）；⑤ 交易类型筛选曾有两套控件两份状态（列筛选一出值，顶部 tab 即成死控件、tab 高亮是说谎的指示器）；⑥ 「模型」/「Key」列的无值兜底显示**本地化类型名**，而这两列是服务端筛选且服务端没有语言包 ⇒ 按屏幕上的文案筛 0 行（改为语言中性占位符 `—`，服务端两列 LIKE 搜同一文案）；⑦ 列筛选曾服务端 (`tx_where`) 施加后**本地再筛一遍** ⇒ 输入 `"deepseek "` 表格空而「共 N 条」与汇总卡仍有数、输入 `%` 服务端全中而本地 0 行、CSV 导出 0 行（改为声明 `serverFilter: true`、三个消费者同变 pass-through）。
- **i18n 与用户可见文案（#161 b4b3fbd / #162 d8031b9 / #163 4bcddca / #164 d8724c2 / #165 37d5b1c / #166 c14a590 / #167 8e40d79 / #168 8cff75c / #169 edd21a7 / #177 92946b7 / #182 75aa3b3 / #183 6f8b599 / #184 75979ad / #200 3d9abcd / #224 4907a1c）** — ① 卡模式（`@media max-width:560px`，thead 隐藏、`td::before` 打印 `data-label`）的 53 处标签全是中文硬编码、英文模式仍显示中文（实测 27 个），改为渲染时持语言键（复用已有列头键，未发明新词）；② 后端错误到英文的映射表按**首个**子串命中返回，两条更具体的被通用词遮蔽（`验证码不存在或已过期` / `验证码错误次数过多` 永不可达）⇒ 英文下显示错误文案，改为最长匹配；③ `aria-label` 与搜索清除提示、英文文案里的代码标识与货币声明（英文管理页表头曾断言货币）；④ 侧栏每个导航项都标了数字快捷键，而 keydown 守卫把范围硬编码为 `1`–`7`，`ops` 加入后第 8 项失效；⑤ `ui/js/api.js` 是请求咽喉、也是唯一自建错误文案却直接用 CJK 字面量的地方（门禁此前只看 3 个文件，`api.js` 在射程外）；⑥ 交易列筛选状态存的是**本地化串**、比较时又按当前语言重新本地化 ⇒ 切语言后表被清空，改为存 `{value,label}`；⑦ 两条指向 2026-08-22 文档扁平化时已删除材料的死指针、两条陈旧/虚假的静态 `data-i18n` 兜底、一处已无数据来源的「方案备注」提示。
- **接口约定从散文搬进门禁（#170 fcff194 / #171 0de4bde / #172 fa67974 / #173 b7becb5 / #225 669d03f / #226 9ecfc67 / #236 83ecf4e / #237 bc4b111）** — 语言包不变量、插值占位符、`ui/js/data.js` 兜底目录 vs 唯一真源 `config/config.example.toml`（含本次改名后的重新对齐）、表格列数与活状态行形状、月聚合查询计划（`perf_gate`：日期函数再包住时间列即失败）、主密钥默认值。同时修掉一条会因 CI 机器负载而非产品缺陷失败的探针（用**最大值**统计量判「阻塞期间健康检查仍被服务」，改为按提示样本判定）。
- **文档与代码对齐（#178 3b1c0b0 / #179 a366162 / #180 d2c7431 / #181 9db4872 / #198 81df12d / #203 cabe4c6 / #216 aeb56c8 / #221 425c1b7）** — 去掉已过时的「跨协议流式 → 400」声明、`gift.rs` 模块头里虚构的 `dashboard` 触发者、过期的门禁计数注释、硬编码的模式/事务类型/API 清单与跨文件行号引用。
- **运营与共享视图（#174 7d9b56e / #175 940850e / #176 4830248）** — `/api/ops/runtime` 新增 `version`（与 `/healthz` 同源）与 uptime，界面可直接确认线上构建版本（此前必须手 curl）；运营页把早就返回却从未渲染的 `total_txs` 卡片补上；共享页补「本月新增」第四张卡（后端 `sharing_row` 增返 `created_at`）。
- **前端 live 容器清单改从 DOM 推导（1e64d98）** — `ui/js/app.js` 里两份手写花名册（`bindLiveRetry` 13 项、`KBD_TABLE_IDS`）都漏了后加的 `#model-body` ⇒ 它的「重试」按钮点了没反应（冒烟证据：渲染器自己声明的 `retryFn` 参数从未被使用）、行也无法用键盘激活。改为由 DOM 推导，新增表格自动纳入。
- **质量门禁**：`cargo test` 249/249（v0.7.22 时 148）+ `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` + `node --check` ×4 + i18n ZH/EN 键集精确相等（786×2）+ 新增 `perf_gate` / `deploy_gate` / `catalog_gate` / `table_gate` 四个纯测试模块（零新依赖）。

## v0.7.22 (2026-09-11)

- **补齐全局 reset 层并让表单/按钮几何对齐原型（rant 2026-09-11T22:01:43，PR #159 f46882c）** — 纯前端改动（`ui/` 内，后端契约与部署结构不变，数据库仍留 NAS、不启用 WAL）：
  - **整层全局 reset（根因）**：原型 `docs/prototype/aitokenpool-console.html` 第 53–72 行的全局 reset 层在整站 UI 重设计迁移时**整层漏掉**，导致所有**未带 class 的表单控件**直接裸露浏览器默认外观 —— 这是登录页 / 共享管理 / 设置页与原型视觉脱节的单一根因。本次补齐 `button { font: inherit; cursor: pointer; border: 0; background: none; color: inherit }`、`input, select, textarea { font: inherit }`、`a { color: inherit; text-decoration: none }`、`img, svg { display: block; max-width: 100% }`、`p { text-wrap: pretty }`、`h1, h2, h3 { text-wrap: balance }` 等条目（均为元素选择器，权重低于既有 `.btn` / `.input` 类选择器）。原型那行 `:focus-visible` 未重复：实现早有等价规则，仅圆角沿用既有 `--radius-sm`（已在代码注释记录该决策）。
  - **补齐依赖 reset 的缺失组件类**：`.num`（此前只有 `.table .num`，表格外的 `.num` 丢失等宽与 tabular-nums 对齐）、`.row`（markup 里 6 处 `class="row"` 此前**完全没有规则**，按钮贴边且不换行）、`.divider`（原型类名，与历史别名 `.login-divider` 合并同规格）、独立 `.avatar`（此前仅 `.user-chip .avatar` 后代选择器）。
  - **几何与原型对齐**：`.btn`（min-height 40px / `padding: 0 16px` / 13.5px / inline-flex）、`.btn-sm`（32px / `0 11px`）、`.input`（`padding: 10px 12px` / 13.5px）、`.auth-tabs button` 显式写出 `border: 0` + `background: none` —— 消除未选中 tab 的浏览器原生 `2px outset` 立体边框，输入框高度 25px → 44px。
  - **12 个裸 input 补 `class="input"`**：登录 / 注册 / 验证码 / 找回密码四族（与全站其余 47 个已有 class 的 input 一致）。
  - **cache-bust** `?v=20260911-4` → `?v=20260911-5`（5 处引用）。
  - **实测证据**（headless computed-style 探针，同视口 1912×836，9 个视图）：浏览器默认样式控件 **108 → 0**、原生 3D（inset/outset）边框 **0**、`.row` 按钮行 18 组 0 异常；identity-keyed A/B 对 310 个元素 × 10 个 computed 字段比对显示 reset 的 `img,svg{display:block}` **未改变任何可见图标尺寸**、`border-radius` 0 处变化，39 处几何变化全为「向原型靠拢」的预期修复。
  - **质量门禁**：`cargo test` 148/148 + `cargo fmt --check` + `cargo clippy` + `node --check` ×4 + i18n ZH/EN 键集精确相等（762×2）。

## v0.7.21 (2026-09-11)

- **全站 UI 按新原型完整重设计（rant 2026-09-11T16:23:43，PR #152 → #157）** — 纯前端改动（后端契约不变），6 片独立可部署提交：
  - **设计 token 层（#152 bafd223）**：`:root` 与浅色主题全面迁移到 OKLCH 色彩系统（`--bg` / `--surface` / `--fg` / `--muted` / `--border` / `--accent` 等），26 个 token 与原型逐字一致；旧 token 名保留为别名，视觉零跳变；原型基线入库 `docs/prototype/aitokenpool-console.html`。
  - **登录页与侧边栏（#153 e33bf2e）**：登录页改左右分栏（左侧品牌叙事 + 三行端点卡片含 OpenAI Responses，右侧表单 + 登录/注册 tab，复用既有 `showAuthForm` 故验证码/找回密码全保留）；侧边栏重构为 brand → 分组导航 → 底部用户 chip（头像/主题切换/退出），游客态同样可达。
  - **共享组件层（#154 75dcb2c）**：补齐此前缺失的 `.btn-secondary` / `.btn-sm`，迁移 stat-card / pill / tag / trend / bar-list / toolbar / search / empty 等组件（全部使用 OKLCH token，明暗主题免覆盖），原型组件覆盖 48/48。
  - **8 视图全部对齐原型**：仪表盘 + 模型市场 + 共享管理（#155 7e094e0）、钱包 + 交易记录（#156 41bb6d1）、设置 + 管理 + 运营（#157 09e4121）。
  - **保留的强功能**：i18n 中英双语（键集 762×2，精确等集）、交易页列筛选整行（焦点保持 + IME 保护）、24h + 自定义时间段、后端真分页、tokenBrk 换行、CSV 导出、withdraw/gift 类型。
  - **新增后端支持**：`/api/ops/runtime` 追加 `today_hours`（今日按小时调用量，0–23 全量补零）与 `key_health`（按厂商聚合上游 key 健康度）。
  - **质量门禁**：`cargo test` 148/148 + `cargo fmt --check` + `cargo clippy` + `node --check` ×4 + i18n ZH/EN 键集精确相等 + headless 明暗双主题 DOM 标记与几何探针校验。

## v0.7.20 (2026-08-25)

- **交易页性能修复（rant 2026-08-25T12:02:13，PR #150）** — dev 库在 NFS 导致的页面慢（浏览器实测 /api/transactions 1.7s、刷新 ~5s）三层修复：① `db.rs open()` 设 `PRAGMA cache_size=-65536`（64MB）+ `mmap_size=67108864`，整库常驻进程内存，NFS 只首读一次（dev 实测 COUNT 2.39s → 0.40s）；② v12 迁移为 transactions 建 4 个索引 `(user_id)` / `(user_id, id DESC)` / `(user_id, time)` / `(user_id, type)`，消除 summary/COUNT/list 全表扫描；③ 前端 `loadTransactions` 用 `Promise.all` 并行拉列表与趋势图，去掉一次串行 ~0.9s 等待。不启用 WAL（网络文件系统不支持）、数据库仍留在 NAS。

## v0.7.19 (2026-08-25)

- **筛选输入不再丢焦点（rant 2026-08-25T11:15:16，PR #148）** — 修复 v0.7.18 服务端列筛选回归：每输入一个字母触发刷新并失去焦点。方案（宿主确认 B）：表头筛选行改为**只渲染一次、重建时保持存活**——buildDataTable 仅重建 tbody 与分页器，筛选输入框 DOM 永不销毁，焦点天然保留；聚焦时不做 fallback 恢复。

## v0.7.18 (2026-08-25)

- **交易列筛选改服务端全量（rant 2026-08-25T10:33:26，PR #146）** — 修复 v0.7.15 后端翻页引入的回归：表格内部列筛选（用户/模型/Key/状态/点数区间等）此前只过滤当前加载页（几十条），全量几千条不在范围内。现改为：后端 `/api/transactions` 支持列筛选参数（model/user_name/key_name LIKE、status 精确、pts_min/pts_max 区间，与 type/时间段叠加），前端筛选变化时重新向后端拉全量子集（page 重置 1）；summary/trend 同步叠加列筛选口径，各区域结果一致。

## v0.7.17 (2026-08-24)

- **趋势图 1:1 渲染修复（rant 2026-08-24T14:29:57，PR #144）** — 根因：SVG viewBox 640 在宽容器被等比放大 ~2.4x（线 1.4px 视觉 ≈3.4px、字 8px ≈19px，v0.7.16 调细"看不出来"即被放大吞掉）。修复：viewBox 宽动态 = 容器宽（1 viewBox 单位 ≈ 1 物理像素），线宽/字号按 CSS 值真实呈现；窄屏保持等比缩小不变形；窗口 resize 防抖重渲染。

## v0.7.16 (2026-08-24)

- **趋势图视觉比例精修（rant 2026-08-24T13:31:02，PR #142）** — 交易页"点数趋势"图更精致：折线 stroke-width 1.8→1.4、坐标轴/时间标签字号 9px→8px、标题 12px→11px、图例 11px→10px、指标切换按钮 12px→11px 并收紧间距；仅调视觉比例，渐变面积/平滑曲线/指标切换/悬停 tooltip 功能不变

## v0.7.15 (2026-08-24)

- **趋势图渐变修复（rant 2026-08-24T12:32:18，PR #138）** — 交易趋势图填充面积不再显示黑色实心：`<stop>` 的 stop-color/stop-opacity 改为内联属性（复用仪表盘 sparkline 写法）+ 每实例唯一渐变 id，删除不可靠的 CSS class 方案；4 个指标切换均显示对应色 0.35→0 渐变面积
- **交易列表时间精确显示（rant 2026-08-24T12:38:44，PR #139）** — 交易记录列表时间列由相对时间（"3 小时前"）改为精确本地时间（`2026-08-24 12:36:12`），相对时间移入悬停提示
- **API Key 最近使用真实数据（rant 2026-08-24T12:41:25，PR #140）** — 设置页 API Key「最近使用」不再硬编码"从未"：api_keys 表新增 last_used 字段，网关计费时更新，list 接口返回真实时间；未使用过的 key 才显示"从未"

## v0.7.14 (2026-08-24)

- **交易记录页真分页（rant 2026-08-24T10:51:57，PR #135）** — 修复假分页：改为后端翻页，`/api/transactions` 支持 `page`/`pageSize`，前端按表格分页参数拉取并显示真实总数（7320 条全部可达），紧凑省略号分页器 + 翻页滚动回顶部
- **点数趋势图 sparkline 化（rant 2026-08-24T10:51:57，PR #136）** — 参考仪表盘「本月点数变化」样式重做：渐变面积填充、连续平滑曲线（去数据点断口）、极简坐标（去虚线网格）、teal 主题配色；**图高减半**（viewBox 190→85），保留指标切换 + 悬停 tooltip

## v0.7.13 (2026-08-24)

- **交易趋势图优化（rant 2026-08-23T16:17:18，PR #133）** —
  - 美观度提升：平滑曲线、渐变面积填充、主题色系配色、自适应坐标轴刻度、悬停数据提示（tooltip）、精致图例
  - **指标可切换**：趋势图上方增加指标选择器（消费点数 / 收入点数 / 净变化 / Token 用量），默认只展示「消费点数」变化

## v0.7.12 (2026-08-23)

- **交易页 UI 改进（rant 2026-08-23T16:01:07）** —
  - 移除交易列表内 time 列的内部筛选（外部时间段筛选已覆盖，两套并存冗余且易混淆）
  - 列表上方新增**趋势图**（手写 SVG 折线图，无外部依赖）：新增 `GET /api/transactions/trend`，按时间桶（hour/day/week）聚合收入/支出点数，口径与 summary 一致；趋势图跟随 tab + 时间段筛选联动
  - **修复统计指标未联动表格内部筛选**：内部筛选（类型/模型/用户/Key 等）变化后，汇总条基于筛选后可见行本地加总（含 Token 总/输入/缓存/输出四指标），无筛选时仍用后端全量 SQL 聚合
  - **修复筛选输入框逐字符刷新**：`.th-filter` 的 input 事件由立即重建改为 300ms 防抖，刷新后恢复焦点并置光标到末尾（此前输入 "sh" 会因中间刷新变成 "hs"）

## v0.7.11 (2026-08-23)

- **P0 cache-billing fix (passthrough path)** — PR #128 (v0.7.10) only patched the cross-protocol SSE conversion path; the same-protocol passthrough path (`UsageCapture::finish()` in `src/gateway.rs`, used when client and upstream speak the same protocol, e.g. openai→openai) still passed the **full** `prompt_tokens` as input (including cache-hit tokens) and only recognized OpenAI's `prompt_tokens_details.cached_tokens` spelling — so DeepSeek responses double-counted cached tokens into the total (~2× tokens, per-call uncached showed 241,776 instead of 112). `finish()` now extracts cached via all three spellings (DeepSeek native `prompt_cache_hit_tokens` → OpenAI `prompt_tokens_details.cached_tokens` → Anthropic `cache_read_input_tokens`) and returns `(input − cached).max(0.0)` disjoint, matching the `sse.rs` logic; tests extended with cached cases for all three spellings (rant 2026-08-23T14:05:02, PR #130)

## v0.7.10 (2026-08-23)

- ~~**Request body limit raised to 70MB**~~ — ⚠️ **correction (v0.7.26, #267): this never took effect.** `RequestBodyLimitLayer` only wraps the outer stream; it never writes the request extension that axum's extractors read, so the effective limit stayed at axum's 2 MB default (direct measurement: a 2.02 MB body → `413 Failed to buffer the request body`). The limit is applied where axum actually reads it in v0.7.26. (rant 2026-08-22T23:20:00)
- **P0 cache-billing fix** — DeepSeek's native top-level `prompt_cache_hit_tokens` was silently dropped (cached=0 → cache hits billed at full miss price, ~30x overcharge); all three spellings (DeepSeek `prompt_cache_hit_tokens` / OpenAI `prompt_tokens_details.cached_tokens` / Anthropic `cache_read_input_tokens`) are now extracted with DeepSeek priority, and all 6 `record_usage` sites disjoint input (`prompt_tokens − cached`) so cached tokens are never double-billed; downstream `input_tokens` forwarding is disjoint too (rant 2026-08-23T08:20:38)

## v0.7.9 (2026-08-22)

- **API Key 名称持久化** — `POST /api/api-keys` now stores the submitted name (was hardcoded empty); new `PATCH /api/api-keys/:id` renames a key (owner-only); settings-page rename now calls the API and reloads instead of faking it in memory (rant 2026-08-22T17:21:39)
- **Transaction table columns** — added a 用户 (user) column (JOIN users) and the api-key name column (api_keys.name via new `api_key_id` on transactions, migration v11 + settle writes); CSV export and i18n updated to match (rant 2026-08-22T17:21:39)

## v0.7.8 (2026-08-22)

- **Transaction-table filter row fix** — filter row no longer stretches to 236px (constrained to ~48px via fixed-height/th-top-aligned filter inputs); the four token columns (input/cached/output/tokens) drop their number-range filters, keeping sort and right-align (rants 2026-08-22T10:11:48/10:12:57)

## v0.7.7 (2026-08-22)

- **Transaction-page UE/UI** — token columns use K/M abbreviation with exact-value hover tooltip; summary bar values right-aligned; Key column shows a transaction-type label for non-keyed rows (topup/gift/withdraw) instead of a bare dash (rant 2026-08-22T08:58:54)
- **Docs refresh** — README zh/en language switcher links, "Powered by EMRG" section, and concise current-state docs (architecture / plan-api-matrix / user-stories rewritten to describe present + roadmap instead of history) (rants 2026-08-22T07:46:46/07:49:58/07:51:36)

## v0.7.6 (2026-08-22)

- **Docker publishing is tag-driven** — `docker-publish.yml` now builds GHCR images only on version tags (`v*`), plus `workflow_dispatch` manual trigger; `latest` follows the newest release tag (rant 2026-08-22T07:14:15)
- **Transaction table column overhaul** — 「模型 / Key」split into two readable columns (`model` + `key_label` from the `keys` table: note > provider/plan), and token usage split into four columns (input non-cache / input cache / output / total) (rants 2026-08-22T06:36:54/06:37:50/06:39:04)
- **Transaction summary fixes** — income whitelist (earn/topup/gift) positive, consume negative; daily gift now writes a `transactions` row; dashboard net/series treats topup as positive (rants 2026-08-22T00:04:21/00:07:08/06:34:37)
- **Dynamic user nickname** — sidebar chip + settings form show the real nickname (rant 2026-08-22T00:01:52)
- **SMTP send retry** — 3 attempts × 2s with fresh transports; 502 with a clear error when verification-code sending fails (rant 2026-08-21T23:52:17)

## v0.7.5 (2026-08-22)

- **New model** `deepseek-v4-flash-vision-exp` (provider=deepseek, same pricing as deepseek-v4-flash: 1.5/0.05/4.5, peak 3.0/0.1/9.0, context 1M, vision=true)

## v0.7.4 (2026-08-21)

- **Per-call token usage breakdown** — settle now writes the split usage (input / cache-hit / output) into `transactions` and `usage_records` (idempotent migration v10; input = total − cache − output, not double-stored)
- `GET /api/transactions` returns `input_tokens / cached_tokens / output_tokens` (older records default to 0)
- Transaction table shows a sub-line with the token breakdown (bilingual i18n, cache/output color-coded + tooltip)

## v0.7.3 (2026-08-20)

- **DeepSeek peak-hour pricing** — optional `peak_input_per_m / peak_output_per_m / peak_cache_hit_input_per_m` fields on the models table and config `[[models]]` (default 0 = peak pricing disabled)
- Peak hours judged in Beijing time (09:00–12:00, 14:00–18:00, fixed Asia/Shanghai, independent of server timezone)
- DeepSeek official peak prices written into the example config; market shows a "peak ×N" badge + detail panel; admin model form supports peak prices; migration v9

## v0.7.2 (2026-08-20)

- **Seed sync-delete** — `seed_models` now also deletes rows for models removed from config (config `[[models]]` fully authoritative; no more ghost market models)
- New test `seed_models_deletes_config_removed_rows`

## v0.7.1 (2026-08-20)

- **Simplified model config** — all model info (provider / price / context / vision…) defined in config.toml `[[models]]` (single source of truth), upserted into the DB on startup
- Removed `data/models.example.json` and the `price_overrides` double-layer mechanism; 10 models moved into config with DeepSeek official CNY prices; `data.js` visitor fallback prices aligned

## v0.7.0 (2026-08-20)

- **Separate billing for cache hits and misses** — usage parsing splits cached tokens (OpenAI `prompt_tokens_details.cached_tokens` / Anthropic `cache_read_input_tokens` / Responses `input_tokens_details.cached_tokens`)
- Billing = miss × input_per_m + hit × cache_hit_input_per_m (default 0 = free cache hits)
- `usage_records.cached_tokens` column (migration v8); DeepSeek official CNY pricing; admin model form gets the cache-hit input price; SSE (converted + passthrough) split the same way

## v0.6.7 (2026-08-19)

- **Logging system** — log4rs replaces env_logger: logs written to `<data-dir>/logs/aitokenpool.log` + stdout
- Size-based rolling (`[log].max_file_size`, default 10MB) with auto-pruning (`[log].max_backups`, default 7); `[log]` config section (dir / level / file_pattern)

## v0.6.6 (2026-08-19)

- **Unified data directory** — `ATP_DATA_DIR` (default `./data`; `--data-dir` > env > default) holds config.toml (auto-copied from the example on first start) + aitokenpool.db + logs/
- DB path always derived from the data dir (config `db_path` ignored); Docker single volume `./atp-data:/data`

## v0.6.5 (2026-08-19)

- **Timezone fix** — all backend JSON time fields normalized to UTC ISO with `Z`; frontend `timeAgo()` parses UTC, dashboard sparkline buckets by local day, absolute-time titles localized

## v0.6.4 (2026-08-19)

- **Admin model-info CRUD** — models table gains context_length / max_output / vision / cache_hit_input_per_m (migration v7); `GET|POST /api/admin/models` + `PATCH|DELETE /api/admin/models/:id` (admin-only, 409 on unique conflict, deleted models billed at 0)
- Management tab in the admin view (search / add / edit / delete); `GET /api/models` exposes the new fields

## v0.6.3 (2026-08-19)

- **Configurable public URL** — `[server].public_url` (default `http://localhost:8080`) + `GET /api/config`; the frontend builds gateway endpoints from it with a same-origin fallback

## v0.6.2 (2026-08-19)

- **Self-registration + email verification** — `POST /api/auth/register` / `verify` / `resend-code`; 6-digit code (10 min validity, 5 wrong attempts invalidate, 60 s resend rate limit); unverified emails can't log in (403)
- Registration form + verification page on the login page (bilingual); SMTP delivery via the `[mail]` config (dev mode prints the code to logs/response when unset)

## v0.6.1 (2026-08-19)

- **First-start initial admin** — an empty DB creates `admin@aitokenpool.local` with a random 16-character password printed to the startup log (first run only) + a zero-balance quota account; idempotent
- `POST /api/auth/change-password` endpoint (old-password check + argon2 update)

## v0.6.0 (2026-08-19)

- **Remove all demo seed data** — first deploy is a clean empty DB (schema only); no demo/admin/ops accounts, balances, or placeholder keys
- Tests use a `#[cfg(test)]`-only `seed_test_users` helper; UI login/settings pages drop the demo-account prefill and hints

## v0.5.2 (2026-08-19)

- **Bugfix: time-sensitive tests** — tests with hardcoded dates switched to SQLite dynamic dates (`datetime('now')` / `strftime('%Y-%m-%d 23:59:59', 'now')`); no more periodic failures when the day rolls over

## v0.5.1 (2026-08-18)

- **Docker publish** — GitHub Actions workflow builds and pushes the GHCR image `ghcr.io/argszero/aitokenpool` on `main` push / `v*` tag (buildx + gha cache)

## v0.5.0 (2026-08-18)

- **P3-B: streaming SSE cross-protocol conversion** — `stream:true` requests from any protocol (openai/anthropic/responses) forward to any protocol upstream with real-time event conversion (openai delta ↔ anthropic content_block_delta ↔ responses output_text.delta, incl. tool-call / thinking deltas and in-stream usage metering; responses→anthropic streaming deferred)

## v0.4.1 (2026-08-18)

- **P3-A follow-up: `GET /v1/models`** — OpenAI-compatible model list (optional auth; Bearer adds `available_keys`; `/models` alias)

## v0.4.0 (2026-08-18)

- **P3-A: three-protocol gateway conversion** — OpenAI Chat / OpenAI Responses / Anthropic: any endpoint can call plans exposing other protocols (auto-convert, zero-loss passthrough on the same protocol) + new `/v1/responses` endpoint

## v0.3.4 (2026-08-18)

- **Integration fixes** — `GET /api/plans` single source of truth (frontend form wired to the API), `data.js` PLANS aligned to the 12 plan ids, idempotent seed placeholder keys, master-key documentation

## v0.3.3 (2026-08-18)

- **P2-C: departments / raise requests / usage / operator** — departments CRUD + member re-assignment, raise-request approval flow, usage reports (users/models/departments aggregations), operator view (runtime/credits/users); schema v4

## v0.3.2 (2026-08-18)

- **P2-B: frontend wired to real APIs** — market / sharing / transactions / dashboard / API-key management all backed by the real backend; org/ops views keep mock placeholders

## v0.3.1 (2026-08-18)

- **P2-A: frontend integration** — backend statically serves `ui/`, API client layer, login/session integration, admin view gated by role; `GET /api/me`

## v0.3.0 (2026-08-18)

- **P1: point rules refined** — new-user daily gift (10-day window, valid same day), deduct earliest-expiring gift first then permanent, admin top-up API (role=admin); gift_balance/gift_grants migration v3

## v0.2.2 (2026-08-18)

- **P0-C: SSE streaming + key encryption + sharing APIs** — streaming with usage metered at the stream tail (no charge on disconnect), upstream keys AES-256-GCM encrypted, sharing / wallet / transaction APIs

## v0.2.1 (2026-08-18)

- **P0-B: gateway forwarding + failover + metering** — OpenAI Chat Completions + Anthropic Messages forwarding, sticky routing with health cooldown (3-switch cap), metering ledger (point calculation, 90/10 split, transactional settle)

## v0.2.0 (2026-08-18)

- **P0-A: backend skeleton** — axum server, TOML config (`Config::validate`: points_per_unit>0 / plan→provider exists / endpoints≥1 / protocol enum), SQLite data layer (idempotent migrations, production empty DB seeds nothing), auth (argon2 + Bearer API key), API-key endpoints
