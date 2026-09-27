//! 引用门禁（R94）：**仓库正文不得引用「不存在于仓库里」的会话仪器文件名**。
//!
//! # 起因
//!
//! 本仓的源码注释与 `ui/README.md` 长期用反引号文件名引用「证明另一半的那台仪器」
//!（jsdom 探针 / Python 编译门禁 / 编辑表 / 预检单 …）。这些仪器住在**仓库外**、从未进仓
//!（根因见 `ui/README.md`：CI 里没有 JS 运行器），于是每一个这样的引用对任何读者都是
//! **打不开的死链** —— 而本仓自己早已把这条约定写在 `state_gate.rs` 的 R165 一块里
//!（「仪器住在仓外，故此处不写文件名 —— #606」），只是**没有执行者**：
//! R94 实测全树 **62 处**违反，其中一处就在写下那句话的**同一个文件、3 行之后**。
//!
//! 这与本仓已收口的几条同族（R76 的 MSRV / R78 的部署产物版本 / R80 的发行 tag /
//! R82 的 CHANGELOG 标题 / R84 的构建上下文）同判：**抄了/引用了没有断言，就是腐烂的栖息地**。
//!
//! 设计约束（与 `deploy_gate.rs` / `body_limit_gate.rs` 同型）：**仅测试期编译**、
//! **零新依赖**、**运行期走盘**（`CARGO_MANIFEST_DIR`，不依赖工作目录）；
//! **阳性对照**：断言扫描器确实在语料里看见了反引号包裹的文件名 ——
//! 否则「扫到 0 条违规」与「扫描器是瞎的」在读数上无法区分。
//!
//! # 规则
//!
//! - **语料**：仓库树里的文本文件（`rs`/`md`/`js`/`html`/`css`/`toml`/`yml`）。
//!   对 `.rs` 只取**注释正文**（站点按定义写在注释里；字符串字面量里的同名 token
//!   是本模块自己的合成夹具，掩掉它才不会假红）。
//! - **站点**：被**单反引号**包裹、整体形如文件名的 token（`` `…/x.js` ``）。
//! - **判词**：该 token 的 basename 命中**会话仪器命名**（`^[rc][0-9]+[-_]`）
//!   **且**仓库里没有同名文件 ⇒ 违规。
//!
//! # 射程（诚实声明）
//!
//! 判据是**命名形态 + 在场检查**，**不**证明「任意非仓库引用」。刻意不报的四类
//!（R93 实测的 12 条假阳性）：运行期产物（`<data>/config.toml`）、**否定**陈述
//!（「仓库没有 `ui/package.json`」）、历史条目（「Removed `data/models.example.json`」）、
//! 以及导出文件名这类**占位符**（`aitokenpool-transactions-YYYYMMDD.csv`）——
//! 它们要么真的是运行期事实、要么正因为不存在才这么写，收进来只会制造假红。

use std::collections::BTreeSet;
use std::path::Path;

/// 扫描的文本扩展名（语料边界）。
const TEXT_EXT: &[&str] = &["rs", "md", "js", "html", "css", "toml", "yml"];

/// 走盘时跳过的目录：版本库、构建产物、会话工作区（`.emrg` 之下才是本任务的 clone）。
const SKIP_DIRS: &[&str] = &[".git", "target", ".emrg", "node_modules"];

/// 被引用的文件名可以有的扩展名（比 `TEXT_EXT` 宽：引用可以指向任何类型的产物）。
const CITED_EXT: &[&str] = &[
    "js", "py", "rs", "sh", "json", "diff", "txt", "ps1", "csv", "toml", "yml", "html", "css", "md",
];

/// 会话仪器的命名形态：`r<digits>_` / `r<digits>-` / `c<digits>_` / `c<digits>-`。
///
/// 这是本任务（与更早的会话）给「一台一次性仪器」起的名字。它**不是**一条通用规则 ——
/// 正因为窄，才配得上「零豁免清单」：本仓没有任何真实文件长这个样子。
fn session_instrument(basename: &str) -> bool {
    let mut it = basename.chars();
    match it.next() {
        Some('r') | Some('c') => {}
        _ => return false,
    }
    let mut digits = 0usize;
    for ch in it {
        if ch.is_ascii_digit() {
            digits += 1;
            continue;
        }
        return digits > 0 && (ch == '-' || ch == '_');
    }
    false
}

