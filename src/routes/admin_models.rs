//! 管理员模型信息 CRUD（rant 2026-08-19T20:40:29）
//!
//! - GET    /api/admin/models：全部模型列表（含 id / 所有字段，供管理表格）
//! - POST   /api/admin/models：新增 {provider, model, currency, input_per_m, output_per_m,
//!   context_length?, max_output?, vision?, cache_hit_input_per_m?}
//!   校验：provider/model 非空、价格 ≥ 0、currency ∈ {USD, CNY}、(provider, model) 唯一冲突 409
//! - PATCH  /api/admin/models/:id：部分更新（任意字段）
//! - DELETE /api/admin/models/:id：直接删除（模型删除后该 model 的调用无价格行 → 0 计费，README 说明）
//! - 权限：require_admin（role=admin，否则 403）
//! - 注意：config.toml `[[models]]` 为唯一真源——启动 seed_models 会同步删除 config 中已移除的
//!   模型行（rant 2026-08-20T11:58:33），故 admin 运行时增删改在下次启动时会被 config 覆盖/清掉；
//!   持久化自定义模型请写入 config.toml。

use axum::extract::State;
use axum::Json;
use rusqlite::params;
use serde::Deserialize;

use crate::routes::{internal, ApiErr, AppState, AuthUser};

/// 新增模型请求
#[derive(Debug, Deserialize)]
pub struct ModelCreate {
    pub provider: String,
    pub model: String,
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default)]
    pub input_per_m: f64,
    #[serde(default)]
    pub output_per_m: f64,
    #[serde(default)]
    pub context_length: i64,
    #[serde(default)]
    pub max_output: i64,
    #[serde(default)]
    pub vision: i64,
    #[serde(default)]
    pub cache_hit_input_per_m: f64,
}

fn default_currency() -> String {
    "USD".to_string()
}

/// 部分更新请求（全字段 Option，只更新出现的字段）
#[derive(Debug, Default, Deserialize)]
pub struct ModelPatch {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub currency: Option<String>,
    pub input_per_m: Option<f64>,
    pub output_per_m: Option<f64>,
    pub context_length: Option<i64>,
    pub max_output: Option<i64>,
    pub vision: Option<i64>,
    pub cache_hit_input_per_m: Option<f64>,
}

/// 校验通用字段：价格 ≥ 0、vision ∈ {0,1}、currency 枚举
fn validate_common(
    currency: &str,
    input_per_m: f64,
    output_per_m: f64,
    vision: i64,
) -> Result<(), ApiErr> {
    if !(currency == "USD" || currency == "CNY") {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "currency 仅支持 USD | CNY" })),
        ));
    }
    if input_per_m < 0.0 || output_per_m < 0.0 {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "价格不能为负数" })),
        ));
    }
    if vision != 0 && vision != 1 {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "vision 仅支持 0 | 1" })),
        ));
    }
    Ok(())
}

/// 行 → JSON（与 dao::list_all_models 字段一致，另含 OpenAI 兼容 context_window）
fn row_to_json(r: &rusqlite::Row) -> rusqlite::Result<serde_json::Value> {
    Ok(serde_json::json!({
        "id": r.get::<_, i64>(0)?,
        "provider": r.get::<_, String>(1)?,
        "model": r.get::<_, String>(2)?,
        "currency": r.get::<_, String>(3)?,
        "input_per_m": r.get::<_, f64>(4)?,
        "output_per_m": r.get::<_, f64>(5)?,
        "context_length": r.get::<_, i64>(6)?,
        "max_output": r.get::<_, i64>(7)?,
        "vision": r.get::<_, i64>(8)?,
        "cache_hit_input_per_m": r.get::<_, f64>(9)?,
        "updated_at": crate::dao::utc_iso(&r.get::<_, String>(10)?),
        "context_window": r.get::<_, i64>(11)?,
    }))
}

const ROW_SELECT: &str = "SELECT id, provider, model, currency, input_per_m, output_per_m, \
                context_length, max_output, vision, cache_hit_input_per_m, updated_at, context_window \
         FROM models ";

