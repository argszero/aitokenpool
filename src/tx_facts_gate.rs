//! 交易事实读模型门禁（R97）：视图 `tx_facts` 的**拼写、读法、时间窗，以及列表是否真的走它**。
//!
//! `src/tx_facts.rs` 把「已折叠的分钟级汇总行」与「尚未折叠的明细」合成一个读模型视图，
//! 好让**删掉已折叠的明细**对聚合读数不可见。视图本身只是一段 SQL 文本，它兑现承诺
//! 与否由四条**调用侧**的形状决定 —— 四条都不在类型系统里，只能由本门禁守：
//!
//! * **R1 拼写**：视图名只有一个拼写点（`tx_facts::VIEW` 之后**放进 SQL 的那一处**），
//!   生产 SQL 一律经 `tx_facts::source()` 取它。**字符串字面量**里手写视图名 ⇒ 一个
//!   新的载体出现了：改名/换形态时它会静默留在原地（`FROM tx_facts` 这种写法尤其隐蔽，
//!   它不单独成串）。判据是「每个**点了这个名字的字符串字面量**都必须住在
//!   `tx_facts.rs` 里」，因此不靠「名字 = 整个字面量」这个过窄的形状。
//! * **R2 读法**：读视图的每一处都必须**声明它读的是什么粒度**。视图的一行是「一个分钟桶
//!   **或**一条尚未折叠的明细」，**不是**一条调用 ⇒ 说得通的形状只有三种：① **聚合**
//!   （含 `SUM(`）—— `summary` / `trend` / 账本读数 / 共享收益；② **逐行**（含 `row_count`）
//!   —— 分页列表：每行自报它代表多少条真实明细（rant 2026-10-15T16:25:04.530850+08:00
//!   验收项 2「每行带计数」）；③ **数行数**（`COUNT(`）—— 分页的 `total`：数的是「视图有
//!   几行」，即这一页翻得出多少行，与列表**同源**是它成立的前提（验收项 3）。
//!   三者都没有 ⇒ 有人把视图当成「一行一条调用」的表在读（既不聚合、也不自报行数）。
//!   ⚠️ `COUNT(` **曾是**本门禁的牙：旧契约是「分页列表留在原始表上」，读视图只有聚合一种
//!   形状，`COUNT(*)` 只可能是「把桶数当条数」。验收项 2/3 翻转了那个前提 —— 列表现在**必须**
//!   数视图的行数 —— 于是这条牙改守它真正要守的那半：「**条数**一律 `SUM(row_count)`」，
//!   而那已是**事实**（分母：`tx_facts.rs::counting_view_rows_is_not_counting_calls` 钉住
//!   两者不相等），不再是形状。形状侧留下的是**声明粒度**：三者缺一即红。
//!   同理视图没有 `counterpart`（展示用的对手方标识在汇总行上不存在）——读视图的 SQL
//!   一旦点它，说明有人在拿视图当**明细列表**用，而汇总行给不出它。
//! * **R3 时间窗**：读侧的时间界必须过 `tx_facts::minute_aligned()`，且那个截断点**长在
//!   `tx_where` 里**。汇总臂的时间列只有 16 字符（分钟桶），明细臂 19 字符
//!   （`YYYY-MM-DD HH:MM:SS`）；带秒的界会让两条臂对同一个桶给出不同答案（短前缀更小），
//!   于是折叠前后读数不同 —— 而前端发的正是 `toISOString()`（秒几乎从不为 0）。
//! * **R4 列表在视图上**（验收项 2 的**存在性**）：视图必须真的在服务分页列表 —— 一处逐行
//!   读法（带 `row_count`）**且**一处行数统计（`COUNT(`）。R1/R2/R3 全是「不许」，只剩它们时
//!   「列表搬回原始表」这一态照样全绿（聚合读法一处都没动）—— 门禁就守不住本轮的成果。
//!   R4 是它的反面：列表走了，两条见证都没了，红。这正是验收项 5 的变异腿「把列表改回读
//!   原始表，门禁必须变红」。⚠️ 它要的是**见证存在**（每类至少一处），不是把调用点总数钉死。
//!
//! # 射程（如实）
//!
//! * **词法的**：它证「源码里那四处形状成立」，**不证**屏幕/接口上的数对 —— 数值等价由
//!   `tx_facts.rs` 的真库测试（视图 vs 明细，三个折叠状态）与 `cargo test` 一起兜。
//! * 语料 = `body_limit_gate::source_files()`（走盘 `src/**`、跳过门禁模块自身）；
//!   掩码 = `deploy_gate::code_mask()`（**等长**掩码，偏移量与原文一一对应）。
//!   两处都**复用**，不另立一份机械（`js_gate::mask_js` 是同一个先例）。
//! * **不写快照**：期望值全部从制品推导（视图名取自 `tx_facts::VIEW`，语料走盘，
//!   调用点由锚点现找）；读视图的**调用点数刻意不钉死** —— 本门禁遍历全部调用点，
//!   新增读者自动进射程，钉一个数反而会变成第二个需要同步的载体。R4 要的是**见证存在**
//!   （逐行读法、行数统计各至少一处），同样不钉总数。
//! * **阳性对照**：`the_scanner_sees_the_carriers_it_judges` 断言扫描器确实看见了视图名的
//!   那个字面量、至少一个读视图的调用点、以及那一个截断点 —— 否则「零违规」与
//!   「扫描器是瞎的」在读数上无法区分（坑 #814）。

