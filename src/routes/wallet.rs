//! 钱包 / 交易 / 仪表盘 API（对齐原型钱包页 + 交易页 + 仪表盘）
//!
//! P0-C（rant 2026-08-18T10:36:04）：
//! - GET /api/wallet → {balance, month_use, month_earn}
//! - GET /api/transactions?type=&page=&page_size= → 分页 + type 过滤（consume/earn/all）
//! - GET /api/dashboard → 本月按类型聚合 + 净变化 + 近 N 天序列（sparkline）

use axum::extract::{Query, State};
use axum::Json;
use rusqlite::params;
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
        t @ ("consume" | "earn" | "topup") => Some(t.to_string()),
        _ => {
            return Err((
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "type 必须为 consume / earn / topup / all" })),
            ))
        }
    };
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let total: i64 = match &type_filter {
        Some(t) => conn
            .query_row(
                "SELECT COUNT(*) FROM transactions WHERE user_id = ?1 AND type = ?2",
                params![auth.user_id, t],
                |r| r.get(0),
            )
            .unwrap_or(0),
        None => conn
            .query_row(
                "SELECT COUNT(*) FROM transactions WHERE user_id = ?1",
                [auth.user_id],
                |r| r.get(0),
            )
            .unwrap_or(0),
    };
    let offset = (page - 1) * page_size;
    let mut stmt = match &type_filter {
        Some(_) => conn
            .prepare(
                "SELECT id, counterpart, key_id, model, tokens, pts, type, status, time \
                 FROM transactions WHERE user_id = ?1 AND type = ?2 \
                 ORDER BY id DESC LIMIT ?3 OFFSET ?4",
            )
            .map_err(internal)?,
        None => conn
            .prepare(
                "SELECT id, counterpart, key_id, model, tokens, pts, type, status, time \
                 FROM transactions WHERE user_id = ?1 \
                 ORDER BY id DESC LIMIT ?2 OFFSET ?3",
            )
            .map_err(internal)?,
    };
    let rows: Vec<serde_json::Value> = match &type_filter {
        Some(t) => stmt
            .query_map(params![auth.user_id, t, page_size, offset], |r| {
                let time: String = r.get(8)?;
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "counterpart": r.get::<_, String>(1)?,
                    "key_id": r.get::<_, Option<i64>>(2)?,
                    "model": r.get::<_, String>(3)?,
                    "tokens": r.get::<_, f64>(4)?,
                    "pts": r.get::<_, f64>(5)?,
                    "type": r.get::<_, String>(6)?,
                    "status": r.get::<_, String>(7)?,
                    "time": crate::dao::utc_iso(&time),
                }))
            })
            .map_err(internal)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(internal)?,
        None => stmt
            .query_map(params![auth.user_id, page_size, offset], |r| {
                let time: String = r.get(8)?;
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "counterpart": r.get::<_, String>(1)?,
                    "key_id": r.get::<_, Option<i64>>(2)?,
                    "model": r.get::<_, String>(3)?,
                    "tokens": r.get::<_, f64>(4)?,
                    "pts": r.get::<_, f64>(5)?,
                    "type": r.get::<_, String>(6)?,
                    "status": r.get::<_, String>(7)?,
                    "time": crate::dao::utc_iso(&time),
                }))
            })
            .map_err(internal)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(internal)?,
    };
    Ok(Json(serde_json::json!({
        "items": rows,
        "total": total,
        "page": page,
        "page_size": page_size,
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
    // 近 7 天净额序列（earn 为正、consume 为负）
    let mut stmt = conn
        .prepare(
            "SELECT date(time), COALESCE(SUM(CASE WHEN type = 'earn' THEN pts ELSE -pts END), 0) \
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
        assert!((net - (1.8 - 2.0)).abs() < 1e-9, "净变化 = earn - consume");
        assert!(!v["series"].as_array().unwrap().is_empty(), "近 7 天序列");
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
                    "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status) VALUES (1, 'c', 1, 'm', 1, ?1, ?2, '成功')",
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
