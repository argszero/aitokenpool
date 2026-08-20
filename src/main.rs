//! AITokenPool — AI Token 共享池（网关 + 账本）
//!
//! 企业版：内部 key 池 + 员工点数配额
//! 公共版：分享闲置 key 赚点数、消费别人 key
//!
//! 架构定论见 docs/architecture.md（中心化方案 A：平台托管 key + 平台执行调用）
//!
//! P0-A（rant 2026-08-17T22:21:52）：服务骨架 + 配置加载 + SQLite 数据层 + 认证。
//! P0-B（rant 2026-08-18T09:55:57）：网关转发 + 路由故障转移 + 计量账本闭环。
//! 配置：<ATP_DATA_DIR>/config.toml（首次启动自动从 config/config.example.toml 复制；
//! 也可 --config 显式指定其它路径）。

mod auth;
mod billing;
mod config;
mod crypto;
mod dao;
mod db;
mod gateway;
mod gift;
mod mail;
mod protocol;
mod router;
mod routes;
mod sse;

use std::sync::Arc;

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "aitokenpool",
    version,
    about = "AI Token 共享池 — 企业 key 池 + 公共共享市场"
)]
struct Args {
    /// 统一数据目录（rant 2026-08-19T20:53:23）：config.toml + aitokenpool.db + logs/ 都在其下。
    /// 优先级：--data-dir > env ATP_DATA_DIR > 默认 ./data
    #[arg(long, env = "ATP_DATA_DIR", default_value = "./data")]
    data_dir: String,
    /// 配置文件路径（可选；缺省 <data-dir>/config.toml）
    #[arg(long)]
    config: Option<String>,
}

/// 解析数据目录（绝对化 + 去除尾部斜杠），并自动创建目录结构
fn resolve_data_dir(dir: &str) -> anyhow::Result<std::path::PathBuf> {
    let p = std::path::Path::new(dir);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()?.join(p)
    };
    std::fs::create_dir_all(&abs)?;
    std::fs::create_dir_all(abs.join("logs"))?;
    Ok(abs)
}

/// 配置路径：--config 显式指定 → 用之（不自动复制）；否则 <data-dir>/config.toml（不存在则首次复制）
fn config_path(
    data_dir: &std::path::Path,
    explicit: Option<&str>,
) -> anyhow::Result<std::path::PathBuf> {
    match explicit {
        Some(p) => Ok(std::path::PathBuf::from(p)),
        None => ensure_config(data_dir),
    }
}

/// 首次启动：<data-dir>/config.toml 不存在 → 从仓库内置示例复制（rant 2026-08-19T20:53:23）
fn ensure_config(data_dir: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    let target = data_dir.join("config.toml");
    if target.exists() {
        return Ok(target);
    }
    let example = "config/config.example.toml";
    if !std::path::Path::new(example).exists() {
        // 运行目录没有示例（如已安装的二进制）→ 生成最小可用配置
        // 注意：日志系统尚未初始化，用 eprintln 而非 log::warn
        eprintln!("示例配置 {example} 不存在，生成最小配置 {target:?}");
        let min = "[server]\naddr = \"0.0.0.0:8080\"\ndb_path = \"aitokenpool.db\"\n";
        std::fs::write(&target, min)?;
        return Ok(target);
    }
    std::fs::copy(example, &target)
        .map_err(|e| anyhow::anyhow!("复制示例配置到 {target:?} 失败: {e}"))?;
    eprintln!("首次启动：已从 {example} 复制配置到 {target:?}");
    Ok(target)
}

/// 解析日志级别字符串 → LevelFilter（非法值报错）
fn parse_log_level(s: &str) -> anyhow::Result<log::LevelFilter> {
    match s.trim().to_lowercase().as_str() {
        "trace" => Ok(log::LevelFilter::Trace),
        "debug" => Ok(log::LevelFilter::Debug),
        "info" => Ok(log::LevelFilter::Info),
        "warn" => Ok(log::LevelFilter::Warn),
        "error" => Ok(log::LevelFilter::Error),
        "off" => Ok(log::LevelFilter::Off),
        other => Err(anyhow::anyhow!(
            "非法日志级别: {other}（允许 trace|debug|info|warn|error|off）"
        )),
    }
}

