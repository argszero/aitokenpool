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
const ZH_KEY_COUNT: usize = 785;
const EN_KEY_COUNT: usize = 785;
const STATIC_ATTR_COUNT: usize = 330;
const STATIC_ATTR_DISTINCT: usize = 305;
const T_LITERAL_COUNT: usize = 537;
const T_LITERAL_DISTINCT: usize = 430;

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
}
