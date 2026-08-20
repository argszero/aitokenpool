//! 邮件发信（注册验证码，rant 2026-08-19T14:36:19 方案 B）
//!
//! - 配置了 SMTP（config [mail].smtp_host 非空）→ 真实发信
//! - 未配置 → dev 模式：验证码打印到日志（便于本地测试；生产必须配置 SMTP）
//!
//! ⚠️ TLS 模式与端口（2026-08-20 实测踩坑）：本实现用 `SmtpTransport::relay`
//! （implicit TLS，连接后立即 TLS 握手），**必须配 465 端口**（smtp.gmail.com:465）。
//! 配 587（STARTTLS 端口）会直接对 587 做 TLS 握手 → Gmail 返回明文 →
//! rustls 报 InvalidContentType。若要用 587 STARTTLS 需改用
//! `SmtpTransport::builder_dangerous(...).tls(Tls::Opportunistic(...))`。

use anyhow::{Context, Result};

use crate::config::Mail;
use lettre::Transport;

/// 发送验证码邮件；dev 模式（未配置 SMTP）时仅打日志并返回 Ok
pub fn send_verification_code(cfg: &Mail, to: &str, code: &str) -> Result<()> {
    if !cfg.configured() {
        log::warn!("[dev] 注册验证码（未配置 SMTP，仅日志；生产请配置 [mail]）：{to} → {code}");
        return Ok(());
    }
    let subject = if cfg.verify_subject.is_empty() {
        "AITokenPool 邮箱验证码".to_string()
    } else {
        cfg.verify_subject.clone()
    };
    let from = if cfg.from.is_empty() {
        "noreply@aitokenpool.local".to_string()
    } else {
        cfg.from.clone()
    };
    let from_name = if cfg.from_name.is_empty() {
        "AITokenPool".to_string()
    } else {
        cfg.from_name.clone()
    };
    let body = format!(
        "您的 AITokenPool 注册验证码是：{code}，10 分钟内有效。\n\
         如非本人操作请忽略本邮件。\n\
         Your AITokenPool verification code: {code} (valid for 10 minutes)."
    );

    let email = lettre::Message::builder()
        .from(
            format!("{from_name} <{from}>")
                .parse()
                .context("from 邮箱格式非法")?,
        )
        .to(to.parse().context("收件邮箱格式非法")?)
        .subject(subject)
        .header(lettre::message::header::ContentType::TEXT_PLAIN)
        .body(body)
        .context("构建邮件失败")?;

    let creds = lettre::transport::smtp::authentication::Credentials::new(
        cfg.smtp_user.clone(),
        cfg.smtp_password.clone(),
    );
    let mailer = lettre::SmtpTransport::relay(&cfg.smtp_host)
        .with_context(|| format!("SMTP 服务器不可用: {}", cfg.smtp_host))?
        .port(cfg.smtp_port)
        .credentials(creds)
        .build();
    mailer
        .send(&email)
        .map_err(|e| {
            log::error!("SMTP 发送验证码到 {to} 失败: {e:?}");
            anyhow::anyhow!("发送验证码到 {to} 失败: {e}")
        })?;
    log::info!("验证码邮件已发送: {to}");
    Ok(())
}
