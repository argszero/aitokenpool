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
//!
//! # 第三条规则（R82）：`CHANGELOG.md` 的最新 `## v<semver>` 标题必须等于 `Cargo.toml` 声明的版本
//!
//! 发行链上共有**四个**版本事实载体，前三个已各有守卫：`Cargo.toml` 是运行时真源
//!（`/healthz` 自报 `CARGO_PKG_VERSION`）、`Dockerfile` / `docker-compose.yml` 里的副本由
//! 上文第二条规则守、发行 **tag** 由 `.github/workflows/docker-publish.yml` 守 —— **只剩
//! `CHANGELOG.md` 的最新 `## v<semver>` 标题没有任何执行者**（它在 23/23 个 tag 上一直正确，
//! 但从未被断言过，与 R76 的 MSRV、R80 的 tag 同形）。R78 之后发行 PR 要改的文件从 3 个变成
//! 5 个：漏改部署产物有第二条规则抓、tag 不符有 docker-publish 抓，**CHANGELOG 的标题没人抓**。
//!
//! 为什么标题**不算散文**（第二条规则明确把散文划出射程）：它处在文件里机器可读的**结构位**，
//! 而这个文件的整体职责就是陈述版本。因此**只守标题、不守正文** —— 正文里的资产令牌枚举之类
//! 仍是散文，走「修数据、不上门禁」。实现与两条伴生测试在文件末尾。
//!
//! # 第四条规则（R84）：编译期内嵌（`include_str!`）的目标必须落在 **Docker 构建上下文**里
//!
//! `include_str!` 是**编译期**读文件：目标不在构建上下文里，`docker build` 就直接失败
//!（`couldn't read …`）。而上下文由**两个**东西共同定义 —— Dockerfile 的 `COPY` 源
//!（哪些路径进得了构建机）与 `.dockerignore`（哪些路径被挡回去）。缺任一条都会漏掉一类。
//!
//! 这条属性此前**没有执行者**：`src/**` 的内嵌站点里，发布期可达的只有 `main.rs` 的那一处
//!（`COPY config ./config` 正是为它加的 —— 理由只写在 `Dockerfile` 的注释里，而**注释不是
//! 执行者**），其余**全部**落在 `#[cfg(test)]` 模块里，安全的唯一原因就是那个属性
//!（R84 落地时实测 **7 处**落在上下文之外、无一例外全在测试期模块里：本文件 5 处
//!〔`docker-compose.yml` / `Dockerfile` / 两份 README / `CHANGELOG.md`〕，`state_gate.rs`
//! 1 处〔引导原型 HTML〕，加上本条规则自己引的 `../.dockerignore`）。
//!
//! 它与 R76（MSRV）/ R80（发行 tag）/ R82（CHANGELOG 标题）是同一族的又一员：**声明有消费者、
//! 没有执行者**；而且失败点正好落在发行链上 —— `docker-publish.yml` 的 publish job 跑的是真
//! `docker build`（打 tag 那步会红），README 首推的 `docker compose up -d --build` 同样会失败，
//! 而 **CI 从不 `docker build`** ⇒ 平时全绿，只有发行/部署才炸。
//!
//! **断言**：每一处内嵌，要么目标在上下文里，要么该站点**发布期不可达** —— 所在模块是
//! `main.rs` 声明的 `#[cfg(test)]` 模块，或站点落在该文件自己的 `#[cfg(test)]` 之后。
//! 名册从 `src/main.rs` **派生**（不写快照），语料是**运行期走盘** `src/**/*.rs`
//!（`CARGO_MANIFEST_DIR`，与 `body_limit_gate.rs::source_files` 同型）⇒ 明天新增的源文件
//! 自动进入射程，没有需要同步的清单。
//!
//! **射程（词法，如实记录）**：本规则判的是「该文件的路径进不进得了上下文」与「该站点发布期
//! 是否可达」—— 它**不**证明 `docker build` 真能跑（那要 Linux 容器，与 R76 同款缺口）、
//! **不**校验 `COPY` 的目标路径写对没有、也**不**实现 `.dockerignore` 的 `**` 与取反（`!`）
//! 语义（本仓现用的那 10 行里没有这两者）。实现与三条测试（轴 + 阳性对照 + 规则牙齿）在文件末尾。

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

