//! 部署产物门禁（C2117）：**主密钥要么不给默认值，要么给合法 64 位 hex**。
//!
//! 起因（rant 2026-09-14T17:30:56）：`docker-compose.yml` 曾把
//! `ATP_MASTER_KEY=${ATP_MASTER_KEY:-dev-master-key-请替换}` 作为默认值。该值不是 64 位 hex，
//! 于是 `src/crypto.rs::from_config` 在 `parse_master_key` 失败后**回退**到随机 dev 主密钥
//!（`crypto.rs:41` 记 ERROR → `crypto.rs:56-58` 生成随机密钥）⇒ 重启后已加密的上游 key
//! 全部解不开 ⇒ 全量 503，且**故障是静默的**（只有启动日志一行 ERROR/WARN）。
//!
//! 同类缺陷已出现两次：本仓库的 compose，以及 GitLab 部署仓库的 `ci/docker-compose.yml`
//!（后者 2026-09-14 实测造成过十余分钟全量 503）。所以把这条约定锁进 `cargo test`。
//!
//! **断言**：扫描随仓库发布的部署/配置产物里出现的 master_key 默认值 ——
//! 「要么不提供默认值，要么是合法 64 位 hex」。三类形态的判定：
//!
//! | 形态 | 例 | 判定 |
//! |---|---|---|
//! | 必填（无默认值） | `${ATP_MASTER_KEY:?…}` | 允许 |
//! | 带默认值 | `${ATP_MASTER_KEY:-<x>}` | `<x>` 必须 64 位 hex |
//! | 裸字面量 | `ATP_MASTER_KEY=<x>` / `master_key = "<x>"` | 空或 64 位 hex |
//! | 非字面量 | `$(openssl rand -hex 32)`（示例命令，不是默认值） | 跳过 |
//!
//! 设计约束（与 `i18n_pack.rs` / `catalog_gate.rs` / `table_gate.rs` / `perf_gate.rs` 同型）：
//! **仅测试期编译**、**零新依赖**、**编译期读入**（不依赖工作目录）；
//! **阳性对照**：断言确实在 compose 里找到了那条设置、config 示例里找到了那一行 ——
//! 否则「扫描到 0 条」也会「通过」。
//!
//! # 第二条规则（R77）：部署产物**抄**的那些版本号必须等于 `Cargo.toml` 声明的那份
//!
//! `docker-compose.yml` / `Dockerfile` 各自抄了一份 `Cargo.toml` 的事实 —— 发行版本号
//! （`image: aitokenpool:<v>`、`# Build: docker build -t aitokenpool:<v> .`、头注释里的
//!「（v<v>）」）与最低 Rust 版本（`FROM rust:<v>-slim`）。跨格式无法 import，只能抄；
//! 而**抄了没有断言**就是腐烂的栖息地（C2059）—— 这份副本生于 `a51b3ba`（v0.6.6）后
//! 一次未改，到 R77 已陈旧 **22 个发行版**：README 推荐的 `docker compose up -d --build`
//! 建出的镜像被标成 `aitokenpool:0.6.6`，而里面的二进制自报 `0.7.28`
//!（`/healthz` 的 `env!("CARGO_PKG_VERSION")`）。
//!
//! 因此与 `catalog_gate.rs` 的 `MODEL_COUNT` 同判：**能抄的前提是有人守**。
//! 期望值**从 `Cargo.toml` 派生**（`manifest_field`，不写快照）：
//!
//! | 形态 | 例 | 判定 |
//! |---|---|---|
//! | 镜像 tag | `image: aitokenpool:<v>` | `<v>` 必须**等于**发行版本号 |
//! | 注释标注 | `（v<v>）` / `(v<v>)` | `<v>` 必须**等于**发行版本号 |
//! | 构建镜像 | `FROM rust:<v>-slim` | `<v>` 必须 **≥** `rust-version` |
//!
//! `latest` 这类移动 tag 不是版本**声明**，不参与判定（README 里正是它）。
//! `FROM rust:<v>` 用 **≥** 而非相等：用**更新**的工具链构建是合法的，被守的是
//!「声明被抬高、Dockerfile 没跟」这一向（与 `ci.yml` 的 `msrv` job 同向）。
//!
//! 代价是**发行 PR 从此还要改这两个文件** —— 漏改会被本门禁当场抓住（这是刻意的）。
//! **射程**：只覆盖部署产物；`CONTRIBUTING.md` / `docs/architecture.md` 里的散文提及
//! 不在射程内（散文无运行时后果，且本仓库既有的约定是「能不抄就不抄」，
//! 见 `docs/plan-api-matrix.md` 第 4 节）。

