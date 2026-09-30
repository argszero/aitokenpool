//! 选项模板转义门禁（R71 落地，承 R69 侦察工作单）：**`<option>` 模板里插进去的裸变量必须过 `esc()`**。
//!
//! 起因：`ui/js/app.js` 的四个下拉框把**服务端 / 配置字符串**直接拼进 `value="…"` 与选项文本，
//! 而同一个文件里、同一个字段、同一个构造器形态的另外几处**全部**转义。这不是取舍，是漂移：
//!
//! | 行 | 构造器 | 属性值槽 | 文本槽 |
//! |---|---|---|---|
//! | `:884` | 市场「厂商」筛选 | 裸 `p` ❌ → `esc(p)` | 裸 `p` ❌ → `esc(p)` |
//! | `:1103` | 上架表单「模型」 | 裸 `m.model` ❌ → `esc(m.model)` | 裸 `m.model` ❌ → `esc(m.model)` |
//! | `:1109` | 上架表单「Plan」 | 裸 `pl.id` ❌ → `esc(pl.id)` | 已转义 ✅ |
//! | `:1127` | 上架表单「厂商」 | 裸 `p` ❌ → `esc(p)` | 已转义 ✅ |
//! | `:2885` | 部门下拉 | 裸 `d.id` ❌ → `esc(d.id)` | 已转义 ✅ |
//! | `:2539-2540` | 设置页「默认模型」 | `esc(m)` ✅ | `esc(m)` ✅ |
//! | `:2185` | 交易表表头筛选 | `esc(val)` ✅ | `esc(lbl)` ✅ |
//!
//! 作者在 `:1109`/`:1127` **已经**给文本槽加了 `esc(`，只是属性值那半没加 —— **同一句判据只加了
//! 一半**（与 `marketplace-success-sentinel`（#308）同形）。转义还是这个文件的成文约定：
//! `ui/README.md` 五处写「先 `esc()`」。
//!
//! # 判据（射程如实）
//!
//! 在 `ui/js/*.js` 里枚举**字符串字面量**，凡内容里有 `<option` 而**没有** `</option>` 的
//! （＝一个选项标签在这里被打开），它**紧跟其后的那个操作数**就是属性值槽；凡内容里有
//! `</option>` 的，它**紧挨着的前一个操作数**就是选项文本槽。两处操作数若是个**裸取值**
//! （标识符或点号成员路径，如 `p` / `m.model` / `pl.id`），就必须被 `esc(` 包住。
//!
//! ⛔ **不检查**：函数调用形式（`T(…)` / `esc(…)` / `D.fmt(…)` / `String(…)`）一律放行 ——
//! 按约定它们产出的是**可信串或预渲染片段**（空选项的标签就是 `T("…")`）。这条豁免是**故意**
//! 的，也是本门禁的射程边界：它证「没有裸变量被原样插进选项模板」，**不证**每个调用返回值
//! 都已经转过义（将来有人写个 `String(p)` 帮凶，本门禁看不见）。同理，它只看 `ui/js/*.js`，
//! 不看 `ui/index.html` 里的静态选项（那些没有插值）。
//!
//! # 为什么不是快照
//!
//! 名册（哪些字面量算选项模板）**从源码现读**，期望值零硬编码；另有一条**结构性阳性对照**：
//! 源码里 `<option` / `</option>` 的**出现次数**必须与「扫描器在字符串字面量里数到的」相等 ——
//! 它同时证两件事：扫描器没有漏掉字面量，且没有注释 / 正则里的假命中（本仓注释到处逐字引用代码）。

use crate::js_gate::{find_from, js_regex_starts, js_skip_regex, mask_js, JS_SOURCES};

/// 一枚字符串字面量（偏移量是**原始源码**的字节位置；掩码等长，不改偏移）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Literal {
    start: usize,
    end: usize,
    content: String,
    line: usize,
}

/// 被检查的槽位：打开标签之后的属性值槽，或闭合标签之前的文本槽。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    AttrValue,
    Text,
}

impl Slot {
    fn label(self) -> &'static str {
        match self {
            Slot::AttrValue => "属性值槽",
            Slot::Text => "文本槽",
        }
    }
}

/// 一枚被检查的插值槽（连同它读到的操作数原文）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Site {
    file: String,
    line: usize,
    slot: Slot,
    operand: String,
    escaped: bool,
}

