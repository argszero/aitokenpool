//! 共享管理 API（对齐原型共享页 US-8/9）
//!
//! P0-C（rant 2026-08-18T10:36:04）：
//! - POST /api/sharings 上架（key 加密落库，DB 无明文）
//! - GET  /api/sharings 我的共享列表（key 脱敏 sk-****xxxx）
//! - PATCH /api/sharings/:id 暂停/恢复/删除（status: paused/on/off，软删）
//! - 可用时间段字段：available_days + start/end（先存后展示，生效判定留 P1）

use axum::extract::{Path, State};
use axum::Json;
use rusqlite::params;
use serde::Deserialize;

use crate::routes::{internal, ApiErr, AppState, AuthUser};

/// 上架请求
#[derive(Debug, Deserialize)]
pub struct CreateSharingReq {
    pub provider: String,
    #[serde(default)]
    pub plan: String,
    pub model: String,
    /// 上游 key（明文，服务端加密后落库）
    pub key: String,
    #[serde(default)]
    pub quota: f64,
    #[serde(default)]
    pub available: Option<Avail>,
    #[serde(default)]
    pub note: String,
}

/// 可用时间段（先存后展示；生效判定留 P1）
#[derive(Debug, Deserialize)]
pub struct Avail {
    /// 星期（1-7）
    #[serde(default)]
    pub days: Vec<u8>,
    /// HH:mm
    #[serde(default)]
    pub start: String,
    /// HH:mm
    #[serde(default)]
    pub end: String,
}

/// 状态变更请求
#[derive(Debug, Deserialize)]
pub struct PatchSharingReq {
    /// paused / on / off（off = 软删除）
    pub status: String,
}

/// key 脱敏：sk-****xxxx（保留前 2 位前缀 + 后 4 位，与原型一致）
fn mask_upstream_key(key: &str) -> String {
    if key.len() > 6 {
        let prefix = &key[..2.min(key.len())];
        let tail = &key[key.len() - 4..];
        format!("{prefix}-****{tail}")
    } else {
        "****".to_string()
    }
}

/// POST /api/sharings：上架共享 key（加密落库）
pub async fn create(
    State(st): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateSharingReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    if req.model.trim().is_empty() || req.key.trim().is_empty() {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "model 与 key 必填" })),
        ));
    }
    let encrypted = st
        .crypto
        .encrypt(req.key.trim().as_bytes())
        .map_err(internal)?;
    let (days, start, end) = match &req.available {
        Some(a) => (
            serde_json::to_string(&a.days).unwrap_or_default(),
            a.start.clone(),
            a.end.clone(),
        ),
        None => (String::new(), String::new(), String::new()),
    };
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    conn.execute(
        "INSERT INTO keys (provider, plan, model, status, owner_id, encrypted_key, quota, available_days, available_start, available_end, note) \
         VALUES (?1, ?2, ?3, 'on', ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            req.provider,
            req.plan,
            req.model,
            auth.user_id,
            encrypted,
            req.quota,
            days,
            start,
            end,
            req.note
        ],
    )
    .map_err(internal)?;
    let id = conn.last_insert_rowid();
    Ok(Json(serde_json::json!({
        "id": id,
        "provider": req.provider,
        "model": req.model,
        "key": mask_upstream_key(&req.key),
        "status": "on",
        "available_days": days,
        "available_start": start,
        "available_end": end,
        "note": req.note,
    })))
}

