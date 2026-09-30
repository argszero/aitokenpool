//! 前端声明门禁（R131）：**`ui/js/*.js` 的每个顶层声明都必须有一个读者**。
//!
//! Rust 有 `dead_code` lint，JS 没有 —— 一枚函数或常量在自己文件里被删掉全部调用点之后，
//! 会**静默留下**：仍被浏览器解析、仍占着文件、仍被读者当作「在用」的证据。本门禁把
//! 「声明了就得有人读」变成一条可执行的不变量。
//!
//! # 起因：两处违反，两条不同的历史
//!
//! 全树扫出**恰好两处**「只在自己声明那一行出现过」的顶层声明，两处都是**漂移不是取舍**
//! （`git log -S` 可复现）：
//!
//! | 声明 | 形态 | 来历 |
//! |---|---|---|
//! | `DAY_LABELS`（`ui/js/app.js`） | 模块级 `const` | `d31641d`（#32 结构化可用时间段）引入；`68f9f70`（#86 i18n）把**唯一读点**换成 `T("share.day." + n)`，**常量留下了**（连值也顺手改了） |
//! | `nowTime()`（`ui/js/app.js`） | `function` | `9a3ff3e`/`cfd38ac` 引入；`89963f3`（#94「零 mock 数据」）删掉它的 5 个调用点（4 处 `D.TRANSACTIONS.unshift` ＋ 1 处 `D.RAISE_REQUESTS.unshift`），**函数体留下** |
//!
//! 两处的共同形状：**读点在一次「搬走口径」的改动里被搬走，被搬走的载体自己没跟着走。**
//! `ui/README.md` 当时还把 `nowTime()` 写成「统一使用」的约定（同一事实的第二个载体，
//! 已随本门禁一并更正 —— 那一半是 `doc-comment-claims` 轴的事：**修数据、不上门禁**）。
//!
//! # 守的是什么
//!
//! * **名册（从 `ui/js/*.js` 的源码派生，⛔ 不写快照）** —— 每个「顶层声明」＝ 缩进 ≤ 2 空格
//!   （四个文件都是 `(function(){…})()` 或 `const api = (() => {…})()` 形态，模块体正好一级）
//!   且以 `function` / `async function` / `const` / `let` / `var` / `class` 开头的行上那个标识符。
//! * **判据** —— 该标识符必须在前端**代码**语料（`ui/index.html` 标记 ＋ 四个 `ui/js/*.js`）
//!   里以**标识符边界**出现**至少两次**（一次是声明本身）。
//!
//! # ⛔ 为什么不把 `ui/README.md` 与 `ui/css/style.css` 算进读者
//!
//! **实测**：把 `ui/README.md` 计入读者，本门禁对它要抓的那两处缺陷**完全失明** ——
//! README 恰好**提到**了这两个名字（`:98` 的「统一使用」句、`:365` 的常量清单），
//! 于是计数从 1 变 2、门禁在**修前树**上全绿（A/B 双腿实测：`html+js` 报 2 条、
//! `html+js+css+README` 报 0 条）。散文里提到一个名字**不是**一个读点。
//! `ui/css/style.css` 同理不算：样式表声明的是类，从不消费 JS 标识符，把它算进来只会
//! 多一条假绿通道（一个死掉的 `hidden` 会被 `.hidden` 规则救活）。
//! 因此读者语料**刻意只有**标记与脚本两份 —— 这是本门禁的核心判据，不是遗漏。
//!
//! # 射程（如实）
//!
//! 本门禁是**词法**的：它证「树上有第二个出现」，**不**证那个出现是**可达的**读点 ——
//! 注释里、字符串里、以及**别处的死代码**里的一次同名出现都能让它变绿（那是「整块死代码」
//! 那条轴的领地，本门禁看不见）。它也不实现 JS 语法：
//!
//! * 只认**行首**的声明形态；`let a = 1, b = 2;` 的第二个名字、`const { a, b } = …` 的解构名、
//!   `window.X = …` 这种「赋值式导出」都不入名册（宁可漏报，不制造假红）。
//! * 缩进 ≤ 2 是**词法**近似：换一种包裹写法（比如给 IIFE 再加一层 `try {}`）会让真声明
//!   落到更深一级 ⇒ **漏报**；缩进 2 的**块内**声明（`if (…) { const x = … }` 写在模块体里）
//!   则会被**多报**成顶层 —— 实测当前语料零命中。
//! * 计数器是**原始文本**上的字节比较（不掩码）：掩码只用于**提取**声明，因为注释里
//!   可以逐字写着一个声明（本仓的注释到处引用代码）。
//!
//! # 掩码必须跳正则字面量（本轮踩到，坑 #641 同族）
//!
//! `app.js` 的 `esc()` 里有 `/[&<>"']/g` —— 当它是字符串起始，掩码器会一路吞到下一个引号，
//! **把后面大段代码连声明一起吃掉**：实测「`function nowTime()` 在修前树上提取不出来」
//! （`base` 130 条 vs 修后 129 条，差值**只有** `DAY_LABELS`），症状是**漏报**而不是假红。
//! 所以掩码器按「前一个有意义字符是不是运算符 / 开括号」判断 `/` 是否开始正则。

