//! SMTP 端口门禁（R127）：**示例配置**与 **serde 默认值**里的 `smtp_port` 必须等于
//! 发信实现真正使用的那个 TLS 模式所要求的端口。
//!
//! 起因：`63fc76fd`（2026-08-20）把发信从 STARTTLS 切成 implicit TLS
//!（`SmtpTransport::relay`），提交信息写着「修复：端口改 465」—— 但那次提交**只改了散文**
//!（`config.example.toml` 的说明行与 `mail.rs` 文件头），**两个真正携带端口值的载体一个没动**：
//! 示例草稿仍是 `smtp_port = 587`，`config.rs::default_smtp_port()` 仍返回 587。
//! 于是照抄示例（或省略该键、走 serde 默认）的用户会得到「对 587 立即做 TLS 握手」
//! ⇒ Gmail 回明文 ⇒ rustls 报 `InvalidContentType` ⇒ 验证码发不出（注册 502）。
//! 与 `deploy_gate.rs` 第二条规则同判：**跨格式的副本能抄，前提是有人守**。
//!
//! # 期望值**派生**，不写快照
//!
//! 真源是**实现自己选用的传输模式**（`src/mail.rs` 的**代码**）：
//!
//! | `mail.rs` 代码里的构造 | 模式 | 期望端口 |
//! |---|---|---|
//! | `SmtpTransport::relay(` | implicit TLS | `lettre::transport::smtp::SUBMISSIONS_PORT`（465） |
//! | `builder_dangerous(` | STARTTLS | `lettre::transport::smtp::SUBMISSION_PORT`（587） |
//!
//! 两个常量都取自 `lettre`（本仓已启用 `smtp-transport` feature）⇒ **端口不写死**；
//! 将来若换成 STARTTLS，门禁会要求那两个载体跟着改成 587。
//!
//! # ⚠️ 两处提取器的处理方式**相反**（本门禁最容易写错的地方）
//!
//! - 读 `config.example.toml` 时，`smtp_port` 那一行**本身就是注释**（示例即草稿）
//!   ⇒ **必须读注释行**（照 `deploy_gate::config_master_keys` 的做法，剥一个前缀 `#`）。
//! - 读 `mail.rs` / `config.rs` 时**必须先清空注释** —— 否则 `mail.rs` 文件头那句
//!   「若要用 587 STARTTLS 需改用 `builder_dangerous(…)`」与 `config.rs` 的文档注释
//!   都会被当成代码，模式判定会同时看到两种模式、默认值提取器会读到注释里的数字。
//!
//! 设计约束（与 `i18n_pack` / `catalog_gate` / `deploy_gate` 同型）：
//! **仅测试期编译**、**零新依赖**、**编译期读入**（不依赖工作目录）。
//!
//! 射程：只证「这两个值载体与实现选用的模式一致」—— **不**证 SMTP 真能连通（无网络仪器），
//! 也**不**校验 `mail.rs` 头部的散文。散文属 `doc-comment-claims` 轴（修数据、不上门禁）；
//! 若将来把 `relay` 换成 STARTTLS，这条门禁会先红，提醒同时更新散文。

use lettre::transport::smtp::{SUBMISSIONS_PORT, SUBMISSION_PORT};

/// 编译期读入的三份载体。
const MAIL_RS: &str = include_str!("mail.rs");
const CONFIG_RS: &str = include_str!("config.rs");
const CONFIG_EXAMPLE: &str = include_str!("../config/config.example.toml");

/// 实现选用的 SMTP TLS 模式 —— 由 `mail.rs` 的**代码**判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TlsMode {
    /// `SmtpTransport::relay`：连接后立即 TLS 握手 ⇒ 提交口 465。
    Implicit,
    /// `builder_dangerous(…).tls(Tls::Opportunistic(…))`：先明文、再 STARTTLS ⇒ 587。
    StartTls,
}

