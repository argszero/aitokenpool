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

pub const SCHEMA_VERSION: i64 = 1;

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
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
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
        "#,
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
}