use crate::body_limit_gate::source_files;
use crate::deploy_gate::code_mask;

/// 生产 SQL 取视图名的**唯一形态**：`FROM <视图> <别名>`。
const CALL: &str = "tx_facts::source(";

/// 时间窗的界唯一的截断点。
const ALIGN: &str = "tx_facts::minute_aligned(";

/// 视图名的**唯一拼写点**所在文件（相对 `src/`）。
const OWNER: &str = "tx_facts.rs";

/// 视图必须长在其中的那个函数（时间窗的绑定点）。
const WHERE_FN: &str = "fn tx_where(";

/// 视图上不存在、只属于明细的列 —— 读视图的 SQL 点到它，就是把视图当明细列表用。
const DETAIL_ONLY: &str = "counterpart";

/// 「这一行代表多少条真实明细」的列 —— 逐行读法**必须**带着它（R2 的第二种声明、R4 的第一条见证）。
const ROW_COUNT: &str = "row_count";

/// 一次扫描的读数 —— 判决与它的证据一起产出（一个藏在脚注里的期望会报出自我一致的谎）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Reading {
    /// 字面量里写下视图名的**文件**（按语料顺序，每个文件至多一次）。
    name_homes: Vec<String>,
    /// 字面量里写下视图名的**次数**。
    name_sites: usize,
    /// 读视图的调用点数。
    view_reads: usize,
    /// 读视图却没声明粒度的调用点（应 0）：既不含 `SUM(`、也不读 `row_count`、也不数行数。
    undeclared: Vec<String>,
    /// 读视图且数行数（`COUNT(`）的调用点 —— 分页的 `total` 正是它（R4 的第二条见证）。
    row_counting: Vec<String>,
    /// 读视图且**逐行**读、带着 `row_count` 的调用点 —— 分页列表正是它（R4 的第一条见证）。
    row_carrying: Vec<String>,
    /// 读视图却点了明细专属列的调用点（应 0）。
    detail_only: Vec<String>,
    /// 拿不到外层 `format!(…)` 的调用点（应 0：取不到就等于那个调用点**没有判据**）。
    unreadable: Vec<String>,
    /// 分钟对齐的调用点数（应 1）。
    aligns: usize,
    /// 对齐调用是否落在 `tx_where` 体内。
    aligned_in_where: bool,
}

impl Reading {
    /// R1：视图名只在 `tx_facts.rs` 的**一个字面量**里被写下。
    fn r1(&self) -> bool {
        self.name_sites == 1 && self.name_homes == [OWNER.to_string()]
    }

    /// R2：读视图的每一处都声明了粒度（聚合 / 逐行带 `row_count` / 数行数），不点明细专属列，
    /// 且每处都读得懂。
    fn r2(&self) -> bool {
        self.undeclared.is_empty() && self.detail_only.is_empty() && self.unreadable.is_empty()
    }