/// 一枚违规：裸取值直接插进了选项模板。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Offender {
    file: String,
    line: usize,
    slot: Slot,
    operand: String,
}

/// 门禁读数：查了多少槽、其中多少已转义、违规清单。
#[derive(Debug, Default)]
struct Report {
    sites: Vec<Site>,
    offenders: Vec<Offender>,
}

impl Report {
    fn push(&mut self, file: &str, line: usize, slot: Slot, operand: String) {
        if operand.is_empty() {
            return;
        }
        let escaped = operand.starts_with("esc(");
        self.sites.push(Site {
            file: file.to_string(),
            line,
            slot,
            operand: operand.clone(),
            escaped,
        });
        if !escaped && is_bare_value(&operand) {
            self.offenders.push(Offender {
                file: file.to_string(),
                line,
                slot,
                operand,
            });
        }
    }
}

/// 行号（1 起，字节偏移换算）。
fn line_of(src: &str, offset: usize) -> usize {
    src[..offset].matches('\n').count() + 1
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r')
}

/// 枚举 `src` 里的字符串 / 模板字面量。注释与正则字面量按 `js_gate` 那套判据跳过
/// （`esc()` 自己的 `/[&<>"']/g` 会被当字符串起始 —— 坑 #641/#778）。
fn scan_literals(src: &str) -> Vec<Literal> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        if c == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            i = find_from(b, i + 2, b"*/").map(|p| p + 2).unwrap_or(b.len());
        } else if c == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            i = b[i..]
                .iter()
                .position(|&x| x == b'\n')
                .map(|p| i + p)
                .unwrap_or(b.len());
        } else if c == b'/' && js_regex_starts(b, i) {
            i = js_skip_regex(b, i);
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
            let content = String::from_utf8_lossy(&b[i + 1..j.min(b.len())]).into_owned();
            out.push(Literal {
                start: i,
                end,
                content,
                line: line_of(src, i),
            });
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

/// 字面量**之后**紧跟的那个操作数（跳过空白与一个 `+`；括号 / 方括号里不算断点）。
/// 不在拼接表达式里（后面不是 `+`）就返回 `None`。
fn operand_after(masked: &str, pos: usize) -> Option<String> {
    let b = masked.as_bytes();
    let mut i = pos;
    while i < b.len() && is_space(b[i]) {
        i += 1;
    }
    if i >= b.len() || b[i] != b'+' {
        return None;
    }
    i += 1;
    while i < b.len() && is_space(b[i]) {
        i += 1;
    }
    let start = i;
    let mut depth = 0i32;
    while i < b.len() {
        match b[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            b'+' | b';' | b',' if depth == 0 => break,
            _ => {}
        }
        i += 1;
    }
    Some(masked[start..i].trim().to_string())
}

/// 字面量**之前**紧挨的那个操作数（向后跳过空白与一个 `+`；括号要配平）。
fn operand_before(masked: &str, pos: usize) -> Option<String> {
    let b = masked.as_bytes();
    let mut i = pos;
    while i > 0 && is_space(b[i - 1]) {
        i -= 1;
    }
    if i == 0 || b[i - 1] != b'+' {
        return None;
    }
    i -= 1;
    while i > 0 && is_space(b[i - 1]) {
        i -= 1;
    }
    let end = i;
    let mut depth = 0i32;
    while i > 0 {
        match b[i - 1] {
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            b'+' | b';' | b',' if depth == 0 => break,
            _ => {}
        }
        i -= 1;
    }
    Some(masked[i..end].trim().to_string())
}

fn ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'$'
}

fn ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$'
}

/// 这个操作数是不是**裸取值**：`p` / `m.model` / `pl.id` 这种标识符或点号成员路径。
/// 带括号（`esc(x)` / `T(…)` / `(a ? b : c)`）、带下标、带运算符的一律不算。
fn is_bare_value(op: &str) -> bool {
    let b = op.as_bytes();
    if b.is_empty() || !ident_start(b[0]) {
        return false;
    }
    let mut i = 1usize;
    while i < b.len() && ident_byte(b[i]) {
        i += 1;
    }
    while i < b.len() {
        if b[i] != b'.' {
            return false;
        }
        i += 1;
        if i >= b.len() || !ident_start(b[i]) {
            return false;
        }
        while i < b.len() && ident_byte(b[i]) {
            i += 1;
        }
    }
    true
}

