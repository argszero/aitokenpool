//! SQLite 数据层（rusqlite bundled）
//!
//! 表结构对齐 docs/architecture.md §5 + P0-A rant（2026-08-17T22:21:52）：
//! users / keys（上游 key）/ api_keys（分发 key，atk_live_ 前缀）/ models /
//! quotas（点数账户）/ transactions / usage_records + schema_version 迁移表。
//!
//! 迁移策略：CREATE TABLE IF NOT EXISTS 幂等；schema_version 记录当前版本，
//! 重复启动不报错。

use anyhow::{Context, Result};
use rusqlite::Connection;

pub const SCHEMA_VERSION: i64 = 4;

/// 打开（或创建）数据库并执行幂等迁移 + dev 种子
pub fn open(path: &str) -> Result<Connection> {
    if let Some(dir) = std::path::Path::new(path).parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("创建数据目录失败: {}", dir.display()))?;
        }
    }
    let conn = Connection::open(path).with_context(|| format!("打开数据库失败: {}", path))?;
    migrate(&conn)?;
    seed(&conn)?;
    Ok(conn)
}

/// 幂等迁移：建表 + schema_version
pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS users (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            email         TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            name          TEXT NOT NULL DEFAULT '',
            role          TEXT NOT NULL DEFAULT 'user',
            created_at    TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE TABLE IF NOT EXISTS keys (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            provider      TEXT NOT NULL,
            plan          TEXT NOT NULL DEFAULT '',
            model         TEXT NOT NULL DEFAULT '',
            status        TEXT NOT NULL DEFAULT 'on',
            owner_id      INTEGER NOT NULL REFERENCES users(id),
            encrypted_key TEXT NOT NULL,
            quota         REAL NOT NULL DEFAULT 0,
            used          REAL NOT NULL DEFAULT 0,
            available_days  TEXT NOT NULL DEFAULT '',
            available_start TEXT NOT NULL DEFAULT '',
            available_end   TEXT NOT NULL DEFAULT '',
            note            TEXT NOT NULL DEFAULT '',
            created_at    TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE TABLE IF NOT EXISTS api_keys (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id    INTEGER NOT NULL REFERENCES users(id),
            key_value  TEXT NOT NULL UNIQUE,
            name       TEXT NOT NULL DEFAULT '',
            status     TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            last_used  TEXT
        );
        CREATE TABLE IF NOT EXISTS models (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            provider    TEXT NOT NULL,
            model       TEXT NOT NULL,
            currency    TEXT NOT NULL DEFAULT 'USD',
            input_per_m REAL NOT NULL DEFAULT 0,
            output_per_m REAL NOT NULL DEFAULT 0,
            updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE TABLE IF NOT EXISTS quotas (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id    INTEGER NOT NULL UNIQUE REFERENCES users(id),
            balance    REAL NOT NULL DEFAULT 0,
            gift_balance REAL NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE TABLE IF NOT EXISTS gift_grants (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id    INTEGER NOT NULL REFERENCES users(id),
            amount     REAL NOT NULL DEFAULT 0,
            granted_at TEXT NOT NULL DEFAULT (datetime('now')),
            expires_at TEXT NOT NULL DEFAULT '',
            status     TEXT NOT NULL DEFAULT 'active'
        );
        CREATE TABLE IF NOT EXISTS transactions (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id      INTEGER NOT NULL REFERENCES users(id),
            counterpart  TEXT NOT NULL DEFAULT '',
            key_id       INTEGER,
            model        TEXT NOT NULL DEFAULT '',
            tokens       REAL NOT NULL DEFAULT 0,
            pts          REAL NOT NULL DEFAULT 0,
            type         TEXT NOT NULL DEFAULT '',
            status       TEXT NOT NULL DEFAULT '成功',
            time         TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE TABLE IF NOT EXISTS usage_records (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id    INTEGER NOT NULL REFERENCES users(id),
            api_key_id INTEGER,
            key_id     INTEGER,
            model      TEXT NOT NULL DEFAULT '',
            tokens     REAL NOT NULL DEFAULT 0,
            cost       REAL NOT NULL DEFAULT 0,
            time       TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE TABLE IF NOT EXISTS departments (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            name       TEXT NOT NULL UNIQUE,
            quota      REAL NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE TABLE IF NOT EXISTS raise_requests (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id     INTEGER NOT NULL REFERENCES users(id),
            amount      REAL NOT NULL DEFAULT 0,
            reason      TEXT NOT NULL DEFAULT '',
            status      TEXT NOT NULL DEFAULT 'pending',
            created_at  TEXT NOT NULL DEFAULT (datetime('now')),
            reviewed_by INTEGER,
            reviewed_at TEXT
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_models_provider_model
            ON models(provider, model);
        "#,
    )?;
    // v2：为旧库补 available_* 列（新建库已在建表语句里）
    ensure_column(
        conn,
        "keys",
        "available_days",
        "available_days TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "keys",
        "available_start",
        "available_start TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "keys",
        "available_end",
        "available_end TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(conn, "keys", "note", "note TEXT NOT NULL DEFAULT ''")?;
    // v3（P1）：点数账户拆分——gift_balance（当前有效赠送点数）+ gift_grants 明细表
    ensure_column(
        conn,
        "quotas",
        "gift_balance",
        "gift_balance REAL NOT NULL DEFAULT 0",
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS gift_grants (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id    INTEGER NOT NULL REFERENCES users(id),
            amount     REAL NOT NULL DEFAULT 0,
            granted_at TEXT NOT NULL DEFAULT (datetime('now')),
            expires_at TEXT NOT NULL DEFAULT '',
            status     TEXT NOT NULL DEFAULT 'active'
        );",
    )?;
    // v4（P2-C）：部门/加额审批——users.dept_id + departments / raise_requests 表
    ensure_column(conn, "users", "dept_id", "dept_id INTEGER")?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS departments (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            name       TEXT NOT NULL UNIQUE,
            quota      REAL NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE TABLE IF NOT EXISTS raise_requests (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id     INTEGER NOT NULL REFERENCES users(id),
            amount      REAL NOT NULL DEFAULT 0,
            reason      TEXT NOT NULL DEFAULT '',
            status      TEXT NOT NULL DEFAULT 'pending',
            created_at  TEXT NOT NULL DEFAULT (datetime('now')),
            reviewed_by INTEGER,
            reviewed_at TEXT
        );",
    )?;
    // schema_version：INSERT OR REPLACE 保证幂等
    let v: i64 = conn
        .query_row("SELECT version FROM schema_version", [], |r| r.get(0))
        .unwrap_or(0);
    if v < SCHEMA_VERSION {
        conn.execute(
            "INSERT OR REPLACE INTO schema_version (version) VALUES (?1)",
            [SCHEMA_VERSION],
        )?;
    }
    Ok(())
}

/// 幂等补列：列不存在才 ALTER TABLE ADD COLUMN
fn ensure_column(conn: &Connection, table: &str, column: &str, ddl: &str) -> Result<()> {
    let sql = format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1");
    let exists: bool = conn
        .prepare(&sql)?
        .exists([column])
        .with_context(|| format!("检查列 {table}.{column} 失败"))?;
    if !exists {
        conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {ddl}"), [])
            .with_context(|| format!("为 {table} 添加列 {column} 失败"))?;
    }
    Ok(())
}

/// 上游 key 加密迁移：旧明文占位（非 v1: 前缀）→ 启动时自动加密
/// 返回迁移条数；已加密 / 已迁移的 key 原样保留
pub fn migrate_key_encryption(conn: &Connection, crypto: &crate::crypto::Crypto) -> Result<usize> {
    let mut stmt = conn.prepare("SELECT id, encrypted_key FROM keys")?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut n = 0usize;
    for (id, stored) in rows {
        if stored.starts_with(crate::crypto::PREFIX) {
            continue; // 已是 v1 密文
        }
        let cipher = crypto.encrypt(stored.as_bytes())?;
        conn.execute(
            "UPDATE keys SET encrypted_key = ?1 WHERE id = ?2",
            rusqlite::params![cipher, id],
        )?;
        n += 1;
    }
    if n > 0 {
        log::info!("密钥加密迁移完成：{n} 条明文 key 已加密");
    }
    Ok(n)
}

/// dev 种子：demo 用户（demo@aitokenpool.local / demo1234，argon2）+ 点数账户 + 示例上游 key
pub fn seed(conn: &Connection) -> Result<()> {
    use crate::auth::hash_password;

    let demo_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM users WHERE email = ?1",
            ["demo@aitokenpool.local"],
            |r| r.get(0),
        )
        .ok();
    let demo_id = match demo_id {
        Some(id) => id,
        None => {
            let hash = hash_password("demo1234")?;
            conn.execute(
                "INSERT INTO users (email, password_hash, name, role) VALUES (?1, ?2, '阿零', 'user')",
                rusqlite::params!["demo@aitokenpool.local", hash],
            )?;
            conn.last_insert_rowid()
        }
    };
    // 点数账户（seed 余额 12471，对齐 UI mock D.USER.balance）
    conn.execute(
        "INSERT OR IGNORE INTO quotas (user_id, balance) VALUES (?1, 12471)",
        [demo_id],
    )?;
    // 示例上游 key（不真实可用的占位：占位密钥 + deepseek paygo plan）
    conn.execute(
        "INSERT OR IGNORE INTO keys (provider, plan, model, status, owner_id, encrypted_key, quota, used) \
         VALUES ('deepseek', 'deepseek-paygo', 'deepseek-v4-flash', 'on', ?1, 'sk-placeholder-encrypted', 1000, 0)",
        [demo_id],
    )?;

    // 管理员账号（P1）：admin@aitokenpool.local / admin1234，role=admin
    let admin_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM users WHERE email = ?1",
            ["admin@aitokenpool.local"],
            |r| r.get(0),
        )
        .ok();
    if admin_id.is_none() {
        let hash = hash_password("admin1234")?;
        conn.execute(
            "INSERT INTO users (email, password_hash, name, role) VALUES (?1, ?2, '管理员', 'admin')",
            rusqlite::params!["admin@aitokenpool.local", hash],
        )?;
        let id = conn.last_insert_rowid();
        conn.execute(
            "INSERT OR IGNORE INTO quotas (user_id, balance) VALUES (?1, 0)",
            [id],
        )?;
    }

    // 运营者账号（P2-C）：ops@aitokenpool.local / ops1234，role=ops
    let ops_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM users WHERE email = ?1",
            ["ops@aitokenpool.local"],
            |r| r.get(0),
        )
        .ok();
    if ops_id.is_none() {
        let hash = hash_password("ops1234")?;
        conn.execute(
            "INSERT INTO users (email, password_hash, name, role) VALUES (?1, ?2, '运营者', 'ops')",
            rusqlite::params!["ops@aitokenpool.local", hash],
        )?;
        let id = conn.last_insert_rowid();
        conn.execute(
            "INSERT OR IGNORE INTO quotas (user_id, balance) VALUES (?1, 0)",
            [id],
        )?;
    }
    Ok(())
}

/// models 表种子（启动时 upsert）：data/models.example.json 价格大表 +
/// config.price_overrides 官方价覆盖（覆盖同名模型）。文件缺失时仅告警不报错。
pub fn seed_models(conn: &Connection, cfg: &crate::config::Config) -> Result<()> {
    let path = "data/models.example.json";
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("models 种子文件缺失（{path}），跳过：{e}");
            return Ok(());
        }
    };
    let v: serde_json::Value =
        serde_json::from_str(&raw).with_context(|| format!("解析 {path} 失败"))?;
    let models = v["models"].as_array().cloned().unwrap_or_default();

    // 官方价覆盖：(provider, model) → (input_per_m, output_per_m, currency)
    let mut overrides = std::collections::HashMap::new();
    for o in &cfg.price_overrides {
        overrides.insert(
            (o.provider.clone(), o.model.clone()),
            (o.input_per_m, o.output_per_m, o.currency.clone()),
        );
    }

    let mut n = 0u32;
    for m in models {
        let provider = m["provider"].as_str().unwrap_or("").to_string();
        let model = m["model"].as_str().unwrap_or("").to_string();
        if provider.is_empty() || model.is_empty() {
            continue;
        }
        let (input, output, currency) = overrides
            .get(&(provider.clone(), model.clone()))
            .cloned()
            .unwrap_or_else(|| {
                (
                    m["input_per_m"].as_f64().unwrap_or(0.0),
                    m["output_per_m"].as_f64().unwrap_or(0.0),
                    m["currency"].as_str().unwrap_or("USD").to_string(),
                )
            });
        conn.execute(
            "INSERT INTO models (provider, model, currency, input_per_m, output_per_m, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now')) \
             ON CONFLICT(provider, model) DO UPDATE SET \
               currency = excluded.currency, \
               input_per_m = excluded.input_per_m, \
               output_per_m = excluded.output_per_m, \
               updated_at = datetime('now')",
            rusqlite::params![provider, model, currency, input, output],
        )?;
        n += 1;
    }
    log::info!("models seed：{n} 行 upsert（来源 {path} + price_overrides）");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_db(tag: &str) -> (Connection, std::path::PathBuf) {
        let p = std::env::temp_dir().join(format!("atp_test_{}_{}.db", std::process::id(), tag));
        let _ = std::fs::remove_file(&p);
        let conn = open(p.to_str().unwrap()).expect("open tmp db");
        (conn, p)
    }

    #[test]
    fn migrate_is_idempotent() {
        let (conn, p) = tmp_db("migrate");
        // 二次迁移不报错（重复启动场景）
        migrate(&conn).expect("第二次 migrate 应成功");
        migrate(&conn).expect("第三次 migrate 应成功");
        let v: i64 = conn
            .query_row("SELECT version FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        drop(conn);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn seed_demo_user_and_quota() {
        let (conn, p) = tmp_db("seed");
        let u: i64 = conn
            .query_row(
                "SELECT id FROM users WHERE email = 'demo@aitokenpool.local'",
                [],
                |r| r.get(0),
            )
            .expect("demo 用户已种子");
        let bal: f64 = conn
            .query_row("SELECT balance FROM quotas WHERE user_id = ?1", [u], |r| {
                r.get(0)
            })
            .expect("demo 点数账户已种子");
        assert_eq!(bal, 12471.0);
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM keys WHERE owner_id = ?1", [u], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(n >= 1, "示例上游 key 已种子");
        drop(conn);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn seed_models_upserts_from_example_and_overrides() {
        let (conn, p) = tmp_db("models");
        let cfg = crate::config::Config::load("config/config.example.toml").unwrap();
        seed_models(&conn, &cfg).expect("seed models");
        // example 文件里的模型已入库
        let cnt: i64 = conn
            .query_row("SELECT COUNT(*) FROM models", [], |r| r.get(0))
            .unwrap();
        assert!(cnt >= 5, "models 已从 example 文件 seed，cnt={cnt}");
        // deepseek-v4-pro 被 price_overrides 覆盖为官方价（0.435 / 0.87 USD）
        let (input, output, currency): (f64, f64, String) = conn
            .query_row(
                "SELECT input_per_m, output_per_m, currency FROM models \
                 WHERE provider = 'deepseek' AND model = 'deepseek-v4-pro'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert!((input - 0.435).abs() < 1e-9, "input={input}");
        assert!((output - 0.87).abs() < 1e-9, "output={output}");
        assert_eq!(currency, "USD");
        // 幂等：重复 seed 不报错且不产生重复行
        seed_models(&conn, &cfg).expect("二次 seed");
        let cnt2: i64 = conn
            .query_row("SELECT COUNT(*) FROM models", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cnt, cnt2, "upsert 不产生重复行");
        drop(conn);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn v2_migration_adds_available_and_note_columns() {
        // 模拟 v1 旧库：建表后不含 available_* / note 列 → migrate 幂等补列
        let p = std::env::temp_dir().join(format!("atp_v1_{}_{}.db", std::process::id(), "v1"));
        let _ = std::fs::remove_file(&p);
        let conn = Connection::open(&p).unwrap();
        conn.execute_batch(
            "CREATE TABLE keys (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider TEXT NOT NULL,
                plan TEXT NOT NULL DEFAULT '',
                model TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'on',
                owner_id INTEGER NOT NULL,
                encrypted_key TEXT NOT NULL,
                quota REAL NOT NULL DEFAULT 0,
                used REAL NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            INSERT INTO keys (provider, plan, model, status, owner_id, encrypted_key) VALUES ('t','p','m','on',1,'sk-plain');",
        )
        .unwrap();
        migrate(&conn).unwrap();
        // 补列成功且旧数据保留
        let row: (String, String, String, String) = conn
            .query_row(
                "SELECT available_days, available_start, available_end, note FROM keys WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(row.0, "");
        assert_eq!(row.3, "");
        let v: i64 = conn
            .query_row("SELECT version FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION, "旧库迁移后版本应为 {SCHEMA_VERSION}");
        drop(conn);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn key_encryption_migration_encrypts_plaintext() {
        let crypto = crate::crypto::Crypto::new([11u8; 32]);
        let (conn, p) = tmp_db("kenc");
        // 手工插入明文占位 key（模拟 v1 遗留）
        conn.execute(
            "INSERT INTO keys (provider, plan, model, status, owner_id, encrypted_key) VALUES ('t','p','m','on',1,'sk-placeholder-encrypted')",
            [],
        )
        .unwrap();
        let n = migrate_key_encryption(&conn, &crypto).expect("迁移成功");
        assert!(n >= 1, "至少迁移一条");
        let stored: String = conn
            .query_row(
                "SELECT encrypted_key FROM keys WHERE provider = 't'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            stored.starts_with(crate::crypto::PREFIX),
            "已加密: {stored}"
        );
        assert_eq!(
            crypto.decrypt(&stored).unwrap(),
            b"sk-placeholder-encrypted",
            "迁移后可解密还原原文"
        );
        // 幂等：二次迁移不再变化
        let n2 = migrate_key_encryption(&conn, &crypto).expect("二次迁移");
        assert_eq!(n2, 0, "已加密的 key 不再重复迁移");
        drop(conn);
        let _ = std::fs::remove_file(p);
    }
}