    /// R3：时间窗的截断点唯一，且长在 `tx_where` 里。
    fn r3(&self) -> bool {
        self.aligns == 1 && self.aligned_in_where
    }

    /// R4：分页列表真的走读模型 —— 逐行读法（带 `row_count`）与它同源的行数统计都在。
    fn r4(&self) -> bool {
        !self.row_carrying.is_empty() && !self.row_counting.is_empty()
    }

    fn ok(&self) -> bool {
        self.r1() && self.r2() && self.r3() && self.r4()
    }

    fn report(&self) -> String {
        format!(
            "r1={} r2={} r3={} r4={} | 名字面量 {} 处/{} 个文件 {:?} · 读视图 {} 处 · 未声明粒度 {:?} · \
             逐行带 {ROW_COUNT} {:?} · 数行数 {:?} · 明细专属 {:?} · 读不懂 {:?} · \
             对齐 {} 处（在 {WHERE_FN} 内 {}）",
            self.r1(),
            self.r2(),
            self.r3(),
            self.r4(),
            self.name_sites,
            self.name_homes.len(),
            self.name_homes,
            self.view_reads,
            self.undeclared,
            self.row_carrying,
            self.row_counting,
            self.detail_only,
            self.unreadable,
            self.aligns,
            self.aligned_in_where,
        )
    }
}

/// `needle` 在 `hay` 里出现的每个字节偏移。
fn occurrences(hay: &str, needle: &str) -> Vec<usize> {
    hay.match_indices(needle).map(|(i, _)| i).collect()
}

/// 视图名**作为一个孤立的标识符**出现在这段文本里吗？
///
/// `tx_facts_gate`（同名前缀）、`tx_facts.rs`（文件名）、`tx_facts::source`（模块路径）
/// 都不是「在 SQL 里写下视图名」—— 路径与文件名的邻接字符是 `.` 或 `:`，一律不算。
fn names_it(text: &str, name: &str) -> bool {
    let bare = |c: char| c.is_alphanumeric() || c == '_' || c == '.' || c == ':';
    text.match_indices(name).any(|(i, _)| {
        !text[..i].chars().next_back().is_some_and(bare)
            && !text[i + name.len()..].chars().next().is_some_and(bare)
    })
}

/// 每个**字符串字面量**在原文里的字节区间（含两侧引号）。
///
/// 开引号的判据是「`structure` 掩成了空格、而 `content` 还留着那个 `"`」—— 于是注释里的
/// 引号（两份掩码都是空格）与字符串内部的引号（会被下面的转义扫描跳过）都不会被当成开头。
/// 两个掩码都是**等长**的，因此这里报出的偏移量可以直接切原文（坑 #723 的同族：
/// 逐行截断 `//` 的写法会把字符串里的 `//` 当注释）。
fn string_literals(content: &str, structure: &str) -> Vec<(usize, usize)> {
    let (c, s) = (content.as_bytes(), structure.as_bytes());
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < c.len() {
        if c[i] != b'"' || s[i] != b' ' {
            i += 1;
            continue;
        }
        let start = i;
        let mut j = i + 1;
        while j < c.len() {
            if c[j] == b'\\' {
                j += 2;
                continue;
            }
            if c[j] == b'"' {
                break;
            }
            j += 1;
        }
        out.push((start, (j + 1).min(c.len())));
        i = j + 1;
    }
    out
}

/// 在**掩码后**的文本里，从 `open`（指向 `(` 或 `{`）配平到配对的收尾符号，返回收尾之后一位。
///
/// ⚠️ 传进来的必须是掩码后的文本（`code_mask(src, true)`）：SQL 字面量里满是 `(` `)`，
/// 不掩掉它们，配平会落在字符串内部（坑 #723 的同族）。
fn matched(masked: &str, open: usize) -> Option<usize> {
    let b = masked.as_bytes();
    let (o, c) = match *b.get(open)? {
        b'(' => (b'(', b')'),
        b'{' => (b'{', b'}'),
        _ => return None,
    };
    let mut depth = 0usize;
    for (i, ch) in b.iter().enumerate().skip(open) {
        if *ch == o {
            depth += 1;
        } else if *ch == c {
            depth -= 1;
            if depth == 0 {
                return Some(i + 1);
            }
        }
    }
    None
}