// ---------------------------------------------------------------------------
// 第三条规则（R82）：CHANGELOG 的最新 `## v<semver>` 标题必须等于 `Cargo.toml` 的 `version`
// ---------------------------------------------------------------------------
//
// 设计动机与射程见文件头的模块文档。要点：期望值**从 `Cargo.toml` 派生**
//（`declared_version()`，不写快照），`include_str!` 编译期读入，**零新文件零新依赖** ——
// 与上一条规则同型。射程只到**标题**：正文（资产令牌枚举等散文）不参与判定。

/// `CHANGELOG.md` 原文 —— 本文件的最新版本标题是发行链上第四个版本事实载体。
const CHANGELOG: &str = include_str!("../CHANGELOG.md");

/// `## v<semver>` 形态的版本标题（`##` 级 ＋ `v` ＋ `x.y.z`，行尾可另有日期等）。
///
/// 只认 `##` 级：`# Changelog` 是文件题头、`### …` 是条目内的小节标题，都不是版本声明。
fn version_headings(src: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let Some(rest) = line.trim_start().strip_prefix("##") else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('v') else {
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

/// 最新（文件里第一条）版本标题。
fn latest_changelog_version(src: &str) -> Option<(usize, String)> {
    version_headings(src).into_iter().next()
}

/// CHANGELOG 版本标题的违规项（人类可读）。空 = 通过。
fn changelog_violations_of(src: &str) -> Vec<String> {
    let ver = declared_version();
    match latest_changelog_version(src) {
        None => vec![format!(
            "CHANGELOG.md: 找不到任何 `## v<semver>` 标题（期望最新条目是 v{ver}）"
        )],
        Some((ln, v)) if v != ver => vec![format!(
            "CHANGELOG.md:{ln}: 最新条目标题是 v{v}，而 Cargo.toml 声明的是 {ver}（发行链上的版本副本没跟上）"
        )],
        Some(_) => Vec::new(),
    }
}

fn changelog_violations() -> Vec<String> {
    changelog_violations_of(CHANGELOG)
}

#[test]
fn the_changelog_heading_carries_the_version_the_manifest_declares() {
    let bad = changelog_violations();
    assert!(
        bad.is_empty(),
        "CHANGELOG.md 的最新版本标题必须与 Cargo.toml 一致：\n{}",
        bad.join("\n")
    );
}

#[test]
fn the_changelog_heading_scanner_actually_sees_the_heading_it_guards() {
    // 阳性对照：扫描器若因改名/改格式而返回空集，上面那条会在空集上「通过」。
    let headings = version_headings(CHANGELOG);
    assert!(
        !headings.is_empty(),
        "CHANGELOG 里应至少有一个 `## v<semver>` 标题"
    );
    let (ln, v) = latest_changelog_version(CHANGELOG).expect("最新标题存在");
    assert!(ln <= 10, "最新版本标题应在文件顶部几行内，实际第 {ln} 行");
    // 被扫到的那行确实是 `## v…` 标题（防「扫到正文里的数字」）。
    let line = CHANGELOG.lines().nth(ln - 1).expect("标题行存在");
    assert!(
        line.starts_with("## v"),
        "被扫到的行应是 `## v…` 标题：{line:?}"
    );
    // 扫描器解出的确实是一个版本号（防「解出空串也照样不比」）。
    // ⚠️ 这里**刻意不**与 `declared_version()` 比较，也不检查标题是否重复：「标题 == 清单版本」
    // 这条主张只能有**一个** owner（上面那条门禁）；否则同一处陈旧会让两条测试同时红，
    // 诊断时看不出谁在报，且报的那条会显得比它守的规则更宽。
    assert!(
        looks_like_version(&v),
        "扫描器应解出 x.y.z 形态的版本：{v:?}"
    );
}

#[test]
fn the_changelog_heading_rule_flags_a_stale_heading() {
    // 规则本身有牙：用**合成输入**（规则与「活文件此刻是否已修」是两件事，不该互相污染读数）。
    let ver = declared_version();
    let ok = format!("# Changelog\n\n## v{ver} (2026-01-01)\n\n- x\n\n## v0.0.1 (2025-01-01)\n");
    assert!(
        changelog_violations_of(&ok).is_empty(),
        "合规输入不得报违规：{:?}",
        changelog_violations_of(&ok)
    );

    let stale = ok.replace(&format!("## v{ver}"), "## v9.9.9");
    assert_ne!(stale, ok, "synthetic 替换必须真的发生");
    let bad = changelog_violations_of(&stale);
    assert!(
        bad.iter().any(|m| m.contains("v9.9.9")),
        "陈旧标题必须被判违规：{bad:?}"
    );

    // 「扫到 0 条」不得等同于「合规」。
    assert!(
        !changelog_violations_of("# Changelog\n\nno headings here\n").is_empty(),
        "没有版本标题时必须报违规，而不是静默通过"
    );

    // 形态：只认 `##` 级；`###` 小节与 `## not-a-version` 都不算。
    assert_eq!(
        version_headings("## v1.2.3 (x)\n### v9.9.9\n## not-a-version\n## v1.2.3\n"),
        vec![(1, "1.2.3".to_string()), (4, "1.2.3".to_string())]
    );
}

// ---------------------------------------------------------------------------
// 第四条规则（R84）：编译期内嵌（`include_str!`）的目标必须在 Docker 构建上下文里
// ---------------------------------------------------------------------------
//
// 动机与射程见文件头的模块文档。要点：
// - **上下文有两个决定者**：Dockerfile 的 `COPY` 源与 `.dockerignore` 的模式。只看一个
//   会漏掉一整类 —— 两份 README 与 `CHANGELOG.md` 是被 `*.md` 拿掉的，而
//   `docker-compose.yml` / `Dockerfile` 是**根本没有** `COPY` 源。
// - **名册与语料都不写快照**：`#[cfg(test)]` 归属从 `src/main.rs` **派生**；语料**走盘**。
// - 走盘而不是手写 `include_str!` 清单：手写清单需要额外的「清单完整性」守卫，
//   `read_dir` 天然覆盖新文件（同 `body_limit_gate.rs::source_files`）。

/// `.dockerignore` 原文 —— 构建上下文的第二个决定者。
const DOCKERIGNORE: &str = include_str!("../.dockerignore");

/// 词法掩码：把注释（以及可选的字符串正文）替换成**等长**空格 —— **字节长度不变**，
/// 因此偏移量与原文一一对应（报行号、比生产区都靠这个）。
///
/// - `mask_strings = false`：只掩注释（`//` 与可嵌套的 `/* */`）。
/// - `mask_strings = true` ：连字符串/字符字面量的正文一起掩 —— 用来判断某个关键字是否
///   落在**代码**位置（字符串里出现的同名字面量不是站点）。
///
/// ⚠️ 为什么不能按行截断第一个 `//`：`"https://…"` 里的 `//` 不是注释，而本仓 `src` 里
/// 就有这种字符串字面量；注释优先于字符串，两者必须在同一个状态机里判。
fn code_mask(src: &str, mask_strings: bool) -> String {
    let b = src.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                out[i] = b' ';
                i += 1;
            }
        } else if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
            let mut depth = 0usize;
            while i < b.len() {
                if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                    depth += 1;
                    out[i] = b' ';
                    out[i + 1] = b' ';
                    i += 2;
                } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                    depth -= 1;
                    out[i] = b' ';
                    out[i + 1] = b' ';
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if b[i] != b'\n' {
                        out[i] = b' ';
                    }
                    i += 1;
                }
            }
        } else if let Some(len) = raw_string_len(&src[i..]) {
            if mask_strings {
                blank(&mut out, i, i + len);
            }
            i += len;
        } else if b[i] == b'"' {
            let len = string_len(&src[i..]).unwrap_or(b.len() - i);
            if mask_strings {
                blank(&mut out, i, i + len);
            }
            i += len;
        } else if b[i] == b'\'' {
            match char_literal_len(&src[i..]) {
                Some(len) => {
                    if mask_strings {
                        blank(&mut out, i, i + len);
                    }
                    i += len;
                }
                None => i += 1, // 生命周期标注（`&'static str`）不是字面量
            }
        } else {
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 把 `[from, to)` 的字节换成空格（**不碰换行**，行号才不漂）。
fn blank(out: &mut [u8], from: usize, to: usize) {
    let end = to.min(out.len());
    for x in out[..end].iter_mut().skip(from) {
        if *x != b'\n' {
            *x = b' ';
        }
    }
}

/// 原始字符串 `r"…"` / `r#"…"#` / `br#"…"#` 的**整段**长度（含起止引号与井号）。
fn raw_string_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = if b.first() == Some(&b'b') { 1 } else { 0 };
    if b.get(i) != Some(&b'r') {
        return None;
    }
    i += 1;
    let mut hashes = 0usize;
    while b.get(i) == Some(&b'#') {
        hashes += 1;
        i += 1;
    }
    if b.get(i) != Some(&b'"') {
        return None;
    }
    i += 1;
    let mut j = i;
    while j < b.len() {
        if b[j] == b'"' {
            let end = j + 1 + hashes;
            if end <= b.len() && b[j + 1..end].iter().all(|c| *c == b'#') {
                return Some(end);
            }
        }
        j += 1;
    }
    Some(b.len()) // 未闭合：吃到尾（这样的源码本来就编不过）
}

