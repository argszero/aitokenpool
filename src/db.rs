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

pub const SCHEMA_VERSION: i64 = 12;

/// 打开（或创建）数据库并执行幂等迁移（生产标准：空库只建表，不种任何假数据）
pub fn open(path: &str) -> Result<Connection> {
    if let Some(dir) = std::path::Path::new(path).parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("创建数据目录失败: {}", dir.display()))?;
        }
    }
    let conn = Connection::open(path).with_context(|| format!("打开数据库失败: {}", path))?;
    // v12（rant 2026-08-25T12:02:13）：NFS 库性能——64MB 页缓存 + 64MB mmap 预读，
    // 整库常驻进程内存，远端存储只首读一次（默认 cache_size 2MB < 库体积 → 每次查询准冷读）。
    // 注意：不要启用 WAL 模式（SQLite 官方明确不支持网络文件系统，有损坏风险）；
    // 数据库不能移本地盘（部署硬约束，库必须留在 NAS）。
    conn.pragma_update(None, "cache_size", -65536)
        .with_context(|| "设置 PRAGMA cache_size 失败".to_string())?;
    conn.pragma_update(None, "mmap_size", 67108864)
        .with_context(|| "设置 PRAGMA mmap_size 失败".to_string())?;
    migrate(&conn)?;
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
            cached_tokens REAL NOT NULL DEFAULT 0,
            output_tokens REAL NOT NULL DEFAULT 0,
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
            cached_tokens REAL NOT NULL DEFAULT 0,
            output_tokens REAL NOT NULL DEFAULT 0,
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
    // v5（rant 2026-08-18T18:10:18）：models.context_window（OpenAI 兼容 /v1/models 用，
    // 默认 1048576；seed 时以 context_length 覆盖）
    ensure_column(
        conn,
        "models",
        "context_window",
        "context_window INTEGER NOT NULL DEFAULT 1048576",
    )?;
    // v6（rant 2026-08-19T14:36:19）：注册邮箱验证——users.verified + email_verifications 表。
    // 注意：verified 默认 1（存量用户视为已信任，升级不锁号；新注册显式写 0 待验证）
    ensure_column(
        conn,
        "users",
        "verified",
        "verified INTEGER NOT NULL DEFAULT 1",
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS email_verifications (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            email      TEXT NOT NULL UNIQUE,
            code_hash  TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            attempts   INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;
    // v7（rant 2026-08-19T20:40:29）：管理员模型信息 CRUD——
    // models 补 context_length / max_output / vision / cache_hit_input_per_m
    //（context_window 为 OpenAI 兼容 /v1/models 字段，与 context_length 并列保留）
    ensure_column(
        conn,
        "models",
        "context_length",
        "context_length INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "models",
        "max_output",
        "max_output INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "models",
        "vision",
        "vision INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "models",
        "cache_hit_input_per_m",
        "cache_hit_input_per_m REAL NOT NULL DEFAULT 0",
    )?;
    // v8（rant 2026-08-20T10:17:27）：计费区分缓存命中/未命中——
    // usage_records 补 cached_tokens（缓存命中输入 token 数，settle 时写入）
    ensure_column(
        conn,
        "usage_records",
        "cached_tokens",
        "cached_tokens REAL NOT NULL DEFAULT 0",
    )?;
    // v9（rant 2026-08-20T11:58:40）：DeepSeek 高峰时段（北京 9-12/14-18）价格翻倍——
    // models 补 peak_* 高峰价三字段（缺省 0 = 不启用高峰计费，沿用空闲价）
    ensure_column(
        conn,
        "models",
        "peak_input_per_m",
        "peak_input_per_m REAL NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "models",
        "peak_cache_hit_input_per_m",
        "peak_cache_hit_input_per_m REAL NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "models",
        "peak_output_per_m",
        "peak_output_per_m REAL NOT NULL DEFAULT 0",
    )?;
    // v10（rant 2026-08-21T14:53:20）：单次调用 token 消耗明细——输入/缓存命中/输出——
    // transactions 补 cached_tokens + output_tokens（输入 = tokens − cached − output 可推导），
    // usage_records 补 output_tokens（与 cached_tokens 同模式）
    ensure_column(
        conn,
        "transactions",
        "cached_tokens",
        "cached_tokens REAL NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "transactions",
        "output_tokens",
        "output_tokens REAL NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "usage_records",
        "output_tokens",
        "output_tokens REAL NOT NULL DEFAULT 0",
    )?;
    // v11（rant 2026-08-22T17:21:39 需求 2）：交易记录表头重构——
    // transactions 补 api_key_id（分发 key 关联字段，Key 列显示 api_keys.name；
    // 历史行无此字段 → NULL，前端兜底走 key_label / 交易类型说明）
    ensure_column(conn, "transactions", "api_key_id", "api_key_id INTEGER")?;
    // v12（rant 2026-08-25T12:02:13）：transactions 查询性能——summary/COUNT/list 原先
    // 全表扫描 + 3 LEFT JOIN（dev 库 23079 行）；建 (user_id) 前缀复合索引覆盖筛选/排序/翻页/时间范围
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_transactions_user_id ON transactions(user_id);
         CREATE INDEX IF NOT EXISTS idx_transactions_user_id_id ON transactions(user_id, id DESC);
         CREATE INDEX IF NOT EXISTS idx_transactions_user_id_time ON transactions(user_id, time);
         CREATE INDEX IF NOT EXISTS idx_transactions_user_id_type ON transactions(user_id, type);",
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

/// 首次启动自动创建初始管理员（rant 2026-08-19T14:35:05：开源项目惯例——安装完应有默认管理员）。
/// - 仅当 users 表为空时创建：`admin@aitokenpool.local` + 随机 16 位字母数字密码（非硬编码）
///   + 其 quotas 账户（balance=0）；不创建 demo/ops 账号、不种余额、不种占位 key。
/// - 幂等：已有用户则跳过（重启不重复），返回 `None`；创建成功返回明文密码
///   （由调用方打印到启动日志，仅首次、明确标注「初始管理员密码，请立即修改」）。
pub fn bootstrap_admin(conn: &Connection) -> Result<Option<String>> {
    use rand::distributions::Alphanumeric;
    use rand::Rng;

    let users: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
    if users > 0 {
        return Ok(None);
    }
    let pw: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(16)
        .map(char::from)
        .collect();
    let hash = crate::auth::hash_password(&pw)?;
    conn.execute(
        "INSERT INTO users (email, password_hash, name, role) VALUES (?1, ?2, '管理员', 'admin')",
        rusqlite::params!["admin@aitokenpool.local", hash],
    )?;
    let id = conn.last_insert_rowid();
    conn.execute(
        "INSERT OR IGNORE INTO quotas (user_id, balance) VALUES (?1, 0)",
        [id],
    )?;
    Ok(Some(pw))
}

/// 测试专用：自建测试用户（demo/admin/ops + 配额 + 占位 key），复刻旧 seed() 行为。
/// 生产 open() 不再种任何数据（rant 2026-08-19T10:41:03：移除所有 demo 种子数据，一律生产标准）。
#[cfg(test)]
pub(crate) fn seed_test_users(conn: &Connection) -> Result<()> {
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
                "INSERT INTO users (email, password_hash, name, role) VALUES (?1, ?2, 'demo', 'user')",
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
    // 示例上游 key（占位密钥 + deepseek paygo plan；仅 keys 表为空时插入一次）
    let key_count: i64 = conn.query_row("SELECT COUNT(*) FROM keys", [], |r| r.get(0))?;
    if key_count == 0 {
        conn.execute(
            "INSERT INTO keys (provider, plan, model, status, owner_id, encrypted_key, quota, used) \
             VALUES ('deepseek', 'deepseek-paygo', 'deepseek-v4-flash', 'on', ?1, 'sk-placeholder-encrypted', 1000, 0)",
            [demo_id],
        )?;
    }

    // 管理员账号：admin@aitokenpool.local / admin1234，role=admin
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

    // 运营者账号：ops@aitokenpool.local / ops1234，role=ops
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

/// models 表种子（启动时同步）：config.toml [[models]] 唯一真源
///（rant 2026-08-20T10:27:13：移除 models.json + price_overrides 双层机制）
///（rant 2026-08-20T11:58:33：upsert 之外同步删除 config 已移除的模型，避免市场幽灵模型）
/// 语义：config 全量权威——启动后 models 表 = config [[models]]（admin 运行时增改在下次启动被 config 覆盖/清掉）
pub fn seed_models(conn: &Connection, cfg: &crate::config::Config) -> Result<()> {
    let mut n = 0u32;
    for m in &cfg.models {
        conn.execute(
            "INSERT INTO models (provider, model, currency, input_per_m, output_per_m, context_window, context_length, max_output, vision, cache_hit_input_per_m, peak_input_per_m, peak_cache_hit_input_per_m, peak_output_per_m, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, datetime('now')) \
             ON CONFLICT(provider, model) DO UPDATE SET \
               currency = excluded.currency, \
               input_per_m = excluded.input_per_m, \
               output_per_m = excluded.output_per_m, \
               context_window = excluded.context_window, \
               context_length = excluded.context_length, \
               max_output = excluded.max_output, \
               vision = excluded.vision, \
               cache_hit_input_per_m = excluded.cache_hit_input_per_m, \
               peak_input_per_m = excluded.peak_input_per_m, \
               peak_cache_hit_input_per_m = excluded.peak_cache_hit_input_per_m, \
               peak_output_per_m = excluded.peak_output_per_m, \
               updated_at = datetime('now')",
            rusqlite::params![
                m.provider,
                m.model,
                m.currency,
                m.input_per_m,
                m.output_per_m,
                m.context_length,
                m.context_length,
                m.max_output,
                m.vision as i64,
                m.cache_hit_input_per_m,
                m.peak_input_per_m,
                m.peak_cache_hit_input_per_m,
                m.peak_output_per_m,
            ],
        )?;
        n += 1;
    }
    // 同步删除 config 中已移除的模型（模型行无外键引用，usage_records/transactions 均按 model 字符串记）
    let mut removed = 0u32;
    let stale: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare("SELECT provider, model FROM models")
            .context("prepare models scan")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut v = Vec::new();
        for row in rows {
            let (p, m) = row?;
            if !cfg
                .models
                .iter()
                .any(|cm| cm.provider == p && cm.model == m)
            {
                v.push((p, m));
            }
        }
        v
    };
    for (p, m) in &stale {
        conn.execute(
            "DELETE FROM models WHERE provider = ?1 AND model = ?2",
            rusqlite::params![p, m],
        )?;
        removed += 1;
    }
    log::info!("models seed：{n} 行 upsert（来源 config.toml [[models]]），删除 {removed} 行 config 已移除模型");
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
    fn transactions_perf_indexes_created_on_migrate() {
        // v12（rant 2026-08-25T12:02:13）：transactions 性能索引在迁移时建好
        //（summary/COUNT/list 原先全表扫描 + 3 LEFT JOIN，dev 库 23079 行）
        let (conn, p) = tmp_db("txidx");
        let names: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'index' AND name LIKE 'idx_transactions_%' ORDER BY name",
            )
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(
            names,
            vec![
                "idx_transactions_user_id",
                "idx_transactions_user_id_id",
                "idx_transactions_user_id_time",
                "idx_transactions_user_id_type",
            ],
            "四个性能索引都应建好"
        );
        drop(conn);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn empty_db_has_no_seeded_users() {
        // rant 2026-08-19T10:41:03：生产标准空库——migrate 后无任何种子用户/配额/占位 key
        let (conn, p) = tmp_db("empty");
        let users: i64 = conn
            .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))
            .unwrap();
        assert_eq!(users, 0, "空库无用户");
        let keys: i64 = conn
            .query_row("SELECT COUNT(*) FROM keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(keys, 0, "空库无占位 key");
        let quotas: i64 = conn
            .query_row("SELECT COUNT(*) FROM quotas", [], |r| r.get(0))
            .unwrap();
        assert_eq!(quotas, 0, "空库无配额");
        let demo: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM users WHERE email = 'demo@aitokenpool.local'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(demo, 0, "无 demo 账号");
        drop(conn);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn seed_test_users_is_idempotent() {
        // 测试辅助函数多次调用不重复插入（幂等性验证）
        let (conn, p) = tmp_db("testseed");
        seed_test_users(&conn).expect("seed 1st");
        seed_test_users(&conn).expect("seed 2nd");
        let users: i64 = conn
            .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))
            .unwrap();
        assert_eq!(users, 3, "demo/admin/ops 各一个，共 3 用户");
        let keys: i64 = conn
            .query_row("SELECT COUNT(*) FROM keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(keys, 1, "占位 key 只插一次");
        drop(conn);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn bootstrap_admin_creates_on_empty_db_and_is_idempotent() {
        // rant 2026-08-19T14:35:05：空库首次启动 → 初始管理员 + 随机密码 + quotas(0)
        let (conn, p) = tmp_db("bootadmin");
        let pw = bootstrap_admin(&conn)
            .expect("bootstrap 1st")
            .expect("有密码");
        assert_eq!(pw.len(), 16, "密码 16 位字母数字");
        assert!(
            pw.chars().all(|c| c.is_ascii_alphanumeric()),
            "密码为字母数字"
        );
        let (email, role, hash): (String, String, String) = conn
            .query_row(
                "SELECT email, role, password_hash FROM users WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(email, "admin@aitokenpool.local");
        assert_eq!(role, "admin");
        assert!(crate::auth::verify_password(&hash, &pw), "密码与日志一致");
        assert!(
            !crate::auth::verify_password(&hash, "wrong-password"),
            "错误密码不匹配"
        );
        let (balance,): (f64,) = conn
            .query_row("SELECT balance FROM quotas WHERE user_id = 1", [], |r| {
                Ok((r.get(0)?,))
            })
            .unwrap();
        assert_eq!(balance, 0.0, "初始管理员余额为 0");
        // 幂等：再启动不重复创建、不再返回密码
        assert!(bootstrap_admin(&conn).expect("bootstrap 2nd").is_none());
        let users: i64 = conn
            .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))
            .unwrap();
        assert_eq!(users, 1, "仅一个初始管理员");
        let keys: i64 = conn
            .query_row("SELECT COUNT(*) FROM keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(keys, 0, "不种占位 key");
        drop(conn);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn bootstrap_admin_skips_when_users_exist() {
        // 已有用户（测试库）→ bootstrap 跳过，不重复创建
        let (conn, p) = tmp_db("bootskip");
        seed_test_users(&conn).expect("seed test users");
        assert!(bootstrap_admin(&conn).expect("bootstrap").is_none());
        let users: i64 = conn
            .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))
            .unwrap();
        assert_eq!(users, 3, "已有用户不受影响");
        let admin: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM users WHERE email = 'admin@aitokenpool.local'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(admin, 1, "测试库自带 admin 仍在（非 bootstrap 创建）");
        drop(conn);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn seed_models_upserts_from_config_models() {
        let (conn, p) = tmp_db("models");
        let cfg = crate::config::Config::load("config/config.example.toml").unwrap();
        seed_models(&conn, &cfg).expect("seed models");
        // config.toml [[models]] 的模型已入库
        let cnt: i64 = conn
            .query_row("SELECT COUNT(*) FROM models", [], |r| r.get(0))
            .unwrap();
        assert!(cnt >= 10, "models 已从 config [[models]] seed，cnt={cnt}");
        // deepseek-v4-pro = config 官方 CNY 价（4.5 / 13.5 / 缓存命中 0.15）+ 高峰价（9.0 / 27.0 / 0.30）
        let (input, output, cache_hit, peak_in, peak_out, peak_cache, currency): (
            f64,
            f64,
            f64,
            f64,
            f64,
            f64,
            String,
        ) = conn
            .query_row(
                "SELECT input_per_m, output_per_m, cache_hit_input_per_m, \
                        peak_input_per_m, peak_output_per_m, peak_cache_hit_input_per_m, currency \
                 FROM models WHERE provider = 'deepseek' AND model = 'deepseek-v4-pro'",
                [],
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
            .unwrap();
        assert!((input - 4.5).abs() < 1e-9, "input={input}");
        assert!((output - 13.5).abs() < 1e-9, "output={output}");
        assert!((cache_hit - 0.15).abs() < 1e-9, "cache_hit={cache_hit}");
        assert!((peak_in - 9.0).abs() < 1e-9, "peak input={peak_in}");
        assert!((peak_out - 27.0).abs() < 1e-9, "peak output={peak_out}");
        assert!(
            (peak_cache - 0.30).abs() < 1e-9,
            "peak cache_hit={peak_cache}"
        );
        assert_eq!(currency, "CNY");
        // flash 也在（config 直接定义；高峰 3.0 / 9.0 / 0.10）
        let (fi, fh, fp_in, fp_out): (f64, f64, f64, f64) = conn
            .query_row(
                "SELECT input_per_m, cache_hit_input_per_m, peak_input_per_m, peak_output_per_m \
                 FROM models WHERE provider = 'deepseek' AND model = 'deepseek-v4-flash'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert!((fi - 1.5).abs() < 1e-9, "flash input={fi}");
        assert!((fh - 0.05).abs() < 1e-9, "flash cache_hit={fh}");
        assert!((fp_in - 3.0).abs() < 1e-9, "flash peak input={fp_in}");
        assert!((fp_out - 9.0).abs() < 1e-9, "flash peak output={fp_out}");
        // 无高峰价的模型 → peak 缺省 0（不启用高峰计费）
        let (z_in, z_cache, z_out): (f64, f64, f64) = conn
            .query_row(
                "SELECT peak_input_per_m, peak_cache_hit_input_per_m, peak_output_per_m FROM models \
                 WHERE provider = 'zhipu' AND model = 'glm-5.3'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (z_in, z_cache, z_out),
            (0.0, 0.0, 0.0),
            "非 DeepSeek 模型高峰价缺省 0"
        );
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
    fn seed_models_deletes_config_removed_rows() {
        // rant 2026-08-20T11:58:33：seed 只 upsert 不删除 → config 移除的模型残留 DB（市场幽灵模型）
        let (conn, p) = tmp_db("models_sync");
        let cfg = crate::config::Config::load("config/config.example.toml").unwrap();
        seed_models(&conn, &cfg).expect("seed models");
        let before: i64 = conn
            .query_row("SELECT COUNT(*) FROM models", [], |r| r.get(0))
            .unwrap();
        // 模拟历史残留：手工插入 config 中不存在的模型（旧版本遗留 / 曾手写 DB）
        conn.execute(
            "INSERT INTO models (provider, model, currency, input_per_m, output_per_m) \
             VALUES ('ghost-provider', 'ghost-model', 'USD', 1.0, 1.0)",
            [],
        )
        .unwrap();
        // 再 seed：ghost 行应被同步删除，数量回到 config 集合大小
        seed_models(&conn, &cfg).expect("二次 seed 应清理 ghost 行");
        let after: i64 = conn
            .query_row("SELECT COUNT(*) FROM models", [], |r| r.get(0))
            .unwrap();
        assert_eq!(after, before, "config 已移除模型应被删除（无幽灵行）");
        let ghost: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM models WHERE provider = 'ghost-provider' AND model = 'ghost-model'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ghost, 0, "ghost 模型行已删除");
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
        // 需要一个属主用户（FK）；生产空库无种子，这里用测试辅助建 demo（id=1）
        seed_test_users(&conn).expect("seed test users");
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