/// 包着调用点 `at` 的那个 `format!(…)` 在**原文**里的切片。
///
/// 取不到 ⇒ `None`：调用点没有长在 `format!` 里（例如被拼进了别的字符串），本门禁当场
/// 要求复核，而不是静默跳过 —— 跳过会让那个调用点**没有判据**。
fn enclosing_format<'a>(orig: &'a str, structure: &str, at: usize) -> Option<&'a str> {
    let fmt = structure[..at].rfind("format!")?;
    let open = fmt + structure[fmt..].find('(')?;
    let end = matched(structure, open)?;
    Some(&orig[fmt..end])
}

/// 这段文本里有没有「数行数」的调用（`COUNT(`，且前面不是标识符字符 —— `row_count` 不算）。
fn counts_rows(span: &str) -> bool {
    let lower = span.to_ascii_lowercase();
    lower.match_indices("count(").any(|(i, _)| {
        let prev = i.checked_sub(1).map(|p| lower.as_bytes()[p]);
        !matches!(prev, Some(c) if c.is_ascii_alphanumeric() || c == b'_')
    })
}

/// `at` 是否落在 `structure` 里以 `sig` 为签名那个函数的**函数体**内。
fn in_fn_body(structure: &str, sig: &str, at: usize) -> bool {
    let Some(f) = structure.find(sig) else {
        return false;
    };
    let Some(open) = structure[f..].find('{').map(|i| i + f) else {
        return false;
    };
    match matched(structure, open) {
        Some(end) => at > open && at < end,
        None => false,
    }
}

/// 扫一份语料，产出读数（判决与证据）。
fn judge(corpus: &[(String, String)]) -> Reading {
    let name = crate::tx_facts::VIEW;
    let mut r = Reading::default();
    for (path, text) in corpus {
        // 内容判据：注释掩掉、字符串正文保留（名字面量正是字符串）。
        let content = code_mask(text, false);
        // 结构判据：注释与字符串正文都掩掉 —— 配平括号与「开引号在哪」靠它。
        let structure = code_mask(text, true);

        let named = string_literals(&content, &structure)
            .into_iter()
            .filter(|(s, e)| names_it(&content[*s..*e], name))
            .count();
        r.name_sites += named;
        if named > 0 {
            r.name_homes.push(path.clone());
        }

        for at in occurrences(&structure, CALL) {
            r.view_reads += 1;
            match enclosing_format(text, &structure, at) {
                Some(span) => {
                    // 粒度的三种声明（R2）。逐行读法**必须**带 `row_count` —— 视图的一行是一个
                    // 分钟桶或一条明细，不是一条调用；不自报行数就等于把桶当成一条真调用。
                    let aggregate = span.contains("SUM(");
                    let counting = counts_rows(span);
                    if counting {
                        r.row_counting.push(format!("{path}: {}", one_line(span)));
                    }
                    if span.contains(ROW_COUNT) && !aggregate && !counting {
                        r.row_carrying.push(format!("{path}: {}", one_line(span)));
                    }
                    if !aggregate && !counting && !span.contains(ROW_COUNT) {
                        r.undeclared.push(format!("{path}: {}", one_line(span)));
                    }
                    if span.contains(DETAIL_ONLY) {
                        r.detail_only.push(format!("{path}: {}", one_line(span)));
                    }
                }
                None => r.unreadable.push(format!("{path}: 偏移 {at}")),
            }
        }

        for at in occurrences(&structure, ALIGN) {
            r.aligns += 1;
            if in_fn_body(&structure, WHERE_FN, at) {
                r.aligned_in_where = true;
            }
        }
    }
    r
}

