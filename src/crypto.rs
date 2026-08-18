//! 上游 key 加密存储（AES-256-GCM，RustCrypto）
//!
//! P0-C（rant 2026-08-18T10:36:04）：
//! - keys.encrypted_key 存 `v1:<nonce_hex>:<cipher_hex>`（含 12 字节随机 nonce）
//! - 主密钥来源（优先级）：env `ATP_MASTER_KEY`（hex 32 字节）→ config `[server] master_key`
//!   → dev 模式随机生成并打印警告（生产必须显式配置）
//! - 写路径加密、读路径解密；旧明文占位 key 启动时自动迁移（见 db::migrate_key_encryption）

use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{anyhow, Result};

/// 密文格式前缀：`v1:`（旧明文无此前缀 → 迁移/拒绝解密）
pub const PREFIX: &str = "v1:";
/// nonce 长度（AES-GCM 推荐 12 字节）
const NONCE_LEN: usize = 12;

/// AES-256-GCM 加密器（内部持有 32 字节主密钥）
#[derive(Clone)]
pub struct Crypto {
    key: [u8; 32],
}

impl Crypto {
    /// 直接以 32 字节主密钥构造
    pub fn new(key: [u8; 32]) -> Self {
        Self { key }
    }

    /// 主密钥解析：env ATP_MASTER_KEY → config master_key（均要求 hex 32 字节）
    /// 两者皆空 → dev 模式：随机密钥 + 警告（进程内有效，重启后旧密文不可解）
    pub fn from_config(config_master_key: &str) -> Self {
        if let Ok(env_key) = std::env::var("ATP_MASTER_KEY") {
            match parse_master_key(&env_key) {
                Ok(k) => {
                    log::info!("使用 ATP_MASTER_KEY 主密钥（env）");
                    return Self::new(k);
                }
                Err(e) => {
                    log::error!("ATP_MASTER_KEY 无效（需 32 字节 hex）: {e}，回退下一来源");
                }
            }
        }
        if !config_master_key.is_empty() {
            match parse_master_key(config_master_key) {
                Ok(k) => {
                    log::info!("使用 config [server].master_key 主密钥");
                    return Self::new(k);
                }
                Err(e) => {
                    log::error!("config master_key 无效（需 32 字节 hex）: {e}，回退 dev 模式");
                }
            }
        }
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        log::warn!(
            "未配置 ATP_MASTER_KEY / [server].master_key —— 使用随机 dev 主密钥（重启后旧密文不可解，生产必须显式配置）"
        );
        Self::new(key)
    }

    /// 加密：`v1:<nonce_hex>:<cipher_hex>`
    pub fn encrypt(&self, plain: &[u8]) -> Result<String> {
        let cipher =
            Aes256Gcm::new_from_slice(&self.key).map_err(|_| anyhow!("AES-256-GCM 初始化失败"))?;
        let mut nonce = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);
        let ct = cipher
            .encrypt(Nonce::from_slice(&nonce), plain)
            .map_err(|_| anyhow!("加密失败"))?;
        Ok(format!(
            "{PREFIX}{}:{}",
            hex::encode(nonce),
            hex::encode(ct)
        ))
    }

    /// 解密：解析 `v1:` 前缀 + nonce + 密文；非 v1 格式（明文/旧格式）→ 报错
    pub fn decrypt(&self, stored: &str) -> Result<Vec<u8>> {
        let rest = stored
            .strip_prefix(PREFIX)
            .ok_or_else(|| anyhow!("非 v1: 密文格式（明文占位？）"))?;
        let (nonce_hex, ct_hex) = rest
            .split_once(':')
            .ok_or_else(|| anyhow!("密文格式错误：缺 nonce/密文分隔"))?;
        let nonce = hex::decode(nonce_hex)?;
        let ct = hex::decode(ct_hex)?;
        if nonce.len() != NONCE_LEN {
            return Err(anyhow!("nonce 长度异常（{}）", nonce.len()));
        }
        let cipher =
            Aes256Gcm::new_from_slice(&self.key).map_err(|_| anyhow!("AES-256-GCM 初始化失败"))?;
        cipher
            .decrypt(Nonce::from_slice(&nonce), ct.as_ref())
            .map_err(|_| anyhow!("解密失败（主密钥不匹配或密文被篡改）"))
    }
}

/// 解析 hex 主密钥（必须 64 hex 字符 = 32 字节）
fn parse_master_key(hex_str: &str) -> Result<[u8; 32]> {
    let bytes = hex::decode(hex_str.trim())?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow!("主密钥必须是 32 字节（64 个 hex 字符）"))?;
    Ok(arr)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_crypto() -> Crypto {
        Crypto::new([7u8; 32])
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let c = test_crypto();
        let stored = c.encrypt(b"sk-abc123secret").unwrap();
        assert!(stored.starts_with(PREFIX), "格式 v1: 前缀");
        assert_eq!(c.decrypt(&stored).unwrap(), b"sk-abc123secret");
    }

    #[test]
    fn same_plaintext_different_nonce() {
        let c = test_crypto();
        let a = c.encrypt(b"sk-x").unwrap();
        let b = c.encrypt(b"sk-x").unwrap();
        assert_ne!(a, b, "随机 nonce 保证两次密文不同");
    }

    #[test]
    fn wrong_master_key_fails_to_decrypt() {
        let c1 = test_crypto();
        let c2 = Crypto::new([8u8; 32]);
        let stored = c1.encrypt(b"sk-abc").unwrap();
        assert!(c2.decrypt(&stored).is_err(), "错误主密钥必须解密失败");
    }

    #[test]
    fn plaintext_old_format_rejected() {
        let c = test_crypto();
        assert!(
            c.decrypt("sk-placeholder-encrypted").is_err(),
            "非 v1: 前缀（明文占位）应报错 → 触发迁移检测"
        );
        assert!(c.decrypt("v1:abcd").is_err(), "截断密文报错");
    }

    #[test]
    fn master_key_hex_validation() {
        let good = "a".repeat(64);
        assert!(parse_master_key(&good).is_ok());
        assert!(parse_master_key("not-hex").is_err());
        assert!(
            parse_master_key(&"a".repeat(62)).is_err(),
            "长度不足 32 字节"
        );
    }
}
