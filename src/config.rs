//! 配置结构（与 config/config.example.toml 一一对应）
//!
//! 设计约定（见 config/config.example.toml 注释）：
//! - providers / plans / 点数规则是「人手工维护」的配置，需可读、可注释；
//! - 模型目录在 config.toml [[models]]（2026-08-20 rant：唯一真源，替代 json + overrides）。
//!
//! P0-A（rant 2026-08-17T22:21:52）：服务骨架 + 配置加载

use serde::Deserialize;

fn default_addr() -> String {
    "0.0.0.0:8080".to_string()
}
fn default_db_path() -> String {
    "data/aitokenpool.db".to_string()
}
/// 对外可达地址缺省（dev 默认；与 addr 解耦——addr 是监听地址，public_url 是对外地址）
fn default_public_url() -> String {
    "http://localhost:8080".to_string()
}

/// 服务（监听 / 数据库路径 / 主密钥 / 对外地址）——config.example.toml 可缺省，走默认值
#[derive(Debug, Clone, Deserialize)]
pub struct Server {
    #[serde(default = "default_addr")]
    pub addr: String,
    #[serde(default = "default_db_path")]
    pub db_path: String,
    /// 上游 key 主密钥（hex 32 字节；P0-C 起生效；env ATP_MASTER_KEY 优先级更高）
    #[serde(default)]
    pub master_key: String,
    /// 平台对外网关地址（不含 /v1 等路径），供前端「接入方式」端点展示；
    /// 生产设置真实域名（如 https://gateway.example.com）；缺省 http://localhost:8080
    #[serde(default = "default_public_url")]
    pub public_url: String,
}

impl Default for Server {
    fn default() -> Self {
        Server {
            addr: default_addr(),
            db_path: default_db_path(),
            master_key: String::new(),
            public_url: default_public_url(),
        }
    }
}

/// 邮件服务（注册验证码，rant 2026-08-19T14:36:19 方案 B）。
/// 未配置（smtp_host 为空）时进入 dev 模式：验证码打印到日志/响应，便于本地测试；
/// 生产部署必须配置 SMTP，否则注册验证码不真正送达（安全风险由部署方承担）。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Mail {
    #[serde(default)]
    pub smtp_host: String,
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
    #[serde(default)]
    pub smtp_user: String,
    #[serde(default)]
    pub smtp_password: String,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub from_name: String,
    #[serde(default)]
    pub verify_subject: String,
}

/// SMTP 端口的缺省值：**由 `mail.rs` 选用的传输模式决定** ——
/// `SmtpTransport::relay` = implicit TLS ⇒ 465（`lettre` 的 `SUBMISSIONS_PORT`）。
/// 填 587（STARTTLS 口）会在连接后立即对 587 做 TLS 握手 ⇒ rustls `InvalidContentType`
/// （2026-08-20 实测，详见 `src/mail.rs` 文件头）。
/// `config.example.toml` 里的同一个值由 `smtp_port_gate.rs` 守着，两者必须一致。
fn default_smtp_port() -> u16 {
    465
}

impl Mail {
    /// 是否配置了真实 SMTP（生产模式）
    pub fn configured(&self) -> bool {
        !self.smtp_host.is_empty()
    }
}

// ---- 日志（rant 2026-08-19T20:54:26：文件输出 + 大小滚动 + 自动清理）----

fn default_log_dir() -> String {
    "logs".to_string()
}
fn default_log_level() -> String {
    "info".to_string()
}
fn default_log_file_pattern() -> String {
    "aitokenpool.{}.log".to_string()
}
fn default_log_max_file_size() -> u64 {
    10_000_000
}
fn default_log_max_backups() -> u32 {
    7
}