/// 像不像是被引用**文件**的名字（而不是散文里的其它反引号内容）。
fn look_like_file(tok: &str) -> bool {
    if tok.is_empty() || tok.contains(char::is_whitespace) || tok.contains('`') {
        return false;
    }
    if !tok
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'))
    {
        return false;
    }
    match tok.rsplit_once('.') {
        Some((stem, ext)) => !stem.is_empty() && CITED_EXT.contains(&ext),
        None => false,
    }
}

/// 一行里被**单反引号**包裹的片段。奇数个反引号时最后一段视为未闭合、照收。
fn backtick_tokens(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for (i, part) in line.split('`').skip(1).enumerate() {
        if i % 2 == 0 {
            out.push(part);
        }
    }
    out
}

/// 只取 Rust 源码的**注释正文**（代码与字符串/字符字面量掩成空格，换行保留）。
///
/// 站点按定义写在注释里；掩掉字符串字面量是必须的 —— 本模块自己的合成夹具就写在字符串里，
/// 不掩的话门禁会把自己的输入读成违规（R84 实测过同形的假红）。
fn rust_comments(src: &str) -> String {
    let cs: Vec<char> = src.chars().collect();
    let mut out: Vec<char> = cs
        .iter()
        .map(|c| if *c == '\n' { '\n' } else { ' ' })
        .collect();
    let mut i = 0usize;
    while i < cs.len() {
        if cs[i] == '/' && cs.get(i + 1) == Some(&'/') {
            i += 2;
            while i < cs.len() && cs[i] != '\n' {
                out[i] = cs[i];
                i += 1;
            }
        } else if cs[i] == '/' && cs.get(i + 1) == Some(&'*') {
            out[i] = '/';
            out[i + 1] = '*';
            i += 2;
            let mut depth = 1usize;
            while i < cs.len() && depth > 0 {
                if cs[i] == '/' && cs.get(i + 1) == Some(&'*') {
                    out[i] = '/';
                    out[i + 1] = '*';
                    depth += 1;
                    i += 2;
                } else if cs[i] == '*' && cs.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    out[i] = '*';
                    out[i + 1] = '/';
                    i += 2;
                } else {
                    out[i] = if cs[i] == '\n' { '\n' } else { cs[i] };
                    i += 1;
                }
            }
        } else if cs[i] == '"' || (cs[i] == 'r' && raw_string_start(&cs, i).is_some()) {
            i = skip_literal(&cs, i);
        } else if cs[i] == '\'' {
            i = skip_char_literal(&cs, i);
        } else {
            i += 1;
        }
    }
    out.into_iter().collect()
}

/// `pos` 处若是原始字符串的开头（`r"` / `r#"` …），返回引号的下标。
fn raw_string_start(cs: &[char], pos: usize) -> Option<usize> {
    let mut j = pos + 1;
    let mut hashes = 0usize;
    while cs.get(j) == Some(&'#') {
        hashes += 1;
        j += 1;
    }
    if cs.get(j) == Some(&'"') && (cs[pos] == 'r' || hashes > 0) {
        Some(j)
    } else {
        None
    }
}

/// 跳过 `pos` 处的一个字符串字面量，返回其后的下标。
fn skip_literal(cs: &[char], pos: usize) -> usize {
    if let Some(q) = raw_string_start(cs, pos) {
        let hashes = q - pos - 1;
        let mut j = q + 1;
        while j < cs.len() {
            if cs[j] == '"' && (0..hashes).all(|k| cs.get(j + 1 + k) == Some(&'#')) {
                return j + 1 + hashes;
            }
            j += 1;
        }
        return j;
    }
    let mut j = pos + 1;
    while j < cs.len() {
        match cs[j] {
            '\\' => j += 2,
            '"' => return j + 1,
            _ => j += 1,
        }
    }
    j
}

