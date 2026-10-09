//! 交易类型名册门禁（R94）：**前端运行期名册 == 服务端受理集合**。
//!
//! 「交易页能把哪些 `type` 值筛出来」是一个事实，它有**三个载体**，只有两个被守：
//!
//! | 载体 | 位置 | 谁在守 |
//! |---|---|---|
//! | 受理集合（**唯一真源**） | `src/routes/wallet.rs::TX_FILTER_TYPES` | —— |
//! | 端点校验 ＋ 400 文案 | `parse_tx_type`（同源取值） | `state_gate` 的 R99 车道（R4：写入 ⊆ 受理） |
//! | **前端「类型」列筛选项** | `ui/js/app.js` 的行内字面量 | **无人守**（本门禁） |
//!
//! 该数组自己的文档写着两句话，本门禁把第二句变成可执行的：
//!
//! > 前端交易页「类型」列筛选的选项即此集合（`ui/js/app.js` 的 tx type options）……
//! > **新增类型时只改这里。**
//!
//! 但 `app.js` 的类型列选项是**手抄**的六元字面量，与那个数组之间**没有任何东西**在比对 ——
//! 换句话说「只改这里」这句承诺，在前端这一半**从未成立**（R99 车道守的是写入点与语言包，
//! 不看 `ui/`；R164 车道守的是排序白名单，不看类型白名单）。
//!
//! # 症状（不是假设，有先例）
//!
//! 前端把**同一个** `type` 值同时发给 `/api/transactions` 与 `/api/transactions/trend`
//! （见 `wallet.rs::transactions` 的行内注释：rant 2026-08-25T10:33:26「列筛选后端化后
//! UI 选项须全被 API 接受」）。C2052 已经踩过一次：`expire` 落地时两个端点只补了列表那一个，
//! 结果是**列表正常、趋势图只显示「趋势数据加载失败」**。那颗雷的**另一半**从未被检查过 ——
//! 若新增类型时只改 `TX_FILTER_TYPES`，前端下拉里就少一个可筛值（改成只改前端则是 400）。
//!
//! # 判据（射程如实）
//!
//! 三条规则，期望值**全部**从制品推导、零快照：
//!
//! * **R1** 前端选项名册（`app.js` 类型列的 `options: () => […]` 字面量）与 `TX_FILTER_TYPES`
//!   **集合相等** —— 这是文档承诺的那句话。比较按**集合**，不按顺序（下拉的展示次序是呈现选择，
//!   今天两边就不同）。
//! * **R2** 受理集合 ⊆ 前端标签表 `TX_TYPE` 的键 —— 被提供的每个值都要有标签，否则
//!   `<option>` 会把**裸库内值**印成文案（`TX_TYPE` 缺失时 `txType(k)` 回落到 `k` 本身）。
//! * **R3** 表格构造器仍然消费 `col.options` —— 否则上面的名册字面量成了死数据，
//!   而 R1/R2 会继续对一个**到不了屏幕**的载体判绿（形状规则：名册 → 屏幕的那一跳）。
//!
//! ⛔ **本门禁是词法的**：它证「源码里那两处名册与受理集合互相同源」，**不证**屏幕上那一刻
//! 真的渲染出了这些 `<option>`（那一跳由 R3 的词法判据近似覆盖）。它同样**只看**
//! 交易类型这一条链路：`MONTH_TYPE_LABELS`（仪表盘「本月点数变化」逐类型行）是同一事实的
//! 另一处前端名册，但它的需求是「覆盖服务端**可能聚合出**的类型」而不是「等于受理集合」，
//! 判据不同源，**刻意不在本门禁射程内**（发现即入册，不是遗漏）。
//!
//! ⛔ **不写快照**：六元名册从 `wallet.rs` 的字面量现读，选项名册与标签键从 `app.js` 现读；
//! 三个锚点在语料里各出现**恰好一次**（本门禁把这条写成断言 —— 多一个载体就响亮地红，
//! 而不是静默地只读第一个）。

use crate::js_gate::{mask_js, JS_SOURCES};