/// 日志配置（[log] 段，随统一数据目录；文件在 <data-dir>/<dir>/，stdout 双写）
#[derive(Debug, Clone, Deserialize)]
pub struct Log {
    /// 相对数据目录的日志目录（默认 "logs"）
    #[serde(default = "default_log_dir")]
    pub dir: String,
    /// trace | debug | info | warn | error（默认 info）
    #[serde(default = "default_log_level")]
    pub level: String,
    /// 滚动文件命名（含 {} 占位符，如 "aitokenpool.{}.log"）
    #[serde(default = "default_log_file_pattern")]
    pub file_pattern: String,
    /// 触发滚动的单文件大小（bytes，默认 10MB）
    #[serde(default = "default_log_max_file_size")]
    pub max_file_size: u64,
    /// 保留的滚动文件数（自动删除更旧，默认 7）
    #[serde(default = "default_log_max_backups")]
    pub max_backups: u32,
}

impl Default for Log {
    fn default() -> Self {
        Log {
            dir: default_log_dir(),
            level: default_log_level(),
            file_pattern: default_log_file_pattern(),
            max_file_size: default_log_max_file_size(),
            max_backups: default_log_max_backups(),
        }
    }
}

// ---- 交易明细归档（rant 2026-10-09T12:28:58：明细写可滚动、有保留期的 JSONL）----

fn default_archive_enabled() -> bool {
    true
}
fn default_archive_dir() -> String {
    "archive".to_string()
}
fn default_archive_max_file_size() -> u64 {
    50_000_000
}
fn default_archive_max_files() -> u32 {
    10
}
fn default_archive_batch() -> i64 {
    2_000
}
fn default_archive_interval_secs() -> u64 {
    60
}

/// 交易明细归档（[archive] 段，rant 2026-10-09T12:28:58 验收项 2）。
///
/// `transactions` 表只增不减会让 SQLite 无限膨胀（dev 实测 97.3 万行、`COUNT(*)` 23.7s）。
/// 明细落到 `TxArchive` 管理的可滚动 JSONL（保留期 = `max_files` × `max_file_size`），
/// SQLite 侧只留保留窗口内的明细与汇总（见 [`Rollup`]）—— 归档的价值在于明细被折叠、删除后仍有原件。
#[derive(Debug, Clone, Deserialize)]
pub struct Archive {
    /// 是否启用（默认 true）
    #[serde(default = "default_archive_enabled")]
    pub enabled: bool,
    /// 归档目录（相对数据目录，默认 "archive"）
    #[serde(default = "default_archive_dir")]
    pub dir: String,
    /// 单文件滚动阈值（bytes，默认 50MB）
    #[serde(default = "default_archive_max_file_size")]
    pub max_file_size: u64,
    /// 保留的归档文件数（更旧的删除，默认 10）
    #[serde(default = "default_archive_max_files")]
    pub max_files: u32,
    /// 每轮最多归档多少行（越小 → 单次持库时间越短，默认 2000）
    #[serde(default = "default_archive_batch")]
    pub batch: i64,
    /// 归档扫描间隔（秒，默认 60；最小 1）
    #[serde(default = "default_archive_interval_secs")]
    pub interval_secs: u64,
}

impl Default for Archive {
    fn default() -> Self {
        Archive {
            enabled: default_archive_enabled(),
            dir: default_archive_dir(),
            max_file_size: default_archive_max_file_size(),
            max_files: default_archive_max_files(),
            batch: default_archive_batch(),
            interval_secs: default_archive_interval_secs(),
        }
    }
}

// ---- 用量明细保留（rant 2026-10-09T12:28:58 的独立余项：第二张只增不减的明细表）----

fn default_usage_retention_enabled() -> bool {
    true
}
fn default_usage_retention_batch() -> i64 {
    2_000
}
fn default_usage_retention_interval_secs() -> u64 {
    60
}

