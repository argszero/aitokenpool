//! 用量明细保留门禁（rant `2026-10-09T12:28:58` 的独立余项）：`usage_records` 可以被删，
//! 但**不许删到读者答不出来**。
//!
//! `transactions` 的删除切片（`tx_retention_gate`）靠一条别的理由立足：真实明细的窗口要能用
//! **分钟**表达，而窗口外那段历史另需载体 —— 交易页改从读模型 `tx_facts` 取数之后，它由
//! **分钟汇总行**承担。`usage_records` 没有那层 —— 它的**每一个**生产读者
//! 都是写死的**月/日**窗口（`routes/ops.rs` 的本月与当天、`routes/admin.rs` 的三张用量报表、
//! `routes/org.rs` 的部门本月已用），于是只需一条更简单的不变量：
//!
//! **删除门槛 ≤ 每一个读者的时间窗下界。**
//!
//! 两侧都**派生**，不写快照：
//!
//! * 读者名册从 `src/routes/*.rs` 现读 —— 每个 `FROM|JOIN usage_records` 是一条读者，它的
//!   下界取同一句 SQL 里的 `time >= <表达式>`；**下界读不出来的站点不会被悄悄丢掉**，它进
//!   `Reading.unpaired` 并让判决失败（R5）；
//! * 门槛从 `src/usage_retention.rs` 的单一拼写点 `KEEP_SINCE` 读。
//!
//! # 六条规则
//!
//! * **R1（结构）**：门槛的日历修饰符列表是**每个读者**下界的**延长**（读者是它的前缀）。
//!   即「门槛 = 读者自己的那个日历点再往前推」—— 这正是「同一套日历谓词」的形状，
//!   于是「门槛 ≤ 读者的下界」不必论证天数（一个月 28/29/30/31 天都自动成立）。
//! * **R2（数值 + 同族）**：两侧都是 `date('now', …)` 的**同一族**，且门槛**严格早于**
//!   每个读者的下界（用真 SQLite 现算一次比较，不看修饰符的字面意思 —— `+1 month` 这种
//!   「延长了修饰符却让日期变晚」的写法只有数值能抓）。
//! * **R3（fail-closed）**：删除 helper 在**发出 `DELETE` 之前**必须挡住「没有归档水位」与
//!   「没有批次」两种情形。没有它，`[archive]` 一关就会把 `cost`（**只有这张表**带的
//!   锚定货币金额）删得只剩不存在。
//! * **R4（唯一删除点）**：`DELETE FROM usage_records` 在生产代码里**只有一处** —— 删除是
//!   一个决定，多一处就是多一个没人守的入口。
//! * **R5（名册完整）**：一个命中读者形态、下界却读不出来的站点**不许被静默跳过** —— 它记进
//!   `Reading.unpaired`，判决因此失败。R1/R2 只在名册上量化，名册少一条就少一条约束：跳过
//!   等于把不变量偷偷读成「门槛 ≤ **我解析得出来**的每个读者」。
//! * **R6（边界可判）**：生产区只在**引出模块**的 `#[cfg(test)]` 处结束。不引出模块的那种
//!   （它只标记单条测试期 helper）**不截断**生产区 —— 按它截断会把它下面每一个读者从名册里
//!   抹掉，而抹掉发生在名册建好之前，R5 判的是建好的名册，看不见（#450，与 #448 同族、深一层）。
//!   这类标记出现在**取事实的语料**（`routes/` 与 `usage_retention.rs`）里就让判决失败：
//!   那段文本里混着测试期代码，门禁不该猜哪几行是生产。
//!
//! # 射程（如实）
//!
//! * **词法的**：它证「源码里这些形状成立」，**不证**删除真的只删对 —— 行为由
//!   `usage_retention.rs::tests` 五条（门槛是上月起点 / 只删已归档且过门槛 / 未归档不删 /
//!   fail-closed / 有界批次）与 `main.rs` 的调度测试兜。
//! * **不证归档件还在**：归档文件的保留窗口是**字节**口径（`max_files × max_file_size`），
//!   与删除门槛的**日历**口径不可比，静态门禁表达不了「几个月的数据装得进那几个文件吗」。
//!   这条边界写在 `usage_retention.rs` 的模块文档里（默认值下 ~500MB ≫ 两个月的行）。
//! * **不写快照**：两侧期望值都从制品推导（下界取自路由 SQL，门槛取自常量）。
//! * **R6 的量程是「取事实的语料」，不是每个文件**：`#[cfg(test)]` 可以合法地只标记单条
//!   测试期助手，而本仓有几条这样的助手必须挂在模块级（`db::seed_test_users` 被 `routes`
//!   的测试用、`tx_facts::view_columns` / `tx_rollup::written_columns` 被门禁模块用），把它们
//!   塞进各自的 `mod tests` 是另一件事。其余文件只贡献 `DELETE` 计数：那里的单条标记**不再**
//!   截断生产区（「悄悄丢东西」那条路已封），其内部文本按生产代码计入 —— 测试期里若出现第二处
//!   `DELETE FROM usage_records`，R4 会响亮地变红，而不是被截掉。
//! * **阳性对照**：`the_scanner_sees_both_sides` 断言扫描器真的读到了读者、门槛与那个 helper，
//!   且名册里**没有**被跳过的站点 —— 否则「零违规」与「扫描器是瞎的」在读数上无法区分（坑 #814）。

