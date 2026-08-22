//! 计量账本（architecture §4.3/4.4 + P1 点数规则细化）
//!
//! P0-B（rant 2026-08-18T09:55:57）：
//! - 成本 = prompt_tokens × input_per_m/1e6 + completion_tokens × output_per_m/1e6
//!   （models 表单价；货币按 config points.anchor_currency 折算）
//! - 点数 = 锚定货币成本 × points_per_unit
//! - 消费者扣 balance（上游调用前预检，余额 ≤ 0 → 402）
//! - 分享者（key 属主）得 90%（平台抽成 10%），写 transactions（consume / earn）
//! - 写 usage_records；更新 keys.used += tokens
//! - 调用+记账事务性处理：上游失败不入账（settle 只在成功响应后调用）
//!
//! P1（rant 2026-08-18T11:03:02）：
//! - 可用余额 = gift_balance + balance（预检与 settle 一致）
//! - 扣减顺序：先扣最早到期的赠送点数（gift_grants 按 expires_at ASC），不足再扣永久 balance

use anyhow::Result;
use rusqlite::Connection;

/// 分享者分成比例（平台抽成 10%）
pub const SHARE_RATIO: f64 = 0.9;
/// 锚定货币为 CNY 时的 USD 汇率占位（正式汇率表留后续 P0）
pub const CNY_PER_USD: f64 = 7.2;

/// 点数舍入：最多保留 5 位小数（宿主 2026-08-20：1 CNY = 1 点，点数可为小数）
pub fn round5(x: f64) -> f64 {
    (x * 100_000.0).round() / 100_000.0
}

/// 把某货币金额折算到锚定货币（同币种 1:1；USD↔CNY 用 CNY_PER_USD；其它未知币种按 1:1 兜底）
pub fn to_anchor(amount: f64, model_currency: &str, anchor: &str) -> f64 {
    if model_currency == anchor {
        return amount;
    }
    match (anchor, model_currency) {
        ("USD", "CNY") => amount / CNY_PER_USD,
        ("CNY", "USD") => amount * CNY_PER_USD,
        _ => amount,
    }
}

/// 原始成本（模型自身货币）：USD/CNY 每百万 token 单价 × token 数。
/// 输入分「缓存未命中」与「缓存命中」两档价（rant 2026-08-20T10:17:27）：
/// 未命中 × input_per_m + 命中 × cache_hit_input_per_m（缺省 0 → 命中部分免费）
pub fn raw_cost(
    input_tokens: f64,
    cached_tokens: f64,
    output_tokens: f64,
    input_per_m: f64,
    cache_hit_input_per_m: f64,
    output_per_m: f64,
) -> f64 {
    input_tokens * input_per_m / 1_000_000.0
        + cached_tokens * cache_hit_input_per_m / 1_000_000.0
        + output_tokens * output_per_m / 1_000_000.0
}

/// 计算点数：锚定货币成本 × points_per_unit
#[allow(clippy::too_many_arguments)] // 价格字段平铺（输入/命中/输出 × 单价 × 货币锚定），保持可读性
pub fn calc_points(
    input_tokens: f64,
    cached_tokens: f64,
    output_tokens: f64,
    input_per_m: f64,
    cache_hit_input_per_m: f64,
    output_per_m: f64,
    points_per_unit: u32,
    model_currency: &str,
    anchor: &str,
) -> f64 {
    let cost = raw_cost(
        input_tokens,
        cached_tokens,
        output_tokens,
        input_per_m,
        cache_hit_input_per_m,
        output_per_m,
    );
    round5(to_anchor(cost, model_currency, anchor) * points_per_unit as f64)
}

/// 北京时间高峰时段判定（rant 2026-08-20T11:58:40：DeepSeek 高峰 9:00-12:00、14:00-18:00，
/// 周一至周日，其余为空闲时段）。固定 Asia/Shanghai（UTC+8，无夏令时），不依赖服务器时区。
pub fn is_peak_hour(utc: &chrono::DateTime<chrono::Utc>) -> bool {
    use chrono::Timelike;
    let sh = utc.with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).expect("+08:00 合法"));
    let h = sh.hour();
    (9..12).contains(&h) || (14..18).contains(&h)
}

