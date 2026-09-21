//! 语言包不变量门禁（C2006）
//!
//! `ui/` 是纯静态前端，此前**没有任何自动化测试覆盖**：语言包的中英键集一致性、
//! `data-i18n*` 属性的可解析性、`T("…")` 字面量的可解析性，全都只能靠人工比对。
//! 本模块把这三类不变量固化为 `cargo test` 的一部分，让「漏键 / 拼错键 / 只改了一种语言」
//! 在 CI 上直接变红，而不是上线后由用户或截图发现界面在显示原始键名（如 `admin.emp.col.member`）。
//!
//! 设计约束：
//! - **仅测试期编译**（`#[cfg(test)] mod`，见 `main.rs`），不进生产二进制；
//! - **不引入依赖**：仓库没有 `ui/package.json`，无法借用 JS 工具链，
//!   因此在 Rust 侧手写等价的静态扫描（原因见 `scan_object_keys` 的注释）；
//! - **只做静态扫描，不执行 JS**；
//! - `ZH_KEY_COUNT` 等常量是**阳性对照**而非装饰，理由见 `packs()`。

use std::collections::{BTreeMap, BTreeSet, HashSet};

/// 前端源文件在**编译期**读入：测试不依赖工作目录与文件系统布局。
const I18N_JS: &str = include_str!("../ui/js/i18n.js");
const INDEX_HTML: &str = include_str!("../ui/index.html");
const APP_JS: &str = include_str!("../ui/js/app.js");
/// `ui/js/api.js` —— 请求咽喉，**唯一**既构造错误文案又调用 `mapErr` 的地方。
///
/// ⚠️ 本文件长期**不在**本模块的输入面里（C2028）：它一次逃过三道断言且理由各异 ——
/// 键存在门禁看到两个包都**有**该键；键使用门禁只认 `T("字面量")`，不认 `mapErr("中文")`；
/// 占位符门禁压根不打开它。于是 `mapErr("登录已过期，请重新登录")` 这类中文原文可以在
/// en 模式下直接抛给用户，而 `cargo test` 全程是绿的（实测 8 种真实后端响应形态里 6 种如此）。
/// 三条断言见 `api_client_error_text_is_key_based`。
const API_JS: &str = include_str!("../ui/js/api.js");
/// `ui/js/data.js` —— 游客兜底表（`MARKET` / `PROVIDERS` / `MODELS` / `PLANS`）。
///
/// 它是**消费语料**（`consumer_code`）的一部分：兜底表被 `marketRows()` 等喂给渲染器，
/// 因此它里面的键引用与 `index.html`、`app.js` 里的同样算可达路径。
const DATA_JS: &str = include_str!("../ui/js/data.js");

/// 后端**用户可见错误文案**的所在地（C2129）。
///
/// `api.js` 把后端 `error` 字段整串交给 `I18n.mapErr()`；`mapErr` 只翻译 `ERR_MAP` 里
/// 手写登记的那几条中文，其余原样返回 ⇒ 后端每新增一条没人记得登记的错误文案，
/// en 界面上就多一句中文，而 `cargo test` 全绿。本清单就是这条断言要扫的语料。
///
/// ⚠️ 手写清单正是本仓反复踩过的坑（C2072 键盘导航名册、C2127 Enter 登记名册）——
/// 所以它由 `backend_error_sources_cover_the_routes_directory` 兜住：该测试把
/// `src/routes/` 的实际目录项与本清单比对，新增路由文件而忘了登记会直接变红，
/// 而不是「静默少扫一个文件、门禁照常通过」。
const BACKEND_ERROR_SOURCES: &[(&str, &str)] = &[
    ("src/gateway.rs", include_str!("gateway.rs")),
    ("src/routes/mod.rs", include_str!("routes/mod.rs")),
    ("src/routes/admin.rs", include_str!("routes/admin.rs")),
    (
        "src/routes/admin_models.rs",
        include_str!("routes/admin_models.rs"),
    ),
    ("src/routes/api_keys.rs", include_str!("routes/api_keys.rs")),
    ("src/routes/ops.rs", include_str!("routes/ops.rs")),
    ("src/routes/org.rs", include_str!("routes/org.rs")),
    ("src/routes/raise.rs", include_str!("routes/raise.rs")),
    ("src/routes/sharing.rs", include_str!("routes/sharing.rs")),
    ("src/routes/wallet.rs", include_str!("routes/wallet.rs")),
];

/// `ERR_MAP` 的区段标记（表体，不含 `var ERR_MAP = ` 与结尾的 `];`）。
const ERR_MAP_START: &str = "var ERR_MAP = [";
const ERR_MAP_END: &str = "\n  ];";

/// 阳性对照真值（口径同 `ZH_KEY_COUNT`：**别口算，让门禁报出真值再照抄**）。
///
/// 这两个数把「提取器静默失真」与「后端/词表真的变了」区分开：语料被判空时，
/// 「每条中文都在词表里」会**恒真**——这正是 C2106 坑 245（提取器返回空字典 ⇒
/// 「0 处漂移」的假绿）。
const ERR_MAP_ENTRY_COUNT: usize = 49;
const BACKEND_ERROR_CJK_COUNT: usize = 45;

/// 语言包区段的起止标记。
///
/// ⚠️ 终点必须取**对象字面量自身的收尾** `"\n  };"`，不能取后面的 `window.I18N`：
/// 后者位于字面量之后数千字符处（中间是运行期代码），会让区段越界扫进代码区，
/// 把三元表达式 `current === "en" ? "en" : "zh-CN"` 里的 `"en" :` 误当对象键，
/// 从而凭空多出一个键 `en`（C2005 坑 69）。
const ZH_START: &str = "var ZH = {";
const EN_START: &str = "var EN = {";
const EN_END: &str = "\n  };";

/// 阳性对照真值：**改动语言包/前端文案时应刻意更新这些数字**。
/// 它们的作用是把「提取器静默失真」与「语言包真的变了」区分开（见 `packs()`）。
///
/// ⚠️ 加数时**别口算**：C2013 先把增量估成 +6（漏了 `fmtUptime` 里的重复调用），
/// 门禁把真值报了出来。改动后请以提取器的实际输出校准，再核对是否与 diff 相符：
/// 先让门禁报出真值、再照抄，**不要**先写一个自己算的数。
///
/// ⚠️ 这里**不记录「本次改了多少」**（C2024）：那段叙述只在写下它的那一次提交里正确，
/// 下一次改语言包的人会改这 6 个常量，却几乎不会想起回来改注释 —— 注释就会在一行之隔
/// 自相矛盾。真实发生过：C2015 按当时实得把键数写成 786，C2019 把常量改成 785 而未动注释，
/// 相隔仅两个提交；`cargo test` 全程是绿的，因为**错的只是注释**（常量本身自洽）。
/// **每一轮的增量属于提交历史**（`git log -p src/i18n_pack.rs`），不在文件里维护副本；
/// 这里只留「改的时候怎么做」的规则，它不随版本腐烂。
///
/// ⚠️ 因此本段刻意不含任何「旧值 → 新值」写法：那种写法读起来就是在断言当前值，
/// 而当前值只有下面这 6 个常量是权威。历史请查 `git log -L 32,52:src/i18n_pack.rs`。
///
/// ⚠️ `T_LITERAL_COUNT` 是 `T("…")` **调用点**总数，不是键数，也不是去重后的键数 ——
/// 三个集合各不相同（坑 99）；说「这个数不该变」之前先确认它在数哪个集合。
const ZH_KEY_COUNT: usize = 788;
const EN_KEY_COUNT: usize = 788;
const STATIC_ATTR_COUNT: usize = 332;
const STATIC_ATTR_DISTINCT: usize = 307;
const T_LITERAL_COUNT: usize = 537;
const T_LITERAL_DISTINCT: usize = 428;

/// 切出语言包区段（起点标记 → 终点标记，含起点）。
fn pack_region<'a>(src: &'a str, start_mark: &str, end_mark: &str) -> &'a str {
    let s = src
        .find(start_mark)
        .unwrap_or_else(|| panic!("语言包起点标记 `{start_mark}` 未找到 —— 语言包结构变了？"));
    let rest = &src[s..];
    let e = rest
        .find(end_mark)
        .unwrap_or_else(|| panic!("语言包终点标记 `{end_mark}` 未找到 —— 语言包结构变了？"));
    &rest[..e]
}

