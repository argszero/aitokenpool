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

use std::collections::{BTreeSet, HashSet};

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
const ZH_KEY_COUNT: usize = 775;
const EN_KEY_COUNT: usize = 775;
const STATIC_ATTR_COUNT: usize = 330;
const STATIC_ATTR_DISTINCT: usize = 305;
const T_LITERAL_COUNT: usize = 520;
const T_LITERAL_DISTINCT: usize = 418;

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
                        out.push(src[start..m].to_string());
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

/// 读出两个语言包，并做阳性对照校验。
///
/// ⚠️ 计数断言是**阳性对照**，不是装饰：C2005 曾把区段标记写错，得到两个**空集**，
/// 于是「中英键集一致」在空集上「通过」—— 假绿的门禁比没有门禁更危险。
/// 因此这里一旦与已知真值不符就**直接 panic**，绝不带着空集或少数集继续断言。
fn packs() -> (BTreeSet<String>, BTreeSet<String>) {
    let zh_raw = scan_object_keys(pack_region(I18N_JS, ZH_START, EN_START));
    let en_raw = scan_object_keys(pack_region(I18N_JS, EN_START, EN_END));
    assert_eq!(
        zh_raw.len(),
        ZH_KEY_COUNT,
        "中文包键数应为 {ZH_KEY_COUNT}，实得 {} —— 提取器已失真（区段标记或扫描规则需重核），拒绝继续",
        zh_raw.len()
    );
    assert_eq!(
        en_raw.len(),
        EN_KEY_COUNT,
        "英文包键数应为 {EN_KEY_COUNT}，实得 {} —— 提取器已失真，拒绝继续",
        en_raw.len()
    );
    (zh_raw.into_iter().collect(), en_raw.into_iter().collect())
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
        let (zh, en) = packs();
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
        let (zh, en) = packs();
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
        let (zh, en) = packs();
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

    /// 阴性对照：把「删键」「拼错键」注入语料，检查器必须真的报错。
    ///
    /// 没有这一条，上面四个断言无法自证「它们有能力失败」—— 一个永远为真的检查等价于没有检查。
    #[test]
    fn checker_detects_injected_defects() {
        let (zh, en) = packs();

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
