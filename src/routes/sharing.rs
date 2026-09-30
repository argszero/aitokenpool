//! 共享管理 API（对齐原型共享页 US-8/9）
//!
//! P0-C（rant 2026-08-18T10:36:04）：
//! - POST /api/sharings 上架（key 加密落库，DB 无明文）
//! - GET  /api/sharings 我的共享列表（key 脱敏 sk-****xxxx）
//! - PATCH /api/sharings/:id 编辑已上架的 key：暂停/恢复/删除（status: paused/on/off，软删）
//!   ＋ 上架时的**全部设置**（provider / plan / model / key / quota / available / note，
//!   部分更新：**省略的字段不修改**）。行 id、上架时间与收益归属保持不变。
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

/// 编辑请求（部分更新）：与 [`CreateSharingReq`] **同一字段集合**，外加 `status`。
///
/// 语义：**省略的字段不修改**（`None` = 沿用当前值），给出的字段整体替换。
/// - `status`：paused / on / off（off = 软删），取值与语义与旧的「只改状态」完全一致；
/// - `key`：⚠️ 前端手里只有**掩码串**（`mask_upstream_key` 的成品）。省略 / 留空 = **保留原密文**，
///   填写新值 = 加密替换 —— 掩码串一旦被写回 `encrypted_key`，原 key 就永久失效且界面无异常提示
///   （见 `patch` 里的注释与 `patch_keeps_the_stored_key_when_no_new_key_is_given` 测试）。
/// - `available`：`Option<Option<Avail>>`，用 [`double_option`] 把「省略」与「显式 null」分开 ——
///   省略 = 不改动时段；`null` = 全天不限（三个字段一并置空）；对象 = 三个字段一并替换。
#[derive(Debug, Deserialize)]
pub struct PatchSharingReq {
    /// paused / on / off（off = 软删除）
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub plan: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// 新上游 key（明文）。省略 / 空串 = 保留原密文
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub quota: Option<f64>,
    #[serde(default, deserialize_with = "double_option")]
    pub available: Option<Option<Avail>>,
    #[serde(default)]
    pub note: Option<String>,
}

/// `Option<Option<T>>` 的 serde 垫片：把「字段**缺席**」与「字段显式为 `null`」区分开。
///
/// - 字段缺席 → 外层 `#[serde(default)]` 给 `None`（＝不修改）
/// - 字段显式 `null` → 内层解出 `None`、本函数包成 `Some(None)`（＝置空）
/// - 字段给出对象 → `Some(Some(a))`（＝替换）
///
/// 没有这层的话，`Option<Avail>` 会把「省略」和「null」都变成 `None`，于是编辑表单里
/// `available: null`（用户清掉时段 = 全天不限）只能被当成「没提交这个字段」而保留旧时段。
fn double_option<'de, D, T>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    serde::Deserialize::deserialize(de).map(Some)
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

/// 上架 / 编辑**共用**的落库前校验：`(provider, model)` 必须可计价、`plan` 必须可路由。
///
/// 为什么要提取成一个函数（而不是各写一遍）：C2052 的教训 —— 这个端点曾有一份手写的类型白名单
/// 副本，于是同一个筛选值在列表端点 200、在趋势端点 400。上架与编辑是**同一条规则的两个入口**，
/// 校验只允许有一份实现：`create` 传请求体的值，`patch` 传**合并后**的值。
///
/// 两道守卫各自防什么：
/// ① 可计价：计费按 `keys.provider` + model 查 `models` 行的价（`dao::get_model_price`：
///    `WHERE provider = ?1 AND model = ?2`），查不到的 (provider, model) 建出来的 key **调不通**
///    —— 路由入口取不到价即拒（503「无法计价」，R156）。校验对象是 `models` 行而不是
///    `[[providers]]` 表：openai / anthropic / google / xai 只有 `[[models]]` 行、没有 provider 行。
/// ② 可路由：路由按 plan id 在 config `[[plans]]` 中解析端点（`gateway::resolve_outbound` /
///    `resolve_endpoint`：`cfg.plans.iter().find(|p| p.id == plan_id)`），查不到即该 key
///    **永远不可路由**（调用 503「暂无可用 key」），却仍以 `status='on'` 落库、被
///    `dao::list_models_with_availability`（`k.status = 'on'`，不认识 plan）计入 `available_keys`
///    ⇒ 市场/共享页会展示一个平台**交不出**的可用性。
///    `create` 的 `plan` 是 `#[serde(default)]`：省略字段即空串，同样不是任何 plan 的 id，一并拒绝
///    （前端上架表单本就必选 plan，`app.js` 提交 `plan: plan.id`，故合法客户端不受影响）。
///    编辑路径传的是**合并后**的 plan：省略 plan 字段时沿用当前值（本就是合法 id），显式改成
///    空串或未知 id 才会被这里拒掉。
fn validate_listing(
    cfg: &crate::config::Config,
    conn: &rusqlite::Connection,
    provider: &str,
    model: &str,
    plan: &str,
) -> Result<(), ApiErr> {
    if conn
        .query_row(
            "SELECT 1 FROM models WHERE provider = ?1 AND model = ?2",
            params![provider, model],
            |_| Ok(()),
        )
        .is_err()
    {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "provider 与 model 不在模型目录中，无法计价" })),
        ));
    }
    if !cfg.plans.iter().any(|p| p.id == plan) {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "plan 不在平台的套餐目录中，无法路由" })),
        ));
    }
    Ok(())
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
    // 上架与编辑共用同一份校验（理由与两道守卫的出处见 `validate_listing`）
    validate_listing(&st.cfg, &conn, &req.provider, &req.model, &req.plan)?;
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

