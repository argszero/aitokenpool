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
//!
//! ⚠️ 重试（rant 2026-08-21T23:52:17）：Gmail 等 SMTP 对数据中心 IP（阿里云/腾讯云等）
//! 会**间歇性静默丢弃**（TCP/TLS 成功但 SMTP banner 不响应 → 15s 超时）。这不是配置错误，
//! 是外部服务不可靠 → 应用层容错：发送失败后延迟 2s 重试，最多 2 次重试（共 3 次尝试）；
//! 每次重试重建 transport（新 TCP+TLS 连接），天然应对瞬时故障。重试仍失败才报错。

use anyhow::{Context, Result};
use std::time::Duration;

use crate::config::Mail;
use lettre::Transport;

/// SMTP 发送失败后的重试间隔（固定 2s，不用退避——Gmail 静默丢弃是瞬时的）
const RETRY_DELAY: Duration = Duration::from_secs(2);
/// 最大尝试次数（1 次初始 + 2 次重试）
const MAX_ATTEMPTS: usize = 3;

/// 通用带重试执行器：`f` 失败 → 延迟 `delay` 重试，最多共 `MAX_ATTEMPTS` 次尝试；
/// 全部失败返回最后一次错误。测试可传 `Duration::ZERO` 避免慢测试。
fn send_with_retry<F>(mut f: F, delay: Duration) -> Result<()>
where
    F: FnMut() -> Result<()>,
{
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 1..=MAX_ATTEMPTS {
        match f() {
            Ok(()) => return Ok(()),
            Err(e) => {
                log::warn!("SMTP 发送失败（attempt {attempt}/{MAX_ATTEMPTS}）：{e:#}");
                last_err = Some(e);
                if attempt < MAX_ATTEMPTS {
                    std::thread::sleep(delay);
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("SMTP 发送失败（未知错误）")))
}

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

    // 发送带重试：失败 → 2s 后重试，最多 2 次重试（rant 2026-08-21T23:52:17）
    send_with_retry(
        || {
            send_once(cfg, &email, to)?;
            log::info!("验证码邮件已发送: {to}");
            Ok(())
        },
        RETRY_DELAY,
    )
}

/// 单次发送：构建 transport（每次全新连接）+ 发送
fn send_once(cfg: &Mail, email: &lettre::Message, to: &str) -> Result<()> {
    let creds = lettre::transport::smtp::authentication::Credentials::new(
        cfg.smtp_user.clone(),
        cfg.smtp_password.clone(),
    );
    let mailer = lettre::SmtpTransport::relay(&cfg.smtp_host)
        .with_context(|| format!("SMTP 服务器不可用: {}", cfg.smtp_host))?
        .port(cfg.smtp_port)
        .credentials(creds)
        // 显式 15s 超时：避免无 pool 时挂住/长等（rant 2026-08-21T14:08:03）
        .timeout(Some(Duration::from_secs(15)))
        .build();
    mailer.send(email).map_err(|e| {
        log::error!("SMTP 发送验证码到 {to} 失败: {e:?}");
        anyhow::anyhow!("发送验证码到 {to} 失败: {e}")
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_succeeds_after_failures() {
        // 前两次失败、第三次成功 → 应重试后成功（共 3 次调用）
        let mut calls = 0;
        let r = send_with_retry(
            || {
                calls += 1;
                if calls < 3 {
                    Err(anyhow::anyhow!("transient failure"))
                } else {
                    Ok(())
                }
            },
            Duration::ZERO,
        );
        assert!(r.is_ok(), "重试后应成功: {r:?}");
        assert_eq!(calls, 3, "应恰好尝试 3 次（1 次初始 + 2 次重试）");
    }

    #[test]
    fn retry_gives_up_after_max_attempts() {
        // 恒失败 → 应报错且不无限重试（恰好 3 次尝试）
        let mut calls = 0;
        let r = send_with_retry(
            || {
                calls += 1;
                Err(anyhow::anyhow!("always fails"))
            },
            Duration::ZERO,
        );
        assert!(r.is_err(), "恒失败最终应报错");
        assert_eq!(
            calls, MAX_ATTEMPTS,
            "应恰好尝试 {MAX_ATTEMPTS} 次，不无限重试"
        );
    }

    #[test]
    fn retry_returns_last_error() {
        // 最后一次错误应被返回（便于上层日志/定位）
        let r = send_with_retry(|| Err(anyhow::anyhow!("final-err")), Duration::ZERO);
        assert!(r.unwrap_err().to_string().contains("final-err"));
    }
}
