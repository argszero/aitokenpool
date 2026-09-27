//! 双语文档对偶门禁（R106）：仓库**唯一**的双语对（`README.md` / `README.en.md`）必须携带**同一组事实**。
//!
//! ## 起因
//!
//! `cabe4c6`（#203，`docs(architecture): correct the ops method, the transaction types and the
//! API list scope`）把中文版「接入方式」那句改成了「API 一览见 `docs/architecture.md`，
//! **权威清单以 `src/routes/mod.rs` 的 `router()` 为准**」，但**只改了中文那半边** ——
//! `git show --name-only cabe4c6` 里没有 `README.en.md`，英文版从此停在 pre-#203 原句
//! （`Full API reference: docs/architecture.md.`）。于是同一句话的两个语言版本把「到哪里找真相」
//! 指向了不同的地方：中文读者被指向**代码**，英文读者被指向一份**会漂移的文档**
//! （`docs/architecture.md` 正是 #203 要把它降级成「一览」的那份）。
//!
//! 两份 README 第 3 行互相声明对方是另一个语言版本，所以它们是**同一份内容的两个载体**，
//! 而不是两篇文章 —— 一个载体落后，就是同一个问题有两个答案。
//!
//! **这不是「零命中即漂移」**：英文版的端点列表本身还在（四个端点逐字相同），
//! 缺的只是「权威在哪」这句指路。它是「同一事实两个载体、其中一个落后」，
//! 与本仓发行链那一族（版本事实被抄进 `Dockerfile` / `docker-compose.yml` / `CHANGELOG.md`，
//! 见 `deploy_gate.rs`）同形；也与 `i18n_pack.rs` 的「zh/en 键集相等」同类 ——
//! 那里管的是语言包，这里管的是文档。
//!
//! ## 判据（全部**派生**，不写快照）
//!
//! 两份文件里四类**与语言无关**的结构位，取**集合**后必须相等：
//!
//! | 族 | 抽取 | 为什么该相等 |
//! |----|------|--------------|
//! | 行内代码 span | `` `…` `` | 代码标识符 / 路径 / 端点 / 配置键 —— 译文不改变它们 |
//! | 链接目标 | `](…)` | 指向同一个目标文档 |
//! | 裸 URL | `<https://…>` autolink | 指向同一个线上实例 |
//! | 数字字面量 | 数字串（前后非词字符） | 端口 / 上限 / 比例 / 位数 |
//!
//! **刻意不比**（语言相关，比了就是噪声）：散文、标题文字、表头、强调 / 粗体、行序、行数、
//! 行内相对位置。取**集合**（而非多重集）也是刻意的：同一件事写两遍与写一遍不算分歧。
//!
//! ## 射程（如实写清）
//!
//! **词法**：证「两边出现同一组结构位」，**不证**译文语义等价、不证译得对、不证行序一致。
//! 一个「把中文那句整段删掉即可对齐」的竞争修法会**过闸** —— 那属于评审的判断，不属本门禁。
//!
//! ## 设计约束（与 `i18n_pack.rs` / `body_limit_gate.rs` 同型）
//!
//! - **仅测试期编译**（`#[cfg(test)] mod readme_gate`，见 `main.rs`）：`include_str!` 在编译期
//!   把两份 README 嵌进来，`#[cfg(test)]` 保证它们不进入发布产物。
//! - **零新依赖**：只用 `std`，扫描手写（本仓其余门禁同样不引 regex）。
//! - **阳性对照**：断言每一族在两份文件里都**非空**、且四族合计够多 —— 抽取器坏掉（两边都抽出
//!   空集）时「集合相等」恒真，那是**在空集上通过**（坑 68）。
//! - **合成牙齿**：一侧删掉一个 span / 链接 / URL / 数字必须被抓到；只改强调与标题则必须干净。

use std::collections::BTreeSet;

/// 中文版原文（编译期嵌入）。
const ZH: &str = include_str!("../README.md");
/// 英文版原文（编译期嵌入）。
const EN: &str = include_str!("../README.en.md");

/// 一个「族」的抽取器。
type Extractor = fn(&str) -> BTreeSet<String>;

