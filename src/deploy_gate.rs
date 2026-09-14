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
