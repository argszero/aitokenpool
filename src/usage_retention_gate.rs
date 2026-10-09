//! 用量明细保留门禁（rant `2026-10-09T12:28:58` 的独立余项）：`usage_records` 可以被删，
//! 但**不许删到读者答不出来**。
//!
//! `transactions` 的删除切片（`tx_retention_gate`）靠一条别的理由立足：列表要逐行身份，所以
//! 窗口必须盖住列表自己提供的预设区间。`usage_records` 没有那层 —— 它的**每一个**生产读者
//! 都是写死的**月/日**窗口（`routes/ops.rs` 的本月与当天、`routes/admin.rs` 的三张用量报表、
//! `routes/org.rs` 的部门本月已用），于是只需一条更简单的不变量：
//!
//! **删除门槛 ≤ 每一个读者的时间窗下界。**
//!
//! 两侧都**派生**，不写快照：
//!
//! * 读者名册从 `src/routes/*.rs` 现读 —— 每个 `FROM|JOIN usage_records` 是一条读者，它的
//!   下界取同一句 SQL 里的 `time >= <表达式>`；
//! * 门槛从 `src/usage_retention.rs` 的单一拼写点 `KEEP_SINCE` 读。
//!
//! # 三条规则
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
//! * **阳性对照**：`the_scanner_sees_both_sides` 断言扫描器真的读到了读者、门槛与那个 helper
//!   —— 否则「零违规」与「扫描器是瞎的」在读数上无法区分（坑 #814）。

use crate::body_limit_gate::source_files;
use crate::deploy_gate::code_mask;

/// 读者所在的目录（相对 `src/`）。
const ROUTES_PREFIX: &str = "routes/";

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

/// 读者源码里生产区的结束标记（之后是 `#[cfg(test)] mod tests`）。
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

    fn all(&self) -> bool {
        self.r1() && self.r2() && self.r3() && self.r4()
    }

    fn blame(&self) -> String {
        format!(
            "r1={} r2={} r3={} r4={} · 读者 {:?} · 门槛 {:?} 修饰符 {:?} · fail-closed {} · 删除点 {} · helper {}",
            self.r1(),
            self.r2(),
            self.r3(),
            self.r4(),
            self.readers
                .iter()
                .map(|r| format!("{}@{}", r.file, r.expr))
                .collect::<Vec<_>>(),
            self.threshold,
            self.threshold_mods,
            self.fail_closed,
            self.delete_sites,
            self.helper_seen,
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

/// 扫一遍语料，产出读数。
fn read() -> Reading {
    // 生产者语料（`*_gate.rs` 已被 `source_files` 跳过）；每个文件的**生产区**截止于
    // `#[cfg(test)]`（`routes/mod.rs` 的测试里有 `insert into usage_records`，不是读者）。
    let mut readers = Vec::new();
    let mut threshold = None;
    let mut fail_closed = false;
    let mut delete_sites = 0usize;
    let mut helper_seen = false;
    for (file, src) in source_files() {
        let masked = code_mask(&src, false);
        let prod = match masked.find(TESTS_MARK) {
            Some(i) => &masked[..i],
            None => &masked[..],
        };
        delete_sites += prod.matches(DELETE_STMT).count();

        if file == "usage_retention.rs" {
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
        threshold,
        threshold_mods,
        fail_closed,
        delete_sites,
        helper_seen,
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
    // 五条独立牙齿：每条只动一个读数，且被判官点名的**正是它针对的那条规则**。
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
}

#[test]
fn the_scanner_sees_both_sides() {
    // 阳性对照：两侧都真的读到了东西，否则「零违规」与「扫描器是瞎的」读数相同（坑 #814）。
    let r = read();
    assert!(
        r.readers.len() >= 6,
        "生产读者一条都没读出来（提取器失真？）：{}",
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
