//! 外观门禁（R128 + R129）：**元素的外观由它自己的类声明** —— 泛化的「按元素名」规则
//! 只服务**没有类**的元素。
//!
//! # 起因（R128）：`.form label` 把盒子强加给带类的 label
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
//! # 起因（R129）：`.card h3` 把外观强加给带类的 h3 —— 那个类**一条声明都没生效**
//!
//! 同一个形状，但受害者不是「被排错行」而是**完全无效**：`.card h3`
//! `{ font-size: 15px; margin-bottom: 14px; color: var(--text) }`（**0,1,1**）压过
//! `.wallet-hero-label` `{ font-size: 12px; color: var(--text-mute); margin-bottom: 0 }`
//! （**0,1,0**）—— 三条**全部**被覆盖，于是钱包页那个「点数余额」标签从来就是普通卡片标题的
//! 样子（实测 computed：`15px / var(--text) / 14px`，而原型第 588 行写着
//! `<h3 class="muted" style="font-size:12px">`）。类里的 `margin-bottom:0`（让大数字贴住标签）
//! 也因此从没用上。**「整个类一条都没生效」＝这个类在 markup 里的存在是一句没有兑现的声明。**
//!
//! # 守的是什么
//!
//! 不是那些具体类，而是它们共同的形状：
//!
//! * **R1（名册，从 `ui/index.html` 派生）** —— 每个用在 `<label>` 上的类，样式表必须有一条
//!   选择器含该 `.类` 的规则**声明了 `display`**：label 的盒子由它的类给全，不许白拿泛化规则的。
//! * **R2（机制）** —— 任何规则，若其选择器的**最右复合选择器**是「裸 `label`」（只有元素名与
//!   `:not(…)` 守卫，没有类 / 属性 / id 限定）且声明了 `display` 或 `flex-direction`，
//!   就必须带一个能排除「带 class 的元素」的 `:not(…)` 守卫（`:not([class])`）。
//! * **R3（死类，名册同样从 `ui/index.html` 派生）** —— 任何被 markup 用上的类，它声明的**每
//!   一条**属性都不得被一条「不带这个元素自己的类、按元素名命中它」的规则赢掉：那样的类一条
//!   声明都留不下，是**死类**（在 markup 里写着、在屏幕上不存在的类）。
//!
//! R1/R2 说的是「谁有权给 label 定盒子」，R3 说的是「这个类到底有没有留下东西」—— 同一条
//! 不变量的两面。R3 之所以**必须比 R2 更宽**（不限 `label`、不限 `display`），是因为 R129 的
//! 受害者是 `h3` 上的字号/颜色/下边距：只盯排布会整片漏掉它。
//!
//! 竞争修法为什么不够：在 `.check-line` 上补 `!important`、或给每个受害类补一条
//! `.form label.X`，都只堵**当下这两个洞**；下一个组件类用在 `<label>` 上时同样的静默竖排会再来。
//! R1 会在那个新类上先红；R3 会在下一个「整个类都没生效」的类上先红。
//!
//! # 期望值全部派生（⛔ 不写快照）
//!
//! 类名册来自 `ui/index.html`、规则来自 `ui/css/style.css`，两者都在编译期读入
//! （仅测试期编译、零新依赖 —— 与 `i18n_pack` / `catalog_gate` / `deploy_gate` / `smtp_port_gate` 同型）。
//!
//! # 射程（如实）
//!
//! 本门禁是**词法**的：它证「谁有权定外观 / 这个类留下了什么」，**不**证屏幕上的样子 ——
//! 那一半归仓外的 jsdom 探针（逐形状的 computed style 对照表）与 headless Chrome 的前后截图。
//! 它**不**实现完整的 CSS 层叠：
//!
//! * R2 并不比较权重，只要求那条泛化规则**命中不到**带类的 label；`:not([class])` 之外等价的
//!   写法（如 `label[class=""]`）本门禁不识别，会报成违规 —— 刻意的窄口径。
//! * R3 只做**词法**近似，比 R2 多两件事、也多两处代价：
//!   * 它**看祖先**（markup 按标签配对入栈，只有真的包住这个元素的选择器才算命中）——
//!     少了这一步，一条 `.page-head p` 会把页面上任何 `<p class="hint">` 都算成被压掉
//!     （实测 7 个假死类）。但它是**词法**配对：`+` / `~`（兄弟）、伪类、属性选择器一律当
//!     「判不了 ⇒ 不算命中」，可能因此**漏报**；`*` 与 `#id` 同理。
//!   * 它比较**权重与书写顺序**（同权重看谁写在后面），但仍不是完整层叠：`!important`、
//!     行内样式、`revert` 等它都不建模。
//!
//!   判据取**必要条件**（这个类的每一条声明都被压掉才算死）⇒ 宁可漏报，也不制造假红。
//!   它**跳过 `@media` 里的规则**（只在某个断点生效的规则不足以让一个类「死掉」），
//!   也**不**把 `[hidden]` 这类**状态**规则算作覆盖 —— 元素本来就不显示，不是组件样式之争。

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

