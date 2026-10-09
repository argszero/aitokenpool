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
// 语言包不变量门禁（C2006）：仅测试期编译 —— 其中的 include_str! 会把 ui/ 源码
// 嵌进二进制，加 #[cfg(test)] 后不会进入发布产物。
#[cfg(test)]
mod i18n_pack;
// 兜底目录同步门禁（C2011）：同样是仅测试期编译 —— data.js 与 config.example.toml
// 都在编译期读入，加 #[cfg(test)] 后不会进入发布产物。
#[cfg(test)]
mod catalog_gate;
mod mail;
mod protocol;
mod router;
mod routes;
mod sse;
mod tx_archive;
mod tx_facts;
mod tx_rollup;
// 表格结构门禁（C2108）：同样是仅测试期编译 —— ui/index.html 与 ui/js/app.js
// 都在编译期读入，加 #[cfg(test)] 后不会进入发布产物。
#[cfg(test)]
mod table_gate;
// 时间谓词门禁（C2116）：同样是仅测试期编译 —— src/routes/*.rs 在编译期读入，
// 加 #[cfg(test)] 后不会进入发布产物。
#[cfg(test)]
mod perf_gate;
// 部署产物门禁（C2117）：同样是仅测试期编译 —— compose / config 示例 / Dockerfile / README
// 都在编译期读入，加 #[cfg(test)] 后不会进入发布产物。
#[cfg(test)]
mod deploy_gate;
// 前端状态槽归属门禁（C2131）：同样是仅测试期编译 —— ui/js/app.js 在编译期读入，
// 加 #[cfg(test)] 后不会进入发布产物。
#[cfg(test)]
mod state_gate;
// 请求体上限门禁（2026-09-21）：同样是仅测试期编译 —— src/*.rs 在测试期读一遍，
// 加 #[cfg(test)] 后不会进入发布产物。
#[cfg(test)]
mod body_limit_gate;
// 引用门禁（R94）：同样是仅测试期编译 —— 全仓文本文件在测试期读一遍，
// 加 #[cfg(test)] 后不会进入发布产物。
#[cfg(test)]
mod citation_gate;
// 双语文档对偶门禁（R106）：同样是仅测试期编译 —— 两份 README 都在编译期读入，
// 加 #[cfg(test)] 后不会进入发布产物。
#[cfg(test)]
mod readme_gate;
// SMTP 端口门禁（R127）：同样是仅测试期编译 —— config 示例 / config.rs / mail.rs
// 都在编译期读入，加 #[cfg(test)] 后不会进入发布产物。
#[cfg(test)]
mod smtp_port_gate;
// 布局门禁（R128 + R132）：同样是仅测试期编译 —— ui/index.html / ui/css/style.css /
// docs/prototype/aitokenpool-console.html（设计基线）都在编译期读入，
// 加 #[cfg(test)] 后不会进入发布产物。
#[cfg(test)]
mod layout_gate;
// 前端声明门禁（R131）：同样是仅测试期编译 —— ui/index.html 与 ui/js/*.js
// 都在编译期读入，加 #[cfg(test)] 后不会进入发布产物。
#[cfg(test)]
mod js_gate;
// 停机预算门禁（rant 2026-09-30T16:25:29）：同样是仅测试期编译 ——
// 本文件（排水上限）与 docker-compose.yml（stop_grace_period）都在编译期读入，
// 加 #[cfg(test)] 后不会进入发布产物。
#[cfg(test)]
mod shutdown_gate;
// 选项模板转义门禁（R71）：同样是仅测试期编译 —— ui/js/*.js 在编译期读入，
// 加 #[cfg(test)] 后不会进入发布产物。
#[cfg(test)]
mod option_escape_gate;
// 交易类型名册门禁（R94）：同样是仅测试期编译 —— src/routes/wallet.rs 与 ui/js/app.js
// 都在编译期读入，加 #[cfg(test)] 后不会进入发布产物。
#[cfg(test)]
mod tx_type_roster_gate;
// 交易事实读模型门禁（rant 2026-10-09T12:28:58 验收项 1 读侧）：同样是仅测试期编译 ——
// src/ 的 SQL 语料在编译期读入，加 #[cfg(test)] 后不会进入发布产物。
#[cfg(test)]
mod tx_facts_gate;

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