/// 用量明细保留（[usage_retention] 段）。
///
/// `usage_records` 是同一笔 `settle` 写的**第二张**只增不减的明细表。与 `[rollup]`（交易）
/// 不同，它**不需要折叠** —— 每个读者都是写死的月/日窗口（见 `src/usage_retention.rs`），
/// 所以本段只驱动一件事：**先归档、再按日历窗口删**。归档用 `[archive]` 的参数与目录，
/// 落在 `<archive.dir>/usage/` 子目录里（水位文件同名，靠目录隔离）。
///
/// 保留窗口**没有**配置项：门槛是 `usage_retention::KEEP_SINCE`
/// （`date('now','start of month','-1 month')`，保留本月 ＋ 上月）—— 它是读者的同一套
/// 日历谓词，`usage_retention_gate` 守着「门槛 ≤ 每个读者的下界」。把它做成天数配置
/// 反而会引入「30 天够不够 31 天」的论证负担。
#[derive(Debug, Clone, Deserialize)]
pub struct UsageRetention {
    /// 是否启用（默认 true）
    #[serde(default = "default_usage_retention_enabled")]
    pub enabled: bool,
    /// 每轮最多删多少行（越小 → 单次持库时间越短，默认 2000）
    #[serde(default = "default_usage_retention_batch")]
    pub batch: i64,
    /// 扫描间隔（秒，默认 60；最小 1）
    #[serde(default = "default_usage_retention_interval_secs")]
    pub interval_secs: u64,
}

impl Default for UsageRetention {
    fn default() -> Self {
        UsageRetention {
            enabled: default_usage_retention_enabled(),
            batch: default_usage_retention_batch(),
            interval_secs: default_usage_retention_interval_secs(),
        }
    }
}

// ---- 交易明细汇总（rant 2026-10-09T12:28:58：明细只留汇总）----

fn default_rollup_enabled() -> bool {
    true
}
fn default_rollup_batch() -> i64 {
    2_000
}
fn default_rollup_interval_secs() -> u64 {
    60
}
fn default_rollup_retain_days() -> i64 {
    30
}

/// 交易明细汇总（[rollup] 段，rant 2026-10-09T12:28:58 验收项 1）。
///
/// `transactions` 只增不减会让 SQLite 无限膨胀（dev 实测 97.3 万行、`COUNT(*)` 23.7s）。
/// 本段驱动两件合成一件事的动作：**把明细折叠成可加汇总行**（`src/tx_rollup.rs`：维度 =
/// 类型+用户+模型+key+状态+分钟，可加量 = 点数/各 token 的 SUM + 行数），**再删掉**已经折叠且
/// 已归档、又落在保留窗口之外的明细。汇总只折叠**已归档**的明细（`id ≤ [archive] 的水位`），
/// 删除只删**已折叠且已归档**的 —— 于是删掉的行永远有原件（JSONL）与聚合（汇总表）两个落脚处；
/// `[archive]` 关掉时水位 0 ⇒ 不折也不删（fail-closed），这是刻意的。
///
/// `retain_days` 是**明细**的保留窗口（不是汇总的）：窗口内的明细仍逐条留在 `transactions` 里，
/// 交易页照常翻页；窗口外的只剩汇总行（与 JSONL 里的原件）。取 0 即「明细只留汇总」的极值
/// —— 折叠追平后 `transactions` 会被清空，交易页只答得出最近一分钟，**这是刻意的取舍，不是缺陷**。
#[derive(Debug, Clone, Deserialize)]
pub struct Rollup {
    /// 是否启用（默认 true；折叠是「明细只留汇总」的前置，回填 97 万行需要很长时间）
    #[serde(default = "default_rollup_enabled")]
    pub enabled: bool,
    /// 每轮最多折叠多少条**明细**（越小 → 单次持库时间越短，默认 2000）
    #[serde(default = "default_rollup_batch")]
    pub batch: i64,
    /// 折叠扫描间隔（秒，默认 60；最小 1）
    #[serde(default = "default_rollup_interval_secs")]
    pub interval_secs: u64,
    /// 明细的保留窗口（天，默认 30）：早于 `now - retain_days` 的**已折叠**明细每轮删一批。
    /// 0 = 不留明细（只留汇总），负数按 0 计。
    #[serde(default = "default_rollup_retain_days")]
    pub retain_days: i64,
}

