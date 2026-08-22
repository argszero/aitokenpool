//! 钱包 / 交易 / 仪表盘 API（对齐原型钱包页 + 交易页 + 仪表盘）
//!
//! P0-C（rant 2026-08-18T10:36:04）：
//! - GET /api/wallet → {balance, month_use, month_earn}
//! - GET /api/transactions?type=&page=&page_size= → 分页 + type 过滤（consume/earn/all）
//! - GET /api/dashboard → 本月按类型聚合 + 净变化 + 近 N 天序列（sparkline）

use axum::extract::{Query, State};
use axum::Json;
use rusqlite::params_from_iter;
use serde::Deserialize;

use crate::dao;
use crate::routes::{internal, ApiErr, AppState, AuthUser};

/// GET /api/wallet
pub async fn wallet(
    State(st): State<AppState>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    // P1：懒加载当日赠送（新人每日 1 点，10 天窗口）
    let _ = crate::gift::ensure_daily_gift(&conn, auth.user_id);
    let (balance, gift_balance) = dao::get_balances(&conn, auth.user_id);
    let month_use: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(pts), 0) FROM transactions \
             WHERE user_id = ?1 AND type = 'consume' AND strftime('%Y-%m', time) = strftime('%Y-%m', 'now')",
            [auth.user_id],
            |r| r.get(0),
        )
        .unwrap_or(0.0);
    let month_earn: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(pts), 0) FROM transactions \
             WHERE user_id = ?1 AND type = 'earn' AND strftime('%Y-%m', time) = strftime('%Y-%m', 'now')",
            [auth.user_id],
            |r| r.get(0),
        )
        .unwrap_or(0.0);
    Ok(Json(serde_json::json!({
        "balance": balance,
        "gift_balance": gift_balance,
        "available": balance + gift_balance,
        "month_use": month_use,
        "month_earn": month_earn,
    })))
}

/// GET /api/transactions 查询参数
#[derive(Debug, Deserialize)]
pub struct TxQuery {
    /// consume / earn / all（缺省 all）
    #[serde(default)]
    pub r#type: String,
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    /// 起始时间（ISO 8601，UTC，SQLite 可解析），time >= start；缺省不限
    pub start: Option<String>,
    /// 结束时间（ISO 8601，UTC，SQLite 可解析），time < end；缺省不限
    pub end: Option<String>,
}

