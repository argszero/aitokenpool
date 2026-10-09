//! 交易明细保留门禁（rant `2026-10-09T12:28:58` 验收项 1 的**删除切片**）：明细可以被删，
//! 但**不许删到查询答不出来**。
//!
//! 前两切片把「已折叠的明细」变成了可删的：`tx_rollup` 把它们折成可加汇总行（#351），
//! 读侧改从读模型 `tx_facts = 汇总 ∪ 未折叠明细` 取数（#352）。本切片真的删。
//! 删除只受一条约束 —— `transactions` 的**分页列表**要的是逐行身份（`id`/`counterpart`），
//! 汇总行没有它，所以列表**永远**只能从明细表答。于是：
//!
//! * **保留窗口必须盖得住列表自己提供的每一个预设区间**。少一天都不行：窗口是
//!   `time < datetime('now', '-N days')`，列表最深预设若比 `N` 还深，用户点那个预设会
//!   看到「共 N 条」和实际能翻的行数一起缩水，而**没有任何提示**（`total` 与行同源，
//!   所以也不会自相矛盾 —— 它会安静地少答一段历史）。
//!   判据两侧都**派生**：预设深度从 `ui/js/app.js::txRangeParams` 读，窗口从
//!   `config/config.example.toml` 的 `[rollup] retain_days` 读。
//! * **盖不住的那类区间 ——「无界」的 —— 必须把边界说给页面**。`#tx-range` 里有两个区间
//!   没有深度可言（「全部时间」、自定义起点）：它们能整段落在保留窗口之外，静态门禁**表达不了**
//!   那种深度。判据因此换成「要么不提供、要么披露」：服务端随响应发布保留边界
//!   （`tx_rollup` 的 `DETAIL_SINCE_FIELD`，与删除同一算式），前端在**缺口**上消费它
//!   （`summary.entries` 与 `total` 的差）。**披露必须长在缺口上，不许长在「列表空」上**：
//!   明细还在（`total > 0`）时同样会少列，而「只在一条明细都没有时才解释」正好让说明只在
//!   **最不需要它**的场合发射（R104）。两侧同样**全派生**：选项名册从 `ui/index.html` 的
//!   `#tx-range` 读（哪些无界由「该分支有没有 `MS(…)` 深度」判定），字段名从 `tx_rollup.rs`
//!   的常量读。
//! * **删除的边界是「已折叠 ∧ 已归档」**。折叠水位（`transactions_rollup.up_to_id`）与
//!   归档水位（`[archive]` 跟到哪一行）分别保证「汇总里有这份数据」与「原件还在文件里」；
//!   少了任一条，删掉的行就**真的没了**。这一条是形状：`delete_folded` 必须把两界都取上界。
//!
//! # 射程（如实）
//!
//! * **词法的**：它证「源码里这两处形状成立」，**不证**删除真的只删对、也**不证**屏幕上
//!   的数对 —— 行为由 `tx_rollup.rs::tests::delete_folded_*` 四条（窗口/水位/批量/读模型等价）
//!   与 `main.rs` 的调度测试兜。
//! * **R4 只证「说给页面了」，不证「说得对」**：它看见的是「发布 + 一个以缺口为准、不带空态
//!   闸门、且被调用过的生产函数」这几个形状，不证那句文案在每一种状态下成立，也**不证它被写
//!   进了哪个元素** —— 一个只把结果喂给空态插槽的改法仍会通过（它至少不再以「列表空」为准）。
//!   屏幕上**看得见**由仓外 jsdom 探针按三态（有缺口 / 无缺口 / 零行）兜。它也不区分
//!   披露的**手段** —— 换一种同样到达页面的写法（例如服务端直接算好两个数之差）仍算通过，
//!   只要边界这个事实面还在。
//! * **不写快照**：两侧期望值都从制品推导（预设取自 JS，窗口取自示例配置），钉死数字
//!   只会变成第二个需要同步的载体。
//! * **阳性对照**：`the_scanner_sees_both_sides` 断言扫描器真的读到了预设深度与窗口 ——
//!   否则「零违规」与「扫描器是瞎的」在读数上无法区分（坑 #814）。

