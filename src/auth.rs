//! 认证：argon2 口令哈希 + API Key 生成/校验
//!
//! P0-A（rant 2026-08-17T22:21:52）：
//! - POST /api/auth/login：email+password → argon2 校验 → 返回该用户有效 API Key（无则生成）
//! - Bearer <api_key> 认证：查 api_keys 表 → 注入用户身份；无效 401

use anyhow::{anyhow, Result};
use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use rand::RngCore;

/// argon2 口令哈希（OWASP 默认参数：m=19MiB, t=2, p=1）
/// v0.6.1 起生产使用：bootstrap 初始管理员建号 + 改密端点（rant 2026-08-19T14:35:05）
pub fn hash_password(pw: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .map_err(|e| anyhow!("argon2 哈希失败: {e}"))?
        .to_string())
}

/// 校验口令是否匹配存储哈希
pub fn verify_password(hash: &str, pw: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(pw.as_bytes(), &parsed)
        .is_ok()
}

/// [`hash_password`] 的 blocking-pool 版本（handler 用）。
///
/// 两个 KDF 的**同步**版本是**阻塞 CPU**（默认参数 m=19MiB/t=2/p=1 实测 ~0.24 s）。async handler
/// 里直接调用它会把调用它的 worker 线程占满：并发请求本应被工作线程池吸收，却变成**整个运行时排队**
/// —— 任何无关请求（含 `/healthz`）都会等它跑完。多 worker 只是把这一步推后，不是可靠边界（C2105：
/// 2 核 CI runner 上未认证注册的 KDF 让并发的 `/healthz` 等了 1.29 s，同一条测试在多核开发机上
/// 通常是 ~0.4 ms ⇒ 间歇性红灯）。放进 blocking pool 后，同时 CPI 计数在**阻塞线程**上等待、
/// 不占 worker，`/healthz` 回到毫秒级。
///
/// ⚠️ 调用方语义：`spawn_blocking` 的 JoinError 此前在 4 个 handler 里各有各的处理方式，本函数统一
/// **panic 传播（resume_unwind）** —— 「worker 死了」与「密码错了」是不同的失败，绝不能把前者混进
/// 后者的 →401 分支（那会掩盖 bug）。panic 本身由 axum 的 catch-panic 转 500。
pub async fn hash_password_async(pw: String) -> Result<String> {
    tokio::task::spawn_blocking(move || hash_password(&pw))
        .await
        .unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()))
}

/// [`verify_password`] 的 blocking-pool 版本（handler 用）。语义同 [`hash_password_async`]。
pub async fn verify_password_async(hash: String, pw: String) -> bool {
    tokio::task::spawn_blocking(move || verify_password(&hash, &pw))
        .await
        .unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()))
}

/// 生成分发 API Key：`atk_live_` + 24 位 hex（12 随机字节），与 UI 原型一致
pub fn generate_api_key() -> String {
    let mut bytes = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    format!("atk_live_{}", hex::encode(bytes))
}

/// API Key 脱敏展示：atk_live_****xxxx（保留后 4 位）
pub fn mask_api_key(key: &str) -> String {
    if key.len() > 8 {
        let tail = &key[key.len() - 4..];
        format!("atk_live_****{tail}")
    } else {
        "****".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_verify_roundtrip() {
        let h = hash_password("demo1234").unwrap();
        assert!(verify_password(&h, "demo1234"));
        assert!(!verify_password(&h, "wrong"));
    }

    #[test]
    fn api_key_format_and_mask() {
        let k = generate_api_key();
        assert!(k.starts_with("atk_live_"));
        assert_eq!(k.len(), "atk_live_".len() + 24);
        let m = mask_api_key(&k);
        assert_eq!(m, format!("atk_live_****{}", &k[k.len() - 4..]));
        assert!(!m.contains(&k[..k.len() - 4]));
    }
}