impl Default for Rollup {
    fn default() -> Self {
        Rollup {
            enabled: default_rollup_enabled(),
            batch: default_rollup_batch(),
            interval_secs: default_rollup_interval_secs(),
            retain_days: default_rollup_retain_days(),
        }
    }
}

/// 顶层配置
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: Server,
    /// 邮件服务（可选；未配置时注册验证码走 dev 日志模式）
    #[serde(default)]
    pub mail: Mail,
    /// 日志（rant 2026-08-19T20:54:26：文件输出 + 滚动 + 清理）
    #[serde(default)]
    pub log: Log,
    /// 交易明细归档（rant 2026-10-09T12:28:58：明细写可滚动、有保留期的 JSONL）
    #[serde(default)]
    pub archive: Archive,
    /// 交易明细汇总（rant 2026-10-09T12:28:58：明细只留汇总）
    #[serde(default)]
    pub rollup: Rollup,
    /// 用量明细保留（rant 2026-10-09T12:28:58 的独立余项：第二张只增不减的明细表）
    #[serde(default)]
    pub usage_retention: UsageRetention,
    pub points: Points,
    pub providers: Vec<Provider>,
    pub plans: Vec<Plan>,
    /// 模型目录（rant 2026-08-20T10:27:13：唯一真源，替代 models.json + price_overrides 双层）
    #[serde(default)]
    pub models: Vec<Model>,
}

/// 点数规则（账本层的锚）
///
/// 生产代码只读 `anchor_currency` / `points_per_unit`（→ `billing::calc_points`）；
/// `display_name` / `symbol` 只有 config.toml 与下面的 parse 测试在读，接线或删除
/// **尚未裁定** ⇒ 先按**字段**静音，而不是整个结构体：结构体级抑制会把**将来**
/// 新增的字段一并静默，这正是本文件此前那六个属性的问题。
#[derive(Debug, Clone, Deserialize)]
pub struct Points {
    /// 货币锚：USD | CNY
    pub anchor_currency: String,
    /// 1 个单位锚定货币 = 多少「点」
    pub points_per_unit: u32,
    /// 显示名（仅 UI）
    #[allow(dead_code)]
    pub display_name: String,
    /// 符号（仅 UI）
    #[allow(dead_code)]
    pub symbol: String,
}

/// 提供商（一家模型厂商）
///
/// 生产代码只读 `id`（`validate()` 用它校验 plan 的 provider 引用）；`name` / `country`
/// 只有 config.toml 与下面的 parse 测试在读 —— 与 `Points` 那两枚同理，按**字段**静音。
#[derive(Debug, Clone, Deserialize)]
pub struct Provider {
    pub id: String,
    #[allow(dead_code)]
    pub name: String,
    #[allow(dead_code)]
    pub country: String,
}

/// Plan 端点（一个可被路由到的上游端点）
#[derive(Debug, Clone, Deserialize)]
pub struct Plan {
    pub id: String,
    pub provider: String,
    /// 显示名（可选；为空时 /api/plans 按 type 推导）
    #[serde(default)]
    pub name: String,
    /// paygo | token | coding
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default)]
    pub interactive_only: bool,
    pub endpoints: Vec<Endpoint>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Endpoint {
    /// openai_chat | anthropic | responses
    pub protocol: String,
    pub base_url: String,
}