/// 扫描「字符串字面量 + 可选空白 + `:`」形态，返回其中被当作对象键的字面量。
///
/// 这里用逐字节扫描而不是正则，有两个硬理由：
/// 1. 仓库**没有正则依赖**（Cargo.toml 里没有 `regex`），不值得为一个测试引入；
/// 2. 即便有，**行首锚定的正则也会漏键** —— `share.day.1`…`.6` 被挤在同一行，
///    按行匹配只能取到第一个，包大小会从 775 静默变成 770。
///    一个「少数」的门禁比没有门禁更危险：它会让人以为查过了。
fn scan_object_keys(region: &str) -> Vec<String> {
    let b = region.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'"' || c == b'\'' {
            let quote = c;
            let start = i + 1;
            let mut j = start;
            while j < b.len() && b[j] != quote {
                if b[j] == b'\\' {
                    j += 1; // 跳过被转义的字符
                }
                j += 1;
            }
            if j >= b.len() {
                break; // 引号未闭合：语料异常，交给上层断言处理
            }
            let mut k = j + 1;
            while k < b.len() && matches!(b[k], b' ' | b'\t' | b'\n' | b'\r') {
                k += 1;
            }
            if k < b.len() && b[k] == b':' {
                // 键名本身是 ASCII，因此 start/j 必在字符边界上
                out.push(region[start..j].to_string());
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

/// 扫描 `data-i18n` / `data-i18n-ph` / `data-i18n-title` / `data-i18n-label` 属性的取值。
fn scan_static_attributes(src: &str) -> Vec<String> {
    let b = src.as_bytes();
    let needle = b"data-i18n";
    let mut out = Vec::new();
    let mut i = 0;
    while i + needle.len() <= b.len() {
        if b[i..].starts_with(needle) {
            // 命中前缀后继续走完可能的后缀（-ph / -title / -label）
            let mut j = i + needle.len();
            while j < b.len() && !matches!(b[j], b'=' | b' ' | b'\t' | b'\n' | b'\r' | b'>') {
                j += 1;
            }
            if j < b.len() && b[j] == b'=' {
                let mut k = j + 1;
                while k < b.len() && matches!(b[k], b' ' | b'\t') {
                    k += 1;
                }
                if k < b.len() && (b[k] == b'"' || b[k] == b'\'') {
                    let quote = b[k];
                    let start = k + 1;
                    let mut m = start;
                    while m < b.len() && b[m] != quote {
                        m += 1;
                    }
                    if m < b.len() {
                        out.push(src[start..m].to_string());
                    }
                    i = m + 1;
                    continue;
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// `data-i18n` 的四种静态属性形态（与 `scan_static_attributes` 的后缀集合一致）。
const I18N_ATTRS: [&str; 4] = [
    "data-i18n",
    "data-i18n-ph",
    "data-i18n-title",
    "data-i18n-label",
];

/// 从 `lt`（`<` 的下标）出发，返回该标签 `>` 的下标。
///
/// **必须跳过引号内的 `>`**：属性值里出现 `>` 是合法 HTML（本仓真实形态：
/// `title="搜索用户名 / 邮箱…"` 之类倒没有，但 `data-i18n` 的**值**由语言包决定，
/// 未来随时可能含尖括号）。一个被 `>` 提前截断的解析器会把后续标记错位、静默漏检。
fn html_tag_end(src: &str, lt: usize) -> Option<usize> {
    let b = src.as_bytes();
    let mut i = lt + 1;
    let mut quote: Option<u8> = None;
    while i < b.len() {
        let c = b[i];
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => {
                if c == b'"' || c == b'\'' {
                    quote = Some(c);
                } else if c == b'>' {
                    return Some(i);
                }
            }
        }
        i += 1;
    }
    None
}

/// 把起始标签体（`<` 与 `>` 之间，不含尖括号）切成 `(属性名, 属性值)`，值已去引号。
///
/// 逐属性切分而不是在整段里搜 `data-i18n=`：后者会命中**别的属性值里**的字符串
/// （如 `title="data-i18n=x"`），把一个装饰性文本当成真属性。
fn tag_attributes(body: &str) -> Vec<(String, String)> {
    fn ws(c: u8) -> bool {
        matches!(c, b' ' | b'\t' | b'\n' | b'\r')
    }
    let b = body.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    // 跳过标签名
    while i < b.len() && !ws(b[i]) && b[i] != b'/' {
        i += 1;
    }
    while i < b.len() {
        while i < b.len() && ws(b[i]) {
            i += 1;
        }
        if i >= b.len() || b[i] == b'/' {
            break;
        }
        let ns = i;
        while i < b.len() && !ws(b[i]) && b[i] != b'=' {
            i += 1;
        }
        let name = body[ns..i].to_string();
        let mut j = i;
        while j < b.len() && ws(b[j]) {
            j += 1;
        }
        if j < b.len() && b[j] == b'=' {
            j += 1;
            while j < b.len() && ws(b[j]) {
                j += 1;
            }
            if j < b.len() && (b[j] == b'"' || b[j] == b'\'') {
                let q = b[j];
                let vs = j + 1;
                let mut e = vs;
                while e < b.len() && b[e] != q {
                    e += 1;
                }
                out.push((name, body[vs..e.min(b.len())].to_string()));
                i = (e + 1).min(b.len());
            } else {
                let vs = j;
                while j < b.len() && !ws(b[j]) {
                    j += 1;
                }
                out.push((name, body[vs..j].to_string()));
                i = j;
            }
        } else {
            out.push((name, String::new()));
            i = j;
        }
    }
    out
}

/// `scan_i18n_nesting` 的结果。带阳性对照字段，好让「0 违规」不被误读为「扫描器瞎了」。
struct I18nNestingScan {
    /// 违规清单（已格式化成可直接断言的文本）
    violations: Vec<String>,
    /// 带**文本** `data-i18n` 属性的元素个数（阳性对照：为 0 ⇒ 扫描器没在看）
    text_carriers: usize,
    /// 解析到的起始标签数（阳性对照：为 0 ⇒ 扫描器没在看）
    start_tags: usize,
    /// EOF 时仍未闭合的元素名（非空 ⇒ 语料把扫描器弄瞎了，拒绝据此判绿）
    leftover: Vec<String>,
}

/// 扫描 `index.html`：`data-i18n*` 属性**绝不能**落在「带文本 `data-i18n` 的祖先元素」内部。
///
/// 为什么（`ui/js/i18n.js::applyStatic`）：
/// ```js
/// els = document.querySelectorAll("[data-i18n]");
/// for (i = 0; i < els.length; i++) { if (key && ZH[key]) els[i].innerHTML = t(key); }
/// ```
/// 祖先那一步把 `innerHTML` **整体换成语包值** ⇒ 后代元素（连同它自己的 `data-i18n` 属性）
/// 被从文档里摘掉；随后循环再对那个**已分离**的节点设值 —— 无异常、无效果。
/// 实测（jsdom）：`<h3 data-i18n="admin.raise.title">加额申请 <span data-i18n="admin.raise.sub">…</span></h3>`
/// 里那个 span 的 `isConnected` 为 `false`，两种语言下都不显示。
///
/// ⚠️ 只有**文本**属性（`data-i18n`）会砸后代：`data-i18n-title` / `-label` / `-ph`
/// 走 `setAttribute`，只写那一个属性，子标记原样保留。因此 `select#tx-range`（带
/// `data-i18n-title`）里的五个 `<option data-i18n="tx.range.*">` 是**合法**形态，
/// 本规则不会误报 —— 这条边界由 `nested_i18n_detector_detects_injected_defects` 钉住。
///
/// ⚠️ 为什么两道既有门禁都看不见这类缺陷：`every_static_i18n_attribute_resolves` 只问
/// 「键在不在两个包里」（在 ⇒ 过），而「按文本找引用」的死键扫描会看到那个键的**字面量
/// 就写在 `index.html` 里**（⇒ 判「有人用」）。于是「一个永远不会被应用的属性」是它们的盲区。
fn scan_i18n_nesting(src: &str) -> I18nNestingScan {
    // HTML 空元素：没有闭合标签，不入栈
    const VOID: [&str; 14] = [
        "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
        "source", "track", "wbr",
    ];
    let b = src.as_bytes();
    // 栈元素：元素名 + 该元素自己的**文本** `data-i18n` 键（没有则 None）
    let mut stack: Vec<(String, Option<String>)> = Vec::new();
    let mut violations: Vec<String> = Vec::new();
    let mut text_carriers = 0usize;
    let mut start_tags = 0usize;
    let mut i = 0usize;
    while let Some(off) = src[i..].find('<') {
        let lt = i + off;
        // 注释：整段跳过（注释里的标记不是结构）—— #296 同族：证据文本必须先剥注释
        if src[lt + 1..].starts_with("!--") {
            match src[lt..].find("-->") {
                Some(p) => {
                    i = lt + p + 3;
                    continue;
                }
                None => break,
            }
        }
        // doctype / 处理指令
        if matches!(b.get(lt + 1), Some(b'!') | Some(b'?')) {
            match src[lt..].find('>') {
                Some(p) => {
                    i = lt + p + 1;
                    continue;
                }
                None => break,
            }
        }
        let gt = match html_tag_end(src, lt) {
            Some(g) => g,
            None => break,
        };
        let body = &src[lt + 1..gt];
        i = gt + 1;
        if let Some(rest) = body.strip_prefix('/') {
            // 闭合标签：弹到最近的同名元素（连同它一起弹出）
            let name = rest.trim().to_ascii_lowercase();
            if let Some(pos) = stack.iter().rposition(|(n, _)| *n == name) {
                stack.truncate(pos);
            }
            continue;
        }
        let self_close = body.trim_end().ends_with('/');
        let name = body
            .split(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        start_tags += 1;
        let attrs = tag_attributes(body);
        let i18n: Vec<(&str, &str)> = attrs
            .iter()
            .filter(|(n, _)| I18N_ATTRS.contains(&n.as_str()))
            .map(|(n, v)| (n.as_str(), v.as_str()))
            .collect();
        if !i18n.is_empty() {
            if let Some((_, Some(ancestor))) = stack.iter().rev().find(|(_, k)| k.is_some()) {
                let line = src[..lt].bytes().filter(|c| *c == b'\n').count() + 1;
                let desc: Vec<String> = i18n.iter().map(|(n, v)| format!("{n}=\"{v}\"")).collect();
                violations.push(format!(
                    "index.html:{line}: {} 落在 data-i18n=\"{ancestor}\" 的元素内部 —— \
                     祖先的 innerHTML 替换会把它连同属性一起摘掉，该属性永远不会生效",
                    desc.join(" ")
                ));
            }
        }
        if VOID.contains(&name.as_str()) || self_close {
            continue;
        }
        match i18n.iter().find(|(n, _)| *n == "data-i18n") {
            Some((_, v)) => {
                text_carriers += 1;
                stack.push((name, Some((*v).to_string())));
            }
            None => stack.push((name, None)),
        }
    }
    I18nNestingScan {
        violations,
        text_carriers,
        start_tags,
        leftover: stack.into_iter().map(|(n, _)| n).collect(),
    }
}

/// 扫描 `T("字面量")` 形态的文案键，返回其中的字面量。
///
/// 前置字符若是标识符字符或 `.` 则跳过，避免命中 `fmt(` / `obj.T(` 之类。
/// 动态拼接（`T("share.day." + n)`）**会被捕获**，随后由调用方按「以 `.` 结尾」排除 ——
/// 这正是本仓今天唯一的特例（见 `every_t_literal_resolves`）。
fn scan_t_literals(src: &str) -> Vec<String> {
    scan_t_literal_sites(src)
        .into_iter()
        .map(|s| s.key)
        .collect()
}

/// `T("字面量")` 的一个调用点位置：键字面量、左括号下标、字面量收尾后的下标。
struct TLitSite {
    key: String,
    /// `T` 之后那个 `(` 的下标
    open_paren: usize,
    /// 键字面量右引号**之后**的下标
    after_quote: usize,
}

/// 扫描 `T("字面量")` 的位置。
///
/// ⚠️ 这是本模块**唯一**的 `T()` 识别规则：`scan_t_literals` 与 `scan_t_call_sites`
/// 都建立在它之上。C2007 曾另写一份「更聪明」的规则去同时取实参，结果调用点数从 520
/// 变成 517 —— 规则一旦分叉，两条门禁统计的就不再是同一批调用点（坑 75）。
/// 因此这里刻意**共享**实现，并让调用点门禁直接以 `T_LITERAL_COUNT` 为阳性对照。
fn scan_t_literal_sites(src: &str) -> Vec<TLitSite> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 2 <= b.len() {
        if b[i] == b'T' && b[i + 1] == b'(' {
            let prev_ok = i == 0
                || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_' || b[i - 1] == b'.');
            if prev_ok {
                let mut k = i + 2;
                while k < b.len() && matches!(b[k], b' ' | b'\t' | b'\n' | b'\r') {
                    k += 1;
                }
                if k < b.len() && (b[k] == b'"' || b[k] == b'\'') {
                    let quote = b[k];
                    let start = k + 1;
                    let mut m = start;
                    while m < b.len() && b[m] != quote {
                        if b[m] == b'\\' {
                            m += 1;
                        }
                        m += 1;
                    }
                    if m < b.len() {
                        out.push(TLitSite {
                            key: src[start..m].to_string(),
                            open_paren: i + 1,
                            after_quote: m + 1,
                        });
                    }
                    i = m + 1;
                    continue;
                }
            }
            i += 1;
        } else {
            i += 1;
        }
    }
    out
}

/// 扫描对象字面量里「字面量键 → 字面量值」的条目。
///
/// 语言包里 `"key": "value"` 是唯一形态（值全部是字符串字面量）。
/// 返回 `None` 形态（非字符串字面量值）会被单独计数，用于阳性对照 ——
/// 若值形态变了，静默少读比报错更危险。
fn scan_object_entries(region: &str) -> Vec<(String, String, bool)> {
    let b = region.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'"' && b[i] != b'\'' {
            i += 1;
            continue;
        }
        let quote = b[i];
        let ks = i + 1;
        let mut j = ks;
        while j < b.len() && b[j] != quote {
            if b[j] == b'\\' {
                j += 1;
            }
            j += 1;
        }
        if j >= b.len() {
            break;
        }
        let key = region[ks..j].to_string();
        let mut k = j + 1;
        while k < b.len() && matches!(b[k], b' ' | b'\t' | b'\n' | b'\r') {
            k += 1;
        }
        if k < b.len() && b[k] == b':' {
            let mut v = k + 1;
            while v < b.len() && matches!(b[v], b' ' | b'\t' | b'\n' | b'\r') {
                v += 1;
            }
            if v < b.len() && (b[v] == b'"' || b[v] == b'\'') {
                let vq = b[v];
                let vs = v + 1;
                let mut e = vs;
                while e < b.len() && b[e] != vq {
                    if b[e] == b'\\' {
                        e += 1;
                    }
                    e += 1;
                }
                if e < b.len() {
                    out.push((key, region[vs..e].to_string(), true));
                    i = e + 1;
                    continue;
                }
            }
            // 键存在但值不是字符串字面量
            out.push((key, String::new(), false));
        }
        i = j + 1;
    }
    out
}

/// `T("key", { … })` 的一个调用点：键字面量 + 该调用点**实际提供的**插值变量名。
struct TCallSite {
    key: String,
    vars: Vec<String>,
}

/// 从语料中每处字符串字面量里取出 `{name}` 形态的占位符（保持出现顺序）。
fn placeholders(s: &str) -> Vec<String> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'{' {
            let start = i + 1;
            let mut j = start;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            if j > start && j < b.len() && b[j] == b'}' {
                out.push(s[start..j].to_string());
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// 扫描 `T("字面量", { … })` 调用点，返回键与实参对象里的变量名。
///
/// 与 `scan_t_literals` 用**同一套**字面量识别规则（`scan_t_literal_sites`），
/// 只是额外从字面量之后扫出实参对象。实参对象里可以嵌套调用
/// （`{ view: T(VIEW_TITLE[id] || id) }` 是真实写法），因此扫描全程跳过字符串内部。
fn scan_t_call_sites(src: &str) -> Vec<TCallSite> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    for site in scan_t_literal_sites(src) {
        let mut vars = Vec::new();
        // 只剩 `)` 立即收尾 ⇒ 无实参对象
        let mut p = site.after_quote;
        while p < b.len() && matches!(b[p], b' ' | b'\t' | b'\n' | b'\r') {
            p += 1;
        }
        if p < b.len() && b[p] == b',' {
            p += 1;
            while p < b.len() && matches!(b[p], b' ' | b'\t' | b'\n' | b'\r') {
                p += 1;
            }
            if p < b.len() && b[p] == b'{' {
                if let Some(e) = match_brace(src, site.open_paren) {
                    vars = obj_var_names(&src[p + 1..e]);
                }
            }
            // 实参不是对象字面量（如 `T(k, someVar)`）：本仓不存在，扫到的变量集为空即可
        }
        out.push(TCallSite {
            key: site.key,
            vars,
        });
    }
    out
}

/// 从 `open_paren`（`(` 的下标）出发，返回与之配对的 `)` 的下标。
/// 全程跳过字符串字面量，避免文案里的括号扰乱配对。
fn match_paren(src: &str, open_paren: usize) -> Option<usize> {
    let b = src.as_bytes();
    let mut depth = 0usize;
    let mut j = open_paren;
    while j < b.len() {
        let c = b[j];
        if c == b'"' || c == b'\'' {
            let q = c;
            j += 1;
            while j < b.len() && b[j] != q {
                if b[j] == b'\\' {
                    j += 1;
                }
                j += 1;
            }
        } else if c == b'(' {
            depth += 1;
        } else if c == b')' {
            depth -= 1;
            if depth == 0 {
                return Some(j);
            }
        }
        j += 1;
    }
    None
}

/// 从 `open_brace`（`{` 的下标）出发，返回与之配对的 `}` 的下标（同样跳过字符串）。
fn match_brace(src: &str, open_paren: usize) -> Option<usize> {
    // 先定位调用收尾，再在其中找花括号，避免越界到下一个语句
    let end = match_paren(src, open_paren)?;
    let region = &src[open_paren..=end];
    let rb = region.as_bytes();
    let bs = region.find('{')?;
    let mut depth = 0usize;
    let mut x = bs;
    while x < rb.len() {
        let c = rb[x];
        if c == b'"' || c == b'\'' {
            let q = c;
            x += 1;
            while x < rb.len() && rb[x] != q {
                if rb[x] == b'\\' {
                    x += 1;
                }
                x += 1;
            }
        } else if c == b'{' {
            depth += 1;
        } else if c == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some(open_paren + x);
            }
        }
        x += 1;
    }
    None
}

/// 取对象字面量**内部**代码里的 `name:` 变量名（跳过字符串与嵌套对象内容）。
fn obj_var_names(body: &str) -> Vec<String> {
    let bb = body.as_bytes();
    let mut vars = Vec::new();
    let mut t = 0;
    while t < bb.len() {
        let c = bb[t];
        if c == b'"' || c == b'\'' {
            let q = c;
            t += 1;
            while t < bb.len() && bb[t] != q {
                if bb[t] == b'\\' {
                    t += 1;
                }
                t += 1;
            }
            t += 1;
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
            let start = t;
            while t < bb.len() && (bb[t].is_ascii_alphanumeric() || bb[t] == b'_' || bb[t] == b'$')
            {
                t += 1;
            }
            let ident = &body[start..t];
            let mut u = t;
            while u < bb.len() && matches!(bb[u], b' ' | b'\t' | b'\n' | b'\r') {
                u += 1;
            }
            // 只有 `ident:` 才算显式变量名（简写 `{ n }` 形态本仓不存在）
            if u < bb.len() && bb[u] == b':' {
                vars.push(ident.to_string());
            }
            continue;
        }
        t += 1;
    }
    vars
}

/// 剥掉 JS 源码里的**注释**，只留代码（供「字面量里不许有中文」这类断言使用）。
///
/// 为什么必须剥：`ui/js/api.js` 的注释本来就是中文，而注释不是用户可见文案。
/// **不能**按「行里含中文就跳过该行」来近似 —— 那样会放过 `const a = "中文"; // note`
/// 这种同行混合，正是要抓的形态之一。
///
/// 处理范围是刻意最小的：`"…"` / `'…'` / `` `…` ``（含转义）**逐字透传**，
/// `//…` 到行尾与 `/* … */` 整段丢弃。其余字节原样保留。
///
/// ⚠️ 必须在**字节**上搬运、最后整体 `String::from_utf8`（C2129）：曾经写成
/// `out.push(b[i] as char)`，于是每个 **UTF-8 字节**被当成一个独立码位 —— 中文字面量会变成
/// 乱码（一个 3 字节汉字 → 3 个 Latin-1 字符，`"中文"` 也再 `contains("中文")` 不成立）。
/// 而所有旧消费者都只问 `is_ascii()`，乱码同样非 ASCII ⇒ **每一道旧门禁照常通过**，
/// 这个失真静默了三轮；直到有消费者拿剥完注释的文本去与**未加工**的源文比 `contains()`
/// （`err_map_pairs` 对 `ERR_MAP`）才暴露：词表 49 条中文全部匹配不上，后端 45 条文案
/// 被误报成「一条都没登记」——一个**假的**红色，照它去改会往词表里塞 45 条永远命不中的条目。
/// 教训：一处提取器若只被 `is_ascii()` 这类**弱谓词**消费，它的失真就没有证人。
fn strip_js_comments(src: &str) -> String {
    let b = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(src.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(b.len());
            continue;
        }
        if c == b'"' || c == b'\'' || c == b'`' {
            let quote = c;
            out.push(c);
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    out.push(b[i]);
                    if i + 1 < b.len() {
                        out.push(b[i + 1]);
                    }
                    i += 2;
                    continue;
                }
                out.push(b[i]);
                if b[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    // 删掉的都是 `//…\n` 与 `/*…*/`，两端都是 ASCII ⇒ 切点必落在字符边界上，
    // 透传的字节序列保持原样，因此这里永远不会失败。
    String::from_utf8(out).expect("strip_js_comments 只搬运字节，输入是 UTF-8 则输出也是")
}

/// 收集既不在中文包、也不在英文包中的键（即会被原样显示给用户的键名）。
fn unresolved<'a>(
    keys: impl IntoIterator<Item = &'a String>,
    zh: &BTreeSet<String>,
    en: &BTreeSet<String>,
) -> Vec<String> {
    keys.into_iter()
        .filter(|k| !zh.contains(*k) || !en.contains(*k))
        .cloned()
        .collect()
}

/// 两个语言包的**键集 + 值表**。
///
/// 键集用于「键是否齐全」类断言；值表用于「文案内容」类断言（如插值占位符，C2007）。
/// 两者必须来自**同一次**扫描，否则会拿 A 版本的键去比对 B 版本的值。
struct LanguagePacks {
    zh_keys: BTreeSet<String>,
    en_keys: BTreeSet<String>,
    zh: BTreeMap<String, String>,
    en: BTreeMap<String, String>,
}

fn packs() -> LanguagePacks {
    let zh_keys = scan_object_keys(pack_region(I18N_JS, ZH_START, EN_START));
    let en_keys = scan_object_keys(pack_region(I18N_JS, EN_START, EN_END));
    assert_eq!(
        zh_keys.len(),
        ZH_KEY_COUNT,
        "中文包键数应为 {ZH_KEY_COUNT}，实得 {} —— 提取器已失真（区段标记或扫描规则需重核），拒绝继续",
        zh_keys.len()
    );
    assert_eq!(
        en_keys.len(),
        EN_KEY_COUNT,
        "英文包键数应为 {EN_KEY_COUNT}，实得 {} —— 提取器已失真，拒绝继续",
        en_keys.len()
    );
    let (zh, zh_non_string) = pack_values(pack_region(I18N_JS, ZH_START, EN_START));
    let (en, en_non_string) = pack_values(pack_region(I18N_JS, EN_START, EN_END));
    // 值的形态也是阳性对照：语言包里每个键都应是字符串字面量值。
    // 若某天出现非字符串值，静默少读会让内容类门禁在残缺语料上「通过」。
    assert_eq!(
        zh_non_string, 0,
        "中文包出现非字符串字面量值 {zh_non_string} 处 —— 值扫描器需重核"
    );
    assert_eq!(
        en_non_string, 0,
        "英文包出现非字符串字面量值 {en_non_string} 处 —— 值扫描器需重核"
    );
    assert_eq!(
        zh.len(),
        ZH_KEY_COUNT,
        "中文包值表条目应为 {ZH_KEY_COUNT}，实得 {} —— 值扫描器已失真",
        zh.len()
    );
    assert_eq!(
        en.len(),
        EN_KEY_COUNT,
        "英文包值表条目应为 {EN_KEY_COUNT}，实得 {} —— 值扫描器已失真",
        en.len()
    );
    LanguagePacks {
        zh_keys: zh_keys.into_iter().collect(),
        en_keys: en_keys.into_iter().collect(),
        zh,
        en,
    }
}

/// 扫出「键 → 文案值」，并单独返回**非字符串字面量值**的处数（供阳性对照使用）。
fn pack_values(region: &str) -> (BTreeMap<String, String>, usize) {
    let mut map = BTreeMap::new();
    let mut non_string = 0usize;
    for (k, v, is_str) in scan_object_entries(region) {
        if is_str {
            map.insert(k, v);
        } else {
            non_string += 1;
        }
    }
    (map, non_string)
}

/// 语言包**不可达键**的日落清单（C2155）。
///
/// 判据：一个语言包键「可达」当且仅当它以**键 token 边界**出现在消费语料
/// （`consumer_code()`）里，或以**动态前缀**（`dynamic_key_prefixes()`，
/// 今日恰好一个 `share.day.`）开头。
///
/// ⚠️ 这是**日落清单**（sunset list），不是豁免注册表 —— 清单即契约，零豁免：
/// - 新增一个没人引用的键 ⇒ 它落进计算出的不可达集合而清单里没有 ⇒ **红**；
/// - 从清单删一条而该键仍不可达 ⇒ **红**（断言是**精确相等**，不是子集）；
/// - 把某条日落键**接上线** ⇒ 它离开不可达集合却仍留在清单里 ⇒ **红**
///   （必须**同时**把该条目从清单删掉）。
///
/// 分类标签来自 C2154 的全量裁定（零活缺陷），逐条证据见
/// `.emrg/memory/latent-unreachable-observations-20260913.md` §39.2：
/// - `dup-sibling`     同包已有一个可达键携带**逐字相同**的值（改名/合并后的重复体，删除零损失）
/// - `zero-mock`       #94（`89963f3`）删掉了 mock 的**渲染**却把**键**留下
/// - `old-design`      被现行设计取代（如成员级「配额」旧表 —— 现行只有**部门**有配额）
/// - `neutral-literal` 值本身就是语言中性字面量（`you@company.com` / `✕` / `中文（简体）`）
/// - `composite`       文本已嵌在一个**可达**的复合键值里
/// - `rename`          改名残留（列头已改用另一个键）
/// - `weak-should-wire` 代码手工拼了更优文案，专键存在但没人用
/// - `host-decision`   宿主裁定族，⛔ 勿自修
const UNREACHABLE_PACK_KEYS: &[(&str, &str)] = &[
    ("admin.emp.col.empty", "old-design"),
    ("admin.emp.col.quota", "old-design"),
    ("admin.emp.col.remain", "old-design"),
    ("admin.emp.col.status", "old-design"),
    ("admin.emp.role.admin", "old-design"),
    ("admin.emp.role.ops", "old-design"),
    ("admin.emp.stats.quota", "old-design"),
    ("admin.emp.stats.quota.sub", "old-design"),
    ("admin.emp.stats.remain", "old-design"),
    ("admin.emp.stats.remain.sub", "old-design"),
    ("admin.emp.stats.used", "old-design"),
    ("admin.emp.stats.used.sub", "old-design"),
    ("admin.org.col.empty", "old-design"),
    ("admin.raise.col.empty", "old-design"),
    ("cnt.items", "old-design"),
    ("common.close", "old-design"),
    ("common.none", "old-design"),
    ("common.ok", "old-design"),
    ("common.save", "old-design"),
    ("common.search", "old-design"),
    ("login.demo", "old-design"),
    ("login.subtitle", "old-design"),
    ("ops.users.col.empty", "old-design"),
    ("settings.ak.col.empty", "old-design"),
    ("settings.ak.gen.ok.mock", "old-design"),
    ("share.col.empty", "old-design"),
    ("tx.brk.cache", "old-design"),
    ("tx.brk.input", "old-design"),
    ("tx.brk.output", "old-design"),
    ("tx.brk.title", "old-design"),
    ("chat.close", "neutral-literal"),
    ("login.email.ph", "neutral-literal"),
    ("settings.prefs.lang.en", "neutral-literal"),
    ("settings.prefs.lang.zh", "neutral-literal"),
    ("admin.emp.dept.ok.unassigned", "weak-should-wire"),
    ("admin.usage.unit.points", "host-decision"),
];

/// `ui/js/i18n.js` 语言包区段**之外**的两段代码：前段（`ERR_MAP` 词表等）与后段（运行期代码）。
fn i18n_non_pack_parts() -> [&'static str; 2] {
    let zh = I18N_JS
        .find(ZH_START)
        .expect("语言包起点标记未见 —— 语言包结构变了？");
    let en = I18N_JS
        .find(EN_START)
        .expect("英文包起点标记未见 —— 语言包结构变了？");
    let end = en
        + I18N_JS[en..]
            .find(EN_END)
            .expect("英文包终点标记未见 —— 语言包结构变了？");
    [&I18N_JS[..zh], &I18N_JS[end..]]
}

/// 剥掉 HTML 注释（`<!-- … -->`）。
///
/// 注释里的 `data-i18n` 标记**不是**消费者：`applyStatic()` 用 `querySelectorAll` 走
/// **DOM**，注释不在 DOM 里，永远不会被替换（#296 同族 —— 证据文本必须先剥注释）。
fn strip_html_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(i) = rest.find("<!--") {
        out.push_str(&rest[..i]);
        out.push('\n'); // 保留一个分隔，避免注释两侧的 token 被粘成一个
        match rest[i..].find("-->") {
            Some(j) => rest = &rest[i + j + 3..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// 消费语料：**谁可以引用一个语言包键**。
///
/// 组成（与 C2154 侦察的「边界尺子」逐字一致，其稳定性见 §39.3）：
/// `app.js` / `api.js` / `data.js` 剥注释后的代码 ＋ `index.html` 剥注释后的标记
/// ＋ `i18n.js` 语言包区段之外的代码。
///
/// ⚠️ 两处都必须**先剥注释**（#296）：注释里出现的键名**不是**消费者。
/// 剥注释**保留字符串字面量**（`strip_js_comments` 逐字透传 `"…"` / `'…'` / `` `…` ``），
/// 因为引用键的正是那些字面量。
fn consumer_code() -> String {
    let [i18n_head, i18n_tail] = i18n_non_pack_parts();
    [
        strip_js_comments(APP_JS),
        strip_js_comments(API_JS),
        strip_js_comments(DATA_JS),
        strip_html_comments(INDEX_HTML),
        strip_js_comments(i18n_head),
        strip_js_comments(i18n_tail),
    ]
    .join("\n")
}

/// 键 token 的组成字符：ASCII 字母数字 ＋ `_` `.` `-`。
///
/// ⚠️ `.` 必须在集合里 —— 否则 `common.ok` 会被 `common.ok.mock` **里面**的子串命中，
/// 让一个孤儿键**假可达**（「子串匹配把兄弟标识符当证据」，坑 #333）。
fn is_key_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')
}

/// `key` 是否以**键 token 边界**出现在 `corpus` 里。
fn key_token_occurs(corpus: &str, key: &str) -> bool {
    if key.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(rel) = corpus[from..].find(key) {
        let i = from + rel;
        let before_ok = match corpus[..i].chars().next_back() {
            Some(c) => !is_key_token_char(c),
            None => true,
        };
        let after_ok = match corpus[i + key.len()..].chars().next() {
            Some(c) => !is_key_token_char(c),
            None => true,
        };
        if before_ok && after_ok {
            return true;
        }
        from = i + 1;
    }
    false
}

/// 动态前缀集合：`T("…" + …)` 形态的字面量前缀（本仓今天**恰好一个**：`share.day.`）。
///
/// 派生自消费语料本身（`scan_t_literals` 取以 `.` 结尾的字面量），**不写名册** ——
/// 手写名册正是本仓反复踩过的坑（C2072 键盘导航、C2127 Enter 登记）。
///
/// ⚠️ 这条规则在**今天的真语料上是冗余的**（A/B 实测，诚实记录）：`share.day.1..7`
/// 同时被 `index.html:305-311` 的周几芯片以 `data-i18n` **静态绑定**，所以即便把
/// `app.js:946-947` 那段拼接删掉，该家族仍然可达、门禁照绿。它的牙齿由
/// `pack_reachability_checker_detects_injected_defects` 用**合成输入**证明
/// （删掉前缀后 `pfx.*` 必须立刻变不可达）—— 这条规则是为**未来**的动态家族准备的：
/// 没有它，`T("<prefix>" + n)` 拼出来的那一族键会被整族误报成孤儿。
fn dynamic_key_prefixes() -> BTreeSet<String> {
    let [head, tail] = i18n_non_pack_parts();
    let mut out = BTreeSet::new();
    for src in [APP_JS, API_JS, DATA_JS, head, tail] {
        for lit in scan_t_literals(&strip_js_comments(src)) {
            if lit.ends_with('.') {
                out.insert(lit);
            }
        }
    }
    out
}

/// 从键集里筛出**不可达**的键（纯函数，便于合成输入自证）。
fn unreachable_keys<'a>(
    keys: impl IntoIterator<Item = &'a String>,
    corpus: &str,
    prefixes: &BTreeSet<String>,
) -> BTreeSet<String> {
    keys.into_iter()
        .filter(|k| {
            !key_token_occurs(corpus, k) && !prefixes.iter().any(|p| k.starts_with(p.as_str()))
        })
        .cloned()
        .collect()
}

/// 真语料上计算出的不可达键集合。
fn computed_unreachable_keys() -> BTreeSet<String> {
    let LanguagePacks { zh_keys, .. } = packs();
    unreachable_keys(zh_keys.iter(), &consumer_code(), &dynamic_key_prefixes())
}

/// 声明的日落清单（去重后的键集）。
fn declared_unreachable_keys() -> BTreeSet<String> {
    UNREACHABLE_PACK_KEYS
        .iter()
        .map(|(k, _)| (*k).to_string())
        .collect()
}

/// 语言包不变量测试。
///
/// 整个模块只在测试期编译（`main.rs` 里是 `#[cfg(test)] mod i18n_pack`），
/// 因此 `include_str!` 嵌进来的前端源码**不会**进入发布二进制。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_pack_keys_are_identical() {
        let LanguagePacks {
            zh_keys: zh,
            en_keys: en,
            ..
        } = packs();
        let only_zh: Vec<&String> = zh.difference(&en).collect();
        let only_en: Vec<&String> = en.difference(&zh).collect();
        assert!(
            only_zh.is_empty() && only_en.is_empty(),
            "中英语言包键集不一致：仅中文有 {only_zh:?}；仅英文有 {only_en:?}"
        );
    }

    #[test]
    fn lang_pack_has_no_duplicate_keys() {
        for (name, region) in [
            ("zh", pack_region(I18N_JS, ZH_START, EN_START)),
            ("en", pack_region(I18N_JS, EN_START, EN_END)),
        ] {
            let keys = scan_object_keys(region);
            assert!(!keys.is_empty(), "{name} 包未扫到任何键 —— 提取器已失真");
            let mut seen = HashSet::new();
            let dups: Vec<String> = keys
                .into_iter()
                .filter(|k| !seen.insert(k.clone()))
                .collect();
            assert!(
                dups.is_empty(),
                "{name} 包存在重复键（后定义者会静默覆盖前者）：{dups:?}"
            );
        }
    }

    #[test]
    fn every_static_i18n_attribute_resolves() {
        let LanguagePacks {
            zh_keys: zh,
            en_keys: en,
            ..
        } = packs();
        let raw = scan_static_attributes(INDEX_HTML);
        let distinct: BTreeSet<String> = raw.iter().cloned().collect();
        assert_eq!(
            raw.len(),
            STATIC_ATTR_COUNT,
            "ui/index.html 的 data-i18n* 属性数应为 {STATIC_ATTR_COUNT}，实得 {} —— 提取器已失真",
            raw.len()
        );
        assert_eq!(
            distinct.len(),
            STATIC_ATTR_DISTINCT,
            "data-i18n* 去重后应为 {STATIC_ATTR_DISTINCT}，实得 {}",
            distinct.len()
        );
        let missing = unresolved(distinct.iter(), &zh, &en);
        assert!(
            missing.is_empty(),
            "以下 data-i18n* 属性在语言包中不存在（页面会原样显示键名）：{missing:?}"
        );
    }

    /// 内容归属门禁（C2150）：`data-i18n*` 属性不得嵌在另一个**带文本 `data-i18n`** 的元素内部。
    ///
    /// 这类属性永远不会生效 —— 祖先的 `innerHTML = t(key)` 会把它连同元素一起从文档里摘掉。
    /// 它同时是**必须**的：`every_static_i18n_attribute_resolves` 与任何「按文本找引用」的
    /// 死键扫描都看不见它（键在两包俱在、字面量也写在 `index.html` 里）。
    #[test]
    fn no_data_i18n_attribute_nests_inside_a_data_i18n_element() {
        let scan = scan_i18n_nesting(INDEX_HTML);
        assert!(
            scan.leftover.is_empty(),
            "index.html 的标签栈在 EOF 未清空（{:?}）—— 扫描器被语料弄瞎了，拒绝据此判绿",
            scan.leftover
        );
        // 阳性对照：扫描器真的在看，且看到了东西
        assert!(scan.start_tags > 0, "未解析到任何起始标签 —— 扫描器已失真");
        assert!(
            scan.text_carriers > 0,
            "未扫到任何带文本 data-i18n 的元素 —— 扫描器已失真，「0 违规」是假的"
        );
        assert!(
            scan.violations.is_empty(),
            "以下 data-i18n* 属性永远不会生效（祖先的 innerHTML 替换会把它们摘掉）：\n  - {}",
            scan.violations.join("\n  - ")
        );
    }

    /// 阴性对照：内容归属检查器必须真的会失败。
    ///
    /// 只断言「当前 0 违规」是不够的 —— 一个恒真的检查等价于没有检查。
    /// 这里拿真实缺陷形态（含修复前的那两处原文）构造语料，断言检查器把它们报出来。
    #[test]
    fn nested_i18n_detector_detects_injected_defects() {
        // ① 缺陷原形：修复前的 `admin.raise.title`（文本祖先 + 后代 `data-i18n` 子元素）
        //    ⚠️ 语料含 `href="#"` ⇒ 必须用 `r##"…"##`：`r#"…"#` 会被 `"#` 提前闭合。
        for bad in [
            r##"<h3 data-i18n="admin.raise.title">加额申请 <span data-i18n="admin.raise.sub">（…）</span></h3>"##,
            r##"<p data-i18n="login.foot">x<a href="#" id="reg-link" data-i18n="login.register">注册</a></p>"##,
        ] {
            let s = scan_i18n_nesting(bad);
            assert_eq!(
                s.violations.len(),
                1,
                "阴性对照失败：嵌套属性未被检出：{bad}"
            );
            assert!(
                s.violations[0].contains("index.html:1:"),
                "违规应带行号：{:?}",
                s.violations[0]
            );
            assert!(s.leftover.is_empty(), "对照语料应为良构：{bad}");
        }

        // ② 正确形态：兄弟 span（修复后的写法）⇒ 不得报出
        let good = r#"<h3><span data-i18n="admin.raise.title">加额申请</span> <span data-i18n="admin.raise.sub">（…）</span></h3>"#;
        let s = scan_i18n_nesting(good);
        assert!(
            s.violations.is_empty(),
            "阳性对照失败：兄弟写法被误报：{:?}",
            s.violations
        );
        assert_eq!(s.text_carriers, 2, "阳性对照：应看到 2 个文本载体");

        // ③ 合法形态：祖先只带**属性型**钩子（`data-i18n-title`），子标记存活。
        //    真实形态＝`select#tx-range` 里的五个 `<option data-i18n="tx.range.*">`。
        let title_parent = r#"<select data-i18n-title="tx.range.title"><option data-i18n="tx.range.24h">24 小时</option><option data-i18n="tx.range.7d">7 天</option></select>"#;
        assert!(
            scan_i18n_nesting(title_parent).violations.is_empty(),
            "阳性对照失败：仅写属性的祖先（data-i18n-title）被误报"
        );
        // 同理，`data-i18n-label` 祖先（真实形态＝`div#help-panel`）
        let label_parent = r#"<div id="help-panel" data-i18n-label="help.title"><strong data-i18n="help.title">快捷键</strong></div>"#;
        assert!(
            scan_i18n_nesting(label_parent).violations.is_empty(),
            "阳性对照失败：data-i18n-label 祖先被误报"
        );

        // ④ 反向：后代带的是**属性型**钩子，同样会被文本祖先摘掉 ⇒ 必须报出
        let nested_attr_kind =
            r##"<p data-i18n="login.foot">x<a href="#" data-i18n-title="login.or">y</a></p>"##;
        assert_eq!(
            scan_i18n_nesting(nested_attr_kind).violations.len(),
            1,
            "阴性对照失败：被摘掉的属性型钩子未被检出"
        );

        // ⑤ 解析器不得被属性值里的 `>` / 引号骗到（否则会静默漏检后面的缺陷）
        let tricky = r#"<p data-i18n="a.b" title="x > y" data-note='a "quoted" b'><span data-i18n="c.d">z</span></p>"#;
        assert_eq!(
            scan_i18n_nesting(tricky).violations.len(),
            1,
            "解析器被属性值里的尖括号/引号骗了"
        );

        // ⑥ 空元素与自闭合标签不得破坏标签栈
        let voids = r#"<div data-i18n="a.b">t<br><img src="x"><input value="y"><span data-i18n="c.d">z</span></div>"#;
        let s = scan_i18n_nesting(voids);
        assert_eq!(s.violations.len(), 1, "空元素破坏了标签栈");
        assert!(s.leftover.is_empty(), "空元素应不入栈：{:?}", s.leftover);

        // ⑦ 注释里的标记不参与结构（#296 同族：证据文本必须先剥注释）
        let commented = r#"<div data-i18n="a.b">t<!-- <span data-i18n="c.d">z</span> --></div>"#;
        let s = scan_i18n_nesting(commented);
        assert!(
            s.violations.is_empty(),
            "注释里的标记被当成了结构：{:?}",
            s.violations
        );

        // ⑧ 违规行号必须是真的行号（不是恒 1）
        let multiline =
            "<div data-i18n=\"a.b\">\n  <p>x</p>\n  <span data-i18n=\"c.d\">z</span>\n</div>";
        let s = scan_i18n_nesting(multiline);
        assert_eq!(s.violations.len(), 1);
        assert!(
            s.violations[0].contains("index.html:3:"),
            "违规行号应为 3，实得 {:?}",
            s.violations[0]
        );
    }

    #[test]
    fn every_t_literal_resolves() {
        let LanguagePacks {
            zh_keys: zh,
            en_keys: en,
            ..
        } = packs();
        let raw = scan_t_literals(APP_JS);
        let distinct: BTreeSet<String> = raw.iter().cloned().collect();
        assert_eq!(
            raw.len(),
            T_LITERAL_COUNT,
            "ui/js/app.js 的 T() 字面量数应为 {T_LITERAL_COUNT}，实得 {} —— 提取器已失真",
            raw.len()
        );
        assert_eq!(
            distinct.len(),
            T_LITERAL_DISTINCT,
            "T() 字面量去重后应为 {T_LITERAL_DISTINCT}，实得 {}",
            distinct.len()
        );
        // 以 `.` 结尾的是**动态拼接前缀**（今天只有 `"share.day."`，拼接出 share.day.1…7），
        // 它本身不是完整键，不能按缺键报错。
        let checked: Vec<&String> = distinct.iter().filter(|k| !k.ends_with('.')).collect();
        let missing = unresolved(checked, &zh, &en);
        assert!(
            missing.is_empty(),
            "以下 T() 字面量在语言包中不存在（界面会原样显示键名）：{missing:?}"
        );
    }

    /// 整包可达性门禁（C2155）：语言包里的每个键都必须有一个**可到达**的消费者。
    ///
    /// 为什么需要：语言包是**双份**的（zh/en），一个没人引用的键在两包里各占一行。
    /// 它不会报错，也不会被上面两条门禁看见 —— `every_static_i18n_attribute_resolves`
    /// 与 `every_t_literal_resolves` 只问「**引用了的**键在不在包里」，方向**相反**。
    /// 而 C2153 证明这类「孤儿键」可以是**活缺陷的指纹**：`share.toggle.relisted`
    /// 两包俱在、无人可达 ⇒ 共享切换的结局少了「重新上架」那一支（按钮写「重新上架」，
    /// toast 写「已恢复」）。C2154 把这 59 个孤儿逐条裁定为残留/弱项/宿主裁定（零活缺陷），
    /// 本门禁把「清单不再增长」变成契约。
    ///
    /// ⚠️ 断言是**精确相等**（不是子集）：清单是**日落清单**，接上线必须同时移出清单。
    #[test]
    fn every_pack_key_reaches_a_consumer() {
        let corpus = consumer_code();
        assert!(
            corpus.len() > 10_000,
            "消费语料仅 {} 字节 —— 语料装载失真（空语料会让**每个**键都判不可达）",
            corpus.len()
        );
        // 正对照：一个**活**键必须被判为可达（扫描器一旦瞎了，它会掉进不可达集合）
        let computed = computed_unreachable_keys();
        assert!(
            !computed.contains("common.points"),
            "正对照失败：`common.points` 是活键却被判不可达 —— 扫描器已失真"
        );
        // 清单不得有重复（重复会让「精确相等」掩盖一条真正缺失的条目）
        let declared = declared_unreachable_keys();
        assert_eq!(
            declared.len(),
            UNREACHABLE_PACK_KEYS.len(),
            "日落清单有重复键：声明 {} 条，去重后 {} 条",
            UNREACHABLE_PACK_KEYS.len(),
            declared.len()
        );
        let added: Vec<&String> = computed.difference(&declared).collect();
        let wired: Vec<&String> = declared.difference(&computed).collect();
        assert!(
            added.is_empty() && wired.is_empty(),
            "语言包可达性漂移：\n  \
             - 新增的不可达键（没人引用的孤儿：给它接上消费者，或加入 `UNREACHABLE_PACK_KEYS` 并注明类别）：{added:?}\n  \
             - 清单里已可达 / 已不存在的键（把日落键接上线后必须**同时**移出清单）：{wired:?}"
        );
    }

    /// 阴性对照：可达性判别式必须真的会失败（恒真的检查等价于没有检查）。
    #[test]
    fn pack_reachability_checker_detects_injected_defects() {
        let keys: Vec<String> = [
            "a.b",
            "a.bc",
            "c.d",
            "pfx.1",
            "pfx.9",
            "live.key",
            "unused.key",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let corpus = r#"T("a.bc"); T("live.key"); T("pfx." + i); <p data-i18n="c.d">x</p>"#;
        let prefixes: BTreeSet<String> = ["pfx.".to_string()].into_iter().collect();
        let un = unreachable_keys(keys.iter(), corpus, &prefixes);

        assert!(un.contains("unused.key"), "没人引用的键未被判不可达");
        assert!(
            un.contains("a.b"),
            "边界规则失效：`a.b` 被 `a.bc` 里的子串命中（子串匹配把兄弟键当证据，坑 #333）"
        );
        assert!(!un.contains("a.bc"), "字面引用的键被判不可达");
        assert!(!un.contains("live.key"), "字面引用的键被判不可达");
        assert!(!un.contains("c.d"), "`data-i18n` 引用的键被判不可达");
        assert!(
            !un.contains("pfx.1") && !un.contains("pfx.9"),
            "动态前缀规则失效：`pfx.*` 被判不可达"
        );

        // 前缀来自语料本身 ⇒ 去掉那段拼接，该家族立刻变不可达（这条是**合成**输入，
        // 因为真树上的 share.day.* 同时被 index.html 静态绑定 ⇒ 该规则在真语料上冗余）
        let no_prefix = unreachable_keys(keys.iter(), corpus, &BTreeSet::new());
        assert!(
            no_prefix.contains("pfx.1") && no_prefix.contains("pfx.9"),
            "去掉前缀后 `pfx.*` 仍被判可达 —— 前缀规则不是从语料派生的"
        );

        // 注释不是消费者（#296）：注释里的键名不得让键假可达
        assert!(
            !key_token_occurs(
                &strip_js_comments("// T(\"ghost.key\")\n/* x=\"ghost.key\" */\nT(\"live.key\")"),
                "ghost.key"
            ),
            "JS 注释里的键名被当成了消费者"
        );
        assert!(
            !key_token_occurs(
                &strip_html_comments("<!-- <p data-i18n=\"ghost.key\">x</p> -->"),
                "ghost.key"
            ),
            "HTML 注释里的键名被当成了消费者"
        );

        // 空语料 ⇒ 每个键都不可达（空集上的「全部可达」是最危险的那种假绿）
        let all = unreachable_keys(keys.iter(), "", &BTreeSet::new());
        assert_eq!(all.len(), keys.len(), "空语料下并非全部键都判不可达");
    }

    /// 插值变量门禁（C2007）：`T("key", { … })` 必须把该键文案里**所有** `{name}` 都补上。
    ///
    /// 为什么需要（`ui/js/i18n.js` 的 `t()` 实现）：
    /// ```js
    /// s = s.replace(/\{(\w+)\}/g, (m, k) => vars[k] !== undefined ? String(vars[k]) : m);
    /// ```
    /// 变量缺失时**占位符按字面返回**，于是界面会直接显示 `{n}` 或 `{n} 次`。
    /// 这类缺陷既不会报错、也不会缺键 —— 上述四条键集断言**全部照绿**，只有肉眼能发现。
    ///
    /// 同族的静态形态更绝对：`data-i18n*` 属性由 `applyStatic()` 用 `innerHTML` 写入，
    /// **没有任何传参通道**，所以静态绑定的文案一旦含占位符就**永远**显示原文。
    #[test]
    fn every_t_call_site_supplies_its_placeholders() {
        let LanguagePacks { zh, en, .. } = packs();
        let sites = scan_t_call_sites(APP_JS);
        // 同一个规则、同一批调用点：直接复用已发布的 T 字面量阳性对照
        assert_eq!(
            sites.len(),
            T_LITERAL_COUNT,
            "调用点扫描器与 T 字面量扫描器规则分叉：字面量应为 {T_LITERAL_COUNT}，实得 {}",
            sites.len()
        );

        let mut problems: Vec<String> = Vec::new();

        // ① 调用点未提供所需变量
        for site in &sites {
            // 以 `.` 结尾的是动态拼接前缀（`"share.day." + n`），本身不是完整键
            if site.key.ends_with('.') {
                continue;
            }
            let mut needed = placeholders(zh.get(&site.key).map(String::as_str).unwrap_or(""));
            needed.extend(placeholders(
                en.get(&site.key).map(String::as_str).unwrap_or(""),
            ));
            needed.sort();
            needed.dedup();
            let missing: Vec<&String> = needed.iter().filter(|p| !site.vars.contains(p)).collect();
            if !missing.is_empty() {
                problems.push(format!(
                    "T(\"{}\") 缺少插值变量 {missing:?}（实际提供 {:?}）—— 界面会原样显示花括号",
                    site.key, site.vars
                ));
            }
        }

        // ② 静态绑定的属性值含占位符 ⇒ 永远无法插值
        for (name, src) in [("ui/index.html", INDEX_HTML), ("ui/js/app.js", APP_JS)] {
            for key in scan_static_attributes(src) {
                if key.contains("${") || key.contains('+') {
                    continue; // 运行期拼接，此处无法静态判定
                }
                for (lang, pack) in [("zh", &zh), ("en", &en)] {
                    if let Some(v) = pack.get(&key) {
                        let ph = placeholders(v);
                        if !ph.is_empty() {
                            problems.push(format!(
                                "{name} 的静态属性 data-i18n=\"{key}\" 绑定的 {lang} 文案含占位符 {ph:?} —— \
                                 静态属性没有传参通道（applyStatic 只用 innerHTML），会永远显示原文"
                            ));
                        }
                    }
                }
            }
        }

        assert!(
            problems.is_empty(),
            "插值占位符不匹配（界面会显示原始占位符而非数值）：\n  - {}",
            problems.join("\n  - ")
        );
    }

    /// 阴性对照：占位符检查器必须真的会失败。
    ///
    /// 只断言「当前代码 0 缺陷」是不够的 —— 一个恒真的检查等价于没有检查。
    /// 这里直接拿语料构造真实缺陷形态，断言检查器把它们报出来。
    #[test]
    fn placeholder_checker_detects_injected_defects() {
        let LanguagePacks { zh, en, .. } = packs();

        // `cnt.calls`（zh "{n} 次" / en "{n}"）确实需要变量 n —— 前置条件。
        // ⚠️ 必须取**值**：C2007 第一版对键名取占位符，得到空集，
        // 被这条前置条件当场拦下（否则整个门禁会在错误语料上「通过」）。
        assert_eq!(
            placeholders(zh.get("cnt.calls").expect("基准包应有 cnt.calls")),
            vec!["n".to_string()],
            "前置条件：cnt.calls 的中文文案应恰含一个占位符 n"
        );

        // ① 模拟「漏传变量」：`T("cnt.calls")`（无实参对象）⇒ 必须报出
        let sites = scan_t_call_sites("T(\"cnt.calls\")");
        assert_eq!(sites.len(), 1, "对照语料应扫出 1 个调用点");
        assert!(sites[0].vars.is_empty(), "对照语料不应扫出任何变量");
        let needed = placeholders(zh.get(&sites[0].key).map(String::as_str).unwrap_or(""));
        assert!(
            needed.iter().any(|p| !sites[0].vars.contains(p)),
            "阴性对照失败：漏传变量未被检出"
        );

        // ② 模拟「变量名写错」：`T("cnt.calls", { count: 1 })` ⇒ 也必须报出
        let sites = scan_t_call_sites("T(\"cnt.calls\", { count: rt.month_calls })");
        assert_eq!(sites.len(), 1, "对照语料应扫出 1 个调用点");
        assert_eq!(
            sites[0].vars,
            vec!["count".to_string()],
            "扫描器应取出实参对象里的变量名 count"
        );
        assert!(
            !sites[0].vars.contains(&"n".to_string()),
            "阴性对照失败：写错的变量名被误认为正确"
        );

        // ③ 阳性对照：变量名正确时不得报错
        let sites = scan_t_call_sites("T(\"cnt.calls\", { n: rt.month_calls })");
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].vars, vec!["n".to_string()]);
        assert!(
            placeholders(en.get("cnt.calls").map(String::as_str).unwrap_or(""))
                .iter()
                .all(|p| sites[0].vars.contains(p)),
            "阳性对照失败：正确传参被误报"
        );

        // ④ 实参对象里可以嵌套调用（真实写法：`{ view: T(VIEW_TITLE[id] || id) }`）。
        //    内层调用的首参是**标识符**而非字面量，因此它不构成本模块意义的「字面量调用点」；
        //    但外层必须仍能正确取出 `view` 这个变量名。
        let sites = scan_t_call_sites("T(\"dash.title\", { view: T(VIEW_TITLE[id] || id) })");
        assert_eq!(
            sites.len(),
            1,
            "内层首参不是字面量，不应额外计入字面量调用点"
        );
        assert_eq!(sites[0].key, "dash.title");
        assert_eq!(
            sites[0].vars,
            vec!["view".to_string()],
            "扫描器未能在嵌套调用下正确取出实参变量"
        );

        // ④b 内层若**也是**字面量调用，则两个调用点都要被覆盖（各自独立校验占位符）
        let sites = scan_t_call_sites("T(\"a.b\", { x: T(\"c.d\") })");
        assert_eq!(sites.len(), 2, "内层字面量调用也应被计入");
        assert_eq!(sites[0].key, "a.b");
        assert_eq!(sites[0].vars, vec!["x".to_string()]);
        assert_eq!(sites[1].key, "c.d");
        assert!(sites[1].vars.is_empty());

        // ⑤ 文案里含括号/逗号不得扰乱实参解析
        let sites = scan_t_call_sites("T(\"a.b\", { n: f(1, 2), m: \"x, y: z\" })");
        assert_eq!(sites.len(), 1);
        assert_eq!(
            sites[0].vars,
            vec!["n".to_string(), "m".to_string()],
            "字符串内部的 `x, y: z` 不应被当作变量名"
        );
    }

    /// `ui/js/api.js` 的错误文案必须**按 key 取**，不得内嵌中文原文（C2029）。
    ///
    /// 为什么单独为它写一条：`api.js` 是请求咽喉（全部 `api.*` 调用点都流经它），却是本模块
    /// 之外的文件，而它喂给 `mapErr` 的形态 `mapErr("中文")` 又**不是**键使用门禁认得的
    /// `T("字面量")` ⇒ 中文原文可在 en 模式下直接抛给用户，而 `cargo test` 全绿（C2028 实测
    /// 8 种真实后端响应形态里 6 种泄漏中文）。三条断言分别对应三条门禁的失效点：
    /// ① 与键存在门禁对应（不写原文，改取键）；② 与键使用门禁对应（取到的键必须存在）；
    /// ③ 与占位符门禁对应（`{n}` 必须由 `vars` 供给）。
    #[test]
    fn api_client_error_text_is_key_based() {
        let LanguagePacks { zh, en, .. } = packs();
        let code = strip_js_comments(API_JS);

        // ① 代码（注释之外）里不得出现任何非 ASCII 字符。
        //    注释本就该是中文，`strip_js_comments` 已剥掉；剩下还带 CJK 的只可能是字面量。
        let offenders: Vec<(usize, &str)> = code
            .lines()
            .enumerate()
            .filter(|(_, l)| !l.is_ascii())
            .map(|(n, l)| (n + 1, l.trim()))
            .collect();
        assert!(
            offenders.is_empty(),
            "ui/js/api.js 的代码里出现非 ASCII 字符（用户可见文案必须走键，注释请用 //）：\n  - {}",
            offenders
                .iter()
                .map(|(n, l)| format!("L{n}: {l}"))
                .collect::<Vec<_>>()
                .join("\n  - ")
        );

        // ② 本文件写的每个键字面量，都必须在**两个包**里都存在。
        //    与 app.js 共用同一个 `T("字面量")` 识别规则（坑 75：规则一旦分叉，
        //    两条门禁统计的就不再是同一批调用点）。
        let keys = scan_t_literals(API_JS);
        assert!(
            !keys.is_empty(),
            "ui/js/api.js 未扫到任何 T() 键字面量 —— 提取器已失真或本文件已不再取键，拒绝继续"
        );
        let distinct: BTreeSet<String> = keys.iter().cloned().collect();
        let missing = unresolved(
            distinct.iter(),
            &zh.keys().cloned().collect(),
            &en.keys().cloned().collect(),
        );
        assert!(
            missing.is_empty(),
            "ui/js/api.js 引用了语言包里不存在的键（界面会原样显示键名）：{missing:?}"
        );

        // ③ 本文件用到的每个键，其文案里的 `{name}` 都必须由调用点供给。
        //    `mapErr` 是反例：它以 `t(key)`（无 vars）收尾，带占位符的值经它只会原样
        //    输出花括号 —— 所以兜底文案必须走 `tr("err.http", { n: … })`，不能进 ERR_MAP。
        for key in &distinct {
            let mut needed = placeholders(zh.get(key).map(String::as_str).unwrap_or(""));
            needed.extend(placeholders(en.get(key).map(String::as_str).unwrap_or("")));
            needed.sort();
            needed.dedup();
            if needed.is_empty() {
                continue;
            }
            // 调用点必须为每个占位符提供变量：在源码里找 `T("key", { … })` 形态
            let needle = format!("T(\"{key}\"");
            let supplied = code.contains(&format!("{needle}, {{"));
            assert!(
                supplied,
                "ui/js/api.js 的 T(\"{key}\") 文案含占位符 {needed:?}，但调用点未提供 vars 对象 —— \
                 界面会原样显示花括号"
            );
        }
    }

    /// 阴性对照：上面三条断言必须真的会失败（否则等于没写）。
    ///
    /// ⚠️ 语料**计数中性**地注入：只往对照语料里加缺陷，不改变真实文件，
    /// 因此它证明的是「检查器有牙齿」，而不是「本次改动没引入缺陷」（坑 85）。
    #[test]
    fn api_js_checker_detects_injected_defects() {
        let LanguagePacks { zh, en, .. } = packs();

        // ① 中文原文（原封不动地模拟 C2028 修前 `api.js` 里的那一行）必须被判为违规
        let bad = "throw { status: 0, message: mapErr(\"网络不可用，请检查后端服务是否启动\") };";
        assert!(
            !strip_js_comments(bad).is_ascii(),
            "阴性对照失败：中文原文未被判出"
        );
        // 同行混合（代码 + 注释）也必须判出 —— 这是「按行跳过」式近似会漏掉的形态
        let mixed = "const s = \"中文\"; // 说明";
        assert!(
            !strip_js_comments(mixed).is_ascii(),
            "阴性对照失败：代码+注释同行时漏判"
        );
        // 注释本身不得被判出（否则本断言会永远为红）
        let commented = "// 这里是中文注释\nconst s = \"ok\";";
        assert!(
            strip_js_comments(commented).is_ascii(),
            "阳性对照失败：注释被误判为字面量"
        );
        // 块注释同理
        let block = "/* 中文块注释 */ const s = \"ok\";";
        assert!(
            strip_js_comments(block).is_ascii(),
            "阳性对照失败：块注释被误判"
        );

        // ② 拼错的键必须被报出（真实文件里存在的键作为阳性对照）
        let zh_keys: BTreeSet<String> = zh.keys().cloned().collect();
        let en_keys: BTreeSet<String> = en.keys().cloned().collect();
        let typo = ["err.nettwork".to_string()];
        assert_eq!(
            unresolved(typo.iter(), &zh_keys, &en_keys).len(),
            1,
            "阴性对照失败：拼错的键未被报出"
        );
        let real = [String::from("err.network")];
        assert!(
            unresolved(real.iter(), &zh_keys, &en_keys).is_empty(),
            "阳性对照失败：真实存在的键被判为缺失"
        );

        // ③ 保真（C2129）：注释之外的文本必须**逐码位**保留。
        //    曾经的 `out.push(b[i] as char)` 把 UTF-8 字节当成码位 ⇒ 中文字面量变乱码；
        //    而上面四条控制全用 `is_ascii()`，乱码同样非 ASCII ⇒ 失真**没有证人**。
        //    直到 `err_map_pairs` 拿剥完注释的 `ERR_MAP` 去与未加工的源文比 `contains()`
        //    才暴露。这条断言直接钉住保真性，而不是绕道一个弱谓词。
        let fidelity = "const a = \"中文\"; // 行注释\nconst b = \"✅\"; /* 块注释 */";
        let stripped = strip_js_comments(fidelity);
        assert!(
            stripped.contains("\"中文\""),
            "strip_js_comments 未逐字透传（`byte as char` 乱码）：{stripped:?}"
        );
        assert!(
            stripped.contains("\"✅\""),
            "strip_js_comments 未逐字透传（`byte as char` 乱码）：{stripped:?}"
        );
        assert!(
            !stripped.contains("行注释") && !stripped.contains("块注释"),
            "strip_js_comments 没剥掉注释：{stripped:?}"
        );

        // ④ 占位符：`err.http` 确实需要 `n`（前置条件），缺 vars 必须被检出
        assert_eq!(
            placeholders(zh.get("err.http").expect("基准包应有 err.http")),
            vec!["n".to_string()],
            "前置条件：err.http 的中文文案应恰含一个占位符 n"
        );
        assert!(
            placeholders(en.get("err.http").map(String::as_str).unwrap_or(""))
                == vec!["n".to_string()],
            "前置条件：err.http 的英文文案应恰含一个占位符 n"
        );
        let without_vars = "T(\"err.http\")";
        assert!(
            !without_vars.contains(", {"),
            "阴性对照失败：缺 vars 的调用点被判为合法"
        );
        let with_vars = "T(\"err.http\", { n: resp.status })";
        assert!(
            with_vars.contains(", {"),
            "阳性对照失败：带 vars 的调用点被判为非法"
        );
    }

    /// 从 `s[i]`（须是 `"`）读一个字符串字面量，返回内容与其后的下标。
    ///
    /// 只处理无转义/简单转义的形态 —— 本模块读的三处源文件（`i18n.js` 的 `ERR_MAP`、
    /// 后端 `"error"` 文案、合成对照语料）都不含复杂转义。
    fn read_quoted(s: &str, i: usize) -> Option<(String, usize)> {
        if s.as_bytes().get(i) != Some(&b'"') {
            return None;
        }
        let b = s.as_bytes();
        let mut j = i + 1;
        while j < b.len() {
            if b[j] == b'\\' {
                j += 2;
                continue;
            }
            if b[j] == b'"' {
                return Some((s[i + 1..j].to_string(), j + 1));
            }
            j += 1;
        }
        None
    }

    fn skip_ws(s: &str, mut i: usize) -> usize {
        let b = s.as_bytes();
        while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\r' | b'\n') {
            i += 1;
        }
        i
    }

    /// 解析 `ERR_MAP` 的 `[ "中文原文", "键" ]` 条目（注释先剥掉）。
    fn err_map_pairs() -> Vec<(String, String)> {
        let body = pack_region(I18N_JS, ERR_MAP_START, ERR_MAP_END);
        let code = strip_js_comments(body);
        let mut out = Vec::new();
        let mut from = 0usize;
        while let Some(rel) = code[from..].find('[') {
            let open = from + rel;
            if let Some((src_text, after)) = read_quoted(&code, skip_ws(&code, open + 1)) {
                let comma = skip_ws(&code, after);
                if code.as_bytes().get(comma) == Some(&b',') {
                    if let Some((key, end)) = read_quoted(&code, skip_ws(&code, comma + 1)) {
                        out.push((src_text, key));
                        from = end;
                        continue;
                    }
                }
            }
            from = open + 1;
        }
        out
    }

    fn is_cjk(c: char) -> bool {
        matches!(c, '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}')
    }

    /// 后端 `"error"` 文案的两种书写形态：
    /// `json!({ "error": "…" })` / `json!({ "error": format!("…") })`，以及
    /// `err_json(StatusCode::X, "…")` / `err_json(StatusCode::X, &format!("…"))`。
    ///
    /// `#[cfg(test)]` 之后的内容一律不读：测试里的断言说明也是中文，但它们不上线。
    fn backend_error_literals(src: &str) -> Vec<String> {
        let src = match src.find("#[cfg(test)]") {
            Some(i) => &src[..i],
            None => src,
        };
        let mut out = Vec::new();

        // 形态 A：`"error"` 之后（可带 `format!(` / `(`）紧跟的字面量
        let mut from = 0usize;
        while let Some(rel) = src[from..].find("\"error\"") {
            let after = from + rel + "\"error\"".len();
            let mut i = skip_ws(src, after);
            if src.as_bytes().get(i) == Some(&b':') {
                i = skip_ws(src, i + 1);
            }
            if src[i..].starts_with("format!") {
                i += "format!".len();
            }
            i = skip_ws(src, i);
            if src.as_bytes().get(i) == Some(&b'(') {
                i = skip_ws(src, i + 1);
            }
            if let Some((lit, end)) = read_quoted(src, i) {
                out.push(lit);
                from = end;
            } else {
                from = after;
            }
        }

        // 形态 B：`err_json(` 的第二个实参（跳过状态码参数）
        let mut from = 0usize;
        while let Some(rel) = src[from..].find("err_json(") {
            let after = from + rel + "err_json(".len();
            let mut i = after;
            while i < src.len() && src.as_bytes()[i] != b',' {
                i += 1;
            }
            i = skip_ws(src, i + 1);
            if src.as_bytes().get(i) == Some(&b'&') {
                i = skip_ws(src, i + 1);
            }
            if src[i..].starts_with("format!") {
                i += "format!".len();
            }
            i = skip_ws(src, i);
            if src.as_bytes().get(i) == Some(&b'(') {
                i = skip_ws(src, i + 1);
            }
            if let Some((lit, end)) = read_quoted(src, i) {
                out.push(lit);
                from = end;
            } else {
                from = after;
            }
        }
        out
    }

    /// 语料里**没有任何词表条目命中**的消息。
    ///
    /// 匹配规则与 `mapErr` 一致：**子串**（`msg.indexOf(条目原文) !== -1`）。
    /// 因此带运行期插值的消息（`部门「{name}」已存在`）在词表里只登记到插值符之前的
    /// 稳定前缀即可 —— 写全模板反而永远匹配不上。
    fn unworded_messages(corpus: &[String], table: &[(String, String)]) -> Vec<String> {
        corpus
            .iter()
            .filter(|m| !table.iter().any(|(cn, _)| m.contains(cn.as_str())))
            .cloned()
            .collect()
    }

    /// `mapErr` 的取值规则：命中多条时取**最长**的那条（否则通用短条目会遮蔽具体条目）。
    fn longest_match(msg: &str, table: &[(String, String)]) -> Option<String> {
        let mut best: Option<&str> = None;
        for (cn, key) in table {
            let better = match best {
                None => true,
                Some(b) => cn.chars().count() > b.chars().count(),
            };
            if msg.contains(cn.as_str()) && better {
                best = Some(key);
            }
        }
        best.map(str::to_string)
    }

    /// 后端每一条中文错误文案，都必须能被 `ERR_MAP` 命中（C2129）。
    ///
    /// `ui/js/api.js` 是唯一同时**构造**错误文案并调用 `mapErr` 的地方：它把后端的
    /// `error` 字段整串交给词表。词表只翻译手写登记过的中文，其余原样返回 ⇒ 后端写了中文、
    /// 而词表没登记，英文界面上就显示中文。
    ///
    /// 修前实测（jsdom 启真 `ui/index.html` + 四脚本，只 stub `fetch`）：`src/` 的 45 条中文
    /// 错误里有 **20 条**不在表里；其中「部门下还有 N 名成员，请先调整成员部门」
    /// （`src/routes/org.rs` DELETE 部门 → 409）由**可达控件**触发 —— 删一个还有成员的部门，
    /// 屏幕上的 toast 就是中文。
    ///
    /// 与 `api_client_error_text_is_key_based` 的分工：那条管**前端**咽喉不得内嵌中文原文，
    /// 这条管**后端**写下的中文原文有没有对应的词表条目。两条都指向同一个咽喉。
    #[test]
    fn every_backend_error_message_reaches_the_wordlist() {
        let LanguagePacks { zh, en, .. } = packs();
        let table = err_map_pairs();
        assert_eq!(
            table.len(),
            ERR_MAP_ENTRY_COUNT,
            "ERR_MAP 解析失真：应得 {ERR_MAP_ENTRY_COUNT} 条，实得 {} —— 表结构变了？",
            table.len()
        );

        let mut corpus: BTreeSet<String> = BTreeSet::new();
        for (_, src) in BACKEND_ERROR_SOURCES {
            corpus.extend(backend_error_literals(src));
        }
        let cjk: Vec<String> = corpus
            .iter()
            .filter(|l| l.chars().any(is_cjk))
            .cloned()
            .collect();
        assert_eq!(
            cjk.len(),
            BACKEND_ERROR_CJK_COUNT,
            "后端中文错误文案数应为 {BACKEND_ERROR_CJK_COUNT}，实得 {} —— \
             提取器已失真或后端文案真的变了（变了就更新这个常量，别让它变成一句空话）",
            cjk.len()
        );

        // ① 不变量：每条后端中文文案都必须被词表命中
        let unworded = unworded_messages(&cjk, &table);
        assert!(
            unworded.is_empty(),
            "后端有 {} 条中文错误不在 ERR_MAP 里 —— en 模式下 mapErr 只能原样返回，\
             用户会在英文界面上看到中文：\n  - {}",
            unworded.len(),
            unworded.join("\n  - ")
        );

        // ② 词表的**目标键**必须在两个包里都存在（否则 mapErr 把键名当文案显示给用户）
        let targets: Vec<String> = {
            let mut t: Vec<String> = table.iter().map(|(_, k)| k.clone()).collect();
            t.sort();
            t.dedup();
            t
        };
        let missing = unresolved(
            targets.iter(),
            &zh.keys().cloned().collect(),
            &en.keys().cloned().collect(),
        );
        assert!(
            missing.is_empty(),
            "ERR_MAP 指向了语言包里不存在的键（界面会显示键名）：{missing:?}"
        );

        // ③ 最长匹配：带插值的消息**渲染后**必须命中具体条目，不被通用短条目吃掉。
        //    这一组是「登记前缀而不是全模板」这个决定的运行期证据 —— 全模板永远匹配不上。
        for (msg, want) in [
            ("部门「研发中心」已存在", "err.deptExists"),
            ("部门下还有 3 名成员，请先调整成员部门", "err.deptNotEmpty"),
            (
                "流式协议转换 openai_chat → anthropic 暂未支持",
                "err.streamConvertUnsupported",
            ),
            ("type 必须为 consume / earn / all", "err.txTypeInvalid"),
            (
                "name 不能为空且 quota 必须大于 0",
                "err.deptNameQuotaRequired",
            ),
            ("验证码不存在或已过期，请重新获取", "err.codeExpired"),
        ] {
            assert_eq!(
                longest_match(msg, &table).as_deref(),
                Some(want),
                "消息 {msg:?} 应映射到 {want}"
            );
        }
        // 阴性对照：泛化到具体条目的遮蔽必须被「最长匹配」挡住
        assert_eq!(
            longest_match("name 不能为空且 quota 必须大于 0", &table).as_deref(),
            Some("err.deptNameQuotaRequired"),
            "通用条目 `quota 必须大于 0` 不得遮蔽更具体的那条"
        );
    }

    /// 阴性对照：词表检查器必须真的会失败（否则第 ① 条断言等于没写）。
    #[test]
    fn backend_error_wordlist_checker_detects_injected_defects() {
        let table = err_map_pairs();

        let injected = ["后端新增的错误文案，还没人登记".to_string()];
        assert_eq!(
            unworded_messages(&injected, &table).len(),
            1,
            "阴性对照失败：未登记的中文错误未被报出"
        );
        let registered = ["点数余额不足".to_string()];
        assert!(
            unworded_messages(&registered, &table).is_empty(),
            "阳性对照失败：已登记的中文错误被判为未登记"
        );

        // 提取器自身的对照：两种书写形态都必须认得出，非文案不得混进来
        let sample = r#"
            fn f() {
                json!({ "error": "中文甲" })
                json!({ "error": format!("中文乙 {u}") })
                json!({ "error": { "message": msg } })
                err_json(StatusCode::BAD_REQUEST, "中文丙")
                err_json(StatusCode::BAD_REQUEST, &format!("流式协议转换 {u}"))
                let decoy = "这不是错误文案";
            }
            #[cfg(test)]
            mod tests { fn t() { json!({ "error": "测试文案" }) } }
        "#;
        let got = backend_error_literals(sample);
        for want in ["中文甲", "中文乙 {u}", "中文丙", "流式协议转换 {u}"] {
            assert!(
                got.iter().any(|g| g == want),
                "提取器漏掉了形态 {want:?}，实得 {got:?}"
            );
        }
        assert!(
            !got.iter().any(|g| g == "测试文案"),
            "`#[cfg(test)]` 之后的字面量不得被算作用户可见文案"
        );
        assert!(
            !got.iter().any(|g| g == "这不是错误文案"),
            "与 `error` 字段无关的字面量不得被误收"
        );
    }

    /// 手写清单的兜底：**文件系统里的「谁会发出错误文案」才是名册**。
    ///
    /// 没有这一条，新增一个文件就是**静默**逃过上面那条门禁（名册不随文件增长 ——
    /// C2072 / C2127 的同一个形状）。这里刻意读一次文件系统（`CARGO_MANIFEST_DIR` 是
    /// 编译期绝对路径，与工作目录无关），把「名册」变成**派生**：
    ///
    /// 判据不是「`src/routes/` 的目录项与名册一致」（那只覆盖一个目录，顶层新增
    /// `src/foo.rs` 照样逃逸），而是**用同一个提取器扫全部 `src/*.rs` 与
    /// `src/routes/*.rs`，产出错误字面量的文件集合必须恰好等于名册**。等号两侧都带牙齿：
    /// 少登记一个有产出的文件 = 漏扫；名册里留一个没有产出的文件 = 名册在腐烂。
    ///
    /// （`src/` 顶层其余文件当前产出 0 条：gate 模块的示例都写在 `#[cfg(test)]` 之内，
    /// 而提取器在第一个 `#[cfg(test)]` 处截断 —— 这正是它必须截断的理由之一。）
    #[test]
    fn backend_error_sources_cover_every_file_that_emits_an_error_literal() {
        let root = env!("CARGO_MANIFEST_DIR");
        let mut candidates: Vec<String> = vec!["src/gateway.rs".to_string()];
        for dir in ["src", "src/routes"] {
            let d = format!("{root}/{dir}");
            for e in std::fs::read_dir(&d)
                .unwrap_or_else(|_| panic!("应能读取 {dir}/"))
                .filter_map(|e| e.ok())
            {
                let name = e.file_name().to_string_lossy().into_owned();
                if !name.ends_with(".rs") {
                    continue;
                }
                let rel = format!("{dir}/{name}");
                if !candidates.contains(&rel) {
                    candidates.push(rel);
                }
            }
        }

        let mut emitters: Vec<String> = Vec::new();
        for rel in &candidates {
            let src = std::fs::read_to_string(format!("{root}/{rel}"))
                .unwrap_or_else(|_| panic!("应能读取 {rel}"));
            if !backend_error_literals(&src).is_empty() {
                emitters.push(rel.clone());
            }
        }
        emitters.sort();

        let mut listed: Vec<String> = BACKEND_ERROR_SOURCES
            .iter()
            .map(|(p, _)| p.to_string())
            .collect();
        listed.sort();

        assert_eq!(
            emitters, listed,
            "BACKEND_ERROR_SOURCES 与「实际发出错误文案的文件」不一致 —— 左＝磁盘上的产出者，\
             右＝名册。漏登记的产出者，其文案不受词表门禁覆盖（英文界面上就是中文）；\
             名册里多出的条目则是在腐烂。"
        );
    }

    /// 阴性对照：把「删键」「拼错键」注入语料，检查器必须真的报错。
    ///
    /// 没有这一条，上面四个断言无法自证「它们有能力失败」—— 一个永远为真的检查等价于没有检查。
    #[test]
    fn checker_detects_injected_defects() {
        let LanguagePacks {
            zh_keys: zh,
            en_keys: en,
            ..
        } = packs();

        // ① 模拟「中文包少了键」：必须被报出
        let mut zh_missing = zh.clone();
        assert!(
            zh_missing.remove("nav.main"),
            "前置条件：基准包里应有 nav.main"
        );
        let keys = ["nav.main".to_string()];
        assert_eq!(
            unresolved(keys.iter(), &zh_missing, &en).len(),
            1,
            "阴性对照失败：中文包删键后检查器未报错"
        );

        // ② 模拟「键名拼错」：必须被报出
        let typo = ["nav.mian".to_string()];
        assert_eq!(
            unresolved(typo.iter(), &zh, &en).len(),
            1,
            "阴性对照失败：拼错的键名未被报出"
        );

        // ③ 阳性对照：真实存在的键不得被报错
        let good = ["nav.main".to_string()];
        assert!(
            unresolved(good.iter(), &zh, &en).is_empty(),
            "阳性对照失败：真实存在的键被误报为缺失"
        );
    }

    /// 「这段文案是在要求用户重新登录吗？」
    ///
    /// 这是**检测器**不是断言：必须能对合成语料给出真、假两种答案，才配拿去断言真实文案
    /// （否则一个恒假的检测器会让下面那条断言空转通过）。
    fn demands_reauthentication(text: &str) -> bool {
        [
            "重新登录",
            "重新认证",
            "登录已过期",
            "sign in again",
            "session expired",
        ]
        .iter()
        .any(|needle| text.contains(needle))
    }

    /// 会话恢复失败的文案**不得**写成「已登出」。
    ///
    /// boot 的会话恢复只在 **401** 时才把用户判成未登录（token 已被服务端作废）；
    /// 网络错误 / 5xx / 网关 504 等**非 401** 失败时 token 仍在，`ui/js/app.js::restoreSession`
    /// 会重试一次后**照常进入 app**（rant 2026-09-14T21:15:02 第 4 条）。此时若屏幕上出现
    /// 「请重新登录」，就是把「加载失败」谎报成「未登录」—— 宿主 2026-09-14 21:00 实测的现象
    /// （`/api/me` 被拖到 504 ⇒ 停在登录页、token 仍在、URL hash 仍指向上次视图）。
    ///
    /// 这里只钉**文案**这一半：两档必须不同，且「加载失败」那档不得要求重新登录。
    /// 视图状态那一半（非 401 必须进 app）是 JS 控制流，CI 里没有 JS 测试运行器，
    /// 由 `ui/README.md` 的「会话恢复」小节作为约定与冒烟测试说明承接。
    #[test]
    fn session_failure_copy_does_not_claim_the_user_is_logged_out() {
        let LanguagePacks { zh, en, .. } = packs();
        let get = |m: &std::collections::BTreeMap<String, String>, k: &str| {
            m.get(k)
                .unwrap_or_else(|| panic!("前置条件：语言包应有 {k}"))
                .clone()
        };
        let fail_zh = get(&zh, "login.session.fail");
        let fail_en = get(&en, "login.session.fail");
        let expired_zh = get(&zh, "login.session.expired");
        let expired_en = get(&en, "login.session.expired");

        // ① 检测器的阳性对照（合成语料，不依赖语言包现状）：它必须认得「要求重新登录」的写法。
        assert!(
            demands_reauthentication("会话已过期，请重新登录")
                && demands_reauthentication("Session expired, please sign in again"),
            "阳性对照失败：检测器认不出「要求重新登录」的写法 —— 下面的断言等于没写"
        );
        // ② 阴性对照：不要求重新登录的写法不得被误报
        assert!(
            !demands_reauthentication("加载中…"),
            "阴性对照失败：检测器把中性的加载提示误报成「要求重新登录」"
        );

        // ③ 两档文案必须不同 —— 相同的话，用户从屏幕上无法分辨自己是「被登出」还是「没连上」。
        assert_ne!(
            fail_zh, expired_zh,
            "登录失败档与未登录档的中文文案相同：用户无法分辨状态"
        );
        assert_ne!(
            fail_en, expired_en,
            "the load-failure and not-signed-in English copies are identical — the two states become \
             indistinguishable on screen"
        );

        // ④ 真正的不变量：token 仍在的用户不该被要求重新登录。
        assert!(
            !demands_reauthentication(&fail_zh),
            "login.session.fail 的中文文案像是在要求重新登录（{fail_zh:?}）—— 但这条路径上 token 仍在，\
             用户并没有被登出；「加载失败」与「未登录」必须分开表达"
        );
        assert!(
            !demands_reauthentication(&fail_en),
            "the English login.session.fail copy reads like a sign-in-again instruction ({fail_en:?}) — \
             the token is still held on this path, so the user is not signed out"
        );
    }

    /// 取 `needle` **最后一次**出现处所属的 `if (…)` 条件头（从该 `if (` 到 `needle` 之间）。
    ///
    /// 取最后一次出现是刻意的：`function handleUnauthorized() {` 是定义、位置更靠前，
    /// 要检查的是**调用点**。
    fn enclosing_if<'a>(src: &'a str, needle: &str) -> Option<&'a str> {
        let at = src.rfind(needle)?;
        let start = src[..at].rfind("if (")?;
        Some(&src[start..at])
    }

    /// 取包含 `needle` 的那条语句（`needle` → 下一个 `;`）。
    ///
    /// 按分号切而不是按行切：调用点换行时按行取会漏掉实参 —— 形状断言必须容忍排版。
    fn statement_containing<'a>(src: &'a str, needle: &str) -> Option<&'a str> {
        let at = src.find(needle)?;
        let rest = &src[at..];
        let end = rest.find(';').unwrap_or(rest.len());
        Some(&rest[..end])
    }

    /// 含**全部** `needles` 的那条语句：`needles[0]` 的每一次命中都要检，命中同一条语句的才算。
    ///
    /// 为什么需要（C2133）：同一个选择器在一个函数里可以出现两次 —— `$("#usage-dept").innerHTML`
    /// 先被清空、再被渲染。只按第一次命中切语句，拿到的是**清空**那句（里面根本没有渲染调用），
    /// 门禁就会报一条与产品无关的假红。加一个 needle 把「哪一条语句」说清楚即可。
    fn statement_containing_all<'a>(src: &'a str, needles: &[&str]) -> Option<&'a str> {
        let (first, rest) = needles.split_first()?;
        let mut from = 0usize;
        while let Some(rel) = src[from..].find(first) {
            let at = from + rel;
            from = at + first.len();
            let tail = &src[at..];
            let end = tail.find(';').unwrap_or(tail.len());
            let stmt = &tail[..end];
            if rest.iter().all(|n| stmt.contains(n)) {
                return Some(stmt);
            }
        }
        None
    }

    /// 401 的语义必须由**调用方**声明：凭据端点的 401 不是「会话过期」（C2120）。
    ///
    /// `ui/js/api.js` 的 `request()` 对 401 一律 `handleUnauthorized()`（清 token + 回登录页 +
    /// 弹「登录已过期，请重新登录」），但 `POST /api/auth/login` **故意**用 401 表示「凭据不对」
    /// （`src/routes/mod.rs::login` 里两处 `unauthorized()`：邮箱不存在 / 口令错）。
    /// 修前实测（jsdom 启真 `index.html` + 四真脚本、驱动**真表单**、后端回 401）：屏幕上
    /// **同时**出现行内「邮箱或密码错误」与一句「登录已过期，请重新登录」—— 后者是错的，
    /// 这个用户从未登录过（`login.session.expired` 被念给了刚输错密码的人）。
    ///
    /// 咽喉层看不出端点语义，只能由调用方在 `opts.on401` 里声明。CI 里没有 JS 运行器，
    /// 因此这里只钉**形状**（调用点声明了 + 咽喉的 401 分支受该声明守卫），
    /// 运行期那一半由 `ui/README.md` 的「401 语义」小节与冒烟测试承接。
    #[test]
    fn credential_401_is_not_a_session_expiry() {
        let api_code = strip_js_comments(API_JS);
        let app_code = strip_js_comments(APP_JS);

        // ① 阳性对照（防「半修」）：会话失效的全局处置必须**仍然**存在。
        //    把 handleUnauthorized 整个删掉确实能让登录页不再弹错提示，但业务端点的 401
        //    就再也不会把用户送回登录页 —— 那是拿掉安全行为，不是修缺陷。
        assert!(
            api_code.contains("function handleUnauthorized"),
            "ui/js/api.js 里找不到 handleUnauthorized 的定义：会话失效的全局处置不得被删掉"
        );
        assert!(
            api_code.contains("T(\"login.session.expired\")"),
            "ui/js/api.js 不再抛出 login.session.expired：业务端点的 401 必须仍判为「会话失效」"
        );
        assert!(
            app_code.contains("window.__atpLogout"),
            "ui/js/app.js 不再注册 __atpLogout 钩子：会话失效时用户不会被送回登录页"
        );

        // ② 咽喉层：401 分支必须被「调用方声明」守卫住，不能无条件登出。
        let guard = enclosing_if(&api_code, "handleUnauthorized()")
            .expect("ui/js/api.js 里找不到对 handleUnauthorized() 的调用");
        assert!(
            guard.contains("CREDENTIAL_401"),
            "ui/js/api.js 对 401 无条件执行 handleUnauthorized()（守卫：{guard:?}）—— \
             凭据端点的 401 会顺带清掉用户的 token 并弹一句「登录已过期」"
        );

        // ③ 登录调用点必须声明自己是凭据端点 —— 这正是会被人「顺手改回去」的那一行。
        let login_stmt = statement_containing(&app_code, "\"/api/auth/login\"")
            .expect("ui/js/app.js 里找不到登录请求");
        assert!(
            login_stmt.contains("CREDENTIAL_401"),
            "登录请求未声明 401 语义（语句：{login_stmt:?}）—— 输错密码会同时弹出\
             「登录已过期，请重新登录」与行内「邮箱或密码错误」两句互相矛盾的提示"
        );

        // ④ 阴性对照：②③ 用的两个提取器必须能对**合成**的修前形态给出相反的答案，
        //    否则上面的断言只是恒真的形状巧合（坑 85：门禁「有牙齿」要用计数中性的注入证明）。
        let before_guard = "if (resp.status === 401) {\n      handleUnauthorized();\n    }";
        assert!(
            !enclosing_if(before_guard, "handleUnauthorized()")
                .expect("合成语料里应能取到守卫")
                .contains("CREDENTIAL_401"),
            "阴性对照失败：提取器认不出「无条件登出」的守卫 —— ② 等于没写"
        );
        let before_call =
            "const r = await api.post(\"/api/auth/login\", { email, password: pass });";
        assert!(
            !statement_containing(before_call, "\"/api/auth/login\"")
                .expect("合成语料里应能取到语句")
                .contains("CREDENTIAL_401"),
            "阴性对照失败：提取器认不出未声明的调用点 —— ③ 等于没写"
        );
        //    阳性对照：同一提取器对**修后**形态必须给出相反答案（真/假两种答案才算检测器）。
        let after_call =
            "const r = await api.post(\"/api/auth/login\", body, { on401: api.CREDENTIAL_401 });";
        assert!(
            statement_containing(after_call, "\"/api/auth/login\"")
                .expect("合成语料里应能取到语句")
                .contains("CREDENTIAL_401"),
            "阳性对照失败：提取器认不出已声明的调用点"
        );
    }

    /// 前端**自己切**线上时间戳的字段族：`dao::utc_iso()` 序列化出去的那一批。
    ///
    /// `transactions.time` 不在其中：`time` 这个名字太泛（游客 mock 的 `MM-DD HH:mm` 也叫
    /// `time`，`dailySeries` 对它取前 5 位是**故意的**），所以交易视图行的形状由
    /// `txs_to_view_row_carries_the_raw_timestamp` 单独按位置钉。
    const WIRE_TS_FIELDS: [&str; 2] = ["created_at", "last_used"];

    /// 前端现成的本地化 helper —— 出现在「被切的表达式」里就算这串是**先交给 helper 再切**的。
    const TIME_HELPERS: [&str; 5] = [
        "fmtPrecise(",
        "timeCell(",
        "timeAgo(",
        "localMD(",
        "utcMonth(",
    ];

    /// `at` 之前那个「表达式窗口」：从最近的 `,;:{}` 或换行起、到 `at` 为止（含两端之间的全部文本）。
    ///
    /// 刻意**不**在 `(` / `)` / 运算符处断开：`created: fmtPrecise(k.created_at)` 是个整体，
    /// 在括号处断开会把 helper 名切出去，于是「切的是 helper 的输出」这条合法形态会被误判成违规。
    fn expression_window(src: &str, at: usize) -> &str {
        let bytes = src.as_bytes();
        let mut i = at;
        while i > 0 {
            let c = bytes[i - 1] as char;
            if matches!(c, ',' | ';' | ':' | '{' | '}' | '\n' | '\r') {
                break;
            }
            i -= 1;
        }
        src[i..at].trim()
    }

    /// `src` 里对线上时间戳的**原地加工**（返回 表达式窗口 + 运算符）。
    fn inline_timestamp_processing(src: &str) -> Vec<(String, &'static str)> {
        let mut out = Vec::new();
        for op in [".slice(", ".replace("] {
            for (at, _) in src.match_indices(op) {
                let window = expression_window(src, at);
                if WIRE_TS_FIELDS.iter().any(|f| window.contains(f))
                    && !TIME_HELPERS.iter().any(|h| window.contains(h))
                {
                    out.push((window.to_string(), op));
                }
            }
        }
        out
    }

    /// 取 JS 函数体：`signature` 之后的第一个 `{` 到配对的 `}`（跳过字符串/模板字面量里的花括号）。
    fn js_function_body<'a>(src: &'a str, signature: &str) -> Option<&'a str> {
        let start = src.find(signature)? + signature.len();
        let open = src[start..].find('{')? + start;
        let bytes = src.as_bytes();
        let (mut depth, mut i, mut quote) = (0i32, open, 0u8);
        while i < bytes.len() {
            let c = bytes[i];
            if quote != 0 {
                if c == b'\\' {
                    i += 2;
                    continue;
                }
                if c == quote {
                    quote = 0;
                }
            } else if c == b'"' || c == b'\'' || c == b'`' {
                quote = c;
            } else if c == b'{' {
                depth += 1;
            } else if c == b'}' {
                depth -= 1;
                if depth == 0 {
                    return Some(&src[open..i + 1]);
                }
            }
            i += 1;
        }
        None
    }

    /// `ui/index.html` 里声明在 `<input>` / `<select>` / `<textarea>` 上的 id（单个标签，不跨 `>`）。
    fn form_control_ids(html: &str) -> std::collections::BTreeSet<String> {
        let mut out = std::collections::BTreeSet::new();
        for tag in ["<input", "<select", "<textarea"] {
            let mut from = 0;
            while let Some(at) = html[from..].find(tag) {
                let start = from + at;
                let end = start + html[start..].find('>').map(|e| e + 1).unwrap_or(0);
                let element = &html[start..end];
                if let Some(id) = element
                    .split("id=\"")
                    .nth(1)
                    .and_then(|r| r.split('"').next())
                {
                    out.insert(id.to_string());
                }
                from = end.max(start + 1);
            }
        }
        out
    }

    /// `at` 处是一个调用（形如 `.addEventListener(`），返回其**实参列表**（含括号），
    /// 括号配平并跳过字符串 / 模板字面量里的括号。
    fn balanced_call_args(src: &str, at: usize) -> Option<&str> {
        let open = at + src[at..].find('(')?;
        let bytes = src.as_bytes();
        let (mut depth, mut i, mut quote) = (0i32, open, 0u8);
        while i < bytes.len() {
            let c = bytes[i];
            if quote != 0 {
                if c == b'\\' {
                    i += 2;
                    continue;
                }
                if c == quote {
                    quote = 0;
                }
            } else if c == b'"' || c == b'\'' || c == b'`' {
                quote = c;
            } else if c == b'(' {
                depth += 1;
            } else if c == b')' {
                depth -= 1;
                if depth == 0 {
                    return Some(&src[open..i + 1]);
                }
            }
            i += 1;
        }
        None
    }

    /// 让 Enter 去**点确认按钮**（`.click(`）的 `keydown` 登记，若其目标是某个表单控件（`controls`），
    /// 就是「逐字段登记」的形态 —— 返回这些目标 id。
    ///
    /// 只认「点了按钮」的登记：`#chat-input` 的 Enter 是「发送消息」（调 `sendChat()`），
    /// 不是表单提交，不得误判；容器级委托的目标是函数参数（`card`），不是 `$("#id")`，也不进这个集合。
    fn enter_click_registrations_on_controls(
        src: &str,
        controls: &std::collections::BTreeSet<String>,
    ) -> Vec<String> {
        let mut out = Vec::new();
        for (at, _) in src.match_indices(".addEventListener(\"keydown\"") {
            // 目标：登记表达式之前的那一段（`$("#id")` / `getElementById("id")` / `card`）
            let back = src[..at]
                .rfind([';', '{', '}', '\n'])
                .map(|i| i + 1)
                .unwrap_or(0);
            let target = src[back..at].trim();
            // 处理体：`addEventListener(` 的整个实参列表（配平括号，跳过字符串）。
            // 不能取「到第一个 `;` 为止」—— `(e) => { if (e.key === "Enter") { e.preventDefault(); …click(); } }`
            // 的第一处 `;` 出现在 `e.preventDefault()` 之后，会把 `.click(` 切在窗口外。
            let body = match balanced_call_args(src, at) {
                Some(b) => b,
                None => continue,
            };
            if !body.contains("\"Enter\"") || !body.contains(".click(") {
                continue;
            }
            let id = target
                .split("\"#")
                .nth(1)
                .and_then(|r| r.split('"').next())
                .or_else(|| {
                    target
                        .split("getElementById(\"")
                        .nth(1)
                        .and_then(|r| r.split('"').next())
                });
            if let Some(id) = id {
                if controls.contains(id) {
                    out.push(id.to_string());
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// 时间戳必须以**原始串**到达渲染器，格式化只能由 helper 做（C2126）。
    ///
    /// 后端用 `dao::utc_iso()` 统一序列化（`format!("{date}T{time}Z")`，见 `src/dao.rs`），
    /// 前端则有现成的本地化 helper（`fmtPrecise` / `timeCell` / `timeAgo` / `localMD`）。
    /// 但 `ui/js/app.js` 里三处消费者把这个串**自己切了**，于是同一把尺子上出现三种错法：
    ///
    /// - 管理员加额申请「已处理」行 `(r.created_at || "").slice(5, 16)` ⇒ 屏幕上真的印出
    ///   `09-13T16:30`：ISO 的 `T` 分隔符泄露进 UI，而且小时是 UTC 的；
    /// - 设置页 API Key「创建时间」`String(k.created_at || "").slice(0, 10)` ⇒ UTC 日，
    ///   东八区用户在当地 08:00 之前看到的是「昨天」；
    /// - 交易视图行 `time: (t.time || "").replace("T", " ").slice(0, 16)` ⇒ **在渲染器之前**
    ///   就把秒抹掉，而这一列（`timeCell(t.time, true)`）与 CSV 导出（`fmtPrecise`）的口径都是
    ///   `HH:MM:SS` ⇒ 屏幕上的秒数永远是伪造的 `00`。C2111 把「导出 = 单元格」统一之后，
    ///   两份口径同源，源头的截断就成了口径本身。
    ///
    /// CI 里没有 JS 运行器，所以这里只钉**形状**（运行期那一半 —— 屏幕/CSV 上到底印出什么 ——
    /// 由 jsdom 仪器与 `ui/README.md` 的「时间戳」小节承接）：
    /// 线上字段不得被原地加工（**加工 helper 的输出可以**，见负/阳性对照）；
    /// 交易视图行的 `time` 属性必须是裸值。
    #[test]
    fn wire_timestamps_reach_the_renderer_unsliced() {
        let app = strip_js_comments(APP_JS);
        assert!(
            app.contains("function txsToView("),
            "ui/js/app.js 没读到（提取器的输入为空）"
        );

        // ① 线上时间戳字段不得被 `.slice()` / `.replace()` 原地加工。
        let found = inline_timestamp_processing(&app);
        assert!(
            found.is_empty(),
            "ui/js/app.js 有 {} 处原地加工线上时间戳：{found:?} —— 服务端的时间戳是 \
             `dao::utc_iso()` 的 `YYYY-MM-DDTHH:MM:SSZ`，自己切会同时泄露 ISO 的 `T` \
             （屏幕上真的出现 `09-13T16:30`）并按 UTC 显示；整串交给 `fmtPrecise` / `timeCell` \
             之后要截断也截它们的输出",
            found.len()
        );

        // ② 交易视图行必须把服务端串**原样**带出去（`time` 太泛，不进 ① 的字段集，按位置钉）。
        let body = js_function_body(&app, "function txsToView(")
            .expect("ui/js/app.js 里找不到 txsToView 的函数体");
        assert_eq!(
            body.matches("time:").count(),
            1,
            "txsToView 里 `time:` 不再唯一，本断言按位置取属性 —— 请同步更新本测试"
        );
        let at = body.find("time:").unwrap() + "time:".len();
        let rhs = &body[at..];
        let rhs = rhs[..rhs.find(',').unwrap_or(rhs.len())].trim();
        assert!(
            !rhs.contains('('),
            "交易视图行的 `time` 不再是裸值（{rhs:?}）：渲染器（`timeCell(..., true)` 与 CSV 导出）\
             拿到的必须是库内原串。在这里先把 `YYYY-MM-DDTHH:MM:SSZ` 切成 `YYYY-MM-DD HH:MM` \
             会把秒抹掉，而两处渲染的口径都是 `HH:MM:SS` ⇒ 屏幕上的秒永远是伪造的 `00`"
        );

        // ③ 阴性对照：提取器必须对**修前**的两种形态给出相反答案（否则 ① 只是恒真的形状巧合）。
        for (before, what) in [
            (
                "esc((r.created_at || \"\").slice(5, 16))",
                "管理员加额申请「已处理」单元格",
            ),
            (
                "created: String(k.created_at || \"\").slice(0, 10),",
                "API Key「创建时间」单元格",
            ),
        ] {
            assert_eq!(
                inline_timestamp_processing(before).len(),
                1,
                "阴性对照失败：提取器认不出修前形态（{what}）—— ① 等于没写"
            );
        }
        //    阳性对照：同一种「切」落在 helper 输出上必须被放过 —— 要截断就截 helper 的结果。
        for (after, what) in [
            (
                "created: fmtPrecise(k.created_at).slice(0, 10),",
                "取本地日期",
            ),
            ("last: k.last_used || null,", "原样透传（渲染器再格式化）"),
        ] {
            assert!(
                inline_timestamp_processing(after).is_empty(),
                "阳性对照失败：提取器把合法形态判成违规（{what}：{after}）"
            );
        }
    }

    /// 行内卡片的 Enter 提交必须**委托到容器**，不得逐字段登记（C2127）。
    ///
    /// 这些卡片是 `<div class="form">` / `<span class="inline-edit">` 而非真 `<form>`
    /// （真表单见 `#share-form`：`type=submit` 按钮让浏览器自己实现隐式提交，全字段免费），
    /// 所以 Enter 得自己实现。逐字段 `$("#某字段").addEventListener("keydown", …)` 把
    /// 「哪些控件能提交」抄成一份**名册**，而名册不会随控件增长：模型表单曾有 10 个可输入
    /// 控件、只登记了 2 个（厂商 / 模型名），输入价 / 缓存命中价 / 输出价 / 高峰三价 /
    /// 上下文窗口 / 最大输出这 8 个按 Enter **毫无反应**（点「确认」都能提交）；同一页的
    /// 部门表单却每个字段都登记了（2/2）⇒ 是漏登记，不是取舍。
    ///
    /// CI 里没有 JS 运行器，运行期那一半（按 Enter 到底有没有发出与点确认**相同**的请求）
    /// 由 jsdom 仪器与 `ui/README.md` 的「行内卡片的 Enter 提交」小节承接。这里钉形状。
    #[test]
    fn enter_submit_is_delegated_to_the_card() {
        let app = strip_js_comments(APP_JS);
        let controls = form_control_ids(INDEX_HTML);
        assert!(
            controls.contains("model-form-in") && controls.len() > 20,
            "ui/index.html 没读到表单控件（提取器输入为空）：{}",
            controls.len()
        );

        // ① 不变量（派生自 markup，而非人写名册）：任何「Enter → 点确认按钮」的登记都不得
        //    挂在 `index.html` 里声明于 input/select/textarea 的 id 上。
        let bad = enter_click_registrations_on_controls(&app, &controls);
        assert!(
            bad.is_empty(),
            "ui/js/app.js 有 {} 处逐字段登记的 Enter 提交：{bad:?} —— 卡片是 `<div class=\"form\">`，\
             没有隐式提交，但「哪些控件能提交」不该靠人写名册（模型表单 10 个可输入控件曾只登记 2 个）。\
             改成容器级委托 `wireEnterSubmit($(\"#卡片\"), \"#确认按钮\")`：挂冒泡，卡片里当前和以后的\
             文本控件都自动生效",
            bad.len()
        );

        // ② 阴性对照：提取器必须认得出修前的两种形态（否则 ① 只是恒真的形状巧合）。
        let mut synthetic = std::collections::BTreeSet::new();
        for id in [
            "model-form-in",
            "model-form-provider",
            "dept-form-quota",
            "chat-input",
        ] {
            synthetic.insert(id.to_string());
        }
        for (before, what) in [
            (
                "$(\"#model-form-in\").addEventListener(\"keydown\", (e) => { if (e.key === \"Enter\") { e.preventDefault(); $(\"#model-confirm\").click(); } });",
                "模型表单「输入价」字段",
            ),
            (
                "$(\"#dept-form-quota\").addEventListener(\"keydown\", (e) => { if (e.key === \"Enter\") { e.preventDefault(); $(\"#dept-confirm\").click(); } });",
                "部门表单「配额」字段",
            ),
        ] {
            assert_eq!(
                enter_click_registrations_on_controls(before, &synthetic).len(),
                1,
                "阴性对照失败：提取器认不出逐字段登记（{what}）—— ① 等于没写"
            );
        }
        //    阳性对照：同样挂在控件上、但**不点按钮**的 Enter（聊天输入框 = 发送消息）不得误判；
        //    容器级委托的目标是函数参数 `card`，也不得误判。
        for (after, what) in [
            (
                "$(\"#chat-input\").addEventListener(\"keydown\", (e) => { if (e.key === \"Enter\") sendChat(); });",
                "聊天输入框（发送消息，不是表单提交）",
            ),
            (
                "card.addEventListener(\"keydown\", (e) => { if (e.key !== \"Enter\") return; btn.click(); });",
                "容器级委托本身",
            ),
        ] {
            assert!(
                enter_click_registrations_on_controls(after, &synthetic).is_empty(),
                "阳性对照失败：提取器把合法形态判成违规（{what}：{after}）"
            );
        }

        // ③ 委托的加载器本身：登记在**容器参数**上，并早退非文本输入（否则焦点在「取消」上
        //    按 Enter 会同时触发取消与提交）。
        let body = js_function_body(&app, "function wireEnterSubmit(")
            .expect("ui/js/app.js 里找不到 wireEnterSubmit 的函数体");
        assert!(
            body.contains("card.addEventListener(\"keydown\""),
            "wireEnterSubmit 不再把 keydown 挂在**容器**上：{body}"
        );
        assert!(
            body.contains("tagName !== \"INPUT\"") && body.contains("NON_TEXT_INPUT_TYPES"),
            "wireEnterSubmit 不再早退非文本输入控件：勾选框 / 下拉 / 按钮上的 Enter 不该提交，\
             而且早退 `button` 才能避免「在『取消』上按 Enter 同时触发取消与提交」"
        );
        assert!(
            body.contains("btn.click()"),
            "wireEnterSubmit 不再点确认按钮：忙碌态（withLoading）、字段校验与请求都留在确认按钮\
             自己的监听器里，委托不得复制一份"
        );

        // ④ 五张行内卡片都必须走这个 helper。这一条是**卡片的**名册（5 个、每个都是有意的动作），
        //    不是**字段的**名册（模型表单里 10 个字段就是这么长出来的）；数量断言让「新增卡片却
        //    另写一套」必须先改这里，而字段级遗漏已由 ① 兜住。
        let mut calls = 0;
        for card in [
            "topup-card",
            "raise-card",
            "ak-new-inline",
            "dept-form-card",
            "model-form-card",
        ] {
            let call = format!("wireEnterSubmit($(\"#{card}\")");
            assert!(
                app.contains(&call),
                "行内卡片 #{card} 没有走 wireEnterSubmit —— 它的 Enter 提交要么缺失、要么又回到了逐字段登记"
            );
            calls += app.match_indices(&call).count();
        }
        assert_eq!(
            calls,
            app.matches("wireEnterSubmit($(\"#").count(),
            "wireEnterSubmit 的调用数与卡片数不一致：新增卡片请同步 ④ 的清单（并确认它是真 <form> \
             还是需要委托）"
        );
    }

    /* ---- C2133：后端不得把中文写进响应的**数据**字段 ---- */

    /// 已裁定的豁免项：`(字面量, 理由)`。
    ///
    /// 必须与提取结果**等价**（`==`，不是 `⊆`）：两侧都有牙 —— 新增一处「后端自造的中文数据
    /// 文案」会红；豁免的那个字面量消失（改掉或删掉）也红，清单不会腐烂。
    ///
    /// 为什么不能要求「一条都没有」：**用户数据**里的中文是合法的（`db.rs` 种子里作为账号名的
    /// `'管理员'`、用户自己填的部门名），它们不是后端自造的显示标签。本清单只收「后端**自己编**
    /// 了一句给用户看的话，塞进数据字段」这一种。
    const DATA_LABEL_EXEMPTIONS: &[(&str, &str)] = &[(
        "用户",
        "注册接口 `\"name\": name` 的默认用户名 —— 那是**用户数据**的默认值（同账号名），不是自造\
         的显示标签；且 `email.split('@').next()` 恒为 `Some`，该默认值不可达",
    )];

    /// 剥掉 Rust 注释，保留字符串字面量（`//` 与 `/* */` 在字面量内不生效）。
    fn strip_rust_comments(src: &str) -> String {
        let b = src.as_bytes();
        let mut out = String::with_capacity(src.len());
        let mut i = 0usize;
        while i < b.len() {
            if b[i] == b'"' {
                if let Some((_, end)) = read_rs_string(src, i) {
                    out.push_str(&src[i..end]);
                    i = end;
                    continue;
                }
            }
            if b[i] == b'/' && b.get(i + 1) == Some(&b'/') {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(b.len());
                continue;
            }
            let ch = src[i..].chars().next().expect("i 在字符边界上");
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    /// 只读第一个 `#[cfg(test)]` 之前的内容：测试里的中文不上线。
    fn cut_rust_test(src: &str) -> &str {
        match src.find("#[cfg(test)]") {
            Some(i) => &src[..i],
            None => src,
        }
    }

    /// `src[i] == '"'` → `(字面量内容, 闭合引号之后的下标)`。
    fn read_rs_string(src: &str, i: usize) -> Option<(String, usize)> {
        if src.as_bytes().get(i) != Some(&b'"') {
            return None;
        }
        let b = src.as_bytes();
        let mut j = i + 1;
        let mut out = String::new();
        while j < b.len() {
            if b[j] == b'\\' {
                if j + 2 > b.len() {
                    return None;
                }
                out.push_str(&src[j..j + 2]);
                j += 2;
                continue;
            }
            if b[j] == b'"' {
                return Some((out, j + 1));
            }
            let ch = src[j..].chars().next()?;
            out.push(ch);
            j += ch.len_utf8();
        }
        None
    }

    /// 与 `src[at]` 处的开定界符配对的闭定界符**之后**的下标（跳过字符串）。
    fn match_rs_delim(src: &str, at: usize, open: u8, close: u8) -> Option<usize> {
        let b = src.as_bytes();
        let (mut depth, mut i) = (0i32, at);
        while i < b.len() {
            if b[i] == b'"' {
                i = read_rs_string(src, i)?.1;
                continue;
            }
            if b[i] == open {
                depth += 1;
            } else if b[i] == close {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            i += 1;
        }
        None
    }

    /// 包含 `at` 的**最内层** `fn` 体（绝对区间）。
    ///
    /// 作用域是必须的：`mod.rs` 的 `me()` 用元组解构拿 `name`（真数据），而 `register()` 里
    /// 另有一个 `let name = … "用户" …`。没有作用域限制，`me()` 就会被误判成泄漏。
    fn enclosing_fn_span(src: &str, at: usize) -> Option<(usize, usize)> {
        let b = src.as_bytes();
        let mut best: Option<(usize, usize)> = None;
        let mut from = 0usize;
        while let Some(rel) = src[from..].find("fn ") {
            let start = from + rel;
            if start >= at {
                break;
            }
            from = start + 3;
            let mut k = start + 3;
            while k < b.len() && (b[k].is_ascii_alphanumeric() || b[k] == b'_') {
                k += 1;
            }
            if k == start + 3 {
                continue; // `fn(` 是函数指针类型，没有名字
            }
            while k < b.len() && b[k].is_ascii_whitespace() {
                k += 1;
            }
            if b.get(k) != Some(&b'(') && b.get(k) != Some(&b'<') {
                continue;
            }
            let Some(brace) = src[k..].find('{').map(|o| o + k) else {
                continue;
            };
            if brace > at {
                continue;
            }
            if let Some(end) = match_rs_delim(src, brace, b'{', b'}') {
                if end > at {
                    best = Some((start, end));
                }
            }
        }
        best
    }

    /// 跳过嵌套 `json!(...)`：那些区域由外层循环自己扫，别在绑定 RHS 里重复计入
    ///（否则 `let user_id = { … json!({ "error": "…" }) … }` 会把**错误文案**算成数据文案）。
    fn strip_json_macros(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut i = 0usize;
        while i < text.len() {
            if text[i..].starts_with("json!") {
                let after = i + "json!".len();
                if let Some(open) = text[after..].find('(').map(|o| o + after) {
                    if let Some(end) = match_rs_delim(text, open, b'(', b')') {
                        i = end;
                        continue;
                    }
                }
            }
            let ch = text[i..].chars().next().expect("i 在字符边界上");
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    /// 同函数内 `at` 之前**最近**的 `let <ident> = <expr>` 的右值（已剥掉嵌套 `json!`）。
    fn local_binding_rhs(src: &str, ident: &str, at: usize) -> Option<String> {
        let (fstart, fend) = enclosing_fn_span(src, at)?;
        let b = src.as_bytes();
        let mut chosen: Option<usize> = None;
        let mut from = fstart;
        while let Some(rel) = src[from..fend].find("let ") {
            let abs = from + rel;
            from = abs + 4;
            let mut k = abs + 4;
            let id_start = k;
            while k < fend && (b[k].is_ascii_alphanumeric() || b[k] == b'_') {
                k += 1;
            }
            if &src[id_start..k] != ident {
                continue;
            }
            while k < fend && b[k].is_ascii_whitespace() {
                k += 1;
            }
            if b.get(k) == Some(&b':') && b.get(k + 1) == Some(&b'=') {
                k += 2;
            } else if b.get(k) == Some(&b'=') {
                k += 1;
            } else {
                continue;
            }
            if abs >= at {
                break;
            }
            chosen = Some(k);
        }
        let start = chosen?;
        let (mut depth, mut i) = (0i32, start);
        while i < b.len() {
            if b[i] == b'"' {
                i = read_rs_string(src, i)?.1;
                continue;
            }
            match b[i] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                b';' if depth == 0 => break,
                _ => {}
            }
            i += 1;
        }
        Some(strip_json_macros(&src[start..i]))
    }

    /// 文本里全部含 CJK 的字面量。
    fn cjk_literals(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < text.len() {
            if text.as_bytes()[i] == b'"' {
                if let Some((lit, end)) = read_rs_string(text, i) {
                    if lit.chars().any(is_cjk) {
                        out.push(lit);
                    }
                    i = end;
                    continue;
                }
            }
            i += 1;
        }
        out
    }

    /// `j` 处的值表达式原文（到顶层 `,` / `}` 为止）；字符串原样保留（含引号）。
    fn json_value_expression(region: &str, j: usize) -> String {
        let b = region.as_bytes();
        let (mut depth, mut i) = (0i32, j);
        let mut out = String::new();
        while i < b.len() {
            if b[i] == b'"' {
                match read_rs_string(region, i) {
                    Some((_, end)) => {
                        out.push_str(&region[i..end]);
                        i = end;
                        continue;
                    }
                    None => break,
                }
            }
            match b[i] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                b',' if depth == 0 => break,
                _ => {}
            }
            let ch = region[i..].chars().next().expect("i 在字符边界上");
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    /// 以 `json!` **数据**字段（key ≠ `error`）交付的中文字面量，返回 `(字面量, 位置)`。
    ///
    /// 三条形态规则，缺一条就会漏掉本轴的一半：
    /// 1. 直接写字面量：`json!({ "name": "中文" })`；
    /// 2. 表达式里的字面量：`json!({ "name": format!("中文 {x}") })`；
    /// 3. **穿透本地绑定**：`let name = … "中文" …; json!({ "name": name })` ——
    ///    计划名的自造标签正是这种写法（`match p.type_ { "paygo" => "API（按量）" }`），
    ///    不穿透就看不见它，门禁会在**改前就是绿的**。
    fn data_field_cjk_literals(file: &str, src: &str) -> Vec<(String, String)> {
        let stripped = strip_rust_comments(src);
        let src = cut_rust_test(&stripped);
        let mut out = Vec::new();
        let mut from = 0usize;
        while let Some(rel) = src[from..].find("json!") {
            let at = from + rel;
            from = at + "json!".len();
            let Some(open) = src[from..].find('(').map(|o| o + from) else {
                continue;
            };
            let Some(end) = match_rs_delim(src, open, b'(', b')') else {
                continue;
            };
            let region = &src[open + 1..end];
            let mut i = 0usize;
            while i < region.len() {
                if region.as_bytes()[i] != b'"' {
                    i += 1;
                    continue;
                }
                let Some((key, after)) = read_rs_string(region, i) else {
                    break;
                };
                let mut j = after;
                while j < region.len() && region.as_bytes()[j].is_ascii_whitespace() {
                    j += 1;
                }
                if region.as_bytes().get(j) != Some(&b':') {
                    i = after;
                    continue;
                }
                j += 1; // 跳过冒号本身 —— 漏掉这一步会把**键**当成值表达式，整条扫描静默失效
                while j < region.len() && region.as_bytes()[j].is_ascii_whitespace() {
                    j += 1;
                }
                let line = src[..open + 1 + i].matches('\n').count() + 1;
                i = after;
                if key == "error" {
                    continue;
                }
                if region.as_bytes().get(j) == Some(&b'"') {
                    if let Some((lit, _)) = read_rs_string(region, j) {
                        if lit.chars().any(is_cjk) {
                            out.push((lit, format!("{file}:{line} 字段 `{key}`")));
                        }
                    }
                    continue;
                }
                let ident_end = {
                    let mut k = j;
                    while k < region.len()
                        && (region.as_bytes()[k].is_ascii_alphanumeric()
                            || region.as_bytes()[k] == b'_')
                    {
                        k += 1;
                    }
                    k
                };
                let next = region.as_bytes().get(ident_end).copied();
                let is_binding = ident_end > j
                    && !region.as_bytes()[j].is_ascii_digit()
                    && next != Some(b'(')
                    && next != Some(b'!');
                if is_binding {
                    let ident = &region[j..ident_end];
                    if let Some(rhs) = local_binding_rhs(src, ident, open + 1 + j) {
                        for lit in cjk_literals(&rhs) {
                            out.push((
                                lit,
                                format!("{file}:{line} 字段 `{key}`（经由 `let {ident}`）"),
                            ));
                        }
                        continue;
                    }
                }
                for lit in cjk_literals(&json_value_expression(region, j)) {
                    out.push((lit, format!("{file}:{line} 字段 `{key}`")));
                }
            }
        }
        out
    }

    /// `src/` 下的全部 `.rs`（递归）：名册由文件系统派生，新增子目录不会静默逃逸。
    fn rust_sources_under_src(root: &str) -> Vec<String> {
        let mut stack = vec!["src".to_string()];
        let mut out = Vec::new();
        while let Some(dir) = stack.pop() {
            let entries = std::fs::read_dir(format!("{root}/{dir}"))
                .unwrap_or_else(|_| panic!("应能读取 {dir}/"));
            for e in entries.filter_map(|e| e.ok()) {
                let rel = format!("{dir}/{}", e.file_name().to_string_lossy());
                if e.path().is_dir() {
                    stack.push(rel);
                } else if rel.ends_with(".rs") {
                    out.push(rel);
                }
            }
        }
        out.sort();
        out
    }

    /// 不变量：**后端不得自造中文显示文案塞进响应的数据字段**（C2133）。
    ///
    /// 咽喉 `api.js` 只把 `error` 字段交给 `mapErr`（词表见 `every_backend_error_message_
    /// reaches_the_wordlist`），数据字段是前端**原样渲染**的 ⇒ 后端在数据字段里放一句中文，
    /// `en` 界面上就是中文，而 `cargo test` 全绿。修前实测两处可达（`en` 语言包下，jsdom 启真
    /// `index.html` + 四脚本）：① `GET /api/admin/usage` 的部门桶名 `（未分配）` → `#usage-dept`；
    /// ② `GET /api/plans` 的 plan 兜底名 `API（按量）`（config 的 `[[plans]]` 全都不写 `name`
    /// ⇒ 恒触发）→ `#sf-plan` 与上架 toast。
    ///
    /// 断言形态是**名册等价**而不是「一条都没有」：用户数据里的中文是合法的（账号名、用户自己
    /// 填的部门名）。判据是「后端**自己编**了一句给用户看的话」—— 那种话归语言包。
    #[test]
    fn backend_data_fields_are_language_neutral() {
        let root = env!("CARGO_MANIFEST_DIR");
        let mut flagged: BTreeMap<String, String> = BTreeMap::new();
        for rel in rust_sources_under_src(root) {
            let src = std::fs::read_to_string(format!("{root}/{rel}"))
                .unwrap_or_else(|_| panic!("应能读取 {rel}"));
            for (lit, site) in data_field_cjk_literals(&rel, &src) {
                flagged.insert(lit, site);
            }
        }

        let got: Vec<String> = flagged.keys().cloned().collect();
        let mut want: Vec<String> = DATA_LABEL_EXEMPTIONS
            .iter()
            .map(|(l, _)| l.to_string())
            .collect();
        want.sort();
        want.dedup();

        assert_eq!(
            got, want,
            "响应数据字段里的中文字面量与已裁定清单不一致 ——\n\
             左＝实际提取到的（提取器与后端源码无关，它会随源码变），右＝裁定清单。\n\
             多出来的：这不是错误文案（`error` 字段有 ERR_MAP 兜底），`en` 界面会原样显示中文；\
             改法是让后端回传 config / 库里的**原值**或语言中性标记，把显示文案搬到客户端语言包。\n\
             少掉的：清单在腐烂 —— 删掉那个字面量时请一并删掉它的豁免条目。\n\
             实测：{flagged:#?}\n\
             豁免清单：{DATA_LABEL_EXEMPTIONS:#?}"
        );

        // 提取器自证：不能是靠「什么都没扫到」通过的（清单非空 ⇒ 这一条同时是阳性对照）
        assert!(
            !DATA_LABEL_EXEMPTIONS.is_empty(),
            "豁免清单为空时上面的等号会退化成「后端一条中文数据文案都没有」，\
             请确认那是有意为之，而不是提取器失真"
        );
        // 名册是**日落清单**，不是注册表：这个类里正确的修法是「后端回传语言中性标记、显示文案
        // 归客户端语言包」，**不是**「把自造的中文登记进来」。所以名额刻意只有 1 条，扩容必须
        // 显式改这个数 —— 加之前先回答：这句话能不能由语言包说？能，就别加。
        //（竞争修法腿就是这么被抓的：保留后端自造名 + 往清单里塞一条，等号那一半会放行，这一半不会。）
        assert_eq!(
            DATA_LABEL_EXEMPTIONS.len(),
            1,
            "豁免清单只收**用户数据的默认值**（不是自造标签）；确需扩容请同时改这个数 —— 故意的减速带"
        );
    }

    /// 阴性/阳性对照：把「漏」与「误收」两种失真都注入合成输入，证明上面那条断言有牙齿。
    #[test]
    fn data_field_scanner_detects_injected_labels() {
        let sample = r#"
fn f(cfg: &Plan) {
    json!({ "name": "中文甲" });
    json!({ "error": "中文乙" });
    json!({ "nested": { "label": "中文丙" } });
    let name = if cfg.name.is_empty() { "中文丁".to_string() } else { cfg.name.clone() };
    json!({ "name": name });
    let user_id = { if bad() { return Err(json!({ "error": "中文戊" })); } 5 };
    json!({ "id": user_id });
    json!({ "note": format!("中文己 {x}") });
    let decoy = "中文庚";
}
fn g(row: (String, String, String)) {
    let (email, name, role) = row;
    json!({ "name": name });
}
#[cfg(test)]
mod tests { fn t() { json!({ "x": "测试中文" }) } }
"#;
        let got: Vec<String> = data_field_cjk_literals("sample.rs", sample)
            .into_iter()
            .map(|(lit, _)| lit)
            .collect();

        // 注意 `中文己 {x}` 比对的是字面量的**原文**（含占位符）：提取器交出来的就是源码里的
        // 那个串，门禁的豁免清单也按原文记账 —— 换成「已格式化的样子」两侧就永远对不上。
        for want in ["中文甲", "中文丙", "中文丁", "中文己 {x}"] {
            assert!(
                got.iter().any(|g| g == want),
                "提取器漏掉 {want:?} —— 漏掉一种形态就等于把那一半的类放行，实得 {got:?}"
            );
        }
        for unwanted in [
            "中文乙",   // `error` 字段：由 ERR_MAP 那条门禁负责
            "中文戊",   // 绑定 RHS 里**嵌套** json! 的错误文案，不得算作数据文案
            "中文庚",   // 与 json! 无关的局部变量
            "测试中文", // `#[cfg(test)]` 之后
        ] {
            assert!(
                !got.iter().any(|g| g == unwanted),
                "{unwanted:?} 不该被算作响应数据字段文案，实得 {got:?}"
            );
        }
        // 元组解构绑定的是**真数据**（用户/部门的实际名字）：必须按**函数作用域**解析绑定，
        // 否则 `g()` 里那个 `name` 会解析到 `f()` 里更早的 `let name`，把 中文丁 数第二遍。
        assert_eq!(
            got.iter().filter(|g| *g == "中文丁").count(),
            1,
            "绑定解析必须限定在**同一个函数**内，实得 {got:?}"
        );
    }

    /// 「标签来自**语言包感知**的解析器」的判别式（C2157）。
    ///
    /// 判别式必须覆盖**全部**解析器，不能锚在单个函数名上：`planLabelById(id)` 是同一个解析器
    /// 的第二个入口（按 plan id 进来），单锚 `planLabel(` 会把它合法的调用判成红 —— 同族坑
    /// #347「名字不是唯一载体」。⚠️ 射程：它证明派生**走了**解析器，不证明分支/算术全对。
    fn resolver_renders_the_label(stmt: &str) -> bool {
        stmt.contains("planLabel(") || stmt.contains("planLabelById(")
    }

    /// 判别式自证：两个合法入口各判一次、一个裸字段形态必须判负（否则放宽是无牙的）。
    #[test]
    fn the_plan_label_resolver_discriminant_covers_every_resolver() {
        assert!(resolver_renders_the_label(
            "const label = provLabel(plan.provider) + \" · \" + planLabel(plan);"
        ));
        assert!(resolver_renders_the_label(
            "const label = provLabel(plan.provider) + \" · \" + planLabelById(planId);"
        ));
        assert!(!resolver_renders_the_label(
            "const label = provLabel(plan.provider) + \" · \" + plan.name;"
        ));
    }

    /// 另一半（C2133）：后端只回传语言中性标记之后，标签必须由客户端补上。
    ///
    /// 只钉生产者（后端无中文）会漏掉「前端把空串直接渲染成空白标签」这条半修；
    /// 只钉消费者则会漏掉「后端继续自造中文」。两半各钉一个方向。
    #[test]
    fn backend_neutral_data_labels_are_localized_in_the_client() {
        let app = strip_js_comments(APP_JS);

        // ① 用量卡片：无部门桶（后端回空串）必须有语言包兜底
        // 注意：`#usage-dept` 在同一个函数里出现**两次**（先清空、后渲染）。按第一次命中切语句
        // 会拿到那句清空（里面根本没有 `barRow`）⇒ 门禁报出一条与产品无关的假红（坑 #286 家族）。
        let stmt = statement_containing_all(&app, &["$(\"#usage-dept\").innerHTML", "barRow("])
            .expect("找不到 #usage-dept 经 barRow 渲染的那条语句");
        let arg = first_call_arg(stmt, "barRow(").expect("部门条应经 barRow 渲染");
        assert!(
            arg.contains("d.name"),
            "部门条的首参应是该行的部门名，实得 {arg:?}"
        );
        assert!(
            arg.contains("T("),
            "无部门桶的标签必须由语言包提供（`d.name || T(\"common.unassigned\")`）——\
             后端已经不再自造它了，前端不兜底就只剩一个空标签：{arg:?}"
        );

        // ② Plan 显示名：一个解析器、四个渲染点（C2157 补上共享表单元格与仪表盘卡两处）
        let body = js_function_body(&app, "function planLabel(")
            .expect("应有 planLabel（config 没写 name 时按 type 取语言包）");
        assert!(
            body.contains("pl.name") && body.contains("T(\"share.planName."),
            "planLabel 必须先看 config 原名、再按 type 取语言包，实得 {body:?}"
        );
        for (site, needle) in [
            ("上架表单的 Plan 下拉", "selPlan.innerHTML"),
            ("上架成功的 toast", "const label = provLabel(plan.provider)"),
        ] {
            let stmt = statement_containing(&app, needle)
                .unwrap_or_else(|| panic!("找不到 {site} 的渲染语句（`{needle}`）"));
            assert!(
                resolver_renders_the_label(stmt),
                "{site} 必须经**语言包感知**的解析器渲染（`planLabel(` 或 `planLabelById(`）——\
                 锚在单个函数名上会把合法的第二个入口判成红；直接读 `plan.name` 则会让 config \
                 未配置时显示空标签：{stmt:?}"
            );
        }
        // C2157：共享表单元格与仪表盘卡渲染的是 `sharingsToView` **派生出来的标签**，
        // 不得再内联 `s.plan || "API"` —— 那是**配置 id**，与同屏的下拉/toast 是两个口径。
        for (site, needle) in [
            ("共享表 Plan 单元格", "esc(provLabel(s.provider))"),
            ("仪表盘「我的共享」卡", "$(\"#dash-sharings\").innerHTML"),
        ] {
            let stmt = statement_containing(&app, needle)
                .unwrap_or_else(|| panic!("找不到 {site} 的渲染语句（`{needle}`）"));
            assert!(
                stmt.contains("esc(s.plan)"),
                "{site} 应渲染派生值 `esc(s.plan)`（由 sharingsToView 经 planLabelById 算出），\
                 实得 {stmt:?}"
            );
        }
        assert!(
            !app.contains("esc(s.plan ||"),
            "共享行又回到「裸配置 id 兜底」的渲染形态（`esc(s.plan || \"API\")`）——\
             同屏上会出现 id 与标签两个口径"
        );
        let derived = js_function_body(&app, "function sharingsToView(")
            .expect("应有 sharingsToView（共享行的视图层）");
        assert!(
            derived.contains("planLabelById("),
            "共享行的 `plan:` 必须经 `planLabelById` 派生（标签的唯一产出点），实得 {derived:?}"
        );
        // `data.js` 兜底表 `D.PLANS[].name` 是**中文硬编码**（C2133 ⛔ 未修）⇒ 标签路径不得读它，
        // 否则 `en` 界面会把中文名印出来。判据＝`planLabelById`（含它的兜底分支）不提 `name`。
        let by_id = js_function_body(&app, "function planLabelById(")
            .expect("应有 planLabelById（按 id 出标签）");
        assert!(
            !by_id.contains("name"),
            "`planLabelById` 不得读 `name`（兜底表的名字是中文硬编码）：兜底行只取语言中性的 type，\
             实得 {by_id:?}"
        );
        // 三个 type 的标签两包俱在 —— 它们是 `planLabel` 对 live plan（name 未配置）的出口。
        let LanguagePacks { zh, en, .. } = packs();
        for k in [
            "share.planName.paygo",
            "share.planName.token",
            "share.planName.coding",
        ] {
            assert!(
                zh.contains_key(k) && en.contains_key(k),
                "`{k}` 应在两个包里都有（planLabel 的出口，缺一个就会在一种语言下印出键名）"
            );
        }

        // ③ 兜底表不得赢过真实清单（C2133 实测的那条路）：Plan 下拉框的重建判据必须是**数据源**，
        //    而不是「建过没有」。一次性守卫在登录后首次渲染时就把兜底表 `D.PLANS` 定了型
        //    （`/api/plans` 那次请求还在路上），它回来后下拉框再也不重建 ⇒ `planLabel` 永远没机会
        //    生效，`en` 界面上显示的就是兜底表里的中文名 —— 光加上 planLabel 是**半修**。
        assert!(
            app.contains("selP.dataset.plansSrc"),
            "Plan 下拉框丢了「数据源变了才重建」的判据（应比对数据源快照，而不是一次性标志）"
        );
        assert!(
            !app.contains("selP.dataset.init"),
            "Plan 下拉框又回到「一次性初始化」守卫：兜底表会赢到底，真实清单回来后不再重建"
        );
    }

    /// `src` 中 `needle` 之后那个调用的**第一个实参**（括号配平，到顶层 `,` 为止）。
    fn first_call_arg<'a>(src: &'a str, needle: &str) -> Option<&'a str> {
        let at = src.find(needle)? + needle.len();
        let b = src.as_bytes();
        let (mut depth, mut i, mut end) = (0i32, at, None);
        while i < b.len() {
            if b[i] == b'"' || b[i] == b'\'' || b[i] == b'`' {
                let q = b[i];
                i += 1;
                while i < b.len() {
                    if b[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if b[i] == q {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            match b[i] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => {
                    if depth == 0 {
                        end = Some(i);
                        break;
                    }
                    depth -= 1;
                }
                b',' if depth == 0 => {
                    end = Some(i);
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        Some(&src[at..end?])
    }
    // ── C2158：运营卡「上游 key 健康」的判定语必须说**数据说的那件事** ──────────────────────────
    //
    // 数据只有**启用 / 停用**（`keys` 表没有 health/error 列；`/api/ops/runtime` 只回
    // `total`/`on`/`off`，`off = total − on` 由后端算）。把这份计数渲染成「健康 / N 个异常 /
    // 全部失败」= 给数据加戏：用户**暂停/下架自己的 key**（正常操作）会让运营者看到**红色
    // 「全部失败」**（C2158 侦察实测，jsdom 真控件复现）。
    //
    // 三条规则**各有独立的牙**（A/B 逐腿见 `c2158_gate_ab.py`）：
    //   ① 键名不得是判定词 —— **改文案不改键名仍红**（键名会撒谎就没法用门禁钉）
    //   ② 消费到的键必须已登记 —— 防「另起一个新判定语键」
    //   ③ 三个状态键必须**都被渲染** —— 防「把 pill 整块删掉」的逃逸
    // ＋ `the_ops_key_state_scanners_have_teeth`（合成输入自证两条判别式）。
    //
    // ⚠️ 射程：门禁是**词法**的 —— 它证明**键名与键集**，不证明渲染出来的**句子**与数据一致
    // （那一半由 jsdom 探针 `A1`–`A4`/`B1`/`C1` 承接）。已按 #341 写进 `ui/README.md`。

    /// 判定词（比对的是**键名**，不是文案）：`ops.keys.` 之后的第一段命中其一即为「用键名断言健康」。
    const OPS_KEY_VERDICT_WORDS: [&str; 7] = [
        "healthy",
        "abnormal",
        "failed",
        "fail",
        "error",
        "down",
        "unhealthy",
    ];

    /// 已登记的状态系键全集（C2158 之后）。消费者**只能**用这些。
    const OPS_KEY_REGISTERED: [&str; 5] = [
        "ops.keys.allOn",
        "ops.keys.someOff",
        "ops.keys.allOff",
        "ops.keys.count",
        "ops.keys.empty",
    ];

    /// 三个**状态**键：必须都被渲染（`count`/`empty` 不是状态判定语，不在其中）。
    const OPS_KEY_STATES: [&str; 3] = ["ops.keys.allOn", "ops.keys.someOff", "ops.keys.allOff"];

    /// `ops.keys.<head>[.…]` 的 `<head>`（小写）。不是 `ops.keys.*` ⇒ `None`。
    fn ops_key_head(key: &str) -> Option<String> {
        let rest = key.strip_prefix("ops.keys.")?;
        Some(rest.split('.').next().unwrap_or("").to_ascii_lowercase())
    }

    /// 键集里「以判定词命名」的那些（规则 ① 的判别式；纯函数 ⇒ 可被合成输入自证）。
    fn ops_key_verdict_keys<'a, I: IntoIterator<Item = &'a String>>(keys: I) -> Vec<String> {
        keys.into_iter()
            .filter(|k| {
                ops_key_head(k).is_some_and(|h| OPS_KEY_VERDICT_WORDS.contains(&h.as_str()))
            })
            .cloned()
            .collect()
    }

    /// 源码里**消费到**的 `ops.keys.*` 键名（调用方须先剥注释；键名取**整串 token**）。
    fn ops_key_consumed(src: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut from = 0usize;
        while let Some(rel) = src[from..].find("\"ops.keys.") {
            let at = from + rel + 1; // 指向 `o`
            let tail = &src[at..];
            let end = tail.find('"').unwrap_or(tail.len());
            let key = &tail[..end];
            if !key.is_empty() && key.chars().all(is_key_token_char) {
                out.insert(key.to_string());
            }
            from = at + key.len().max(1);
        }
        out
    }

    #[test]
    fn the_ops_key_health_pill_names_the_state_it_counts() {
        let LanguagePacks {
            zh_keys, en_keys, ..
        } = packs();
        let app = strip_js_comments(APP_JS);

        // 提取器阳性对照：扫不到键 ⇒ 后面几条断言全是空转。
        assert!(
            !zh_keys.is_empty() && !en_keys.is_empty(),
            "语言包键集为空 —— 提取器已失真，拒绝继续"
        );
        let consumed = ops_key_consumed(&app);
        assert!(
            !consumed.is_empty(),
            "在 ui/js/app.js 里一个 `ops.keys.*` 键都没扫到 —— 提取器已失真（键名或语料变了？）"
        );

        // ① 键名不得是判定词。
        let offenders = ops_key_verdict_keys(zh_keys.iter().chain(en_keys.iter()));
        assert!(
            offenders.is_empty(),
            "语言包里仍有以「判定词」命名的键：{offenders:?} —— 数据只有启用/停用，\
             用 healthy/failed 命名就是在宣称健康信息（C2158）"
        );

        // ② 消费到的键必须已登记。
        let unregistered: Vec<&String> = consumed
            .iter()
            .filter(|k| !OPS_KEY_REGISTERED.contains(&k.as_str()))
            .collect();
        assert!(
            unregistered.is_empty(),
            "运营卡的 key 状态块消费了**未登记**的键：{unregistered:?} —— 登记集是 \
             {OPS_KEY_REGISTERED:?}；新键必须先想清楚它说的是哪个状态"
        );

        // ③ 三个状态键必须都被渲染。
        let missing: Vec<&str> = OPS_KEY_STATES
            .iter()
            .copied()
            .filter(|k| !consumed.contains(*k))
            .collect();
        assert!(
            missing.is_empty(),
            "运营卡**少渲染**了状态键 {missing:?} —— 三态（全部启用 / N 个停用 / 全部停用）\
             必须都印出来：删掉 pill 不是修法（C2158）"
        );
    }

    #[test]
    fn the_ops_key_state_scanners_have_teeth() {
        // ① 判定词判别式：判定词命中，状态系键不得被绊倒。
        let synthetic: Vec<String> = [
            "ops.keys.healthy",
            "ops.keys.failed",
            "ops.keys.fail",
            "ops.keys.unhealthy",
            "ops.keys.allOn",
            "ops.keys.someOff",
            "ops.keys.allOff",
            "ops.keys.count",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        let got = ops_key_verdict_keys(synthetic.iter());
        assert_eq!(
            got.len(),
            4,
            "判定词判别式应恰命中 healthy/failed/fail/unhealthy 四个，实得 {got:?}"
        );
        assert!(
            !got.iter()
                .any(|k| k.contains("all") || k.contains("someOff")),
            "状态系键不得被判为判定词：{got:?}"
        );

        // ② 消费者扫描器：取**整串**键名，不取前缀；且只认 `"ops.keys.` 引号形态。
        let sample =
            "const a = T(\"ops.keys.allOn\"); const b = T(\"ops.keys.someOff\", { n: 1 });";
        let seen = ops_key_consumed(sample);
        assert_eq!(seen.len(), 2, "扫描器应取到两个键，实得 {seen:?}");
        assert!(seen.contains("ops.keys.allOn") && seen.contains("ops.keys.someOff"));
        let nested = ops_key_consumed("T(\"ops.keys.allOnExtra\")");
        assert_eq!(nested.len(), 1, "扫描器必须取整串键名而非前缀：{nested:?}");
        assert!(nested.contains("ops.keys.allOnExtra"), "{nested:?}");

        // ③ 剥注释是调用方的责任：注释里的键名**不得**被算作消费。
        let commented =
            strip_js_comments("// T(\"ops.keys.healthy\")\nconst x = T(\"ops.keys.allOn\");");
        assert_eq!(
            ops_key_consumed(&commented).len(),
            1,
            "剥注释后注释里的键名仍被算作消费 —— 正是 C2158 之前那个假「健康」的残留形态：\
             实得 {commented:?}"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // C2172 —— 市场工具栏计数必须用**它所数的那些行**的单位说话
    // ─────────────────────────────────────────────────────────────────────────

    /// 本仓给**单个上游 key** 起的名所在的键（`share.col.key`，值 `Key`）。
    ///
    /// 这是本门禁里**唯一**写下的键名，而且它是**角色**不是名册：要的是「App 称呼单个 key 的
    /// 那个词」，不是某条具体文案。为什么这把尺子取自**共享视图**的列头，而不是市场行的 pill：
    /// 计数与 pill 在**同一屏**，尺子若取自被怀疑的那个元素，判别式就**循环**了
    /// （#351 —— 探针第一版正是这么写，于是「把 pill 的 key 词删掉」这种化妆式修法会让轴腿
    /// 静默变绿）。规则⑤再把它钉回市场行的 pill 家族，所以这把尺子不能被悄悄换掉。
    const KEY_UNIT_LABEL_KEY: &str = "share.col.key";

    /// 市场行「可用性」pill 的键前缀 —— 规则⑤用它代替写死某一条 pill 键名。
    /// 问的是「市场行里有没有一条 pill 用『单个 key 的名』说话」，不是某条具体文案。
    const AVAIL_PILL_PREFIX: &str = "mk.avail.";

    /// `src` 里第一个 `attr="…"` 形态的**值**。
    fn first_attr_value(src: &str, attr: &str) -> Option<String> {
        let pat = format!("{attr}=\"");
        let i = src.find(&pat)?;
        let rest = &src[i + pat.len()..];
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    }

    /// 以 `<tbody id="body_id">` 为锚，取它所在表格 `<thead>` 里**第一个** `data-i18n` 键。
    ///
    /// **派生**而非名册：列头换键、加列、换表都跟着变；锚点用 `id`（表里唯一的稳定身份）。
    /// 先剥 HTML 注释（#296：注释里的表不算表）。
    fn table_head_key(html: &str, body_id: &str) -> Option<String> {
        let html = strip_html_comments(html);
        let anchor = format!("id=\"{body_id}\"");
        let body = html.find(&anchor)?;
        let table = html[..body].rfind("<table")?;
        let head_start = html[table..body].find("<thead")? + table;
        let head_end = html[head_start..body].find("</thead>")? + head_start;
        first_attr_value(&html[head_start..head_end], "data-i18n")
    }

    /// 所有写 `#mk-count` 的位点：`(位点数, 每个位点里 `T("…")` 的键)`。
    ///
    /// 键为 `None` ⇒ 该位点没有经 `T("字面量")` 取值（提取器读不到 ⇒ 响亮变红，
    /// 而不是静默少算一个位点）。⚠️ 调用方必须先剥 JS 注释（#296）。
    ///
    /// ⚠️ 三处收口，都是被 A/B 打出来的（#335 同族：判别式必须按**语句边界**收口；
    /// #348：新写判别式的第一遍输出首先是关于**仪器**的 claim）：
    /// ① 只看**同一条语句**（到 `;` 为止）里的 `T(` —— 否则「元素被赋值"4 个模型"」这种硬编码
    ///    位点会去读**后面某句无关的** `T("mk.collapse")`，把「没有包键」读成一个**错键**
    ///    （实测：m_hardcoded 腿报 `Some("mk.collapse")` 而不是 `None`）；
    /// ② 该语句必须是在写这个元素的**内容**（`textContent` / `innerHTML`）；
    /// ③ `T(` 与键字面量之间**允许换行/空白** —— `T(\n  "key"` 是合法写法，第一版用
    ///    `find("T(\"")` 直接找 `T("`，跨行写法的位点被读成 `None`（实测：合成腿报 `(1, [None])`）。
    fn mk_count_fill_keys(app: &str) -> (usize, Vec<Option<String>>) {
        let needle = "\"#mk-count\"";
        let mut sites = 0usize;
        let mut keys: Vec<Option<String>> = Vec::new();
        let mut from = 0usize;
        while let Some(rel) = app[from..].find(needle) {
            sites += 1;
            from += rel + needle.len();
            let rest = &app[from..];
            let stmt = match rest.find(';') {
                Some(end) => &rest[..end],
                None => rest,
            };
            let writes_content = stmt.contains("textContent") || stmt.contains("innerHTML");
            let key = if writes_content {
                stmt.find("T(").and_then(|i| {
                    let after = stmt[i + 2..].trim_start();
                    let after = after.strip_prefix('"')?;
                    let end = after.find('"')?;
                    Some(after[..end].to_string())
                })
            } else {
                None
            };
            keys.push(key);
        }
        (sites, keys)
    }

    /// 行身份列头（`厂商 / 模型`）最后一段 ——「这一行是什么」的单位词（小写、去空白）。
    ///
    /// `None` ＝ 列头不是 `A / B` 形态：提取器分不出单位，规则③必须**响亮变红**，
    /// 否则「含单位词」会退化成「含整串」（一个恒真的检查）。
    fn row_unit_word(row_head: &str) -> Option<String> {
        if !row_head.contains('/') {
            return None;
        }
        let tail = row_head.rsplit('/').next()?.trim().to_lowercase();
        if tail.is_empty() {
            None
        } else {
            Some(tail)
        }
    }

    /// 计数文案是否用**行**的单位说话（纯函数，便于合成输入自证）。
    fn count_names_the_row_unit(count: &str, row_head: &str) -> bool {
        match row_unit_word(row_head) {
            Some(u) => count.to_lowercase().contains(&u),
            None => false,
        }
    }

    /// 计数文案是否**回避**了「单个 key」的名（纯函数；尺子为空 ⇒ 判违反，空尺子即空检查）。
    fn count_avoids_the_key_word(count: &str, key_word: &str) -> bool {
        let k = key_word.trim().to_lowercase();
        !k.is_empty() && !count.to_lowercase().contains(&k)
    }

    /// 一个语言包里 `mk.avail.` 族的 pill 有多大、其中几条用 `word` 说话 —— `(族大小, 命中数)`。
    fn avail_pills_naming(table: &BTreeMap<String, String>, word: &str) -> (usize, usize) {
        let w = word.trim().to_lowercase();
        let mut family = 0usize;
        let mut hits = 0usize;
        for (k, v) in table {
            if k.starts_with(AVAIL_PILL_PREFIX) {
                family += 1;
                if !w.is_empty() && v.to_lowercase().contains(&w) {
                    hits += 1;
                }
            }
        }
        (family, hits)
    }

    /// 市场工具栏计数（`#mk-count`）数的是**模型行**，文案就必须用「模型」这个单位说话。
    ///
    /// 缺陷形状（C2172 实测，两个包都错）：`#mk-count` 印 `T("cnt.on", { n: list.length })`，
    /// 而 `cnt.on` 的值是 `"{n} 个在售 key"` / `"{n} keys on sale"` —— **同一屏**的表头写
    /// `厂商 / 模型`、行内 pill 自己写「可用 · 3 key / 无 key」⇒ 数字是**行数**、单位是**key 数**，
    /// 两重矛盾（夹具 Σkey = 6 ≠ 4 行）。设计基线（`docs/prototype/aitokenpool-console.html`）写的
    /// 是「共 N 个模型」⇒ 漂移，非取舍。
    ///
    /// 五条规则，各有独立的牙：
    /// ① 计数键**派生**自「谁在填这个元素」（`app.js` 里 `#mk-count` 之后的 `T(…)`）：
    ///    每个位点都必须经 `T("字面量")` 取值、且所有位点**同名**（同一格必须同源）；
    ///    位点用了不止一个键时，**每一个**键都要按 ①③④ 审（不能只挑一个来审）；
    /// ② 行身份键**派生**自同屏表头（`index.html` 里 `#mk-body` 所在表格 `<thead>` 的第一个
    ///    `data-i18n`），其值必须是 `A / B` 形态（否则单位分不出来 ⇒ 响亮变红）；
    /// ③ 两包里计数文案都必须含行身份的单位词（`模型` / `model`）；
    /// ④ 两包里计数文案都**不得**含「单个 key」的名（尺子＝`share.col.key` 的值）；
    /// ⑤ 那把尺子必须与市场行的 `mk.avail.` pill 家族**同词** —— 尺子不能被悄悄换掉。
    ///
    /// ⚠️ 射程（诚实记录，同时写进 `ui/README.md`）：
    /// - 只钉**单位**，不钉**数值**：数值由 DOM 探针 `c2172_probe.js` 的 `F1/F2` 腿钉
    ///   （数字必须等于行数，且不等于 key 总数）；
    /// - 删掉**一个**填充位点（如空态那句）本门禁看不见（位点数会一起降），由探针
    ///   `P0b/F1/T1` 三条腿拒掉；
    /// - 规则④/⑤ 的尺子取自**共享视图**的列头（不是市场行的 pill）⇒ 有人把「无 key」改成「无」
    ///   时门禁照绿（探针 `K2` 腿拒掉）；反过来，有人把两处 key 词**一起**改名时 ④ 会退化成
    ///   空检查 —— 此时仍由 ③ 守住轴（③ 才是本轴的主牙）。
    #[test]
    fn the_marketplace_count_is_expressed_in_the_unit_of_its_rows() {
        let app = strip_js_comments(APP_JS);
        let packs = packs();
        let mut problems: Vec<String> = Vec::new();

        // ① 计数键派生自填充位点
        let (sites, keys) = mk_count_fill_keys(&app);
        if sites == 0 {
            problems.push(
                "① 无位点: `#mk-count` 没有任何填充位点 —— 计数被删掉，或提取器失真".to_string(),
            );
        }
        let mut distinct: BTreeSet<&str> = BTreeSet::new();
        for (i, k) in keys.iter().enumerate() {
            match k {
                Some(k) => {
                    distinct.insert(k.as_str());
                }
                None => problems.push(format!(
                    "① 无键: `#mk-count` 第 {} 个填充位点没有经 `T(\"字面量\")` 取到键（硬编码，或提取器读不出）—— 勿静默少算一个位点",
                    i + 1
                )),
            }
        }
        if distinct.len() > 1 {
            problems.push(format!(
                "① 不同源: `#mk-count` 的填充位点用了不止一个键 {distinct:?} —— 同一格必须同源"
            ));
        }
        // 每个用来填这个元素的键都得合格 ⇒ 逐个审（①③④ 循环在下面按语言展开）
        let count_keys: Vec<&str> = distinct.iter().copied().collect();

        // ② 行身份键派生自同屏表头
        let row_key = table_head_key(INDEX_HTML, "mk-body").unwrap_or_default();
        if row_key.is_empty() {
            problems.push(
                "② 提取器失真: `#mk-body` 所在表格的 `<thead>` 里取不到 `data-i18n`".to_string(),
            );
        }

        // ③④⑤ 逐包比对
        for (lang, table) in [("zh", &packs.zh), ("en", &packs.en)] {
            // ② 行身份列头（与计数键无关，先判一次）
            let row_head = match table.get(&row_key) {
                Some(v) => v.clone(),
                None => {
                    problems.push(format!(
                        "② 非包键 {lang}: 行身份键 `{row_key}` 不在 {lang} 包里 —— 提取器读到的不是包键"
                    ));
                    String::new()
                }
            };
            if !row_head.is_empty() && row_unit_word(&row_head).is_none() {
                problems.push(format!(
                    "② 列头形状 {lang}: 行身份列头 `{row_key}` 的值 `{row_head}` 不是 `A / B` 形态 —— 单位词分不出来"
                ));
            }

            // 尺子：本仓给「单个 key」的名。空尺子会让规则④变成空检查 ⇒ 必须先自证可信。
            let key_word = table.get(KEY_UNIT_LABEL_KEY).cloned().unwrap_or_default();
            let ruler = key_word.trim().to_lowercase();
            if key_word.is_empty() || ruler.is_empty() || key_word.contains("{n}") {
                problems.push(format!(
                    "⑤ 尺子失真 {lang}: `{KEY_UNIT_LABEL_KEY}` 的 {lang} 值是 `{key_word}` —— 它不是「单个 key 的名字」（空的尺子会让规则④变成空检查）"
                ));
            }
            let (pill_family, pill_hits) = avail_pills_naming(table, &key_word);
            if pill_family == 0 {
                problems.push(format!(
                    "⑤ 语料失真 {lang}: {lang} 包里没有 `{AVAIL_PILL_PREFIX}` 族的市场行 pill —— 规则⑤会变成空检查"
                ));
            } else if pill_hits == 0 {
                problems.push(format!(
                    "⑤ 跨视图用词 {lang}: 「单个 key」的名只在共享列头（`{KEY_UNIT_LABEL_KEY}` = `{key_word}`）出现，市场行 pill 家族（{pill_family} 条）一条都没用它"
                ));
            }

            // ①③④ 逐个填充键判定：**每一个**用来填这个元素的键都必须合格
            //（填充点越多、越不能只挑一个键来审）
            for ck in &count_keys {
                let count_text = match table.get(*ck) {
                    Some(v) => v.clone(),
                    None => {
                        problems.push(format!(
                            "① 非包键 {lang}: 计数键 `{ck}` 不在 {lang} 包里 —— 提取器读到的不是包键"
                        ));
                        continue;
                    }
                };
                if !count_text.contains("{n}") {
                    problems.push(format!(
                        "① 非计数文案 {lang}: 计数键 `{ck}` 的值 `{count_text}` 没有 `{{n}}` 占位符"
                    ));
                }
                if row_unit_word(&row_head).is_some()
                    && !count_names_the_row_unit(&count_text, &row_head)
                {
                    problems.push(format!(
                        "③ 行单位 {lang}: 市场工具栏计数没用行单位说话 —— 计数 `{ck}` = `{count_text}`；它数的那些行的身份列头 `{row_key}` = `{row_head}`"
                    ));
                }
                if !ruler.is_empty() && !count_avoids_the_key_word(&count_text, &key_word) {
                    problems.push(format!(
                        "④ key 单位 {lang}: 市场工具栏计数拿「单个 key」的名 `{key_word}` 当总数的单位 —— `{ck}` = `{count_text}`（它数的是模型行）"
                    ));
                }
            }
        }

        assert!(
            problems.is_empty(),
            "市场工具栏计数的单位漂移（C2172）：\n  - {}",
            problems.join("\n  - ")
        );
    }

    /// 阴极对照：单位一致性判别式必须真的会失败（恒真的检查等价于没有检查）。
    #[test]
    fn marketplace_count_unit_checker_detects_injected_defects() {
        // ③ 行单位：含单位词才算数；列头不是 `A / B` 形态 ⇒ 判违反（不能退化成「含整串」）
        assert!(count_names_the_row_unit("4 models", "Provider / model"));
        assert!(count_names_the_row_unit("4 个模型", "厂商 / 模型"));
        assert!(!count_names_the_row_unit(
            "4 keys on sale",
            "Provider / model"
        ));
        assert!(!count_names_the_row_unit("4 个在售 key", "厂商 / 模型"));
        assert!(!count_names_the_row_unit(
            "4 Provider model rows",
            "Provider model"
        ));
        assert!(!count_names_the_row_unit("4 models", " / "));

        // ④ key 单位：尺子为空 ⇒ 判违反（不空转）
        assert!(count_avoids_the_key_word("4 models", "Key"));
        assert!(!count_avoids_the_key_word("4 keys on sale", "Key"));
        assert!(!count_avoids_the_key_word("4 个在售 key", "Key"));
        assert!(!count_avoids_the_key_word("4 models", "   "));
        assert!(!count_avoids_the_key_word("4 models", ""));

        // ⑤ pill 家族（派生自 `mk.avail.` 前缀，不写死键名）
        let mut pill_pack: BTreeMap<String, String> = BTreeMap::new();
        pill_pack.insert("mk.avail.none".to_string(), "No key".to_string());
        pill_pack.insert(
            "mk.avail.multi".to_string(),
            "Available · {n} keys".to_string(),
        );
        pill_pack.insert("share.col.key".to_string(), "Key".to_string());
        assert_eq!(
            avail_pills_naming(&pill_pack, "Key"),
            (2, 2),
            "pill 家族应只数 `mk.avail.` 前缀的键"
        );
        assert_eq!(
            avail_pills_naming(&pill_pack, "  "),
            (2, 0),
            "空词不得命中任何 pill"
        );
        let no_family: BTreeMap<String, String> = BTreeMap::new();
        assert_eq!(
            avail_pills_naming(&no_family, "Key"),
            (0, 0),
            "没有 pill 家族时必须报族大小为 0（规则⑤才不会空转）"
        );

        // ② 表头提取：锚点是 `#<body_id>` 所在的那张表，且只看它的 `<thead>`
        let html = r##"<table><thead><tr><th data-i18n="other.col">x</th></tr></thead><tbody id="other-body"></tbody></table>
<table><thead><tr><th class="num" data-i18n="mk.col.in">单价</th><th data-i18n="mk.col.providerModel">厂商 / 模型</th></tr></thead><tbody id="mk-body"></tbody></table>"##;
        assert_eq!(
            table_head_key(html, "mk-body").as_deref(),
            Some("mk.col.in"),
            "必须取 `#mk-body` 那张表 `<thead>` 里的**第一个** `data-i18n`"
        );
        assert_eq!(
            table_head_key(html, "other-body").as_deref(),
            Some("other.col"),
            "锚点认错表 ⇒ 单位词会被别处的列头污染"
        );
        assert!(
            table_head_key(html, "nope-body").is_none(),
            "不存在的锚点必须返回 None"
        );
        // 注释里的表不算表（#296）
        let commented = r##"<!-- <table><thead><tr><th data-i18n="ghost.col">x</th></tr></thead><tbody id="mk-body"></tbody></table> -->
<table><thead><tr><th data-i18n="real.col">y</th></tr></thead><tbody id="mk-body"></tbody></table>"##;
        assert_eq!(
            table_head_key(commented, "mk-body").as_deref(),
            Some("real.col"),
            "HTML 注释里的表被当成了表（#296：注释不是代码）"
        );

        // ① 填充位点提取：同名、异名、没有 `T("…")`、以及注释里的位点
        let same = r##"$("#mk-count").textContent = T("cnt.models", { n: 0 });
$("#mk-count").textContent = T("cnt.models", { n: list.length });"##;
        let (n, ks) = mk_count_fill_keys(same);
        assert_eq!(n, 2, "两个填充位点应被数到 2 处");
        assert_eq!(
            ks,
            vec![
                Some("cnt.models".to_string()),
                Some("cnt.models".to_string())
            ]
        );
        let divergent = r##"$("#mk-count").textContent = T("cnt.models", { n: 0 });
$("#mk-count").textContent = T("cnt.on", { n: list.length });"##;
        let (_, ks) = mk_count_fill_keys(divergent);
        assert_ne!(
            ks[0], ks[1],
            "两处写成不同的键时必须分得出来（否则「同一格必须同源」恒真）"
        );
        let literal = r##"$("#mk-count").textContent = "4 个在售 key";"##;
        let (n, ks) = mk_count_fill_keys(literal);
        assert_eq!(
            (n, ks),
            (1, vec![None]),
            "硬编码写入必须读成「没有包键」而不是静默跳过"
        );
        // ⚠️ 这条腿是**被 A/B 打出来的**（m_hardcoded 首跑：硬编码位点被读成 `Some("mk.collapse")`）：
        //    同文件里**后面某句无关的** `T("…")` 不得被当成这个元素的键（#335：按语句边界收口）。
        let unrelated = "$(\"#mk-count\").textContent = \"4 个模型\";\n        x.innerHTML = T(\"mk.collapse\", {});";
        assert_eq!(
            mk_count_fill_keys(unrelated),
            (1, vec![None]),
            "元素写完字面量之后，别处一句无关的 T(\"…\") 被当成了它的键"
        );
        // 跨行的 `T(` 调用仍须被认出（收口到语句，不是收口到行）
        let multiline =
            "$(\"#mk-count\").textContent = T(\n  \"cnt.models\",\n  { n: list.length }\n);\n";
        assert_eq!(
            mk_count_fill_keys(multiline),
            (1, vec![Some("cnt.models".to_string())]),
            "跨行的 T( 调用没被认出 —— 收口过紧"
        );
        let ghost = "// $(\"#mk-count\").textContent = T(\"ghost.key\", { n: 0 })";
        assert_eq!(
            mk_count_fill_keys(&strip_js_comments(ghost)).0,
            0,
            "JS 注释里的填充位点被当成了位点（#296：注释不是代码）"
        );
    }
}