impl TlsMode {
    /// 该模式在 `lettre` 里的默认提交端口 —— **派生**，不写快照。
    fn expected_port(self) -> u16 {
        match self {
            TlsMode::Implicit => SUBMISSIONS_PORT,
            TlsMode::StartTls => SUBMISSION_PORT,
        }
    }

    fn what(self) -> &'static str {
        match self {
            TlsMode::Implicit => "implicit TLS（SmtpTransport::relay）",
            TlsMode::StartTls => "STARTTLS（builder_dangerous）",
        }
    }
}

/// 从 `mail.rs` 的代码（注释已清空）判定传输模式。
fn tls_mode(mail_code: &str) -> Option<TlsMode> {
    if mail_code.contains("SmtpTransport::relay(") {
        Some(TlsMode::Implicit)
    } else if mail_code.contains("builder_dangerous(") {
        Some(TlsMode::StartTls)
    } else {
        None
    }
}

/// `mail.rs` 确实把配置里的端口送进了 transport（`.port(…smtp_port…)`）——
/// 否则那两个值载体就算一致也没有意义（值是死的）。
fn forwards_configured_port(mail_code: &str) -> bool {
    let mut rest = mail_code;
    while let Some(pos) = rest.find(".port(") {
        let after = &rest[pos + ".port(".len()..];
        let end = after.find(')').unwrap_or(after.len());
        if after[..end].contains("smtp_port") {
            return true;
        }
        rest = &after[end..];
    }
    false
}

/// `config.example.toml` 里的每一处 `smtp_port = <n>`（**允许带 `#` 前缀** —— 那正是示例草稿的形态），
/// 返回 `(行号, 值)`。空集 = 提取器失效，门禁必须响亮失败而不是静默通过。
fn example_smtp_ports(src: &str) -> Vec<(usize, u16)> {
    let mut out = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let body = line.trim_start();
        // 示例草稿就是注释行 ⇒ 剥掉一个 `#` 再看。
        let body = body.strip_prefix('#').unwrap_or(body).trim_start();
        let Some(rest) = body.strip_prefix("smtp_port") else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('=') else {
            continue; // 例如 `smtp_port_backup` 之类，不是赋值
        };
        let value = rest.trim().trim_matches(|c| c == '"' || c == '\'');
        if let Ok(n) = value.parse::<u16>() {
            out.push((i + 1, n));
        }
    }
    out
}

/// `src/config.rs::default_smtp_port()` 的返回字面量（`(行号, 值)`）。
fn default_smtp_port(src: &str) -> Option<(usize, u16)> {
    let masked = mask_comments(src);
    let start = masked.find("fn default_smtp_port(")?;
    let body = &masked[start..];
    let open = body.find('{')?;
    let close = body[open..].find('}')? + open;
    let region = &body[open + 1..close];
    let ds = region.find(|c: char| c.is_ascii_digit())?;
    let digits: String = region[ds..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let n = digits.parse::<u16>().ok()?;
    let line = masked[..start + open + 1 + ds].matches('\n').count() + 1;
    Some((line, n))
}

/// 词法掩码：把注释替换成**等长**空格（字节长度不变 ⇒ 偏移与行号与原文一一对应）。
/// 字符串/字符字面量**整段跳过但不清空** —— `"https://…"` 里的 `//` 不是注释。
fn mask_comments(src: &str) -> String {
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
                    if i + 1 < out.len() {
                        out[i + 1] = b' ';
                    }
                    i += 2;
                } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                    depth -= 1;
                    out[i] = b' ';
                    if i + 1 < out.len() {
                        out[i + 1] = b' ';
                    }
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
        } else if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2; // 转义序列
                    continue;
                }
                if b[i] == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
        } else if b[i] == b'\'' {
            match char_literal_len(&b[i..]) {
                Some(len) => i += len,
                None => i += 1, // 生命周期标注（`&'static str`）不是字面量
            }
        } else {
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 从 `'` 起的**字符字面量**整段长度（含两侧引号）；生命周期标注返回 `None`。
fn char_literal_len(s: &[u8]) -> Option<usize> {
    let mut j = 1usize;
    if s.get(j) == Some(&b'\\') {
        j += 2; // `'\n'` / `'\''`
    } else {
        j += utf8_len(*s.get(j)?);
    }
    if s.get(j) == Some(&b'\'') {
        Some(j + 1)
    } else {
        None
    }
}

/// UTF-8 首字节给出的整个码点长度。
fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else {
        4
    }
}