use crate::body_limit_gate::source_files;
use crate::deploy_gate::code_mask;
// JS 用专用的掩码机（`code_mask` 是给 Rust 写的，会在含多字节字符的**代码**位置上切字节）；
// 同一台机器 `js_gate` 已经在用（`tx_facts_gate` 用 Rust 那台是同一个先例）。
use crate::js_gate::mask_js;

/// 提供交易列表那套预设区间的函数（深度从这里派生）。
const RANGE_FN: &str = "function txRangeParams(";

/// 预设区间的表达形态：`MS(<整数乘积>)`，`MS(h)` 就是 `h` 小时。
const MS: &str = "MS(";

/// 保留窗口所在文件的键。
const RETAIN_KEY: &str = "retain_days";

/// 示例配置里窗口所在的段。
const RETAIN_SECTION: &str = "[rollup]";

/// 阈值所在文件（相对 `src/`）。
const ROLLUP_FILE: &str = "tx_rollup.rs";

/// 删除函数名（边界形状从这里读）。
const DELETE_FN: &str = "pub fn delete_folded(";

/// 折叠水位函数（「汇总里有这份数据」）。
const FOLD_WATERMARK: &str = "watermark(conn)";

/// 归档水位入参（「原件还在文件里」）。
const ARCHIVE_BOUND: &str = "archive_watermark";

/// `#tx-range` 那个 select 的标记 —— 选项名册从这里派生（R4 的「提供了哪些区间」）。
const RANGE_SELECT: &str = "id=\"tx-range\"";

/// 选项值的形态（`<option value="24h">`）。
const OPTION_VALUE: &str = "<option value=\"";

/// 明细保留边界字段名的**唯一拼写点**（`tx_rollup.rs` 的 `pub const`）。
const BOUNDARY_CONST: &str = "pub const DETAIL_SINCE_FIELD: &str = \"";

/// 服务端发布边界的地方（R4 的发布侧）。
const PUBLISH_FILE: &str = "wallet.rs";

/// 前端消费边界的地方（R4 的消费侧）—— `ui/js/app.js`，即 `oracle` 里那份语料。
const CONSUME_FILE: &str = "app.js";

/// 披露文案的调用前缀 —— 边界必须与它**同处一个函数体**才算「有人在生产披露」。
const DISCLOSURE_CALL: &str = "T(\"tx.empty.";

/// 缺口两侧那两个数的标识符：账本条数（读模型口径）与明细行数。
///
/// 生产披露的函数体必须让它们**在同一行相遇** —— 「有缺口」是两者的**差**，不是「列表空不空」。
/// 只要求相遇，不钉死写法（减法 / 比较 / 都算）。
const ENTRY_IDENT: &str = "entries";
const ROW_IDENT: &str = "total";

/// 「列表非空就不解释」的闸门形态 —— R104 轴的原形状：披露只在**最不需要它**的零行态发射。
/// 生产披露的函数体**必须没有**它。与 `DISCLOSURE_CALL` 一样是个**词法拼写点**（射程见文件头）。
///
/// 判据只看**生产函数**的体。消费者按「这句话该在哪儿出现」挑插槽不算违规 —— 常驻的那句只在
/// 明细还有行时占位、零行态把同一句话交给表格自己的副标题（见 `ui/js/app.js` 的
/// `renderTxArchivedNote`），那是**位置**的选择，不是「要不要解释」的选择：两种状态都在解释，
/// 屏幕上也不会出现两遍。区别在于「按列表空不空决定**要不要说**」（R104，违规）与「按列表空不空
/// 决定这句话**落在哪个插槽**」（合规）；前者会把说明挡在最需要它的状态之外，后者不会。
const EMPTY_GUARD: &str = "total > 0";

