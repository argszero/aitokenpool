//! 数据访问层（rusqlite 直查，简单分层）
//!
//! P0-A（rant 2026-08-17T22:21:52）：认证 + API Key 管理所需的最小查询集。

use anyhow::{anyhow, Result};
use rusqlite::Connection;

use crate::auth;

/// 按邮箱查用户 → (id, password_hash)
pub fn find_user_by_email(conn: &Connection, email: &str) -> Option<(i64, String)> {
    conn.query_row(
        "SELECT id, password_hash FROM users WHERE email = ?1",
        [email],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .ok()
}

/// 取该用户的「有效」API Key；没有则生成一个（登录即返回可用的 key）
pub fn get_or_create_api_key(conn: &Connection, user_id: i64) -> Result<String> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT key_value FROM api_keys WHERE user_id = ?1 AND status = 'active' ORDER BY id LIMIT 1",
            [user_id],
            |r| r.get(0),
        )
        .ok();
    if let Some(k) = existing {
        return Ok(k);
    }
    let key = auth::generate_api_key();
    conn.execute(
        "INSERT INTO api_keys (user_id, key_value, name, status) VALUES (?1, ?2, '', 'active')",
        rusqlite::params![user_id, key],
    )?;
    Ok(key)
}

/// 生成新 API Key（设置页「生成新 Key」）
pub fn create_api_key(conn: &Connection, user_id: i64, name: &str) -> Result<String> {
    let key = auth::generate_api_key();
    conn.execute(
        "INSERT INTO api_keys (user_id, key_value, name, status) VALUES (?1, ?2, ?3, 'active')",
        rusqlite::params![user_id, key, name],
    )?;
    Ok(key)
}

/// 列出用户的 API Key（key 值脱敏；P2-B 起只列 active，revoked 不再显示）
pub fn list_api_keys(conn: &Connection, user_id: i64) -> Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare(
        "SELECT id, key_value, name, status, created_at FROM api_keys \
         WHERE user_id = ?1 AND status = 'active' ORDER BY id DESC",
    )?;
    let rows = stmt.query_map([user_id], |r| {
        let raw: String = r.get(1)?;
        Ok(serde_json::json!({
            "id": r.get::<_, i64>(0)?,
            "key": auth::mask_api_key(&raw),
            "name": r.get::<_, String>(2)?,
            "status": r.get::<_, String>(3)?,
            "created_at": r.get::<_, String>(4)?,
        }))
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 软删 API Key（P2-B：status → 'revoked'）；仅属主可删；返回是否删除成功
pub fn revoke_api_key(conn: &Connection, user_id: i64, key_id: i64) -> Result<bool> {
    let n = conn.execute(
        "UPDATE api_keys SET status = 'revoked' WHERE id = ?1 AND user_id = ?2 AND status = 'active'",
        rusqlite::params![key_id, user_id],
    )?;
    Ok(n == 1)
}

/// Bearer 认证：按 key 查归属用户 + api_key id + 角色 → Some((user_id, api_key_id, role))
pub fn find_api_key_user_and_id(conn: &Connection, key: &str) -> Option<(i64, i64, String)> {
    conn.query_row(
        "SELECT a.user_id, a.id, u.role FROM api_keys a \
         JOIN users u ON u.id = a.user_id \
         WHERE a.key_value = ?1 AND a.status = 'active'",
        [key],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .ok()
}

/// 上游 key 行（路由/网关用）
#[derive(Debug, Clone)]
pub struct KeyRow {
    pub id: i64,
    pub provider: String,
    pub plan: String,
    pub owner_id: i64,
    pub encrypted_key: String,
}

/// 查某模型的健康 key（status='on'）——路由候选集
pub fn find_keys_by_model(conn: &Connection, model: &str) -> Result<Vec<KeyRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, provider, plan, owner_id, encrypted_key FROM keys \
         WHERE model = ?1 AND status = 'on'",
    )?;
    let rows = stmt.query_map([model], |r| {
        Ok(KeyRow {
            id: r.get(0)?,
            provider: r.get(1)?,
            plan: r.get(2)?,
            owner_id: r.get(3)?,
            encrypted_key: r.get(4)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 用户可用余额拆分 → (permanent, gift)；可用总额 = 两者之和
pub fn get_balances(conn: &Connection, user_id: i64) -> (f64, f64) {
    conn.query_row(
        "SELECT balance, gift_balance FROM quotas WHERE user_id = ?1",
        [user_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap_or((0.0, 0.0))
}

/// 用户可用余额（赠送 + 永久）——网关预检口径
pub fn get_available_balance(conn: &Connection, user_id: i64) -> f64 {
    let (permanent, gift) = get_balances(conn, user_id);
    permanent + gift
}

/// 模型单价（按 provider+model）→ (input_per_m, output_per_m, currency)
pub fn get_model_price(
    conn: &Connection,
    provider: &str,
    model: &str,
) -> Option<(f64, f64, String)> {
    conn.query_row(
        "SELECT input_per_m, output_per_m, currency FROM models WHERE provider = ?1 AND model = ?2",
        rusqlite::params![provider, model],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .ok()
}

/// 市场页：models 表 + key 可用性（可用 key 数）
pub fn list_models_with_availability(conn: &Connection) -> Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare(
        "SELECT m.provider, m.model, m.currency, m.input_per_m, m.output_per_m, \
                (SELECT COUNT(*) FROM keys k WHERE k.model = m.model AND k.status = 'on') AS avail \
         FROM models m ORDER BY m.provider, m.model",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(serde_json::json!({
            "provider": r.get::<_, String>(0)?,
            "model": r.get::<_, String>(1)?,
            "currency": r.get::<_, String>(2)?,
            "input_per_m": r.get::<_, f64>(3)?,
            "output_per_m": r.get::<_, f64>(4)?,
            "available_keys": r.get::<_, i64>(5)?,
        }))
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 更新最近使用时间
pub fn touch_api_key(conn: &Connection, key: &str) -> Result<()> {
    conn.execute(
        "UPDATE api_keys SET last_used = datetime('now') WHERE key_value = ?1",
        [key],
    )?;
    Ok(())
}

/// OpenAI 兼容模型列表（GET /v1/models，rant 2026-08-18T18:10:18）：
/// id=model 名、display_name=model 名、context_window 来自 models.context_window；
/// with_availability=true 时附加 available_keys（带 Bearer 时）。
pub fn list_models_openai(
    conn: &Connection,
    with_availability: bool,
) -> Result<Vec<serde_json::Value>> {
    let avail_sub = if with_availability {
        ",\n                (SELECT COUNT(*) FROM keys k WHERE k.model = m.model AND k.status = 'on') AS avail"
    } else {
        ""
    };
    let sql = format!(
        "SELECT m.model, m.context_window{avail_sub} \
         FROM models m ORDER BY m.provider, m.model"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| {
        let mut v = serde_json::json!({
            "id": r.get::<_, String>(0)?,
            "object": "model",
            "created": 0,
            "owned_by": "aitokenpool",
            "display_name": r.get::<_, String>(0)?,
            "context_window": r.get::<_, i64>(1)?,
        });
        if with_availability {
            v["available_keys"] = serde_json::json!(r.get::<_, i64>(2)?);
        }
        Ok(v)
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 校验口令（供登录用）
pub fn verify_user_password(conn: &Connection, email: &str, pw: &str) -> Result<i64> {
    let (id, hash) = find_user_by_email(conn, email).ok_or_else(|| anyhow!("用户不存在"))?;
    if auth::verify_password(&hash, pw) {
        Ok(id)
    } else {
        Err(anyhow!("口令错误"))
    }
}
