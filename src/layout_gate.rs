//! 布局门禁（R128）：**`<label>` 的盒子由它自己的类声明 —— 按元素名命中的「字段标签」规则
//! 不得把排布强加给带类的 label。**
//!
//! # 起因
//!
//! `ui/css/style.css` 里那条字段标签规则原本是**无条件**的
//! `.form label { display: flex; flex-direction: column; … }`（选择器权重 **0,1,1**），
//! 于是它压过组件层自己的排布（两者都是 **0,1,0**，权重低的先输，与书写顺序无关）：
//!
//! | 受害类 | 症状 | 来历 |
//! |---|---|---|
//! | `.check-line` | 设置页「通知」3 枚复选 + 「偏好 · 表格密度」2 枚单选被**竖排**：钮叠在文字上方并居中 | #157 只把 markup 的 `class="check"` 改成原型规范名（#154 引入的组件层），**没改样式表**里那条把复选掰回 row 的 `.form label.check`，它从此不再命中任何元素 |
//! | `.chip` | 共享上架表单的星期 chip 从药丸变成**圆饼** | #32 把 chip 从 `<button>` 换成 `<label>`，#154 给它的 `display:inline-flex` 从此被 `display:flex` 覆盖 |
//!
//! 两处受害者来历不同、根因同一个：**一条泛化规则给「所有 label」定了盒子。**
//!
//! # 守的是什么
//!
//! 不是那两个具体类，而是它们共同的形状：
//!
//! * **R1（名册，从 `ui/index.html` 派生）** —— 每个用在 `<label>` 上的类，样式表必须有一条
//!   选择器含该 `.类` 的规则**声明了 `display`**：label 的盒子由它的类给全，不许白拿泛化规则的。
//! * **R2（机制）** —— 任何规则，若其选择器的**最右复合选择器**是「裸 `label`」（只有元素名与
//!   `:not(…)` 守卫，没有类 / 属性 / id 限定）且声明了 `display` 或 `flex-direction`，
//!   就必须带一个能排除「带 class 的元素」的 `:not(…)` 守卫（`:not([class])`）。
//!
//! 两条合起来就是那条不变量：**泛化的字段标签规则只服务没有类的 label。**
//!
//! 竞争修法为什么不够：在 `.check-line` 上补 `!important`、或给每个受害类补一条
//! `.form label.X`，都只堵**当下这两个洞**；下一个组件类用在 `<label>` 上时同样的静默竖排会再来。
//! R1 会在那个新类上先红。
//!
//! # 期望值全部派生（⛔ 不写快照）
//!
//! 类名册来自 `ui/index.html`、规则来自 `ui/css/style.css`，两者都在编译期读入
//! （仅测试期编译、零新依赖 —— 与 `i18n_pack` / `catalog_gate` / `deploy_gate` / `smtp_port_gate` 同型）。
//!
//! # 射程（如实）
//!
//! 本门禁是**词法**的：它证「谁有权给 label 定盒子」，**不**证屏幕上的排布 ——
//! 那一半归仓外的 jsdom 探针（逐形状的 `display`/`flex-direction` 对照表）与 headless Chrome
//! 的前后截图。它也**不**实现 CSS 层叠（不比较权重、不排序），只要求那条泛化规则
//! **命中不到**带类的 label；`:not([class])` 之外等价的写法（如 `label[class=""]`）本门禁
//! 不识别，会报成违规 —— 这是刻意的窄口径，宁可让人来改这一行。

use std::collections::BTreeSet;

/// 编译期读入的两份载体：markup 给类名册，样式表给规则。
const INDEX_HTML: &str = include_str!("../ui/index.html");
const STYLE_CSS: &str = include_str!("../ui/css/style.css");

// ------------------------------------------------------------------ 词法扫描 ---