/// 初始化日志（rant 2026-08-19T20:54:26：文件输出 + 大小滚动 + 自动清理 + stdout 双写）。
/// 文件：<data-dir>/<log.dir>/aitokenpool.log，按 max_file_size 滚动，保留 max_backups 份。
fn init_logging(data_dir: &std::path::Path, cfg: &config::Log) -> anyhow::Result<()> {
    use log4rs::append::console::ConsoleAppender;
    use log4rs::append::rolling_file::policy::compound::roll::fixed_window::FixedWindowRoller;
    use log4rs::append::rolling_file::policy::compound::trigger::size::SizeTrigger;
    use log4rs::append::rolling_file::policy::compound::CompoundPolicy;
    use log4rs::append::rolling_file::RollingFileAppender;
    use log4rs::config::{Appender, Config as L4Config, Root};

    let level = parse_log_level(&cfg.level)?;
    let log_dir = data_dir.join(&cfg.dir);
    std::fs::create_dir_all(&log_dir)?;

    // 滚动策略：大小触发 + 固定窗口（保留 max_backups 份，自动删除更旧）
    let pattern = log_dir.join(&cfg.file_pattern);
    let roller = FixedWindowRoller::builder()
        .build(&pattern.to_string_lossy(), cfg.max_backups)
        .map_err(|e| anyhow::anyhow!("构建日志滚动器失败: {e}"))?;
    let policy = CompoundPolicy::new(
        Box::new(SizeTrigger::new(cfg.max_file_size)),
        Box::new(roller),
    );
    let file = RollingFileAppender::builder()
        .build(log_dir.join("aitokenpool.log"), Box::new(policy))
        .map_err(|e| anyhow::anyhow!("构建日志文件 appender 失败: {e}"))?;
    let console = ConsoleAppender::builder().build();

    let lcfg = L4Config::builder()
        .appender(Appender::builder().build("file", Box::new(file)))
        .appender(Appender::builder().build("console", Box::new(console)))
        .build(
            Root::builder()
                .appender("file")
                .appender("console")
                .build(level),
        )
        .map_err(|e| anyhow::anyhow!("构建日志配置失败: {e}"))?;
    log4rs::init_config(lcfg).map_err(|e| anyhow::anyhow!("初始化日志系统失败: {e}"))?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // 统一数据目录（rant 2026-08-19T20:53:23）：config/db/logs 同目录，方便 Docker 单 volume 挂载
    let data_dir = resolve_data_dir(&args.data_dir)?;
    let cfg_path = config_path(&data_dir, args.config.as_deref())?;

    let mut cfg = match config::Config::load(cfg_path.to_str().unwrap_or("config.toml")) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("加载配置失败: {e}");
            eprintln!("提示: 请先复制示例配置到数据目录:");
            eprintln!(
                "  cp config/config.example.toml {}/config.toml",
                data_dir.display()
            );
            std::process::exit(1);
        }
    };
    // 日志系统（rant 2026-08-19T20:54:26）：文件 + 滚动 + 双写；配置加载后才能初始化
    init_logging(&data_dir, &cfg.log)?;
    // 数据库路径统一由 data-dir 决定（配置里 db_path 忽略；config.example 的 data/ 前缀也失效）
    cfg.server.db_path = data_dir
        .join("aitokenpool.db")
        .to_string_lossy()
        .into_owned();

    let addr = cfg.server.addr.clone();
    let db_path = cfg.server.db_path.clone();
    log::info!("打开数据库: {db_path}");
    let conn = db::open(&db_path)?;
    db::seed_models(&conn, &cfg)?;

    // 首次启动自动创建初始管理员（rant 2026-08-19T14:35:05）：仅空库时创建，
    // 密码随机生成、仅此一次打印到启动日志，提示立即修改
    if let Some(pw) = db::bootstrap_admin(&conn)? {
        log::warn!("============================================================");
        log::warn!("⚠️ 初始管理员账号已自动创建（仅首次启动）");
        log::warn!("⚠️ 账号: admin@aitokenpool.local");
        log::warn!("⚠️ 初始管理员密码: {pw}");
        log::warn!("⚠️ 请立即登录并修改密码（POST /api/auth/change-password）");
        log::warn!("============================================================");
    }

    // P0-C：主密钥 + 旧明文 key 加密迁移
    let crypto = crypto::Crypto::from_config(&cfg.server.master_key);
    let migrated = db::migrate_key_encryption(&conn, &crypto)?;
    if migrated > 0 {
        log::info!("已加密迁移 {migrated} 条上游 key");
    }

    let cfg = Arc::new(cfg);
    let state = routes::AppState::new(conn, cfg, crypto);
    let app = routes::router()
        .with_state(state)
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    log::info!("AITokenPool 服务已启动: http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_log_level_maps_strings() {
        assert_eq!(parse_log_level("info").unwrap(), log::LevelFilter::Info);
        assert_eq!(parse_log_level("DEBUG").unwrap(), log::LevelFilter::Debug);
        assert_eq!(parse_log_level("warn").unwrap(), log::LevelFilter::Warn);
        assert_eq!(parse_log_level("trace").unwrap(), log::LevelFilter::Trace);
        assert_eq!(parse_log_level("error").unwrap(), log::LevelFilter::Error);
        assert_eq!(parse_log_level("off").unwrap(), log::LevelFilter::Off);
        assert!(parse_log_level("verbose").is_err(), "非法级别应报错");
    }
}