/// 一次扫描的读数 —— 判决与它的证据一起产出。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Reading {
    /// `txRangeParams` 里读出的预设深度（天，去重升序）。
    presets_days: Vec<i64>,
    /// 读不懂的 `MS(…)` 表达式（应 0：读不懂就等于那条预设**没有判据**）。
    unreadable: Vec<String>,
    /// 示例配置里的保留窗口（天）；读不到 = `None`。
    retain_days: Option<i64>,
    /// 删除函数体里是否出现「只删明细表」。
    deletes_details_only: bool,
    /// 删除函数体里是否带上折叠水位。
    has_fold_bound: bool,
    /// 删除函数体里是否带上归档水位（**参数名出现**不算数）。
    has_archive_bound: bool,
    /// 折叠水位与归档水位是否落在**同一条语句**上 —— 「取两者的上界」这件事的形状。
    /// ⚠️ 只看名字出现过是不够的：签名里就有 `archive_watermark`（A/B 的 B 腿实测：
    /// 把它从 `min` 里摘掉，签名仍让「名字出现过」为真 ⇒ 假绿）。
    bounds_combined: bool,
    /// 删除函数体里是否真的有时间窗（不是把已折叠的全删光）。
    has_time_window: bool,
    /// `#tx-range` 提供的选项值（按出现顺序）—— R4 的「提供了哪些区间」。
    range_options: Vec<String>,
    /// 其中**无界**的：`txRangeParams` 里没有 `MS(…)` 深度可用（「全部时间」/ 自定义起点）。
    /// 点它们会越过明细边界，而静态门禁表达不了那种深度 ⇒ 只能要求披露。
    unbounded_options: Vec<String>,
    /// 明细保留边界字段名（从 `tx_rollup.rs` 的单一拼写点读出）；读不到 = `None`。
    boundary_field: Option<String>,
    /// 服务端真的发布了它（发布侧的源里引用了那个常量，而不是又写一遍字面量）。
    boundary_published: bool,
    /// 前端真的在生产披露它 —— 生产函数的形状（见 `Disclosure`）：以**缺口**为准、不带空态
    /// 闸门、且被调用过。
    disclosure: Disclosure,
}

impl Reading {
    /// R1：产品自己提供的每个预设区间都落在保留窗口内。
    fn r1(&self) -> bool {
        match (self.presets_days.last(), self.retain_days) {
            (Some(deepest), Some(window)) => *deepest <= window,
            _ => false,
        }
    }

    /// R2：删除只碰明细表、且两界（已折叠 ∧ 已归档）都在。
    fn r2(&self) -> bool {
        self.deletes_details_only && self.has_fold_bound && self.bounds_combined
    }

    /// R3：删除按时间窗分批，而不是「已折叠的全删」。
    fn r3(&self) -> bool {
        self.has_time_window
    }

    /// R4：盖不住的无界区间必须被披露（服务端发布 + 前端在**缺口**上生产说明）。
    ///
    /// 「无界区间一条都没有」是另一种**合法**解（把 `全部时间` / 自定义从控件里去掉），
    /// 那时规则自动成立 —— 它守的是「别让一种深度无界的区间静默地少答」，不是「必须提供它」。
    fn r4(&self) -> bool {
        self.unbounded_options.is_empty()
            || (self.boundary_published && self.disclosure.reaches_page())
    }

    fn blame(&self) -> String {
        format!(
            "r1={} r2={} r3={} r4={} · 预设 {:?} 天 · 窗口 {:?} 天 · 读不懂 {:?} · 只删明细 {} · 折叠界 {} · 归档界 {} · 两界同句 {} · 时间窗 {} · 区间选项 {:?} · 无界 {:?} · 边界字段 {:?} · 已发布 {} · 披露生产函数 {:?}（以缺口为准 {} · 空态闸门 {} · 被调用 {}）",
            self.r1(),
            self.r2(),
            self.r3(),
            self.r4(),
            self.presets_days,
            self.retain_days,
            self.unreadable,
            self.deletes_details_only,
            self.has_fold_bound,
            self.has_archive_bound,
            self.bounds_combined,
            self.has_time_window,
            self.range_options,
            self.unbounded_options,
            self.boundary_field,
            self.boundary_published,
            self.disclosure.name,
            self.disclosure.gap_derived,
            self.disclosure.gated_on_empty,
            self.disclosure.called,
        )
    }

    /// 全部门禁规则。
    fn all(&self) -> bool {
        self.r1() && self.r2() && self.r3() && self.r4()
    }
}