/// 编译期读入的部署/配置产物。
const FILES: &[(&str, &str)] = &[
    ("docker-compose.yml", include_str!("../docker-compose.yml")),
    (
        "config/config.example.toml",
        include_str!("../config/config.example.toml"),
    ),
    ("Dockerfile", include_str!("../Dockerfile")),
    ("README.md", include_str!("../README.md")),
    ("README.en.md", include_str!("../README.en.md")),
];

const ENV_KEY: &str = "ATP_MASTER_KEY";

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 一行里 `ATP_MASTER_KEY=` 之后的形态。
#[derive(Debug, PartialEq)]
enum Form<'a> {
    /// `${VAR:?msg}` —— 缺失即失败，没有默认值。
    Required,
    /// `${VAR:-<默认值>}`。
    Default(&'a str),
    /// 裸字面量（YAML env 项）。
    Literal(&'a str),
    /// 不是字面量默认值（`$(…)` 命令替换、`${VAR}` 透传、变量引用）。
    NotALiteral,
}

fn classify(value: &str) -> Form<'_> {
    let v = value.trim();
    if let Some(inner) = v.strip_prefix("${") {
        if let Some(end) = inner.find('}') {
            let spec = &inner[..end];
            if let Some((_, def)) = spec.split_once(":-") {
                return Form::Default(def);
            }
            if spec.contains(":?") {
                return Form::Required;
            }
            return Form::NotALiteral; // ${VAR} 纯透传
        }
    }
    if v.contains('$') {
        return Form::NotALiteral; // $(openssl rand -hex 32) 之类
    }
    Form::Literal(v.trim_matches(|c| c == '"' || c == '\''))
}

/// `docker-compose.yml` / `Dockerfile` / README 里的 `ATP_MASTER_KEY=…`（不含行首注释说明）。
fn env_assignments(src: &str) -> Vec<(usize, Form<'_>)> {
    let mut out = Vec::new();
    for (i, line) in src.lines().enumerate() {
        // 只认「像设置值」的行：`- KEY=` / `-e KEY=` / `export KEY=`。
        // 行首 `#` 的纯说明（如「⚠️ 生产环境必须设置 ATP_MASTER_KEY（32 字节 hex…」）不含 `=`，天然被排除。
        let Some(pos) = line.find(&format!("{ENV_KEY}=")) else {
            continue;
        };
        let after = &line[pos + ENV_KEY.len() + 1..];
        out.push((i + 1, classify(after)));
    }
    out
}

/// `config.example.toml` 里 `master_key = "…"`（允许被 `#` 注释 —— 那正是「示例」形态）。
fn config_master_keys(src: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let body = line.trim_start();
        let body = body.strip_prefix('#').unwrap_or(body).trim_start();
        let Some(rest) = body.strip_prefix("master_key") else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('=') else {
            continue; // 例如 `master_key_file` 之类，不是赋值
        };
        out.push((i + 1, rest.trim().trim_matches(|c| c == '"' || c == '\'')));
    }
    out
}