use crate::body_limit_gate::source_files;
use crate::deploy_gate::code_mask;

/// 读者所在的目录（相对 `src/`）。
const ROUTES_PREFIX: &str = "routes/";

/// 门槛与删除 helper 所在的文件（相对 `src/`）。它与 [`ROUTES_PREFIX`] 一起构成门禁**取事实**
/// 的语料：名册来自前者，门槛/helper 来自后者。
const RETENTION_FILE: &str = "usage_retention.rs";

/// 读者语句里出现的两张形态 —— 一条读者就是一个 `FROM`/`JOIN usage_records`。
const READER_FROM: &str = "FROM usage_records";
const READER_JOIN: &str = "JOIN usage_records";

/// 读者下界的写法：`time >= <表达式>`（`ur.time >= …` 也含这个子串）。
const BOUND: &str = "time >= ";

/// 门槛的唯一拼写点（`src/usage_retention.rs`）。
const THRESHOLD_CONST: &str = "pub const KEEP_SINCE: &str = \"";

/// 删除 helper 与它的删除语句（R3 / R4 的落点）。
const HELPER_FN: &str = "pub fn delete_archived(";
const DELETE_STMT: &str = "DELETE FROM usage_records";

/// 测试期属性的字面量。它是**属性**，不是模块：只有后面跟着 `mod …` 的那一个才是文件边界
/// （#450；判定见 [`production_region`]）。
const TESTS_MARK: &str = "#[cfg(test)]";

/// 一条生产读者。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Reader {
    /// 出处（`routes/ops.rs` 之类），出问题时报出来。
    file: String,
    /// 命中位置（掩码文本里的下标）—— 只用来给同一条读者去重，不参与判决。
    at: usize,
    /// 下界表达式原文（`date('now', 'start of month')`）。
    expr: String,
    /// 解析出的日历修饰符（`["start of month"]`）；解析不了 = `None`（判决会响亮地失败）。
    mods: Option<Vec<String>>,
}

/// 一次扫描的读数 —— 判决与它的证据一起产出。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Reading {
    /// 生产读者名册。
    readers: Vec<Reader>,
    /// 门槛表达式（`usage_retention.rs` 的单一拼写点）；读不到 = `None`。
    threshold: Option<String>,
    /// 门槛的日历修饰符；解析不了 = `None`。
    threshold_mods: Option<Vec<String>>,
    /// 删除 helper 在 `DELETE` 之前是否挡住「无水位 / 无批次」。
    fail_closed: bool,
    /// 生产代码里 `DELETE FROM usage_records` 的出现次数。
    delete_sites: usize,
    /// 扫描器是否真的看见了那个 helper。
    helper_seen: bool,
    /// 命中读者形态、但下界读不出来的站点（`文件@掩码字节偏移`）—— R5 的落点。
    ///
    /// 空 = 名册完整；非空 = 有读者被跳过了，而 R1/R2 只在名册上量化（#448）。
    unpaired: Vec<String>,
    /// 取事实的语料里**不引出模块**的 `#[cfg(test)]`（`文件@掩码字节偏移`）—— R6 的落点。
    ///
    /// 空 = 生产区的边界都是真的模块边界；非空 = 那段文本里混着测试期代码，
    /// 「哪几行算生产」只能靠猜（#450）。
    bare_markers: Vec<String>,
}

