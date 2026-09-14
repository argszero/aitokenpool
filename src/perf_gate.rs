//! 时间谓词门禁（C2116）：**时间列不得被日期函数包住**。
//!
//! 起因（rant 2026-09-14T16:51:14）：月聚合查询在 NAS 上要 4~10s。真因不是 NAS 吞吐
//! （实测 NAS 4KB 热读 1.9µs，与本地盘同级），而是**一次查询要摸多少页** ——
//! `strftime('%Y-%m', time) = strftime('%Y-%m', 'now')` 把索引列包在函数里 ⇒
//! SQLite 无法用 `time` 上的索引 ⇒ 全表扫描。同库同查询实测：
//! `strftime` 3845ms → 范围谓词 2321ms → 覆盖索引 119ms（18~32x）。
//!
//! 修法是把谓词改写成**闭区间**（语义等价且索引可用）：
//!
//! ```text
//! strftime('%Y-%m', X) = strftime('%Y-%m', 'now')
//!   ⟺  X >= date('now','start of month') AND X < date('now','start of month','+1 month')
//! date(X) = date('now')
//!   ⟺  X >= date('now') AND X < date('now','+1 day')
//! ```
//!
//! ⚠️ **只写左界不等价**（会放进未来月份的记录）—— 必须写**闭区间**。
//!
//! 为什么需要门禁：这个缺陷**改前改后测试全绿**（聚合值一样，只是慢了 30 倍），
//! 唯一的运行期症状是「慢」，而慢在本机（SSD、2.8MB 库）根本看不出来 —— 只在
//! dev 的 NAS + 79MB 库上暴露。任何后续编辑都可能把某处再包一层函数而**没有任何
//! 反馈**。本模块把这条不变量固化进 `cargo test`。
//!
//! 设计约束（与 `i18n_pack.rs` / `catalog_gate.rs` / `table_gate.rs` 同型）：
//! - **仅测试期编译**（`#[cfg(test)] mod`，见 `main.rs`），不进生产二进制；
//! - **零新依赖**：只做逐字节扫描，不解析 SQL、不连数据库；
//! - **关联方式全是位置性的**：生产区 = 文件里第一次出现行首 `#[cfg(test)]` 之前的部分
//!   （`src/routes/*.rs` 的测试模块一律在文件末尾且 `#[cfg(test)]` 顶格）；
//! - **阳性对照**：断言「扫描到 4 个文件」「闭区间重写逐文件计数」「测试里的旧写法
//!   仍在（证明区域切分没多吃）」；检测器本身另有合成输入的阴性/阳性对照，
//!   保证它不会在空集上「通过」（C2005 坑 68）。

/// 编译期读入各文件（测试不依赖工作目录与文件系统布局）。
const FILES: &[(&str, &str)] = &[
    ("wallet.rs", include_str!("routes/wallet.rs")),
    ("ops.rs", include_str!("routes/ops.rs")),
    ("admin.rs", include_str!("routes/admin.rs")),
    ("org.rs", include_str!("routes/org.rs")),
];

/// 生产区里**月聚合闭区间**的期望处数（`… 'start of month', '+1 month'`）。
/// 它是阳性对照：某处被改回函数包裹时，这个计数会先掉下来。
const MONTH_RANGES: &[(&str, usize)] = &[
    ("wallet.rs", 3),
    ("ops.rs", 3),
    ("admin.rs", 3),
    ("org.rs", 1),
];

/// 生产区 = 第一次行首 `#[cfg(test)]` 之前的部分。
fn production_region(src: &str) -> &str {
    src.split("\n#[cfg(test)]").next().unwrap_or(src)
}

/// 违规行：返回 `(1 基行号, 行文本)`。
///
/// 两条规则（都只看**生产区**的非注释行）：
/// 1. `strftime('%Y` —— 月桶函数包住时间列 ⇒ 索引失效（唯一合法的 `strftime` 是
///    `strftime('%H', …)` 的小时桶**键**，不是过滤条件）；
/// 2. `) = date('now')` / `) = days.day` —— `date(<时间列>) = <常量>` 同款。
fn violations(src: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, line) in production_region(src).lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue; // 注释里引用旧写法是文档，不是缺陷
        }
        if line.contains("strftime('%Y")
            || line.contains(") = date('now')")
            || line.contains(") = days.day")
        {
            out.push((i + 1, line.trim().to_string()));
        }
    }
    out
}