/// 受理集合（唯一真源）所在的文件。
const WALLET_RS: &str = include_str!("routes/wallet.rs");

/// 受理集合的声明锚（`pub const TX_FILTER_TYPES: [&str; N] = [ … ];`）。
const ACCEPTED_ANCHOR: &str = "TX_FILTER_TYPES: [&str; ";

/// 前端「类型」列选项行的身份标记 —— 该列的标签由 `txType(k)` 造（状态列用 `txStatus(s)`），
/// 因此它在语料里唯一，是找到**那一处** `options: () => [` 的把手。
const TYPE_LABEL_CALL: &str = "label: txType(k)";

/// 选项数组字面量的开头。
const OPTIONS_ANCHOR: &str = "options: () => [";

/// 前端类型标签表的声明锚。
const TX_TYPE_ANCHOR: &str = "const TX_TYPE = {";

/// 表格构造器消费 `col.options` 的那一跳（`buildDataTable` 的 select 分支）。
const RENDER_ANCHOR: &str = "typeof col.options";

/// 一次扫描的读数 —— 判决与它的证据一起产出（一个藏在脚注里的期望会报出自我一致的谎）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Reading {
    /// `TX_FILTER_TYPES` 头里声明的长度。
    declared: usize,
    /// 从数组字面量里读出来的受理集合。
    accepted: Vec<String>,
    /// 前端类型列选项字面量里的值。
    options: Vec<String>,
    /// `TX_TYPE` 的键。
    labels: Vec<String>,
    /// 构造器是否仍然消费 `col.options`。
    render_consumes: bool,
}

impl Reading {
    /// 前置（空集上的集合断言会假绿，坑 #68）：声明长度与实读成员**相等**且都非空，
    /// 两份前端名册都非空。
    fn preconditions(&self) -> bool {
        !self.accepted.is_empty()
            && !self.options.is_empty()
            && !self.labels.is_empty()
            && self.accepted.len() == self.declared
    }

    /// R1：前端选项名册 == 受理集合（集合语义）。
    fn r1(&self) -> bool {
        set_eq(&self.options, &self.accepted)
    }

    /// R2：受理集合 ⊆ 前端标签表。
    fn r2(&self) -> bool {
        subset(&self.accepted, &self.labels)
    }

    /// R3：名册到屏幕的那一跳仍在。
    fn r3(&self) -> bool {
        self.render_consumes
    }

    fn ok(&self) -> bool {
        self.preconditions() && self.r1() && self.r2() && self.r3()
    }

    fn report(&self) -> String {
        format!(
            "preconditions={} r1={} r2={} r3={} | declared={} accepted={:?} options={:?} labels={:?}",
            self.preconditions(),
            self.r1(),
            self.r2(),
            self.r3(),
            self.declared,
            self.accepted,
            self.options,
            self.labels
        )
    }
}

fn set_eq(a: &[String], b: &[String]) -> bool {
    subset(a, b) && subset(b, a)
}

fn subset(a: &[String], b: &[String]) -> bool {
    a.iter().all(|x| b.iter().any(|y| y == x))
}

/// 在**原始**文本里数出现次数（锚点唯一性自证用）。
fn count(hay: &str, needle: &str) -> usize {
    hay.match_indices(needle).count()
}

/// 从 `from` 起（`from` 指向数组的 `[` 之后）读到配对的 `]`，收集其中的字符串字面量值。
fn quoted_list(src: &str, from: usize) -> Vec<String> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = from;
    while i < b.len() {
        match b[i] {
            b']' => break,
            q @ (b'"' | b'\'') => {
                let mut j = i + 1;
                while j < b.len() && b[j] != q {
                    if b[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
                let end = j.min(b.len());
                out.push(src[i + 1..end].to_string());
                i = end + 1;
            }
            _ => i += 1,
        }
    }
    out
}

/// 从 `from` 起（指向 `{` 之后）读到 `}`，收集「后面紧跟 `:` 的标识符」＝对象的键名。
///
/// ⚠️ 传进来的必须是**掩码后**的文本：值里的字符串（`T("tx.type.gift")`）会被涂成空格，
/// 于是字符串内部的冒号与点号不可能被误判成键分隔符。
fn ident_keys(masked: &str, from: usize) -> Vec<String> {
    let b = masked.as_bytes();
    let mut out = Vec::new();
    let mut i = from;
    while i < b.len() {
        let c = b[i];
        if c == b'}' {
            break;
        }
        if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'$') {
                i += 1;
            }
            let mut k = i;
            while k < b.len() && (b[k] == b' ' || b[k] == b'\t') {
                k += 1;
            }
            if k < b.len() && b[k] == b':' {
                out.push(masked[start..i].to_string());
            }
        } else {
            i += 1;
        }
    }
    out
}

