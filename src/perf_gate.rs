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
    // 第三、四条不变量（v15 / 按需 JOIN / 行构造器不得发 SQL）也覆盖共享页
    ("sharing.rs", include_str!("routes/sharing.rs")),
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

// ---------------------------------------------------------------------------
// 第二条不变量（rant 2026-09-14T21:15:02 第 3 条）：**聚合语句按需 JOIN**。
//
// `wallet.rs` 的 summary / COUNT / trend 三条语句只碰 `t.*`。而只要 SQL 里出现
// `tx_joins()` 那三个 LEFT JOIN，SQLite 就**不再选用覆盖索引**
// `idx_transactions_user_id_time_type_pts_tokens`，退回 `idx_transactions_user_id_id`
// 逐行回表：dev 库（18.6 万行 / NFS）同一条查询 **0.21s → 22.6s（≈100×）**，
// 拖垮共享 DB 锁下的 `/api/me` ⇒ 前端「刷新 #/sharing 跳回登录页」。
//
// 本机（SSD + 小库）**完全看不出来** —— 聚合值一模一样，唯一症状是慢。
// 因此这条不变量只能固化在门禁里：条件拼接入口 `tx_joins_if(needs_joins(…))` 之外的
// 任何**无条件** `tx_joins()` 调用都是缺陷（JOIN 与 `tx_where` 的引用会随之分叉）。
// ---------------------------------------------------------------------------

/// 生产区里 `name` 的**调用**处数（行首 `fn ` 的定义行与注释行不计）。
fn calls_of(src: &str, name: &str) -> usize {
    production_region(src)
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with("//") && !is_fn_def(l)
        })
        .filter(|l| l.contains(name))
        .count()
}

/// 行首 `fn` 定义行的函数名，含 `pub` / `pub(crate)` / `async` 前缀：
/// `pub async fn list(…)` → `list`，`fn tx_joins_if(…)` → `tx_joins_if`。
/// 不是定义行 → `None`。
fn fn_name(line: &str) -> Option<&str> {
    let mut s = line.trim_start();
    for prefix in ["pub(crate) ", "pub ", "async "] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest;
        }
    }
    s.strip_prefix("fn ")?.split(['(', '<', ' ']).next()
}

/// 该行是不是函数定义行（`fn` / `pub fn` / `pub async fn` …）。
fn is_fn_def(line: &str) -> bool {
    fn_name(line).is_some()
}

/// 直接调用无条件 `tx_joins()` 的位置（应恒为空）。
///
/// 位置性归属：向上找最近的 `fn ` 行 = 该调用的归属函数；只有包装器 `tx_joins_if`
/// 内部的那一次是合法的（它把「是否需要」与「拼接」绑在一起）。
fn unconditional_join_calls(src: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut owner = "";
    for (i, line) in production_region(src).lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with("//") {
            continue; // 注释里引用函数名是文档，不是调用
        }
        if is_fn_def(line) {
            owner = fn_name(line).unwrap_or("");
            continue; // 定义行本身不是调用
        }
        if line.contains("tx_joins()") && owner != "tx_joins_if" {
            out.push((i + 1, line.trim().to_string()));
        }
    }
    out
}

/// `fn tx_joins_if` 之后的函数体行（到顶格 `}` 为止）。
///
/// 用途：包装器可以「看似正常」地被改成**恒真**（`if needed` → `if true`）——
/// 调用点一处未动、`tx_joins()` 也没有新的直接调用，[`unconditional_join_calls`] 会说没问题，
/// 而这正是同一条 100× 退化，所以必须把「无 JOIN 分支存在」也钉住。
fn body_of<'a>(src: &'a str, name: &str) -> Vec<(usize, &'a str)> {
    let mut out = Vec::new();
    let mut inside = false;
    for (i, line) in production_region(src).lines().enumerate() {
        if inside {
            if line.starts_with('}') {
                break; // 顶格 `}` = 函数结束
            }
            out.push((i + 1, line));
        } else if fn_name(line) == Some(name) {
            inside = true;
        }
    }
    out
}