#[test]
fn no_date_function_wraps_a_time_column_in_production() {
    assert_eq!(FILES.len(), 4, "应扫描 4 个路由文件");
    let mut all = Vec::new();
    for (name, src) in FILES {
        for (ln, text) in violations(src) {
            all.push(format!("{name}:{ln}: {text}"));
        }
    }
    assert!(
        all.is_empty(),
        "时间列被日期函数包住 ⇒ 索引失效（NAS 上月聚合 4~10s）。改成闭区间：\n{}",
        all.join("\n")
    );
}

#[test]
fn the_closed_range_rewrites_are_all_present() {
    // 阳性对照：没有这条，`violations` 返回空集时上面那条测试会在空集上「通过」。
    for (name, want) in MONTH_RANGES {
        let src = FILES
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, s)| *s)
            .unwrap_or_else(|| panic!("{name} 不在 FILES 里"));
        let got = production_region(src)
            .matches("start of month', '+1 month'")
            .count();
        assert_eq!(
            got, *want,
            "{name} 的月聚合闭区间处数应为 {want}，实测 {got}"
        );
    }
    // 另两条一次性重写：7 天序列的 JOIN（wallet）与「今日按小时」（ops）。
    let wallet = production_region(FILES[0].1);
    assert!(
        wallet.contains("t.time >= days.day AND t.time < date(days.day, '+1 day')"),
        "wallet 的 7 天序列 JOIN 应使用闭区间"
    );
    let ops = production_region(FILES[1].1);
    assert!(
        ops.contains("time >= date('now') AND time < date('now', '+1 day')"),
        "ops 的「今日按小时」应使用闭区间"
    );
}

#[test]
fn the_test_module_oracle_kept_the_old_form() {
    // 区域切分不得「多吃」：`wallet.rs` 的测试里那句旧写法是**独立的规格对照**
    // （见该测试注释），必须原样保留 —— 它证明重写后的生产查询仍满足旧语义。
    let wallet = FILES[0].1;
    assert_eq!(
        wallet
            .matches("strftime('%Y-%m', time) = strftime('%Y-%m', 'now')")
            .count(),
        1,
        "wallet 的测试模块里应恰好保留 1 处旧写法（规格对照）"
    );
    assert!(
        production_region(wallet)
            .find("strftime('%Y-%m', time) = strftime('%Y-%m', 'now')")
            .is_none(),
        "生产区不得出现旧写法（文档注释里引用片段是允许的，等式不行）"
    );
    // 区域切分确实发生了 —— 否则上面的 1 可能与「切片没生效」得到同一个数。
    assert!(
        production_region(wallet).len() < wallet.len(),
        "wallet.rs 的生产区应短于整文件（测试模块必须被切掉）"
    );
}

#[test]
fn detector_flags_the_pre_fix_shape_and_spares_the_range_form() {
    // 检测器自身的对照：喂合成输入，确认它**真的会红**（不是恒真谓词）。
    let bad = "let q = \"SELECT SUM(pts) FROM transactions \\\n\
                   WHERE user_id = ?1 AND strftime('%Y-%m', time) = strftime('%Y-%m', 'now')\";\n";
    let hits = violations(bad);
    assert_eq!(hits.len(), 1, "含旧写法的合成输入应恰好报 1 处：{hits:?}");

    let day_bad = "LEFT JOIN transactions t ON date(t.time) = days.day AND t.user_id = ?1 \\\n";
    assert_eq!(
        violations(day_bad).len(),
        1,
        "date(<列>) = days.day 也应被报出"
    );

    // 阴性对照：修好后的闭区间写法（含右界）不得被报出。
    let good = "WHERE user_id = ?1 AND time >= date('now', 'start of month') \\\n\
                    AND time < date('now', 'start of month', '+1 month')\";\n\
                LEFT JOIN transactions t ON t.time >= days.day AND t.time < date(days.day, '+1 day') \\\n";
    assert!(
        violations(good).is_empty(),
        "闭区间写法不应被报出：{:?}",
        violations(good)
    );

    // 注释行不报（文档里引用旧写法是合法的）。
    assert!(
        violations("// 旧写法 strftime('%Y-%m', time) = strftime('%Y-%m', 'now')\n").is_empty(),
        "注释里的旧写法不该算违规"
    );
}