/// 读 `TX_FILTER_TYPES`：先读头里声明的长度，再读数组字面量的成员。
///
/// 锚点**自己以 `[&str; ` 结尾**（那个 `[` 属于长度标注）⇒ 头与字面量的那个 `[` 必须分开找。
fn read_accepted(wallet: &str) -> (usize, Vec<String>) {
    let at = wallet
        .find(ACCEPTED_ANCHOR)
        .unwrap_or_else(|| panic!("`{ACCEPTED_ANCHOR}` 未找到 —— 受理集合搬走了？"));
    let rest = &wallet[at + ACCEPTED_ANCHOR.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    let declared: usize = digits
        .parse()
        .unwrap_or_else(|_| panic!("`{ACCEPTED_ANCHOR}` 后面不是长度标注：{rest:.40}"));
    // 头的收尾 `]`，然后是 `=`，然后是字面量的 `[`。
    let head_end = rest
        .find(']')
        .unwrap_or_else(|| panic!("受理集合的头部没有收尾 `]`：{rest:.40}"));
    let after = &rest[head_end..];
    let open = after
        .find('[')
        .unwrap_or_else(|| panic!("受理集合头部之后没有数组字面量：{after:.40}"));
    let members = quoted_list(rest, head_end + open + 1);
    (declared, members)
}

/// 读前端「类型」列筛选项名册。
///
/// ⚠️ 定位用**掩码**文本（注释里逐字引用代码是常态，坑见 `js_gate` 模块头），
/// 取值用**原始**文本（字面量正是要被读的东西，掩码会把它们涂掉）。
fn read_options(app: &str, masked: &str) -> Vec<String> {
    let label_at = masked
        .find(TYPE_LABEL_CALL)
        .unwrap_or_else(|| panic!("`{TYPE_LABEL_CALL}` 未找到 —— 类型列的选项构造搬走了？"));
    // 往回找最近的 `options: () => [`。
    let open = masked[..label_at]
        .rfind(OPTIONS_ANCHOR)
        .unwrap_or_else(|| panic!("`{TYPE_LABEL_CALL}` 之前没有 `{OPTIONS_ANCHOR}`"));
    let bracket = open + OPTIONS_ANCHOR.len();
    quoted_list(app, bracket)
}

/// 读前端类型标签表 `TX_TYPE` 的键名册。
fn read_labels(masked: &str) -> Vec<String> {
    let at = masked
        .find(TX_TYPE_ANCHOR)
        .unwrap_or_else(|| panic!("`{TX_TYPE_ANCHOR}` 未找到 —— 标签表搬走了？"));
    ident_keys(masked, at + TX_TYPE_ANCHOR.len())
}

/// 扫一份语料，产出读数（判决与证据）。
fn judge(wallet: &str, app: &str) -> Reading {
    let masked = mask_js(app);
    let (declared, accepted) = read_accepted(wallet);
    Reading {
        declared,
        accepted,
        options: read_options(app, &masked),
        labels: read_labels(&masked),
        render_consumes: masked.contains(RENDER_ANCHOR),
    }
}

// ---------------------------------------------------------------------------------------------
// 门禁
// ---------------------------------------------------------------------------------------------

/// 语料在编译期读入：受理集合在 `src/`，前端两份名册在 `ui/js/app.js`。
fn app_js() -> &'static str {
    JS_SOURCES
        .iter()
        .find(|(name, _)| *name == "app.js")
        .map(|(_, src)| *src)
        .expect("`JS_SOURCES` 里必须有 app.js")
}

