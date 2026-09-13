//! 表格结构门禁（C2108）：列数一致 + 容器形态契约。
//!
//! `ui/` 的七张表把「列数」这一件事**手工写在三处**，三者必须相等：
//!
//! 1. `ui/index.html` 里该表的 `<thead>` 有几个 `<th>`；
//! 2. `ui/js/app.js` 的空态行 `emptyRow(N, …)` 的 `N`（`<td colspan="N">`）；
//! 3. 同表的加载失败行 `loadErrorRow(N, …)` 的 `N`，以及行模板 `<td data-label=…>` 的个数。
//!
//! 写错了**没有任何运行期报错**：`colspan` 小于列数时浏览器只是把那行画窄，
//! 大于列数时被静默截断 —— 而空态/失败态只有在「列表为空」或「接口失败」时才出现，
//! 正常路径永远看不到它。这正是 `#97` 的真实事故：给 admin model 表加第 8 列时，
//! 表头改成 8 列而空态仍是 `colspan=7`，直到 C1988 才被人眼发现、`#162` 修复。
//! 仓库既没有 `regex`，也没有任何门禁读表格结构（`i18n_pack.rs` 只读键与调用点，
//! `catalog_gate.rs` 只读模型目录）——本模块把这条对应关系固化进 `cargo test`。
//!
//! 第二条契约（同一类「表格标记结构」的隐形约定）：**`<tbody>` 容器只能注入 `<tr>` 形态**。
//! 浏览器会把 `<tbody>` 里的裸 `<div>` 提升到表外，于是降级文案会渲染在表格之外、
//! 且重试按钮脱离 `setLiveError` 的容器级委托（`loadErrorRow` 的注释写明了这一点）。
//! 所以：`<tbody>` 容器 ⇒ 必须用 `loadErrorRow`；`<div>` 容器 ⇒ 用 `loadErrorHtml`。
//!
//! 设计约束（与 `i18n_pack.rs` / `catalog_gate.rs` 同型）：
//! - **仅测试期编译**（`#[cfg(test)] mod`，见 `main.rs`），不进生产二进制；
//! - **零新依赖**：不用 `regex`（仓库没有），只做逐字节扫描；不执行 JS、不起浏览器；
//! - **关联方式全是位置性的**（不靠花括号配对、不做 JS 解析）：
//!   `emptyRow(N` 的归属容器 = 它之前最近的一次 `$("#ID").innerHTML =`；
//!   `loadErrorRow(N` 的归属容器 = 同一个表达式里的 `setLiveError($("#ID"), `。
//! - **阳性对照**：闸门断言「扫描到 7 张表 / 7 个空态 / 7 个失败态」以及每张表的列数真值。
//!   扫描器若因写错而返回空集，这些计数会先失败，而不是让集合断言在空集上「通过」（C2005 坑 68）。

/// 编译期读入两侧真源（测试不依赖工作目录与文件系统布局）。
const INDEX_HTML: &str = include_str!("../ui/index.html");
const APP_JS: &str = include_str!("../ui/js/app.js");

/// 已知真值：`(tbody id, 列数)`。**改动表格列数时应刻意更新这里**。
///
/// 它是扫描器的阳性对照，也是本门禁最核心的断言 —— 两侧的 `N` 都拿来跟它比。
const TABLES: &[(&str, usize)] = &[
    ("mk-body", 7),
    ("share-body", 8),
    ("api-keys", 6),
    ("emp-body", 7),
    ("dept-body", 7),
    ("model-body", 8),
    ("ops-body", 4),
];

/// 需要空态/失败态、但**不是** `<tbody>` 的容器：它们只能用裸 `<div>` 形态的 `loadErrorHtml`。
/// （`#tx-table` 是 `.table-wrap` div，其余三个是卡片里的列表容器。）
const NON_TBODY_LIVE_CONTAINERS: &[&str] =
    &["tx-table", "dash-sharings", "usage-model", "ops-stats"];

fn nth(src: &str, needle: &str, from: usize) -> Option<usize> {
    src[from..].find(needle).map(|i| i + from)
}

/// 数一个**开始标签**的出现次数，且要求标签名后紧跟非字母（`<th>`、`<th ` 算，`<thead` 不算）。
///
/// 这个边界正是本模块第一版探针踩过的坑：`"<th".count()` 会把 `<thead>` 也算进去，
/// 于是七张表全部报「表头比 colspan 多 1 列」的**假阳性**。
fn count_tag(src: &str, tag: &str) -> usize {
    let open = format!("<{tag}");
    let mut n = 0;
    let mut at = 0;
    while let Some(i) = nth(src, &open, at) {
        at = i + open.len();
        let next = src[at..].chars().next();
        if !next.map(|c| c.is_ascii_alphanumeric()).unwrap_or(false) {
            n += 1;
        }
    }
    n
}