/// 跳过 `pos` 处的一个字符字面量（认不出就把 `'` 当寿命标记，返回 `pos + 1`）。
fn skip_char_literal(cs: &[char], pos: usize) -> usize {
    match cs.get(pos + 1) {
        Some('\\') => {
            for end in [pos + 3, pos + 4, pos + 6] {
                if cs.get(end) == Some(&'\'') {
                    return end + 1;
                }
            }
            pos + 1
        }
        Some(_) => {
            if cs.get(pos + 2) == Some(&'\'') {
                pos + 3
            } else {
                pos + 1
            }
        }
        None => pos + 1,
    }
}

/// 一台文件里的所有违规（`文件:行 引用了仓外仪器 \`名字\``）。
///
/// `present` 是仓库里**真实在场**的文件名集合（basename 与仓库相对路径两种写法都收）。
fn violations_of(files: &[(String, String)], present: &BTreeSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    for (rel, src) in files {
        let text = if rel.ends_with(".rs") {
            rust_comments(src)
        } else {
            src.clone()
        };
        for (i, line) in text.lines().enumerate() {
            for raw in backtick_tokens(line) {
                let tok = raw.trim();
                if !look_like_file(tok) {
                    continue;
                }
                let base = tok.rsplit('/').next().unwrap_or(tok);
                if !session_instrument(base) {
                    continue;
                }
                if present.contains(base) || present.contains(tok) {
                    continue;
                }
                out.push(format!("{rel}:{} 引用了仓外仪器 `{tok}`", i + 1));
            }
        }
    }
    out.sort();
    out
}

/// 语料（仓库相对路径, 文本）与「真实在场的文件名」集合。运行期走盘，不依赖工作目录。
fn scan_corpus() -> (Vec<(String, String)>, BTreeSet<String>) {
    fn walk(
        dir: &Path,
        root: &Path,
        text: &mut Vec<(String, String)>,
        names: &mut BTreeSet<String>,
    ) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if SKIP_DIRS.contains(&name.as_str()) {
                    continue;
                }
                walk(&p, root, text, names);
                continue;
            }
            names.insert(name);
            let ext = p
                .extension()
                .map(|x| x.to_string_lossy().to_string())
                .unwrap_or_default();
            if TEXT_EXT.contains(&ext.as_str()) {
                if let Ok(s) = std::fs::read_to_string(&p) {
                    let rel = p
                        .strip_prefix(root)
                        .unwrap_or(&p)
                        .to_string_lossy()
                        .replace('\\', "/");
                    text.push((rel, s));
                }
            }
        }
    }
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut text = Vec::new();
    let mut names = BTreeSet::new();
    walk(&root, &root, &mut text, &mut names);
    text.sort();
    (text, names)
}

/// 语料里被反引号包裹、形如文件名的 token 总数（阳性对照的分母）。
fn cited_tokens(files: &[(String, String)]) -> usize {
    let mut n = 0usize;
    for (rel, src) in files {
        let text = if rel.ends_with(".rs") {
            rust_comments(src)
        } else {
            src.clone()
        };
        for line in text.lines() {
            n += backtick_tokens(line)
                .into_iter()
                .filter(|t| look_like_file(t.trim()))
                .count();
        }
    }
    n
}

#[test]
fn every_citation_names_a_file_the_repository_has() {
    let (files, present) = scan_corpus();
    let bad = violations_of(&files, &present);
    assert!(
        bad.is_empty(),
        "仓库正文不得引用仓外仪器文件名（#606）：\n{}",
        bad.join("\n")
    );
}

#[test]
fn the_citation_scanner_actually_sees_the_citations_it_guards() {
    // 阳性对照：语料里必须真的有反引号包裹的文件名 —— 否则上面那条「0 条违规」是空转。
    let (files, _) = scan_corpus();
    let tokens = cited_tokens(&files);
    assert!(
        tokens >= 200,
        "扫描器必须真的看见语料里的反引号文件名（否则 0 条违规不可信）：{tokens}"
    );
    // 而且 `.rs` 的注释掩码没把语料吃空：注释里仍看得到引用（例：`ui/README.md`）。
    let rs_with_citations = files
        .iter()
        .filter(|(rel, src)| {
            rel.ends_with(".rs") && cited_tokens(&[(rel.clone(), src.clone())]) > 0
        })
        .count();
    assert!(
        rs_with_citations >= 3,
        "至少几个 `.rs` 的注释里应当能看见反引号文件名：{rs_with_citations}"
    );
}