/// 普通字符串 `"…"` 的整段长度（含两侧引号；`\"` 不结束）。
fn string_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.first() != Some(&b'"') {
        return None;
    }
    let mut i = 1usize;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return Some(i + 1),
            _ => i += 1,
        }
    }
    Some(b.len())
}

/// 字符字面量 `'x'` / `'\n'` / `'"'` / `'中'` 的整段长度；`None` = 不是字面量。
fn char_literal_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.first() != Some(&b'\'') {
        return None;
    }
    if b.get(1) == Some(&b'\\') {
        let mut i = 2usize;
        while i < b.len() && i < 8 {
            if b[i] == b'\'' {
                return Some(i + 1);
            }
            i += 1;
        }
        return None;
    }
    let c = s.get(1..)?.chars().next()?;
    if c == '\'' {
        return None;
    }
    let end = 1 + c.len_utf8();
    if b.get(end) == Some(&b'\'') {
        Some(end + 1)
    } else {
        None
    }
}

/// `src/**/*.rs`：(仓库相对路径, 文本)。运行期走盘（`CARGO_MANIFEST_DIR`），不依赖工作目录。
///
/// ⚠️ **不**跳过 `*_gate.rs`（与 `body_limit_gate.rs::source_files` 不同）：本规则要判的
/// 正是那些门禁模块自己的内嵌站点，跳过它们等于把要守的东西从语料里删掉。
fn rust_sources() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<(String, String)>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, root, out);
            } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
                let rel = p
                    .strip_prefix(root)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .replace('\\', "/");
                if let Ok(text) = std::fs::read_to_string(&p) {
                    out.push((format!("src/{rel}"), text));
                }
            }
        }
    }
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    walk(&root, &root, &mut out);
    out.sort();
    out
}