/// 四类与语言无关的结构位。
const FAMILIES: &[(&str, Extractor)] = &[
    ("行内代码 span", code_spans),
    ("链接目标", link_targets),
    ("裸 URL", bare_urls),
    ("数字字面量", number_literals),
];

/// 词字符（前 / 后紧邻它时，数字串属于标识符，不算数字字面量）。
fn is_word_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// 行内代码 span：成对单反引号之间、**不含换行**的内容（去掉首尾空白）。
///
/// 围栏（```` ``` ````）抽不到东西：紧随其后的那个反引号立刻终结匹配窗口，
/// 所以 ```` ```bash ```` 与 ```` ``` ```` 都不算 span。
fn code_spans(text: &str) -> BTreeSet<String> {
    let b = text.as_bytes();
    let mut out = BTreeSet::new();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] != b'`' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < b.len() && b[j] != b'`' && b[j] != b'\n' {
            j += 1;
        }
        if j < b.len() && b[j] == b'`' && j > i + 1 {
            let span = text[i + 1..j].trim();
            if !span.is_empty() {
                out.insert(span.to_string());
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

/// markdown 链接目标：`](target)`。
fn link_targets(text: &str) -> BTreeSet<String> {
    let b = text.as_bytes();
    let mut out = BTreeSet::new();
    let mut i = 0usize;
    while i + 1 < b.len() {
        if b[i] == b']' && b[i + 1] == b'(' {
            if let Some(end) = text[i + 2..].find(')') {
                let target = text[i + 2..i + 2 + end].trim();
                if !target.is_empty() {
                    out.insert(target.to_string());
                    i += 2 + end + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

/// URL 里允许出现的字符（RFC 3986 的 pchar / 路径 / 查询 / 片段集合）。
fn is_url_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-._~:/?#[]@!$&'()*+,;=%".contains(c)
}

/// 句读标点：URL 只会**跟着**它出现，不会是 URL 的一部分。
fn is_url_tail_punct(c: char) -> bool {
    matches!(
        c,
        '.' | ',' | ';' | ':' | '!' | '?' | '"' | '\'' | ')' | ']' | '>'
    )
}

/// 文本里第一个 `http://` / `https://` 的位置。
fn scheme_at(text: &str) -> Option<usize> {
    match (text.find("https://"), text.find("http://")) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// 裸 URL：按 URL 字符集扫描，直到不属于该集合的字符为止 —— 空格、换行、**中文标点**都会止住它
/// （按空白切词会漏掉后者：`<https://x.test/>，限` 不是无法处理的，只是会被切错）。
/// 末尾再剥掉句读标点。markdown 链接里的 URL 同样会被抽到 —— 那属于另一族，不冲突。
fn bare_urls(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = text;
    while let Some(pos) = scheme_at(rest) {
        let tail = &rest[pos..];
        let end = tail
            .char_indices()
            .find(|(_, c)| !is_url_char(*c))
            .map_or(tail.len(), |(i, _)| i);
        let url = tail[..end].trim_end_matches(is_url_tail_punct);
        if url.starts_with("http") {
            out.insert(url.to_string());
        }
        if end == 0 {
            break;
        }
        rest = &tail[end..];
    }
    out
}

/// 数字字面量：连续数字串（可含小数点），且前后都不是词字符（`v1.86` 里的数字不算）。
///
/// 末尾的点去掉：`the limit is 16.` 是句号，不是小数。
fn number_literals(text: &str) -> BTreeSet<String> {
    let b = text.as_bytes();
    let mut out = BTreeSet::new();
    let mut i = 0usize;
    while i < b.len() {
        if !b[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
            i += 1;
        }
        let prev = if start == 0 { 0 } else { b[start - 1] };
        let next = if i < b.len() { b[i] } else { 0 };
        if !is_word_byte(prev) && !is_word_byte(next) {
            let literal = text[start..i].trim_end_matches('.');
            if !literal.is_empty() {
                out.insert(literal.to_string());
            }
        }
    }
    out
}

/// 四族合计项数（抽取器健康度用）。
fn fact_count(text: &str) -> usize {
    FAMILIES
        .iter()
        .map(|(_, extract)| extract(text).len())
        .sum()
}

/// 逐族比较，返回人类可读的分歧行（空 = 对偶）。
fn divergences(zh: &str, en: &str) -> Vec<String> {
    let mut out = Vec::new();
    for &(name, extract) in FAMILIES {
        let a = extract(zh);
        let b = extract(en);
        for item in a.difference(&b) {
            out.push(format!("{name}：仅中文版有 `{item}`"));
        }
        for item in b.difference(&a) {
            out.push(format!("{name}：仅英文版有 `{item}`"));
        }
    }
    out
}

#[test]
fn the_two_readmes_carry_the_same_facts() {
    let drift = divergences(ZH, EN);
    assert!(
        drift.is_empty(),
        "README.md 与 README.en.md 是同一份文档的两个载体，结构位必须一致（{} 处分歧）：\n{}",
        drift.len(),
        drift.join("\n")
    );
}

#[test]
fn every_family_is_present_on_both_sides() {
    for &(name, extract) in FAMILIES {
        assert!(
            !extract(ZH).is_empty(),
            "{name}：中文版一项都抽不到 —— 抽取器坏了"
        );
        assert!(
            !extract(EN).is_empty(),
            "{name}：英文版一项都抽不到 —— 抽取器坏了"
        );
    }
    // 阳性对照（下限，不是快照）：抽取器真的看见了语料。今天实测两份各 30+ 项。
    for (label, text) in [("README.md", ZH), ("README.en.md", EN)] {
        let n = fact_count(text);
        assert!(
            n >= 30,
            "{label}：四族合计只抽到 {n} 项 —— 抽取器坏了（在空集上通过是假绿）"
        );
    }
}

#[test]
fn a_fact_lost_on_one_side_is_reported() {
    let zh = "看 `router()` 与 `src/routes/mod.rs`，见 [docs](docs/a.md) 和 <https://x.test/>，限 16 位。";
    let full = "see `router()` and `src/routes/mod.rs`, [docs](docs/a.md) and <https://x.test/>, 16 chars.";
    // 两侧齐全 ⇒ 干净
    assert!(
        divergences(zh, full).is_empty(),
        "{:?}",
        divergences(zh, full)
    );
    // 英文侧掉了代码 span ⇒ 必须被抓到（这正是 #203 丢下的那一句）
    for missing in [
        ("span", "see and `src/routes/mod.rs`, [docs](docs/a.md) and <https://x.test/>, 16 chars."),
        ("link", "see `router()` and `src/routes/mod.rs`, and <https://x.test/>, 16 chars."),
        ("url", "see `router()` and `src/routes/mod.rs`, [docs](docs/a.md) and , 16 chars."),
        ("number", "see `router()` and `src/routes/mod.rs`, [docs](docs/a.md) and <https://x.test/>, chars."),
    ] {
        let drift = divergences(zh, missing.1);
        assert!(
            !drift.is_empty(),
            "英文侧掉了 {} 却没报分歧：{:?}",
            missing.0,
            drift
        );
    }
}

#[test]
fn language_dependent_positions_are_not_compared() {
    // 标题、表头、强调、行序、换行位置都随语言变 —— 比它们就是噪声。
    let zh = "## 接入方式\n\n**重要**：看 `router()`。\n\n| 文档 | 说明 |\n|----|----|\n";
    let en = "## Gateway access\n\n**Important**: see `router()`.\n\n| Doc | Description |\n|-----|-------------|\n";
    assert!(divergences(zh, en).is_empty(), "{:?}", divergences(zh, en));
    // 折行不算分歧：span 不得跨行，故 `a`/`b` 分成两行与写在一行等价。
    assert!(divergences("`a`\n`b`\n", "`a` `b`\n").is_empty());
    // 围栏不是 span，围栏内的命令两侧相同即可。
    assert!(code_spans("```bash\ncargo run\n```\n").is_empty());
    assert!(divergences("```bash\ncargo run\n```\n", "```bash\ncargo run\n```\n").is_empty());
    // 标识符里的数字不算数字字面量（`v1.86` / `key2`）。
    assert!(number_literals("MSRV 是 v1.86，见 key2").is_empty());
}