/// 模型定义（rant 2026-08-20T10:27:13：config.toml 唯一真源，替代 models.json + price_overrides）
#[derive(Debug, Clone, Deserialize)]
pub struct Model {
    pub provider: String,
    pub model: String,
    pub currency: String,
    /// 每百万 tokens（缓存未命中输入价）
    pub input_per_m: f64,
    /// 缓存命中输入价（缺省 0 = 命中免费）
    #[serde(default)]
    pub cache_hit_input_per_m: f64,
    /// 每百万 tokens 输出价
    pub output_per_m: f64,
    /// 高峰时段输入价（rant 2026-08-20T11:58:40：DeepSeek 高峰 9-12/14-18 北京时翻倍；
    /// 缺省 0 = 不启用高峰计费，沿用 input_per_m）
    #[serde(default)]
    pub peak_input_per_m: f64,
    /// 高峰时段缓存命中输入价（缺省 0）
    #[serde(default)]
    pub peak_cache_hit_input_per_m: f64,
    /// 高峰时段输出价（缺省 0）
    #[serde(default)]
    pub peak_output_per_m: f64,
    #[serde(default)]
    pub context_length: i64,
    #[serde(default)]
    pub max_output: i64,
    #[serde(default)]
    pub vision: bool,
}

impl Config {
    /// 从 TOML 文件加载配置
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let s = std::fs::read_to_string(path)?;
        let cfg: Config = toml::from_str(&s)?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// 校验规则（issue #6：points_per_unit > 0、plan 引用的 provider 必须存在、
    /// endpoints 至少 1 个、protocol 枚举合法）
    pub fn validate(&self) -> anyhow::Result<()> {
        use anyhow::anyhow;

        if self.points.points_per_unit == 0 {
            return Err(anyhow!("[points] points_per_unit 必须 > 0，当前为 0"));
        }
        if self.providers.is_empty() {
            return Err(anyhow!("providers 不能为空"));
        }
        let ids: std::collections::HashSet<&str> =
            self.providers.iter().map(|p| p.id.as_str()).collect();
        for plan in &self.plans {
            if !ids.contains(plan.provider.as_str()) {
                return Err(anyhow!(
                    "plan[{}] 引用了不存在的 provider: {}",
                    plan.id,
                    plan.provider
                ));
            }
            if plan.endpoints.is_empty() {
                return Err(anyhow!("plan[{}] endpoints 至少 1 个", plan.id));
            }
            for ep in &plan.endpoints {
                match ep.protocol.as_str() {
                    "openai_chat" | "anthropic" | "responses" => {}
                    other => {
                        return Err(anyhow!(
                            "plan[{}] 非法 protocol: {}（允许 openai_chat | anthropic | responses）",
                            plan.id, other
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_config_example_ok() {
        let cfg =
            Config::load("config/config.example.toml").expect("解析 config.example.toml 应成功");
        // 校验规则也应通过
        cfg.validate().expect("example 配置应通过校验");
        // 点数
        assert_eq!(cfg.points.anchor_currency, "CNY");
        assert_eq!(cfg.points.points_per_unit, 1);
        assert_eq!(cfg.points.display_name, "点数");
        assert_eq!(cfg.points.symbol, "P");
        // providers
        assert_eq!(cfg.providers.len(), 6);
        assert!(cfg.providers.iter().any(|p| p.id == "deepseek"));
        assert!(cfg
            .providers
            .iter()
            .any(|p| p.id == "zhipu" && p.country == "CN"));
        // plans
        assert!(cfg.plans.len() >= 7);
        let dp = cfg
            .plans
            .iter()
            .find(|p| p.id == "deepseek-paygo")
            .expect("deepseek-paygo 应存在");
        assert_eq!(dp.provider, "deepseek");
        assert_eq!(dp.type_, "paygo");
        assert_eq!(dp.endpoints.len(), 3);
        assert_eq!(dp.endpoints[0].protocol, "openai_chat");
        assert_eq!(dp.endpoints[0].base_url, "https://api.deepseek.com");
        let al = cfg
            .plans
            .iter()
            .find(|p| p.id == "aliyun-token-plan")
            .expect("aliyun-token-plan 应存在");
        assert!(al.interactive_only);
        // models（rant 2026-08-20T10:27:13：config 唯一真源，替代 json + overrides）
        assert!(
            cfg.models.len() >= 10,
            "models 应从 config [[models]] 解析，len={}",
            cfg.models.len()
        );
        let dv = cfg
            .models
            .iter()
            .find(|m| m.model == "deepseek-v4-pro")
            .unwrap();
        assert_eq!(dv.input_per_m, 4.5);
        assert_eq!(dv.cache_hit_input_per_m, 0.15);
        assert_eq!(dv.currency, "CNY");
        // 高峰价（rant 2026-08-20T11:58:40）：pro 9.0 / 0.30 / 27.0
        assert_eq!(dv.peak_input_per_m, 9.0);
        assert_eq!(dv.peak_cache_hit_input_per_m, 0.30);
        assert_eq!(dv.peak_output_per_m, 27.0);
        let flash = cfg
            .models
            .iter()
            .find(|m| m.model == "deepseek-flash")
            .expect("deepseek-flash（V4.1-Flash，官方 2026-09-14 的现名）应在模型目录中");
        assert_eq!(flash.input_per_m, 1.0);
        assert_eq!(flash.cache_hit_input_per_m, 0.02);
        assert_eq!(flash.output_per_m, 4.0);
        assert_eq!(flash.peak_input_per_m, 2.0);
        assert_eq!(flash.peak_cache_hit_input_per_m, 0.04);
        assert_eq!(flash.peak_output_per_m, 8.0);
        assert!(
            flash.vision,
            "V4.1-Flash 原生多模态（官方「图像理解：支持」）"
        );
        // 已下线的旧名不得再出现在目录中：市场列的是「现在能买的模型」
        // （官方：deepseek-v4-flash / deepseek-v4-flash-vision-exp「对应模型已下线」）
        assert!(
            cfg.models.iter().all(
                |m| m.model != "deepseek-v4-flash" && m.model != "deepseek-v4-flash-vision-exp"
            ),
            "已下线的 DeepSeek 旧模型名不应留在模型目录中"
        );
        // 未配置高峰价的模型 → 缺省 0（不启用高峰计费）
        let zhipu = cfg.models.iter().find(|m| m.provider == "zhipu").unwrap();
        assert_eq!(zhipu.peak_input_per_m, 0.0, "无高峰价字段 → 缺省 0");
        // server 默认值
        assert_eq!(cfg.server.addr, "0.0.0.0:8080");
        assert_eq!(cfg.server.db_path, "data/aitokenpool.db");
        // public_url（rant 2026-08-19T20:37:37：接入方式 URL 配置化）
        assert_eq!(cfg.server.public_url, "https://gateway.example.com");
        // 日志（rant 2026-08-19T20:54:26：文件输出 + 滚动 + 清理）
        assert_eq!(cfg.log.dir, "logs");
        assert_eq!(cfg.log.level, "info");
        assert_eq!(cfg.log.file_pattern, "aitokenpool.{}.log");
        assert_eq!(cfg.log.max_file_size, 10_000_000);
        assert_eq!(cfg.log.max_backups, 7);
    }

    #[test]
    fn log_defaults_when_section_absent() {
        // 未配置 [log] 段 → 全默认值
        let d = Log::default();
        assert_eq!(d.dir, "logs");
        assert_eq!(d.level, "info");
        assert_eq!(d.max_file_size, 10_000_000);
        assert_eq!(d.max_backups, 7);
    }

    #[test]
    fn archive_defaults_when_section_absent() {
        // 未配置 [archive] 段 → 全默认值（rant 2026-10-09T12:28:58）
        let d = Archive::default();
        assert!(d.enabled, "默认启用 —— 缺省即守护明细保留期");
        assert_eq!(d.dir, "archive");
        assert_eq!(d.max_file_size, 50_000_000);
        assert_eq!(d.max_files, 10);
        assert_eq!(d.batch, 2_000);
        assert_eq!(d.interval_secs, 60);
    }

    #[test]
    fn archive_example_section_matches_defaults() {
        // config.example.toml 的 [archive] 段必须解析出与缺省一致的样例值
        // （CONTRIBUTING：涉及配置的改动同步示例文件）
        let cfg = Config::load("config/config.example.toml").unwrap();
        assert!(cfg.archive.enabled);
        assert_eq!(cfg.archive.dir, "archive");
        assert_eq!(cfg.archive.max_file_size, 50_000_000);
        assert_eq!(cfg.archive.max_files, 10);
        assert_eq!(cfg.archive.batch, 2_000);
        assert_eq!(cfg.archive.interval_secs, 60);
    }

    #[test]
    fn rollup_defaults_when_section_absent() {
        // 未配置 [rollup] 段 → 全默认值（rant 2026-10-09T12:28:58 验收项 1）
        let d = Rollup::default();
        assert!(d.enabled, "默认启用 —— 折叠是「明细只留汇总」的前置");
        assert_eq!(d.batch, 2_000);
        assert_eq!(d.interval_secs, 60);
        assert_eq!(d.retain_days, 30, "明细默认保留 30 天");
    }

    #[test]
    fn rollup_example_section_matches_defaults() {
        // config.example.toml 的 [rollup] 段必须解析出与缺省一致的样例值
        // （CONTRIBUTING：涉及配置的改动同步示例文件）
        let cfg = Config::load("config/config.example.toml").unwrap();
        assert!(cfg.rollup.enabled);
        assert_eq!(cfg.rollup.batch, 2_000);
        assert_eq!(cfg.rollup.interval_secs, 60);
        assert_eq!(cfg.rollup.retain_days, Rollup::default().retain_days);
    }

    #[test]
    fn usage_retention_defaults_when_section_absent() {
        // 未配置 [usage_retention] 段 → 全默认值（rant 2026-10-09T12:28:58 的独立余项）
        let d = UsageRetention::default();
        assert!(d.enabled, "默认启用 —— 库不再无限增长的目标句就是方向");
        assert_eq!(d.batch, 2_000);
        assert_eq!(d.interval_secs, 60);
    }

    #[test]
    fn usage_retention_example_section_matches_defaults() {
        // config.example.toml 的 [usage_retention] 段必须解析出与缺省一致的样例值
        // （CONTRIBUTING：涉及配置的改动同步示例文件）
        let cfg = Config::load("config/config.example.toml").unwrap();
        assert!(cfg.usage_retention.enabled);
        assert_eq!(cfg.usage_retention.batch, 2_000);
        assert_eq!(cfg.usage_retention.interval_secs, 60);
    }

    #[test]
    fn server_public_url_defaults_to_localhost() {
        // 未配置 public_url 时缺省 http://localhost:8080（dev 默认，与 addr 解耦）
        assert_eq!(Server::default().public_url, "http://localhost:8080");
    }

    #[test]
    fn validate_rejects_zero_points_per_unit() {
        let mut cfg = Config::load("config/config.example.toml").unwrap();
        cfg.points.points_per_unit = 0;
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("points_per_unit"), "err: {err}");
    }

    #[test]
    fn validate_rejects_missing_provider_ref() {
        let mut cfg = Config::load("config/config.example.toml").unwrap();
        cfg.plans[0].provider = "nonexistent".to_string();
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("不存在的 provider"), "err: {err}");
    }

    #[test]
    fn validate_rejects_empty_endpoints() {
        let mut cfg = Config::load("config/config.example.toml").unwrap();
        cfg.plans[0].endpoints.clear();
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("endpoints 至少 1 个"), "err: {err}");
    }

    #[test]
    fn validate_rejects_illegal_protocol() {
        let mut cfg = Config::load("config/config.example.toml").unwrap();
        cfg.plans[0].endpoints[0].protocol = "grpc".to_string();
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("非法 protocol"), "err: {err}");
    }
}