/// 判官：`ui/js/*.js` 里每个选项模板的插值槽。
fn judge(sources: &[(&str, &str)]) -> Report {
    let mut report = Report::default();
    for (file, src) in sources {
        let masked = mask_js(src);
        for lit in scan_literals(src) {
            let opens = lit.content.contains("<option");
            let closes = lit.content.contains("</option>");
            if opens && !closes {
                if let Some(op) = operand_after(&masked, lit.end) {
                    report.push(file, lit.line, Slot::AttrValue, op);
                }
            }
            if closes {
                if let Some(op) = operand_before(&masked, lit.start) {
                    report.push(file, lit.line, Slot::Text, op);
                }
            }
        }
    }
    report
}

/// 把违规压成一行一句的判词（点名**文件 ＋ 行号 ＋ 槽位 ＋ 读到的操作数**）。
fn describe(offenders: &[Offender]) -> String {
    offenders
        .iter()
        .map(|o| {
            format!(
                "{}:{} {} `{}`（裸取值插进选项模板，应为 `esc({})`）",
                o.file,
                o.line,
                o.slot.label(),
                o.operand,
                o.operand
            )
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

// ---------------------------------------------------------------------- 测试 ---

/// **轴**：`ui/js/*.js` 的每个选项模板插值槽都必须是 `esc(…)`。
#[test]
fn every_option_template_interpolation_is_escaped() {
    let report = judge(JS_SOURCES);
    let escaped = report.sites.iter().filter(|s| s.escaped).count();
    println!(
        "选项模板门禁：查到 {} 个插值槽（已转义 {} / 裸取值 {}）",
        report.sites.len(),
        escaped,
        report.sites.len() - escaped
    );
    assert!(
        report.sites.len() >= 10,
        "只查到 {} 个插值槽 —— 扫描器没读到语料（门禁不能因为看不见而变绿）",
        report.sites.len()
    );
    assert!(
        escaped > 0,
        "一个已转义的槽都没读到 —— 阳性对照失败，判据大概是坏的"
    );
    assert!(
        report.offenders.is_empty(),
        "选项模板里有 {} 处裸取值插值：{}",
        report.offenders.len(),
        describe(&report.offenders)
    );
}

/// **结构性阳性对照**：源码里的 `<option` / `</option>` 出现次数，必须与「扫描器在字符串
/// 字面量里数到的」**相等**。
///
/// 等式的两侧各有含义：左边多出来 ⇒ 有标签在注释或正则里（本仓注释到处逐字引用代码），
/// 右边少 ⇒ 扫描器漏了字面量。两侧都不需要任何硬编码的期望值。
#[test]
fn the_scanner_reads_every_option_tag_the_sources_carry() {
    let mut raw_opens = 0usize;
    let mut lit_opens = 0usize;
    let mut raw_closes = 0usize;
    let mut lit_closes = 0usize;
    for (file, src) in JS_SOURCES {
        let literals = scan_literals(src);
        let mut opens = 0usize;
        let mut closes = 0usize;
        for lit in &literals {
            opens += lit.content.matches("<option").count();
            closes += lit.content.matches("</option>").count();
        }
        assert_eq!(
            src.matches("<option").count(),
            opens,
            "{file}：源码里的 `<option` 与「字符串字面量里的」对不上 ⇒ 要么有注释/正则里的假命中，要么扫描器漏了字面量"
        );
        assert_eq!(
            src.matches("</option>").count(),
            closes,
            "{file}：`</option>` 同上"
        );
        raw_opens += src.matches("<option").count();
        lit_opens += opens;
        raw_closes += src.matches("</option>").count();
        lit_closes += closes;
    }
    assert_eq!(raw_opens, lit_opens);
    assert_eq!(raw_closes, lit_closes);
    assert!(
        raw_opens > 0,
        "整份语料里一个选项标签都没有 —— 名册空了，门禁等于没在守"
    );
    println!("选项标签：<option {raw_opens} / </option> {raw_closes}（全部在字符串字面量里）");
}

/// **有牙**：在真语料上把一处**本来已转义**的选项构造器还原成裸插值（计数中性：只改一处、
/// 不增删行），判官必须点名**恰好**那两个槽（属性值槽 ＋ 文本槽），且都是被改的那个变量。
///
/// 锚点刻意选**修前修后都在**的那一处（设置页「默认模型」下拉，`:2540`）：这样这条仪器在
/// A/B 两棵树上都跑得动、都为绿 —— 于是「修前树红的是什么」这个问题只有一个答案（轴那条），
/// 不会被一条**锚点依赖**的测试搅混（坑 #721/#730）。
#[test]
fn the_judge_reddens_on_a_bare_interpolation_in_the_real_corpus() {
    let src = JS_SOURCES
        .iter()
        .find(|(f, _)| *f == "app.js")
        .map(|(_, s)| *s)
        .expect("app.js 必须在语料里");
    let escaped = r#"'<option value="' + esc(m) + '">' + esc(m) + "</option>""#;
    assert!(
        src.contains(escaped),
        "语料里找不到「默认模型」那条已转义的选项构造器 —— 判据的锚点漂了"
    );
    let bare = r#"'<option value="' + m + '">' + m + "</option>""#;
    let offset = src.find(escaped).expect("锚点偏移");
    let mutated = src.replacen(escaped, bare, 1);
    assert_ne!(mutated, src);
    let line = line_of(&mutated, offset);

    // 注入前：那一行不该有任何违规（否则「注入造成了它」这句话就不成立）。
    let before = judge(&[("app.js", src)]);
    assert!(
        before.offenders.iter().all(|o| o.line != line),
        "注入之前那一行就已经是违规了：{}",
        describe(&before.offenders)
    );

    // 注入后：**那一行**恰好两个槽中招（属性值 ＋ 文本），操作数就是被剥掉 `esc(` 的那个变量。
    // 只认那一行，是为了让这条仪器在 A/B 两棵树上都成立 —— 语料别处还有没有违规是**轴那条**
    // 测试的事（坑 #721/#730）。
    let report = judge(&[("app.js", mutated.as_str())]);
    let on_line: Vec<&Offender> = report.offenders.iter().filter(|o| o.line == line).collect();
    assert_eq!(
        on_line.len(),
        2,
        "还原一处裸插值应当恰好点名那一行的两个槽，实际：{}",
        describe(&report.offenders)
    );
    assert!(on_line.iter().all(|o| o.operand == "m"));
    assert_eq!(
        on_line.iter().filter(|o| o.slot == Slot::AttrValue).count(),
        1,
        "属性值槽应当中招一次"
    );
    assert_eq!(
        on_line.iter().filter(|o| o.slot == Slot::Text).count(),
        1,
        "文本槽应当中招一次"
    );
}

/// 掩码是承重的：把一条**裸插值**的选项构造器写进注释，判官必须看不见它
/// （本仓注释到处逐字引用代码；不掩码就会把注释当违规或把真代码当合规）。
#[test]
fn the_judge_ignores_builders_that_only_exist_in_comments() {
    let synthetic = r#"
function f(providers) {
  // 反例（注释里）：providers.map((p) => '<option value="' + p + '">' + p + "</option>")
  el.innerHTML = providers.map((p) => '<option value="' + esc(p) + '">' + esc(p) + "</option>").join("");
}
"#;
    let report = judge(&[("synthetic.js", synthetic)]);
    assert!(
        report.offenders.is_empty(),
        "注释里的构造器被当成违规了（掩码没生效）：{}",
        describe(&report.offenders)
    );
    assert_eq!(report.sites.len(), 2, "真代码里的两个槽应当都被读到");

    // 对照：把同一句话从注释里放出来 ⇒ 立刻红。
    let uncommented = synthetic.replace("  // 反例（注释里）：", "  ");
    let report = judge(&[("synthetic.js", uncommented.as_str())]);
    assert_eq!(
        report.offenders.len(),
        2,
        "注释标记去掉之后应当点名两个裸取值槽，实际：{}",
        describe(&report.offenders)
    );
}

/// 判据本身的自证：哪些操作数算「裸取值」。
#[test]
fn bare_value_is_the_thing_that_needs_escaping() {
    for ok in ["p", "m.model", "pl.id", "d.id", "$el.value"] {
        assert!(is_bare_value(ok), "`{ok}` 应当算裸取值");
    }
    for no in [
        "esc(p)",
        "T(\"x\")",
        "(a ? b : c)",
        "a[0]",
        "a + b",
        "",
        "String(p)",
    ] {
        assert!(
            !is_bare_value(no),
            "`{no}` 不该算裸取值（调用/表达式一律放行）"
        );
    }
}