/// 违规项（人类可读）。空 = 通过。
fn violations() -> Vec<String> {
    let mut bad = Vec::new();
    for (name, src) in FILES {
        for (ln, form) in env_assignments(src) {
            match form {
                Form::Default(d) if !is_hex64(d) => bad.push(format!(
                    "{name}:{ln}: 默认值不是 64 位 hex（缺省时会静默回退随机主密钥）：{d:?}"
                )),
                Form::Literal(l) if !l.is_empty() && !is_hex64(l) => bad.push(format!(
                    "{name}:{ln}: 字面量主密钥不是 64 位 hex（缺省时会静默回退随机主密钥）：{l:?}"
                )),
                _ => {}
            }
        }
        for (ln, val) in config_master_keys(src) {
            if !val.is_empty() && !is_hex64(val) {
                bad.push(format!(
                    "{name}:{ln}: master_key 示例值不是 64 位 hex：{val:?}"
                ));
            }
        }
    }
    bad
}

// ---------------------------------------------------------------------------
// 第二条规则（R77）：部署产物抄的版本号必须与 `Cargo.toml` 声明的一致
// ---------------------------------------------------------------------------

/// `Cargo.toml` 原文 —— 本门禁的**唯一期望来源**（不写快照）。
const MANIFEST: &str = include_str!("../Cargo.toml");

/// 取 `[package]` 段里某个字段的值（去引号）。
///
/// 只认 `[package]` 段：依赖表里也有 `version = "0.7"` 这类同名字段，全局匹配会把它们
/// 当成期望值 —— 同名字段必须按**所属段**收窄（同 C2173 的教训）。
fn manifest_field(key: &str) -> Option<String> {
    let mut in_package = false;
    for line in MANIFEST.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_package = t == "[package]";
            continue;
        }
        if !in_package || t.starts_with('#') {
            continue;
        }
        let Some(rest) = t.strip_prefix(key) else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('=') else {
            continue; // 例如 `versioned = …` 之类，不是赋值
        };
        return Some(
            rest.trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_string(),
        );
    }
    None
}

/// 发行版本号（`Cargo.toml` 的 `version`）。
fn declared_version() -> String {
    manifest_field("version").expect("Cargo.toml 的 [package] 段必须有 version")
}

/// 声明的最低 Rust 版本（`Cargo.toml` 的 `rust-version`）。
fn declared_msrv() -> String {
    manifest_field("rust-version").expect("Cargo.toml 的 [package] 段必须有 rust-version")
}

/// `x.y.z` 形态的版本号。`latest` 这类移动 tag 不是版本**声明** ⇒ 不参与判定。
fn looks_like_version(tok: &str) -> bool {
    tok.contains('.')
        && tok.chars().all(|c| c.is_ascii_digit() || c == '.')
        && !tok.starts_with('.')
        && !tok.ends_with('.')
}

fn as_tuple(v: &str) -> Vec<u32> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// `a >= b`（缺失分量按 0 补：`1.86` == `1.86.0`）。
fn at_least(a: &str, b: &str) -> bool {
    let (mut x, mut y) = (as_tuple(a), as_tuple(b));
    while x.len() < y.len() {
        x.push(0);
    }
    while y.len() < x.len() {
        y.push(0);
    }
    x >= y
}

/// 一行里 `<image>:<tag>` 形态的镜像引用；只收**版本号** tag。
fn image_tags(src: &str, image: &str) -> Vec<(usize, String)> {
    let needle = format!("{image}:");
    let mut out = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let mut from = 0usize;
        while let Some(pos) = line[from..].find(&needle) {
            let start = from + pos + needle.len();
            let tok: String = line[start..]
                .chars()
                .take_while(|c| !c.is_whitespace() && !"\"'`".contains(*c))
                .collect();
            if looks_like_version(&tok) {
                out.push((i + 1, tok));
            }
            from = start;
            if from >= line.len() {
                break;
            }
        }
    }
    out
}

/// 头注释里的 `(v<版本>)` / `（v<版本>）`（本仓库用它标注「这份产物属于哪个发行版」）。
fn version_comments(src: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let cs: Vec<char> = line.chars().collect();
        let mut j = 1;
        while j + 1 < cs.len() {
            if cs[j] == 'v' && (cs[j - 1] == '(' || cs[j - 1] == '（') && cs[j + 1].is_ascii_digit()
            {
                let mut k = j + 1;
                while k < cs.len() && (cs[k].is_ascii_digit() || cs[k] == '.') {
                    k += 1;
                }
                if matches!(cs.get(k), Some(')') | Some('）')) {
                    out.push((i + 1, cs[j + 1..k].iter().collect()));
                    j = k;
                    continue;
                }
            }
            j += 1;
        }
    }
    out
}