/// 构建 transactions 查询条件与绑定参数。
/// 参数顺序固定：user_id → type → start → end（存在的才加入，占位符序号递增）。
/// `prefix` 非空时列名加前缀（如 "t"），供带 JOIN 的列表查询使用。
fn tx_where(
    prefix: &str,
    user_id: i64,
    type_filter: &Option<String>,
    start: &Option<String>,
    end: &Option<String>,
) -> (String, Vec<rusqlite::types::Value>) {
    let col = |c: &str| {
        if prefix.is_empty() {
            c.to_string()
        } else {
            format!("{prefix}.{c}")
        }
    };
    let mut conds: Vec<String> = Vec::new();
    let mut binds: Vec<rusqlite::types::Value> = Vec::new();
    conds.push(format!("{} = ?1", col("user_id")));
    binds.push(rusqlite::types::Value::Integer(user_id));
    if let Some(t) = type_filter {
        conds.push(format!("{} = ?{}", col("type"), binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(t.clone()));
    }
    // 库内 time 为 datetime('now')（UTC "YYYY-MM-DD HH:MM:SS"），
    // 前端传 ISO 8601（RFC3339）由 handler 用 chrono 规范化为同格式后再比较（字符串序 = 时间序）。
    for (c, v) in [(start, ">="), (end, "<")] {
        if let Some(s) = c {
            if !s.trim().is_empty() {
                conds.push(format!("{} {v} ?{}", col("time"), binds.len() + 1));
                binds.push(rusqlite::types::Value::Text(s.trim().to_string()));
            }
        }
    }
    (conds.join(" AND "), binds)
}

fn default_page() -> u32 {
    1
}
fn default_page_size() -> u32 {
    20
}

/// GET /api/transactions：时间倒序 + type 过滤 + 分页
pub async fn transactions(
    State(st): State<AppState>,
    auth: AuthUser,
    Query(q): Query<TxQuery>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let page = q.page.max(1);
    let page_size = q.page_size.clamp(1, 100);
    let type_filter = match q.r#type.as_str() {
        "" | "all" => None,
        t @ ("consume" | "earn" | "topup" | "gift") => Some(t.to_string()),
        _ => {
            return Err((
                axum::http::StatusCode::BAD_REQUEST,
                Json(
                    serde_json::json!({ "error": "type 必须为 consume / earn / topup / gift / all" }),
                ),
            ))
        }
    };
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    // start/end（rant 2026-08-22T10:50:00）：RFC3339/ISO 8601 → 规范化为 UTC "YYYY-MM-DD HH:MM:SS"
    // （与库内 datetime('now') 一致，字符串比较即时间比较）；非法格式 400。
    let norm = |s: &Option<String>| -> Result<Option<String>, (axum::http::StatusCode, Json<serde_json::Value>)> {
        match s {
            None => Ok(None),
            Some(v) if v.trim().is_empty() => Ok(None),
            Some(v) => chrono::DateTime::parse_from_rfc3339(v.trim())
                .map(|dt| Some(dt.with_timezone(&chrono::Utc).format("%Y-%m-%d %H:%M:%S").to_string()))
                .map_err(|_| {
                    (
                        axum::http::StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({ "error": "时间参数需为 ISO 8601（RFC3339），如 2026-08-22T00:00:00Z" })),
                    )
                }),
        }
    };
    let start = norm(&q.start)?;
    let end = norm(&q.end)?;
    // 汇总（rant 2026-08-22T00:04:21/00:07:08）：全量 SQL 聚合（不依赖分页），
    // 按当前 type + 时间段筛选；口径 = income 白名单（earn/topup/gift）为正、consume 为负。
    let (where_sql, where_binds) = tx_where("", auth.user_id, &type_filter, &start, &end);
    let summary_sql = format!(
        "SELECT \
            COALESCE(SUM(CASE WHEN type IN ('earn','topup','gift') THEN pts ELSE 0 END), 0), \
            COALESCE(SUM(CASE WHEN type = 'consume' THEN pts ELSE 0 END), 0), \
            COALESCE(SUM(CASE WHEN type IN ('earn','topup','gift') THEN pts ELSE -pts END), 0), \
            COALESCE(SUM(tokens), 0), \
            COALESCE(SUM(tokens - cached_tokens - output_tokens), 0), \
            COALESCE(SUM(cached_tokens), 0), \
            COALESCE(SUM(output_tokens), 0) \
            FROM transactions WHERE {where_sql}"
    );
    let summary: serde_json::Value = conn
        .query_row(&summary_sql, params_from_iter(where_binds.iter()), |r| {
            Ok(serde_json::json!({
                "income_pts": r.get::<_, f64>(0)?,
                "expense_pts": r.get::<_, f64>(1)?,
                "net_pts": r.get::<_, f64>(2)?,
                "tokens": r.get::<_, f64>(3)?,
                "input_tokens": r.get::<_, f64>(4)?,
                "cached_tokens": r.get::<_, f64>(5)?,
                "output_tokens": r.get::<_, f64>(6)?,
            }))
        })
        .map_err(internal)?;
    let total: i64 = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM transactions WHERE {where_sql}"),
            params_from_iter(where_binds.iter()),
            |r| r.get(0),
        )
        .unwrap_or(0);
    let offset = (page - 1) * page_size;
    // rant 2026-08-22T06:36:54/06:37:50：模型/Key 列 — 补 key_label（JOIN keys：
    // note 非空用 note，否则 provider / plan，plan 空则仅 provider；key 已删 → NULL）
    let (list_where, mut list_binds) = tx_where("t", auth.user_id, &type_filter, &start, &end);
    let n = list_binds.len();
    list_binds.push(rusqlite::types::Value::Integer(page_size as i64));
    list_binds.push(rusqlite::types::Value::Integer(offset as i64));
    let list_sql = format!(
        "SELECT t.id, t.counterpart, t.key_id, t.api_key_id, t.model, t.tokens, t.cached_tokens, t.output_tokens, \
                t.pts, t.type, t.status, t.time, \
                CASE WHEN k.note <> '' THEN k.note \
                     WHEN k.plan <> '' THEN k.provider || ' / ' || k.plan \
                     ELSE k.provider END AS key_label, \
                u.name AS user_name, \
                ak.name AS key_name \
         FROM transactions t \
         LEFT JOIN keys k ON k.id = t.key_id \
         LEFT JOIN users u ON u.id = t.user_id \
         LEFT JOIN api_keys ak ON ak.id = t.api_key_id \
         WHERE {list_where} \
         ORDER BY t.id DESC LIMIT ?{} OFFSET ?{}",
        n + 1,
        n + 2
    );
    let mut stmt = conn.prepare(&list_sql).map_err(internal)?;
    let rows: Vec<serde_json::Value> = stmt
        .query_map(params_from_iter(list_binds.iter()), |r| {
            let time: String = r.get(11)?;
            let tokens: f64 = r.get(5)?;
            let cached: f64 = r.get(6)?;
            let output: f64 = r.get(7)?;
            let input = tokens - cached - output;
            Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?,
                "counterpart": r.get::<_, String>(1)?,
                "key_id": r.get::<_, Option<i64>>(2)?,
                "api_key_id": r.get::<_, Option<i64>>(3)?,
                "model": r.get::<_, String>(4)?,
                "tokens": tokens,
                "input_tokens": input,
                "cached_tokens": cached,
                "output_tokens": output,
                "pts": r.get::<_, f64>(8)?,
                "type": r.get::<_, String>(9)?,
                "status": r.get::<_, String>(10)?,
                "time": crate::dao::utc_iso(&time),
                "key_label": r.get::<_, Option<String>>(12)?,
                "user_name": r.get::<_, Option<String>>(13)?,
                "key_name": r.get::<_, Option<String>>(14)?,
            }))
        })
        .map_err(internal)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(internal)?;
    Ok(Json(serde_json::json!({
        "items": rows,
        "total": total,
        "page": page,
        "page_size": page_size,
        "summary": summary,
    })))
}