#[test]
fn the_smtp_port_carriers_match_the_mode_the_sender_uses() {
    let mail_code = mask_comments(MAIL_RS);
    let mode = tls_mode(&mail_code).unwrap_or_else(|| {
        panic!(
            "找不到 SMTP 传输模式：`src/mail.rs` 的代码里既没有 `SmtpTransport::relay(` \
             也没有 `builder_dangerous(` —— 门禁的判据失效了，请更新 `tls_mode()`"
        )
    });
    let expected = mode.expected_port();

    let samples = example_smtp_ports(CONFIG_EXAMPLE);
    assert!(
        !samples.is_empty(),
        "`config/config.example.toml` 里找不到 `smtp_port = <n>` —— 门禁的提取器失效了"
    );
    let (df_line, df_port) = default_smtp_port(CONFIG_RS).unwrap_or_else(|| {
        panic!("`src/config.rs::default_smtp_port()` 里读不到返回值 —— 门禁的提取器失效了")
    });

    let mut wrong = Vec::new();
    for (line, port) in &samples {
        if *port != expected {
            wrong.push(format!(
                "config/config.example.toml:{line} 示例草稿写 `smtp_port = {port}`，应为 {expected}"
            ));
        }
    }
    if df_port != expected {
        wrong.push(format!(
            "src/config.rs:{df_line} `default_smtp_port()` 返回 {df_port}，应为 {expected}"
        ));
    }
    assert!(
        wrong.is_empty(),
        "`src/mail.rs` 用 {} ⇒ 提交口是 {expected}。但端口值载体与它不一致：\n  - {}\n\
         端口必须与实现选用的传输模式一致：照抄示例、或省略 `smtp_port` 走 serde 默认的用户，\
         会因协议不匹配而连接失败（rustls InvalidContentType）。",
        mode.what(),
        wrong.join("\n  - ")
    );
}

#[test]
fn the_smtp_port_scanner_reaches_every_carrier_and_strips_comments() {
    // 阳性对照：三个提取点都必须**真的取到东西** —— 否则「扫到 0 条」也会让轴测试通过。
    let mail_code = mask_comments(MAIL_RS);
    let mode = tls_mode(&mail_code).expect("模式提取器失效（空集）");
    assert!(
        forwards_configured_port(&mail_code),
        "`mail.rs` 里没有把配置端口送进 `.port(…)` —— 值成了死的"
    );
    let samples = example_smtp_ports(CONFIG_EXAMPLE);
    assert!(!samples.is_empty(), "示例配置提取器失效（空集）");
    assert!(
        samples.iter().all(|(_, p)| *p > 0),
        "示例配置里读到 0 —— 提取器读错了行"
    );
    let (_, d) = default_smtp_port(CONFIG_RS).expect("config.rs 提取器失效（空集）");
    assert!(d > 0, "config.rs 默认值读到 0 —— 提取器读错了行");

    // 掩码器**在干活**（判据是派生的、不是快照）：`mail.rs` 文件头那句散文提到了
    // STARTTLS 的改法关键字 —— 原文里必须有、清空注释后必须没有。
    // 否则模式判定会同时看到 relay 与 builder_dangerous，门禁就废了。
    assert!(
        MAIL_RS.contains("builder_dangerous("),
        "前提变了：`mail.rs` 文件头不再提 STARTTLS 改法，请复核本对照"
    );
    assert!(
        !mail_code.contains("builder_dangerous("),
        "注释没有被掩码掉 —— 模式判定会同时看到两种模式"
    );
    // `config.rs` 的文档注释里也有数字（本门禁写下的「465」）—— 必须被掩掉，
    // 否则默认值提取器可能读到注释里的字面量。
    // 判据用**差值**而不是「掩码后不含 `//`」：`config.rs` 的字符串字面量里本来就有
    // 形如 `"http://localhost:8080"` 的 `//`，掩码器**刻意**保留字符串正文。
    let masked_config = mask_comments(CONFIG_RS);
    assert!(
        CONFIG_RS.matches("//").count() > masked_config.matches("//").count(),
        "config.rs 的注释未被掩码（掩码前后 `//` 数应减少；字符串里的 `//` 会保留）"
    );
    assert!(
        masked_config.contains("http://"),
        "掩码器连字符串正文一起清了 —— 它会误吞代码里的 `//` 判定"
    );

    // `TlsMode` 的映射确实是 lettre 的两个常量（派生自依赖，不写快照）。
    assert_ne!(
        TlsMode::Implicit.expected_port(),
        TlsMode::StartTls.expected_port(),
        "lettre 的两个提交口不该相同"
    );
    assert_eq!(mode, TlsMode::Implicit, "本仓当前用的是 implicit TLS");
}