/// 单条共享（含收益汇总）；key 先解密再脱敏展示
fn sharing_row(
    conn: &rusqlite::Connection,
    crypto: &crate::crypto::Crypto,
    r: &rusqlite::Row,
) -> rusqlite::Result<serde_json::Value> {
    let id: i64 = r.get(0)?;
    let provider: String = r.get(1)?;
    let plan: String = r.get(2)?;
    let model: String = r.get(3)?;
    let status: String = r.get(4)?;
    let encrypted_key: String = r.get(5)?;
    let quota: f64 = r.get(6)?;
    let used: f64 = r.get(7)?;
    let days: String = r.get(8)?;
    let start: String = r.get(9)?;
    let end: String = r.get(10)?;
    let note: String = r.get(11)?;
    // 解密 → 脱敏（sk-****xxxx）；解密失败展示 ****
    let masked = crypto
        .decrypt(&encrypted_key)
        .ok()
        .and_then(|k| String::from_utf8(k).ok())
        .map(|k| mask_upstream_key(&k))
        .unwrap_or_else(|| "****".to_string());
    // 收益：该 key 的 earn 交易累计
    let earn: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(pts), 0) FROM transactions WHERE key_id = ?1 AND type = 'earn'",
            [id],
            |r| r.get(0),
        )
        .unwrap_or(0.0);
    Ok(serde_json::json!({
        "id": id,
        "provider": provider,
        "plan": plan,
        "model": model,
        "status": status,
        "key": masked,
        "quota": quota,
        "used": used,
        "earn": earn,
        "available_days": days,
        "available_start": start,
        "available_end": end,
        "note": note,
    }))
}

/// GET /api/sharings：我的共享列表（脱敏）
pub async fn list(
    State(st): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<serde_json::Value>>, ApiErr> {
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let crypto = st.crypto.clone();
    let mut stmt = conn
        .prepare(
            "SELECT id, provider, plan, model, status, encrypted_key, quota, used, \
                    available_days, available_start, available_end, note \
             FROM keys WHERE owner_id = ?1 ORDER BY id DESC",
        )
        .map_err(internal)?;
    let rows = stmt
        .query_map([auth.user_id], |r| sharing_row(&conn, &crypto, r))
        .map_err(internal)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(internal)?);
    }
    Ok(Json(out))
}