/// 共享列表 / 单条共用的**同一份** `SELECT`（两个调用点只差 `WHERE`）。
///
/// 收益（`earn`）用**一次批量聚合**左连进来，而不是每行再跑一次
/// `SELECT SUM(pts) … WHERE key_id = ?1 AND type = 'earn'`（rant 2026-09-14T21:15:02 第 2 条）：
/// 共享页 N 行 ⇒ N 次子查询（每次 prepare + 索引查找），批量版只读一遍覆盖索引
/// `idx_transactions_key_id_type_pts`。200,000 行 / 8 个 key 本机实测：8 次子查询 1.54 ms，
/// 批量 0.02 ms；NAS 上每次子查询还要多摸若干页，差距随 N 放大。
///
/// 列顺序决定 [`sharing_row`] 的下标读取，且两个调用点必须**逐字相同** —— 只改一处会让所有字段
/// 静默错位（不报错、类型也往往恰好兼容）。提取成常量就是为了让这件事只剩下一个地方可改
/// （镜像 `admin_models.rs::ROW_SELECT` 的写法）。新增列一律**追加在末尾**。
const ROW_SELECT: &str = "SELECT k.id, k.provider, k.plan, k.model, k.status, k.encrypted_key, \
                k.quota, k.used, k.available_days, k.available_start, k.available_end, k.note, \
                k.created_at, COALESCE(e.earn, 0) \
         FROM keys k \
         LEFT JOIN (SELECT key_id, SUM(pts) AS earn FROM transactions WHERE type = 'earn' \
                    GROUP BY key_id) e ON e.key_id = k.id ";

/// 单条共享（含收益汇总）；key 先解密再脱敏展示。
///
/// ⚠️ 本函数**只读列、不发 SQL**（收益来自 `ROW_SELECT` 的批量聚合；`perf_gate` 有断言守着）——
/// 一旦在这里补一次 `query_row`，列表端点就退回 N+1。
fn sharing_row(
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
    // 收益：该 key 的 earn 交易累计 —— 由 `ROW_SELECT` 的批量聚合给出（不是每行一次子查询）
    let earn: f64 = r.get(13)?;
    // 解密 → 脱敏（sk-****xxxx）；解密失败展示 ****
    let masked = crypto
        .decrypt(&encrypted_key)
        .ok()
        .and_then(|k| String::from_utf8(k).ok())
        .map(|k| mask_upstream_key(&k))
        .unwrap_or_else(|| "****".to_string());
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
        .prepare(&format!(
            "{ROW_SELECT} WHERE k.owner_id = ?1 ORDER BY k.id DESC"
        ))
        .map_err(internal)?;
    let rows = stmt
        .query_map([auth.user_id], |r| sharing_row(&crypto, r))
        .map_err(internal)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(internal)?);
    }
    Ok(Json(out))
}