#[test]
fn the_transaction_aggregates_never_join_unconditionally() {
    assert_eq!(FILES[0].0, "wallet.rs");
    let src = FILES[0].1;
    // ① 不变量：无条件三 JOIN 不得有任何 `tx_joins_if` 之外的调用点。
    let hits = unconditional_join_calls(src);
    assert!(
        hits.is_empty(),
        "无条件 LEFT JOIN 会让优化器弃用覆盖索引（dev 实测 0.21s → 22.6s），\
         聚合语句改走 `tx_joins_if(needs_joins(&filters))`：\n{}",
        hits.iter()
            .map(|(l, t)| format!("  src/routes/wallet.rs:{l}: {t}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    // ② 不变量（另一面）：包装器本身不得被改成恒真 —— 调用点一处不动也照样退化。
    let body = body_of(src, "tx_joins_if");
    assert!(
        body.iter().any(|(_, l)| l.contains("\"\"")),
        "`tx_joins_if` 必须保留**无 JOIN** 的分支（`\"\"`），否则它就成了恒真"
    );
    assert!(
        body.iter().any(|(_, l)| l.contains("tx_joins(")),
        "`tx_joins_if` 需要 JOIN 时仍应复用 `tx_joins`（唯一一份 JOIN 文案）"
    );
    // ③ 阳性对照：按需拼接确实还在用 —— summary / COUNT / trend 三处。
    // （新增聚合语句时请一并更新此计数：这是有意的摩擦，不是噪音。）
    assert_eq!(
        calls_of(src, "tx_joins_if("),
        3,
        "wallet 的三条聚合语句都应经 `tx_joins_if` 拼接 JOIN"
    );
    // ④ 阳性对照：判定与拼接共用同一谓词。
    assert_eq!(
        calls_of(src, "needs_joins("),
        2,
        "JOIN 与 `tx_where` 的引用必须共用 `needs_joins`"
    );
}

#[test]
fn join_detector_flags_a_bare_call_and_spares_the_wrapper() {
    // 检测器自身的对照：喂合成输入，确认它**真的会红**（不是恒真谓词）。
    let bare = "fn count_sql() -> String {\n    \
                format!(\"SELECT COUNT(*) FROM transactions t {} WHERE x\", tx_joins())\n}\n";
    assert_eq!(
        unconditional_join_calls(bare).len(),
        1,
        "聚合语句里直接写 `tx_joins()` 应恰好报 1 处"
    );

    // 阴性对照：包装器内部的那一次是「按需」本身，不得报出。
    let wrapper = "fn tx_joins_if(needed: bool) -> &'static str {\n    \
                   if needed {\n        tx_joins()\n    } else {\n        \"\"\n    }\n}\n";
    assert!(
        unconditional_join_calls(wrapper).is_empty(),
        "包装器内部的调用不该算违规"
    );
    // 定义行不是调用。
    let def = "fn tx_joins() -> &'static str {\n    \"LEFT JOIN keys k ON k.id = t.key_id\"\n}\n";
    assert!(unconditional_join_calls(def).is_empty(), "定义行不该算违规");
    // 注释里的引用合法。
    assert!(unconditional_join_calls("// 见 tx_joins()\n").is_empty());

    // `calls_of` 的分母：定义行不计入调用数（否则上面两条阳性对照会虚高）。
    assert_eq!(calls_of(wrapper, "tx_joins()"), 1);
    assert_eq!(calls_of(def, "tx_joins()"), 0);

    // `body_of` 的位置性：切出的正是包装器函数体（含 `true` 与 `""` 两个分支），
    // 不含定义行、不含紧随其后的其它函数。
    let body = body_of(wrapper, "tx_joins_if");
    assert_eq!(body.len(), 5, "包装器函数体应为 5 行：{body:?}");
    assert!(
        body.iter().any(|(_, l)| l.contains("\"\"")),
        "应切出空串分支"
    );
    assert!(
        body_of(
            "fn tx_joins_if(needed: bool) -> &'static str {\n    if needed { tx_joins() }\n}\n",
            "tx_joins_if"
        )
        .iter()
        .all(|(_, l)| !l.contains("\"\"")),
        "恒真版本（无空串分支）应被 body_of 原样暴露出来"
    );
}

// ---------------------------------------------------------------------------
// 第三条不变量（rant 2026-09-14T21:15:02 第 2 条）：**行构造器不得发 SQL**。
//
// `sharing.rs::sharing_row` 是「把一行记录变成 JSON」的纯函数。它曾在行内跑
// `SELECT SUM(pts) FROM transactions WHERE key_id = ?1 AND type = 'earn'` 取收益 ⇒
// 列表 N 行就是 N 次查询（N+1）：每次都要 prepare + 索引查找，NAS 上还要多摸几页，
// 代价随 N 线性增长（本机 200,000 行 / 8 key：8 次子查询 1.54ms，一次批量聚合 0.02ms）。
//
// 收益现在由 `ROW_SELECT` 的**一个**批量聚合左连给出，行构造器只读列。这条规则把
// 「行构造器里再补一次 query_row」变成 CI 红灯 —— 那种改动**能编译、测试也大多会过**
// （值可能仍然对），唯一症状是慢，而慢在小库/SSD 上看不出来。
// ---------------------------------------------------------------------------

/// 会发出 SQL 的调用（`prepare` / `query_row` / `execute` …）。
const SQL_CALLS: &[&str] = &[
    "conn.prepare(",
    ".prepare(",
    "query_row(",
    "query_map(",
    "execute(",
    "execute_batch(",
    "prepare_cached(",
];

/// 函数 `name` 的函数体里发出的 SQL 调用（1 基行号、行文本）。
fn sql_in_fn(src: &str, name: &str) -> Vec<(usize, String)> {
    body_of(src, name)
        .into_iter()
        .filter(|(_, l)| !l.trim_start().starts_with("//"))
        .filter(|(_, l)| SQL_CALLS.iter().any(|c| l.contains(c)))
        .map(|(i, l)| (i, l.trim().to_string()))
        .collect()
}

#[test]
fn the_sharing_row_builder_runs_no_sql() {
    let src = FILES
        .iter()
        .find(|(n, _)| *n == "sharing.rs")
        .map(|(_, s)| *s)
        .expect("sharing.rs 应在 FILES 里");
    let hits = sql_in_fn(src, "sharing_row");
    assert!(
        hits.is_empty(),
        "行构造器里发 SQL ⇒ 列表端点退回 N+1（每行一次查询）；收益应来自 `ROW_SELECT` 的批量聚合：\n{}",
        hits.iter()
            .map(|(l, t)| format!("  src/routes/sharing.rs:{l}: {t}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    // 阳性对照：同一份源码里的端点函数**当然**发 SQL —— 证明扫描器看的是真内容，
    // 而不是「body_of 永远切出空集」这种假绿。
    assert!(
        !sql_in_fn(src, "list").is_empty(),
        "`list` 端点应当发 SQL（对照组）"
    );
    assert!(
        !sql_in_fn(src, "patch").is_empty(),
        "`patch` 端点应当发 SQL（对照组）"
    );
}

#[test]
fn row_builder_detector_flags_a_per_row_query() {
    // 检测器自身的对照：喂合成输入，确认它**真的会红**。
    let bad = "fn sharing_row(crypto: &C, r: &Row) -> Result<V> {\n    \
               let earn: f64 = conn.query_row(\"SELECT SUM(pts)\", [id], |r| r.get(0))?;\n    \
               Ok(json!({}))\n}\n";
    assert_eq!(
        sql_in_fn(bad, "sharing_row").len(),
        1,
        "行内 `query_row` 应恰好报 1 处"
    );
    // 阴性对照：只读列的版本（本仓库现在的形状）不得报出。
    let good = "fn sharing_row(crypto: &C, r: &Row) -> Result<V> {\n    \
                let earn: f64 = r.get(13)?;\n    \
                Ok(json!({ \"earn\": earn }))\n}\n";
    assert!(
        sql_in_fn(good, "sharing_row").is_empty(),
        "只读列的行构造器不该被报出：{:?}",
        sql_in_fn(good, "sharing_row")
    );
    // 注释里的 SQL 是文档（本仓库的注释里就写着旧写法），不算违规。
    let commented = "fn sharing_row(crypto: &C, r: &Row) -> Result<V> {\n    \
                     // 旧写法：conn.query_row(\"SELECT SUM(pts) …\") —— 每行一次\n    \
                     Ok(json!({}))\n}\n";
    assert!(sql_in_fn(commented, "sharing_row").is_empty());
}

#[test]
fn no_date_function_wraps_a_time_column_in_production() {
    assert_eq!(FILES.len(), 5, "应扫描 5 个路由文件");
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