use std::collections::BTreeSet;

/// 声明语料：`ui/js/*.js` —— **只有这里**找声明。
/// `include_str!` 在编译期读入 ⇒ 改任一 `ui/js/*.js` 都会重编并重跑本门禁。
pub(crate) const JS_SOURCES: &[(&str, &str)] = &[
    ("app.js", include_str!("../ui/js/app.js")),
    ("api.js", include_str!("../ui/js/api.js")),
    ("data.js", include_str!("../ui/js/data.js")),
    ("i18n.js", include_str!("../ui/js/i18n.js")),
];

/// 读者语料的标记那一半（见模块文档：⛔ 刻意不含 README / CSS）。
const INDEX_HTML: &str = include_str!("../ui/index.html");

/// 一枚顶层声明（名字 ＋ 在**自己文件**里的行号，报错时点名用）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Declaration {
    file: String,
    line: usize,
    name: String,
}

/// 一枚没有读者的声明（连同它在整个读者语料里的出现次数，恒 < 2）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Offender {
    file: String,
    line: usize,
    name: String,
    occurrences: usize,
}

/// 门禁读数：名册大小 ＋ 违规清单（门禁自己把看到的东西印出来，别让主张只能靠断言名来读）。
#[derive(Debug, Default)]
struct Report {
    declarations: Vec<Declaration>,
    offenders: Vec<Offender>,
}

// ------------------------------------------------------------------ 词法扫描 ---

/// 这个 `/` 是否**开始一个正则字面量**（而不是除号）。
///
/// 判据＝前一个有意义字符是不是运算符 / 开括号一类：`( , = : [ ! & | ? { } ; + - * % ~ ^ < >`，
/// 或者它前面什么都没有。除号前面的字符总是标识符字符 / `)` / `]` / 数字 / 引号，落不进这个集合。
/// 必须做这一步的原因是 `esc()` 的 `/[&<>"']/g`（见模块文档的「掩码必须跳正则字面量」）。
fn js_regex_starts(b: &[u8], i: usize) -> bool {
    let mut k = i;
    while k > 0 {
        let c = b[k - 1];
        if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
            k -= 1;
            continue;
        }
        return matches!(
            c,
            b'(' | b','
                | b'='
                | b':'
                | b'['
                | b'!'
                | b'&'
                | b'|'
                | b'?'
                | b'{'
                | b'}'
                | b';'
                | b'+'
                | b'-'
                | b'*'
                | b'%'
                | b'~'
                | b'^'
                | b'<'
                | b'>'
        );
    }
    true
}

/// `i` 指向正则字面量的起始 `/`；返回到它之后（含 flags）。字符类 `[...]` 里的 `/` 不算收尾；
/// 换行即放弃（正则不能跨行 ⇒ 那说明这里其实是除号，别吞代码）。
fn js_skip_regex(b: &[u8], i: usize) -> usize {
    let mut j = i + 1;
    let mut in_class = false;
    while j < b.len() {
        let c = b[j];
        if c == b'\\' {
            j += 2;
            continue;
        }
        if c == b'\n' {
            return j;
        }
        if c == b'[' {
            in_class = true;
        } else if c == b']' {
            in_class = false;
        } else if c == b'/' && !in_class {
            return j + 1;
        }
        j += 1;
    }
    b.len()
}