#[test]
fn the_citation_rule_flags_the_shapes_it_guards() {
    // 规则本身有牙：合成输入（规则与「活树此刻是否合规」是两件事，不该互相污染读数）。
    let present: BTreeSet<String> = ["wallet.rs".to_string(), "README.md".to_string()]
        .into_iter()
        .collect();
    let files = |rel: &str, src: &str| vec![(rel.to_string(), src.to_string())];

    // ① 注释里引用仓外仪器名 ⇒ 违规。
    let bad = files("src/x.rs", "// 那一半归 jsdom 探针 `r92_probe.js` 证。\n");
    let v = violations_of(&bad, &present);
    assert_eq!(v.len(), 1, "仓外仪器引用必须被抓到：{v:?}");
    assert!(
        v[0].contains("r92_probe.js") && v[0].contains("src/x.rs:1"),
        "{v:?}"
    );

    // ② 同一个 token 写在**字符串字面量**里 ⇒ 不是站点（夹具/示例，掩掉防假红）。
    let lit = files("src/x.rs", "const S: &str = \"见 `r92_probe.js`\";\n");
    assert!(
        violations_of(&lit, &present).is_empty(),
        "字符串里的同名 token 不是站点"
    );

    // ③ 引用**在场**的文件 ⇒ 放行（含子目录写法）。
    let ok = files(
        "src/x.rs",
        "// 见 `wallet.rs`、`README.md` 与 `ui/js/app.js`。\n",
    );
    assert!(
        violations_of(&ok, &present).is_empty(),
        "在场文件不得报违规：{ok:?}"
    );

    // ④ 不在场的**非仪器名** ⇒ 刻意放行（射程声明：运行期产物、否定陈述、占位符等）。
    let rt = files("src/x.rs", "// 首次启动自动复制 `<data>/config.toml`。\n");
    assert!(
        violations_of(&rt, &present).is_empty(),
        "运行期产物路径不在射程内"
    );

    // ⑤ Markdown 语料没有注释语法 ⇒ 整篇都是站点。
    let md = files(
        "ui/README.md",
        "探针 `tmp/c2136_probe.js` 只用于本地证明**方向**。\n",
    );
    let mv = violations_of(&md, &present);
    assert_eq!(mv.len(), 1, "Markdown 正文里的引用必须被抓到：{mv:?}");

    // ⑥ 子句里的仪器名同样算（`tmp/` 前缀不豁免）。
    let sub = files("ui/README.md", "`c2172_probe.js` 的 `F1/F2` 腿钉数值。\n");
    assert_eq!(violations_of(&sub, &present).len(), 1);
}

#[test]
fn the_instrument_naming_predicate_is_not_a_net_that_catches_anything() {
    // 判据要窄：只有「一次会话给一台仪器起的名字」才算，本仓的真实文件名一个都不沾。
    for good in [
        "r92_probe.js",
        "c2172_probe.js",
        "c2170-probe.js",
        "r99-landing-kit.md",
        "r165_compile_gate.py",
        "r156_model_price_invariant_probe.py",
    ] {
        assert!(session_instrument(good), "{good} 必须被认成会话仪器名");
    }
    for bad in [
        "README.md",
        "wallet.rs",
        "_gate.rs",
        "foo.rs",
        "config.toml",
        "package.json",
        "aitokenpool-transactions-YYYYMMDD.csv",
        "r.json",
        "r_probe.js",
        "cx_probe.js",
        "c_probe.js",
    ] {
        assert!(!session_instrument(bad), "{bad} 不得被认成会话仪器名");
    }
    // 文件名的形状判据同样要窄：散文里的反引号内容不算站点。
    for good in ["ui/js/app.js", "config.toml", "r92_probe.js"] {
        assert!(look_like_file(good), "{good} 应当被认成文件名");
    }
    for bad in [
        "fn main()",
        "cargo test",
        "",
        "gateway.rs 的行为",
        "see also",
    ] {
        assert!(!look_like_file(bad), "{bad} 不是文件名");
    }
}