/// 首次启动：<data-dir>/config.toml 不存在 → 从仓库内置示例复制（rant 2026-08-19T20:53:23）；
/// 无示例文件（如独立二进制分发）→ 写入编译期内嵌的完整默认配置（rant 2026-08-20：开箱即用）。
fn ensure_config(data_dir: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    let target = data_dir.join("config.toml");
    if target.exists() {
        return Ok(target);
    }
    let example = "config/config.example.toml";
    let content = if std::path::Path::new(example).exists() {
        // 源码仓库 / Docker 镜像内置示例：带注释可读，直接复制
        std::fs::read_to_string(example)
            .map_err(|e| anyhow::anyhow!("读取示例配置 {example} 失败: {e}"))?
    } else {
        // 独立二进制分发（运行目录无示例）→ 用编译期内嵌的完整默认配置，
        // 含 providers / plans / models 全量模板，首次启动即开箱即用
        // 注意：日志系统尚未初始化，用 eprintln 而非 log::warn
        eprintln!("示例配置 {example} 不存在，使用编译期内嵌默认配置");
        DEFAULT_CONFIG.to_string()
    };
    std::fs::write(&target, content)
        .map_err(|e| anyhow::anyhow!("写入配置到 {target:?} 失败: {e}"))?;
    eprintln!("首次启动：已生成配置 {target:?}");
    Ok(target)
}

/// 编译期内嵌的完整默认配置（2026-08-20：任何安装方式首次启动即开箱即用——
/// 含 providers / plans / models 全量模板，用户按需改 master_key / public_url / 上游 key 即可）
const DEFAULT_CONFIG: &str = include_str!("../config/config.example.toml");

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

/// 停机排水上限（秒）。
///
/// 收到 SIGTERM / SIGINT 后，进程**停止接受新连接**并等待在途请求自然完成；超过这个
/// 上限仍未排完就强制退出。`docker-compose.yml` 的 `stop_grace_period` 必须**大于**
/// 它，否则 docker 的计时器会先到、用 SIGKILL 把正在排水的进程直接打死
/// —— 这两个数的关系由 `shutdown_gate.rs` 守着。
///
/// 已知边界（不假装无损）：SSE / 长连接请求可能被这个上限截断，
/// `/v1/chat/completions` 的流式路径即属此类。
pub const DRAIN_LIMIT_SECS: u64 = 8;

/// 等待停机信号，返回信号名（仅用于日志）。
///
/// `docker stop` / Swarm 更新 / `docker compose up -d` 重建容器都发 **SIGTERM**；
/// 前台 Ctrl-C 发 SIGINT。两者都当作停机信号。
#[cfg(unix)]
async fn shutdown_signal() -> &'static str {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = signal(SignalKind::terminate()).expect("注册 SIGTERM 处理器失败");
    let mut intr = signal(SignalKind::interrupt()).expect("注册 SIGINT 处理器失败");
    tokio::select! {
        _ = term.recv() => "SIGTERM",
        _ = intr.recv() => "SIGINT",
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() -> &'static str {
    let _ = tokio::signal::ctrl_c().await;
    "Ctrl-C"
}

/// 优雅排水：停止接受新连接，等待在途请求自然完成。
///
/// `shutdown` 解析为 `()`（axum 的约定）；**硬上限不在这里** —— 它由 `main()` 的
/// `drain_deadline` 兜底，这样测试可以单独验证「排水本身」而不必触发「强制退出」。
async fn serve_graceful(
    listener: tokio::net::TcpListener,
    app: axum::Router,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}

