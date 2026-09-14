//! UI 兜底目录与配置目录的同步门禁（C2010 施工单 / C2011 实现）
//!
//! `ui/js/data.js` 的注释自述其两张表是「对齐 `config.toml [[models]]` 官方价」：
//! - `MODELS` —— 上架表单的模型选择兜底（`PROVIDERS` 也由它派生）⇒ 是**配置目录的镜像**；
//! - `MARKET` —— 游客浏览市场（游客可见）⇒ 是 `MODELS` 的**子集**。
//!
//! 两者的耦合不是文案层面的：`ui/js/app.js` 的详情渲染**按模型名**跨表查
//! `D.MODELS.find((x) => x.model === m.model)` 来填「最大 tokens」单元格，
//! 查不到就回落成「未上架」。因此一旦 `config.example.toml` 改了模型名而 `data.js` 没跟，
//! **游客会看到不存在的模型名、错误的价格和「未上架」单元格** —— 界面不报错、测试不报错。
//!
//! 这正是 `ce6d0db` 真实发生过的事故（同一次提交改了 `config.example.toml` 与「同步」
//! `ui/js/data.js`，却漏改了 `MARKET`，并往 `MODELS` 里塞进一个只存在于 JS 的幽灵模型），
//! 而当时**没有任何自动化守卫**能发现它。本模块把这条对应关系固化进 `cargo test`。
//!
//! 设计约束（与 `i18n_pack.rs` 同型）：
//! - **仅测试期编译**（`#[cfg(test)] mod`，见 `main.rs`），不进生产二进制；
//! - **零新依赖**：配置侧直接复用生产解析器（`toml` 已是依赖，且 `Config` 就是生产结构体），
//!   JS 侧沿用 `i18n_pack.rs` 的逐字节扫描（仓库**没有** `regex`，也没有也不该有 JS 工具链）；
//! - **只做静态扫描，不执行 JS**。

use std::collections::{BTreeMap, BTreeSet};

/// 前端兜底数据与配置真源在**编译期**读入：测试不依赖工作目录与文件系统布局。
///
/// `config/config.example.toml` 本身已经是生产二进制的内嵌配置
/// （`main.rs` 的 `DEFAULT_CONFIG`），因此这里 `include_str!` 它不引入任何新东西。
const DATA_JS: &str = include_str!("../ui/js/data.js");
const CONFIG_TOML: &str = include_str!("../config/config.example.toml");

/// 阳性对照真值：**改动模型目录 / 兜底表 / plan 清单时应刻意更新这些数字**。
///
/// 它们的作用是把「提取器静默失真」与「数据真的变了」区分开：若扫描器写错而返回空集，
/// 集合断言会**在空集上"通过"**（C2005 坑 68），这些计数会先把运行中止。
const MODEL_COUNT: usize = 13;
const MARKET_COUNT: usize = 7;
const PLAN_COUNT: usize = 12;

/// 已知真值（从配置里读出的官方价，CNY 计价）——用于确认解析器真的读到了正确字段。
const KNOWN_MODEL: &str = "deepseek-flash";
const KNOWN_INPUT: f64 = 1.0;
const KNOWN_OUTPUT: f64 = 4.0;

/// 价格比较的容差。两侧都由十进制字面量解析而来，实际是精确相等；
/// 留一个极小容差只为避免浮点表示差异造成的假红。
const EPS: f64 = 1e-9;

/// 一个模型的**可比字段**（不含纯展示字段如 `tag`）。
#[derive(Debug, Clone, PartialEq)]
struct Row {
    provider: String,
    currency: String,
    input: f64,
    output: f64,
    ctx: i64,
    /// 缺失表示为 `None`：配置里 `max_output = 0`（`#[serde(default)]`）与
    /// `data.js` 里的 `max: null` 是同一语义。
    max: Option<i64>,
}

// ---------------------------------------------------------------- 扫描：剪切数组