/// 等长掩码：把 CSS 注释与字符串字面量里的字节换成空格，**偏移量不变**（保留换行以便报行号）。
///
/// 为什么必须掩码：样式表里到处是**解释性注释**，注释里常常逐字写着选择器
/// （本仓就有 `.stat:hover`、`.bar-label` 这类历史注释）—— 不掩码就会把散文读成规则。
fn mask_css(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            out[i] = b' ';
            out[i + 1] = b' ';
            i += 2;
            while i < b.len() {
                if b[i] == b'*' && i + 1 < b.len() && b[i + 1] == b'/' {
                    out[i] = b' ';
                    out[i + 1] = b' ';
                    i += 2;
                    break;
                }
                if b[i] != b'\n' {
                    out[i] = b' ';
                }
                i += 1;
            }
        } else if b[i] == b'"' || b[i] == b'\'' {
            let quote = b[i];
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    if b[i] != b'\n' {
                        out[i] = b' ';
                    }
                    i += 1;
                    if i < b.len() {
                        if b[i] != b'\n' {
                            out[i] = b' ';
                        }
                        i += 1;
                    }
                    continue;
                }
                if b[i] == quote {
                    i += 1;
                    break;
                }
                if b[i] != b'\n' {
                    out[i] = b' ';
                }
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    String::from_utf8(out).expect("掩码只把字节换成空格，UTF-8 不变")
}

/// 一条 CSS 规则：选择器列表原文 + 声明块原文（注释已被掩码）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Rule {
    selector: String,
    body: String,
}

/// 取出所有「**选择器 { 声明 }**」规则；at-rule 容器（`@media … { … }`）自身不算规则，
/// 它里面的规则照常取出（靠「声明块里不再有 `{`」区分两者）。
fn css_rules(css: &str) -> Vec<Rule> {
    let masked = mask_css(css);
    let bytes = masked.as_bytes();
    let mut rules = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new(); // (开括号字节位, 选择器起点)
    let mut cursor = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                stack.push((i, cursor));
                cursor = i + 1;
            }
            b'}' => {
                if let Some((open, start)) = stack.pop() {
                    let selector = masked[start..open].trim();
                    let body = &masked[open + 1..i];
                    if !selector.is_empty() && !selector.starts_with('@') && !body.contains('{') {
                        rules.push(Rule {
                            selector: selector.to_string(),
                            body: body.to_string(),
                        });
                    }
                }
                cursor = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    rules
}

/// 按**顶层**逗号切开选择器列表（括号/方括号里的逗号不算分隔符）。
fn split_selector_list(selector: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    for ch in selector.chars() {
        match ch {
            '(' | '[' => {
                depth += 1;
                cur.push(ch);
            }
            ')' | ']' => {
                depth -= 1;
                cur.push(ch);
            }
            ',' if depth == 0 => {
                if !cur.trim().is_empty() {
                    parts.push(cur.trim().to_string());
                }
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() {
        parts.push(cur.trim().to_string());
    }
    parts
}

/// 最右复合选择器（`.form label:not([class])` → `label:not([class])`）。
fn rightmost_compound(selector: &str) -> &str {
    let selector = selector.trim();
    let bytes = selector.as_bytes();
    let mut depth = 0i32;
    let mut cut = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            b'>' | b'+' | b'~' | b' ' | b'\t' | b'\n' if depth == 0 => cut = i + 1,
            _ => {}
        }
        i += 1;
    }
    selector[cut..].trim()
}

/// 拆掉一个复合选择器里的 `:not(…)` 守卫，返回（剩余文本, 是否存在排除「带 class 元素」的守卫）。
fn strip_not_guards(compound: &str) -> (String, bool) {
    let chars: Vec<char> = compound.chars().collect();
    let mut rest = String::new();
    let mut class_guard = false;
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == ':' && chars[i..].starts_with(&[':', 'n', 'o', 't', '(']) {
            let mut depth = 1i32;
            let mut j = i + 5;
            while j < chars.len() {
                if chars[j] == '(' {
                    depth += 1;
                } else if chars[j] == ')' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                j += 1;
            }
            let arg: String = chars[i + 5..j.min(chars.len())].iter().collect();
            if arg.contains("[class]") {
                class_guard = true;
            }
            rest.push(' ');
            i = j + 1;
        } else {
            rest.push(chars[i]);
            i += 1;
        }
    }
    (rest, class_guard)
}