/// GET /api/admin/models：全部模型（管理表格数据源）
pub async fn list(
    State(st): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<serde_json::Value>>, ApiErr> {
    crate::routes::org::require_admin(&auth)?;
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let out = crate::dao::list_all_models(&conn).map_err(internal)?;
    Ok(Json(out))
}

/// POST /api/admin/models：新增模型
pub async fn create(
    State(st): State<AppState>,
    auth: AuthUser,
    Json(req): Json<ModelCreate>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    crate::routes::org::require_admin(&auth)?;
    let provider = req.provider.trim().to_string();
    let model = req.model.trim().to_string();
    if provider.is_empty() || model.is_empty() {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "provider 与 model 不能为空" })),
        ));
    }
    validate_common(&req.currency, req.input_per_m, req.output_per_m, req.vision)?;
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let dup: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM models WHERE provider = ?1 AND model = ?2)",
            params![provider, model],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if dup {
        return Err((
            axum::http::StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "模型已存在（provider+model 唯一）" })),
        ));
    }
    let id: i64 = conn
        .query_row(
            "INSERT INTO models (provider, model, currency, input_per_m, output_per_m, \
                    context_length, max_output, vision, cache_hit_input_per_m, context_window, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, datetime('now')) \
             RETURNING id",
            params![
                provider,
                model,
                req.currency,
                req.input_per_m,
                req.output_per_m,
                req.context_length,
                req.max_output,
                req.vision,
                req.cache_hit_input_per_m,
                // OpenAI 兼容 context_window 与 context_length 对齐（/v1/models 用）
                req.context_length,
            ],
            |r| r.get::<_, i64>(0),
        )
        .map_err(internal)?;
    let row = conn
        .query_row(&format!("{ROW_SELECT} WHERE id = ?1"), [id], row_to_json)
        .map_err(internal)?;
    Ok(Json(row))
}

/// PATCH /api/admin/models/:id：部分更新
pub async fn patch(
    State(st): State<AppState>,
    auth: AuthUser,
    axum::extract::Path(id): axum::extract::Path<i64>,
    Json(req): Json<ModelPatch>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    crate::routes::org::require_admin(&auth)?;
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM models WHERE id = ?1)",
            [id],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if !exists {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "模型不存在" })),
        ));
    }
    // 部分更新：先取当前值，合并后校验 + 写回（简单且避免逐列拼接 SQL）
    let cur: (String, String, String, f64, f64, i64, i64, i64, f64) = conn
        .query_row(
            "SELECT provider, model, currency, input_per_m, output_per_m, \
                    context_length, max_output, vision, cache_hit_input_per_m \
             FROM models WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                ))
            },
        )
        .map_err(internal)?;
    let provider = req.provider.unwrap_or(cur.0).trim().to_string();
    let model = req.model.unwrap_or(cur.1).trim().to_string();
    let currency = req.currency.unwrap_or(cur.2);
    let input_per_m = req.input_per_m.unwrap_or(cur.3);
    let output_per_m = req.output_per_m.unwrap_or(cur.4);
    let context_length = req.context_length.unwrap_or(cur.5);
    let max_output = req.max_output.unwrap_or(cur.6);
    let vision = req.vision.unwrap_or(cur.7);
    let cache_hit_input_per_m = req.cache_hit_input_per_m.unwrap_or(cur.8);
    if provider.is_empty() || model.is_empty() {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "provider 与 model 不能为空" })),
        ));
    }
    validate_common(&currency, input_per_m, output_per_m, vision)?;
    // 改名时检查 (provider, model) 唯一（排除自身）
    let dup: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM models WHERE provider = ?1 AND model = ?2 AND id <> ?3)",
            params![provider, model, id],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if dup {
        return Err((
            axum::http::StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "模型已存在（provider+model 唯一）" })),
        ));
    }
    conn.execute(
        "UPDATE models SET provider = ?1, model = ?2, currency = ?3, input_per_m = ?4, \
                output_per_m = ?5, context_length = ?6, max_output = ?7, vision = ?8, \
                cache_hit_input_per_m = ?9, context_window = ?6, updated_at = datetime('now') \
         WHERE id = ?10",
        params![
            provider,
            model,
            currency,
            input_per_m,
            output_per_m,
            context_length,
            max_output,
            vision,
            cache_hit_input_per_m,
            id,
        ],
    )
    .map_err(internal)?;
    let row = conn
        .query_row(&format!("{ROW_SELECT} WHERE id = ?1"), [id], row_to_json)
        .map_err(internal)?;
    Ok(Json(row))
}

/// DELETE /api/admin/models/:id：直接删除（0 计费语义见 rant：无价格行 → 调用按 0 计费）
pub async fn remove(
    State(st): State<AppState>,
    auth: AuthUser,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    crate::routes::org::require_admin(&auth)?;
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
    let n = conn
        .execute("DELETE FROM models WHERE id = ?1", [id])
        .map_err(internal)?;
    if n == 0 {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "模型不存在" })),
        ));
    }
    Ok(Json(serde_json::json!({ "status": "ok", "id": id })))
}