/// 数带 `colspan` 的 `<td>`（整行单元格，如市场表的详情行）—— 它们不占单独的列。
fn count_spanning_tds(src: &str) -> usize {
    let mut n = 0;
    let mut at = 0;
    while let Some(i) = nth(src, "<td", at) {
        at = i + "<td".len();
        if !src[at..]
            .chars()
            .next()
            .map(|c| c.is_ascii_alphanumeric())
            .unwrap_or(false)
            && src[at..].trim_start().starts_with("colspan")
        {
            n += 1;
        }
    }
    n
}

/// `ui/index.html`：每个 `<tbody id="X">` 所在 `<table>` 的 `<thead>` 列数。
fn table_columns(html: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(start) = nth(html, "<table", at) {
        let end = nth(html, "</table>", start).expect("表格必须有 </table>");
        at = end + 1;
        let block = &html[start..end];
        let Some(body) = nth(block, "<tbody id=\"", 0) else {
            continue; // 无 tbody 的表（如原型里静态渲染的）不参与本契约
        };
        let id_start = body + "<tbody id=\"".len();
        let id_end = id_start
            + block[id_start..]
                .find('"')
                .expect("tbody id 必须有结束引号");
        let thead_start = nth(block, "<thead", 0).expect("有 tbody 的表必须有 thead");
        let thead_end = nth(block, "</thead>", thead_start).expect("thead 必须闭合");
        out.push((
            block[id_start..id_end].to_string(),
            count_tag(&block[thead_start..thead_end], "th"),
        ));
    }
    out
}

/// 从 `loadErrorRow(` 出现处**向前**取同表达式里的容器 id：`setLiveError($("#ID"), loadErrorRow(`。
fn error_row_container(js: &str, call_at: usize) -> String {
    let seg_start = js[..call_at]
        .rfind("setLiveError(")
        .expect("loadErrorRow 必须写在 setLiveError(容器, …) 里 —— 否则没人知道它属于哪张表");
    let seg = &js[seg_start..call_at];
    let q = seg
        .find("$(\"#")
        .expect("setLiveError 的第一个实参应是 $(\"#id\")");
    let id_start = q + "$(\"#".len();
    let id_end = id_start + seg[id_start..].find("\")").expect("id 必须有结束引号");
    seg[id_start..id_end].to_string()
}