/// 这个复合选择器是不是「裸 `label`」（去掉 `:not(…)` 守卫后只剩元素名 `label`），
/// 以及它是否带一个排除带类元素的守卫。
fn is_bare_label(compound: &str) -> (bool, bool) {
    let (rest, guard) = strip_not_guards(compound);
    (rest.trim() == "label", guard)
}

/// CSS 标识符字节（`.` 之后是这些字节就说明类名没在此结束）。
fn is_css_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b >= 0x80
}

/// 这个选择器是否**真正命中**类 `class` —— `.check` 不得被 `.check-line` 里的前缀蒙混过关
/// （#156 家族：计数/命中一律要标识符边界）。
fn selector_targets_class(selector: &str, class: &str) -> bool {
    let needle = format!(".{class}");
    let bytes = selector.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = selector[from..].find(&needle) {
        let at = from + rel;
        let after = at + needle.len();
        if after >= selector.len() || !is_css_ident_byte(bytes[after]) {
            return true;
        }
        from = after;
    }
    false
}

/// 声明块里是否声明了属性 `prop`（要求前一个非空白字符是 `;` 或块首、后面紧跟 `:`，
/// 以免 `flex-direction` 里的 `direction` 之类别名串进来）。
fn body_declares(body: &str, prop: &str) -> bool {
    let mut from = 0usize;
    while let Some(rel) = body[from..].find(prop) {
        let at = from + rel;
        let before_ok = body[..at]
            .chars()
            .rev()
            .find(|c| !c.is_whitespace())
            .is_none_or(|c| c == ';' || c == '{');
        let after_ok = body[at + prop.len()..].trim_start().starts_with(':');
        if before_ok && after_ok {
            return true;
        }
        from = at + prop.len();
    }
    false
}

/// 从 markup 里取属性值（只认前面是空白或标签开头的那个属性名）。
fn attr_value(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=");
    let mut from = 0usize;
    while let Some(rel) = tag[from..].find(&needle) {
        let at = from + rel;
        if at == 0 || tag[..at].ends_with(|c: char| c.is_whitespace()) {
            let rest = &tag[at + needle.len()..];
            let mut chars = rest.chars();
            if let Some(q @ ('"' | '\'')) = chars.next() {
                return Some(chars.take_while(|&c| c != q).collect());
            }
        }
        from = at + needle.len();
    }
    None
}

/// 名册：`ui/index.html` 里每个用在 `<label>` 上的类（**派生**，不是快照）。
fn label_classes(html: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let bytes = html.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = html[from..].find("<label") {
        let at = from + rel;
        let is_tag = matches!(
            bytes.get(at + 6),
            None | Some(b'>') | Some(b'/') | Some(b' ') | Some(b'\n') | Some(b'\t') | Some(b'\r')
        );
        if is_tag {
            if let Some(end) = html[at..].find('>') {
                if let Some(value) = attr_value(&html[at..at + end], "class") {
                    for token in value.split_whitespace() {
                        out.insert(token.to_string());
                    }
                }
            }
        }
        from = at + 6;
    }
    out
}

// ------------------------------------------------------------------ 判决 ---

#[derive(Debug, Default, PartialEq, Eq)]
struct Report {
    /// R1 违规：用在 `<label>` 上、但样式表没为它声明 `display` 的类。
    classes_without_display: Vec<String>,
    /// R2 违规：按元素名命中 label、强加排布、又没排除带类 label 的规则。
    unguarded_field_label_rules: Vec<String>,
}

impl Report {
    fn ok(&self) -> bool {
        self.classes_without_display.is_empty() && self.unguarded_field_label_rules.is_empty()
    }
}

