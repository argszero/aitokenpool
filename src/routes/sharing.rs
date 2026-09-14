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
///
/// ⚠️ 按**字符**（而非字节）截断：`key` 由用户提交、可以是任意 UTF-8，按字节切片会在多字节
/// 字符内部断开并 panic（`byte index N is not a char boundary`）。
fn mask_upstream_key(key: &str) -> String {
    let n = key.chars().count();
    if n > 6 {
        let prefix: String = key.chars().take(2).collect();
        let tail: String = key.chars().skip(n - 4).collect();
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
    // 只能上架平台**能计价**的 (provider, model)：计费按 `keys.provider` + model 查 `models` 行的价
    // （`dao::get_model_price`：`WHERE provider = ?1 AND model = ?2`），查不到即 0 计费 ——
    // 一次真实调用会**静默变成免费**（消费者不扣点、分享者无收益）。校验对象是 `models` 行而不是
    // `[[providers]]` 表：openai / anthropic / google / xai 只有 `[[models]]` 行、没有 provider 行。
    if conn
        .query_row(
            "SELECT 1 FROM models WHERE provider = ?1 AND model = ?2",
            params![req.provider, req.model],
            |_| Ok(()),
        )
        .is_err()
    {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "provider 与 model 不在模型目录中，无法计价" })),
        ));
    }
    // 只能上架平台**能路由**的 plan：路由按 plan id 在 config `[[plans]]` 中解析端点
    // （`gateway::resolve_outbound` / `resolve_endpoint`：`cfg.plans.iter().find(|p| p.id == plan_id)`），
    // 查不到即该 key **永远不可路由**（调用 503「暂无可用 key」），却仍以 `status='on'` 落库、被
    // `dao::list_models_with_availability`（`k.status = 'on'`，不认识 plan）计入 `available_keys`
    // ⇒ 市场/共享页会展示一个平台**交不出**的可用性。
    // `plan` 是 `#[serde(default)]`：省略字段即空串，同样不是任何 plan 的 id，一并拒绝
    // （前端上架表单本就必选 plan，`app.js` 提交 `plan: plan.id`，故合法客户端不受影响）。
    if !st.cfg.plans.iter().any(|p| p.id == req.plan) {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "plan 不在平台的套餐目录中，无法路由" })),
        ));
    }
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
///
/// ⚠️ 本函数**按下标读列**（`r.get(N)`），因此列顺序由调用方的 `SELECT` 决定：
/// 两个调用点（`list` / `patch`）的列表**必须逐字一致**。只改一处会让所有字段静默错位
/// （不报错、类型也往往恰好兼容），所以新增列一律**追加在末尾**，并同步改两处。
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
    // keys.used = 该 key 已消耗的**点数**，与 `quota` 同单位（进度条 `used / quota`、卡片「已用 N 点」
    // 都按点数渲染）。由 `billing::settle` 按 `p.pts` 累加；历史库由迁移按账本 `SUM(consume pts)` 重算。
    let used: f64 = r.get(7)?;
    let days: String = r.get(8)?;
    let start: String = r.get(9)?;
    let end: String = r.get(10)?;
    let note: String = r.get(11)?;
    let created_at: String = r.get(12)?;
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
        // 上架时间（UTC 带 Z，与 api_keys 的 created_at 同口径）；前端据此算「本月新增」与表格上架时间列
        "created_at": crate::dao::utc_iso(&created_at),
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
                    available_days, available_start, available_end, note, created_at \
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
                    available_days, available_start, available_end, note, created_at \
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
            Some(r#"{"provider":"deepseek","plan":"deepseek-paygo","model":"deepseek-flash","key":"sk-realsecret1234","quota":1000,"available":{"days":[1,2,3,4,5],"start":"09:00","end":"18:00"},"note":"工作日共享"}"#),
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
            Some(r#"{"provider":"deepseek","plan":"deepseek-paygo","model":"deepseek-flash","key":"sk-patchme9999"}"#),
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

    /// 上架时间字段（C2015，rant 驱动：共享页「本月新增」卡的数据源）
    ///
    /// 锁三件事：
    /// ① `created_at` 随**两个** SELECT 一起到位（`list` 与 `patch` 走的是两份列清单，
    ///    而 `sharing_row` 按下标读列 —— 只改一处会让既有字段**静默错位**，故两处都断言）；
    /// ② 既有字段未因新增列而错位（下标读取的回归护栏）；
    /// ③ 序列化口径是 **UTC**（带 `Z`）。前端据此算「本月」，用本地时间解析会在月末跨月：
    ///    本用例把上架时间按到 `2026-08-31 17:30:00Z` —— UTC 月是 8 月，
    ///    而本地（UTC+8）已是 9 月 1 日 01:30 ⇒ 两种口径**可区分**，不是自证。
    #[tokio::test]
    async fn created_at_is_exposed_and_utc_serialized() {
        let st = test_state("created_at");
        let key = login(st.clone()).await;
        let (_, body) = send(
            st.clone(),
            "POST",
            "/api/sharings",
            Some(r#"{"provider":"deepseek","plan":"deepseek-paygo","model":"deepseek-flash","key":"sk-created1234","used":0,"note":"utc"}"#),
            &key,
        )
        .await;
        let id: i64 = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
            .as_i64()
            .unwrap();

        // 回拨到「UTC 月末、本地已跨月」的时刻
        {
            let conn = st.db.lock().unwrap();
            conn.execute(
                "UPDATE keys SET created_at = '2026-08-31 17:30:00' WHERE id = ?1",
                [id],
            )
            .unwrap();
        }

        let (s, body) = send(st.clone(), "GET", "/api/sharings", None, &key).await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let arr: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        let row = arr.iter().find(|r| r["id"] == id).expect("列表应含该行");

        assert_eq!(
            row["created_at"], "2026-08-31T17:30:00Z",
            "上架时间应为 UTC ISO（带 Z，且时刻未被平移到本地时区）"
        );
        assert_eq!(&row["created_at"].as_str().unwrap()[..7], "2026-08");
        // 既有字段未错位
        assert_eq!(row["note"], "utc");
        assert_eq!(row["provider"], "deepseek");
        assert_eq!(row["status"], "on");
        assert_eq!(row["key"], "sk-****1234");
        assert_eq!(row["used"], 0.0);

        // PATCH 的单条响应走**另一份** SELECT —— 两处不一致时这里会错位
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
        assert_eq!(v["created_at"], "2026-08-31T17:30:00Z", "PATCH 响应同样带");
        assert_eq!(v["note"], "utc");
        assert_eq!(v["status"], "paused");
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

    // 回归：key 由用户提交，可以是任意 UTF-8；按字节切片曾在多字节字符内部 panic
    // （`byte index N is not a char boundary`），使 POST /api/sharings 直接崩掉。
    #[tokio::test]
    async fn mask_upstream_key_is_char_boundary_safe() {
        // 多字节字符：不再 panic，短于阈值时整体遮蔽
        assert_eq!(mask_upstream_key("中中中"), "****");
        assert_eq!(mask_upstream_key("aa中中"), "****");
        // 多字节字符：超过阈值时前后缀按「字符」截断（而非字节）
        assert_eq!(mask_upstream_key("aa中中中中中"), "aa-****中中中中");
        // ASCII 行为逐字节保持不变
        assert_eq!(mask_upstream_key("sk-realsecret1234"), "sk-****1234");
        assert_eq!(mask_upstream_key("short"), "****");

        // 端到端：带非 ASCII key 的 POST /api/sharings 必须正常返回（此前会 panic）
        let st = test_state("utf8mask");
        let token = login(st.clone()).await;
        let (s, body) = send(
            st,
            "POST",
            "/api/sharings",
            Some(r#"{"provider":"deepseek","plan":"deepseek-paygo","model":"deepseek-flash","key":"aa中中中中中","quota":1000,"available":{"days":[1],"start":"09:00","end":"18:00"}}"#),
            &token,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["key"], "aa-****中中中中", "非 ASCII key 应正常脱敏");
    }

    /// 上架只接受平台**能计价**的 (provider, model)（C2065）。
    ///
    /// 计费按 `keys.provider` + model 查 `models` 行的价（`dao::get_model_price`），查不到
    /// `settle_usage` 取 `None => (0.0, 0.0)` ⇒ 一次真实调用会**静默变成免费**：消费者不扣点、
    /// 分享者无收益、`usage_records.cost=0`，而调用本身 200 正常返回。故入口拒绝。
    ///
    /// 本用例锁三件事：① 目录外的 provider → 400；② 同 provider 下拼错的 model → 400；
    /// ③ **400 时不得落库**（校验必须在 `INSERT` 之前）；④ 阳性对照：目录中的组合 → 200 且落库
    /// （证明上面的 400 来自这一条规则，不是「上架坏了」）。
    #[tokio::test]
    async fn create_rejects_unpriceable_provider_model() {
        let st = test_state("unpriceable");
        let key = login(st.clone()).await;
        let count = |st: &crate::routes::AppState| -> i64 {
            let conn = st.db.lock().unwrap();
            conn.query_row("SELECT COUNT(*) FROM keys", [], |r| r.get(0))
                .unwrap()
        };
        let before = count(&st);

        // ① provider 不在模型目录（model 是真实存在的）
        let (s, body) = send(
            st.clone(),
            "POST",
            "/api/sharings",
            Some(r#"{"provider":"WRONG","plan":"deepseek-paygo","model":"deepseek-flash","key":"sk-wrong1234","quota":100}"#),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::BAD_REQUEST, "body: {body}");

        // ② 同一 provider、model 拼错
        let (s, body) = send(
            st.clone(),
            "POST",
            "/api/sharings",
            Some(r#"{"provider":"deepseek","plan":"deepseek-paygo","model":"deepseek-v4-flsh","key":"sk-typo12345","quota":100}"#),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::BAD_REQUEST, "body: {body}");

        // ③ 两次拒绝都不得写入（校验在 INSERT 之前）
        assert_eq!(count(&st), before, "被拒绝的上架不得落库");

        // ④ 阳性对照：模型目录中的组合 → 200 且落库
        let (s, body) = send(
            st.clone(),
            "POST",
            "/api/sharings",
            Some(r#"{"provider":"deepseek","plan":"deepseek-paygo","model":"deepseek-flash","key":"sk-control1234","quota":100}"#),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        assert_eq!(count(&st), before + 1, "合法上架应落库一行");
    }

    /// 上架只接受平台**能路由**的 plan（C2067）。
    ///
    /// 路由按 plan id 在 config `[[plans]]` 中解析端点（`gateway::resolve_outbound` /
    /// `resolve_endpoint`：`cfg.plans.iter().find(|p| p.id == plan_id)`）。`plan` 不在其中
    /// （含 `#[serde(default)]` 的空串）时该 key **永远不可路由**，但 `create` 仍会把它以
    /// `status='on'` 落库，并被 `dao::list_models_with_availability` 计入 `available_keys`
    /// ⇒ 市场与共享页展示「可用」，实际调用却 503「暂无可用 key」。故入口拒绝。
    ///
    /// 本用例锁四件事：① 未知 plan → 400；② **省略 plan**（落库即空串）→ 400；
    /// ③ **400 时不得落库**（校验须在 `INSERT` 之前）；④ 阳性对照：config 中的 plan → 200 且落库。
    /// 末尾再加一条**失配配对**断言（沿用 C2066 判据：计数面与可达面必须成对断言，见坑 #162）：
    /// 直接写入一条历史遗留的未知 plan 行，断言它**被计入 `available_keys`** 而它引用的 plan
    /// **不在 config 中**（⇒ 路由解析不到）—— 这正是本条守卫要挡住的情形。
    #[tokio::test]
    async fn create_rejects_unroutable_plan() {
        let st = test_state("unroutable");
        let key = login(st.clone()).await;
        let count = |st: &crate::routes::AppState| -> i64 {
            let conn = st.db.lock().unwrap();
            conn.query_row("SELECT COUNT(*) FROM keys", [], |r| r.get(0))
                .unwrap()
        };
        let before = count(&st);

        // ① plan 不在 config [[plans]] 中
        let (s, body) = send(
            st.clone(),
            "POST",
            "/api/sharings",
            Some(r#"{"provider":"deepseek","plan":"no-such-plan","model":"deepseek-flash","key":"sk-phantom1111","quota":100}"#),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::BAD_REQUEST, "body: {body}");

        // ② plan 省略 ⇒ `#[serde(default)]` 落库即空串，同样不是任何 plan 的 id
        let (s, body) = send(
            st.clone(),
            "POST",
            "/api/sharings",
            Some(r#"{"provider":"deepseek","model":"deepseek-flash","key":"sk-noplan2222","quota":100}"#),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::BAD_REQUEST, "body: {body}");

        // ③ 两次拒绝都不得写入（校验在 INSERT 之前）
        assert_eq!(count(&st), before, "被拒绝的上架不得落库");

        // ④ 阳性对照：config 中的 plan → 200 且落库（否则上面的 400 可能是「上架整体坏了」）
        let (s, body) = send(
            st.clone(),
            "POST",
            "/api/sharings",
            Some(r#"{"provider":"deepseek","plan":"deepseek-paygo","model":"deepseek-flash","key":"sk-control3333","quota":100}"#),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        assert_eq!(count(&st), before + 1, "合法上架应落库一行");

        // 失配配对：历史遗留的「未知 plan 却 status='on'」行 —— 计数算它，config 里没有它
        let avail = |st: &crate::routes::AppState| -> i64 {
            let conn = st.db.lock().unwrap();
            crate::dao::list_models_with_availability(&conn)
                .unwrap()
                .iter()
                .find(|m| m["model"] == "deepseek-flash")
                .expect("市场列表应含 deepseek-flash")["available_keys"]
                .as_i64()
                .unwrap()
        };
        let avail_before = avail(&st);
        {
            let conn = st.db.lock().unwrap();
            conn.execute(
                "INSERT INTO keys (provider, plan, model, status, owner_id, encrypted_key, quota, used) \
                 VALUES ('deepseek', 'no-such-plan', 'deepseek-flash', 'on', 1, 'sk-legacy-enc', 100, 0)",
                [],
            )
            .unwrap();
        }
        assert!(
            st.cfg.plans.iter().all(|p| p.id != "no-such-plan"),
            "前提：该 plan 不在 config 中（⇒ 路由解析不到）"
        );
        assert_eq!(
            avail(&st),
            avail_before + 1,
            "未知 plan 的遗留行仍被计入 available_keys —— 正是本条守卫要挡住的情形"
        );
    }
}