/// 硬上限计时器：`armed` 收到信号后开始计时，`limit` 内仍未排水完成就调用 `on_timeout`。
///
/// 生产传入「打日志 + `std::process::exit(0)`」；测试传入一个记录闭包 —— 于是
///「超时强制退出」这条路径也有执行者，而不是只活在注释里。
async fn drain_deadline(
    armed: tokio::sync::oneshot::Receiver<()>,
    limit: std::time::Duration,
    on_timeout: impl FnOnce() + Send + 'static,
) {
    if armed.await.is_err() {
        return; // 信号任务没跑成 ⇒ 永不触发
    }
    tokio::time::sleep(limit).await;
    on_timeout();
}

/// 一轮交易明细归档：拿到库就写，拿不到（库正忙）就跳过。返回本轮写入的行数。
///
/// `try_lock` 是**刻意**的：`db` 是全站唯一的 `Mutex<Connection>`，一次宽窗聚合可以在
/// NFS 上占用数秒（rant 2026-10-09T12:28:58 的背景）。归档是**后台维护**，绝不能与请求
/// 抢锁 —— 库正忙时安静跳过，下一轮再来。
fn tx_archive_tick(
    db: &std::sync::Mutex<rusqlite::Connection>,
    ar: &tx_archive::TxArchive,
    batch: i64,
) -> usize {
    match db.try_lock() {
        Ok(conn) => match tx_archive::archive_pending(&conn, ar, batch) {
            Ok(n) => n,
            Err(e) => {
                log::warn!("交易明细归档失败: {e}");
                0
            }
        },
        Err(_) => 0,
    }
}

/// 启动交易明细归档任务：每 `interval_secs` 秒把新事务行追加到可滚动、有保留期的 JSONL。
///
/// rant 2026-10-09T12:28:58 验收项 2（详细记录写入可滚动、有保留期的 JSONL 文件）的调度点；
/// 验收项 1（明细只留汇总）是后续改动，届时「已归档」的水位就是「可安全删除」的分界。
fn spawn_tx_archive(
    db: Arc<std::sync::Mutex<rusqlite::Connection>>,
    data_dir: &std::path::Path,
    cfg: &config::Archive,
) -> anyhow::Result<()> {
    let ar = tx_archive::TxArchive::new(
        data_dir.join(&cfg.dir),
        cfg.max_file_size,
        cfg.max_files as usize,
    )?;
    let batch = cfg.batch;
    let every = std::time::Duration::from_secs(cfg.interval_secs.max(1));
    log::info!(
        "交易明细归档已启用: {}（每 {}s 一批 ≤ {} 行）",
        ar.dir().display(),
        every.as_secs(),
        batch
    );
    let ar = std::sync::Arc::new(ar);
    tokio::spawn(async move {
        loop {
            // 归档是**同步 DB I/O**（库在 NAS 上，见 rant 2026-10-09T12:28:58 的背景）：
            // 放到 blocking 线程执行，别占住 tokio worker —— worker 是 `/healthz` 的命脉。
            let (db, ar) = (db.clone(), ar.clone());
            let n = tokio::task::spawn_blocking(move || tx_archive_tick(&db, &ar, batch))
                .await
                .unwrap_or(0);
            if n > 0 {
                log::debug!("交易明细归档: +{n} 行");
            }
            tokio::time::sleep(every).await;
        }
    });
    Ok(())
}

/// 一轮交易明细汇总：拿到库就折，拿不到（库正忙）就跳过。返回本批折叠的明细行数。
///
/// 与归档同一条纪律：`try_lock` + 有界批次 —— 后台维护绝不与请求抢锁。
/// `archive_dir` 只用来读归档水位：汇总**只允许折叠已归档的明细**，所以归档没跟上的部分
/// 本轮折不动（下一轮再看）。这只会让汇总**落后**于归档，绝不会超前 —— 落后是安全的。
fn tx_rollup_tick(
    db: &std::sync::Mutex<rusqlite::Connection>,
    archive_dir: &std::path::Path,
    batch: i64,
) -> usize {
    match db.try_lock() {
        Ok(conn) => {
            let archived = tx_archive::watermark_at(archive_dir);
            match tx_rollup::fold_pending(&conn, archived, batch) {
                Ok(n) => n,
                Err(e) => {
                    log::warn!("交易明细汇总失败: {e}");
                    0
                }
            }
        }
        Err(_) => 0,
    }
}

