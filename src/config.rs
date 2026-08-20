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

fn default_smtp_port() -> u16 {
    587
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

/// 顶层配置
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // P0-A 仅用 server；points/providers/plans 由后续 P0 网关/定价阶段消费（parse 测试已校验）
pub struct Config {
    #[serde(default)]
    pub server: Server,
    /// 邮件服务（可选；未配置时注册验证码走 dev 日志模式）
    #[serde(default)]
    pub mail: Mail,
    /// 日志（rant 2026-08-19T20:54:26：文件输出 + 滚动 + 清理）
    #[serde(default)]
    pub log: Log,
    pub points: Points,
    pub providers: Vec<Provider>,
    pub plans: Vec<Plan>,
    /// 模型目录（rant 2026-08-20T10:27:13：唯一真源，替代 models.json + price_overrides 双层）
    #[serde(default)]
    pub models: Vec<Model>,
}

/// 点数规则（账本层的锚）
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct Points {
    /// 货币锚：USD | CNY
    pub anchor_currency: String,
    /// 1 个单位锚定货币 = 多少「点」
    pub points_per_unit: u32,
    /// 显示名（仅 UI）
    pub display_name: String,
    /// 符号（仅 UI）
    pub symbol: String,
}

/// 提供商（一家模型厂商）
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub country: String,
    pub has_plan: bool,
}

/// Plan 端点（一个可被路由到的上游端点）
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct Plan {
    pub id: String,
    pub provider: String,
    /// 显示名（可选；为空时 /api/plans 按 type 推导）
    #[serde(default)]
    pub name: String,
    /// paygo | token | coding
    #[serde(rename = "type")]
    pub type_: String,
    /// key 前缀约定（错配会 401）
    pub key_prefix: String,
    #[serde(default)]
    pub interactive_only: bool,
    pub endpoints: Vec<Endpoint>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct Endpoint {
    /// openai_chat | anthropic | responses
    pub protocol: String,
    pub base_url: String,
}

/// 模型定义（rant 2026-08-20T10:27:13：config.toml 唯一真源，替代 models.json + price_overrides）
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
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
        assert!(cfg
            .providers
            .iter()
            .any(|p| p.id == "deepseek" && !p.has_plan));
        assert!(cfg
            .providers
            .iter()
            .any(|p| p.id == "zhipu" && p.has_plan && p.country == "CN"));
        // plans
        assert!(cfg.plans.len() >= 7);
        let dp = cfg
            .plans
            .iter()
            .find(|p| p.id == "deepseek-paygo")
            .expect("deepseek-paygo 应存在");
        assert_eq!(dp.provider, "deepseek");
        assert_eq!(dp.type_, "paygo");
        assert_eq!(dp.key_prefix, "sk-");
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
            .find(|m| m.model == "deepseek-v4-flash")
            .unwrap();
        assert_eq!(flash.input_per_m, 1.5);
        assert_eq!(flash.cache_hit_input_per_m, 0.05);
        assert_eq!(flash.peak_input_per_m, 3.0);
        assert_eq!(flash.peak_output_per_m, 9.0);
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
