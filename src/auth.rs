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