/// 切出 `marker` 之后的**第一个方括号数组**的内部文本（不含方括号）。
///
/// 用括号配对而不是正则：数组里含字符串（`tag: "推理"`、`note: "…/api/coding/paas/v4…"`），
/// 扫描时必须跳过字符串内部，否则字符串里的 `]` 会提前终止区段。
fn array_region<'a>(src: &'a str, marker: &str) -> &'a str {
    let s = src
        .find(marker)
        .unwrap_or_else(|| panic!("在 ui/js/data.js 中未找到标记 `{marker}` —— 兜底表结构变了？"));
    let rest = &src[s..];
    let open = rest
        .find('[')
        .unwrap_or_else(|| panic!("标记 `{marker}` 之后未找到 `[` —— 兜底表结构变了？"));
    let b = rest.as_bytes();
    let mut depth = 0i32;
    let mut i = open;
    while i < b.len() {
        match b[i] {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return &rest[open + 1..i];
                }
            }
            b'"' | b'\'' => {
                let q = b[i];
                i += 1;
                while i < b.len() && b[i] != q {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    panic!("标记 `{marker}` 的数组未闭合 —— 兜底表结构变了？")
}

/// 切出区段内的**顶层对象字面量**（不含花括号），同样跳过字符串内部。
fn top_level_objects(region: &str) -> Vec<&str> {
    let b = region.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'{' {
            let start = i;
            let mut depth = 0i32;
            while i < b.len() {
                match b[i] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            out.push(&region[start + 1..i]);
                            i += 1;
                            break;
                        }
                    }
                    b'"' | b'\'' => {
                        let q = b[i];
                        i += 1;
                        while i < b.len() && b[i] != q {
                            if b[i] == b'\\' {
                                i += 1;
                            }
                            i += 1;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    out
}

// ------------------------------------------------------- 扫描：取对象里的字段值

/// 取出 `key: <value>` 的**原始文本**（字符串含引号）。
///
/// 键名匹配要求：前一个字符不是标识符字符 / `.`（否则 `peakIn` 里的 `in`、
/// `"minimax"` 里的 `in` 会被误命中），且键名之后紧跟 `:`。
fn field_raw<'a>(obj: &'a str, key: &str) -> Option<&'a str> {
    let b = obj.as_bytes();
    let kb = key.as_bytes();
    let mut i = 0;
    while i + kb.len() <= b.len() {
        if b[i..].starts_with(kb) {
            let prev_ok = i == 0
                || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_' || b[i - 1] == b'.');
            let after = i + kb.len();
            if prev_ok && after < b.len() && b[after] == b':' {
                let mut k = after + 1;
                while k < b.len() && matches!(b[k], b' ' | b'\t') {
                    k += 1;
                }
                if k >= b.len() {
                    return None;
                }
                if b[k] == b'"' || b[k] == b'\'' {
                    let q = b[k];
                    let mut e = k + 1;
                    while e < b.len() && b[e] != q {
                        if b[e] == b'\\' {
                            e += 1;
                        }
                        e += 1;
                    }
                    if e >= b.len() {
                        return None; // 引号未闭合：语料异常，交给上层断言处理
                    }
                    return Some(&obj[k..=e]);
                }
                let mut e = k;
                while e < b.len() && !matches!(b[e], b',' | b'}' | b'\n' | b'\r') {
                    e += 1;
                }
                return Some(obj[k..e].trim());
            }
        }
        i += 1;
    }
    None
}

fn field_string(obj: &str, key: &str) -> Option<String> {
    let raw = field_raw(obj, key)?;
    let t = raw.trim_matches(|c| c == '"' || c == '\'');
    Some(t.to_string())
}

/// 解析 `USD(5.0)` / `CNY(1.5)` 形态，返回（币种, 数值）。
///
/// ⚠️ 数值**就是该币种下的金额**，`USD(5.0)` 意为 5.0 美元。
/// 不要再去乘汇率（C2010 坑 84：探针把 `USD()` 又乘 7.2，假报 4 处价格漂移）。
fn field_money(obj: &str, key: &str) -> Option<(String, f64)> {
    let raw = field_raw(obj, key)?;
    let open = raw.find('(')?;
    let close = raw.rfind(')')?;
    if close <= open {
        return None;
    }
    let cur = raw[..open].trim().to_string();
    let val: f64 = raw[open + 1..close].trim().parse().ok()?;
    Some((cur, val))
}

fn field_int(obj: &str, key: &str) -> Option<i64> {
    let raw = field_raw(obj, key)?;
    raw.split_whitespace().next()?.parse().ok()
}

/// `null` → `Some(None)`；数字 → `Some(Some(n))`；字段不存在 → `None`。
fn field_optional_int(obj: &str, key: &str) -> Option<Option<i64>> {
    let raw = field_raw(obj, key)?;
    if raw == "null" {
        return Some(None);
    }
    raw.parse::<i64>().ok().map(Some)
}

// ------------------------------------------------------------------ 解析两侧数据

/// 从生产解析器读出的配置侧目录。
fn config_side() -> (BTreeMap<String, Row>, BTreeSet<String>) {
    let cfg: crate::config::Config = toml::from_str(CONFIG_TOML)
        .unwrap_or_else(|e| panic!("config/config.example.toml 无法用生产结构体解析：{e}"));
    cfg.validate()
        .expect("config/config.example.toml 应通过生产校验 validate()");

    let mut models = BTreeMap::new();
    for m in &cfg.models {
        models.insert(
            m.model.clone(),
            Row {
                provider: m.provider.clone(),
                currency: m.currency.clone(),
                input: m.input_per_m,
                output: m.output_per_m,
                ctx: m.context_length,
                // 0 与「未设置」同义（字段带 #[serde(default)]）
                max: if m.max_output == 0 {
                    None
                } else {
                    Some(m.max_output)
                },
            },
        );
    }

    let plans: BTreeSet<String> = cfg.plans.iter().map(|p| p.id.clone()).collect();
    (models, plans)
}

/// 从 `ui/js/data.js` 扫出的兜底侧目录。
fn fallback_side() -> (BTreeMap<String, Row>, BTreeSet<String>, BTreeSet<String>) {
    let mut models = BTreeMap::new();
    for obj in top_level_objects(array_region(DATA_JS, "const MODELS = [")) {
        let name = field_string(obj, "model")
            .unwrap_or_else(|| panic!("MODELS 中有一行缺少 model 字段 —— 兜底表结构变了？{obj}"));
        let (cur, input) = field_money(obj, "in")
            .unwrap_or_else(|| panic!("MODELS 的 `{name}` 缺少形如 CNY(1.5) 的 in 字段"));
        let (cur_out, output) = field_money(obj, "out")
            .unwrap_or_else(|| panic!("MODELS 的 `{name}` 缺少形如 CNY(1.5) 的 out 字段"));
        assert_eq!(
            cur, cur_out,
            "MODELS 的 `{name}` 的 in/out 币种不一致（{cur} vs {cur_out}）—— 兜底表语义异常"
        );
        let ctx =
            field_int(obj, "ctx").unwrap_or_else(|| panic!("MODELS 的 `{name}` 缺少 ctx 字段"));
        let max = field_optional_int(obj, "max")
            .unwrap_or_else(|| panic!("MODELS 的 `{name}` 缺少 max 字段（无上限时应写 null）"));
        models.insert(
            name,
            Row {
                provider: field_string(obj, "provider")
                    .unwrap_or_else(|| panic!("MODELS 的 `{}` 缺少 provider 字段", "?")),
                currency: cur,
                input,
                output,
                ctx,
                max,
            },
        );
    }

    let market: BTreeSet<String> = top_level_objects(array_region(DATA_JS, "MARKET: ["))
        .into_iter()
        .map(|obj| {
            field_string(obj, "model")
                .unwrap_or_else(|| panic!("MARKET 中有一行缺少 model 字段 —— 兜底表结构变了？"))
        })
        .collect();

    let plans: BTreeSet<String> = top_level_objects(array_region(DATA_JS, "PLANS: ["))
        .into_iter()
        .map(|obj| {
            field_string(obj, "id")
                .unwrap_or_else(|| panic!("PLANS 中有一行缺少 id 字段 —— 兜底表结构变了？"))
        })
        .collect();

    (models, market, plans)
}

// -------------------------------------------------------------------- 纯比较逻辑
//
// 下面三个函数是**纯函数**（只吃解析结果，不读文件），因此可以在阴性对照里
// 用合成/变异数据证明它们「有能力失败」——否则门禁是否为真无法自证。

/// 双向差集：返回（仅左有 = 多出来的，仅右有 = 缺失的）。
fn set_diff(left: &BTreeSet<String>, right: &BTreeSet<String>) -> (Vec<String>, Vec<String>) {
    (
        left.difference(right).cloned().collect(),
        right.difference(left).cloned().collect(),
    )
}

/// 子集违例：`sub` 中不在 `sup` 里的名字。
fn subset_violations(sub: &BTreeSet<String>, sup: &BTreeSet<String>) -> Vec<String> {
    sub.difference(sup).cloned().collect()
}

/// 逐字段比较同一模型的两侧数据，返回人类可读的差异说明。
fn row_mismatches(name: &str, js: &Row, cfg: &Row) -> Vec<String> {
    let mut out = Vec::new();
    if js.provider != cfg.provider {
        out.push(format!(
            "{name}: provider 不一致（兜底 {} vs 配置 {}）",
            js.provider, cfg.provider
        ));
    }
    if js.currency != cfg.currency {
        out.push(format!(
            "{name}: 币种不一致（兜底 {} vs 配置 {}）—— 价格量纲已错",
            js.currency, cfg.currency
        ));
    }
    // 价格在**各自声明的币种**下比较：币种不同时上面已报错，这里不再折算。
    if js.currency == cfg.currency {
        if (js.input - cfg.input).abs() > EPS {
            out.push(format!(
                "{name}: 输入价不一致（兜底 {} vs 配置 {} {}）",
                js.input, cfg.input, cfg.currency
            ));
        }
        if (js.output - cfg.output).abs() > EPS {
            out.push(format!(
                "{name}: 输出价不一致（兜底 {} vs 配置 {} {}）",
                js.output, cfg.output, cfg.currency
            ));
        }
    }
    if js.ctx != cfg.ctx {
        out.push(format!(
            "{name}: 上下文长度不一致（兜底 {} vs 配置 {}）",
            js.ctx, cfg.ctx
        ));
    }
    if js.max != cfg.max {
        out.push(format!(
            "{name}: 最大输出不一致（兜底 {:?} vs 配置 {:?}）",
            js.max, cfg.max
        ));
    }
    out
}

/// 对所有共有模型做逐字段比较并汇总。
fn all_row_mismatches(js: &BTreeMap<String, Row>, cfg: &BTreeMap<String, Row>) -> Vec<String> {
    let mut out = Vec::new();
    for (name, jrow) in js {
        if let Some(crow) = cfg.get(name) {
            out.extend(row_mismatches(name, jrow, crow));
        }
    }
    out
}

// -------------------------------------------------------------------------- 测试

#[cfg(test)]
mod tests {
    use super::*;

    /// 配置解析器本身必须先被证明是活的：计数与已知真值都对得上。
    ///
    /// 没有这一条，后面「集合相等」的断言可能只是在**比较两个空集**（C2005 坑 68）
    /// 或在比较一个被我写坏的提取器。
    #[test]
    fn extractors_are_alive() {
        let (cfg_models, cfg_plans) = config_side();
        let (js_models, js_market, js_plans) = fallback_side();

        assert_eq!(
            cfg_models.len(),
            MODEL_COUNT,
            "配置 [[models]] 应解析出 {MODEL_COUNT} 个模型，实得 {} —— 解析器或数据已变",
            cfg_models.len()
        );
        assert_eq!(
            js_models.len(),
            MODEL_COUNT,
            "data.js MODELS 应扫出 {MODEL_COUNT} 行，实得 {} —— 扫描器或数据已变",
            js_models.len()
        );
        assert_eq!(
            js_market.len(),
            MARKET_COUNT,
            "data.js MARKET 应扫出 {MARKET_COUNT} 行，实得 {}",
            js_market.len()
        );
        assert_eq!(
            js_plans.len(),
            PLAN_COUNT,
            "data.js PLANS 应扫出 {PLAN_COUNT} 行，实得 {}",
            js_plans.len()
        );
        assert_eq!(
            cfg_plans.len(),
            PLAN_COUNT,
            "配置 [[plans]] 应解析出 {PLAN_COUNT} 条，实得 {}",
            cfg_plans.len()
        );

        // 已知真值：确认读到的是「价格」而不是别的数字列
        let known = cfg_models
            .get(KNOWN_MODEL)
            .unwrap_or_else(|| panic!("配置中应有 {KNOWN_MODEL}"));
        assert!(
            (known.input - KNOWN_INPUT).abs() < EPS && (known.output - KNOWN_OUTPUT).abs() < EPS,
            "{KNOWN_MODEL} 的官方价应为 {KNOWN_INPUT}/{KNOWN_OUTPUT}，实得 {}/{} —— 解析器读错了字段",
            known.input,
            known.output
        );
    }

    /// ① `MODELS` 与配置 `[[models]]` 必须**同名同数**（镜像关系，双向都要查）。
    ///
    /// 双向是关键，且两个方向对应两类真实事故：
    /// - 「兜底多出来的」= 幽灵模型（`ce6d0db` 塞进 `gemini-3.5-flash-lite`）；
    /// - 「兜底缺失的」= 新增模型忘了同步（`ce6d0db` 之后新增 `deepseek-flash` 时）。
    ///
    /// 只查一个方向会漏掉其中一类（C2010 更正：此前我误以为该断言应为「子集」）。
    #[test]
    fn models_mirror_the_config_catalog() {
        let (cfg_models, _) = config_side();
        let (js_models, _, _) = fallback_side();

        let js_names: BTreeSet<String> = js_models.keys().cloned().collect();
        let cfg_names: BTreeSet<String> = cfg_models.keys().cloned().collect();
        let (phantom, missing) = set_diff(&js_names, &cfg_names);

        assert!(
            phantom.is_empty(),
            "ui/js/data.js 的 MODELS 含配置中不存在的模型（幽灵行，上架表单会列出不存在的模型）：{phantom:?}"
        );
        assert!(
            missing.is_empty(),
            "配置新增的模型未同步进 ui/js/data.js 的 MODELS（上架表单会缺项）：{missing:?}"
        );
    }

    /// ② 游客市场的每一行都必须能在 `MODELS` 里按名解析。
    ///
    /// 判据来自代码本身：`app.js` 的详情渲染按名跨表查 `D.MODELS.find(x => x.model === m.model)`，
    /// 查不到就渲染成「未上架」。游客模式无需登录即可到达（登录页「游客浏览」→ `MARKET`）。
    #[test]
    fn every_market_row_resolves_in_models() {
        let (js_models, js_market, _) = fallback_side();
        let js_names: BTreeSet<String> = js_models.keys().cloned().collect();
        let bad = subset_violations(&js_market, &js_names);
        assert!(
            bad.is_empty(),
            "以下游客市场模型的名称在 MODELS 中查不到（游客会看到「未上架」且无最大 tokens）：{bad:?}"
        );
    }

    /// ③ `PLANS` 的 id 必须与配置 `[[plans]]` 一致。
    ///
    /// 上架表单把 `payload.plan` 直接提交给后端，id 不一致会导致后端拒收。
    #[test]
    fn plans_match_the_config_plans() {
        let (_, cfg_plans) = config_side();
        let (_, _, js_plans) = fallback_side();
        let (only_js, only_cfg) = set_diff(&js_plans, &cfg_plans);
        assert!(
            only_js.is_empty(),
            "ui/js/data.js 的 PLANS 含配置中不存在的 plan id（表单可提交出后端不认的值）：{only_js:?}"
        );
        assert!(
            only_cfg.is_empty(),
            "配置新增的 plan 未同步进 ui/js/data.js 的 PLANS：{only_cfg:?}"
        );
    }

    /// ④ 同名模型的 `provider` / 价格 / 上下文 / 最大输出必须一致。
    ///
    /// 价格在**各自声明的币种**下比较，不做汇率折算（C2010 坑 84）。
    #[test]
    fn shared_model_fields_match_the_config() {
        let (cfg_models, _) = config_side();
        let (js_models, _, _) = fallback_side();
        let diffs = all_row_mismatches(&js_models, &cfg_models);
        assert!(
            diffs.is_empty(),
            "兜底表与配置的模型字段不一致（都是游客/表单直接可见的量）：\n  {}",
            diffs.join("\n  ")
        );
    }

    /// 阴性对照：把真实事故形态注入（幽灵行 / 缺失行 / 改名 / 错价），上述比较必须真的报错。
    ///
    /// 没有这一条，「门禁是否会失败」无法自证 —— 一个永远为真的检查等价于没有检查。
    /// 这里刻意**变异真实解析结果**（而不是另造一套玩具数据），保证检查函数在真实形状上有效。
    #[test]
    fn checker_detects_injected_defects() {
        let (cfg_models, cfg_plans) = config_side();
        let (js_models, js_market, js_plans) = fallback_side();

        // 前置条件：起点是干净的，否则「报了错」说明不了任何事
        let js_names: BTreeSet<String> = js_models.keys().cloned().collect();
        let cfg_names: BTreeSet<String> = cfg_models.keys().cloned().collect();
        assert_eq!(set_diff(&js_names, &cfg_names), (vec![], vec![]));
        assert!(all_row_mismatches(&js_models, &cfg_models).is_empty());
        assert!(subset_violations(&js_market, &js_names).is_empty());

        // ① 幽灵行（`ce6d0db` 的真实形态）：兜底多出一个配置里没有的模型
        let mut phantom_side = js_names.clone();
        phantom_side.insert("gemini-3.5-flash-lite".to_string());
        let (phantom, _) = set_diff(&phantom_side, &cfg_names);
        assert_eq!(
            phantom,
            vec!["gemini-3.5-flash-lite".to_string()],
            "阴性对照失败：幽灵模型未被报出"
        );

        // ② 缺失行：配置新增模型但兜底未同步
        let mut stale_side = js_names.clone();
        stale_side.remove(KNOWN_MODEL);
        let (_, missing) = set_diff(&stale_side, &cfg_names);
        assert_eq!(
            missing,
            vec![KNOWN_MODEL.to_string()],
            "阴性对照失败：兜底缺行未被报出"
        );

        // ③ `MARKET` 引用了已改名的模型 —— 按名解析必须失败
        let mut drifted_market = js_market.clone();
        drifted_market.insert("glm-5.2".to_string()); // 已改名为 glm-5.3
        assert_eq!(
            subset_violations(&drifted_market, &js_names),
            vec!["glm-5.2".to_string()],
            "阴性对照失败：MARKET 中的改名残留未被报出"
        );

        // ④ 错价（真实事故形态：旧的 DeepSeek flash 价格 1.5/4.5，官方已于 2026-09-10 下调为 1.0/4.0）
        let mut wrong = js_models.get(KNOWN_MODEL).cloned().unwrap();
        wrong.input = 1.008;
        wrong.output = 2.016;
        let diffs = row_mismatches(KNOWN_MODEL, &wrong, cfg_models.get(KNOWN_MODEL).unwrap());
        assert_eq!(
            diffs.len(),
            2,
            "阴性对照失败：错价未被报出（应报输入价+输出价 2 条）"
        );

        // ⑤ 币种被写错（量纲错）：必须报出，且不得因为币种不同就去比数值
        let mut bad_cur = js_models.get(KNOWN_MODEL).cloned().unwrap();
        bad_cur.currency = "USD".to_string();
        let diffs = row_mismatches(KNOWN_MODEL, &bad_cur, cfg_models.get(KNOWN_MODEL).unwrap());
        assert!(
            diffs.iter().any(|d| d.contains("币种不一致")),
            "阴性对照失败：币种写错未被报出"
        );

        // ⑥ plan id 拼错
        let mut bad_plans = js_plans.clone();
        bad_plans.insert("zhipu-codng".to_string());
        let (only_js, _) = set_diff(&bad_plans, &cfg_plans);
        assert_eq!(
            only_js,
            vec!["zhipu-codng".to_string()],
            "阴性对照失败：拼错的 plan id 未被报出"
        );

        // ⑦ 阳性对照：干净数据不得被误报
        assert!(subset_violations(&js_market, &js_names).is_empty());
        assert!(all_row_mismatches(&js_models, &cfg_models).is_empty());
    }
}