/// 纯函数判决：同一份 (markup, 样式表) 输入 ⇒ 同一份报告（变异体靠它在同一棵树上走）。
fn judge(html: &str, css: &str) -> Report {
    let rules = css_rules(css);
    let mut report = Report::default();

    for class in label_classes(html) {
        let declared = rules.iter().any(|rule| {
            body_declares(&rule.body, "display")
                && split_selector_list(&rule.selector)
                    .iter()
                    .any(|part| selector_targets_class(part, &class))
        });
        if !declared {
            report.classes_without_display.push(class);
        }
    }

    for rule in &rules {
        if !body_declares(&rule.body, "display") && !body_declares(&rule.body, "flex-direction") {
            continue;
        }
        for part in split_selector_list(&rule.selector) {
            let (bare_label, guarded) = is_bare_label(rightmost_compound(&part));
            if bare_label && !guarded {
                report
                    .unguarded_field_label_rules
                    .push(format!("{part} {{ {} }}", rule.body.trim()));
            }
        }
    }

    report
}

/// R2 的阳性对照：样式表里**确实存在**带守卫的字段标签规则（否则「零违规」是空集上的假绿）。
fn guarded_field_label_rules(css: &str) -> Vec<String> {
    css_rules(css)
        .iter()
        .filter(|rule| {
            split_selector_list(&rule.selector).iter().any(|part| {
                let (bare_label, guarded) = is_bare_label(rightmost_compound(part));
                bare_label && guarded
            })
        })
        .map(|rule| rule.selector.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 轴：活树同时满足两条规则。
    #[test]
    fn the_field_label_rule_does_not_impose_a_layout_on_classed_labels() {
        let report = judge(INDEX_HTML, STYLE_CSS);
        assert!(
            report.classes_without_display.is_empty(),
            "R1：这些类被用在 <label> 上，样式表却没为它们声明 display ⇒ label 的盒子只能靠泛化规则白拿：{:?}",
            report.classes_without_display
        );
        assert!(
            report.unguarded_field_label_rules.is_empty(),
            "R2：这些规则按元素名命中 label、强加 display/flex-direction，却没有排除带类的 label：{:?}",
            report.unguarded_field_label_rules
        );
    }

    /// 阳性对照：名册与「带守卫的字段标签规则」都必须**真的看到东西**。
    #[test]
    fn the_scanners_see_something() {
        let roster = label_classes(INDEX_HTML);
        assert!(
            roster.len() >= 4,
            "名册只派生出 {roster:?} —— 扫描器坏了，R1 会在空集上假绿"
        );
        for expected in ["check-line", "chip", "f", "checkbox"] {
            assert!(
                roster.contains(expected),
                "名册里少了 `{expected}`（本来就在 index.html 的 <label> 上）：{roster:?}"
            );
        }
        let guarded = guarded_field_label_rules(STYLE_CSS);
        assert!(
            guarded.len() >= 2,
            "样式表里找不到带 `:not([class])` 守卫的字段标签规则（只找到 {guarded:?}）—— R2 会在空集上假绿"
        );
    }

    /// 名册**派生自 markup**，不是快照：换一棵树就换一份名册；`<labelx>` / 别的元素不算。
    #[test]
    fn the_roster_is_derived_from_the_markup() {
        let synthetic =
            r#"<label class="alpha beta">x</label><label>无类</label><span class="gamma">z</span>"#;
        let got = label_classes(synthetic);
        let want: BTreeSet<String> = ["alpha", "beta"].iter().map(|s| s.to_string()).collect();
        assert_eq!(got, want, "名册必须只数 <label> 上的类");

        assert!(
            label_classes(r#"<labelx class="nope"></labelx><div class="alsonot"></div>"#)
                .is_empty(),
            "`<labelx>` 不是 label 起始标签，`<div>` 上的类更不是"
        );
    }

    /// 扫描器不把注释/字符串读成规则（样式表里到处是逐字写着选择器的解释性注释）。
    #[test]
    fn the_scanners_do_not_read_comments_or_strings() {
        let css = concat!(
            "/* .form label { display: flex; flex-direction: column; } */\n",
            ".form label:not([class]) { display: flex; flex-direction: column; }\n",
            ".note::before { content: \".form label { display:flex; flex-direction: column }\"; }\n",
        );
        let rules = css_rules(css);
        assert_eq!(rules.len(), 2, "注释与字符串不该被读成规则：{rules:?}");
        assert!(
            judge("", css).unguarded_field_label_rules.is_empty(),
            "带守卫的那条不该算违规"
        );
        // 牙齿：同一段语料把守卫摘掉 ⇒ 恰好那条翻红。
        let unguarded = css.replace(".form label:not([class])", ".form label");
        assert_eq!(
            judge("", &unguarded).unguarded_field_label_rules.len(),
            1,
            "摘掉守卫后应当恰好一条违规"
        );
    }

    /// 每个变异体只翻它针对的那一条规则（在同一棵活树上改一个 token）。
    #[test]
    fn each_variant_flips_only_its_own_rule() {
        assert!(judge(INDEX_HTML, STYLE_CSS).ok(), "基线必须全绿");

        // m1 / m2 —— R2：把两条字段标签规则的守卫摘掉（复原成缺陷时的写法）。
        for target in [".form label:not([class])", ".login-form label:not([class])"] {
            let mutated = STYLE_CSS.replace(target, &target.replace(":not([class])", ""));
            assert_ne!(mutated, STYLE_CSS, "锚点 `{target}` 在样式表里找不到");
            let report = judge(INDEX_HTML, &mutated);
            assert!(
                report.classes_without_display.is_empty(),
                "摘守卫不该碰到 R1：{:?}",
                report.classes_without_display
            );
            assert_eq!(
                report.unguarded_field_label_rules.len(),
                1,
                "摘掉 `{target}` 的守卫后应当恰好一条 R2 违规：{:?}",
                report.unguarded_field_label_rules
            );
        }

        // m3 —— R1：拿掉 `.checkbox` 那条规则里的 `display`（它会白拿 `.form label` 的盒子）。
        let mutated = STYLE_CSS.replace(
            ".checkbox, .checkbox-line { display: flex; flex-direction: row !important;",
            ".checkbox, .checkbox-line { flex-direction: row !important;",
        );
        assert_ne!(
            mutated, STYLE_CSS,
            "锚点（.checkbox 的 display）在样式表里找不到"
        );
        let report = judge(INDEX_HTML, &mutated);
        assert_eq!(report.classes_without_display, vec!["checkbox".to_string()]);
        assert!(report.unguarded_field_label_rules.is_empty());

        // m4 —— R1：markup 里给 label 加一个新类，而样式表没有它的规则。
        let mutated = INDEX_HTML.replace(
            "<label class=\"check-line\">",
            "<label class=\"check-line brand-new\">",
        );
        assert_ne!(
            mutated, INDEX_HTML,
            "锚点（设置页的 check-line label）在 markup 里找不到"
        );
        let report = judge(&mutated, STYLE_CSS);
        assert_eq!(
            report.classes_without_display,
            vec!["brand-new".to_string()]
        );
        assert!(report.unguarded_field_label_rules.is_empty());
    }

    /// 逐腿读数（门禁自己把看到的东西印出来，别让主张只能靠断言名来读）。
    #[test]
    fn leg_readings() {
        let report = judge(INDEX_HTML, STYLE_CSS);
        println!(
            "R1 类名册（派生自 ui/index.html） = {:?}",
            label_classes(INDEX_HTML)
        );
        println!("R1 违规 = {:?}", report.classes_without_display);
        println!(
            "R2 带守卫的字段标签规则 = {:?}",
            guarded_field_label_rules(STYLE_CSS)
        );
        println!("R2 违规 = {:?}", report.unguarded_field_label_rules);
        println!("样式表规则总数 = {}", css_rules(STYLE_CSS).len());
    }
}