/// PATCH /api/sharings/:id：暂停/恢复/删除（status: paused/on/off）
pub async fn patch(
    State(st): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<PatchSharingReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    if !matches!(req.status.as_str(), "paused" | "on" | "off") {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "status 必须为 paused / on / off" })),
        ));
    }
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let n = conn
        .execute(
            "UPDATE keys SET status = ?1 WHERE id = ?2 AND owner_id = ?3",
            params![req.status, id, auth.user_id],
        )
        .map_err(internal)?;
    if n == 0 {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "共享不存在或不属于当前用户" })),
        ));
    }
    let crypto = st.crypto.clone();
    let row = conn
        .query_row(
            "SELECT id, provider, plan, model, status, encrypted_key, quota, used, \
                    available_days, available_start, available_end, note \
             FROM keys WHERE id = ?1",
            [id],
            |r| sharing_row(&conn, &crypto, r),
        )
        .map_err(internal)?;
    Ok(Json(row))
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
        let p = std::env::temp_dir().join(format!("atp_share_{}_{}.db", std::process::id(), tag));
        let _ = std::fs::remove_file(&p);
        let conn = crate::db::open(p.to_str().unwrap()).expect("open tmp db");
        crate::db::seed_test_users(&conn).expect("seed test users");
        let cfg = crate::config::Config::load("config/config.example.toml").unwrap();
        crate::db::seed_models(&conn, &cfg).expect("seed models");
        let crypto = crate::crypto::Crypto::new([13u8; 32]);
        AppState::new(conn, Arc::new(cfg), crypto)
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

    async fn send(
        st: AppState,
        method: &str,
        uri: &str,
        payload: Option<&str>,
        bearer: &str,
    ) -> (axum::http::StatusCode, String) {
        let mut b = Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", format!("Bearer {bearer}"));
        let resp = match payload {
            Some(body_str) => {
                b = b.header("content-type", "application/json");
                router()
                    .with_state(st)
                    .oneshot(b.body(Body::from(body_str.to_string())).unwrap())
                    .await
                    .unwrap()
            }
            None => router()
                .with_state(st)
                .oneshot(b.body(Body::empty()).unwrap())
                .await
                .unwrap(),
        };
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&bytes).to_string())
    }

    #[tokio::test]
    async fn create_encrypts_key_and_list_masks() {
        let st = test_state("create");
        let key = login(st.clone()).await;

        let (s, body) = send(
            st.clone(),
            "POST",
            "/api/sharings",
            Some(r#"{"provider":"deepseek","plan":"deepseek-paygo","model":"deepseek-v4-flash","key":"sk-realsecret1234","quota":1000,"available":{"days":[1,2,3,4,5],"start":"09:00","end":"18:00"},"note":"工作日共享"}"#),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let id = v["id"].as_i64().unwrap();

        // DB 中无明文（加密落库）——锁作用域块内，绝不让 MutexGuard 跨 await
        let (_stored, days, start, end, note) = {
            let conn = st.db.lock().unwrap();
            let stored: String = conn
                .query_row("SELECT encrypted_key FROM keys WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .unwrap();
            assert!(
                stored.starts_with(crate::crypto::PREFIX),
                "密文前缀: {stored}"
            );
            assert!(!stored.contains("sk-realsecret1234"), "DB 不得存明文");
            // 可用时间段字段正确
            let (days, start, end, note): (String, String, String, String) = conn
                .query_row(
                    "SELECT available_days, available_start, available_end, note FROM keys WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .unwrap();
            (stored, days, start, end, note)
        };
        assert!(days.contains("1") && days.contains("5"), "days={days}");
        assert_eq!(start, "09:00");
        assert_eq!(end, "18:00");
        assert_eq!(note, "工作日共享");

        // 列表脱敏：sk-****1234，不含真实 key
        let (s, body) = send(st.clone(), "GET", "/api/sharings", None, &key).await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let arr: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        assert!(arr.iter().any(|r| r["id"] == id));
        let row = arr.iter().find(|r| r["id"] == id).unwrap();
        assert_eq!(row["key"], "sk-****1234", "脱敏: {}", row["key"]);
        assert!(!body.contains("sk-realsecret1234"), "列表不得泄露明文");
        assert_eq!(row["status"], "on");
        assert_eq!(row["available_end"], "18:00");
    }

    #[tokio::test]
    async fn patch_pause_and_off() {
        let st = test_state("patch");
        let key = login(st.clone()).await;
        let (_, body) = send(
            st.clone(),
            "POST",
            "/api/sharings",
            Some(r#"{"provider":"deepseek","model":"deepseek-v4-flash","key":"sk-patchme9999"}"#),
            &key,
        )
        .await;
        let id: i64 = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
            .as_i64()
            .unwrap();

        // 暂停
        let (s, body) = send(
            st.clone(),
            "PATCH",
            &format!("/api/sharings/{id}"),
            Some(r#"{"status":"paused"}"#),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["status"], "paused");

        // 软删（off）
        let (s, _) = send(
            st.clone(),
            "PATCH",
            &format!("/api/sharings/{id}"),
            Some(r#"{"status":"off"}"#),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK);
        let status: String = {
            let conn = st.db.lock().unwrap();
            conn.query_row("SELECT status FROM keys WHERE id = ?1", [id], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(status, "off", "软删保留账本引用");

        // 非法 status → 400
        let (s, _) = send(
            st.clone(),
            "PATCH",
            &format!("/api/sharings/{id}"),
            Some(r#"{"status":"deleted"}"#),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::BAD_REQUEST);

        // 他人 id → 404
        let (s, _) = send(
            st,
            "PATCH",
            "/api/sharings/99999",
            Some(r#"{"status":"on"}"#),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn sharing_requires_bearer() {
        let st = test_state("nobearer");
        let resp = router()
            .with_state(st)
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/sharings")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::UNAUTHORIZED);
    }
}