/// `txRangeParams` 函数体在源码里的**字节区间** `[open, end)`（含第一个 `{`，不含配平的 `}`）。
///
/// 掩码是**等长**的（`mask_js` 只把注释/字符串/正则的字节涂成空格）⇒ 在**掩码文本**上算出的
/// 下标对**原文**同样有效。于是函数体的**长度**可信，而**内容**要按问题挑文本：深度（`MS(…）`
/// 与结构（`else`）看掩码文本（字符串/注释已抹平），选项值（`txRange === "24h"`，那本身就是
/// 个**字符串字面量**）只能看原文 —— 它在掩码文本里已经成了空格。
/// 从第一个 `{` 起按花括号配平 —— 掩码后字符串/注释已抹平，深度可信。
fn range_fn_body(masked: &str) -> Option<(usize, usize)> {
    let start = masked.find(RANGE_FN)?;
    let body = &masked[start..];
    let open = body.find('{')?;
    let mut depth = 0i32;
    let mut end = body.len();
    for (i, ch) in body.char_indices().skip(open) {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    Some((start + open, start + end))
}

/// 从 `txRangeParams` 的函数体里读全部 `MS(<整数乘积>)` 的深度（换算成天）。
///
/// 只认**整数的乘式**（`24`、`24 * 7`、`24 * 30`）：算术一旦含变量或小数，静态门禁就
/// 读不出深度 —— 那时把它记进 `unreadable`，让判决**响亮地**失败，而不是默认通过。
fn presets_days(body: &str) -> (Vec<i64>, Vec<String>) {
    let mut days = Vec::new();
    let mut unreadable = Vec::new();
    let mut rest = body;
    while let Some(i) = rest.find(MS) {
        let after = &rest[i + MS.len()..];
        let Some(close) = after.find(')') else {
            unreadable.push(rest[i..].to_string());
            break;
        };
        let expr = &after[..close];
        match hours_of(expr) {
            Some(h) => days.push(h / 24),
            None => unreadable.push(format!("{MS}{expr}")),
        }
        rest = &after[close..];
    }
    days.sort_unstable();
    days.dedup();
    (days, unreadable)
}

/// `24` / `24 * 7` / `24*30` → 小时数。含变量、小数或除法则读不懂（`None`）。
fn hours_of(expr: &str) -> Option<i64> {
    let mut acc: Option<i64> = None;
    for part in expr.split('*') {
        let n: i64 = part.trim().parse().ok()?;
        acc = Some(acc.map_or(n, |a| a * n));
    }
    acc
}

/// `#tx-range` 的选项值（`ui/index.html` 里那个 select 块，按出现顺序）。
///
/// 名册从**标记**派生，不写死列表：控件加了区间，这里自动多一条。
fn range_options(html: &str) -> Vec<String> {
    let Some(i) = html.find(RANGE_SELECT) else {
        return Vec::new();
    };
    let tail = &html[i..];
    let end = tail.find("</select>").unwrap_or(tail.len());
    let block = &tail[..end];
    let mut vals = Vec::new();
    let mut rest = block;
    while let Some(j) = rest.find(OPTION_VALUE) {
        let after = &rest[j + OPTION_VALUE.len()..];
        match after.find('"') {
            Some(q) => {
                vals.push(after[..q].to_string());
                rest = &after[q..];
            }
            None => break,
        }
    }
    vals
}

/// 无界的选项：在 `txRangeParams` 的函数体里，它那条分支**没有** `MS(…)` 深度。
///
/// 三条形态都算无界，且都从源码读出来，不看注释：
/// * 有分支但分支体不含 `MS(`（自定义：起点由用户给，深度不可知）；
/// * 根本没有分支（「全部时间」：既没有 `start` 也没有 `end`）；
/// * 不在函数体里出现的选项值（与上一条同义，只是写法更散）。
///
/// ⚠️ 两个文本各司其职，缺一不可：**分支头**（`txRange === "<v>"`）里的选项值是个字符串
/// 字面量，掩码后被涂成空格，只能在 `orig` 里找；**分支体**（到下一个 `else` 之前）里判有没有
/// `MS(` 要看 `masked`（否则字符串/注释里出现的 `MS(` 会冒充深度）。掩码等长 ⇒ `orig` 里找到的
/// 偏移量对 `masked` 同样有效。
fn unbounded_options(orig: &str, masked: &str, options: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for v in options {
        let needle = format!("txRange === \"{v}\"");
        let Some(i) = orig.find(&needle) else {
            out.push(v.clone());
            continue;
        };
        let after = &masked[i + needle.len()..];
        let seg_end = after.find("else").unwrap_or(after.len());
        if !after[..seg_end].contains(MS) {
            out.push(v.clone());
        }
    }
    out
}

/// 从 `tx_rollup.rs` 读明细保留边界的**字段名**（单一拼写点的字面量）。
fn boundary_field(rollup: &str) -> Option<String> {
    let i = rollup.find(BOUNDARY_CONST)?;
    let after = &rollup[i + BOUNDARY_CONST.len()..];
    let q = after.find('"')?;
    Some(after[..q].to_string())
}

/// 生产「明细缺口」披露的那个函数，以及它的形状。
///
/// 判据全部落在**生产函数**身上，而不是「文件里出现过字段名」：那样它出现在任何一处
/// （注释里、与披露无关的助手函数里）都算消费。形状上要的是「**说缺口的那段代码**知道边界、
/// 且以缺口为准」。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Disclosure {
    /// 函数名 —— `None` ⇒ 没有任何函数在生产披露。
    name: Option<String>,
    /// 体里两个数**在同一行相遇**（拿它们的差当判据，而不是「列表空不空」）。
    gap_derived: bool,
    /// 体里带着「列表非空 ⇒ 不解释」的闸门 —— R104 轴的原形状。
    gated_on_empty: bool,
    /// 除定义行之外还被**调用**过 ⇒ 不是「定义了但没人调」的死代码（屏幕上不会有任何东西）。
    called: bool,
}