/// 把一段（多行）SQL 压成一行，报错信息才带得上证据。
fn one_line(span: &str) -> String {
    span.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------------------------
// 门禁
// ---------------------------------------------------------------------------------------------

/// 语料 = 生产区（走盘 `src/**`，门禁模块自身不在内）。
fn corpus() -> Vec<(String, String)> {
    source_files()
}

/// **阳性对照**：扫描器确实看见了它要判的三样东西。
///
/// 没有这条，「零违规」与「语料/锚点全都没读进来」在读数上完全一样（坑 #814）。
#[test]
fn the_scanner_sees_the_carriers_it_judges() {
    let r = judge(&corpus());
    assert!(
        r.name_sites >= 1,
        "语料里没读到点名视图名的字符串字面量 —— 名字的声明搬走了？{}",
        r.report()
    );
    assert!(
        r.view_reads >= 1,
        "语料里没读到任何 `{CALL}` 调用点 —— 读视图的查询搬走了？{}",
        r.report()
    );
    assert!(
        r.aligns >= 1,
        "语料里没读到 `{ALIGN}` 调用点 —— 时间窗的截断点搬走了？{}",
        r.report()
    );
    assert!(
        r.aligned_in_where,
        "`{ALIGN}` 不在 `{WHERE_FN}` 体内 —— 时间窗的绑定点搬走了？{}",
        r.report()
    );
    assert!(
        corpus().iter().any(|(p, _)| p == OWNER),
        "语料里没有 `{OWNER}` —— 视图的声明不在射程内，本门禁守的是空气",
    );
    println!("{}", r.report());
}

/// **轴**：视图名只有一个拼写点、读它的每一处都声明了粒度、时间窗只有一个截断点，
/// 且分页列表真的走它。
#[test]
fn the_view_is_named_once_and_read_in_declared_grains() {
    let r = judge(&corpus());
    assert!(
        r.r1(),
        "视图名被写进了不止一个字面量（或不在 `{OWNER}` 里）—— 新的载体要一并纳入本门禁：{}",
        r.report()
    );
    assert!(
        r.r2(),
        "读视图的查询里有没声明粒度的（既不聚合、也不带 `{ROW_COUNT}`、也不数行数）/ \
         点明细专属列 / 读不懂的调用点：{}",
        r.report()
    );
    assert!(
        r.r3(),
        "时间窗的分钟对齐不止一处、或不在 `{WHERE_FN}` 里：{}",
        r.report()
    );
    assert!(
        r.r4(),
        "读模型没有在服务分页列表 —— 逐行读法（带 `{ROW_COUNT}`）与它同源的行数统计（`COUNT(`）\
         必须各有一处：列表搬回原始表了？{}",
        r.report()
    );
}

// ---------------------------------------------------------------------------------------------
// 合成语料（牙用；形状照抄真实语料，去掉一切与判据无关的部分）
// ---------------------------------------------------------------------------------------------

/// 视图名的声明 —— 与真语料同形（`pub const VIEW: &str = …;`）。
fn owner_fixture() -> String {
    format!("pub const VIEW: &str = \"{}\";\n", crate::tx_facts::VIEW)
}

/// 一份合成语料：`tx_facts.rs` 声明视图名；`routes/wallet.rs` 里有一个 `fn_name` 函数
/// （内含分钟对齐）与一个读视图的聚合查询。
///
/// `select` 是那个查询的选择列表，`fn_name` 用来把截断点搬进/搬出 `tx_where`。
/// `with_list` 决定语料里有没有**分页列表的两条见证**（逐行读法带 `row_count` ＋ 行数统计
/// `COUNT(`）—— R4 的变异腿就是把它们去掉。
///
/// ⚠️ 配对必须真的平衡：`format!(…, tx_facts::source("t"))` 是**两**层括号，少一层时
/// `matched()` 报 `None`，症状是「扫描器读不懂语料」——看着像门禁坏了，其实是夹具写错了。
fn fixture_with(fn_name: &str, select: &str, with_list: bool) -> Vec<(String, String)> {
    let list = if with_list {
        format!(
            "fn tx_list(...) {{\n    \
             let sql = format!(\"SELECT t.id, t.{ROW_COUNT} FROM {{}} WHERE {{w}}\", {CALL}\"t\"));\n}}\n\
             fn tx_total(...) {{\n    \
             let sql = format!(\"SELECT COUNT(*) FROM {{}} WHERE {{w}}\", {CALL}\"t\"));\n}}\n"
        )
    } else {
        String::new()
    };
    vec![
        (OWNER.to_string(), owner_fixture()),
        (
            "routes/wallet.rs".to_string(),
            format!(
                "fn {fn_name}(...) {{\n    \
                 binds.push(rusqlite::types::Value::Text({ALIGN}s)));\n}}\n\
                 fn summary(...) {{\n    \
                 let sql = format!(\"SELECT {select} FROM {{}} {{}} WHERE {{w}}\", {CALL}\"t\"));\n}}\n\
                 {list}"
            ),
        ),
    ]
}

/// 带分页列表见证的合成语料（各条 R1/R2/R3 牙都建在它上面，于是那些牙只翻自己那一条）。
fn fixture(fn_name: &str, select: &str) -> Vec<(String, String)> {
    fixture_with(fn_name, select, true)
}

/// 对照语料（全绿）。
fn good_fixture() -> Vec<(String, String)> {
    fixture("tx_where", "COALESCE(SUM(t.pts), 0)")
}

/// **牙（对照）**：合成语料本该全绿 —— 否则下面几条「红了」不知道红在谁身上。
#[test]
fn the_synthetic_corpus_starts_green() {
    let r = judge(&good_fixture());
    assert!(r.ok(), "对照语料本该全绿：{}", r.report());
}

/// **牙（R1）**：另一个文件把视图名写进 SQL 字面量（且**不是**完整的一句话）⇒ 只翻 R1。
#[test]
fn a_second_file_writing_the_view_name_reddens_the_spelling_rule() {
    let mut c = good_fixture();
    c.push((
        "routes/ops.rs".to_string(),
        format!(
            "let sql = \"SELECT SUM(pts) FROM {}\";\n",
            crate::tx_facts::VIEW
        ),
    ));
    let r = judge(&c);
    assert!(
        !r.r1() && r.r2() && r.r3() && r.r4(),
        "字面量里手写的视图名没被点名（且只该点这一条）：{}",
        r.report()
    );
}

/// **牙（R1）**：同一个文件里第二次写下它也算 —— 拼写点应当唯一。
#[test]
fn a_second_literal_in_the_owner_file_reddens_the_spelling_rule() {
    let mut c = good_fixture();
    c[0].1.push_str(&format!(
        "pub const ALIAS: &str = \"{}\";\n",
        crate::tx_facts::VIEW
    ));
    let r = judge(&c);
    assert!(
        !r.r1() && r.r2() && r.r3() && r.r4(),
        "第二个拼写点没被点名：{}",
        r.report()
    );
}

/// **牙（R1）**：只是**提了一句**模块名（`tx_facts.rs` 这种）不算拼写视图名。
#[test]
fn mentioning_the_module_file_name_is_not_spelling_the_view() {
    let mut c = good_fixture();
    c.push((
        "router.rs".to_string(),
        "let p = \"tx_facts.rs\";\n".to_string(),
    ));
    let r = judge(&c);
    assert!(r.r1(), "把模块文件名当成了视图名的拼写点：{}", r.report());
}

/// **牙（R2）·对照**：`COUNT(*)` 今天是**合法声明** —— 分页的 `total` 数的是「视图有几行」，
/// 而它与列表同源正是验收项 3 的要求。这一条钉住那次翻转（旧契约下它是红的）：
/// `COUNT(` 不再是违规，「数**条数**必须 `SUM(row_count)`」那半归**事实**管，分母是
/// `tx_facts.rs::counting_view_rows_is_not_counting_calls`。
#[test]
fn counting_the_views_rows_is_a_declared_grain() {
    let c = fixture("tx_where", "COUNT(*)");
    let r = judge(&c);
    assert!(r.ok(), "`COUNT(*)` 被误判成违规：{}", r.report());
}

/// **牙（R2）·对照**：`SUM(row_count)` 是**聚合**声明，不是逐行读法，也不该被当成数行数。
#[test]
fn summing_the_row_count_is_a_declared_grain() {
    let c = fixture(
        "tx_where",
        "COALESCE(SUM(t.pts), 0), COALESCE(SUM(t.row_count), 0)",
    );
    let r = judge(&c);
    assert!(r.ok(), "`SUM(row_count)` 被误判：{}", r.report());
}

/// **牙（R2）**：读视图却点了明细专属列 ⇒ 有人拿视图当明细列表用。
#[test]
fn a_view_reader_that_touches_a_detail_only_column_reddens() {
    let c = fixture("tx_where", "COALESCE(SUM(t.pts), 0), t.counterpart");
    let r = judge(&c);
    assert!(
        !r.r2() && r.r1() && r.r3() && r.r4(),
        "明细专属列没被点名：{}",
        r.report()
    );
}

/// **牙（R2）**：读视图却**不声明粒度** —— 逐行取列却不带 `row_count`（既不聚合、也不数行数）
/// ⇒ 有人把视图当成「一行一条调用」的表在读。
#[test]
fn a_view_reader_that_declares_no_grain_reddens() {
    let c = fixture("tx_where", "t.id, t.pts, t.type");
    let r = judge(&c);
    assert!(
        !r.r2() && r.r1() && r.r3() && r.r4(),
        "没声明粒度的读法没被点名：{}",
        r.report()
    );
}

/// **牙（R3）**：截断点搬出 `tx_where` ⇒ 只翻 R3。
#[test]
fn an_aligner_outside_the_where_builder_reddens() {
    let c = fixture("somewhere_else", "COALESCE(SUM(t.pts), 0)");
    let r = judge(&c);
    assert!(
        !r.r3() && r.r1() && r.r2() && r.r4(),
        "截断点不在 `{WHERE_FN}` 里却没被点名：{}",
        r.report()
    );
}

/// **牙（R4）**：分页列表搬回原始表 —— 语料里不再有逐行读法、也没有行数统计 ⇒ 只翻 R4。
///
/// 这是验收项 5 的变异腿在门禁侧的对照：R1/R2/R3 照旧全绿（聚合读法一处都没动），
/// 只有 R4 知道列表已经不在读模型上了。
#[test]
fn a_list_that_leaves_the_view_reddens_the_list_rule() {
    let c = fixture_with("tx_where", "COALESCE(SUM(t.pts), 0)", false);
    let r = judge(&c);
    assert!(
        !r.r4() && r.r1() && r.r2() && r.r3(),
        "列表搬走之后 R4 没红（其余三条本该照旧全绿）：{}",
        r.report()
    );
}

/// **牙（R4）**：只把**行数统计**搬回原始表（`total` 不再数视图的行数）⇒ 同样只翻 R4 ——
/// 「列表与 `total` 同源」是两条见证，各有一条牙。
#[test]
fn a_total_that_leaves_the_view_reddens_the_list_rule() {
    let mut c = good_fixture();
    c[1].1 = c[1].1.replace("SELECT COUNT(*)", "SELECT SUM(t.pts)");
    let r = judge(&c);
    assert!(
        !r.r4() && r.r1() && r.r2() && r.r3(),
        "`total` 搬走之后 R4 没红：{}",
        r.report()
    );
}

/// **牙（掩码）**：注释里逐字引用的视图名与读法**不是**载体。
///
/// 本仓的注释到处在逐字引用代码（本模块的文档就是一例）；不掩码的话，一条写着
/// 「旧写法：`FROM tx_facts`」的注释会把已经删掉的载体种回语料里（坑 #723 同族）。
#[test]
fn a_quoted_carrier_in_a_comment_is_not_a_carrier() {
    let mut c = good_fixture();
    let quoted = format!(
        "// 旧实现：let sql = \"SELECT COUNT(*) FROM {} {CALL}\\\"t\\\")\";\n",
        crate::tx_facts::VIEW
    );
    c[1].1.insert_str(0, &quoted);
    let r = judge(&c);
    assert!(r.ok(), "注释里逐字引用的载体被当成了载体：{}", r.report());
}
