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
use std::collections::{BTreeMap, BTreeSet};

/// 前端源码在**编译期**读入：测试不依赖工作目录与文件系统布局。
const APP_JS: &str = include_str!("../ui/js/app.js");

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
        assert_eq!(
            writers,
            holders,
            "写 `Live.{TX_SLOT}` 的函数与写 `{TX_EVIDENCE}*` 的函数不是同一批 —— \
             缓存内容与守卫手上的证据脱钩。实测证据写点：{:?}",
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
}
