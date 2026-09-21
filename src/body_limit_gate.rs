//! 请求体上限门禁（2026-09-21）：**全树只有一个请求体上限的数**。
//!
//! 起因（rant 2026-09-18T09:14:18 的第二次现身）：上限分布在**两层**上 ——
//! axum 提取器那层（`DefaultBodyLimit::max`，`String` / `Json` 的 2 MiB 默认值只认它写的
//! `DefaultBodyLimitKind` 扩展）与 tower-http 的外层粗闸（`RequestBodyLimitLayer::new`，
//! 只看 `Content-Length`，超限时**不读体直接 413**）。两层是**两个数**：
//!
//! - 外层写得更小 ⇒ 它**抢答**，实际生效的就是外层（2026-09-21 把内层从 8 MiB 抬到
//!   277 MiB 时，外层还停在 `70 * 1024 * 1024`：实测 71 MiB 的体在外层被 413 掉 ——
//!   抬了个寂寞。这与 `v0.7.10` 那次「配了一个值 ≠ 生效了」是同族缺陷）；
//! - 外层写得更大 ⇒ 它抬不动内层（不写那个扩展），实际生效的是内层。
//!
//! **只有让两层引用同一个常量**，这个「min(两层) 才是真相」的陷阱才消失。本门禁把这条
//! 不变量固化进 `cargo test`：任何一处层构造写了**别的数**、或另立**第二个** `…BODY_LIMIT`
//! 常量，都会在这里变红。
//!
//! 设计约束（与 `i18n_pack.rs` / `perf_gate.rs` / `state_gate.rs` 同型）：
//! - **仅测试期编译**（`#[cfg(test)] mod body_limit_gate`，见 `main.rs`），不进生产二进制；
//! - **零新依赖**：逐字节扫描 + `std::fs` 走一遍 `src/`，不解析 Rust；
//! - **生产区**＝文件里**最后一个**行首 `#[cfg(test)]` 之前的部分（`main.rs` 的
//!   `mod xxx_gate;` 声明自身也是 `#[cfg(test)]` 顶格，故取**最后**一个而不是第一个）；
//! - **跳过** `*_gate.rs` 与 `i18n_pack.rs`：这些模块把别的源码当**语料**内嵌（字符串里
//!   就会出现同名调用），扫它们等于自救式误报；
//! - **阳性对照**：断言「走到的文件数 ≥ 20」「调用点处数恰为 1 / 3」「生产区确实被切短」，
//!   检测器另有三条合成输入的阴性 / 阳性对照（防「在空集上通过」，C2005 坑 68）。
//!
//! ⚠️ **已知射程**：行注释按 `//` 截断（字符串字面量里的 `//` 也会被截，如测试里的
//! `http://127.0.0.1:9`）—— 截断只会让那一行**少**看到内容；层构造行不会与 URL 同行，
//! 故只可能漏报、不可能误报。块注释 `/* */` 内的调用同样会被当成代码（今日 0 处）。

use std::path::PathBuf;

/// 唯一真源的名字。
const LIMIT: &str = "GATEWAY_BODY_LIMIT";

/// 待扫的层构造函数（含左括号）。
const LAYERS: &[&str] = &["RequestBodyLimitLayer::new(", "DefaultBodyLimit::max("];

/// 生产区 = **最后一个**行首 `#[cfg(test)]` 之前的部分。
fn production_region(src: &str) -> &str {
    match src.rfind("\n#[cfg(test)]") {
        Some(i) => &src[..i],
        None => src,
    }
}