/// 按是否高峰挑选生效价格：高峰时段命中且配置了高峰价（> 0）→ 用高峰价；
/// 高峰价缺省 0 → 沿用空闲价（不启用高峰计费的模型行为不变）。
pub fn effective_prices(
    peak: bool,
    input_per_m: f64,
    cache_hit_input_per_m: f64,
    output_per_m: f64,
    peak_input_per_m: f64,
    peak_cache_hit_input_per_m: f64,
    peak_output_per_m: f64,
) -> (f64, f64, f64) {
    if !peak {
        return (input_per_m, cache_hit_input_per_m, output_per_m);
    }
    (
        if peak_input_per_m > 0.0 {
            peak_input_per_m
        } else {
            input_per_m
        },
        if peak_cache_hit_input_per_m > 0.0 {
            peak_cache_hit_input_per_m
        } else {
            cache_hit_input_per_m
        },
        if peak_output_per_m > 0.0 {
            peak_output_per_m
        } else {
            output_per_m
        },
    )
}

/// 入账参数
pub struct SettleParams {
    /// 消费者（调用者）user_id
    pub consumer_id: i64,
    /// 分发 API key id（可空）
    pub api_key_id: Option<i64>,
    /// 上游 key id
    pub key_id: i64,
    /// 上游 key 属主（分享者）user_id
    pub owner_id: i64,
    pub model: String,
    /// 本次调用 token 总数
    pub tokens: f64,
    /// 缓存命中输入 token 数（v0.7.0，rant 2026-08-20T10:17:27）
    pub cached_tokens: f64,
    /// 输出 token 数（v0.7.4，rant 2026-08-21T14:53:20：单次调用明细 输入/缓存/输出）
    pub output_tokens: f64,
    /// 消费者应扣点数
    pub pts: f64,
    /// 锚定货币成本（usage_records.cost）
    pub cost: f64,
}