impl Reading {
    /// R1：门槛的修饰符列表是每个读者下界的**延长**（读者是它的前缀）。
    fn r1(&self) -> bool {
        let Some(t) = &self.threshold_mods else {
            return false;
        };
        !self.readers.is_empty()
            && self.readers.iter().all(|r| {
                r.mods
                    .as_ref()
                    .map(|m| m.len() <= t.len() && t[..m.len()] == m[..])
                    .unwrap_or(false)
            })
    }

    /// R2：同族（`date('now', …)`）且门槛**严格早于**每个读者的下界（真 SQLite 现算）。
    fn r2(&self) -> bool {
        let Some(t) = &self.threshold else {
            return false;
        };
        if !is_calendar(t) || self.readers.is_empty() {
            return false;
        }
        self.readers
            .iter()
            .all(|r| is_calendar(&r.expr) && strictly_earlier(t, &r.expr).unwrap_or(false))
    }

    /// R3：fail-closed。
    fn r3(&self) -> bool {
        self.fail_closed
    }

    /// R4：删除语句唯一（且那一处就是被扫到过的 helper 所在的文件）。
    fn r4(&self) -> bool {
        self.helper_seen && self.delete_sites == 1
    }

    /// R5：名册完整 —— 没有站点因为「下界读不出来」而被静默跳过。
    fn r5(&self) -> bool {
        self.unpaired.is_empty()
    }

    /// R6：边界可判 —— 取事实的语料里没有「不引出模块」的 `#[cfg(test)]`。
    fn r6(&self) -> bool {
        self.bare_markers.is_empty()
    }

    fn all(&self) -> bool {
        self.r1() && self.r2() && self.r3() && self.r4() && self.r5() && self.r6()
    }

    fn blame(&self) -> String {
        format!(
            "r1={} r2={} r3={} r4={} r5={} r6={} · 读者 {:?} · 门槛 {:?} 修饰符 {:?} · fail-closed {} · 删除点 {} · helper {} · 被跳过的站点 {:?} · 非模块标记 {:?}",
            self.r1(),
            self.r2(),
            self.r3(),
            self.r4(),
            self.r5(),
            self.r6(),
            self.readers
                .iter()
                .map(|r| format!("{}@{}", r.file, r.expr))
                .collect::<Vec<_>>(),
            self.threshold,
            self.threshold_mods,
            self.fail_closed,
            self.delete_sites,
            self.helper_seen,
            self.unpaired,
            self.bare_markers,
        )
    }
}

/// `date('now', …)` 同族判定（不做数值判断 —— 那是 [`strictly_earlier`] 的事）。
fn is_calendar(expr: &str) -> bool {
    expr.trim().starts_with("date('now'")
}

/// 从 `date('now', 'm1', 'm2')` 里取出修饰符列表（引号剥掉、按出现顺序）。
fn mods_of(expr: &str) -> Option<Vec<String>> {
    let rest = expr.trim().strip_prefix("date(")?.strip_suffix(')')?;
    let parts: Vec<&str> = rest.split(',').map(str::trim).collect();
    if parts.first() != Some(&"'now'") {
        return None;
    }
    Some(
        parts[1..]
            .iter()
            .map(|p| p.trim_matches('\'').to_string())
            .collect(),
    )
}

