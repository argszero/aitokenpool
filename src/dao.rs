//! 数据访问层（rusqlite 直查，简单分层）
//!
//! P0-A（rant 2026-08-17T22:21:52）：认证 + API Key 管理所需的最小查询集。

use anyhow::Result;
use rusqlite::Connection;

use crate::auth;

/// 把 SQLite UTC 时间字符串转 UTC ISO 带 Z（rant 2026-08-19T20:45:32 全站时区）：
/// - 'YYYY-MM-DD HH:MM:SS' → 'YYYY-MM-DDTHH:MM:SSZ'
/// - 'YYYY-MM-DD'（纯日期，如 dashboard series）→ 'YYYY-MM-DDT00:00:00Z'
/// - 已含 T/Z 原样返回；空串返回空串。
///
/// 前端 new Date() 按 UTC 解析，避免把 UTC 当本地时间（差 8 小时）。
pub fn utc_iso(s: &str) -> String {
    let s = s.trim();
    if s.is_empty() {
        return String::new();
    }
    if s.contains('T') || s.contains('Z') {
        return s.to_string();
    }
    let is_date = |d: &str| d.len() == 10 && d.as_bytes()[4] == b'-' && d.as_bytes()[7] == b'-';
    if let Some((date, time)) = s.split_once(' ') {
        if is_date(date) {
            return format!("{date}T{time}Z");
        }
    }
    if is_date(s) {
        return format!("{s}T00:00:00Z");
    }
    s.to_string()
}

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
/// rant 2026-08-24T12:41:25：返回 last_used（网关 Bearer 鉴权时 touch_api_key 维护，NULL=从未使用）
pub fn list_api_keys(conn: &Connection, user_id: i64) -> Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare(
        "SELECT id, key_value, name, status, created_at, last_used FROM api_keys \
         WHERE user_id = ?1 AND status = 'active' ORDER BY id DESC",
    )?;
    let rows = stmt.query_map([user_id], |r| {
        let raw: String = r.get(1)?;
        let created_at: String = r.get(4)?;
        let last_used: Option<String> = r.get(5)?;
        Ok(serde_json::json!({
            "id": r.get::<_, i64>(0)?,
            "key": auth::mask_api_key(&raw),
            "full_key": raw, // 属主可见完整值（前端展示仍用脱敏 key，复制时用 full_key）
            "name": r.get::<_, String>(2)?,
            "status": r.get::<_, String>(3)?,
            "created_at": utc_iso(&created_at),
            "last_used": last_used.as_deref().map(utc_iso),
        }))
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 改名 API Key（rant 2026-08-22T17:21:39：改名须持久化）；仅属主、仅 active 可改；返回是否成功
pub fn rename_api_key(conn: &Connection, user_id: i64, key_id: i64, name: &str) -> Result<bool> {
    let n = conn.execute(
        "UPDATE api_keys SET name = ?1 WHERE id = ?2 AND user_id = ?3 AND status = 'active'",
        rusqlite::params![name, key_id, user_id],
    )?;
    Ok(n == 1)
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

/// 模型单价（按 provider+model）→ (input_per_m, output_per_m, cache_hit_input_per_m,
/// peak_input_per_m, peak_output_per_m, peak_cache_hit_input_per_m, currency)
/// 高峰价缺省 0 = 不启用高峰计费（billing::effective_prices 处理回落）
pub fn get_model_price(
    conn: &Connection,
    provider: &str,
    model: &str,
) -> Option<(f64, f64, f64, f64, f64, f64, String)> {
    conn.query_row(
        "SELECT input_per_m, output_per_m, cache_hit_input_per_m, \
                peak_input_per_m, peak_output_per_m, peak_cache_hit_input_per_m, currency \
         FROM models WHERE provider = ?1 AND model = ?2",
        rusqlite::params![provider, model],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        },
    )
    .ok()
}

/// 市场页：models 表 + key 可用性（可用 key 数）
pub fn list_models_with_availability(conn: &Connection) -> Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare(
        "SELECT m.provider, m.model, m.currency, m.input_per_m, m.output_per_m, m.context_window, \
                m.context_length, m.max_output, m.vision, m.cache_hit_input_per_m, \
                m.peak_input_per_m, m.peak_output_per_m, m.peak_cache_hit_input_per_m, \
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
            "context_window": r.get::<_, i64>(5)?,
            "context_length": r.get::<_, i64>(6)?,
            "max_output": r.get::<_, i64>(7)?,
            "vision": r.get::<_, i64>(8)?,
            "cache_hit_input_per_m": r.get::<_, f64>(9)?,
            "peak_input_per_m": r.get::<_, f64>(10)?,
            "peak_output_per_m": r.get::<_, f64>(11)?,
            "peak_cache_hit_input_per_m": r.get::<_, f64>(12)?,
            "available_keys": r.get::<_, i64>(13)?,
        }))
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 管理员模型列表（rant 2026-08-19T20:40:29）：全部字段 + id，供 /api/admin/models
pub fn list_all_models(conn: &Connection) -> Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare(
        "SELECT id, provider, model, currency, input_per_m, output_per_m, \
                context_length, max_output, vision, cache_hit_input_per_m, \
                peak_input_per_m, peak_output_per_m, peak_cache_hit_input_per_m, updated_at \
         FROM models ORDER BY provider, model",
    )?;
    let rows = stmt.query_map([], |r| {
        let updated_at: String = r.get(13)?;
        Ok(serde_json::json!({
            "id": r.get::<_, i64>(0)?,
            "provider": r.get::<_, String>(1)?,
            "model": r.get::<_, String>(2)?,
            "currency": r.get::<_, String>(3)?,
            "input_per_m": r.get::<_, f64>(4)?,
            "output_per_m": r.get::<_, f64>(5)?,
            "context_length": r.get::<_, i64>(6)?,
            "max_output": r.get::<_, i64>(7)?,
            "vision": r.get::<_, i64>(8)?,
            "cache_hit_input_per_m": r.get::<_, f64>(9)?,
            "peak_input_per_m": r.get::<_, f64>(10)?,
            "peak_output_per_m": r.get::<_, f64>(11)?,
            "peak_cache_hit_input_per_m": r.get::<_, f64>(12)?,
            "updated_at": utc_iso(&updated_at),
        }))
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 更新最近使用时间。
///
/// rant 2026-09-15T10:33:34：在这台 NFS 上，**每一次真写都会打掉客户端缓存**，紧随其后的查询
/// 重新冷读（同一库同一条 COUNT：真写后 1.1s/次 vs 热 13ms）。本函数跑在每个已认证请求的
/// 鉴权路径上（`routes::mod::AuthUser::from_request_parts`），此前每请求都写一次。
///
/// `last_used` 只用于 key 列表展示「最近使用」（分钟级精度足够）⇒ 60 秒内不重复写，把
/// 「每请求一次真写」降为「每分钟至多一次」。守卫命中 0 行时 SQLite 不落页、不打掉 NFS 缓存
/// （实测提交 5.8ms vs 真写 39ms）。
pub fn touch_api_key(conn: &Connection, key: &str) -> Result<()> {
    conn.execute(
        "UPDATE api_keys SET last_used = datetime('now') \
         WHERE key_value = ?1 \
           AND (last_used IS NULL OR last_used < datetime('now', '-60 seconds'))",
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

/* ---- 注册 / 邮箱验证（rant 2026-08-19T14:36:19 方案 B）---- */

/// 邮箱是否已注册（含未验证用户）
pub fn email_taken(conn: &Connection, email: &str) -> bool {
    conn.query_row("SELECT 1 FROM users WHERE email = ?1", [email], |_| Ok(()))
        .is_ok()
}

/// 创建未验证用户（role='user'，verified=0）→ 返回 user_id；不建 quotas（验证通过后建）
pub fn create_unverified_user(
    conn: &Connection,
    email: &str,
    name: &str,
    password_hash: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO users (email, password_hash, name, role, verified) VALUES (?1, ?2, ?3, 'user', 0)",
        rusqlite::params![email, password_hash, name],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 用户是否已验证（users.verified == 1）
pub fn user_verified(conn: &Connection, user_id: i64) -> bool {
    conn.query_row("SELECT verified FROM users WHERE id = ?1", [user_id], |r| {
        r.get::<_, i64>(0)
    })
    .map(|v| v == 1)
    .unwrap_or(false)
}

/// 验证通过后激活用户 + 建 quotas 账户（balance=0, gift_balance=0）
pub fn activate_user(conn: &Connection, email: &str) -> Result<()> {
    conn.execute("UPDATE users SET verified = 1 WHERE email = ?1", [email])?;
    let id: i64 = conn.query_row("SELECT id FROM users WHERE email = ?1", [email], |r| {
        r.get(0)
    })?;
    conn.execute(
        "INSERT OR IGNORE INTO quotas (user_id, balance, gift_balance) VALUES (?1, 0, 0)",
        [id],
    )?;
    Ok(())
}

/// 存储验证码（6 位数字的 sha256 hex）；同一邮箱重复注册/重发 → 覆盖旧码。
/// 过期时间由 SQL 自身计算（`datetime('now', '+10 minutes')`）：`find_valid_verification` 的谓词是
/// `expires_at > datetime('now')`，写入方必须写入**该谓词能解释**的值——把字面量 `'+10 minutes'`
/// 当参数存进去的话，记录存在却永远判为过期（写入口径与读取口径必须同一）。
pub fn store_verification_code(conn: &Connection, email: &str, code_hash: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO email_verifications (email, code_hash, expires_at, attempts) \
         VALUES (?1, ?2, datetime('now', '+10 minutes'), 0)",
        rusqlite::params![email, code_hash],
    )?;
    Ok(())
}

/// 重发限频检查 + 写入新码，**在同一把锁内**完成（原子）：距上次发送 < 60 秒 → 返回 false（不写入）。
/// 检查与写入分段（各自加锁）会让并发重发同时通过检查 ⇒ 同一邮箱一次突发收到多封验证码。返回 true
/// 表示已写入（`INSERT OR REPLACE` 重置 created_at，本次写入即下次限频的起点）。
pub fn begin_resend_verification(conn: &Connection, email: &str, code_hash: &str) -> Result<bool> {
    if resend_too_soon(conn, email) {
        return Ok(false);
    }
    store_verification_code(conn, email, code_hash)?;
    Ok(true)
}

/// 取验证记录 → Some((code_hash, attempts))；过期或不存在 → None
pub fn find_valid_verification(conn: &Connection, email: &str) -> Option<(String, i64)> {
    conn.query_row(
        "SELECT code_hash, attempts FROM email_verifications \
         WHERE email = ?1 AND expires_at > datetime('now')",
        [email],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .ok()
}

/// 最近一次发送时间距现在是否 < 60 秒（重发限频）
pub fn resend_too_soon(conn: &Connection, email: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM email_verifications WHERE email = ?1 AND created_at > datetime('now', '-60 seconds')",
        [email],
        |_| Ok(()),
    )
    .is_ok()
}

/// 校验失败计数 +1；返回失败后是否已达上限（≥5 → 删除记录）
pub fn bump_verification_attempt(conn: &Connection, email: &str) -> Result<bool> {
    conn.execute(
        "UPDATE email_verifications SET attempts = attempts + 1 WHERE email = ?1",
        [email],
    )?;
    let attempts: i64 = conn.query_row(
        "SELECT attempts FROM email_verifications WHERE email = ?1",
        [email],
        |r| r.get(0),
    )?;
    if attempts >= 5 {
        conn.execute("DELETE FROM email_verifications WHERE email = ?1", [email])?;
        return Ok(true);
    }
    Ok(false)
}

/// 验证成功后删除验证码记录
pub fn clear_verification(conn: &Connection, email: &str) -> Result<()> {
    conn.execute("DELETE FROM email_verifications WHERE email = ?1", [email])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_iso_converts_sqlite_utc_strings() {
        // 完整时间戳 → UTC ISO 带 Z
        assert_eq!(utc_iso("2026-08-19 12:00:00"), "2026-08-19T12:00:00Z");
        // 纯日期（dashboard series）→ 当天零点 UTC
        assert_eq!(utc_iso("2026-08-19"), "2026-08-19T00:00:00Z");
        // 已带 T/Z 原样
        assert_eq!(utc_iso("2026-08-19T12:00:00Z"), "2026-08-19T12:00:00Z");
        // 空 / 空白 → 空
        assert_eq!(utc_iso(""), "");
        assert_eq!(utc_iso("  "), "");
        // 非标准原样
        assert_eq!(utc_iso("just now"), "just now");
    }

    fn tmp_conn(tag: &str) -> (Connection, std::path::PathBuf) {
        let p = std::env::temp_dir().join(format!("atp_dao_{}_{}.db", std::process::id(), tag));
        let _ = std::fs::remove_file(&p);
        let conn = crate::db::open(p.to_str().unwrap()).expect("open tmp db");
        conn.execute(
            "INSERT INTO users (id, email, password_hash, name, role) \
             VALUES (1, 'dao@t.local', 'x', 'u', 'user')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO api_keys (id, user_id, key_value, name, status) \
             VALUES (1, 1, 'atk_live_x', 'k', 'active')",
            [],
        )
        .unwrap();
        (conn, p)
    }

    fn last_used(conn: &Connection) -> Option<String> {
        conn.query_row(
            "SELECT last_used FROM api_keys WHERE key_value = 'atk_live_x'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn set_last_used(conn: &Connection, modifier: &str) {
        conn.execute(
            &format!("UPDATE api_keys SET last_used = datetime('now', '{modifier}') WHERE id = 1"),
            [],
        )
        .unwrap();
    }

    /// 调一次 `touch_api_key` 并返回该语句**匹配的行数**（0 = SQLite 无页可落、不会真写文件）。
    fn touch_rows(conn: &Connection, key: &str) -> usize {
        touch_api_key(conn, key).unwrap();
        conn.changes() as usize
    }

    #[test]
    fn touch_api_key_writes_at_most_once_per_guard_window() {
        // rant 2026-09-15T10:33:34：本函数在鉴权路径上（每个已认证请求都会经过），此前每请求
        // 写一次 `last_used = datetime('now')`（秒级精度 ⇒ 同一秒内同值）；而在这台 NFS 上
        // **一次真写就打掉客户端缓存**，紧随其后的查询重新冷读（同一条 COUNT 1.1s vs 热 13ms）。
        // 守卫把「每请求一次真写」降为「每分钟至多一次」——`last_used` 只用于列表展示。
        let (conn, p) = tmp_conn("touch_guard");

        // ① 从未使用过 → 第一次必须写（否则「最近使用」永远是空）
        assert_eq!(
            last_used(&conn),
            None,
            "前置：新 key 的 last_used 应为 NULL"
        );
        assert_eq!(touch_rows(&conn, "atk_live_x"), 1, "首次调用必须写入");
        let first = last_used(&conn).expect("首次调用必须写入 last_used");

        // ② 窗口内（30 秒前）重复调用 → **不写**：该 UPDATE 匹配 0 行 ⇒ SQLite 不落页
        set_last_used(&conn, "-30 seconds");
        let stale = last_used(&conn).unwrap();
        assert_eq!(
            touch_rows(&conn, "atk_live_x"),
            0,
            "守卫窗口内仍改动了行（= 每个已认证请求一次真写，会打掉 NFS 客户端缓存）"
        );
        assert_eq!(touch_rows(&conn, "atk_live_x"), 0, "连续调用同样不该写");
        assert_eq!(last_used(&conn).unwrap(), stale, "窗口内 last_used 不该变");

        // ③ 窗口外（90 秒前）→ 必须写，否则展示会永远停在旧值
        set_last_used(&conn, "-90 seconds");
        assert_eq!(
            touch_rows(&conn, "atk_live_x"),
            1,
            "超过守护窗口后不再更新 last_used（展示会停在旧值）"
        );
        assert!(
            last_used(&conn).unwrap() > stale,
            "窗口外刷新后 last_used 应前进"
        );
        assert!(first <= last_used(&conn).unwrap(), "时间只能前进");

        // ④ 不存在的 key → 本来就不该产生写入
        assert_eq!(touch_rows(&conn, "atk_live_missing"), 0);

        drop(conn);
        let _ = std::fs::remove_file(p);
    }
}