/// 事务性入账：扣消费者（先赠送后永久）→ 加分享者 90% → 两条 transactions →
/// usage_records → keys.used。任一步失败整体回滚（调用方只在成功响应后调用，
/// 天然满足「失败不入账」）
pub fn settle(conn: &mut Connection, p: &SettleParams) -> Result<()> {
    let tx = conn.transaction()?;

    // 消费者扣减：先扣最早到期的赠送点数，剩余从永久 balance 扣
    let remaining = crate::gift::deduct_gift_first(&tx, p.consumer_id, p.pts)?;
    tx.execute(
        "UPDATE quotas SET balance = balance - ?1, updated_at = datetime('now') WHERE user_id = ?2",
        rusqlite::params![remaining, p.consumer_id],
    )?;

    // 分享者加 90%（平台抽成 10%；分成点数同样舍入到 5 位小数）
    let earn = round5(p.pts * SHARE_RATIO);
    tx.execute(
        "INSERT OR IGNORE INTO quotas (user_id, balance) VALUES (?1, 0)",
        [p.owner_id],
    )?;
    tx.execute(
        "UPDATE quotas SET balance = balance + ?1, updated_at = datetime('now') WHERE user_id = ?2",
        rusqlite::params![earn, p.owner_id],
    )?;

    // transactions：consume（消费者）+ earn（分享者），counterpart 记对方；
    // api_key_id 记录分发 key（v11，rant 2026-08-22T17:21:39 需求 2：Key 列显示 api_keys.name）
    tx.execute(
        "INSERT INTO transactions (user_id, counterpart, key_id, api_key_id, model, tokens, cached_tokens, output_tokens, pts, type, status) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'consume', '成功')",
        rusqlite::params![
            p.consumer_id,
            p.owner_id.to_string(),
            p.key_id,
            p.api_key_id,
            p.model,
            p.tokens,
            p.cached_tokens,
            p.output_tokens,
            p.pts
        ],
    )?;
    tx.execute(
        "INSERT INTO transactions (user_id, counterpart, key_id, api_key_id, model, tokens, cached_tokens, output_tokens, pts, type, status) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'earn', '成功')",
        rusqlite::params![
            p.owner_id,
            p.consumer_id.to_string(),
            p.key_id,
            p.api_key_id,
            p.model,
            p.tokens,
            p.cached_tokens,
            p.output_tokens,
            earn
        ],
    )?;

    // usage_records
    tx.execute(
        "INSERT INTO usage_records (user_id, api_key_id, key_id, model, tokens, cached_tokens, output_tokens, cost) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            p.consumer_id,
            p.api_key_id,
            p.key_id,
            p.model,
            p.tokens,
            p.cached_tokens,
            p.output_tokens,
            p.cost
        ],
    )?;

    // keys.used 累计（key 不存在 → 报错回滚：账本不允许记到幽灵 key）
    let n = tx.execute(
        "UPDATE keys SET used = used + ?1 WHERE id = ?2",
        rusqlite::params![p.tokens, p.key_id],
    )?;
    if n != 1 {
        return Err(anyhow::anyhow!("key {} not found", p.key_id));
    }

    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn tmp_db(tag: &str) -> (Connection, std::path::PathBuf) {
        let p = std::env::temp_dir().join(format!("atp_bill_{}_{}.db", std::process::id(), tag));
        let _ = std::fs::remove_file(&p);
        let conn = db::open(p.to_str().unwrap()).expect("open tmp db");
        db::seed_test_users(&conn).expect("seed test users");
        (conn, p)
    }

    #[test]
    fn to_anchor_same_currency_identity() {
        assert_eq!(to_anchor(10.0, "USD", "USD"), 10.0);
        assert_eq!(to_anchor(10.0, "CNY", "CNY"), 10.0);
    }

    #[test]
    fn to_anchor_cross_currency() {
        // USD 锚定：CNY 价 ÷ 7.2
        assert!((to_anchor(72.0, "CNY", "USD") - 10.0).abs() < 1e-9);
        // CNY 锚定：USD 价 × 7.2
        assert!((to_anchor(10.0, "USD", "CNY") - 72.0).abs() < 1e-9);
        // 未知币种按 1:1 兜底
        assert_eq!(to_anchor(5.0, "EUR", "USD"), 5.0);
    }

    #[test]
    fn calc_points_basic() {
        // 100 prompt × 10 USD/M + 50 completion × 20 USD/M = 0.002 USD
        // → CNY 锚定 ×7.2 = 0.0144 CNY → 1 点/CNY = 0.0144 点（保留 5 位小数）
        let pts = calc_points(100.0, 0.0, 50.0, 10.0, 0.0, 20.0, 1, "USD", "CNY");
        assert!((pts - 0.0144).abs() < 1e-9, "pts={pts}");
    }

    #[test]
    fn calc_points_currency_conversion() {
        // 1000 prompt × 10 CNY/M = 0.01 CNY → 1 点/CNY = 0.01 点
        let pts = calc_points(1000.0, 0.0, 0.0, 10.0, 0.0, 0.0, 1, "CNY", "CNY");
        assert!((pts - 0.01).abs() < 1e-9, "pts={pts}");
        // USD 价 → CNY 锚定 ×7.2：1000 prompt × 10 USD/M = 0.01 USD = 0.072 CNY
        let pts_usd = calc_points(1000.0, 0.0, 0.0, 10.0, 0.0, 0.0, 1, "USD", "CNY");
        assert!((pts_usd - 0.072).abs() < 1e-9, "pts_usd={pts_usd}");
    }

    #[test]
    fn calc_points_cache_hit_mixed_pricing() {
        // rant 2026-08-20T10:17:27：输入分「缓存未命中 / 命中」两档价。
        // 1000 未命中 × 1.5 CNY/M + 1000 命中 × 0.05 CNY/M + 1000 输出 × 4.5 CNY/M
        // = (1500 + 50 + 4500)/1e6 CNY = 0.00605 CNY → 1 点/CNY = 0.00605 点
        let pts = calc_points(1000.0, 1000.0, 1000.0, 1.5, 0.05, 4.5, 1, "CNY", "CNY");
        assert!((pts - 0.00605).abs() < 1e-9, "pts={pts}");
        // 命中价缺省 0 → 命中部分不计费（只按未命中 + 输出）
        let pts_zero = calc_points(1000.0, 1000.0, 1000.0, 1.5, 0.0, 4.5, 1, "CNY", "CNY");
        let expect_zero = (1500.0 + 4500.0) / 1e6;
        assert!((pts_zero - expect_zero).abs() < 1e-9, "pts_zero={pts_zero}");
    }

    #[test]
    fn is_peak_hour_beijing_boundaries() {
        // rant 2026-08-20T11:58:40：高峰 = 北京时 9:00-12:00、14:00-18:00（周一至周日）。
        // 边界测试：8:59 vs 9:00、11:59 vs 12:00、13:59 vs 14:00、17:59 vs 18:00（北京时 = UTC+8）
        fn beijing(h: u32, mi: u32) -> chrono::DateTime<chrono::Utc> {
            use chrono::TimeZone;
            let dt = chrono::FixedOffset::east_opt(8 * 3600)
                .unwrap()
                .with_ymd_and_hms(2026, 8, 20, h, mi, 0)
                .unwrap();
            dt.with_timezone(&chrono::Utc)
        }
        // 9:00 高峰开始
        assert!(is_peak_hour(&beijing(9, 0)), "北京 9:00 应高峰");
        assert!(!is_peak_hour(&beijing(8, 59)), "北京 8:59 应空闲");
        // 12:00 高峰结束
        assert!(!is_peak_hour(&beijing(12, 0)), "北京 12:00 应空闲");
        assert!(is_peak_hour(&beijing(11, 59)), "北京 11:59 应高峰");
        // 14:00 第二段高峰开始
        assert!(is_peak_hour(&beijing(14, 0)), "北京 14:00 应高峰");
        assert!(!is_peak_hour(&beijing(13, 59)), "北京 13:59 应空闲");
        // 18:00 第二段高峰结束
        assert!(!is_peak_hour(&beijing(18, 0)), "北京 18:00 应空闲");
        assert!(is_peak_hour(&beijing(17, 59)), "北京 17:59 应高峰");
        // 周日同样高峰（周一至周日全覆盖）
        use chrono::TimeZone;
        let sun = chrono::FixedOffset::east_opt(8 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 8, 23, 10, 0, 0)
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert!(is_peak_hour(&sun), "周日 10:00 应高峰（含周末）");
        // 夜间空闲
        assert!(!is_peak_hour(&beijing(0, 0)), "北京 0:00 应空闲");
        assert!(!is_peak_hour(&beijing(23, 0)), "北京 23:00 应空闲");
    }

    #[test]
    fn effective_prices_selects_peak_or_falls_back() {
        // 空闲时段 → 一律空闲价
        assert_eq!(
            effective_prices(false, 1.5, 0.05, 4.5, 3.0, 0.10, 9.0),
            (1.5, 0.05, 4.5)
        );
        // 高峰时段 + 全量高峰价 → 用高峰价
        assert_eq!(
            effective_prices(true, 1.5, 0.05, 4.5, 3.0, 0.10, 9.0),
            (3.0, 0.10, 9.0)
        );
        // 高峰时段 + 高峰价缺省 0 → 沿用空闲价（不启用高峰计费的模型行为不变）
        assert_eq!(
            effective_prices(true, 1.5, 0.05, 4.5, 0.0, 0.0, 0.0),
            (1.5, 0.05, 4.5)
        );
        // 高峰时段 + 部分缺省 → 缺省项沿用空闲价
        assert_eq!(
            effective_prices(true, 1.5, 0.05, 4.5, 3.0, 0.0, 9.0),
            (3.0, 0.05, 9.0)
        );
    }

    #[test]
    fn calc_points_uses_peak_price_in_peak_hour() {
        // CNY 锚定 1 点/元：高峰 1M 输入 × 3.0 CNY/M = 3.0 CNY = 3.0 点；空闲 1.5 → 1.5 点
        let peak = effective_prices(true, 1.5, 0.05, 4.5, 3.0, 0.10, 9.0);
        let off = effective_prices(false, 1.5, 0.05, 4.5, 3.0, 0.10, 9.0);
        let pts_peak = calc_points(
            1_000_000.0,
            0.0,
            0.0,
            peak.0,
            peak.1,
            peak.2,
            1,
            "CNY",
            "CNY",
        );
        let pts_off = calc_points(1_000_000.0, 0.0, 0.0, off.0, off.1, off.2, 1, "CNY", "CNY");
        assert!(
            (pts_peak - 3.0).abs() < 1e-9,
            "高峰 1M 输入 = 3.0 点: {pts_peak}"
        );
        assert!(
            (pts_off - 1.5).abs() < 1e-9,
            "空闲 1M 输入 = 1.5 点: {pts_off}"
        );
        // 未配置高峰价 → 高峰/空闲点数一致
        let no_peak = effective_prices(true, 1.5, 0.05, 4.5, 0.0, 0.0, 0.0);
        let pts_no = calc_points(
            1_000_000.0,
            0.0,
            0.0,
            no_peak.0,
            no_peak.1,
            no_peak.2,
            1,
            "CNY",
            "CNY",
        );
        assert!(
            (pts_no - 1.5).abs() < 1e-9,
            "未配置高峰价 → 1.5 点: {pts_no}"
        );
    }

    #[test]
    fn round5_limits_precision() {
        // 最多 5 位小数：0.123456 → 0.12346；整数保持不变
        assert_eq!(round5(0.123456), 0.12346);
        assert_eq!(round5(2.0), 2.0);
        assert_eq!(round5(0.0144), 0.0144);
        assert_eq!(round5(1.0 / 3.0), 0.33333);
    }

    #[test]
    fn settle_splits_90_10_and_writes_ledger() {
        let (mut conn, p) = tmp_db("settle");
        // 属主用户（user_id=2）与 key
        conn.execute(
            "INSERT INTO users (id, email, password_hash, name, role) VALUES (100, 'owner@t.local', 'x', '分享者', 'user')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO quotas (user_id, balance) VALUES (100, 0)", [])
            .unwrap();
        conn.execute(
            "INSERT INTO keys (id, provider, plan, model, status, owner_id, encrypted_key, quota, used) \
             VALUES (9, 'test', 'test-plan', 'test-model', 'on', 2, 'sk-test', 1000, 0)",
            [],
        )
        .unwrap();
        // demo（user_id=1）余额 12471
        let bal_before: f64 = conn
            .query_row("SELECT balance FROM quotas WHERE user_id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(bal_before, 12471.0);

        let params = SettleParams {
            consumer_id: 1,
            api_key_id: Some(3),
            key_id: 9,
            owner_id: 100,
            model: "test-model".into(),
            tokens: 150.0,

            cached_tokens: 30.0,
            output_tokens: 20.0,
            pts: 2.0,
            cost: 0.002,
        };
        settle(&mut conn, &params).unwrap();

        // 消费者扣 2.0
        let bal_c: f64 = conn
            .query_row("SELECT balance FROM quotas WHERE user_id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!((bal_c - (12471.0 - 2.0)).abs() < 1e-9);
        // 分享者加 1.8（90%）
        let bal_o: f64 = conn
            .query_row("SELECT balance FROM quotas WHERE user_id = 100", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!((bal_o - 1.8).abs() < 1e-9, "owner balance={bal_o}");
        // transactions 两条
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
        let t_consume: String = conn
            .query_row("SELECT type FROM transactions WHERE user_id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(t_consume, "consume");
        let t_earn: String = conn
            .query_row(
                "SELECT type FROM transactions WHERE user_id = 100",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(t_earn, "earn");
        // api_key_id 已写入两条 transactions（rant 2026-08-22T17:21:39 需求 2）
        let ak_consume: Option<i64> = conn
            .query_row(
                "SELECT api_key_id FROM transactions WHERE user_id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ak_consume, Some(3));
        let ak_earn: Option<i64> = conn
            .query_row(
                "SELECT api_key_id FROM transactions WHERE user_id = 100",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ak_earn, Some(3));
        // usage_records 一条
        let u: i64 = conn
            .query_row("SELECT COUNT(*) FROM usage_records", [], |r| r.get(0))
            .unwrap();
        assert_eq!(u, 1);
        // 明细持久化（rant 2026-08-21T14:53:20）：transactions 与 usage_records 均记 cached/output
        let (cached_t, output_t): (f64, f64) = conn
            .query_row(
                "SELECT cached_tokens, output_tokens FROM transactions WHERE user_id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!((cached_t - 30.0).abs() < 1e-9, "consume cached={cached_t}");
        assert!((output_t - 20.0).abs() < 1e-9, "consume output={output_t}");
        let (cached_u, output_u): (f64, f64) = conn
            .query_row(
                "SELECT cached_tokens, output_tokens FROM usage_records",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!((cached_u - 30.0).abs() < 1e-9, "usage cached={cached_u}");
        assert!((output_u - 20.0).abs() < 1e-9, "usage output={output_u}");
        // keys.used 更新
        let used: f64 = conn
            .query_row("SELECT used FROM keys WHERE id = 9", [], |r| r.get(0))
            .unwrap();
        assert!((used - 150.0).abs() < 1e-9);

        drop(conn);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn settle_rolls_back_on_error() {
        let (mut conn, p) = tmp_db("rollback");
        // key 不存在 → keys.used 更新失败 → 整体回滚（无 transactions / usage_records）
        let params = SettleParams {
            consumer_id: 1,
            api_key_id: None,
            key_id: 99999,
            owner_id: 1,
            model: "m".into(),
            tokens: 10.0,

            cached_tokens: 0.0,
            output_tokens: 0.0,
            pts: 1.0,
            cost: 0.001,
        };
        assert!(settle(&mut conn, &params).is_err());
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "回滚后不应有 transactions");
        let bal: f64 = conn
            .query_row("SELECT balance FROM quotas WHERE user_id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(bal, 12471.0, "回滚后余额不变");

        drop(conn);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn settle_deducts_gift_first_then_permanent() {
        let (mut conn, p) = tmp_db("settle_gift");
        // 消费者 user_id=1：赠送 1 点（当天 23:59:59 过期）+ 永久 10 点
        conn.execute(
            "INSERT INTO gift_grants (user_id, amount, granted_at, expires_at, status) \
             VALUES (1, 1, datetime('now'), strftime('%Y-%m-%d 23:59:59', 'now'), 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE quotas SET gift_balance = 1, balance = 10 WHERE user_id = 1",
            [],
        )
        .unwrap();
        // 分享者 user_id=2 与 key
        conn.execute(
            "INSERT INTO users (id, email, password_hash, name, role) VALUES (100, 'owner2@t.local', 'x', '分享者', 'user')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO quotas (user_id, balance) VALUES (100, 0)", [])
            .unwrap();
        conn.execute(
            "INSERT INTO keys (id, provider, plan, model, status, owner_id, encrypted_key, quota, used) \
             VALUES (8, 'test', 'test-plan', 'test-model', 'on', 2, 'sk-test', 1000, 0)",
            [],
        )
        .unwrap();

        let params = SettleParams {
            consumer_id: 1,
            api_key_id: Some(3),
            key_id: 8,
            owner_id: 100,
            model: "test-model".into(),
            tokens: 100.0,

            cached_tokens: 0.0,
            output_tokens: 0.0,
            pts: 3.0,
            cost: 0.003,
        };
        settle(&mut conn, &params).unwrap();

        // 赠送 1 点全部花掉（used）+ 永久扣 2 点
        let gift: f64 = conn
            .query_row(
                "SELECT gift_balance FROM quotas WHERE user_id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(gift, 0.0, "赠送先扣光");
        let g_status: String = conn
            .query_row(
                "SELECT status FROM gift_grants WHERE user_id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(g_status, "used");
        let bal: f64 = conn
            .query_row("SELECT balance FROM quotas WHERE user_id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!((bal - 8.0).abs() < 1e-9, "永久扣 2 点: {bal}");
        // 分享者照常 90%
        let owner: f64 = conn
            .query_row("SELECT balance FROM quotas WHERE user_id = 100", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!((owner - 2.7).abs() < 1e-9, "分享者 3×0.9=2.7: {owner}");
        // 两条 transactions
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);

        drop(conn);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn settle_expired_gift_not_consumed() {
        let (mut conn, p) = tmp_db("settle_expired");
        // 一笔已过期（昨天）的赠送：settle 前惰性清理 → 只扣永久
        conn.execute(
            "INSERT INTO gift_grants (user_id, amount, granted_at, expires_at, status) \
             VALUES (1, 1, datetime('now', '-1 day'), strftime('%Y-%m-%d 23:59:59', 'now', '-1 day'), 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE quotas SET gift_balance = 1, balance = 10 WHERE user_id = 1",
            [],
        )
        .unwrap();
        let params = SettleParams {
            consumer_id: 1,
            api_key_id: None,
            key_id: 1, // seed 里的 demo key
            owner_id: 1,
            model: "m".into(),
            tokens: 10.0,

            cached_tokens: 0.0,
            output_tokens: 0.0,
            pts: 2.0,
            cost: 0.002,
        };
        settle(&mut conn, &params).unwrap();
        let gift: f64 = conn
            .query_row(
                "SELECT gift_balance FROM quotas WHERE user_id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(gift, 0.0, "过期赠送不参与扣减");
        let g_status: String = conn
            .query_row(
                "SELECT status FROM gift_grants WHERE user_id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(g_status, "expired", "惰性标记 expired");
        let bal: f64 = conn
            .query_row("SELECT balance FROM quotas WHERE user_id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        // 10 - 2（消费）+ 1.8（同属主 90% 分成）= 9.8
        assert!(
            (bal - 9.8).abs() < 1e-9,
            "过期赠送不扣，全部从永久扣: {bal}"
        );

        drop(conn);
        let _ = std::fs::remove_file(p);
    }
}