/// `a` 是否**严格早于** `b` —— 交给真 SQLite 现算（两侧都是 `date('now', …)` 表达式，
/// 同一时刻求值 ⇒ 与「今天是几号」无关）。读不懂 = `Err`（判决失败，而不是默认通过）。
fn strictly_earlier(a: &str, b: &str) -> Result<bool, rusqlite::Error> {
    let conn = rusqlite::Connection::open_in_memory()?;
    conn.query_row(&format!("SELECT (({a}) < ({b}))"), [], |r| {
        r.get::<_, i64>(0)
    })
    .map(|v| v == 1)
}

/// 从一段 SQL 里取 `time >= <表达式>` 的表达式（配平括号，含 `date(` 前缀）。
fn bound_expr(seg: &str) -> Option<String> {
    let i = seg.find(BOUND)?;
    let after = &seg[i + BOUND.len()..];
    let start = after.find("date(")?;
    let after = &after[start..];
    let open = after.find('(')?;
    let mut depth = 0i32;
    for (k, ch) in after[open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(after[..open + k + 1].trim().to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// 一条 SQL 语句里的读者下界：从 `usage_records` 那个位置往后找，**不越出当前字符串字面量**。
///
/// 「不越界」是判据的一部分：`routes/` 里每个读者各是一段字符串字面量，越过它的 `"` 之后
/// 就是别的语句 —— 那会让「找不到下界」这件事看起来像「找到了别人的下界」。
fn reader_bound(src: &str, at: usize) -> Option<String> {
    let tail = &src[at..];
    let stop = tail.find('"').unwrap_or(tail.len());
    bound_expr(&tail[..stop])
}

/// 一段掩码文本的生产区：**第一个引出模块的** `#[cfg(test)]` 之前的部分，以及这段文本里
/// **不引出模块**的那些标记的偏移。
///
/// 为什么不能按第一个 `#[cfg(test)]` 一律截断（#450）：那是一个**属性**，可以只标记单条测试期
/// helper（`#[cfg(test)] fn fixture() …`）。按它截断会让它下面每一个读者从名册里消失 —— 而消失
/// 发生在名册建好之前，R5 判的是建好的名册，看不见（与 #448 同族、深一层）。
fn production_region(masked: &str) -> (&str, Vec<usize>) {
    let mut bare = Vec::new();
    let mut from = 0usize;
    while let Some(i) = masked[from..].find(TESTS_MARK) {
        let at = from + i;
        if attributes_a_module(masked, at) {
            return (&masked[..at], bare);
        }
        bare.push(at);
        from = at + TESTS_MARK.len();
    }
    (masked, bare)
}

/// `at` 处的 `#[cfg(test)]` 是不是在标记一个模块（`mod tests {` / `mod tests;`）。
///
/// 只看标记之后**第一行有内容的代码**：空行与属性行（`#[allow(…)]`）跳过（注释已被掩码成
/// 空白），再剥掉可见性前缀（`pub`、`pub(crate)`、`pub(super)` …）。写成同一行的
/// `#[cfg(test)] mod tests {` 也一样判得出来。
fn attributes_a_module(masked: &str, at: usize) -> bool {
    let tail = &masked[at + TESTS_MARK.len()..];
    for line in tail.lines() {
        let mut l = line.trim();
        if l.is_empty() || l.starts_with("#[") {
            continue;
        }
        if let Some(rest) = l.strip_prefix("pub") {
            let rest = rest.trim_start();
            l = match rest.strip_prefix('(') {
                Some(_) => match rest.find(')') {
                    Some(k) => rest[k + 1..].trim_start(),
                    None => rest,
                },
                None => rest,
            };
        }
        return l
            .strip_prefix("mod ")
            .map(|name| {
                name.trim_start()
                    .starts_with(|c: char| c.is_alphanumeric() || c == '_')
            })
            .unwrap_or(false);
    }
    false
}

/// 扫一遍语料，产出读数。
fn read() -> Reading {
    // 生产者语料（`*_gate.rs` 已被 `source_files` 跳过）；每个文件的**生产区**截止于第一个
    // **引出模块**的 `#[cfg(test)]`（`routes/mod.rs` 的测试里有 `insert into usage_records`，
    // 不是读者）。不引出模块的那种不截断生产区 —— 截断会把下面的读者从名册里抹掉（#450）。
    let mut readers = Vec::new();
    let mut unpaired = Vec::new();
    let mut bare_markers = Vec::new();
    let mut threshold = None;
    let mut fail_closed = false;
    let mut delete_sites = 0usize;
    let mut helper_seen = false;
    for (file, src) in source_files() {
        let masked = code_mask(&src, false);
        let (prod, bare) = production_region(&masked);
        // R6：取事实的两份语料里，一条只标记单条 helper 的 `#[cfg(test)]` 让「哪几行算生产」
        // 变成猜的 —— 记下来，判决失败（#450）。
        if file.starts_with(ROUTES_PREFIX) || file == RETENTION_FILE {
            bare_markers.extend(bare.iter().map(|i| format!("{file}@{i}")));
        }
        delete_sites += prod.matches(DELETE_STMT).count();

        if file == RETENTION_FILE {
            helper_seen = prod.contains(HELPER_FN);
            if let Some(i) = prod.find(THRESHOLD_CONST) {
                let after = &prod[i + THRESHOLD_CONST.len()..];
                if let Some(q) = after.find('"') {
                    threshold = Some(after[..q].to_string());
                }
            }
            // fail-closed 的形状：helper 里**发出 DELETE 之前**的文本必须挡住两种「没有」。
            if let Some(i) = prod.find(HELPER_FN) {
                let body = &prod[i..];
                let head = match body.find(DELETE_STMT) {
                    Some(d) => &body[..d],
                    None => body,
                };
                fail_closed = head.contains("return")
                    && head.contains("batch")
                    && head.contains("archive_watermark")
                    && head.matches("<= 0").count() >= 2;
            }
            continue;
        }

        if !file.starts_with(ROUTES_PREFIX) {
            continue;
        }
        // 每个 `FROM|JOIN usage_records` 是一条读者。两条形态命中**不同的**出现位置，去重
        // 只按位置 —— 按表达式去重会把同一份报表里三条同窗口的读者（`admin.rs` 的成员/模型/
        // 部门）并成一条，名册就不完整了（读者数要能回答「有几处窗口」）。
        for pat in [READER_FROM, READER_JOIN] {
            let mut base = 0usize;
            while let Some(i) = prod[base..].find(pat) {
                let at = base + i;
                base = at + pat.len();
                let Some(expr) = reader_bound(prod, at) else {
                    // R5：**记下来**，不要静默跳过 —— 跳过会让名册少一条约束却仍然全绿（#448）。
                    unpaired.push(format!("{file}@{at}"));
                    continue;
                };
                if readers
                    .iter()
                    .any(|r: &Reader| r.file == file && r.at == at)
                {
                    continue;
                }
                readers.push(Reader {
                    file: file.clone(),
                    at,
                    mods: mods_of(&expr),
                    expr,
                });
            }
        }
    }
    let threshold_mods = threshold.as_deref().and_then(mods_of);
    Reading {
        readers,
        unpaired,
        threshold,
        threshold_mods,
        fail_closed,
        delete_sites,
        helper_seen,
        bare_markers,
    }
}

#[test]
fn the_delete_threshold_never_reaches_past_a_readers_window() {
    let r = read();
    assert!(
        r.r1(),
        "用量明细的删除门槛没有建立在读者的日历谓词之上（门槛的修饰符不再是读者下界的延长）\
         ⇒ 某条读数的窗口会被删空：{}",
        r.blame()
    );
}

#[test]
fn the_delete_threshold_is_the_same_calendar_family_and_strictly_earlier() {
    let r = read();
    assert!(
        r.r2(),
        "删除门槛与读者下界不同族，或没有**严格早于**它 —— 那样「门槛 ≤ 每个读者的下界」就成了一句\
         需要人来看的话（`date('now','start of month','+1 month')` 一样是「延长修饰符」却更晚）：{}",
        r.blame()
    );
}

#[test]
fn the_delete_is_fail_closed_without_an_archive_watermark_or_a_batch() {
    let r = read();
    assert!(
        r.r3(),
        "删除 helper 在发出 DELETE 之前没有挡住「没有归档水位 / 没有批次」⇒ `[archive]` 一关，\
         会把 `usage_records.cost`（只有这张表带的锚定货币金额）删得只剩不存在：{}",
        r.blame()
    );
}

#[test]
fn the_usage_delete_statement_has_exactly_one_site() {
    let r = read();
    assert!(
        r.r4(),
        "`DELETE FROM usage_records` 在生产代码里应当**恰好一处**（一个决定、一个守门人）：{}",
        r.blame()
    );
}

#[test]
fn every_usage_retention_rule_has_a_tooth() {
    // 七条独立牙齿：每条只动一个读数，且被判官点名的**正是它针对的那条规则**。
    let base = read();
    assert!(base.all(), "基线: {}", base.blame());

    // ① 门槛退到「当天」：读者的月窗反而比门槛深 ⇒ R1 与 R2 都会红（R1 是判官）。
    let mut m = base.clone();
    m.threshold_mods = Some(Vec::new());
    m.threshold = Some("date('now')".to_string());
    assert_ne!(base, m, "变异体「门槛退到当天」没改动任何读数");
    assert!(!m.r1(), "门槛退到当天应打翻 R1: {}", m.blame());
    assert!(!m.r2(), "门槛退到当天应打翻 R2: {}", m.blame());
    assert!(!m.all());

    // ② 门槛推到**下个月**：修饰符仍是延长（R1 通过），只有数值比较抓得住（R2 点名）。
    let mut m = base.clone();
    m.threshold_mods = Some(vec!["start of month".into(), "+1 month".into()]);
    m.threshold = Some("date('now','start of month','+1 month')".to_string());
    assert!(m.r1(), "「+1 month」仍是延长，R1 不该被抓: {}", m.blame());
    assert!(!m.r2(), "门槛推到下月应打翻 R2: {}", m.blame());
    assert!(!m.all(), "{}", m.blame());

    // ③ 门槛换成**天数**口径：数值上仍早（R2 通过），但不再是读者那一族的延长（R1 点名）。
    let mut m = base.clone();
    m.threshold_mods = Some(vec!["-40 days".into()]);
    m.threshold = Some("date('now','-40 days')".to_string());
    assert!(m.r2(), "天数门槛数值上仍早，R2 不该被抓: {}", m.blame());
    assert!(
        !m.r1(),
        "天数门槛应打翻 R1（不再是读者日历点的延长）: {}",
        m.blame()
    );
    assert!(!m.all(), "{}", m.blame());

    // ④ 删掉 fail-closed。
    let mut m = base.clone();
    m.fail_closed = false;
    assert!(!m.r3(), "删掉 fail-closed 应打翻 R3: {}", m.blame());
    assert!(!m.all());

    // ⑤ 多一个删除点。
    let mut m = base.clone();
    m.delete_sites += 1;
    assert!(!m.r4(), "多一处删除应打翻 R4: {}", m.blame());
    assert!(!m.all());

    // ⑥ 名册里多一个被跳过的站点（命中了读者形态，下界读不出来）。
    let mut m = base.clone();
    m.unpaired = vec!["routes/ops.rs@3587".to_string()];
    assert!(!m.r5(), "被跳过的站点应打翻 R5: {}", m.blame());
    assert!(!m.all(), "{}", m.blame());

    // ⑦ 取事实的语料里多一个「不引出模块」的 `#[cfg(test)]`：那段文本的边界不再是可判的。
    let mut m = base.clone();
    m.bare_markers = vec!["routes/ops.rs@1234".to_string()];
    assert!(!m.r6(), "非模块的测试期标记应打翻 R6: {}", m.blame());
    assert!(!m.all(), "{}", m.blame());
}

#[test]
fn a_bare_test_marker_is_not_a_module_boundary() {
    // 逐字对照 #450 的形状：生产代码 → 一条 `#[cfg(test)]` 的测试期 helper → 一条**读者**
    // → 真正的模块边界（`#[cfg(test)] mod tests`）。
    let src = "fn a() {}\n\
               #[cfg(test)]\n\
               fn fixture() -> i64 { 0 }\n\
               const Q: &str = \"FROM usage_records WHERE time >= date('now', 'start of month')\";\n\
               #[cfg(test)]\n\
               mod tests {\n\
                   const T: &str = \"FROM usage_records WHERE time >= date('now')\";\n\
               }\n";
    let (prod, bare) = production_region(src);
    assert!(
        prod.contains("FROM usage_records"),
        "生产区被一条单条 helper 的 `#[cfg(test)]` 截断了 —— 它下面的读者会从名册里消失，\
         而消失发生在名册建好之前（正是 #450）: {prod:?}"
    );
    assert!(
        !prod.contains("const T:"),
        "生产区没有停在真正的模块边界上（测试代码算进了生产）: {prod:?}"
    );
    assert_eq!(
        bare.len(),
        1,
        "没记下那个不引出模块的 `#[cfg(test)]`（R6 没有牙齿）: {bare:?}"
    );

    // 反向：**引出模块**的标记照旧截断，且不算违规 —— 换行、属性叠放、同行三种写法。
    for src in [
        "#[cfg(test)]\nmod tests {\n    const Q: &str = \"x\";\n}\n",
        "fn a() {}\n#[cfg(test)]\n#[allow(dead_code)]\npub(crate) mod tests {\n}\n",
        "fn a() {}\n#[cfg(test)] mod tests { }\n",
        "fn a() {}\n#[cfg(test)]\nmod test_support;\n",
    ] {
        let (prod, bare) = production_region(src);
        assert!(
            bare.is_empty(),
            "引出模块的标记被当成了违规: {src:?} → {bare:?}"
        );
        assert!(
            !prod.contains("const Q:") && !prod.contains("mod "),
            "引出模块的标记没有截断生产区: {src:?} → {prod:?}"
        );
    }
}

#[test]
fn the_scanner_sees_both_sides() {
    // 阳性对照：两侧都真的读到了东西，否则「零违规」与「扫描器是瞎的」读数相同（坑 #814）。
    let r = read();
    // 先说**原因**再说后果：一条不引出模块的 `#[cfg(test)]` 会让读者从名册里消失，而名册少了
    // 一条只由下面的条数兜着 —— 那条兜底说不出原因（#450）。
    assert!(
        r.bare_markers.is_empty(),
        "取事实的语料里出现了不引出模块的 `#[cfg(test)]` —— 那段文本里混着测试期代码，门禁不该\
         猜哪几行是生产（把那条 helper 收进 `mod tests`，或让它引出模块）：{:?} · {}",
        r.bare_markers,
        r.blame()
    );
    assert!(
        r.readers.len() >= 6,
        "生产读者一条都没读出来（提取器失真？）：{}",
        r.blame()
    );
    assert!(
        r.unpaired.is_empty(),
        "有站点命中了读者形态、下界却读不出来 —— 它们被跳过、名册里没有它们，而 R1/R2 只在\
         名册上量化（写法变了？见 #448）：{:?} · {}",
        r.unpaired,
        r.blame()
    );
    assert!(
        r.readers.iter().all(|x| x.mods.is_some()),
        "有读者的下界解析不出来（写法变了？解析不了就等于那条读者**没有判据**）：{}",
        r.blame()
    );
    assert!(
        r.threshold_mods.is_some(),
        "门槛的单一拼写点读不出来（`{THRESHOLD_CONST}` 变了？）：{}",
        r.blame()
    );
    assert!(
        r.helper_seen,
        "没看见删除 helper（函数名变了？）：{}",
        r.blame()
    );
    assert!(
        r.delete_sites == 1,
        "`DELETE FROM usage_records` 的计数是 {}（扫描器读错了语料？）：{}",
        r.delete_sites,
        r.blame()
    );
    // 反面：读者里必须**至少两种**下界（月与当天）—— 只有一种说明提取器把某条 SQL 读重了。
    let mut distinct: Vec<String> = r.readers.iter().map(|x| x.expr.clone()).collect();
    distinct.sort();
    distinct.dedup();
    assert!(
        distinct.len() >= 2,
        "读者下界只有一种（提取器把不同 SQL 读成了同一条？）：{:?}",
        distinct
    );
}