impl Disclosure {
    /// 披露真的到达页面。
    fn reaches_page(&self) -> bool {
        self.name.is_some() && self.gap_derived && !self.gated_on_empty && self.called
    }

    /// 四件事里哪几件不成立（`name` 缺失时后三件无意义，仍如实列出）。
    fn why_not(&self) -> String {
        format!(
            "生产函数 {:?} · 以缺口为准 {} · 空态闸门 {} · 被调用 {}",
            self.name, self.gap_derived, self.gated_on_empty, self.called
        )
    }
}

/// 走一遍 `js` 里的 `function` 定义，读出**生产披露**的那个函数（最内层的匹配体）及它的形状。
///
/// 与「零行态消费」那版判据的差别有两处，两处都是本轴所在：
/// * **取最内层的匹配体**。旧版取「第一个匹配的函数体」，而 `ui/js/app.js` 最外层是
///   `(function () { … })()` 那层 IIFE —— 它的「体」配平后就是**整个文件**，于是
///   「边界字段名与披露文案同处一个函数体」这条判据被模块包装器**恒真**满足
///   （文件里出现过就等于同处一体）。取「没有任何匹配体落在它之内」的那个，判据才真的
///   落在某个函数上。
/// * **不许**用「列表空不空」当闸门（`total > 0`）—— 那正是 R104 说的「披露只在最不需要
///   它的场合发射」。
///
/// 掩码等长（`mask_js` 只把注释/字符串/正则涂成空格）⇒ 函数体的**边界**在掩码文本上算，
/// 而**内容**按问题挑文本：字符串字面量（`T("tx.empty.…`）只能在原文里找；「两个数在同一行
/// 相遇」与闸门形态是**结构**，要在掩码文本里找，否则注释里逐字引用的代码会冒充判据。
fn disclosure(js: &str, field: &str) -> Disclosure {
    let masked = mask_js(js);
    // 每个 `function <名字>` 各自的体（按源码顺序）。刻意**不跳过**已被包住的那些：
    // 外层体的末尾就是文件末尾，跳过它就再也走不到里面。
    let mut found: Vec<(usize, usize, String)> = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = masked[from..].find("function ") {
        let abs = from + rel;
        from = abs + "function ".len();
        let after = &masked[from..];
        let name_len = after
            .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
            .unwrap_or(after.len());
        let name = after[..name_len].to_string();
        if name.is_empty() {
            continue; // 匿名（含那层 IIFE 的 `(function (`）：没有名字就无从谈「谁被调用」
        }
        let Some(rel_open) = masked[abs..].find('{') else {
            continue;
        };
        let open = abs + rel_open;
        let mut depth = 0i32;
        let mut end = masked.len();
        for (k, ch) in masked[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + k;
                        break;
                    }
                }
                _ => {}
            }
        }
        if masked[abs..end].contains(field) && js[abs..end].contains(DISCLOSURE_CALL) {
            found.push((abs, end, name));
        }
    }
    // 最内层：没有任何**别的**匹配体落在它之内。同层的取源码顺序里的第一个（确定性）。
    let chosen = found.iter().find(|(a, e, _)| {
        !found
            .iter()
            .any(|(a2, e2, _)| !(*a2 == *a && *e2 == *e) && *a2 >= *a && *e2 <= *e)
    });
    let Some((abs, end, name)) = chosen.map(|(a, e, n)| (*a, *e, n.clone())) else {
        return Disclosure::default();
    };
    let body = &masked[abs..end];
    // 定义行自己也算一次 `name(` ⇒ 计数 ≥ 2 才是真有一处**调用**。
    let mentions = masked.matches(&format!("{name}(")).count();
    Disclosure {
        gap_derived: body
            .lines()
            .any(|l| l.contains(ENTRY_IDENT) && l.contains(ROW_IDENT)),
        gated_on_empty: body.contains(EMPTY_GUARD),
        called: mentions >= 2,
        name: Some(name),
    }
}

