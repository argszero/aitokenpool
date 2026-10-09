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
//! * **删除的边界是「已折叠 ∧ 已归档」**。折叠水位（`transactions_rollup.up_to_id`）与
//!   归档水位（`[archive]` 跟到哪一行）分别保证「汇总里有这份数据」与「原件还在文件里」；
//!   少了任一条，删掉的行就**真的没了**。这一条是形状：`delete_folded` 必须把两界都取上界。
//!
//! # 射程（如实）
//!
//! * **词法的**：它证「源码里这两处形状成立」，**不证**删除真的只删对、也**不证**屏幕上
//!   的数对 —— 行为由 `tx_rollup.rs::tests::delete_folded_*` 四条（窗口/水位/批量/读模型等价）
//!   与 `main.rs` 的调度测试兜。
//! * **看不见 `custom` 区间**：用户自选的时间段可以任意深，静态门禁无从表达；
//!   本门禁只保证**产品自己提供的预设**答得出来。
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

    fn blame(&self) -> String {
        format!(
            "r1={} r2={} r3={} · 预设 {:?} 天 · 窗口 {:?} 天 · 读不懂 {:?} · 只删明细 {} · 折叠界 {} · 归档界 {} · 两界同句 {} · 时间窗 {}",
            self.r1(),
            self.r2(),
            self.r3(),
            self.presets_days,
            self.retain_days,
            self.unreadable,
            self.deletes_details_only,
            self.has_fold_bound,
            self.has_archive_bound,
            self.bounds_combined,
            self.has_time_window,
        )
    }
}

/// 从 `function txRangeParams(` 的函数体里读全部 `MS(<整数乘积>)` 的深度（换算成天）。
///
/// 只认**整数的乘式**（`24`、`24 * 7`、`24 * 30`）：算术一旦含变量或小数，静态门禁就
/// 读不出深度 —— 那时把它记进 `unreadable`，让判决**响亮地**失败，而不是默认通过。
fn presets_days(js: &str) -> (Vec<i64>, Vec<String>) {
    let masked = mask_js(js);
    let Some(start) = masked.find(RANGE_FN) else {
        return (Vec::new(), vec!["找不到 txRangeParams".to_string()]);
    };
    // 函数体：从第一个 `{` 起按花括号配平（掩码后字符串/注释已等长抹平，深度可信）。
    let body = &masked[start..];
    let Some(open) = body.find('{') else {
        return (Vec::new(), vec!["txRangeParams 没有函数体".to_string()]);
    };
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
    let body = &body[open..end];

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
    let (presets, unreadable) = presets_days(include_str!("../ui/js/app.js"));
    let rollup = source_files()
        .into_iter()
        .find(|(name, _)| name.ends_with(ROLLUP_FILE))
        .map(|(_, src)| src)
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
fn the_retention_window_covers_every_preset_the_list_offers_negatives() {
    // 四条独立牙齿：每条只翻一条规则，其余不动。
    let base = read();
    assert!(
        base.r1() && base.r2() && base.r3(),
        "基线: {}",
        base.blame()
    );

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
    ] {
        let mut m = base.clone();
        mutate(&mut m);
        assert_ne!(base, m, "变异体「{name}」没改动任何读数");
        assert!(
            !(m.r1() && m.r2() && m.r3()),
            "变异体「{name}」应当被打翻，却仍全绿: {}",
            m.blame()
        );
    }
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
}