/// 一条 CSS 规则：选择器列表原文 + 声明块原文（注释已被掩码）+ 是否嵌在 at-rule 里。
///
/// `in_media` 只有 R3 用：一个只在某断点生效的规则不足以让一个类「死掉」，把它算作
/// 完整覆盖会过度声称（见模块文档的「射程」）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Rule {
    selector: String,
    body: String,
    in_media: bool,
}

/// 取出所有「**选择器 { 声明 }**」规则；at-rule 容器（`@media … { … }`）自身不算规则，
/// 它里面的规则照常取出（靠「声明块里不再有 `{`」区分两者），并标上 `in_media`。
fn css_rules(css: &str) -> Vec<Rule> {
    let masked = mask_css(css);
    let bytes = masked.as_bytes();
    let mut rules = Vec::new();
    // (开括号字节位, 选择器起点, 是不是 at-rule 容器)
    let mut stack: Vec<(usize, usize, bool)> = Vec::new();
    let mut cursor = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                let is_at = masked[cursor..i].trim().starts_with('@');
                stack.push((i, cursor, is_at));
                cursor = i + 1;
            }
            b'}' => {
                if let Some((open, start, _)) = stack.pop() {
                    let selector = masked[start..open].trim();
                    let body = &masked[open + 1..i];
                    if !selector.is_empty() && !selector.starts_with('@') && !body.contains('{') {
                        let in_media = stack.iter().any(|(_, _, is_at)| *is_at);
                        rules.push(Rule {
                            selector: selector.to_string(),
                            body: body.to_string(),
                            in_media,
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

/// markup 里的一个元素：标签名、类表、以及**祖先链**（由外到内，每个祖先也带标签名与类表）。
///
/// R3 靠祖先链把「按元素类型」的规则限在真正包住这个元素的那几条上 —— 不验证祖先，一条
/// `.page-head p` 会把任何 `<p class="hint">` 都算成被压掉（实测 7 个假死类）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Elem {
    tag: String,
    classes: Vec<String>,
    ancestors: Vec<(String, Vec<String>)>,
}

/// HTML 空元素（不会嵌套，不入栈）。
const VOID_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// markup 里的全部元素（**派生**，不是快照）—— 只做词法配对，不求完整 HTML 语义。
fn elements(html: &str) -> Vec<Elem> {
    let mut out = Vec::new();
    let mut stack: Vec<(String, Vec<String>)> = Vec::new(); // 未闭合的祖先
    let mut from = 0usize;
    while let Some(rel) = html[from..].find('<') {
        let at = from + rel;
        if html[at..].starts_with("<!--") {
            let Some(end) = html[at + 4..].find("-->") else {
                break;
            };
            from = at + 4 + end + 3;
            continue;
        }
        if html[at..].starts_with("</") {
            let Some(end) = html[at..].find('>') else {
                break;
            };
            let name = html[at + 2..at + end].trim().to_ascii_lowercase();
            if let Some(pos) = stack.iter().rposition(|(t, _)| *t == name) {
                stack.truncate(pos);
            }
            from = at + end + 1;
            continue;
        }
        if matches!(html.as_bytes().get(at + 1), None | Some(b'!') | Some(b'?')) {
            from = at + 1;
            continue;
        }
        let mut j = at + 1;
        while j < html.len() && (is_ident_byte(html.as_bytes()[j]) || html.as_bytes()[j] == b'-') {
            j += 1;
        }
        if j == at + 1 {
            from = at + 1;
            continue;
        }
        let tag = html[at + 1..j].to_ascii_lowercase();
        let Some(end) = html[j..].find('>') else {
            break;
        };
        let tag_text = &html[at..j + end];
        let classes: Vec<String> = attr_value(tag_text, "class")
            .map(|v| v.split_whitespace().map(|t| t.to_string()).collect())
            .unwrap_or_default();
        out.push(Elem {
            tag: tag.clone(),
            classes: classes.clone(),
            ancestors: stack.clone(),
        });
        let self_closing = tag_text.ends_with('/');
        if !self_closing && !VOID_TAGS.contains(&tag.as_str()) {
            stack.push((tag, classes));
        }
        from = j + end + 1;
    }
    out
}

/// 名册：`ui/index.html` 里每个「`<元素 类=…>`」配对（**派生**，不是快照）。
///
/// R3 用它：一个类在 markup 里被用上，就必须在那个元素上留下点东西 —— 否则那句 `class=`
/// 是一句没有兑现的声明。
fn class_sites(html: &str) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    for el in elements(html) {
        for class in el.classes {
            out.insert((el.tag.clone(), class));
        }
    }
    out
}

/// 声明块里声明的**属性名**（按书写顺序去重）。语料里没有把 `;` 写进值的声明，
/// 所以按 `;` 切分即可。
fn declared_props(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for decl in body.split(';') {
        if let Some(k) = decl.find(':') {
            let prop = decl[..k].trim();
            if !prop.is_empty() && !out.iter().any(|p| p == prop) {
                out.push(prop.to_string());
            }
        }
    }
    out
}

/// 一个复合选择器（`.card h3:not([class])` 里的 `h3:not([class])`）的词法拆解。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Compound {
    /// 与前一个复合选择器之间的组合符（第一个恒为 `' '`）。
    combinator: char,
    /// 前导元素名（`.wallet-hero-label` 这种纯类选择器没有）。
    element: String,
    /// 类名（不含 `.`）。
    classes: Vec<String>,
    /// 带排除「带类元素」的 `:not([class])` 守卫。
    class_guard: bool,
    /// 还有词法判不了的成分（伪类 / 属性 / `#id` / `*`）⇒ 一律当「命中不了」。
    opaque: bool,
}

impl Compound {
    fn empty() -> Self {
        Compound {
            combinator: ' ',
            element: String::new(),
            classes: Vec::new(),
            class_guard: false,
            opaque: false,
        }
    }
}

/// 把一个选择器部分拆成复合选择器链（组合符 `>` / `+` / `~` / 空白）。
fn split_compounds(part: &str) -> Vec<Compound> {
    let chars: Vec<char> = part.chars().collect();
    let mut out: Vec<Compound> = Vec::new();
    let mut cur = Compound::empty();
    let mut pending = ' ';
    let mut started = false;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '>' | '+' | '~' => {
                if started {
                    out.push(std::mem::replace(&mut cur, Compound::empty()));
                    started = false;
                }
                pending = c;
                i += 1;
            }
            c if c.is_whitespace() => {
                if started {
                    out.push(std::mem::replace(&mut cur, Compound::empty()));
                    started = false;
                }
                i += 1;
            }
            '.' => {
                if !started {
                    cur.combinator = pending;
                    pending = ' ';
                    started = true;
                }
                i += 1;
                let start = i;
                while i < chars.len() && (is_ident_byte(chars[i] as u8) || chars[i] == '-') {
                    i += 1;
                }
                cur.classes.push(chars[start..i].iter().collect());
            }
            ':' => {
                if !started {
                    cur.combinator = pending;
                    pending = ' ';
                    started = true;
                }
                if chars.get(i + 1) == Some(&':') {
                    cur.opaque = true; // 伪元素：词法判不了
                    i += 2;
                } else if chars[i..].starts_with(&[':', 'n', 'o', 't', '(']) {
                    let mut depth = 1i32;
                    let mut j = i + 5;
                    while j < chars.len() {
                        match chars[j] {
                            '(' => depth += 1,
                            ')' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        j += 1;
                    }
                    let arg: String = chars[i + 5..j.min(chars.len())].iter().collect();
                    if arg.contains("[class]") {
                        cur.class_guard = true;
                    } else {
                        cur.opaque = true;
                    }
                    i = j + 1;
                } else {
                    cur.opaque = true;
                    i += 1;
                }
            }
            '[' => {
                if !started {
                    cur.combinator = pending;
                    pending = ' ';
                    started = true;
                }
                cur.opaque = true;
                while i < chars.len() && chars[i] != ']' {
                    i += 1;
                }
                i += 1;
            }
            '#' | '*' => {
                if !started {
                    cur.combinator = pending;
                    pending = ' ';
                    started = true;
                }
                cur.opaque = true;
                i += 1;
                while i < chars.len() && is_ident_byte(chars[i] as u8) {
                    i += 1;
                }
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                if !started {
                    cur.combinator = pending;
                    pending = ' ';
                    started = true;
                }
                let start = i;
                while i < chars.len() && (is_ident_byte(chars[i] as u8) || chars[i] == '-') {
                    i += 1;
                }
                cur.element = chars[start..i].iter().collect();
            }
            _ => {
                if !started {
                    cur.combinator = pending;
                    pending = ' ';
                    started = true;
                }
                cur.opaque = true;
                i += 1;
            }
        }
    }
    if started {
        out.push(cur);
    }
    out
}

/// 一个复合选择器能否套在某个「祖先 / 元素」上（标签名与类都要对上）。
fn compound_fits(compound: &Compound, node: (&str, &[String])) -> bool {
    if compound.opaque || compound.class_guard {
        return false;
    }
    if !compound.element.is_empty() && !compound.element.eq_ignore_ascii_case(node.0) {
        return false;
    }
    compound
        .classes
        .iter()
        .all(|c| node.1.iter().any(|x| x == c))
}

/// 这条选择器（复合选择器链）能否命中这个元素 —— **带祖先匹配**。
///
/// R3 需要它：不验证祖先的话，一条 `.page-head p` 会把页面上任何 `<p class="hint">` 都算成
/// 「被按元素名压掉」（实测 7 个假死类）。`+` / `~`（兄弟）词法判不了 ⇒ 一律当命中不了。
fn matches_element(compounds: &[Compound], el: &Elem) -> bool {
    let Some(last) = compounds.last() else {
        return false;
    };
    if !compound_fits(last, (el.tag.as_str(), el.classes.as_slice())) {
        return false;
    }
    let chain: Vec<(&str, &[String])> = el
        .ancestors
        .iter()
        .rev()
        .map(|(t, c)| (t.as_str(), c.as_slice()))
        .collect();
    let mut next = 0usize; // 下一个可用的祖先下标（由内向外）
    for i in (0..compounds.len() - 1).rev() {
        let rel = compounds[i + 1].combinator;
        if rel == '+' || rel == '~' {
            return false;
        }
        if rel == '>' {
            let Some(node) = chain.get(next) else {
                return false;
            };
            if !compound_fits(&compounds[i], *node) {
                return false;
            }
            next += 1;
        } else {
            let mut found = None;
            for (idx, node) in chain.iter().enumerate().skip(next) {
                if compound_fits(&compounds[i], *node) {
                    found = Some(idx);
                    break;
                }
            }
            let Some(idx) = found else {
                return false;
            };
            next = idx + 1;
        }
    }
    true
}

/// 状态规则（`[hidden]` 等）：元素本来就不显示，不是「组件样式之争」，不算覆盖。
fn is_state_rule(part: &str) -> bool {
    ["[hidden]", "[disabled]", "[open]", "[checked]", "[aria-"]
        .iter()
        .any(|s| part.contains(s))
}

/// 去掉 `:not(…)` 的实参（按 Selectors 4，它自身不计权重；与参考实现同款）。
fn strip_not_args(part: &str) -> String {
    let chars: Vec<char> = part.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == ':' && chars[i..].starts_with(&[':', 'n', 'o', 't', '(']) {
            let mut depth = 1i32;
            let mut j = i + 5;
            while j < chars.len() {
                match chars[j] {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            i = j + 1;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b >= 0x80
}

/// 复合选择器的权重 `(id/属性/伪类, 类, 元素)`。
fn specificity(part: &str) -> (u32, u32, u32) {
    let s = strip_not_args(part);
    let bytes = s.as_bytes();
    let (mut a, mut b, mut c) = (0u32, 0u32, 0u32);
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'#' => {
                a += 1;
                i += 1;
                while i < bytes.len() && is_ident_byte(bytes[i]) {
                    i += 1;
                }
            }
            b'[' => {
                a += 1;
                while i < bytes.len() && bytes[i] != b']' {
                    i += 1;
                }
                i += 1;
            }
            b'.' => {
                b += 1;
                i += 1;
                while i < bytes.len() && is_ident_byte(bytes[i]) {
                    i += 1;
                }
            }
            b':' => {
                if bytes.get(i + 1) == Some(&b':') {
                    c += 1;
                    i += 2;
                } else {
                    b += 1;
                    i += 1;
                }
                while i < bytes.len() && is_ident_byte(bytes[i]) {
                    i += 1;
                }
                // 带实参的伪类（`:nth-child(2)`）—— 实参里的标识符不算元素名。
                if bytes.get(i) == Some(&b'(') {
                    let mut depth = 0i32;
                    while i < bytes.len() {
                        match bytes[i] {
                            b'(' => depth += 1,
                            b')' => {
                                depth -= 1;
                                if depth == 0 {
                                    i += 1;
                                    break;
                                }
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                }
            }
            x if x.is_ascii_alphabetic() || x == b'_' => {
                c += 1;
                while i < bytes.len() && is_ident_byte(bytes[i]) {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    (a, b, c)
}

// ------------------------------------------------------------------ 判决 ---

#[derive(Debug, Default, PartialEq, Eq)]
struct Report {
    /// R1 违规：用在 `<label>` 上、但样式表没为它声明 `display` 的类。
    classes_without_display: Vec<String>,
    /// R2 违规：按元素名命中 label、强加排布、又没排除带类 label 的规则。
    unguarded_field_label_rules: Vec<String>,
    /// R3 违规：`<元素 类=…>` 里这个类的**每一条**声明都被「按元素名命中该元素类型、
    /// 权重不低于它」的规则覆盖掉 —— 死类，一句没有兑现的声明。
    dead_class_sites: Vec<String>,
}

impl Report {
    fn ok(&self) -> bool {
        self.classes_without_display.is_empty()
            && self.unguarded_field_label_rules.is_empty()
            && self.dead_class_sites.is_empty()
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

    for (tag, class) in dead_class_sites(html, &rules) {
        report.dead_class_sites.push(format!(".{class} on <{tag}>"));
    }

    report
}

/// R3：找出「整条类都没生效」的 `<元素 类=…>` 配对。
///
/// 判据是**必要条件**：这个类在样式表里**声明了**东西，而那些声明**逐条**都被一个「不带
/// 本元素的类、按元素名命中它」的规则压掉。逐条比对**权重与书写顺序**（同权重看谁写在后面），
/// 且只在规则**真的命中**这个元素（祖先链对得上）时才算 —— 只做词法近似，不实现完整层叠
/// （见模块文档的「射程」）。
fn dead_class_sites(html: &str, rules: &[Rule]) -> Vec<(String, String)> {
    struct Matched {
        part: String,
        body: String,
        spec: (u32, u32, u32),
        order: usize,
        compounds: Vec<Compound>,
    }

    let mut out: Vec<(String, String)> = Vec::new();
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    for el in elements(html) {
        if el.classes.is_empty() {
            continue;
        }
        // 命中这个元素的所有规则部分（`@media` 里的不算：只在某断点生效的规则不足以让一个类「死掉」）。
        let mut matching: Vec<Matched> = Vec::new();
        for (order, rule) in rules.iter().enumerate() {
            if rule.in_media {
                continue;
            }
            for part in split_selector_list(&rule.selector) {
                let compounds = split_compounds(&part);
                if !matches_element(&compounds, &el) {
                    continue;
                }
                matching.push(Matched {
                    spec: specificity(&part),
                    body: rule.body.clone(),
                    part,
                    order,
                    compounds,
                });
            }
        }

        for class in &el.classes {
            let site = (el.tag.clone(), class.clone());
            if seen.contains(&site) {
                continue;
            }
            // 这个类自己声明了什么。
            let declared: BTreeSet<String> = matching
                .iter()
                .filter(|m| {
                    m.compounds
                        .iter()
                        .any(|c| c.classes.iter().any(|x| x == class))
                })
                .flat_map(|m| declared_props(&m.body))
                .collect();
            if declared.is_empty() {
                continue;
            }

            let every_declaration_loses = declared.iter().all(|prop| {
                let mut cands: Vec<&Matched> = matching
                    .iter()
                    .filter(|m| !is_state_rule(&m.part) && body_declares(&m.body, prop))
                    .collect();
                if cands.is_empty() {
                    return false;
                }
                // 赢家：权重最高，同权重看书写顺序（CSS 的层叠）。
                cands.sort_by_key(|m| std::cmp::Reverse((m.spec, m.order)));
                let winner = cands[0];
                // 赢家只要用到本元素自己的类，就说明这个类留下了东西。
                !winner
                    .compounds
                    .iter()
                    .any(|c| c.classes.iter().any(|x| el.classes.contains(x)))
            });

            if every_declaration_loses {
                seen.insert(site.clone());
                out.push(site);
            }
        }
    }
    out
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

    /// 轴：活树同时满足三条规则。
    #[test]
    fn an_element_appearance_comes_from_its_own_class() {
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
        assert!(
            report.dead_class_sites.is_empty(),
            "R3：这些 `<元素 类=…>` 上，类的每一条声明都被「按元素名命中该元素类型」的规则盖掉了 \
             ⇒ 那个类一条都没生效：{:?}",
            report.dead_class_sites
        );
    }

    /// 阳性对照：三条规则的扫描器都必须**真的看到东西**。
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
        // R3：类站名册同样必须真的看到东西（否则「零死类」是空集上的假绿）。
        let sites = class_sites(INDEX_HTML);
        assert!(
            sites.len() >= 20,
            "markup 里只派生出 {} 个 `<元素 类=…>` 配对 —— 扫描器坏了",
            sites.len()
        );
        for expected in [("h3", "wallet-hero-label"), ("label", "check-line")] {
            assert!(
                sites.contains(&(expected.0.to_string(), expected.1.to_string())),
                "类站名册里少了 {expected:?}（本来就在 index.html 里）：{} 个配对",
                sites.len()
            );
        }
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
            assert!(
                report.dead_class_sites.is_empty(),
                "摘 label 的守卫不该碰到 R3：{:?}",
                report.dead_class_sites
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
        assert!(
            report.dead_class_sites.is_empty(),
            "拿掉 display 不该碰到 R3（这个类还留着别的声明）：{:?}",
            report.dead_class_sites
        );

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
        assert!(report.dead_class_sites.is_empty());

        // m5 —— R3：把 `.card h3:not([class])` 的守卫摘掉（复原成 R129 缺陷时的写法）
        // ⇒ 只有 `.wallet-hero-label` 这个类整条失效，另外两条规则一无所知。
        let mutated = STYLE_CSS.replace(".card h3:not([class])", ".card h3");
        assert_ne!(
            mutated, STYLE_CSS,
            "锚点（.card h3 的守卫）在样式表里找不到"
        );
        let report = judge(INDEX_HTML, &mutated);
        assert_eq!(
            report.dead_class_sites,
            vec![".wallet-hero-label on <h3>".to_string()],
            "摘掉 `.card h3` 的守卫后应当恰好一个死类站"
        );
        assert!(report.classes_without_display.is_empty());
        assert!(report.unguarded_field_label_rules.is_empty());
    }

    /// R3 的机制：祖先、守卫、`@media`、状态规则与权重各自都算数（词法近似也要有牙）。
    #[test]
    fn the_dead_class_rule_honours_ancestry_guards_media_and_specificity() {
        let html = r#"<div class="card"><h3 class="tone">a</h3></div>"#;
        // 死类：`.card h3`（0,1,1）逐条压过 `.tone`（0,1,0）的全部声明。
        let dead =
            ".card h3 { color: red; font-size: 15px; }\n.tone { color: blue; font-size: 12px; }\n";
        assert_eq!(
            dead_class_sites(html, &css_rules(dead)),
            vec![("h3".to_string(), "tone".to_string())],
            "`.card h3` 权重更高、又真的包住这个 h3 ⇒ 整条类失效"
        );

        // 祖先：同一个 h3 不在 `.card` 里，那条规则根本命中不到它。
        assert!(
            dead_class_sites(r#"<h3 class="tone">a</h3>"#, &css_rules(dead)).is_empty(),
            "`.card h3` 包不住这个 h3 ⇒ 不算覆盖（祖先匹配是 R3 的一半）"
        );

        // 守卫：`.card h3:not([class])` 命中不到带类的 h3 ⇒ 这个类活着。
        let guarded = ".card h3:not([class]) { color: red; font-size: 15px; }\n                       .tone { color: blue; font-size: 12px; }\n";
        assert!(
            dead_class_sites(html, &css_rules(guarded)).is_empty(),
            "带 `:not([class])` 守卫的规则不该算覆盖"
        );

        // `@media`：只在某断点生效的规则不足以让一个类「死掉」。
        let media = "@media (max-width: 600px) { .card h3 { color: red; font-size: 15px; } }\n                     .tone { color: blue; font-size: 12px; }\n";
        assert!(
            dead_class_sites(html, &css_rules(media)).is_empty(),
            "`@media` 里的规则不该算覆盖"
        );

        // 状态规则：`[hidden]` 是布尔状态，不是组件样式之争。
        let state =
            "[hidden] { color: red; font-size: 15px; }\n.tone { color: blue; font-size: 12px; }\n";
        assert!(
            dead_class_sites(html, &css_rules(state)).is_empty(),
            "状态规则不该算覆盖"
        );

        // 权重：这个类自己的规则更具体 ⇒ 它赢着，不是死类。
        let stronger = ".card h3 { color: red; font-size: 15px; }\n.card .tone { color: blue; font-size: 12px; }\n";
        assert!(
            dead_class_sites(html, &css_rules(stronger)).is_empty(),
            "`.card .tone`（0,2,0）压过 `.card h3`（0,1,1）⇒ 这个类还留着东西"
        );

        // 牙齿：只被压掉一条声明 ≠ 死类。
        let partial = ".card h3 { color: red; }\n.tone { color: blue; font-size: 12px; }\n";
        assert!(
            dead_class_sites(html, &css_rules(partial)).is_empty(),
            "`font-size` 还留着 ⇒ 不是死类（判据是「逐条全被压掉」）"
        );
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
        println!(
            "R3 类站名册（派生自 ui/index.html） = {} 个配对",
            class_sites(INDEX_HTML).len()
        );
        println!("R3 违规 = {:?}", report.dead_class_sites);
        println!("样式表规则总数 = {}", css_rules(STYLE_CSS).len());
    }
}