/// 从示例配置的 `[rollup]` 段读保留窗口（天）。注释行不算（同段注释里提到过这个键名）。
fn retain_days(toml: &str) -> Option<i64> {
    let mut in_section = false;
    for raw in toml.lines() {
        let line = raw.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            in_section = line == RETAIN_SECTION;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some(v) = line.strip_prefix(RETAIN_KEY) {
            let v = v.trim_start().strip_prefix('=')?;
            return v.split('#').next()?.trim().replace('_', "").parse().ok();
        }
    }
    None
}

/// 扫一遍语料，产出读数。
fn read() -> Reading {
    // 消费侧的语料：`ui/js/app.js` 在 `src/` 之外，`source_files()` 走不到，只能内嵌
    // （`CONSUME_FILE` 是它在文档与断言里的名字）。
    let app_js = include_str!("../ui/js/app.js");
    let masked_js = mask_js(app_js);
    // 掩码等长 ⇒ 同一对下标切出两个文本：结构/深度看掩码，字面量看原文。
    let (bs, be) = range_fn_body(&masked_js).unwrap_or((0, 0));
    let masked_body = &masked_js[bs..be];
    let orig_body = &app_js[bs..be];
    let (presets, unreadable) = presets_days(masked_body);
    let options = range_options(include_str!("../ui/index.html"));
    let unbounded = unbounded_options(orig_body, masked_body, &options);
    let rollup = source_files()
        .into_iter()
        .find(|(name, _)| name.ends_with(ROLLUP_FILE))
        .map(|(_, src)| src)
        .unwrap_or_default();
    let field = boundary_field(&rollup);
    // 发布侧：引用常量名（不是又写一遍字面量 —— 那会变成第二个拼写点）。
    let published = source_files()
        .into_iter()
        .find(|(name, _)| name.ends_with(PUBLISH_FILE))
        .map(|(_, src)| src.contains("tx_rollup::DETAIL_SINCE_FIELD"))
        .unwrap_or(false);
    let consumed = field
        .as_deref()
        .map(|f| disclosure(app_js, f))
        .unwrap_or_default();
    // ⚠️ `mask_strings = false`：这里要看的正是**字符串字面量里**的 SQL。掩掉字符串会把
    // 被判物一起抹平（读数变成「函数体里什么都没有」），而掩掉注释是必须的 ——
    // 否则文档注释里的 `DELETE FROM transactions` 会冒充站点。
    let masked = code_mask(&rollup, false);
    // 删除函数体：从函数名起，到测试模块为止（它是该文件最后一个生产项）。
    let body = masked
        .find(DELETE_FN)
        .map(|i| {
            let tail = &masked[i..];
            let cut = tail.find("#[cfg(test)]").unwrap_or(tail.len());
            &tail[..cut]
        })
        .unwrap_or("");

    Reading {
        presets_days: presets,
        unreadable,
        retain_days: retain_days(include_str!("../config/config.example.toml")),
        // 「只删明细表」= 出现 `DELETE FROM transactions` 而**不**出现汇总表的删除形态。
        deletes_details_only: body.contains("DELETE FROM transactions")
            && !body.contains("DELETE FROM transactions_rollup"),
        has_fold_bound: body.contains(FOLD_WATERMARK),
        has_archive_bound: body.contains(ARCHIVE_BOUND),
        // 「两界取上界」的形状：**同一条语句**同时点到折叠水位与归档水位。
        // 不钉死 `.min(` 的写法 —— 换成 `if` 分支也是同一个上界，但**必须**两界同现。
        bounds_combined: body
            .lines()
            .any(|l| l.contains(FOLD_WATERMARK) && l.contains(ARCHIVE_BOUND)),
        has_time_window: body.contains("time <"),
        range_options: options,
        unbounded_options: unbounded,
        boundary_field: field,
        boundary_published: published,
        disclosure: consumed,
    }
}