/// 启动交易明细汇总任务：每 `interval_secs` 秒把新明细折叠成可加汇总行。
///
/// rant 2026-10-09T12:28:58 验收项 1（明细只留汇总）的调度点。它只写 `transactions_rollup`；
/// 明细的删除与读路径的改接是后续切片 —— 先得让汇总行存在，并且能对明细逐条对账。
fn spawn_tx_rollup(
    db: Arc<std::sync::Mutex<rusqlite::Connection>>,
    archive_dir: std::path::PathBuf,
    cfg: &config::Rollup,
) -> anyhow::Result<()> {
    let batch = cfg.batch;
    let every = std::time::Duration::from_secs(cfg.interval_secs.max(1));
    log::info!(
        "交易明细汇总已启用: {}（每 {}s 一批 ≤ {batch} 行）",
        archive_dir.display(),
        every.as_secs(),
    );
    tokio::spawn(async move {
        loop {
            // 同归档：同步 DB I/O 走 blocking 线程，不占 tokio worker（worker 是 `/healthz` 的命脉）。
            let (db, dir) = (db.clone(), archive_dir.clone());
            let n = tokio::task::spawn_blocking(move || tx_rollup_tick(&db, &dir, batch))
                .await
                .unwrap_or(0);
            if n > 0 {
                log::debug!("交易明细汇总: 折叠 +{n} 行");
            }
            tokio::time::sleep(every).await;
        }
    });
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
    // 归档配置必须在 AppState::new 之前取出 —— cfg 随后被移动到 state 里
    let archive_cfg = cfg.archive.clone();
    let rollup_cfg = cfg.rollup.clone();
    let state = routes::AppState::new(conn, cfg, crypto);
    // 交易明细归档（rant 2026-10-09T12:28:58 验收项 2）：明细写可滚动、有保留期的 JSONL
    if archive_cfg.enabled {
        spawn_tx_archive(state.db.clone(), &data_dir, &archive_cfg)?;
    }
    // 交易明细汇总（同 rant 验收项 1）：明细折叠成可加汇总行。它读**归档水位**作为折叠上界
    // （只折已归档的明细 ⇒ 将来删明细不丢原件），所以要拿到同一个归档目录。
    if rollup_cfg.enabled {
        spawn_tx_rollup(
            state.db.clone(),
            data_dir.join(&archive_cfg.dir),
            &rollup_cfg,
        )?;
    }
    let app = routes::router()
        .with_state(state)
        // 请求体上限（连同外层粗闸）在 `routes::router()` 里与 `routes::GATEWAY_BODY_LIMIT`
        // 绑在一起。这里**曾经**另写一层 `RequestBodyLimitLayer::new(70 * 1024 * 1024)`：
        // 它是独立的一个数，抬上层就会被它悄悄钳住（rant 2026-09-18T09:14:18 的同族缺陷），
        // 故移走 —— 上限只有一个数，见 `body_limit_gate.rs`。
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    log::info!("AITokenPool 服务已启动: http://{addr}");

    // 停机（rant 2026-09-30T16:25:29）：收到 SIGTERM / SIGINT → 停止接受新连接、
    // 等待在途请求完成；`DRAIN_LIMIT_SECS` 秒仍未排完则强制退出。容器侧的
    // `stop_grace_period` 必须大于这个上限（`shutdown_gate.rs` 守着这对数）。
    let (armed_tx, armed_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(drain_deadline(
        armed_rx,
        std::time::Duration::from_secs(DRAIN_LIMIT_SECS),
        || {
            log::warn!("排水超过上限 {DRAIN_LIMIT_SECS}s，强制退出（SSE/长连接可能被截断）");
            std::process::exit(0);
        },
    ));
    serve_graceful(listener, app, async move {
        let sig = shutdown_signal().await;
        log::info!("收到 {sig}：停止接受新连接，等待在途请求完成（上限 {DRAIN_LIMIT_SECS}s）");
        let _ = armed_tx.send(());
    })
    .await?;
    log::info!("在途请求已排水完成，正常退出");
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

    #[test]
    fn embedded_default_config_is_complete() {
        // 内嵌默认配置必须可解析且含全量模板（providers/plans/models），
        // 保证独立二进制分发首次启动即开箱即用（rant 2026-08-20）
        let cfg: crate::config::Config =
            toml::from_str(DEFAULT_CONFIG).expect("内嵌默认配置应能解析");
        assert!(cfg.providers.len() >= 6, "providers 模板");
        assert!(cfg.plans.len() >= 7, "plans 模板");
        assert!(cfg.models.len() >= 10, "models 模板");
    }

    #[test]
    fn ensure_config_writes_default_when_no_example() {
        // 无 config/config.example.toml 文件（模拟独立二进制分发）→ 写入内嵌完整配置
        let dir = std::env::temp_dir().join(format!("atp_ec_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 用临时 cwd 隔离，避免读到仓库里的 config/config.example.toml
        let path = {
            // 构造一个 data_dir，并临时把 cwd 切到空目录（仓库内跑测试时工作区有 example）
            let old = std::env::current_dir().unwrap();
            std::env::set_current_dir(&dir).unwrap();
            let r = ensure_config(std::path::Path::new(&dir));
            std::env::set_current_dir(old).unwrap();
            r.unwrap()
        };
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("[[providers]]"), "内嵌默认应含 providers");
        assert!(content.contains("[[plans]]"), "内嵌默认应含 plans");
        assert!(content.contains("[[models]]"), "内嵌默认应含 models");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- 交易明细归档（rant 2026-10-09T12:28:58 验收项 2）----

    #[test]
    fn tx_archive_tick_writes_then_idles_and_never_blocks_on_a_busy_db() {
        let dir = std::env::temp_dir().join(format!("atp_main_txarchive_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let db = std::sync::Mutex::new(crate::db::open(":memory:").unwrap());
        {
            let conn = db.lock().unwrap();
            // transactions.user_id 有外键 ⇒ 先种用户
            conn.execute(
                "INSERT INTO users (id, email, password_hash) VALUES (1, 'u1@test', 'h')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status) \
                 VALUES (1, '', NULL, 'm', 1, 1, 'consume', '成功')",
                [],
            )
            .unwrap();
        }
        let ar = crate::tx_archive::TxArchive::new(&dir, 1_000_000, 5).unwrap();

        assert_eq!(tx_archive_tick(&db, &ar, 100), 1, "首轮应写入那 1 行");
        assert_eq!(
            tx_archive_tick(&db, &ar, 100),
            0,
            "追平后应为 0（不重复写）"
        );
        assert_eq!(ar.load_watermark(), 1, "水位应停在那行 id");

        // 库被占住 ⇒ 本轮安静跳过（0），而不是阻塞等在锁上
        let guard = db.lock().unwrap();
        assert_eq!(
            tx_archive_tick(&db, &ar, 100),
            0,
            "库正忙时必须跳过，不得抢锁"
        );
        drop(guard);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- 交易明细汇总（rant 2026-10-09T12:28:58 验收项 1）----

    #[test]
    fn tx_rollup_tick_folds_only_archived_rows_and_never_blocks_on_a_busy_db() {
        let dir = std::env::temp_dir().join(format!("atp_main_txrollup_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let db = std::sync::Mutex::new(crate::db::open(":memory:").unwrap());
        {
            let conn = db.lock().unwrap();
            conn.execute(
                "INSERT INTO users (id, email, password_hash) VALUES (1, 'u1@test', 'h')",
                [],
            )
            .unwrap();
            for i in 0..2 {
                conn.execute(
                    "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status, time) \
                     VALUES (1, '', NULL, 'm', 1, 1, 'consume', '成功', ?1)",
                    [format!("2026-10-09 12:34:0{i}")],
                )
                .unwrap();
            }
        }
        let archive_dir = dir.join("archive");
        std::fs::create_dir_all(&archive_dir).unwrap();

        // 归档水位 0（归档还没跟到任何一行）⇒ 一行都不折
        assert_eq!(tx_rollup_tick(&db, &archive_dir, 100), 0, "未归档不得折叠");
        // 归档追到第 2 行 ⇒ 折叠 2 条（同一分钟 ⇒ 一行汇总）
        std::fs::write(archive_dir.join("watermark"), "2").unwrap();
        assert_eq!(tx_rollup_tick(&db, &archive_dir, 100), 2);
        assert_eq!(tx_rollup_tick(&db, &archive_dir, 100), 0, "追平后为 0");
        {
            let conn = db.lock().unwrap();
            let (rows, n): (i64, i64) = conn
                .query_row(
                    "SELECT COUNT(*), COALESCE(SUM(row_count), 0) FROM transactions_rollup",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!((rows, n), (1, 2), "同一分钟两条明细 ⇒ 一行汇总、行数 2");
        }

        // 库被占住 ⇒ 本轮安静跳过（0），而不是阻塞等在锁上
        let guard = db.lock().unwrap();
        assert_eq!(tx_rollup_tick(&db, &archive_dir, 100), 0, "库正忙必须跳过");
        drop(guard);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- 停机排水（rant 2026-09-30T16:25:29）----

    #[tokio::test]
    async fn the_drain_deadline_force_stops_a_stuck_drain() {
        use std::time::Duration;
        let (armed_tx, armed_rx) = tokio::sync::oneshot::channel::<()>();
        let (hit_tx, hit_rx) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn(drain_deadline(
            armed_rx,
            Duration::from_millis(20),
            move || {
                let _ = hit_tx.send(());
            },
        ));
        armed_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), hit_rx)
            .await
            .expect("收到停机信号后，上限内未排完应触发强制退出")
            .expect("超时闭包应被调用");
        task.await.unwrap();
    }

    #[tokio::test]
    async fn the_drain_deadline_stays_silent_until_the_signal_arrives() {
        use std::time::Duration;
        let (armed_tx, armed_rx) = tokio::sync::oneshot::channel::<()>();
        let (hit_tx, mut hit_rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(drain_deadline(
            armed_rx,
            Duration::from_millis(10),
            move || {
                let _ = hit_tx.send(());
            },
        ));
        assert!(
            tokio::time::timeout(Duration::from_millis(200), &mut hit_rx)
                .await
                .is_err(),
            "未收到停机信号时不得触发强制退出"
        );
        drop(armed_tx); // 取消计时任务
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn serve_graceful_finishes_the_in_flight_request() {
        use axum::routing::get;
        use std::time::Duration;

        // 处理器先回报「已进入」，睡 300ms 后应答 —— 停机发生在睡眠期间。
        #[derive(Clone)]
        struct Entered(std::sync::Arc<tokio::sync::Notify>);
        async fn slow(
            axum::extract::State(entered): axum::extract::State<Entered>,
        ) -> impl axum::response::IntoResponse {
            entered.0.notify_one();
            tokio::time::sleep(Duration::from_millis(300)).await;
            // 显式关连接：排水要等的是「在途请求」，不是空闲的 keep-alive 连接
            let mut headers = axum::http::HeaderMap::new();
            headers.insert(
                axum::http::header::CONNECTION,
                axum::http::HeaderValue::from_static("close"),
            );
            (headers, "done")
        }

        let entered = std::sync::Arc::new(tokio::sync::Notify::new());
        let app = axum::Router::new()
            .route("/slow", get(slow))
            .with_state(Entered(entered.clone()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(serve_graceful(listener, app, async move {
            let _ = stop_rx.await;
        }));

        let inflight = tokio::spawn(async move {
            reqwest::Client::new()
                .get(format!("http://{addr}/slow"))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap()
        });
        // 请求真的进了处理器再触发停机 —— 不是靠 sleep 猜
        tokio::time::timeout(Duration::from_secs(5), entered.notified())
            .await
            .expect("在途请求应先进入处理器");
        stop_tx.send(()).unwrap();

        assert_eq!(
            inflight.await.unwrap(),
            "done",
            "在途请求应在排水期内完成，而不是被停机掐断"
        );
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("排水完成后 serve 应返回")
            .unwrap()
            .unwrap();
    }
}