/// GET /api/dashboard：本月按类型聚合 + 净变化 + 近 7 天净额序列
pub async fn dashboard(
    State(st): State<AppState>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    // 本月按类型聚合
    let mut stmt = conn
        .prepare(
            "SELECT type, COALESCE(SUM(pts), 0) FROM transactions \
             WHERE user_id = ?1 AND strftime('%Y-%m', time) = strftime('%Y-%m', 'now') \
             GROUP BY type",
        )
        .map_err(internal)?;
    let month = stmt
        .query_map([auth.user_id], |r| {
            Ok(serde_json::json!({
                "type": r.get::<_, String>(0)?,
                "pts": r.get::<_, f64>(1)?,
            }))
        })
        .map_err(internal)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(internal)?;
    // 近 7 天净额序列（earn/topup/gift 为正、consume 为负；rant 2026-08-22T06:34:37：
    // 原先只把 earn 当正数 → topup 充值被误算为负）
    let mut stmt = conn
        .prepare(
            "SELECT date(time), COALESCE(SUM(CASE WHEN type IN ('earn','topup','gift') THEN pts ELSE -pts END), 0) \
             FROM transactions \
             WHERE user_id = ?1 AND date(time) >= date('now', '-6 days') \
             GROUP BY date(time) ORDER BY date(time)",
        )
        .map_err(internal)?;
    let series = stmt
        .query_map([auth.user_id], |r| {
            let date: String = r.get(0)?;
            Ok(serde_json::json!({
                "date": crate::dao::utc_iso(&date),
                "pts": r.get::<_, f64>(1)?,
            }))
        })
        .map_err(internal)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(internal)?;
    let net: f64 = series
        .iter()
        .map(|s| s["pts"].as_f64().unwrap_or(0.0))
        .sum();
    Ok(Json(serde_json::json!({
        "month": month,
        "net": net,
        "series": series,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::router;
    use axum::body::Body;
    use axum::http::Request;
    use std::sync::Arc;
    use tower::util::ServiceExt;

    fn test_state(tag: &str) -> AppState {
        let p = std::env::temp_dir().join(format!("atp_wallet_{}_{}.db", std::process::id(), tag));
        let _ = std::fs::remove_file(&p);
        let conn = crate::db::open(p.to_str().unwrap()).expect("open tmp db");
        crate::db::seed_test_users(&conn).expect("seed test users");
        let cfg = crate::config::Config::load("config/config.example.toml").unwrap();
        crate::db::seed_models(&conn, &cfg).expect("seed models");
        let crypto = crate::crypto::Crypto::new([17u8; 32]);
        AppState::new(conn, Arc::new(cfg), crypto)
    }

    async fn get(st: AppState, uri: &str, bearer: &str) -> (axum::http::StatusCode, String) {
        let resp = router()
            .with_state(st)
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(uri)
                    .header("authorization", format!("Bearer {bearer}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&bytes).to_string())
    }

    async fn login(st: AppState) -> String {
        let resp = router()
            .with_state(st)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"email":"demo@aitokenpool.local","password":"demo1234"}"#.to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        v["api_key"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn wallet_summary_and_dashboard() {
        let st = test_state("wallet");
        let key = login(st.clone()).await;
        // 种子交易：consume 2.0 + earn 1.8（本月）
        {
            let conn = st.db.lock().unwrap();
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status) VALUES (1, '2', 1, 'm', 150, 2.0, 'consume', '成功')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status) VALUES (1, '3', 2, 'm', 150, 1.8, 'earn', '成功')",
                [],
            )
            .unwrap();
        }
        let (s, body) = get(st.clone(), "/api/wallet", &key).await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!((v["balance"].as_f64().unwrap() - 12471.0).abs() < 1e-9);
        assert!((v["month_use"].as_f64().unwrap() - 2.0).abs() < 1e-9);
        assert!((v["month_earn"].as_f64().unwrap() - 1.8).abs() < 1e-9);

        let (s, body) = get(st.clone(), "/api/dashboard", &key).await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let month = v["month"].as_array().unwrap();
        assert!(month.len() >= 2, "本月 consume+earn 两类聚合: {month:?}");
        let net = v["net"].as_f64().unwrap();
        // 净变化 = earn - consume + gift（demo 今日首次拉 wallet 触发每日赠送 +1 并写 transactions；
        // rant 2026-08-22T00:04:21：赠送入账后净变化含 gift）
        assert!(
            (net - (1.8 - 2.0 + 1.0)).abs() < 1e-9,
            "净变化 = earn - consume + gift: {net}"
        );
        assert!(!v["series"].as_array().unwrap().is_empty(), "近 7 天序列");
    }

    #[tokio::test]
    async fn transactions_summary_and_dashboard_net_with_topup() {
        // rant 2026-08-22T00:04:21/00:07:08/06:34:37：
        // - /api/transactions.summary：income 白名单（earn/topup/gift）为正、consume 为负 + token 统计
        // - dashboard net：topup 充值不得被算成负数
        let st = test_state("txsum");
        let key = login(st.clone()).await;
        {
            let conn = st.db.lock().unwrap();
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status) \
                 VALUES (1, 'admin', NULL, '', 0, 2000.0, 'topup', '成功')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, cached_tokens, output_tokens, pts, type, status) \
                 VALUES (1, '2', 1, 'm', 1000, 200.0, 100.0, 0.5, 'consume', '成功')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status) \
                 VALUES (1, '', NULL, '', 0, 1.0, 'gift', '成功')",
                [],
            )
            .unwrap();
        }
        // all → summary：income=2001（topup+gift）、expense=0.5、net=2000.5；token 统计
        let (s, body) = get(
            st.clone(),
            "/api/transactions?type=all&page=1&page_size=10",
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let sum = &v["summary"];
        assert!(
            (sum["income_pts"].as_f64().unwrap() - 2001.0).abs() < 1e-9,
            "{sum}"
        );
        assert!(
            (sum["expense_pts"].as_f64().unwrap() - 0.5).abs() < 1e-9,
            "{sum}"
        );
        assert!(
            (sum["net_pts"].as_f64().unwrap() - 2000.5).abs() < 1e-9,
            "{sum}"
        );
        assert!(
            (sum["tokens"].as_f64().unwrap() - 1000.0).abs() < 1e-9,
            "{sum}"
        );
        assert!(
            (sum["input_tokens"].as_f64().unwrap() - 700.0).abs() < 1e-9,
            "input = tokens - cached - output = 1000-200-100: {sum}"
        );
        assert!(
            (sum["cached_tokens"].as_f64().unwrap() - 200.0).abs() < 1e-9,
            "{sum}"
        );
        assert!(
            (sum["output_tokens"].as_f64().unwrap() - 100.0).abs() < 1e-9,
            "{sum}"
        );
        // type=consume 筛选 → summary 只含 consume
        let (s, body) = get(
            st.clone(),
            "/api/transactions?type=consume&page=1&page_size=10",
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let sum = &v["summary"];
        assert!((sum["income_pts"].as_f64().unwrap()).abs() < 1e-9, "{sum}");
        assert!(
            (sum["expense_pts"].as_f64().unwrap() - 0.5).abs() < 1e-9,
            "{sum}"
        );
        // dashboard net：topup+gift 为正 → +2000 -0.5 +1 = 2000.5（不得把 topup 算成负）
        let (s, body) = get(st.clone(), "/api/dashboard", &key).await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let net = v["net"].as_f64().unwrap();
        assert!(
            (net - 2000.5).abs() < 1e-9,
            "dashboard net 应含 topup+gift 为正（=2000.5，实际 {net}）"
        );
    }

    #[tokio::test]
    async fn transactions_time_range_filter() {
        // rant 2026-08-22T10:50:00：start/end 时间段过滤（列表 + summary 联动）
        let st = test_state("txrange");
        let key = login(st.clone()).await;
        {
            let conn = st.db.lock().unwrap();
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, model, tokens, pts, type, status, time) \
                 VALUES (1, 'old', 'm', 0, 5.0, 'consume', '成功', datetime('now', '-3 days'))",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, model, tokens, pts, type, status) \
                 VALUES (1, 'new', 'm', 0, 2.0, 'consume', '成功')",
                [],
            )
            .unwrap();
        }
        let now = chrono::Utc::now();
        // 注意：query string 中 "+" 会被解码为空格，故测试用 Z 结尾格式（前端 encodeURIComponent 无此问题）
        let z = |d: chrono::Duration| (now - d).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let start_2d = z(chrono::Duration::hours(48));
        let end_1d = z(chrono::Duration::hours(24));
        let has_pts = |items: &serde_json::Value, pts: f64| {
            items
                .as_array()
                .unwrap()
                .iter()
                .any(|it| (it["pts"].as_f64().unwrap_or(0.0) - pts).abs() < 1e-9)
        };
        let sum_of = |v: &serde_json::Value| -> f64 {
            v["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|it| it["pts"].as_f64().unwrap())
                .sum()
        };
        // start = 2 天前 → 含今天的记录（2.0），不含 3 天前（5.0）
        let (s, body) = get(
            st.clone(),
            &format!("/api/transactions?type=consume&start={start_2d}"),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(has_pts(&v["items"], 2.0), "start 过滤应含今天记录: {body}");
        assert!(
            !has_pts(&v["items"], 5.0),
            "start 过滤应排除 3 天前: {body}"
        );
        // summary 与列表同区间联动（start 过滤后 expense_pts = 区间内列表 pts 之和）
        assert!(
            (v["summary"]["expense_pts"].as_f64().unwrap() - sum_of(&v)).abs() < 1e-9,
            "summary 应随 start 时间段联动: {body}"
        );
        // end = 1 天前 → 含 3 天前（5.0），不含今天（2.0）
        let (s, body) = get(
            st.clone(),
            &format!("/api/transactions?type=consume&end={end_1d}"),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(has_pts(&v["items"], 5.0), "end 过滤应含 3 天前: {body}");
        assert!(!has_pts(&v["items"], 2.0), "end 过滤应排除今天: {body}");
        assert!(
            (v["summary"]["expense_pts"].as_f64().unwrap() - sum_of(&v)).abs() < 1e-9,
            "summary 应随 end 时间段联动: {body}"
        );
        // start+end 窄区间（now-49h ~ now-47h）：显式插入的两条都不在区间
        let (s, body) = get(
            st.clone(),
            &format!(
                "/api/transactions?type=consume&start={}&end={}",
                z(chrono::Duration::hours(49)),
                z(chrono::Duration::hours(47))
            ),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(
            !has_pts(&v["items"], 5.0) && !has_pts(&v["items"], 2.0),
            "窄区间不应含显式插入的两条: {body}"
        );
    }

    #[tokio::test]
    async fn transactions_key_label() {
        // rant 2026-08-22T06:36:54/06:37:50：/api/transactions 每行返回 key_label
        // （note 非空 → note；否则 provider / plan；key 已删/无 key → null）
        let st = test_state("keylabel");
        let key = login(st.clone()).await;
        {
            let conn = st.db.lock().unwrap();
            // 种子 key（id=1，note 空）：key_label = provider / plan
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, cached_tokens, output_tokens, pts, type, status) \
                 VALUES (1, '2', 1, 'deepseek-v4-flash', 1000, 200.0, 100.0, 0.5, 'consume', '成功')",
                [],
            )
            .unwrap();
            // note 非空的 key → key_label = note
            conn.execute(
                "INSERT INTO keys (provider, plan, model, status, owner_id, encrypted_key, quota, note) \
                 VALUES ('openai', 'gpt-paygo', 'gpt-5.3', 'on', 2, 'enc2', 1000, '工作日共享')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, cached_tokens, output_tokens, pts, type, status) \
                 VALUES (1, '2', 2, 'gpt-5.3', 500, 0, 0, 0.3, 'consume', '成功')",
                [],
            )
            .unwrap();
            // 无 key（topup）→ key_label = null
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status) \
                 VALUES (1, 'admin', NULL, '', 0, 2000.0, 'topup', '成功')",
                [],
            )
            .unwrap();
        }
        let (s, body) = get(
            st.clone(),
            "/api/transactions?type=all&page=1&page_size=10",
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let items = v["items"].as_array().unwrap();
        let find = |ty: &str, model: &str| {
            items
                .iter()
                .find(|i| i["type"] == ty && i["model"] == model)
                .expect("row found")
                .clone()
        };
        let seeded = find("consume", "deepseek-v4-flash");
        assert_eq!(seeded["key_label"], "deepseek / deepseek-paygo");
        assert_eq!(seeded["model"], "deepseek-v4-flash");
        let noted = find("consume", "gpt-5.3");
        assert_eq!(noted["key_label"], "工作日共享");
        let topup = find("topup", "");
        assert!(
            topup["key_label"].is_null(),
            "无 key 交易 key_label 为 null: {topup}"
        );
    }

    #[tokio::test]
    async fn transactions_user_and_api_key_name() {
        // rant 2026-08-22T17:21:39 需求 2：/api/transactions 每行返回
        // user_name（transactions.user_id JOIN users）+ key_name（api_key_id JOIN api_keys）；
        // 历史行无 api_key_id → key_name null（前端兜底 key_label / 交易类型说明）
        let st = test_state("txusernm");
        let key = login(st.clone()).await;
        {
            let conn = st.db.lock().unwrap();
            // 登录流程已为 demo 建分发 key（get_or_create_api_key，name 空）——起个名字
            // （api_keys.name = 设置页用户起的名字）
            conn.execute(
                "UPDATE api_keys SET name = '我的测试key' WHERE user_id = 1",
                [],
            )
            .unwrap();
            let ak_id: i64 = conn
                .query_row("SELECT id FROM api_keys WHERE user_id = 1", [], |r| {
                    r.get(0)
                })
                .unwrap();
            // 上游 key 为 seed_test_users 种子行（id=1：deepseek / deepseek-paygo，note 空）
            // 新行：带 api_key_id → key_name = api_keys.name
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, api_key_id, model, tokens, cached_tokens, output_tokens, pts, type, status) \
                 VALUES (1, '2', 1, ?1, 'deepseek-v4-flash', 1000, 200.0, 100.0, 0.5, 'consume', '成功')",
                rusqlite::params![ak_id],
            )
            .unwrap();
            // 历史行：无 api_key_id → key_name null，key_label 兜底
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status) \
                 VALUES (1, '2', 1, 'deepseek-v4-flash', 500, 0.3, 'consume', '成功')",
                [],
            )
            .unwrap();
        }
        let (s, body) = get(
            st.clone(),
            "/api/transactions?type=all&page=1&page_size=10",
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let items = v["items"].as_array().unwrap();
        let named = items
            .iter()
            .find(|i| i["tokens"] == 1000.0)
            .expect("新行 found")
            .clone();
        assert_eq!(named["user_name"], "demo", "用户列 = users.name: {named}");
        assert_eq!(
            named["key_name"], "我的测试key",
            "Key 列 = api_keys.name: {named}"
        );
        assert_eq!(named["key_label"], "deepseek / deepseek-paygo");
        let legacy = items
            .iter()
            .find(|i| i["tokens"] == 500.0)
            .expect("历史行 found")
            .clone();
        assert_eq!(legacy["user_name"], "demo");
        assert!(
            legacy["key_name"].is_null(),
            "历史行 key_name 为 null: {legacy}"
        );
        assert_eq!(legacy["key_label"], "deepseek / deepseek-paygo");
    }

    #[tokio::test]
    async fn transactions_filter_and_pagination() {
        let st = test_state("tx");
        let key = login(st.clone()).await;
        {
            let conn = st.db.lock().unwrap();
            for i in 0..5 {
                let t = if i % 2 == 0 { "consume" } else { "earn" };
                conn.execute(
                    "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, cached_tokens, output_tokens, pts, type, status) VALUES (1, 'c', 1, 'm', 100, 10, 20, ?1, ?2, '成功')",
                    rusqlite::params![i as f64 + 1.0, t],
                )
                .unwrap();
            }
        }
        // 全部 + 分页（page=1 page_size=3 → 3 条；total=5）
        let (s, body) = get(st.clone(), "/api/transactions?page=1&page_size=3", &key).await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["total"], 5);
        assert_eq!(v["items"].as_array().unwrap().len(), 3);
        // 时间倒序：最新在前
        let items = v["items"].as_array().unwrap();
        assert!(items[0]["id"].as_i64().unwrap() > items[1]["id"].as_i64().unwrap());
        // 明细（rant 2026-08-21T14:53:20）：tokens=100 / cached=10 / output=20 → input=70
        assert!((items[0]["tokens"].as_f64().unwrap() - 100.0).abs() < 1e-9);
        assert!(
            (items[0]["input_tokens"].as_f64().unwrap() - 70.0).abs() < 1e-9,
            "输入=总量−缓存−输出"
        );
        assert!((items[0]["cached_tokens"].as_f64().unwrap() - 10.0).abs() < 1e-9);
        assert!((items[0]["output_tokens"].as_f64().unwrap() - 20.0).abs() < 1e-9);
        // 时区（rant 2026-08-19T20:45:32）：time 返回 UTC ISO 带 Z（前端按 UTC 解析，不再差 8 小时）
        let t0 = items[0]["time"].as_str().expect("time 为字符串");
        assert!(
            t0.ends_with('Z') && t0.contains('T'),
            "time 应为 UTC ISO 带 Z: {t0}"
        );
        // type=consume 过滤 → 3 条
        let (_, body) = get(st.clone(), "/api/transactions?type=consume", &key).await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["total"], 3);
        assert!(v["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["type"] == "consume"));
        // type=earn → 2 条
        let (_, body) = get(st.clone(), "/api/transactions?type=earn", &key).await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["total"], 2);
        // 非法 type → 400
        let (s, _) = get(st.clone(), "/api/transactions?type=hack", &key).await;
        assert_eq!(s, axum::http::StatusCode::BAD_REQUEST);
        // 无认证 → 401
        let resp = router()
            .with_state(st)
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/wallet")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::UNAUTHORIZED);
    }
}