/// `include_str!` / `include_bytes!` 的站点：`(行号, 字节偏移, 目标字面量)`。
///
/// **只在代码位置认定站点**：注释里的（本仓 8 处）与字符串正文里的同名字面量都不算。
/// 后者不是假想 —— 本文件自己的测试夹具就是字符串里的 `include_str!("…")`：R84 实测，
/// 「按行截断 `//`」的旧写法把它们读成站点，还读出 `…CHANGELOG.md\` 这种带反斜杠的目标。
fn include_sites(src: &str) -> Vec<(usize, usize, String)> {
    let no_comments = code_mask(src, false);
    let code_only = code_mask(src, true);
    let mut out = Vec::new();
    for kw in ["include_str!(", "include_bytes!("] {
        let mut from = 0usize;
        while let Some(rel) = no_comments[from..].find(kw) {
            let at = from + rel;
            // 关键字本身必须落在代码里：字符串正文在 code_only 里已被掩成空格
            if code_only.as_bytes().get(at) != Some(&b' ') {
                if let Some(target) = literal_at(src, at + kw.len()) {
                    let line = src[..at].matches('\n').count() + 1;
                    out.push((line, at, target));
                }
            }
            from = at + kw.len();
        }
    }
    out.sort_by_key(|(_, off, _)| *off);
    out
}