/// **锚点唯一性（阳性对照）**：三个把手在各自的语料里各出现**恰好一次**。
///
/// 它同时证两件事：扫描器读的是**被守的那一处**，且语料里没有第二处同形载体被静默忽略
/// （多一处 ⇒ 本门禁当场红，要求把新载体一并纳入，而不是继续只读第一个）。
#[test]
fn the_anchors_name_one_carrier_each() {
    assert_eq!(
        count(WALLET_RS, ACCEPTED_ANCHOR),
        1,
        "受理集合的锚在 wallet.rs 里不止一处 —— 新的载体要一并纳入本门禁"
    );
    let masked = mask_js(app_js());
    assert_eq!(
        count(&masked, TYPE_LABEL_CALL),
        1,
        "`{TYPE_LABEL_CALL}` 在 app.js 的代码语料里不止一处"
    );
    assert_eq!(
        count(&masked, TX_TYPE_ANCHOR),
        1,
        "`{TX_TYPE_ANCHOR}` 在 app.js 的代码语料里不止一处"
    );
    assert_eq!(
        count(app_js(), OPTIONS_ANCHOR),
        2,
        "`{OPTIONS_ANCHOR}` 的出现次数变了（今天：类型列 ＋ 状态列）—— 把手需要重新核对"
    );
    assert!(
        masked.contains(RENDER_ANCHOR),
        "表格构造器不再消费 `col.options` —— 名册字面量已成死数据（R3 的前提）"
    );
}

/// **轴**：前端交易页「类型」列筛选项 == 服务端受理集合，且每个值都有标签。
#[test]
fn the_ui_offers_exactly_the_accepted_transaction_types() {
    let r = judge(WALLET_RS, app_js());
    assert!(
        r.preconditions(),
        "前置不成立（声明长度与实读成员不相等，或某份名册是空的）：{}",
        r.report()
    );
    assert!(
        r.r1(),
        "前端类型列筛选项与 `TX_FILTER_TYPES` 不同源 —— 「新增类型时只改这里」不成立：{}",
        r.report()
    );
    assert!(
        r.r2(),
        "受理集合里有值找不到前端标签（`<option>` 会印出裸库内值）：{}",
        r.report()
    );
    assert!(r.r3(), "名册到屏幕的那一跳断了：{}", r.report());
}

/// **牙**：名册两侧任一处漂移都要被点名（且只点名那一条规则）。
#[test]
fn a_roster_that_drifts_from_the_accepted_set_reddens() {
    let wallet = wallet_fixture(3, &["consume", "earn", "topup"]);
    let app = app_fixture(&["consume", "earn", "topup"], &["consume", "earn", "topup"]);
    let base = judge(&wallet, &app);
    assert!(base.ok(), "对照语料本该全绿：{}", base.report());

    // (a) 前端少一个值（后端的受理集合没跟着改）
    let missing = app_fixture(&["consume", "earn"], &["consume", "earn", "topup"]);
    let r = judge(&wallet, &missing);
    assert!(
        !r.r1() && r.preconditions() && r.r2() && r.r3(),
        "{}",
        r.report()
    );

    // (b) 前端多一个值（API 会 400 —— C2052 的形态就是这一侧）
    let extra = app_fixture(
        &["consume", "earn", "topup", "refund"],
        &["consume", "earn", "topup", "refund"],
    );
    let r = judge(&wallet, &extra);
    assert!(
        !r.r1() && r.preconditions() && r.r2() && r.r3(),
        "{}",
        r.report()
    );

    // (c) 顺序不同不算漂移（集合语义 —— 今天两边次序本就不同）
    let reordered = app_fixture(&["topup", "consume", "earn"], &["consume", "earn", "topup"]);
    let r = judge(&wallet, &reordered);
    assert!(r.ok(), "顺序不该影响判决：{}", r.report());
}