/// 把 `out[from..to)` 涂成空格（保留换行，好让行号还准）。
fn blank(out: &mut [u8], from: usize, to: usize) {
    for byte in out.iter_mut().take(to).skip(from) {
        if *byte != b'\n' {
            *byte = b' ';
        }
    }
}

/// **等长掩码**：注释 / 正则字面量 / 字符串与模板字面量里的字节全换成空格，**偏移量不变**。
///
/// 为什么必须掩码：本仓的注释到处**逐字引用代码**（本轮就是这么被咬的 —— 一条写着
/// `nowTime()` 的注释会把已删的函数重新种回语料）。掩码只服务于**提取**，计数仍在原始文本上。
fn mask_js(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        if c == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            let end = match find_from(b, i + 2, b"*/") {
                Some(p) => p + 2,
                None => b.len(),
            };
            blank(&mut out, i, end);
            i = end;
        } else if c == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            let end = match b[i..].iter().position(|&x| x == b'\n') {
                Some(p) => i + p,
                None => b.len(),
            };
            blank(&mut out, i, end);
            i = end;
        } else if c == b'/' && js_regex_starts(b, i) {
            let end = js_skip_regex(b, i);
            blank(&mut out, i, end);
            i = end;
        } else if c == b'"' || c == b'\'' || c == b'`' {
            let quote = c;
            let mut j = i + 1;
            while j < b.len() && b[j] != quote {
                if b[j] == b'\\' {
                    j += 1;
                }
                j += 1;
            }
            let end = (j + 1).min(b.len());
            blank(&mut out, i, end);
            i = end;
        } else {
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 在 `b` 里从 `from` 起找 needle 的首个出现位置。
fn find_from(b: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || from >= b.len() || b.len() - from < needle.len() {
        return None;
    }
    (from..=b.len() - needle.len()).find(|&i| &b[i..i + needle.len()] == needle)
}

/// 这个字节算不算标识符字符（ASCII 字母数字 / `_` / `$`）。越界的 `0` 不算。
fn is_ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$'
}

/// 取 `s` 开头的那个标识符（不是标识符开头就返回 `None`）。
fn ident_prefix(s: &str) -> Option<String> {
    let name: String = s
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
        .collect();
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        None
    } else {
        Some(name)
    }
}

/// 顶层声明名册：缩进 ≤ 2、以声明关键字开头的行（在**掩码后**的源码上取，故注释/字符串里的
/// 「声明」不会被读成声明）。
fn top_level_declarations(src: &str) -> Vec<(String, usize)> {
    let masked = mask_js(src);
    let mut out = Vec::new();
    for (idx, line) in masked.split('\n').enumerate() {
        let indent = line.chars().take_while(|c| *c == ' ' || *c == '\t').count();
        if indent > 2 {
            continue;
        }
        let rest = line.trim_start_matches([' ', '\t']);
        let rest = rest.strip_prefix("async ").unwrap_or(rest);
        let name = rest
            .strip_prefix("function ")
            .or_else(|| rest.strip_prefix("class "))
            .or_else(|| rest.strip_prefix("const "))
            .or_else(|| rest.strip_prefix("let "))
            .or_else(|| rest.strip_prefix("var "))
            .and_then(ident_prefix);
        if let Some(name) = name {
            out.push((name, idx + 1));
        }
    }
    out
}

/// `name` 在 `hay` 里以**标识符边界**出现了几次。
///
/// 「边界」＝ 前后那个字节都不是标识符字符（非 ASCII 字节天然算边界，故 `「fmtM」` 会被数到）。
fn identifier_count(hay: &str, name: &str) -> usize {
    let hb = hay.as_bytes();
    let nb = name.as_bytes();
    if nb.is_empty() || hb.len() < nb.len() {
        return 0;
    }
    let mut n = 0usize;
    for i in 0..=(hb.len() - nb.len()) {
        if &hb[i..i + nb.len()] != nb {
            continue;
        }
        let before = if i == 0 { 0 } else { hb[i - 1] };
        let after = if i + nb.len() == hb.len() {
            0
        } else {
            hb[i + nb.len()]
        };
        if !is_ident_byte(before) && !is_ident_byte(after) {
            n += 1;
        }
    }
    n
}

