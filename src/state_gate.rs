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

use std::collections::BTreeSet;

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
}