/// PATCH /api/sharings/:id：编辑已上架的 key（部分更新）。
///
/// 旧行为（只改 status）是它的一个子集，取值与语义不变；现在同一入口也能改上架时的**全部设置**。
/// 实现走「先取当前值 → 合并 → 校验 → 写回」（镜像 `admin_models::patch`）：避免逐列拼接 SQL，
/// 也让「哪些字段没提交」这件事只剩一处判断。
///
/// ⚠️ **上游 key 的三态**（本函数最容易做错的一处）：前端手里只有**掩码串**（`sk-****xxxx`，
/// 由 `mask_upstream_key` 产出）。所以 `key` 的正确语义是 —— 省略 / 留空 = **保留原密文**，
/// 只有给出非空白新值才加密替换。掩码串一旦被当成「新值」写回 `encrypted_key`，原 key 就永久
/// 失效（之后的调用全部 401），而界面不会有任何异常提示 —— 这条由
/// `patch_keeps_the_stored_key_when_no_new_key_is_given` 的用例钉住（解密后必须仍等于原明文、
/// 且不等于掩码串）。
pub async fn patch(
    State(st): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<PatchSharingReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    if let Some(status) = req.status.as_deref() {
        if !matches!(status, "paused" | "on" | "off") {
            return Err((
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "status 必须为 paused / on / off" })),
            ));
        }
    }
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    // 归属校验的两条出口（`SELECT` 查不到 / `UPDATE` 影响 0 行）给**同一个** 404：
    // 「不是我的行」与「不存在的行」不区分，也就不泄露某个 id 是否存在。
    // 错误值只在一个地方写（错误文案词表门禁按**站点**计数）。
    let not_found = || {
        (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "共享不存在或不属于当前用户" })),
        )
    };
    // 取当前行（合并的底）。归属校验在这里就生效：不是我的行 → 当作不存在（404）。
    let cur = match conn.query_row(
        "SELECT provider, plan, model, status, quota, encrypted_key, available_days, available_start, available_end, note \
         FROM keys WHERE id = ?1 AND owner_id = ?2",
        params![id, auth.user_id],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, f64>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, String>(9)?,
            ))
        },
    ) {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Err(not_found()),
        Err(e) => return Err(internal(e)),
    };
    let provider = req.provider.unwrap_or(cur.0);
    let plan = req.plan.unwrap_or(cur.1);
    let model = req.model.unwrap_or(cur.2);
    let status = req.status.unwrap_or(cur.3);
    let quota = req.quota.unwrap_or(cur.4);
    // 只有给出非空白新值才重新加密；其余一律沿用原密文（掩码串绝不写回，见本函数文档）
    let encrypted = match req.key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
        Some(k) => st.crypto.encrypt(k.as_bytes()).map_err(internal)?,
        None => cur.5,
    };
    // 时段三态：省略 = 原样；null = 三字段一并置空（全天不限）；对象 = 三字段一并替换。
    // 三个字段必须**一起**动 —— 只改一半会留下自相矛盾的时段（C2146 的同形问题）。
    let (days, start, end) = match req.available {
        Some(Some(a)) => (
            serde_json::to_string(&a.days).unwrap_or_default(),
            a.start,
            a.end,
        ),
        Some(None) => (String::new(), String::new(), String::new()),
        None => (cur.6, cur.7, cur.8),
    };
    let note = req.note.unwrap_or(cur.9);
    // 与上架**同一条规则**（`validate_listing`）：合并后的 (provider, model, plan) 仍须可计价 / 可路由
    validate_listing(&st.cfg, &conn, &provider, &model, &plan)?;
    let n = conn
        .execute(
            "UPDATE keys SET provider = ?1, plan = ?2, model = ?3, status = ?4, quota = ?5, \
             encrypted_key = ?6, available_days = ?7, available_start = ?8, available_end = ?9, note = ?10 \
             WHERE id = ?11 AND owner_id = ?12",
            params![
                provider,
                plan,
                model,
                status,
                quota,
                encrypted,
                days,
                start,
                end,
                note,
                id,
                auth.user_id
            ],
        )
        .map_err(internal)?;
    if n == 0 {
        return Err(not_found());
    }
    let crypto = st.crypto.clone();
    let row = conn
        .query_row(&format!("{ROW_SELECT} WHERE k.id = ?1"), [id], |r| {
            sharing_row(&crypto, r)
        })
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

    /// rant 2026-09-14T21:15:02 第 2 条：共享页的收益（`earn`）**一次批量聚合**取回，
    /// 而不是每行一次 `SELECT SUM(pts) … WHERE key_id = ?1 AND type = 'earn'`。
    ///
    /// 两件事必须同时成立，所以这条测试同时断言它们：
    /// ① 值正确（`earn` 只算 `type='earn'`，`consume` 不计入）且**跨用户不串味** ——
    ///    批量聚合读的是**全库** key 的 earn，再按 `key_id` 左连，一旦归属判断写错，
    ///    别人的收益会贴到我的行上（信息泄露 + 金额错）；
    /// ② `list` 与 `patch` **两个调用点**给出同一个值（共用的 `ROW_SELECT` 是唯一真源）。
    #[tokio::test]
    async fn sharings_earn_is_one_batched_aggregate() {
        let st = test_state("earn");
        let key = login(st.clone()).await;

        // 两个 key（同一用户）：key_a 有 earn、key_b 只有 consume
        let mut ids = Vec::new();
        for (i, name) in ["a", "b"].iter().enumerate() {
            let (s, body) = send(
                st.clone(),
                "POST",
                "/api/sharings",
                Some(&format!(
                    r#"{{"provider":"deepseek","plan":"deepseek-paygo","model":"deepseek-flash","key":"sk-{name}00000000","quota":{}}}"#,
                    1000 + i
                )),
                &key,
            )
            .await;
            assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
            ids.push(
                serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
                    .as_i64()
                    .unwrap(),
            );
        }
        let (key_a, key_b) = (ids[0], ids[1]);

        // 另一个用户的 key + 一笔巨大的 earn：不许出现在我的列表里，也不许贴到我的行上
        let (uid, other_key, other_uid) = {
            let conn = st.db.lock().unwrap();
            let uid: i64 = conn
                .query_row(
                    "SELECT id FROM users WHERE email = 'demo@aitokenpool.local'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            conn.execute(
                "INSERT INTO users (email, password_hash, name, role) VALUES ('other@x.local', 'x', 'other', 'user')",
                [],
            )
            .unwrap();
            let other_uid = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO keys (owner_id, provider, plan, model, status, encrypted_key, quota, used, \
                 available_days, available_start, available_end, note) \
                 VALUES (?1, 'deepseek', 'deepseek-paygo', 'deepseek-flash', 'on', 'v1:x', 10, 0, '', '', '', '')",
                [other_uid],
            )
            .unwrap();
            let other_key = conn.last_insert_rowid();
            // key_a：earn 3.5 + earn 1.5 = 5.0（消费 9.0 不计入收益）
            conn.execute_batch(&format!(
                "INSERT INTO transactions (user_id, key_id, type, pts) VALUES
                   ({uid}, {key_a}, 'earn', 3.5),
                   ({uid}, {key_a}, 'earn', 1.5),
                   ({uid}, {key_a}, 'consume', 9.0),
                   ({uid}, {key_b}, 'consume', 2.0);
                 INSERT INTO transactions (user_id, key_id, type, pts) VALUES
                   ({other_uid}, {other_key}, 'earn', 1000.0);"
            ))
            .unwrap();
            (uid, other_key, other_uid)
        };

        let (s, body) = send(st.clone(), "GET", "/api/sharings", None, &key).await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let arr: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        // 登录会自动给用户建一把分发 key（`dao.rs`），故列表 3 行：那一把 earn 0
        assert_eq!(arr.len(), 3, "只应看到自己的三把 key：{body}");
        let row_a = arr.iter().find(|r| r["id"] == key_a).unwrap();
        let row_b = arr.iter().find(|r| r["id"] == key_b).unwrap();
        assert_eq!(row_a["earn"], 5.0, "earn 只累计 type='earn'：{body}");
        assert_eq!(row_b["earn"], 0.0, "只有 consume 的行收益为 0：{body}");
        assert!(
            arr.iter().all(|r| r["id"] != other_key),
            "别的用户的 key 不得出现在我的列表里：{body}"
        );
        let earn_sum: f64 = arr.iter().map(|r| r["earn"].as_f64().unwrap()).sum();
        assert_eq!(
            earn_sum, 5.0,
            "别的 key 的 1000.0 若被串进来，总和会是 1005.0：{body}"
        );
        // quota 未被收益污染（同一次查询里两列都来自 keys 行）
        assert_eq!(row_a["quota"], 1000.0);
        assert_eq!(row_b["quota"], 1001.0);

        // ② PATCH 单条走同一份 ROW_SELECT ⇒ 同一个值
        let (s, body) = send(
            st.clone(),
            "PATCH",
            &format!("/api/sharings/{key_a}"),
            Some(r#"{"status":"paused"}"#),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let one: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            one["earn"], 5.0,
            "patch 与 list 必须给出同一个 earn（同一份 SELECT）：{body}"
        );
        assert_eq!(one["status"], "paused");

        // 阳性对照：夹具本身有效 —— 把别的 key 的 1000.0 也算进来会得到 1005.0
        assert_ne!(uid, other_uid);
    }

    /// 同一件事的**计划层**证据（`perf_gate` 只保证源码形状，计划才证明优化器真的这么跑）：
    /// 列表查询里的 earn 必须是**一个**未被关联的子查询（`MATERIALIZE`），
    /// 且它扫的是覆盖索引 `idx_transactions_key_id_type_pts`（v15 迁移建的），不是回表。
    /// 关联子查询（`CORRELATED SCALAR SUBQUERY`）就是 N+1 的形状 —— 那正是本改动要去掉的。
    #[test]
    fn the_list_query_aggregates_earn_once_over_a_covering_index() {
        let st = test_state("plan");
        let conn = st.db.lock().unwrap();
        let uid: i64 = conn
            .query_row(
                "SELECT id FROM users WHERE email = 'demo@aitokenpool.local'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        // 3000 行 / 3 个 key：行数太少优化器会「合理地」选全表扫
        conn.execute_batch(&format!(
            "WITH RECURSIVE c(n) AS (SELECT 0 UNION ALL SELECT n+1 FROM c WHERE n < 2999)
             INSERT INTO transactions (user_id, key_id, type, pts)
               SELECT {uid}, 1 + (n % 3), CASE WHEN n % 3 = 0 THEN 'earn' ELSE 'consume' END, 0.5 FROM c;"
        ))
        .unwrap();
        let sql = format!("{ROW_SELECT} WHERE k.owner_id = ?1 ORDER BY k.id DESC");
        let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let plan: Vec<String> = stmt
            .query_map([uid], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let plan = plan.join(" | ");
        assert!(
            !plan.contains("CORRELATED"),
            "earn 不得是关联子查询（每行一次 ⇒ N+1）：{plan}"
        );
        assert!(
            plan.contains("COVERING INDEX idx_transactions_key_id_type_pts"),
            "earn 聚合法应扫覆盖索引（v15 迁移建的）：{plan}"
        );
        assert!(
            !plan.contains("SCAN transactions\n") && !plan.contains("SCAN transactions |"),
            "不得对 transactions 做非覆盖全表扫：{plan}"
        );
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
    /// 计费按 `keys.provider` + model 查 `models` 行的价（`dao::get_model_price`），查不到即
    /// **无法计价**：路由入口在发往上游之前就把这样的调用拒掉（503，R156）⇒ 建出来的 key 一次也
    /// 调不通。故入口拒绝。
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

    /// 上架时的**全部设置**都能在页内改（rant 2026-09-30T13:12:07 第 2/3 条）：
    /// `PATCH /api/sharings/:id` 接受与 `POST /api/sharings` 同一字段集合。
    ///
    /// 锁三件事：① 七组设置一次改完都生效；② **保持原行 id**（不是「删旧行再建新行」）；
    /// ③ 三件不可改的事实不变 —— 行仍是**一行**、上架时间不动、账本归属（`key_id`）不动。
    #[tokio::test]
    async fn patch_edits_every_listing_field_in_place() {
        let st = test_state("editall");
        let key = login(st.clone()).await;
        let (s, body) = send(
            st.clone(),
            "POST",
            "/api/sharings",
            Some(r#"{"provider":"deepseek","plan":"deepseek-paygo","model":"deepseek-flash","key":"sk-editable1234","quota":1000,"available":{"days":[1,2],"start":"09:00","end":"18:00"},"note":"原备注"}"#),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
            .as_i64()
            .unwrap();
        let created_before: String = {
            let conn = st.db.lock().unwrap();
            conn.query_row("SELECT created_at FROM keys WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap()
        };

        // 厂商 / Plan / 模型 / 额度 / 时段 / 备注 一次改（key 另有用例）——
        // 换成 config 里另一组可计价 (provider, model) 与可路由 plan
        let (s, body) = send(
            st.clone(),
            "PATCH",
            &format!("/api/sharings/{id}"),
            Some(r#"{"provider":"zhipu","plan":"zhipu-coding","model":"glm-5.3","quota":42,"available":{"days":[6,7],"start":"10:30","end":"11:45"},"note":"改过的备注"}"#),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["id"], id, "编辑必须保持原行 id（不得删旧建新）");
        assert_eq!(v["provider"], "zhipu");
        assert_eq!(v["plan"], "zhipu-coding");
        assert_eq!(v["model"], "glm-5.3");
        assert_eq!(v["quota"], 42.0);
        assert_eq!(v["note"], "改过的备注");
        assert_eq!(v["available_start"], "10:30");
        assert_eq!(v["available_end"], "11:45");
        assert_eq!(
            v["available_days"], "[6,7]",
            "days 序列化成 JSON 串，与 create 逐字一致"
        );

        // 列表里仍是**同一行**；上架时间未被改动
        let (_, body) = send(st.clone(), "GET", "/api/sharings", None, &key).await;
        let arr: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        let rows: Vec<&serde_json::Value> = arr.iter().filter(|r| r["id"] == id).collect();
        assert_eq!(rows.len(), 1, "编辑后仍只有一行：{body}");
        assert_eq!(rows[0]["model"], "glm-5.3");
        let created_after: String = {
            let conn = st.db.lock().unwrap();
            conn.query_row("SELECT created_at FROM keys WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap()
        };
        assert_eq!(created_after, created_before, "上架时间不得被编辑改动");
    }

    /// rant 2026-09-30T13:12:07 第 5 条（**硬约束**）：编辑表单手里只有**掩码串**，
    /// 因此「省略 / 留空 = 保留原密文，填新值 = 加密替换」。
    ///
    /// 验收判据（rant 原文）：未填新值时保存后 `encrypted_key` 解密结果**仍等于原 key**，
    /// 且**不等于掩码串**。反过来 —— 掩码串被写回 —— 原 key 就永久失效（之后的调用全部 401），
    /// 而界面不会有任何异常提示。所以这条不能只靠前端自觉，后端必须钉住。
    #[tokio::test]
    async fn patch_keeps_the_stored_key_when_no_new_key_is_given() {
        let st = test_state("editkey");
        let key = login(st.clone()).await;
        let (_, body) = send(
            st.clone(),
            "POST",
            "/api/sharings",
            Some(r#"{"provider":"deepseek","plan":"deepseek-paygo","model":"deepseek-flash","key":"sk-original9999","quota":10,"note":"n"}"#),
            &key,
        )
        .await;
        let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
            .as_i64()
            .unwrap();
        // 掩码串 = 前端手里唯一的那个值（`sharing_row` → `mask_upstream_key`）
        let mask = {
            let (_, body) = send(st.clone(), "GET", "/api/sharings", None, &key).await;
            let arr: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
            arr.iter().find(|r| r["id"] == id).unwrap()["key"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(mask, "sk-****9999");

        let decrypt = || -> String {
            let conn = st.db.lock().unwrap();
            let stored: String = conn
                .query_row("SELECT encrypted_key FROM keys WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .unwrap();
            String::from_utf8(st.crypto.decrypt(&stored).expect("密文应可解密")).unwrap()
        };

        // ① 省略 key / 空串 / 纯空白 —— 三种「没给新值」都保留原密文
        for (label, payload) in [
            ("省略 key", r#"{"note":"只改备注"}"#),
            ("空串 key", r#"{"key":""}"#),
            ("空白 key", r#"{"key":"   "}"#),
        ] {
            let (s, body) = send(
                st.clone(),
                "PATCH",
                &format!("/api/sharings/{id}"),
                Some(payload),
                &key,
            )
            .await;
            assert_eq!(s, axum::http::StatusCode::OK, "{label}: {body}");
            let plain = decrypt();
            assert_eq!(plain, "sk-original9999", "{label}: 原 key 必须原样保留");
            assert_ne!(plain, mask, "{label}: 掩码串绝不能被写回 encrypted_key");
        }

        // ② 阳性对照：给出新值 → 加密替换（证明 ① 不是「压根没更新」而通过）
        let (s, _) = send(
            st.clone(),
            "PATCH",
            &format!("/api/sharings/{id}"),
            Some(r#"{"key":"sk-brandnew7777"}"#),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK);
        assert_eq!(decrypt(), "sk-brandnew7777", "填新值应加密替换");
    }

    /// 部分更新语义（rant 第 3/7 条）：**省略的字段不修改**；`available: null` 明确表示
    /// 「全天不限」⇒ 三个时段字段**一并**置空（不得半保留）。
    ///
    /// 后半截是 `double_option` 垫片的牙齿：没有它，`Option<Avail>` 无法把「省略」与「null」
    /// 分开 —— `null` 会被当成「没提交这个字段」，于是用户清掉时段后 `days`/`start`/`end` 仍是旧值。
    #[tokio::test]
    async fn patch_only_touches_the_fields_it_is_given() {
        let st = test_state("editpartial");
        let key = login(st.clone()).await;
        let (_, body) = send(
            st.clone(),
            "POST",
            "/api/sharings",
            Some(r#"{"provider":"deepseek","plan":"deepseek-paygo","model":"deepseek-flash","key":"sk-partial1234","quota":500,"available":{"days":[3],"start":"08:00","end":"20:00"},"note":"原备注"}"#),
            &key,
        )
        .await;
        let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
            .as_i64()
            .unwrap();

        // ① 只改备注 + 额度：其余字段（含时段三字段）原样
        let (s, body) = send(
            st.clone(),
            "PATCH",
            &format!("/api/sharings/{id}"),
            Some(r#"{"note":"只改备注","quota":7,"status":"paused"}"#),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["note"], "只改备注");
        assert_eq!(v["quota"], 7.0);
        assert_eq!(v["status"], "paused", "status 语义不变（仍是同一入口）");
        assert_eq!(v["provider"], "deepseek", "省略的 provider 不修改");
        assert_eq!(v["model"], "deepseek-flash", "省略的 model 不修改");
        assert_eq!(v["plan"], "deepseek-paygo", "省略的 plan 不修改");
        assert_eq!(v["available_days"], "[3]", "省略 available 不修改时段");
        assert_eq!(v["available_start"], "08:00");
        assert_eq!(v["available_end"], "20:00");

        // ② available: null = 全天不限 ⇒ 三字段**一起**置空
        let (s, body) = send(
            st.clone(),
            "PATCH",
            &format!("/api/sharings/{id}"),
            Some(r#"{"available":null}"#),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["available_days"], "", "null 应清空 days");
        assert_eq!(v["available_start"], "", "null 应清空 start（不得半保留）");
        assert_eq!(v["available_end"], "", "null 应清空 end（不得半保留）");
        assert_eq!(v["note"], "只改备注", "② 只动时段，其余不变");
        assert_eq!(v["quota"], 7.0);
    }

    /// rant 第 4 条：编辑路径**复用**上架路径的校验，不是第二份实现（C2052 的教训）。
    ///
    /// 判据不只是「两边都 400」，而是两句报错**逐字相同** —— 各自实现两份规则时，措辞与
    /// 判定条件都会分叉。另加「400 时不得落库」：校验必须在 `UPDATE` 之前。
    #[tokio::test]
    async fn patch_reuses_the_create_validation() {
        let st = test_state("editvalidate");
        let key = login(st.clone()).await;
        let (_, body) = send(
            st.clone(),
            "POST",
            "/api/sharings",
            Some(r#"{"provider":"deepseek","plan":"deepseek-paygo","model":"deepseek-flash","key":"sk-validate1234","quota":100}"#),
            &key,
        )
        .await;
        let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
            .as_i64()
            .unwrap();
        let snapshot = |field: &str| -> String {
            let conn = st.db.lock().unwrap();
            conn.query_row(
                &format!("SELECT {field} FROM keys WHERE id = ?1"),
                [id],
                |r| r.get(0),
            )
            .unwrap()
        };
        let (provider_before, model_before, plan_before) =
            (snapshot("provider"), snapshot("model"), snapshot("plan"));

        // ① 不可计价的 (provider, model)：create 与 patch 给**同一句话**
        let bad_model = r#"{"provider":"deepseek","plan":"deepseek-paygo","model":"no-such-model","key":"sk-x1234567"}"#;
        let (s_create, body_create) =
            send(st.clone(), "POST", "/api/sharings", Some(bad_model), &key).await;
        assert_eq!(
            s_create,
            axum::http::StatusCode::BAD_REQUEST,
            "{body_create}"
        );
        let (s, body_patch) = send(
            st.clone(),
            "PATCH",
            &format!("/api/sharings/{id}"),
            Some(r#"{"model":"no-such-model"}"#),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::BAD_REQUEST, "{body_patch}");
        assert_eq!(
            body_patch, body_create,
            "不可计价的报错必须来自同一份实现（逐字相同）"
        );
        assert_eq!(snapshot("model"), model_before, "400 时不得落库");

        // ② 不可路由的 plan：同样逐字相同
        let bad_plan = r#"{"provider":"deepseek","plan":"no-such-plan","model":"deepseek-flash","key":"sk-x1234567"}"#;
        let (s_create, body_create) =
            send(st.clone(), "POST", "/api/sharings", Some(bad_plan), &key).await;
        assert_eq!(
            s_create,
            axum::http::StatusCode::BAD_REQUEST,
            "{body_create}"
        );
        let (s, body_patch) = send(
            st.clone(),
            "PATCH",
            &format!("/api/sharings/{id}"),
            Some(r#"{"plan":"no-such-plan"}"#),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::BAD_REQUEST, "{body_patch}");
        assert_eq!(
            body_patch, body_create,
            "不可路由的报错必须来自同一份实现（逐字相同）"
        );
        assert_eq!(snapshot("plan"), plan_before, "400 时不得落库");
        assert_eq!(snapshot("provider"), provider_before);
    }

    /// 归属校验保持 `WHERE id = ? AND owner_id = ?`（rant 第 6 条）：别人的行既改不动、也不
    /// 因为「存在但不是我的」而与「不存在」区分开（两种都是 404）。
    #[tokio::test]
    async fn patch_cannot_edit_another_users_sharing() {
        let st = test_state("editowner");
        let key = login(st.clone()).await;
        let (id, other_id) = {
            let conn = st.db.lock().unwrap();
            let uid: i64 = conn
                .query_row(
                    "SELECT id FROM users WHERE email = 'demo@aitokenpool.local'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            conn.execute(
                "INSERT INTO users (email, password_hash, name, role) VALUES ('other2@x.local', 'x', 'o', 'user')",
                [],
            )
            .unwrap();
            let other_uid = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO keys (owner_id, provider, plan, model, status, encrypted_key, quota, used, \
                 available_days, available_start, available_end, note) \
                 VALUES (?1, 'deepseek', 'deepseek-paygo', 'deepseek-flash', 'on', 'v1:x', 10, 0, '', '', '', '别人的')",
                [other_uid],
            )
            .unwrap();
            let other_id = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO keys (owner_id, provider, plan, model, status, encrypted_key, quota, used, \
                 available_days, available_start, available_end, note) \
                 VALUES (?1, 'deepseek', 'deepseek-paygo', 'deepseek-flash', 'on', 'v1:x', 10, 0, '', '', '', '我的')",
                [uid],
            )
            .unwrap();
            (conn.last_insert_rowid(), other_id)
        };

        // 别人的行：改任何字段都是 404（不是 403 —— 不透露该 id 是否存在）
        let (s, _) = send(
            st.clone(),
            "PATCH",
            &format!("/api/sharings/{other_id}"),
            Some(r#"{"note":"偷改"}"#),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::NOT_FOUND);
        assert_eq!(snapshot_note(&st, other_id), "别人的", "不得改动别人的行");

        // 阳性对照：我自己的行改得动（证明 404 来自归属而不是「PATCH 全坏了」）
        let (s, _) = send(
            st.clone(),
            "PATCH",
            &format!("/api/sharings/{id}"),
            Some(r#"{"note":"我的新备注"}"#),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK);
        assert_eq!(snapshot_note(&st, id), "我的新备注");
    }

    fn snapshot_note(st: &AppState, id: i64) -> String {
        let conn = st.db.lock().unwrap();
        conn.query_row("SELECT note FROM keys WHERE id = ?1", [id], |r| r.get(0))
            .unwrap()
    }
}