/// 判官：读者语料 ＝ 标记（`markup`）＋ 每份脚本自身，用 `\n` 连接。
fn judge(sources: &[(&str, &str)], markup: &str) -> Report {
    let readers = std::iter::once(markup.to_string())
        .chain(sources.iter().map(|(_, src)| (*src).to_string()))
        .collect::<Vec<_>>()
        .join("\n");
    let mut report = Report::default();
    for (file, src) in sources {
        for (name, line) in top_level_declarations(src) {
            let occurrences = identifier_count(&readers, &name);
            report.declarations.push(Declaration {
                file: (*file).to_string(),
                line,
                name: name.clone(),
            });
            if occurrences < 2 {
                report.offenders.push(Offender {
                    file: (*file).to_string(),
                    line,
                    name,
                    occurrences,
                });
            }
        }
    }
    report
}

/// 把违规清单压成一行一句的判词（点名**文件 ＋ 行号 ＋ 名字 ＋ 出现次数**）。
fn describe(offenders: &[Offender]) -> String {
    offenders
        .iter()
        .map(|o| {
            format!(
                "{}:{} `{}`（全语料出现 {} 次）",
                o.file, o.line, o.name, o.occurrences
            )
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// 磁盘上的 `ui/js/*.js`（从**文件系统**派生，与 `JS_SOURCES` 这份编译期名册互为对证）。
fn js_files_on_disk() -> BTreeSet<String> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/js");
    let mut out = BTreeSet::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".js") {
            out.insert(name);
        }
    }
    out
}

// ---------------------------------------------------------------------- 测试 ---

/// **轴**：`ui/js/*.js` 的每个顶层声明都必须有读者。
#[test]
fn every_js_declaration_has_a_reader() {
    let report = judge(JS_SOURCES, INDEX_HTML);
    assert!(
        report.offenders.is_empty(),
        "这些顶层声明在 `ui/index.html` ＋ `ui/js/*.js` 里只出现过一次（＝只有声明自己），\
         即声明了却没有任何读者 —— 删掉它，或在 `ui/README.md` 里说明为什么留：{}",
        describe(&report.offenders)
    );
}

/// 名册必与磁盘对得上：新增 `ui/js/*.js` 而忘了登记 ⇒ 红（fail-closed，而不是静默不设防）。
#[test]
fn the_roster_covers_every_js_file_on_disk() {
    let on_disk = js_files_on_disk();
    assert!(
        !on_disk.is_empty(),
        "读不到 ui/js/ 下的脚本 —— 磁盘名册读法坏了（CARGO_MANIFEST_DIR = {}）",
        env!("CARGO_MANIFEST_DIR")
    );
    let in_roster: BTreeSet<String> = JS_SOURCES.iter().map(|(n, _)| (*n).to_string()).collect();
    assert_eq!(
        in_roster, on_disk,
        "编译期名册与磁盘上的 ui/js/*.js 不一致：新增脚本要同时加进 JS_SOURCES"
    );
}

/// 提取器的牙齿：注释 / 字符串 / 正则里的「声明」不算声明；真声明算。
#[test]
fn the_declaration_extractor_has_teeth() {
    let names = |src: &str| -> Vec<String> {
        top_level_declarations(src)
            .into_iter()
            .map(|(n, _)| n)
            .collect()
    };

    // 真声明：模块体（缩进 2）与文件级（缩进 0）都算。
    assert_eq!(
        names("(function () {\n  const alpha = 1;\n  function beta() {}\n  let gamma = 2;\n  var delta = 3;\n  class Epsilon {}\n  async function zeta() {}\n})();\n"),
        vec!["alpha", "beta", "gamma", "delta", "Epsilon", "zeta"]
    );
    // 缩进更深 ⇒ 不是顶层（函数体里的声明）。
    assert_eq!(
        names("(function () {\n  function outer() {\n    const inner = 1;\n  }\n})();\n"),
        vec!["outer"]
    );
    // 注释里的「声明」不算 —— 本仓的注释到处逐字引用代码（这正是本门禁的存在理由）。
    assert_eq!(
        names("(function () {\n  // const commented = 1;\n  /*\n  function blocked() {}\n  */\n})();\n"),
        Vec::<String>::new()
    );
    // 字符串 / 模板字面量里的「声明」不算。
    assert_eq!(
        names("(function () {\n  const s = \"const inString = 1;\";\n  const t = `\n  function inTemplate() {}\n`;\n})();\n"),
        vec!["s", "t"]
    );
    // 正则字面量里的引号**不得**让掩码器吞掉后面的代码（坑 #641 同族：`esc()` 的 `/[&<>\"']/g`）。
    assert_eq!(
        names("(function () {\n  const esc = (s) => String(s).replace(/[&<>\"']/g, (c) => (\"\" + c));\n  function after() {}\n})();\n"),
        vec!["esc", "after"]
    );
    // 解构 / 多个声明的第二名字不入名册（刻意的窄口径，见模块文档的射程）。
    assert_eq!(
        names("(function () {\n  const { a, b } = o;\n  let x = 1, y = 2;\n})();\n"),
        vec!["x"]
    );
}