/// 读出紧跟在 `marker` 之后的一个十进制字面量（`emptyRow(7,` ⇒ 7）。
fn number_after(src: &str, from: usize) -> usize {
    let digits: String = src[from..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    assert!(
        !digits.is_empty(),
        "期望 {from} 处是十进制字面量，实际是 {:?}",
        &src[from..(from + 12).min(src.len())]
    );
    digits.parse().expect("字面量应是合法整数")
}

/// 收集 `marker(` 形式的**调用**位置：要求紧跟其后的首字符是十进制数字。
///
/// 这一步是必需的 —— 两个辅助函数自己的**定义**长这样：
/// `function emptyRow(colspan, text, sub, actionHtml)` / `function loadErrorRow(colspan, …)`，
/// 形参不是数字。不加这个判别，扫描器会把定义当成一个「站点」并在取数字时炸掉
/// （第一版正是如此：报出 `期望 … 处是十进制字面量，实际是 "colspan, emp"`）。
fn call_sites(src: &str, marker: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(i) = nth(src, marker, at) {
        at = i + marker.len();
        if src[at..]
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
        {
            out.push(i);
        }
    }
    out
}

/// `ui/js/app.js`：所有 `emptyRow(N, …)`，连同其归属容器与行模板的 `<td` 数。
///
/// 行模板区间＝该容器最近一次 `.innerHTML =` 到这条 `emptyRow(` 之间 ——
/// 这段正是 `… .join("") : emptyRow(N, …)` 里「真行模板」的部分。
/// 返回值：`(容器 id, N, 行模板 <td 数, 其中带 colspan 的个数)`。
fn empty_row_sites(js: &str) -> Vec<(String, usize, usize, usize)> {
    let mut out = Vec::new();
    for call in call_sites(js, "emptyRow(") {
        let n = number_after(js, call + "emptyRow(".len());
        let assign = js[..call]
            .rfind(".innerHTML")
            .expect("emptyRow 之前必须有一次 .innerHTML = 赋值（否则无法归属）");
        let head = &js[..assign];
        let q = head.rfind("$(\"#").expect("赋值左侧应是 $(\"#id\")");
        let id_start = q + "$(\"#".len();
        let id_end = id_start + head[id_start..].find("\")").expect("id 必须有结束引号");
        let region = &js[assign..call];
        let tds = count_tag(region, "td");
        // 「整行单元格」＝带 colspan 的 <td>（详情行）：它们不占一列，要从行模板列数里剔除
        let colspan_tds = count_spanning_tds(region);
        out.push((head[id_start..id_end].to_string(), n, tds, colspan_tds));
    }
    out
}

/// `ui/js/app.js`：所有 `loadErrorRow(N, …)`，连同其所属容器。
fn error_row_sites(js: &str) -> Vec<(String, usize)> {
    call_sites(js, "loadErrorRow(")
        .into_iter()
        .map(|call| {
            let n = number_after(js, call + "loadErrorRow(".len());
            (error_row_container(js, call), n)
        })
        .collect()
}

/// `ui/js/app.js` / `ui/index.html`：写死的 `colspan="N"`（排除两个辅助函数的定义）。
fn literal_colspans(src: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(i) = nth(src, "colspan=\"", at) {
        at = i + "colspan=\"".len();
        if src[at..]
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
        {
            out.push(number_after(src, at));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols() -> Vec<(String, usize)> {
        table_columns(INDEX_HTML)
    }

    /// 扫描器自证：`<thead>` 不得被数成 `<th>`（本模块第一版探针的真实假阳性）。
    #[test]
    fn tag_scanner_is_not_fooled_by_a_longer_tag_name() {
        assert_eq!(count_tag("<thead><tr><th>a</th></tr></thead>", "th"), 1);
        assert_eq!(count_tag("<th></th><th></th>", "th"), 2);
        assert_eq!(count_tag("<th class=\"num\">x</th>", "th"), 1);
        assert_eq!(count_tag("<thead></thead>", "th"), 0);
        assert_eq!(count_tag("<table><tbody></tbody></table>", "td"), 0);
        assert_eq!(count_tag("<td>1</td><td colspan=\"2\">2</td>", "td"), 2);
    }

    /// 阳性对照 + 真值：扫描器必须恰好找到这七张表、且列数与 `TABLES` 逐项相同。
    /// 扫描器写错而返回空集时，这里先失败（集合断言在空集上会「通过」，C2005 坑 68）。
    #[test]
    fn every_tbody_table_is_scanned_with_its_true_column_count() {
        let found = cols();
        assert_eq!(found.len(), TABLES.len(), "扫描到的表格数不对：{found:?}");
        for (id, want) in TABLES {
            let got = found.iter().find(|(x, _)| x == id).unwrap_or_else(|| {
                panic!("index.html 里找不到 tbody id={id}（表格被改名/删除？）")
            });
            assert_eq!(got.1, *want, "{id} 的 <thead> 列数变了：{found:?}");
        }
    }

    /// 站点归属：空态/失败态站点必须都落在**已登记**的表格上。
    /// 多出来的一处 = 新表没登记进 `TABLES`，或扫描错位（归属到别的容器）。
    #[test]
    fn every_live_state_site_belongs_to_a_known_table() {
        let found = cols();
        let empties = empty_row_sites(APP_JS);
        let errors = error_row_sites(APP_JS);
        assert_eq!(empties.len(), TABLES.len(), "空态站点数不对：{empties:?}");
        assert_eq!(errors.len(), TABLES.len(), "失败态站点数不对：{errors:?}");
        for (id, ..) in &empties {
            assert!(
                found.iter().any(|(x, _)| x == id),
                "空态站点归属到了未知容器 {id}（{found:?}）"
            );
        }
        for (id, _) in &errors {
            assert!(
                found.iter().any(|(x, _)| x == id),
                "失败态站点归属到了未知容器 {id}（{found:?}）"
            );
        }
    }

    /// 空态 `colspan` == 表头列数（`#97`/`#162` 事故的原始形态：加列时改表头忘了改空态）。
    #[test]
    fn empty_rows_match_their_table_column_count() {
        let empties = empty_row_sites(APP_JS);
        for (id, want) in TABLES {
            let (_, n, ..) = empties
                .iter()
                .find(|(x, ..)| x == id)
                .unwrap_or_else(|| panic!("{id} 没有空态（emptyRow）站点"));
            assert_eq!(n, want, "{id} 空态 colspan={n}，表头是 {want} 列");
        }
    }

    /// 失败态 `colspan` == 表头列数。空态与失败态**分开断言**：一条断言里同时查两者时，
    /// 只坏一处也会让整条测试红，A/B（逐项回退）就分不清是哪一处被拒。
    #[test]
    fn error_rows_match_their_table_column_count() {
        let errors = error_row_sites(APP_JS);
        for (id, want) in TABLES {
            let (_, n) = errors
                .iter()
                .find(|(x, _)| x == id)
                .unwrap_or_else(|| panic!("{id} 没有失败态（loadErrorRow）站点"));
            assert_eq!(n, want, "{id} 失败态 colspan={n}，表头是 {want} 列");
        }
    }

    /// 行模板每列恰好一个 `<td>`：把「整行单元格」（详情行那种带 `colspan` 的）剔除后，
    /// `<td>` 数必须等于表头列数。少一个 = 真行里那一列没有单元格 —— 空态数对了也没用。
    #[test]
    fn row_templates_have_one_cell_per_column() {
        let empties = empty_row_sites(APP_JS);
        for (id, want) in TABLES {
            let (_, _, tds, colspan_tds) = empties
                .iter()
                .find(|(x, ..)| x == id)
                .unwrap_or_else(|| panic!("{id} 没有空态（emptyRow）站点"));
            assert_eq!(
                tds - colspan_tds,
                *want,
                "{id} 行模板有 {} 个 <td>（另有 {colspan_tds} 个整行单元格），表头是 {want} 列",
                tds - colspan_tds
            );
        }
    }

    /// 写死的 `colspan="N"`（详情行的写法）同样必须等于其所属表格的列数。
    ///
    /// 目前全仓库只允许存在一处写死形式（市场表详情行）；`index.html` 一律走
    /// `emptyRow`/`loadErrorRow`，静态页里冒出写死 `colspan` 即失败。
    #[test]
    fn detail_row_colspan_matches_its_table() {
        let mk = cols().into_iter().find(|(x, _)| x == "mk-body").unwrap().1;
        let literals = literal_colspans(APP_JS);
        let html_literals = literal_colspans(INDEX_HTML);
        assert!(
            html_literals.is_empty(),
            "index.html 里出现了写死的 colspan={html_literals:?}：静态表应只用 emptyRow/loadErrorRow"
        );
        assert_eq!(
            literals,
            vec![mk],
            "app.js 里应恰好有一个写死的 colspan（市场表详情行，= {mk}），实际 {literals:?}"
        );
    }

    /// 交易表是**动态**列数：它的空态必须由 `columns.length` 导出，而不是抄一个数字。
    #[test]
    fn the_dynamic_table_derives_its_colspan_from_the_column_list() {
        assert!(
            APP_JS.contains("<td colspan=\"' + columns.length + '\""),
            "交易表的空态应使用 columns.length 派生 colspan（抄死数字会随列数漂移）"
        );
        // 反证：交易表的失败态属于 div 容器，只能是裸 div 形态
        let err = error_row_sites(APP_JS);
        assert!(
            !err.iter().any(|(id, _)| id == "tx-table"),
            "#tx-table 是 div 容器，不该用 loadErrorRow（<tr> 进 div 无效）：{err:?}"
        );
    }

    /// 容器形态契约：`<tbody>` 容器不得注入裸 `<div>`（浏览器会把它提升到表外），
    /// 且 `loadErrorRow` 只允许出现在 `<tbody>` 容器上。
    ///
    /// ⚠️ 反向（「div 容器必须用 loadErrorHtml」）**不做逐站点文本断言**：加额申请表的容器
    /// 是以变量传入的（`const el = $("#raise-requests")`），纯文本扫描解析不到变量。
    /// 该方向由「`loadErrorRow` 的容器集合 == tbody 集合」这条更强的不变量覆盖
    /// （见 `every_live_state_site_belongs_to_a_known_table` + `NON_TBODY_LIVE_CONTAINERS` 对照）。
    #[test]
    fn tbody_containers_get_rows_and_never_a_bare_div() {
        for (id, _) in TABLES {
            let row_form = format!("setLiveError($(\"#{id}\"), loadErrorRow(");
            assert!(
                APP_JS.contains(&row_form),
                "{id} 是 <tbody> 容器，降级态必须用 loadErrorRow（该函数的注释写明：\
                 裸 <div> 会被浏览器提升到表外、重试按钮脱离容器委托）"
            );
            let div_form = format!("setLiveError($(\"#{id}\"), loadErrorHtml(");
            assert!(
                !APP_JS.contains(&div_form),
                "{id} 是 <tbody> 容器，却注入了裸 <div> 形态的 loadErrorHtml"
            );
        }
        // 非 <tbody> 容器（以 $("#id") 字面量传入的那些）仍应是裸 div 形态
        for id in NON_TBODY_LIVE_CONTAINERS {
            let div_form = format!("setLiveError($(\"#{id}\"), loadErrorHtml(");
            assert!(APP_JS.contains(&div_form), "#{id} 的降级态应是裸 div 形态");
        }
        // 加额申请表在 index.html 里必须是 div（不是 tbody）；它的 setLiveError 用变量传容器
        assert!(
            INDEX_HTML.contains("<div id=\"raise-requests\">"),
            "raise-requests 应是 div 容器：它自带 <table> 标记，注入的也是 <div> 形态空态"
        );
        assert!(
            APP_JS.contains("setLiveError(el, loadErrorHtml("),
            "raise-requests 的降级态应走变量容器 + loadErrorHtml"
        );
    }
}