#[test]
fn the_smtp_port_rule_rejects_a_mismatch_and_the_extractors_have_teeth() {
    // 合成输入：拿**另一个模式**的端口当示例值（派生，不写字面量）⇒ 提取器应读出它，
    // 且它与 implicit TLS 的期望值不等（这正是轴测试会判红的那种形态）。
    let implicit = TlsMode::Implicit.expected_port();
    let starttls = TlsMode::StartTls.expected_port();
    let fake_example = format!("# [mail]\n# smtp_port = {starttls}\n");
    assert_eq!(
        example_smtp_ports(&fake_example),
        vec![(2, starttls)],
        "示例提取器没能读出注释草稿里的值"
    );
    assert_ne!(starttls, implicit, "两种模式的端口应不同 ⇒ 错值必须被判红");

    // 未注释的形态也要认（用户可能把草稿取消注释后再提交）。
    assert_eq!(
        example_smtp_ports(&format!("smtp_port = {starttls}\n")),
        vec![(1, starttls)]
    );
    // 相邻键不得被误当成 `smtp_port`（`smtp_port_backup` 这类）。
    assert!(
        example_smtp_ports("# smtp_port_backup = 1\n").is_empty(),
        "前缀匹配把 `smtp_port_backup` 读成了 `smtp_port`"
    );

    // 默认值提取器：读函数体里的那个数字。
    assert_eq!(
        default_smtp_port(&format!(
            "fn default_smtp_port() -> u16 {{\n    {starttls}\n}}\n"
        )),
        Some((2, starttls))
    );
    // 注释里的数字不得被读出来（否则「注释里写着 465、代码是 587」会假绿）。
    let commented =
        format!("/// 应为 {implicit}\nfn default_smtp_port() -> u16 {{\n    {starttls}\n}}\n");
    assert_eq!(
        default_smtp_port(&commented),
        Some((3, starttls)),
        "默认值提取器读到了注释里的数字"
    );

    // 掩码器：字符串里的 `//` 不是注释，字符串里的 `/*` 也不是。
    let with_url = "let u = \"https://example.com/a\"; // 尾注释\nlet v = 1;\n";
    let m = mask_comments(with_url);
    assert!(
        m.contains("https://example.com/a"),
        "字符串里的 // 被误当注释"
    );
    assert!(
        !m.contains("尾注释") && m.contains("let v = 1;"),
        "行注释未被掩码"
    );
    let with_block = "let s = \"/* not a comment */\";\n/* real */ let t = 2;\n";
    let mb = mask_comments(with_block);
    assert!(
        mb.contains("/* not a comment */"),
        "字符串里的块注释起始被误判"
    );
    assert!(!mb.contains("/* real */"), "块注释未被掩码");
}