/// 判官自己：注入一枚死声明必须被点名；活的不许被点名。
#[test]
fn the_judge_detects_an_injected_dead_declaration() {
    let markup = "<div id=\"live\"></div>\n";
    let live = "(function () {\n  const dead = 1;\n  const used = 2;\n  used;\n})();\n";
    let report = judge(&[("app.js", live)], markup);
    assert_eq!(report.declarations.len(), 2, "两枚声明都该入名册");
    assert_eq!(
        report.offenders,
        vec![Offender {
            file: "app.js".into(),
            line: 2,
            name: "dead".into(),
            occurrences: 1,
        }],
        "只有 `dead` 该被点名：{}",
        describe(&report.offenders)
    );
    assert!(describe(&report.offenders).contains("`dead`"));

    // 读者语料是**并集**：在另一份脚本里读到也算有读者。
    let consumer = "(function () {\n  const shared = 1;\n  doIt(shared);\n})();\n";
    let consumer2 = "(function () {\n  otherScriptUses(shared);\n})();\n";
    let report = judge(&[("api.js", consumer), ("data.js", consumer2)], markup);
    assert!(
        report.offenders.is_empty(),
        "跨文件读点应算读者：{}",
        describe(&report.offenders)
    );

    // 阳性对照：往标记里补一个读点，死声明就活了 —— 证明计数真的走的是并集而不是单份文件。
    let report = judge(&[("app.js", live)], "<div id=\"dead\"></div>\n");
    assert!(
        report.offenders.is_empty(),
        "标记里的读点应算读者：{}",
        describe(&report.offenders)
    );
}

/// 计数器的牙齿：标识符边界（`fmtM` 不得被 `fmtMega` 里的子串命中）。
#[test]
fn the_identifier_counter_respects_boundaries() {
    assert_eq!(identifier_count("fmtM", "fmtM"), 1);
    assert_eq!(identifier_count("fmtMega", "fmtM"), 0);
    assert_eq!(identifier_count("fmtTokens", "fmtM"), 0);
    assert_eq!(identifier_count("a.fmtM b", "fmtM"), 1);
    assert_eq!(identifier_count("a.fmtM(b, fmtM)", "fmtM"), 2);
    // 非 ASCII 字节天然是边界（本仓中文注释紧贴标识符）。
    assert_eq!(identifier_count("「fmtM」", "fmtM"), 1);
    // 模板 / 属性 / 方法名都算（门禁只看文本）。
    assert_eq!(identifier_count("obj.fmtM", "fmtM"), 1);
    assert_eq!(identifier_count("", "fmtM"), 0);
}

/// 逐腿读数（把门禁看到的东西印出来）。
#[test]
fn leg_readings() {
    let report = judge(JS_SOURCES, INDEX_HTML);
    let mut names: Vec<&str> = report
        .declarations
        .iter()
        .map(|d| d.name.as_str())
        .collect();
    names.sort_unstable();
    names.dedup();
    println!(
        "声明语料 = {:?}",
        JS_SOURCES.iter().map(|(n, _)| *n).collect::<Vec<_>>()
    );
    println!("磁盘上的 ui/js/*.js = {:?}", js_files_on_disk());
    println!(
        "提取到的顶层声明 = {} 条（去重名字 {} 个）",
        report.declarations.len(),
        names.len()
    );
    println!(
        "违规 = {}",
        if report.offenders.is_empty() {
            "无".to_string()
        } else {
            describe(&report.offenders)
        }
    );
    println!(
        "读者语料字节数 = {}（标记）＋ {}",
        INDEX_HTML.len(),
        JS_SOURCES.iter().map(|(_, s)| s.len()).sum::<usize>()
    );
}
