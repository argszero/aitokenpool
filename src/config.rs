//! 配置结构（与 config/config.example.toml 一一对应）
//!
//! 设计约定（见 config/config.example.toml 注释）：
//! - providers / plans / 点数规则是「人手工维护」的配置，需可读、可注释；
//! - 模型价格大表在 data/models.json，本文件只放「官方价覆盖」price_overrides。
//!
//! P0-A（rant 2026-08-17T22:21:52）：服务骨架 + 配置加载

use serde::Deserialize;

fn default_addr() -> String {
    "0.0.0.0:8080".to_string()
}
fn default_db_path() -> String {
    "data/aitokenpool.db".to_string()
}

/// 服务（监听 / 数据库路径 / 主密钥）——config.example.toml 可缺省，走默认值
#[derive(Debug, Clone, Deserialize)]
pub struct Server {
    #[serde(default = "default_addr")]
    pub addr: String,
    #[serde(default = "default_db_path")]
    pub db_path: String,
    /// 上游 key 主密钥（hex 32 字节；P0-C 起生效；env ATP_MASTER_KEY 优先级更高）
    #[serde(default)]
    pub master_key: String,
}

impl Default for Server {
    fn default() -> Self {
        Server {
            addr: default_addr(),
            db_path: default_db_path(),
            master_key: String::new(),
        }
    }
}

/// 顶层配置
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // P0-A 仅用 server；points/providers/plans 由后续 P0 网关/定价阶段消费（parse 测试已校验）
pub struct Config {
    #[serde(default)]
    pub server: Server,
    pub points: Points,
    pub providers: Vec<Provider>,
    pub plans: Vec<Plan>,
    #[serde(default)]
    pub price_overrides: Vec<PriceOverride>,
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

/// 官方价覆盖（覆盖 data/models.json 聚合源价格）
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct PriceOverride {
    pub provider: String,
    pub model: String,
    pub currency: String,
    pub input_per_m: f64,
    pub output_per_m: f64,
    #[serde(default)]
    pub cache_hit_input_per_m: Option<f64>,
    #[serde(default)]
    pub source: Option<String>,
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
        assert_eq!(cfg.points.anchor_currency, "USD");
        assert_eq!(cfg.points.points_per_unit, 1000);
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
        // price_overrides
        assert!(cfg.price_overrides.len() >= 2);
        let dv = cfg
            .price_overrides
            .iter()
            .find(|o| o.model == "deepseek-v4-pro")
            .unwrap();
        assert_eq!(dv.input_per_m, 0.435);
        assert!(dv.source.is_some());
        // server 默认值
        assert_eq!(cfg.server.addr, "0.0.0.0:8080");
        assert_eq!(cfg.server.db_path, "data/aitokenpool.db");
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
