//! AITokenPool — AI Token 共享池（网关 + 账本）
//!
//! 企业版：内部 key 池 + 员工点数配额
//! 公共版：分享闲置 key 赚点数、消费别人 key
//!
//! 架构定论见 docs/architecture.md（中心化方案 A：平台托管 key + 平台执行调用）
//!
//! P0-A（rant 2026-08-17T22:21:52）：服务骨架 + 配置加载 + SQLite 数据层 + 认证。
//! 启动：cargo run -- --config config/config.toml（默认 config/config.toml，
//! 不存在时提示复制 config.example.toml）。

mod auth;
mod config;
mod dao;
mod db;
mod routes;

use std::sync::Arc;

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "aitokenpool",
    version,
    about = "AI Token 共享池 — 企业 key 池 + 公共共享市场"
)]
struct Args {
    /// 配置文件路径（默认 config/config.toml）
    #[arg(long, default_value = "config/config.toml")]
    config: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args = Args::parse();

    let cfg = match config::Config::load(&args.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("加载配置失败: {e}");
            eprintln!("提示: 请先复制示例配置:");
            eprintln!("  cp config/config.example.toml config/config.toml");
            std::process::exit(1);
        }
    };

    let addr = cfg.server.addr.clone();
    let db_path = cfg.server.db_path.clone();
    log::info!("打开数据库: {db_path}");
    let conn = db::open(&db_path)?;

    let state = routes::AppState::new(conn, Arc::new(cfg));
    let app = routes::router()
        .with_state(state)
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    log::info!("AITokenPool 服务已启动: http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