#[test]
fn the_retention_window_covers_every_preset_the_list_offers() {
    let r = read();
    assert!(
        r.r1(),
        "交易列表提供的预设区间比明细保留窗口还深 ⇒ 点那个预设会安静地少答一段历史（`total` \
         与行同源，不会自相矛盾，只会一起缩水）：{}",
        r.blame()
    );
}

#[test]
fn the_unbounded_ranges_are_disclosed_to_the_page() {
    let r = read();
    assert!(
        r.r4(),
        "交易列表提供了**深度无界**的区间（{:?}），而明细保留边界没有以「缺口」的形式说给页面 —— \
         那一屏会同时出现聚合答得出的数和一条明细都没有、或**少列**的列表，配一句「试试调整筛选条件」\
         （错的建议：筛选条件怎么调都变不出库里已经不存在的行）。要么把边界发布出去、并让一个\
         **以 `entries` 与 `total` 的差为准**（而不是以「列表空不空」为准）的函数生产那句说明，\
         要么不提供这些区间：{}\n  披露生产函数：{}",
        r.unbounded_options,
        r.blame(),
        r.disclosure.why_not()
    );
}

#[test]
fn the_retention_window_covers_every_preset_the_list_offers_negatives() {
    // 五条独立牙齿：每条只翻一条规则，其余不动。
    let base = read();
    assert!(base.all(), "基线: {}", base.blame());

    for (name, mutate) in [
        (
            "窗口收到列表最深的预设之下",
            Box::new(|r: &mut Reading| {
                r.retain_days = Some(r.presets_days.last().copied().unwrap_or(0) - 1);
            }) as Box<dyn Fn(&mut Reading)>,
        ),
        (
            "删掉折叠水位界限",
            Box::new(|r: &mut Reading| {
                r.has_fold_bound = false;
            }),
        ),
        (
            "删掉归档水位界限（名字还在签名里，但不再与折叠水位同句）",
            Box::new(|r: &mut Reading| {
                r.bounds_combined = false;
            }),
        ),
        (
            "无条件全删（没有时间窗）",
            Box::new(|r: &mut Reading| {
                r.has_time_window = false;
            }),
        ),
        (
            "无界区间还在，但服务端不发布边界了",
            Box::new(|r: &mut Reading| {
                // R4 的牙只在「还有无界区间」时才有对象。产品完全可以**合法地**把无界区间
                // 从控件里去掉（那时不需要披露），所以这里**构造**前提，而不是靠语料恰好非空
                // —— 否则一条合法的产品改动会让这条牙报「没打翻」。
                if r.unbounded_options.is_empty() {
                    r.unbounded_options = vec!["synthetic".to_string()];
                }
                r.boundary_published = false;
            }),
        ),
        (
            "无界区间还在，但前端不再生产披露（没人说这件事）",
            Box::new(|r: &mut Reading| {
                if r.unbounded_options.is_empty() {
                    r.unbounded_options = vec!["synthetic".to_string()];
                }
                r.disclosure.name = None;
            }),
        ),
        (
            "披露不再以「缺口」为准（两个数不在同一行相遇）",
            Box::new(|r: &mut Reading| {
                if r.unbounded_options.is_empty() {
                    r.unbounded_options = vec!["synthetic".to_string()];
                }
                r.disclosure.gap_derived = false;
            }),
        ),
        (
            "披露重新带上「列表非空就不解释」的闸门（R104 轴的原形状）",
            Box::new(|r: &mut Reading| {
                if r.unbounded_options.is_empty() {
                    r.unbounded_options = vec!["synthetic".to_string()];
                }
                r.disclosure.gated_on_empty = true;
            }),
        ),
        (
            "披露的函数定义了却没人调用（死代码 ⇒ 屏幕上什么都不会出现）",
            Box::new(|r: &mut Reading| {
                if r.unbounded_options.is_empty() {
                    r.unbounded_options = vec!["synthetic".to_string()];
                }
                r.disclosure.called = false;
            }),
        ),
    ] {
        let mut m = base.clone();
        mutate(&mut m);
        assert_ne!(base, m, "变异体「{name}」没改动任何读数");
        assert!(
            !m.all(),
            "变异体「{name}」应当被打翻，却仍全绿: {}",
            m.blame()
        );
    }

    // R4 的**另一种合法解**：把无界的区间从控件里去掉（那时没有东西需要披露）。
    // 这条必须**绿** —— 否则门禁逼着产品必须提供无界区间，那是把手段当成了目的。
    let mut dropped = base.clone();
    dropped.unbounded_options.clear();
    assert!(
        dropped.r4(),
        "无界区间一条都没有时 R4 应当自动成立: {}",
        dropped.blame()
    );
}