/// `FROM rust:<版本>[-slim]` —— `rust-version` 的副本。
fn rust_base_images(src: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let t = line.trim_start();
        let Some(rest) = t.strip_prefix("FROM ").or_else(|| t.strip_prefix("from ")) else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix("rust:") else {
            continue;
        };
        let ver: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if looks_like_version(&ver) {
            out.push((i + 1, ver));
        }
    }
    out
}

/// 版本类违规项（人类可读）。空 = 通过。
fn version_violations_of(files: &[(&str, &str)]) -> Vec<String> {
    let ver = declared_version();
    let msrv = declared_msrv();
    let mut bad = Vec::new();
    for (name, src) in files {
        for (ln, tag) in image_tags(src, "aitokenpool") {
            if tag != ver {
                bad.push(format!(
                    "{name}:{ln}: 镜像 tag 是 {tag}，而 Cargo.toml 声明的是 {ver}（副本没跟上）"
                ));
            }
        }
        for (ln, v) in version_comments(src) {
            if v != ver {
                bad.push(format!(
                    "{name}:{ln}: 注释标注的发行版是 v{v}，而 Cargo.toml 声明的是 {ver}"
                ));
            }
        }
        for (ln, v) in rust_base_images(src) {
            if !at_least(&v, &msrv) {
                bad.push(format!(
                    "{name}:{ln}: 构建镜像 rust:{v} 低于 Cargo.toml 声明的 rust-version {msrv}"
                ));
            }
        }
    }
    bad
}

fn version_violations() -> Vec<String> {
    version_violations_of(FILES)
}

#[test]
fn the_deploy_artifacts_carry_the_version_the_manifest_declares() {
    let bad = version_violations();
    assert!(
        bad.is_empty(),
        "部署产物里的版本号必须与 Cargo.toml 一致：\n{}",
        bad.join("\n")
    );
}

#[test]
fn no_master_key_default_that_is_not_valid_hex() {
    let bad = violations();
    assert!(
        bad.is_empty(),
        "主密钥默认值必须「要么不提供，要么合法 64 位 hex」：\n{}",
        bad.join("\n")
    );
}

#[test]
fn the_scanner_actually_finds_the_settings_it_guards() {
    // 阳性对照：扫描器若因改名/改路径而返回空集，上面那条会在空集上「通过」。
    let compose = FILES
        .iter()
        .find(|(n, _)| *n == "docker-compose.yml")
        .unwrap()
        .1;
    let env = env_assignments(compose);
    // compose 里除了真正的那条设置，文件顶部的「快速开始」注释也含 `ATP_MASTER_KEY=`。
    // 因此阳性对照按**形态计数**，不按行数：恰好 1 条 Required，且没有一条是字面量默认值。
    let required = env.iter().filter(|(_, f)| *f == Form::Required).count();
    assert_eq!(
        required, 1,
        "compose 里应恰好有 1 处「必填、无默认值」的设置：{env:?}"
    );
    assert!(
        env.iter()
            .all(|(_, f)| !matches!(f, Form::Default(_) | Form::Literal(_))),
        "compose 不得提供任何字面量主密钥默认值：{env:?}"
    );
    // 扫描器确实**看见了**注释里的示例命令（否则「不可见」与「合规」无法区分）。
    assert!(
        env.iter().any(|(_, f)| *f == Form::NotALiteral),
        "compose 顶部的示例命令应被扫描到并判为非字面量：{env:?}"
    );
    assert!(
        env.iter().any(|(ln, f)| *ln == 28 && *f == Form::Required),
        "第 28 行的 env 项应是唯一生效的设置：{env:?}"
    );
    // 兜底：被注释掉的旧非法默认值不得复活。
    assert!(
        !compose.contains("dev-master-key"),
        "compose 不得再出现非法的 dev-master-key 默认值"
    );

    let cfg = FILES
        .iter()
        .find(|(n, _)| *n == "config/config.example.toml")
        .unwrap()
        .1;
    let keys = config_master_keys(cfg);
    assert_eq!(
        keys.len(),
        1,
        "config 示例里应恰好有 1 行 master_key：{keys:?}"
    );
    assert!(is_hex64(keys[0].1), "示例值应是合法 hex：{:?}", keys[0].1);
}