/// 从 `pos` 起解析一个字面量实参（跳过空白；支持 `"…"` 与 `r#"…"#`）。
///
/// 认不出来的形态（如 `b"…"`）返回 `None` ⇒ 该站点不入名册 —— 所以阳性对照断言的是
/// **站点的准确条数**，任何形态漏读都会让那条断言变红。
fn literal_at(src: &str, pos: usize) -> Option<String> {
    let rest = src.get(pos..)?;
    let trimmed = rest.trim_start();
    if let Some(len) = raw_string_len(trimmed) {
        let raw = trimmed.get(..len)?;
        let q = raw.find('"')?;
        let hashes = raw[..q].chars().filter(|c| *c == '#').count();
        let body_end = raw.len().checked_sub(1 + hashes)?;
        return Some(raw[q + 1..body_end].to_string());
    }
    if trimmed.starts_with('"') {
        let len = string_len(trimmed)?;
        let body = trimmed.get(1..len.checked_sub(1)?)?;
        return Some(body.replace("\\\"", "\"").replace("\\\\", "\\"));
    }
    None
}

/// 生产区（最后一个行首 `#[cfg(test)]` 之前）在**掩码文本**里的长度。
///
/// 切割规则与 `body_limit_gate.rs::production_region` 刻意一致：两个门禁对「生产区」不该
/// 有两个口径。（取**最后**一个而非第一个 —— 顶格的 `#[cfg(test)] mod X;` 出现在 `main.rs` 顶部。）
fn release_region_len(masked: &str) -> usize {
    match masked.rfind("\n#[cfg(test)]") {
        Some(i) => i,
        None => masked.len(),
    }
}