#[test]
fn the_scanner_sees_both_sides() {
    // 阳性对照：两侧都真的读到了东西，否则「零违规」与「扫描器是瞎的」读数相同（坑 #814）。
    let r = read();
    assert!(
        r.presets_days.len() >= 3,
        "预设深度一个都没读出来（提取器失真？）：{}",
        r.blame()
    );
    assert!(
        r.presets_days.contains(&30),
        "列表最深预设应能读出来（`MS(24 * 30)`）：{}",
        r.blame()
    );
    assert!(r.unreadable.is_empty(), "有读不懂的预设: {}", r.blame());
    assert!(
        r.retain_days.is_some(),
        "示例配置里读不到 {RETAIN_KEY}（段名或键名变了？）：{}",
        r.blame()
    );
    assert!(
        r.deletes_details_only,
        "没在删除函数体里看见「只删明细表」的形状（函数名变了？）：{}",
        r.blame()
    );
    // R4 三个提取器各自读到东西（缺任一条，R4 会变成恒真或恒假的空转）。
    assert!(
        r.range_options.len() >= 3,
        "`#tx-range` 的选项一个都没读出来（选择器或 `{RANGE_SELECT}` 变了？）：{}",
        r.blame()
    );
    assert!(
        r.boundary_field.is_some(),
        "`tx_rollup.rs` 里读不到边界字段名的单一拼写点（常量名变了？）：{}",
        r.blame()
    );
    assert!(
        r.boundary_published,
        "服务端没引用那个常量（发布侧又写了一遍字面量，或发布点没了）：{}",
        r.blame()
    );
    assert!(
        r.disclosure.reaches_page(),
        "`{CONSUME_FILE}` 里没有一个「以缺口为准、不带空态闸门、且被调用过」的披露生产函数\
         （生产点没了，或它又退回了 R104 的原形状）：{}",
        r.blame()
    );
    // 反面：不能**所有**区间都被判成无界 —— 那说明「分支头」按字符串字面量找、在掩码文本里
    // 找不到（选项值本来就写在引号里），判据整条瞎掉，R4 就成了恒真的空转。
    assert!(
        r.unbounded_options.len() < r.range_options.len(),
        "所有区间都被判成无界（分支头是字符串字面量，掩码文本里找不到 ⇒ 判据全瞎）：{}",
        r.blame()
    );
}