#[test]
fn classifier_and_config_parser_flag_the_pre_fix_shapes() {
    // 检测器自身的对照（合成输入）—— 确认它们**真的会红**。
    assert_eq!(
        classify("${ATP_MASTER_KEY:-dev-master-key-请替换}"),
        Form::Default("dev-master-key-请替换")
    );
    assert_eq!(classify("${MASTER:?msg}"), Form::Required);
    assert_eq!(classify("$(openssl rand -hex 32)"), Form::NotALiteral);
    assert_eq!(classify("\"$(openssl rand -hex 32)\""), Form::NotALiteral);
    let hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    assert_eq!(classify(&format!("${{K:-{hex}}}")), Form::Default(hex));
    assert_eq!(classify(&format!("\"{hex}\"")), Form::Literal(hex));

    // config 解析：带引号 / 被注释 / 注释内前缀相似词都要正确处理。
    assert_eq!(
        config_master_keys("master_key = \"abc\"\n"),
        vec![(1, "abc")]
    );
    assert_eq!(
        config_master_keys("# master_key = \"abc\"\n"),
        vec![(1, "abc")]
    );
    assert_eq!(config_master_keys("master_key = \"\"\n"), vec![(1, "")]);
    assert!(config_master_keys("master_keyring = \"abc\"\n").is_empty());

    // 端到端：非法默认值会被判违规，合法/必填/命令替换不会。
    let bad = format!("- {ENV_KEY}=${{{ENV_KEY}:-dev-master-key-x}}\n");
    assert!(
        !env_assignments(&bad)
            .iter()
            .all(|(_, f)| matches!(f, Form::Required | Form::NotALiteral)),
        "非法默认值必须被判为违规形态：{:?}",
        env_assignments(&bad)
    );
}

#[test]
fn the_version_scanners_actually_see_the_copies_they_guard() {
    // 期望值确实**从 Cargo.toml 派生**（与编译期版本同源），不是抄来的常量。
    let ver = declared_version();
    let msrv = declared_msrv();
    assert_eq!(ver, env!("CARGO_PKG_VERSION"), "期望值应与编译期版本同源");
    assert!(looks_like_version(&ver), "`version` 应是 x.y.z：{ver:?}");
    assert!(
        looks_like_version(&msrv),
        "`rust-version` 应是 x.y：{msrv:?}"
    );
    // 同名字段按段收窄：依赖表里的 `version = "0.7"` 不得顶替 `[package]` 的版本。
    assert!(
        !FILES
            .iter()
            .any(|(_, s)| s.contains("axum") && s.contains("0.7.28")),
        "依赖段不得被当成期望来源"
    );

    // 扫描器必须**真的看见**那些行 —— 否则「改名/改路径后扫到 0 条」也会「通过」。
    let compose = FILES
        .iter()
        .find(|(n, _)| *n == "docker-compose.yml")
        .unwrap()
        .1;
    let tags = image_tags(compose, "aitokenpool");
    assert_eq!(tags.len(), 1, "compose 里应恰好 1 处镜像 tag：{tags:?}");
    assert!(
        compose
            .lines()
            .nth(tags[0].0 - 1)
            .is_some_and(|l| l.contains("image:")),
        "被扫到的那处应是生效的 `image:` 字段：{tags:?}"
    );
    assert_eq!(
        version_comments(compose).len(),
        1,
        "compose 头部应有 1 处版本标注"
    );

    let dockerfile = FILES.iter().find(|(n, _)| *n == "Dockerfile").unwrap().1;
    assert_eq!(
        image_tags(dockerfile, "aitokenpool").len(),
        2,
        "Dockerfile 的 build/run 示例各有一处 `aitokenpool:…`"
    );
    assert_eq!(
        version_comments(dockerfile).len(),
        1,
        "Dockerfile 头部应有 1 处版本标注"
    );
    let froms = rust_base_images(dockerfile);
    assert_eq!(
        froms.len(),
        1,
        "Dockerfile 应有 1 处 `FROM rust:…`：{froms:?}"
    );

    // README 用的是移动 tag（`latest`），不是版本声明 —— 不得被当成违规。
    for name in ["README.md", "README.en.md"] {
        let src = FILES.iter().find(|(n, _)| *n == name).unwrap().1;
        assert!(
            image_tags(src, "aitokenpool").is_empty(),
            "{name} 的 tag 是 `latest`，不该被判为版本声明"
        );
    }
}