/// `main.rs` 里带 `#[cfg(test)]` 的模块名（`mod X;` 的**紧前一行**是 `#[cfg(test)]`）。
///
/// 块模块（`mod tests {`）不收集 —— 它没有对应文件，是文件内的测试模块。
fn test_only_modules(main_rs: &str) -> Vec<String> {
    let lines: Vec<&str> = main_rs.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(name) = line
            .trim()
            .strip_prefix("mod ")
            .and_then(|r| r.strip_suffix(';'))
        else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        if i > 0 && lines[i - 1].trim() == "#[cfg(test)]" {
            out.push(name.to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

/// 文件所属的**顶层**模块名（`src/routes/admin.rs` → `routes`；`src/deploy_gate.rs` → `deploy_gate`）。
fn top_module(rel: &str) -> String {
    let p = rel.strip_prefix("src/").unwrap_or(rel);
    match p.split_once('/') {
        Some((first, _)) => first.to_string(),
        None => p.trim_end_matches(".rs").to_string(),
    }
}

/// 以 `base_dir` 为基准解析内嵌的相对路径（Rust 语义：`include_str!` 相对**包含者所在目录**）。
fn resolve_include(base_dir: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in base_dir.split('/').chain(rel.split('/')) {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// Dockerfile 的构建上下文 `COPY` 源。跳过 `COPY --from=…`（跨阶段拷贝不读上下文）。
fn copy_sources(dockerfile: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in dockerfile.lines() {
        let Some(rest) = line.trim().strip_prefix("COPY ") else {
            continue;
        };
        let toks: Vec<&str> = rest.split_whitespace().collect();
        if toks
            .iter()
            .any(|t| t.trim_start_matches('-').starts_with("from="))
        {
            continue; // 源在上一阶段，不在构建上下文里
        }
        let args: Vec<&str> = toks.into_iter().filter(|t| !t.starts_with("--")).collect();
        if args.len() < 2 {
            continue; // 至少一个源 + 一个目标
        }
        for s in &args[..args.len() - 1] {
            out.push(s.trim_start_matches("./").to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

/// `.dockerignore` 的模式（跳过空行与 `#` 注释）。
fn ignore_patterns(src: &str) -> Vec<String> {
    src.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// 单个模式是否命中该路径（Docker 语义）。
///
/// `*` 与 `?` **不跨 `/`** —— 所以 `.dockerignore` 里的 `*.md` 只拿掉根级的 `*.md`，
/// 而 `ui/README.md` 靠 `COPY ui ./ui` 照样进上下文。命中某个**目录**即命中其下全部，
/// 故模式可以命中路径本身，也可以命中它的任一祖先。
fn pattern_matches(pat: &str, path: &str) -> bool {
    let pat = pat.trim().trim_end_matches('/');
    if pat.is_empty() || pat.starts_with('#') {
        return false;
    }
    let segs: Vec<&str> = pat.split('/').collect();
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    (1..=parts.len())
        .any(|k| segs.len() == k && segs.iter().zip(&parts[..k]).all(|(p, s)| seg_glob(p, s)))
}

/// 单段通配匹配（`*` / `?`，均不跨段 —— 调用方已按 `/` 切好）。
fn seg_glob(pat: &str, s: &str) -> bool {
    let (p, t) = (pat.as_bytes(), s.as_bytes());
    let (mut i, mut j, mut star, mut mark) = (0usize, 0usize, None::<usize>, 0usize);
    while j < t.len() {
        if i < p.len() && (p[i] == b'?' || p[i] == t[j]) {
            i += 1;
            j += 1;
        } else if i < p.len() && p[i] == b'*' {
            star = Some(i);
            mark = j;
            i += 1;
        } else if let Some(sp) = star {
            i = sp + 1;
            mark += 1;
            j = mark;
        } else {
            return false;
        }
    }
    while i < p.len() && p[i] == b'*' {
        i += 1;
    }
    i == p.len()
}

/// 该路径是否落在构建上下文里 = **有 `COPY` 源覆盖** ∧ **未被 `.dockerignore` 命中**。
fn in_build_context(path: &str, copies: &[String], patterns: &[String]) -> bool {
    let covered = copies.iter().any(|c| {
        let c = c.trim_end_matches('/');
        !c.is_empty() && (path == c || path.starts_with(&format!("{c}/")))
    });
    covered && !patterns.iter().any(|p| pattern_matches(p, path))
}

/// 越界内嵌的违规项（人类可读）。空 = 通过。
///
/// 四个入参（语料 / `COPY` 源 / ignore 模式 / cfg(test) 名册）都可注入，好让合成语料单独
/// 测量**规则本身** —— 规则与「活树此刻是否合规」是两件事，不该互相污染读数。
fn include_context_violations(
    files: &[(String, String)],
    copies: &[String],
    patterns: &[String],
    test_only: &[String],
) -> Vec<String> {
    let mut bad = Vec::new();
    for (rel, src) in files {
        let text = code_mask(src, false);
        let prod = release_region_len(&text);
        let dir = match rel.rfind('/') {
            Some(i) => &rel[..i],
            None => "",
        };
        if test_only.iter().any(|m| *m == top_module(rel)) {
            continue; // 整个模块仅测试期编译 ⇒ 发布构建根本看不到它的内嵌
        }
        for (line, off, target) in include_sites(src) {
            if off >= prod {
                continue; // 落在该文件自己的 `#[cfg(test)]` 之后 ⇒ 发布期不可达
            }
            let resolved = resolve_include(dir, &target);
            if !in_build_context(&resolved, copies, patterns) {
                bad.push(format!(
                    "{rel}:{line}: {target} → {resolved} 不在 Docker 构建上下文里，而 {rel} 属发布期模块（发布构建会编译到它，构建机会因缺该文件而失败）"
                ));
            }
        }
    }
    bad
}

/// 活树上的四组输入：语料 / `COPY` 源 / ignore 模式 / cfg(test) 名册。
struct BuildContextInputs {
    files: Vec<(String, String)>,
    copies: Vec<String>,
    patterns: Vec<String>,
    test_only: Vec<String>,
}

fn build_context_inputs() -> BuildContextInputs {
    let files = rust_sources();
    let dockerfile = FILES
        .iter()
        .find(|(n, _)| *n == "Dockerfile")
        .expect("FILES 里应有 Dockerfile")
        .1;
    let main_rs = files
        .iter()
        .find(|(r, _)| r == "src/main.rs")
        .map(|(_, s)| s.as_str())
        .unwrap_or_default();
    let test_only = test_only_modules(main_rs);
    let copies = copy_sources(dockerfile);
    let patterns = ignore_patterns(DOCKERIGNORE);
    BuildContextInputs {
        files,
        copies,
        patterns,
        test_only,
    }
}

#[test]
fn every_compile_time_include_is_in_the_docker_build_context_or_test_only() {
    let ctx = build_context_inputs();
    let bad = include_context_violations(&ctx.files, &ctx.copies, &ctx.patterns, &ctx.test_only);
    assert!(
        bad.is_empty(),
        "发布期模块不得内嵌构建上下文之外的文件（`docker build` 会因缺该文件失败）：\n{}",
        bad.join("\n")
    );
}

#[test]
fn the_build_context_scanner_sees_the_includes_and_the_context_it_judges_them_by() {
    // 阳性对照：扫描器若因改名/改路径而返回空集，上面那条会在空集上「通过」。
    let ctx = build_context_inputs();
    let (files, copies, patterns, test_only) =
        (&ctx.files, &ctx.copies, &ctx.patterns, &ctx.test_only);
    assert!(
        files.len() >= 20,
        "只走到 {} 个源文件 —— 走盘本身坏了（空集上通过是假绿）",
        files.len()
    );
    let sites: usize = files.iter().map(|(_, s)| include_sites(s).len()).sum();
    assert!(sites >= 40, "只扫到 {sites} 处内嵌站点，远少于本仓实际数量");

    // 掩码器**在干活**（这条对照是派生出来的，不是快照）：原文里的关键字出现次数必须
    // **严格多于**代码位置的站点数 —— 差额正是注释里的（本仓十余行）与被字符串包住的
    // （本文件自己的夹具、以及本模块 `for kw in […]` 里那两串关键字本身就是字符串字面量）。
    let raw: usize = files
        .iter()
        .map(|(_, s)| s.matches("include_str!(").count() + s.matches("include_bytes!(").count())
        .sum();
    assert!(
        raw > sites,
        "掩码器没在过滤：原文 {raw} 处 vs 代码位置 {sites} 处"
    );

    // 读出来的每个目标都必须是**干净的路径字面量**。这是 R84 实测缺陷的回归断言：
    // 「按行截断 `//`」的旧写法把本文件的字符串夹具读成站点，读出的目标就带反斜杠。
    let self_src = files
        .iter()
        .find(|(r, _)| r == "src/deploy_gate.rs")
        .map(|(_, s)| s.as_str())
        .expect("语料里应有 src/deploy_gate.rs");
    assert!(
        self_src.matches("include_str!(\\\"").count() >= 3,
        "本文件的字符串夹具应仍在（否则反斜杠那条对照空转）"
    );
    for (rel, src) in files {
        for (line, _, t) in include_sites(src) {
            assert!(
                !t.is_empty() && !t.chars().any(|c| "\"\\; \n".contains(c)),
                "{rel}:{line} 读出的目标不像路径字面量：{t:?}"
            );
        }
    }

    // 上下文的两把尺子都要解析出来，且解析结果与实测一致。
    for want in ["src", "ui", "config", "Cargo.toml"] {
        assert!(
            copies.iter().any(|c| c == want),
            "上下文 COPY 源应含 {want}：{copies:?}"
        );
    }
    assert!(
        !copies.iter().any(|c| c.contains("target/release")),
        "`COPY --from=builder` 是跨阶段拷贝，不该被当成上下文源：{copies:?}"
    );
    assert!(
        patterns.len() >= 5 && patterns.iter().any(|p| p == "*.md"),
        ".dockerignore 的模式应被解析出来：{patterns:?}"
    );

    // 匹配语义：`*` 不跨 `/`（Docker 规则）。
    assert!(pattern_matches("*.md", "README.md"));
    assert!(!pattern_matches("*.md", "ui/README.md"));
    assert!(pattern_matches("docs/", "docs/prototype/x.html"));
    assert!(pattern_matches("config/config.toml", "config/config.toml"));
    assert!(!pattern_matches(
        "config/config.toml",
        "config/config.example.toml"
    ));
    assert!(
        in_build_context("ui/README.md", copies, patterns),
        "`ui/README.md` 在上下文里：`COPY ui ./ui` 覆盖它，而 `*.md` 不跨 `/`"
    );

    // 名册确实派生出来了：非空、不把块模块（`mod tests {`）当文件模块、且每个名字都真有文件。
    assert!(test_only.len() >= 5, "cfg(test) 名册应非空：{test_only:?}");
    assert!(
        !test_only.iter().any(|m| m == "tests"),
        "`mod tests {{` 是块模块，没有对应文件：{test_only:?}"
    );
    for m in test_only {
        assert!(
            files
                .iter()
                .any(|(rel, _)| rel == &format!("src/{m}.rs") || rel == &format!("src/{m}/mod.rs")),
            "名册里的 {m} 应在语料里有对应文件：{test_only:?}"
        );
    }

    // **非空转**：本仓至少有一处内嵌落在上下文之外 —— 否则这条规则是空转的（那才是假绿）。
    let outside = files
        .iter()
        .filter(|(rel, src)| {
            let dir = match rel.rfind('/') {
                Some(i) => &rel[..i],
                None => "",
            };
            include_sites(src)
                .iter()
                .any(|(_, _, t)| !in_build_context(&resolve_include(dir, t), copies, patterns))
        })
        .count();
    assert!(
        outside >= 1,
        "本仓应至少有一处内嵌落在上下文之外（否则该规则空转）；若它真的归零，请重新评估本规则"
    );
}

#[test]
fn the_build_context_rule_flags_a_release_module_that_embeds_outside_the_context() {
    // 规则本身有牙：合成输入（规则与「活树此刻是否合规」是两件事，不该互相污染读数）。
    let copies = vec![
        "Cargo.toml".to_string(),
        "src".to_string(),
        "ui".to_string(),
        "config".to_string(),
    ];
    let patterns = vec!["docs/".to_string(), "*.md".to_string()];
    let files = |src: &str| vec![("src/foo.rs".to_string(), src.to_string())];

    let outside = "const A: &str = include_str!(\"../CHANGELOG.md\");\n";
    // 发布期模块 + 越界目标 ⇒ 报违规。
    let bad = include_context_violations(&files(outside), &copies, &patterns, &[]);
    assert_eq!(bad.len(), 1, "越界内嵌必须被抓到：{bad:?}");
    assert!(
        bad[0].contains("src/foo.rs:1") && bad[0].contains("CHANGELOG.md"),
        "{bad:?}"
    );
    // 同一个站点，模块改为 `#[cfg(test)]` ⇒ 放行。
    let ok = include_context_violations(&files(outside), &copies, &patterns, &["foo".to_string()]);
    assert!(ok.is_empty(), "仅测试期模块不得报违规：{ok:?}");
    // 目标在上下文里 ⇒ 发布期模块也放行。
    let inside = "const B: &str = include_str!(\"../ui/js/app.js\");\n";
    assert!(include_context_violations(&files(inside), &copies, &patterns, &[]).is_empty());
    // 站点落在该文件自己的 `#[cfg(test)]` 之后 ⇒ 发布期不可达，放行。
    let inline =
        "fn f() {}\n#[cfg(test)]\nmod tests {\n    const C: &str = include_str!(\"../CHANGELOG.md\");\n}\n";
    assert!(include_context_violations(&files(inline), &copies, &patterns, &[]).is_empty());
    // 注释里的 `include_str!` 不是站点。
    let commented = "// const D: &str = include_str!(\"../CHANGELOG.md\");\n";
    assert!(include_context_violations(&files(commented), &copies, &patterns, &[]).is_empty());
    // 被 `.dockerignore`（目录前缀）挡掉的路径同样是越界。
    let docs = "const E: &str = include_str!(\"../docs/x.html\");\n";
    assert_eq!(
        include_context_violations(&files(docs), &copies, &patterns, &[]).len(),
        1,
        "`docs/` 之下的目标必须报违规"
    );
}