/// **牙**：标签表漏一个受理值 ⇒ 只翻 R2；名册空了 ⇒ 只翻前置。
#[test]
fn a_missing_label_and_an_empty_roster_each_redden_their_own_rule() {
    let wallet = wallet_fixture(3, &["consume", "earn", "topup"]);

    // 标签表漏掉 `topup`：选项仍齐全（R1 真），标签缺一个（R2 假）。
    let no_label = app_fixture(&["consume", "earn", "topup"], &["consume", "earn"]);
    let r = judge(&wallet, &no_label);
    assert!(
        !r.r2() && r.r1() && r.preconditions() && r.r3(),
        "{}",
        r.report()
    );

    // 选项名册为空：前置假（空集上的集合断言会假绿，坑 #68）。
    let empty = app_fixture(&[], &["consume", "earn", "topup"]);
    let r = judge(&wallet, &empty);
    assert!(!r.preconditions() && !r.r1(), "{}", r.report());

    // 数组头声明的长度与实读成员不符（解析器少读了一个成员）⇒ 前置假，不是静默放过。
    let truncated = wallet_fixture(4, &["consume", "earn", "topup"]);
    let r = judge(
        &truncated,
        &app_fixture(&["consume", "earn", "topup"], &["consume", "earn", "topup"]),
    );
    assert!(!r.preconditions(), "{}", r.report());
}

/// **牙（掩码）**：注释里逐字引用的名册**不是**载体。
///
/// 语料里到处是「逐字引用代码」的注释；若不掩码，一条写着 `options: () => ["refund"]` 的注释
/// 会把不存在的成员种回语料里（`js_gate` 模块头记着同一个坑）。
#[test]
fn a_roster_quoted_in_a_comment_is_not_a_carrier() {
    let wallet = wallet_fixture(3, &["consume", "earn", "topup"]);
    let app = app_fixture(&["consume", "earn", "topup"], &["consume", "earn", "topup"]);
    let with_comment = format!(
        "// 旧实现：options: () => [\"refund\"].map((k) => ({{ value: k, label: txType(k) }}))\n{app}"
    );
    let r = judge(&wallet, &with_comment);
    assert_eq!(
        r.options,
        vec![
            "consume".to_string(),
            "earn".to_string(),
            "topup".to_string()
        ],
        "注释里的名册被当成了载体：{}",
        r.report()
    );
    assert!(r.ok(), "{}", r.report());
}

/// **牙（R3）**：构造器不再消费 `col.options` ⇒ 名册成了死数据。
#[test]
fn a_builder_that_stops_consuming_the_roster_reddens() {
    let wallet = wallet_fixture(3, &["consume", "earn", "topup"]);
    let app = app_fixture(&["consume", "earn", "topup"], &["consume", "earn", "topup"])
        .replace(RENDER_ANCHOR, "typeof col.whatever");
    let r = judge(&wallet, &app);
    assert!(
        !r.r3() && r.r1() && r.r2() && r.preconditions(),
        "{}",
        r.report()
    );
}

// ---------------------------------------------------------------------------------------------
// 合成语料（牙用；形状照抄真实语料，去掉一切与判据无关的部分）
// ---------------------------------------------------------------------------------------------

fn wallet_fixture(declared: usize, members: &[&str]) -> String {
    format!(
        "pub const TX_FILTER_TYPES: [&str; {declared}] = [{}];\n",
        members
            .iter()
            .map(|m| format!("\"{m}\""))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn app_fixture(options: &[&str], labels: &[&str]) -> String {
    let label_body = labels
        .iter()
        .map(|k| format!("{k}: () => T(\"tx.type.{k}\"),"))
        .collect::<String>();
    let option_body = options
        .iter()
        .map(|k| format!("\"{k}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "const TX_TYPE = {{{label_body}}};\n\
         const TX_COLUMNS = [\n\
         \x20 {{ key: \"type\", filter: \"select\", options: () => [{option_body}].map((k) => ({{ value: k, label: txType(k) }})) }},\n\
         ];\n\
         const opts = (typeof col.options === \"function\" ? col.options() : (col.options || []));\n"
    )
}