/// 按 `//` 截断行注释（保持行结构，便于报行号）。
fn strip_line_comments(src: &str) -> String {
    src.lines()
        .map(|l| match l.find("//") {
            Some(i) => &l[..i],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 一处的实参文本（从左括号到配对右括号，跨行通过；去掉首尾空白与尾逗号）。
fn call_sites(src: &str, callee: &str) -> Vec<(usize, String)> {
    let text = strip_line_comments(src);
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = text[from..].find(callee) {
        let at = from + rel;
        let line = text[..at].matches('\n').count() + 1;
        let rest = &text[at + callee.len()..];
        let mut depth = 1i32;
        let mut arg = String::new();
        for ch in rest.chars().take(400) {
            match ch {
                '(' => {
                    depth += 1;
                    arg.push(ch);
                }
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    arg.push(ch);
                }
                _ => arg.push(ch),
            }
        }
        out.push((line, arg.trim().trim_end_matches(',').trim().to_string()));
        from = at + callee.len();
    }
    out
}

/// 层构造写了别的数（返回可读的违规描述）。
fn violations(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for callee in LAYERS {
        for (line, arg) in call_sites(src, callee) {
            if arg != LIMIT {
                out.push(format!("{line}: {callee}{arg}) —— 上限必须写成 {LIMIT}"));
            }
        }
    }
    out
}

/// 生产区里所有以 `BODY_LIMIT` 结尾的标识符（应恒为 `GATEWAY_BODY_LIMIT`）。
fn limit_names(src: &str) -> Vec<String> {
    let text = strip_line_comments(production_region(src));
    let mut out = Vec::new();
    for token in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
        if token.ends_with("BODY_LIMIT") && !token.is_empty() {
            out.push(token.to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

/// 走一遍 `src/`，返回 `(相对路径, 文本)`；跳过门禁模块自身。
fn source_files() -> Vec<(String, String)> {
    fn walk(dir: &PathBuf, root: &PathBuf, out: &mut Vec<(String, String)>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, root, out);
            } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
                let name = p.file_name().unwrap().to_string_lossy().to_string();
                // 门禁模块把别的源码当语料内嵌 ⇒ 扫它们会误报（见模块文档）。
                if name.ends_with("_gate.rs") || name == "i18n_pack.rs" {
                    continue;
                }
                let rel = p
                    .strip_prefix(root)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .replace('\\', "/");
                if let Ok(text) = std::fs::read_to_string(&p) {
                    out.push((rel, text));
                }
            }
        }
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    walk(&root, &root, &mut out);
    out.sort();
    out
}

#[test]
fn every_body_limit_layer_is_the_same_single_number() {
    let files = source_files();
    assert!(
        files.len() >= 20,
        "只走到 {} 个源文件 —— 扫描器本身坏了（空集上通过是假绿）",
        files.len()
    );

    let mut inner = 0usize; // `DefaultBodyLimit::max(…)` 处数
    let mut outer = 0usize; // `RequestBodyLimitLayer::new(…)` 处数
    let mut cut = 0usize; // 真的被切掉测试模块的文件数
    let mut bad: Vec<String> = Vec::new();
    for (rel, src) in &files {
        let prod = production_region(src);
        if prod.len() < src.len() {
            cut += 1;
        }
        for v in violations(src) {
            bad.push(format!("{rel}:{v}"));
        }
        inner += call_sites(prod, LAYERS[1]).len();
        outer += call_sites(prod, LAYERS[0]).len();
        for name in limit_names(src) {
            if name != LIMIT {
                bad.push(format!("{rel}: 出现了第二个上限常量 `{name}`"));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "请求体上限必须只有一个数（{LIMIT}）：\n{}",
        bad.join("\n")
    );
    // 阳性对照：调用点确实被扫到了（删掉一层 / 漏扫文件都会让计数掉下来），
    // 且生产区切分确实生效（切割规则失效时，测试里的 `70 * 1024 * 1024` 会混进来）。
    assert!(
        cut >= 5,
        "只有 {cut} 个文件切出了生产区 —— 切割规则坏了（本仓带测试模块的文件远多于 5 个）"
    );
    assert_eq!(
        outer, 1,
        "外层的 `RequestBodyLimitLayer::new(…)` 应恰有 1 处"
    );
    assert_eq!(inner, 3, "三条网关路由各一处 `DefaultBodyLimit::max(…)`");
}

#[test]
fn the_layer_argument_is_read_across_lines() {
    // rustfmt 会把长实参折行；实参抽取必须跨行（否则会读出空串而误判）。
    let folded = "fn f() {\n    .layer(DefaultBodyLimit::max(\n        GATEWAY_BODY_LIMIT,\n    ))\n}\n#[cfg(test)]\nmod t {}\n";
    assert_eq!(call_sites(folded, LAYERS[1]), vec![(2, LIMIT.to_string())]);
    assert!(violations(folded).is_empty());
}

#[test]
fn detector_flags_a_hardcoded_number() {
    let src = "fn f() {\n    .layer(RequestBodyLimitLayer::new(70 * 1024 * 1024))\n}\n#[cfg(test)]\nmod t {}\n";
    let v = violations(src);
    assert_eq!(v.len(), 1, "硬编码的数必须被抓到：{v:?}");
    assert!(v[0].contains("70 * 1024 * 1024"), "{v:?}");
}

#[test]
fn detector_flags_a_second_constant_name() {
    let src = "const INNER_BODY_LIMIT: usize = 8 * 1024 * 1024;\nfn f() {\n    .layer(DefaultBodyLimit::max(INNER_BODY_LIMIT))\n}\n";
    assert_eq!(violations(src).len(), 1, "换了名字也是第二个数");
    assert_eq!(limit_names(src), vec!["INNER_BODY_LIMIT".to_string()]);
}

#[test]
fn detector_ignores_commented_out_layers() {
    let src =
        "fn f() {\n    // .layer(RequestBodyLimitLayer::new(70 * 1024 * 1024)) 曾经这样写\n}\n";
    assert!(violations(src).is_empty(), "{:?}", violations(src));
}