#[test]
fn the_version_scanners_flag_the_stale_shapes() {
    // 什么算「版本号」：`latest` / 空 / 非数字串都不是。
    assert!(looks_like_version("0.6.6"));
    assert!(looks_like_version("1.86"));
    assert!(!looks_like_version("latest"));
    assert!(!looks_like_version(""));
    assert!(!looks_like_version("atp-data"));
    assert!(!looks_like_version(".1"));

    // 镜像 tag：移动 tag 跳过；YAML 服务名（`aitokenpool:` 后无版本）不得被算进去。
    assert!(image_tags("ghcr.io/argszero/aitokenpool:latest", "aitokenpool").is_empty());
    assert_eq!(
        image_tags("    image: aitokenpool:0.6.6", "aitokenpool"),
        vec![(1, "0.6.6".to_string())]
    );
    assert!(image_tags("  aitokenpool:\n", "aitokenpool").is_empty());

    // 版本标注：两种括号都要认。
    assert_eq!(
        version_comments("# …（v0.6.6）"),
        vec![(1, "0.6.6".to_string())]
    );
    assert_eq!(
        version_comments("# … (v0.7.28)"),
        vec![(1, "0.7.28".to_string())]
    );
    assert!(version_comments("(v) x\n").is_empty());

    // 构建镜像：低于声明 ⇒ 违规；等于/高于 ⇒ 合规（用更新工具链构建是合法的）。
    assert_eq!(
        rust_base_images("FROM rust:1.86-slim AS builder"),
        vec![(1, "1.86".to_string())]
    );
    assert!(rust_base_images("FROM debian:bookworm-slim AS runtime").is_empty());
    assert!(at_least("1.86", "1.86"));
    assert!(at_least("1.86.0", "1.86"));
    assert!(at_least("1.90", "1.86"));
    assert!(!at_least("1.85", "1.86"));

    // 规则本身有牙：拿**合规**的合成输入，各自改成陈旧值 ⇒ 必须报违规。
    // （合成输入而非活文件：规则与「活文件此刻是否已修」是两件事，不该互相污染读数。）
    let ver = declared_version();
    let msrv = declared_msrv();
    let ok =
        format!("    image: aitokenpool:{ver}\n# （v{ver}）\nFROM rust:{msrv}-slim AS builder\n");
    assert!(
        version_violations_of(&[("synthetic", &ok)]).is_empty(),
        "合规输入不得报违规"
    );

    let cases: &[(&str, &str)] = &[
        (&format!("aitokenpool:{ver}"), "aitokenpool:0.0.1"),
        (&format!("（v{ver}）"), "（v0.0.1）"),
        (&format!("FROM rust:{msrv}-slim"), "FROM rust:1.0-slim"),
    ];
    for (from, to) in cases {
        let stale = ok.replace(from, to);
        assert_ne!(stale, ok, "synthetic 替换必须真的发生：{from:?}");
        let bad = version_violations_of(&[("synthetic", &stale)]);
        assert!(
            bad.iter().any(|m| m.contains("0.0.1") || m.contains("1.0")),
            "{to:?} 必须被判违规：{bad:?}"
        );
    }
}
