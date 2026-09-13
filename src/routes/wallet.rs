//! 钱包 / 交易 / 仪表盘 API（对齐原型钱包页 + 交易页 + 仪表盘）
//!
//! P0-C（rant 2026-08-18T10:36:04）：
//! - GET /api/wallet → {balance, month_use, month_earn}
//! - GET /api/transactions?type=&page=&page_size= → 分页 + type 过滤（consume/earn/topup/gift/expire/withdraw/all）
//! - GET /api/dashboard → 本月按类型聚合 + 本月净变化 + 近 7 天净额序列（sparkline）

use axum::extract::{Query, State};
use axum::Json;
use rusqlite::params_from_iter;
use serde::Deserialize;

use crate::dao;
use crate::routes::{internal, ApiErr, AppState, AuthUser};

/// 交易 `type` 过滤器的**唯一真源** —— 凡是受理 `type` 查询参数的端点都用它校验。
///
/// 前端交易页「类型」列筛选的选项即此集合（`ui/js/app.js` 的 tx type options），且前端把
/// **同一个** `type` 值同时发给 `/api/transactions` 与 `/api/transactions/trend`
/// （`app.js` 的 `loadTransactions`）⇒ 两个端点必须接受**完全相同**的取值集合，否则同一个
/// 筛选值会在一端 200、另一端 400（C2052：趋势端点曾漏 `expire`，列表正常而趋势图只显示
/// 「趋势数据加载失败」）。**新增类型时只改这里。**
///
/// ⚠️ 这是「**受理哪些值**」的集合，不是「值算收入还是支出」的**方向**集合 —— 方向由
/// [`TX_INCOME_TYPES`] / [`TX_EXPENSE_TYPES`] 定义，两者概念不同，勿混用
/// （`withdraw` 受理但尚无 writer：受理集合与方向集合本就不是同一件事）。
pub const TX_FILTER_TYPES: [&str; 6] = ["consume", "earn", "topup", "gift", "expire", "withdraw"];

/// 交易**方向**的唯一真源 —— 收入类型（点数入账）；其补集见 [`TX_EXPENSE_TYPES`]。
///
/// 方向由 `type` 决定、**不由 `pts` 的符号决定**（每个 writer 都把 `pts` 存成非负数，
/// C2045/C2047/C2050）。本模块消费它的地方有两处，且**都从这一个数组取值**：
///
/// 1. **SQL** —— 经 [`sql_in_list`] 渲染成 `IN (…)` 值列表（[`signed_pts_expr`]、`summary`、`trend`）；
/// 2. **Rust** —— `dashboard` 的 `net` 直接 `contains` 判断方向（C2056：此处曾内联
///    `match ty { "earn" | "topup" | "gift" => … }`，是同一规则的**第二份副本**——
///    只改本数组不会影响它；现已改为同源取值）。
///
/// ⇒ 调整**某个已有类型的方向**只需改这两个数组，SQL 与 Rust 两侧同时生效。
/// 凡需要「有符号点数」的地方一律用 [`signed_pts_expr`]，勿再写内联字面量。
const TX_INCOME_TYPES: [&str; 3] = ["earn", "topup", "gift"];

/// 交易方向：支出类型（离开账户）—— 消费 / 赠送过期 / 提现。见 [`TX_INCOME_TYPES`]。
const TX_EXPENSE_TYPES: [&str; 3] = ["consume", "expire", "withdraw"];

/// 把方向集合渲染成 SQL 的 `IN (…)` 值列表（如 `'earn','topup','gift'`）——
/// SQL 与 Rust 共用同一份集合（C2056），因此两侧不可能再分叉。
fn sql_in_list(types: &[&str]) -> String {
    types
        .iter()
        .map(|t| format!("'{t}'"))
        .collect::<Vec<_>>()
        .join(",")
}

/// 「点数」列在 UI 上**渲染/展示**的有符号值：收入为正、支出为负
/// （前端 `signedPts()`，`ui/js/app.js`）。
///
/// 该列的一切消费方 —— 区间筛选、排序、CSV 导出 —— 都必须与此同口径：筛选/排序是对
/// 「用户看到的数字」的主张，不是对库内原始值的主张。C2054：区间筛选曾直接比较库内 `pts`，
/// 而每个 writer 都存正数 ⇒ 用户按表里看到的负数（如 `-3.7`）筛选会得到 **0 行**。
///
/// `prefix` 是表别名（如 `"t"`；无别名时传 `""`），与 [`tx_where`] 的 `col()` 同一约定。
fn signed_pts_expr(prefix: &str) -> String {
    let col = |c: &str| {
        if prefix.is_empty() {
            c.to_string()
        } else {
            format!("{prefix}.{c}")
        }
    };
    let (ty, pts) = (col("type"), col("pts"));
    format!(
        "(CASE WHEN {ty} IN ({}) THEN {pts} ELSE -{pts} END)",
        sql_in_list(&TX_INCOME_TYPES)
    )
}

/// 解析 `type` 查询参数：`""` / `"all"` → `None`（不筛），[`TX_FILTER_TYPES`] 成员 → `Some(成员)`，
/// 其余 → 400（文案由 [`TX_FILTER_TYPES`] **派生**，因此不可能再与集合分叉）。
/// `/api/transactions` 与 `/api/transactions/trend` 共用此函数。
fn parse_tx_type(raw: &str) -> Result<Option<String>, ApiErr> {
    match raw {
        "" | "all" => Ok(None),
        t if TX_FILTER_TYPES.contains(&t) => Ok(Some(t.to_string())),
        _ => Err((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("type 必须为 {} / all", TX_FILTER_TYPES.join(" / ")),
            })),
        )),
    }
}

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

/// 交易列筛选（rant 2026-08-25T10:33:26：列筛选从本地当前页改为后端全量过滤）。
/// 与前端 TX_COLUMNS 各列 filter 对应：文本列（model/user_name/key_name）LIKE 匹配、
/// select 列（status）精确匹配、number-range（pts）区间匹配；与 type/start/end 叠加。
/// 值为字符串以宽容空值/非法输入（解析失败按未筛处理）。
#[derive(Debug, Default, Deserialize)]
pub struct TxColFilters {
    /// 模型名 LIKE（%v%）
    pub model: Option<String>,
    /// 用户名 LIKE（JOIN users u）
    pub user_name: Option<String>,
    /// Key 名 LIKE（JOIN api_keys ak；历史行兜底 key_label 表达式）
    pub key_name: Option<String>,
    /// 状态精确匹配（库内中文值：成功/入账/处理中）
    pub status: Option<String>,
    /// 点数下限（>=，按列**渲染的有符号值**：收入正 / 支出负 —— 与 `signedPts()` 同口径）
    pub pts_min: Option<String>,
    /// 点数上限（<=，同上：有符号值）
    pub pts_max: Option<String>,
}

/// GET /api/transactions 查询参数
#[derive(Debug, Deserialize)]
pub struct TxQuery {
    /// 交易类型过滤（缺省 `all` 不筛）；取值见 [`TX_FILTER_TYPES`]，非法值 400
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
    /// 列筛选（model/user_name/key_name/status/pts_min/pts_max）
    #[serde(flatten)]
    pub filters: TxColFilters,
}

/// 构建 transactions 查询条件与绑定参数。
/// 参数顺序固定：user_id → type → start → end → 列筛选（存在的才加入，占位符序号递增）。
/// 列筛选引用 JOIN 表列（u.name / ak.name / k.*）——调用方须带对应 LEFT JOIN
/// （列表/summary/trend 统一带 keys/users/api_keys 三 JOIN，LEFT JOIN 主键 1:1 不放大行数）。
fn tx_where(
    prefix: &str,
    user_id: i64,
    type_filter: &Option<String>,
    start: &Option<String>,
    end: &Option<String>,
    f: &TxColFilters,
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
    // 列筛选：文本列 LIKE、status 精确、pts 区间
    let like = |v: &Option<String>| -> Option<String> {
        v.as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| format!("%{s}%"))
    };
    if let Some(s) = like(&f.model) {
        conds.push(format!("{} LIKE ?{}", col("model"), binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(s));
    }
    if let Some(s) = like(&f.user_name) {
        conds.push(format!("u.name LIKE ?{}", binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(s));
    }
    if let Some(s) = like(&f.key_name) {
        // 与前端 Key 列显示口径一致：key_name 优先，历史行兜底 key_label（note/provider/plan）
        conds.push(format!(
            "COALESCE(ak.name, CASE WHEN k.note <> '' THEN k.note \
                WHEN k.plan <> '' THEN k.provider || ' / ' || k.plan \
                ELSE k.provider END) LIKE ?{}",
            binds.len() + 1
        ));
        binds.push(rusqlite::types::Value::Text(s));
    }
    if let Some(s) = f
        .status
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        conds.push(format!("{} = ?{}", col("status"), binds.len() + 1));
        binds.push(rusqlite::types::Value::Text(s.to_string()));
    }
    // pts 区间：按**渲染的有符号值**比较（收入正 / 支出负），与列渲染、summary/trend 同口径。
    // C2054：原先直接比较库内 `pts`，而每个 writer 都存正数 ⇒ 用户按表里看到的负数筛选得 0 行。
    let signed_pts = signed_pts_expr(prefix);
    for (v, op) in [(f.pts_min.as_deref(), ">="), (f.pts_max.as_deref(), "<=")] {
        if let Some(n) = v.and_then(|s| s.trim().parse::<f64>().ok()) {
            conds.push(format!("{signed_pts} {op} ?{}", binds.len() + 1));
            binds.push(rusqlite::types::Value::Real(n));
        }
    }
    (conds.join(" AND "), binds)
}

/// transactions 三 JOIN（keys/users/api_keys）片段：列表/summary/trend 共用，
/// 使列筛选中的 user_name/key_name 可引用 JOIN 表列。
fn tx_joins() -> &'static str {
    "LEFT JOIN keys k ON k.id = t.key_id \
     LEFT JOIN users u ON u.id = t.user_id \
     LEFT JOIN api_keys ak ON ak.id = t.api_key_id"
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
    // 类型过滤：取值集合与 400 文案均来自 TX_FILTER_TYPES（列筛选 select 含 withdraw，
    // rant 2026-08-25T10:33.26：列筛选后端化后 UI 选项须全被 API 接受）。
    let type_filter = parse_tx_type(&q.r#type)?;
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
    // 按当前 type + 时间段 + 列筛选；口径 = income 白名单（earn/topup/gift）为正，
    // 支出 = 其余「离开账户」的类型（consume 消费 / expire 赠送过期 / withdraw 提现）——
    // 方向由 `type` 决定，不是 `pts` 的符号（每个 writer 都存正数，C2045/C2050）。
    let (where_sql, where_binds) =
        tx_where("t", auth.user_id, &type_filter, &start, &end, &q.filters);
    let signed_pts = signed_pts_expr("t");
    let income = sql_in_list(&TX_INCOME_TYPES);
    let expense = sql_in_list(&TX_EXPENSE_TYPES);
    let summary_sql = format!(
        "SELECT \
            COALESCE(SUM(CASE WHEN t.type IN ({income}) THEN t.pts ELSE 0 END), 0), \
            COALESCE(SUM(CASE WHEN t.type IN ({expense}) THEN t.pts ELSE 0 END), 0), \
            COALESCE(SUM({signed_pts}), 0), \
            COALESCE(SUM(t.tokens), 0), \
            COALESCE(SUM(t.tokens - t.cached_tokens - t.output_tokens), 0), \
            COALESCE(SUM(t.cached_tokens), 0), \
            COALESCE(SUM(t.output_tokens), 0) \
            FROM transactions t {} WHERE {where_sql}",
        tx_joins()
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
            &format!(
                "SELECT COUNT(*) FROM transactions t {} WHERE {where_sql}",
                tx_joins()
            ),
            params_from_iter(where_binds.iter()),
            |r| r.get(0),
        )
        .unwrap_or(0);
    let offset = (page - 1) * page_size;
    // rant 2026-08-22T06:36:54/06:37:50：模型/Key 列 — 补 key_label（JOIN keys：
    // note 非空用 note，否则 provider / plan，plan 空则仅 provider；key 已删 → NULL）
    let (list_where, mut list_binds) =
        tx_where("t", auth.user_id, &type_filter, &start, &end, &q.filters);
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

/// GET /api/transactions/trend 查询参数
#[derive(Debug, Deserialize)]
pub struct TxTrendQuery {
    /// 交易类型过滤（缺省 `all` 不筛）；取值见 [`TX_FILTER_TYPES`]，非法值 400 ——
    /// 与 `/api/transactions` 同一集合（前端把同一个值发给两个端点）
    #[serde(default)]
    pub r#type: String,
    /// 起始时间（同 /api/transactions，ISO 8601 UTC）
    pub start: Option<String>,
    /// 结束时间（同 /api/transactions，ISO 8601 UTC）
    pub end: Option<String>,
    /// 聚合粒度：hour / day / week（缺省 day；非法值回退 day）
    #[serde(default)]
    pub bucket: String,
    /// 列筛选（model/user_name/key_name/status/pts_min/pts_max，与 /api/transactions 一致）
    #[serde(flatten)]
    pub filters: TxColFilters,
}

/// GET /api/transactions/trend（rant 2026-08-23T16:01:07：交易页趋势图数据源）
/// 按时间桶聚合，口径与 summary 一致（income 白名单 earn/topup/gift 为正、其余支出类型为负 ——
/// 方向由 `type` 决定，不是 `pts` 的符号；
/// token 字段：tokens 总 / input = tokens − cached − output / cached / output）。
/// 返回仅含非空桶，按时间升序；前端负责连续时间轴补齐。
pub async fn transactions_trend(
    State(st): State<AppState>,
    auth: AuthUser,
    Query(q): Query<TxTrendQuery>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    // 与 /api/transactions **共用**同一解析（C2052：此处曾是一个手写的白名单副本，
    // C2051 给类型集合加 `expire` 时只改了列表端点 ⇒ 同一筛选值在列表 200、在趋势 400）。
    let type_filter = parse_tx_type(&q.r#type)?;
    let bucket = match q.bucket.as_str() {
        "hour" | "day" | "week" => q.bucket.clone(),
        _ => "day".to_string(),
    };
    let conn = st.db.lock().map_err(|_| internal("db lock poisoned"))?;
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
    let (where_sql, where_binds) =
        tx_where("t", auth.user_id, &type_filter, &start, &end, &q.filters);
    // 桶表达式：day/week 产出 "YYYY-MM-DD"，hour 产出 "YYYY-MM-DD HH:00"
    let expr = match bucket.as_str() {
        "hour" => "%Y-%m-%d %H:00",
        "week" => "%Y-%m-%d", // 配 modifier：周一起始
        _ => "%Y-%m-%d",
    };
    let mods = if bucket == "week" {
        ", 'weekday 1', '-7 days'"
    } else {
        ""
    };
    let signed_pts = signed_pts_expr("t");
    let income = sql_in_list(&TX_INCOME_TYPES);
    let expense = sql_in_list(&TX_EXPENSE_TYPES);
    let trend_sql = format!(
        "SELECT strftime('{expr}', t.time{mods}) AS b, \
            COALESCE(SUM(CASE WHEN t.type IN ({income}) THEN t.pts ELSE 0 END), 0), \
            COALESCE(SUM(CASE WHEN t.type IN ({expense}) THEN t.pts ELSE 0 END), 0), \
            COALESCE(SUM({signed_pts}), 0), \
            COALESCE(SUM(t.tokens), 0), \
            COALESCE(SUM(t.tokens - t.cached_tokens - t.output_tokens), 0), \
            COALESCE(SUM(t.cached_tokens), 0), \
            COALESCE(SUM(t.output_tokens), 0), \
            COUNT(*) \
         FROM transactions t {} WHERE {where_sql} GROUP BY b ORDER BY b",
        tx_joins()
    );
    let mut stmt = conn.prepare(&trend_sql).map_err(internal)?;
    let buckets: Vec<serde_json::Value> = stmt
        .query_map(params_from_iter(where_binds.iter()), |r| {
            let b: String = r.get(0)?;
            // 桶起点 → UTC ISO（hour 桶带小时，day/week 桶为当日 00:00）
            let iso = if bucket == "hour" {
                format!("{}:00Z", b.replace(' ', "T"))
            } else {
                format!("{b}T00:00:00Z")
            };
            Ok(serde_json::json!({
                "t": iso,
                "income": r.get::<_, f64>(1)?,
                "expense": r.get::<_, f64>(2)?,
                "net": r.get::<_, f64>(3)?,
                "tokens": r.get::<_, f64>(4)?,
                "input_tokens": r.get::<_, f64>(5)?,
                "cached_tokens": r.get::<_, f64>(6)?,
                "output_tokens": r.get::<_, f64>(7)?,
                "count": r.get::<_, i64>(8)?,
            }))
        })
        .map_err(internal)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(internal)?;
    Ok(Json(serde_json::json!({
        "bucket": bucket,
        "buckets": buckets,
    })))
}

/// GET /api/dashboard：本月按类型聚合 + 本月净变化 + 近 7 天净额序列。
///
/// 两个窗口刻意不同，各自标注清楚（C2049：`net` 曾按 `series` 求和，与它上方的当月行自相矛盾）：
/// - `month`（逐类型）与 `net`（净变化）＝**本月**（`strftime('%Y-%m', time)`）；`net` 就是 `month` 行的有符号和；
/// - `series` ＝**近 7 天**（`date(time) >= date('now','-6 days')`），仅供 sparkline 使用。
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
        .prepare(&format!(
            "SELECT date(time), COALESCE(SUM({}), 0) \
             FROM transactions \
             WHERE user_id = ?1 AND date(time) >= date('now', '-6 days') \
             GROUP BY date(time) ORDER BY date(time)",
            signed_pts_expr("")
        ))
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
    // 净变化 = 上方逐类型行的有符号和（同一个月窗口）：
    // 收入 = [`TX_INCOME_TYPES`] 成员（earn/topup/gift），其余为支出 —— 方向取自**同一个数组**，
    // 与下方 series 的 CASE、transactions_trend 的 net 列、routes/ops.rs 的月份流水同口径
    // （每个 writer 都写非负 pts，方向由 type 决定）。C2056：此处曾内联 `match "earn"|"topup"|"gift"`。
    let net: f64 = month
        .iter()
        .map(|m| {
            let pts = m["pts"].as_f64().unwrap_or(0.0);
            if TX_INCOME_TYPES.contains(&m["type"].as_str().unwrap_or("")) {
                pts
            } else {
                -pts
            }
        })
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
    async fn dashboard_net_is_the_month_net_not_the_7_day_series() {
        // C2049（施工单 fix/dashboard-net-window）：`/api/dashboard` 的 `net` 曾按 `series`
        // （近 7 天）求和，而它上方的逐类型行是**本月**聚合 ⇒ 面板里「本月净变化」那一行
        // 不等于它下面那些行之和。断言写的是**规格**（用独立 SQL 算当月有符号和），不是实现里的算式。
        //
        // 夹具刻意横跨两个窗口（纯数据，不伪造时钟）：
        //   A: consume 10.0 @ datetime('now','start of month') —— 必在当月；仅当「日 ≤ 7」才落在近 7 天窗口内
        //   B: consume  4.0 @ datetime('now','-6 days')        —— 必在近 7 天窗口内；仅当「日 ≥ 7」才落在当月
        //   C: earn     3.0 @ now                              —— 两个窗口都必含
        // ⚠️ 校准：两个窗口只在本月第 7 天重合（那天窗口 = 当月 1..7 日 ⊆ 当月），此时**任何**纯数据夹具都
        // 观测不到差异（坑 144：观测能力有定义域，写出来，不用时钟技巧硬凑）。故本测试在「日 1..6 / 8..31」
        // 能判别新实现与旧实现，第 7 天两条实现都通过 —— 断言仍是规格，只是当天的数据不具备判别力。
        let st = test_state("dashmonthnet");
        let key = login(st.clone()).await;
        {
            let conn = st.db.lock().unwrap();
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status, time) \
                 VALUES (1, '2', 1, 'm', 150, 10.0, 'consume', '成功', datetime('now','start of month'))",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status, time) \
                 VALUES (1, '2', 1, 'm', 150, 4.0, 'consume', '成功', datetime('now','-6 days'))",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status, time) \
                 VALUES (1, '3', 2, 'm', 150, 3.0, 'earn', '成功', datetime('now'))",
                [],
            )
            .unwrap();
        }
        let (s, body) = get(st.clone(), "/api/dashboard", &key).await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let net = v["net"].as_f64().unwrap();
        // 独立 SQL：按同一「方向由 type 决定」的约定，分别在本月窗口与近 7 天窗口上求和
        let (month_spec, series_spec) = {
            let conn = st.db.lock().unwrap();
            let sign = "CASE WHEN type IN ('earn','topup','gift') THEN pts ELSE -pts END";
            let m: f64 = conn
                .query_row(
                    &format!(
                        "SELECT COALESCE(SUM({sign}), 0) FROM transactions \
                         WHERE user_id = 1 AND strftime('%Y-%m', time) = strftime('%Y-%m', 'now')"
                    ),
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            let w: f64 = conn
                .query_row(
                    &format!(
                        "SELECT COALESCE(SUM({sign}), 0) FROM transactions \
                         WHERE user_id = 1 AND date(time) >= date('now', '-6 days')"
                    ),
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            (m, w)
        };
        eprintln!(
            "C2049 dashboard month={} series={} net={} | month_spec={month_spec} series_spec={series_spec} \
             discriminating_today={}",
            v["month"],
            v["series"],
            v["net"],
            (month_spec - series_spec).abs() > 1e-9
        );
        assert!(
            (net - month_spec).abs() < 1e-9,
            "net 必须是本月净变化（= 上方逐类型行之和 = {month_spec}），实际 {net}"
        );
    }

    #[tokio::test]
    async fn dashboard_net_direction_holds_for_every_type() {
        // C2056（施工单 fix/direction-set-single-source）：`dashboard` 的 `net` 判定方向时曾内联
        // `match ty { "earn" | "topup" | "gift" => … }` —— 与 `signed_pts_expr` 用的 SQL 白名单是
        // 同一条规则的**两份副本**，只改其一不会影响另一。本测试把**六种类型全部**摆出来，期望值
        // **硬编码在测试里**（不从任何常量派生）⇒ 任一侧被单独改动都会让它变红。
        //
        // 期望（收入为正 / 支出为负）：
        //   earn 3.0 + topup 2.0 + gift 1.0 = 6.0（收入）
        //   consume 10.0 + expire 0.5 + withdraw 0.25 = 10.75（支出）
        //   net = 6.0 − 10.75 = −4.75
        let st = test_state("dashalltypes");
        let key = login(st.clone()).await;
        let rows = [
            ("earn", 3.0),
            ("topup", 2.0),
            ("gift", 1.0),
            ("consume", 10.0),
            ("expire", 0.5),
            ("withdraw", 0.25),
        ];
        {
            let conn = st.db.lock().unwrap();
            for (ty, pts) in rows {
                conn.execute(
                    "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status, time) \
                     VALUES (1, 'c', 1, 'm', 0, ?1, ?2, '成功', datetime('now','start of month'))",
                    rusqlite::params![pts, ty],
                )
                .unwrap();
            }
        }
        let (s, body) = get(st.clone(), "/api/dashboard", &key).await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        // 逐类型行＝本月按 type 聚合，原样返回库内的**非负** pts（方向不由 pts 符号决定）
        let month: std::collections::HashMap<String, f64> = v["month"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                (
                    m["type"].as_str().unwrap().to_string(),
                    m["pts"].as_f64().unwrap(),
                )
            })
            .collect();
        for (ty, pts) in rows {
            assert!(
                (month[ty] - pts).abs() < 1e-9,
                "month[{ty}] 应为库内非负值 {pts}，实际 {month:?}"
            );
        }
        let net = v["net"].as_f64().unwrap();
        assert!(
            (net - (-4.75)).abs() < 1e-9,
            "net 应为「收入(earn+topup+gift) − 支出(consume+expire+withdraw)」= −4.75，实际 {net}"
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
    async fn transactions_column_filters() {
        // rant 2026-08-25T10:33:26：列筛选（model/user_name/key_name/status/pts 区间）
        // 从本地当前页过滤改为后端全量过滤——列表 total/summary 联动；trend 同口径。
        let st = test_state("txcolf");
        let key = login(st.clone()).await;
        {
            let conn = st.db.lock().unwrap();
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
            // 3 条：model/点数各异；1 条 earn + 状态「处理中」
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, api_key_id, model, tokens, pts, type, status) \
                 VALUES (1, '2', 1, ?1, 'deepseek-v4-flash', 100, 10.0, 'consume', '成功')",
                rusqlite::params![ak_id],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status) \
                 VALUES (1, '3', 1, 'gpt-4o', 200, 20.0, 'consume', '成功')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status) \
                 VALUES (1, '4', 2, 'claude-3-5-sonnet', 300, 30.0, 'earn', '处理中')",
                [],
            )
            .unwrap();
        }
        // model LIKE（%deepseek%）→ 1 条；summary 联动为消费 10
        let (s, body) = get(st.clone(), "/api/transactions?model=deepseek", &key).await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["total"], 1, "model LIKE 全量过滤: {body}");
        assert_eq!(
            v["items"].as_array().unwrap()[0]["model"],
            "deepseek-v4-flash"
        );
        assert!(
            (v["summary"]["expense_pts"].as_f64().unwrap() - 10.0).abs() < 1e-9,
            "summary 随列筛选联动: {body}"
        );
        // status 精确（处理中，URL-encoded）→ 1 条 earn
        let (s, body) = get(
            st.clone(),
            "/api/transactions?status=%E5%A4%84%E7%90%86%E4%B8%AD",
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["total"], 1, "status 精确过滤: {body}");
        assert_eq!(v["items"].as_array().unwrap()[0]["type"], "earn");
        // pts 区间：**按列渲染的有符号值**比较（C2054）——三条行的呈现值是
        // consume -10.0 / consume -20.0 / earn +30.0，故区间 [-25,-15] → 1 条（-20.0）
        let (s, body) = get(
            st.clone(),
            "/api/transactions?pts_min=-25&pts_max=-15",
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["total"], 1, "pts 区间按渲染值过滤: {body}");
        assert!((v["items"].as_array().unwrap()[0]["pts"].as_f64().unwrap() - 20.0).abs() < 1e-9);
        // 阴性对照：库里存的正数 20.0 不再命中 [15,25]（修复前正是它命中）——
        // 若此断言失败，说明筛选又退回了「比较库内原始 pts」
        let (s, body) = get(st.clone(), "/api/transactions?pts_min=15&pts_max=25", &key).await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["total"], 0, "库内正数不得再命中（渲染值是负数）: {body}");
        // 阳性对照：收入行的渲染值 +30.0 仍落在 [+25,+35]（筛选没有把收入也一起翻掉）
        let (s, body) = get(st.clone(), "/api/transactions?pts_min=25&pts_max=35", &key).await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["total"], 1, "收入行按渲染值命中: {body}");
        assert_eq!(v["items"].as_array().unwrap()[0]["type"], "earn");
        // user_name LIKE（demo，全部 3 条命中）
        let (s, body) = get(st.clone(), "/api/transactions?user_name=dem", &key).await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["total"], 3, "user_name LIKE: {body}");
        // key_name LIKE（我的测试key）→ 仅 api_key_id 那条（10.0）
        let (s, body) = get(
            st.clone(),
            "/api/transactions?key_name=%E6%88%91%E7%9A%84",
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["total"], 1, "key_name LIKE: {body}");
        assert!((v["items"].as_array().unwrap()[0]["pts"].as_f64().unwrap() - 10.0).abs() < 1e-9);
        // 列筛选 + type 叠加（model=deepseek & type=consume）→ 仍 1 条
        let (s, body) = get(
            st.clone(),
            "/api/transactions?type=consume&model=deepseek",
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["total"], 1, "type + 列筛选叠加: {body}");
        // trend 与列表同口径（model=deepseek → 单桶 expense=10）
        let (s, body) = get(
            st.clone(),
            "/api/transactions/trend?model=deepseek&bucket=day",
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let buckets = v["buckets"].as_array().unwrap();
        assert_eq!(buckets.len(), 1, "trend 随列筛选: {body}");
        assert!(
            (buckets[0]["expense"].as_f64().unwrap() - 10.0).abs() < 1e-9,
            "{body}"
        );
    }

    #[tokio::test]
    async fn pts_range_filter_matches_the_rendered_signed_value() {
        // C2054：交易页「点数」列**渲染**的是有符号值（income 正 / expense 负，`signedPts()`），
        // 但区间筛选曾比较库内原始 `pts`（每个 writer 都存正数）⇒ 用户按表里看到的数字筛选
        // （例如「点数 ≤ 0」想看支出）会得到 0 行。规格：**筛选命中的行 = 渲染值落在区间内的行**。
        let st = test_state("ptsrange");
        let key = login(st.clone()).await;
        {
            let conn = st.db.lock().unwrap();
            // 与 billing.rs 存的完全一样：两行都存正数，方向在 type
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status) \
                 VALUES (1, '2', 1, 'm', 150, 3.7, 'consume', '成功')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, key_id, model, tokens, pts, type, status) \
                 VALUES (1, '3', 2, 'm', 150, 0.5, 'earn', '成功')",
                [],
            )
            .unwrap();
        }
        // 渲染值：consume → -3.7、earn → +0.5
        let cases: [(&str, usize, &str); 4] = [
            // 「点数 ≤ 0」＝用户眼里所有支出行
            ("pts_max=0", 1, "consume"),
            // 直接框住单元格里的 -3.7
            ("pts_min=-4&pts_max=-3", 1, "consume"),
            // 没有单元格落在这个区间（-3.7/+0.5 都不在）——修复前返回那条存了 3.7 的消费行
            ("pts_min=3&pts_max=4", 0, ""),
            // 阳性对照：收入行的渲染值 +0.5 仍被命中（证明筛选不是把一切都翻成负数）
            ("pts_min=0&pts_max=1", 1, "earn"),
        ];
        for (qs, want_total, want_type) in cases {
            let (s, body) = get(st.clone(), &format!("/api/transactions?{qs}"), &key).await;
            assert_eq!(s, axum::http::StatusCode::OK, "q={qs} body: {body}");
            let v: serde_json::Value = serde_json::from_str(&body).unwrap();
            assert_eq!(v["total"], want_total, "q={qs} 按渲染值命中数: {body}");
            if want_total > 0 {
                assert_eq!(
                    v["items"].as_array().unwrap()[0]["type"],
                    want_type,
                    "q={qs}: {body}"
                );
            }
        }
        // trend 与列表同口径（同一 where 片段）：pts_max=0 只留消费腿 → expense=3.7、income=0
        let (s, body) = get(
            st.clone(),
            "/api/transactions/trend?bucket=day&pts_max=0",
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let buckets = v["buckets"].as_array().unwrap();
        assert_eq!(buckets.len(), 1, "trend 同口径: {body}");
        assert!(
            (buckets[0]["expense"].as_f64().unwrap() - 3.7).abs() < 1e-9,
            "{body}"
        );
        assert!(
            (buckets[0]["income"].as_f64().unwrap()).abs() < 1e-9,
            "{body}"
        );
    }

    #[tokio::test]
    async fn transactions_trend_buckets() {
        // rant 2026-08-23T16:01:07：/api/transactions/trend 按时间桶聚合趋势
        // （口径与 summary 一致：income 白名单 earn/topup/gift 为正、consume 为负；token 四字段）
        let st = test_state("trend");
        let key = login(st.clone()).await;
        {
            let conn = st.db.lock().unwrap();
            // 今天：consume 1.0（token: 总100 / 缓存80 / 输出10 → 输入10）+ earn 2.0
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, model, tokens, cached_tokens, output_tokens, pts, type, status) \
                 VALUES (1, 'a', 'm', 100, 80, 10, 1.0, 'consume', '成功')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, model, tokens, pts, type, status) \
                 VALUES (1, 'b', 'm', 0, 2.0, 'earn', '成功')",
                [],
            )
            .unwrap();
            // 2 天前：consume 5.0
            conn.execute(
                "INSERT INTO transactions (user_id, counterpart, model, tokens, pts, type, status, time) \
                 VALUES (1, 'old', 'm', 0, 5.0, 'consume', '成功', datetime('now', '-2 days'))",
                [],
            )
            .unwrap();
        }
        let now = chrono::Utc::now();
        let z = |d: chrono::Duration| (now - d).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        // day bucket + 3 天窗口 → 2 个非空桶（2 天前 + 今天），时间升序
        let (s, body) = get(
            st.clone(),
            &format!(
                "/api/transactions/trend?type=all&bucket=day&start={}",
                z(chrono::Duration::days(3))
            ),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["bucket"], "day", "body: {body}");
        let buckets = v["buckets"].as_array().unwrap();
        assert_eq!(buckets.len(), 2, "应有 2 个非空桶: {body}");
        let old = &buckets[0];
        assert!(
            (old["expense"].as_f64().unwrap() - 5.0).abs() < 1e-9,
            "旧桶 expense=5: {body}"
        );
        assert_eq!(old["count"].as_i64().unwrap(), 1, "旧桶 count: {body}");
        let today = &buckets[1];
        assert!(
            (today["income"].as_f64().unwrap() - 2.0).abs() < 1e-9,
            "今天桶 income=2: {body}"
        );
        assert!(
            (today["expense"].as_f64().unwrap() - 1.0).abs() < 1e-9,
            "今天桶 expense=1: {body}"
        );
        assert!(
            (today["net"].as_f64().unwrap() - 1.0).abs() < 1e-9,
            "今天桶 net=1: {body}"
        );
        assert!(
            (today["tokens"].as_f64().unwrap() - 100.0).abs() < 1e-9,
            "今天桶 tokens=100: {body}"
        );
        assert!(
            (today["input_tokens"].as_f64().unwrap() - 10.0).abs() < 1e-9,
            "今天桶 input=10: {body}"
        );
        assert!(
            (today["cached_tokens"].as_f64().unwrap() - 80.0).abs() < 1e-9,
            "今天桶 cached=80: {body}"
        );
        assert!(
            (today["output_tokens"].as_f64().unwrap() - 10.0).abs() < 1e-9,
            "今天桶 output=10: {body}"
        );
        assert_eq!(today["count"].as_i64().unwrap(), 2, "今天桶 count: {body}");
        assert!(
            today["t"].as_str().unwrap().ends_with("T00:00:00Z"),
            "day 桶时间应为当日 00:00Z: {body}"
        );
        // hour bucket：今天两条 + 2 天前一条 → 2 个桶（跨天），t 带小时
        let (s, body) = get(
            st.clone(),
            &format!(
                "/api/transactions/trend?type=all&bucket=hour&start={}",
                z(chrono::Duration::days(3))
            ),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let buckets = v["buckets"].as_array().unwrap();
        assert_eq!(buckets.len(), 2, "hour 桶应 2 个: {body}");
        assert!(
            buckets
                .iter()
                .all(|b| chrono::DateTime::parse_from_rfc3339(b["t"].as_str().unwrap()).is_ok()),
            "hour 桶 t 应为合法 RFC3339: {body}"
        );
        // type=consume 筛选 → 只聚合 consume
        let (s, body) = get(
            st.clone(),
            &format!(
                "/api/transactions/trend?type=consume&bucket=day&start={}",
                z(chrono::Duration::days(3))
            ),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let buckets = v["buckets"].as_array().unwrap();
        assert_eq!(buckets.len(), 2, "consume 桶应 2 个: {body}");
        assert!(
            buckets
                .iter()
                .all(|b| (b["income"].as_f64().unwrap()).abs() < 1e-9),
            "consume 筛选下 income 应全 0: {body}"
        );
        // 非法 bucket → 回退 day 不报错
        let (s, body) = get(
            st.clone(),
            &format!(
                "/api/transactions/trend?type=all&bucket=month&start={}",
                z(chrono::Duration::days(3))
            ),
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["bucket"], "day", "非法 bucket 应回退 day: {body}");
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

    #[tokio::test]
    async fn expired_gift_is_visible_in_the_ledger_through_the_handlers() {
        // C2050：惰性清扫发生在 GET /api/wallet（ensure_daily_gift → expire_past_gifts）。
        // 过期点数真的离开账户 ⇒ 账单必须能看到这一行，且汇总口径把它算作**支出**。
        // 断言写的是规格（收入白名单 / 支出集合 / 净额 = 收入 − 支出 / 对账），不是实现的复述。
        let st = test_state("expire");
        let key = login(st.clone()).await;

        // 1) 首次拉钱包 → 触发每日赠送（1 点 active）
        let (s, body) = get(st.clone(), "/api/wallet", &key).await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let avail0 = serde_json::from_str::<serde_json::Value>(&body).unwrap()["available"]
            .as_f64()
            .unwrap();

        // 2) 把赠送推到过期，再拉钱包 → 清扫发生
        {
            let conn = st.db.lock().unwrap();
            conn.execute(
                "UPDATE gift_grants SET expires_at = datetime('now', '-1 minute') \
                 WHERE user_id = 1 AND status = 'active'",
                [],
            )
            .unwrap();
        }
        let (s, body) = get(st.clone(), "/api/wallet", &key).await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let avail1 = serde_json::from_str::<serde_json::Value>(&body).unwrap()["available"]
            .as_f64()
            .unwrap();
        assert!(
            (avail0 - avail1 - 1.0).abs() < 1e-9,
            "过期 1 点离开账户: {avail0} -> {avail1}"
        );

        // 3) 账单里能看到 expire 行，且汇总把它算作支出
        let (s, body) = get(
            st.clone(),
            "/api/transactions?type=all&page=1&page_size=50",
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let items = v["items"].as_array().unwrap();
        assert!(
            items
                .iter()
                .any(|r| r["type"] == "expire" && (r["pts"].as_f64().unwrap() - 1.0).abs() < 1e-9),
            "账单中应有 expire 行: {items:?}"
        );
        let inc = v["summary"]["income_pts"].as_f64().unwrap();
        let exp = v["summary"]["expense_pts"].as_f64().unwrap();
        let net = v["summary"]["net_pts"].as_f64().unwrap();
        assert!(
            (inc - 1.0).abs() < 1e-9,
            "收入只有赠送的 1 点（过期不算收入）: {inc}"
        );
        assert!(exp >= 1.0, "支出应含过期的 1 点: {exp}");
        assert!(
            (net - (inc - exp)).abs() < 1e-9,
            "净额 = 收入 − 支出: {inc} - {exp} != {net}"
        );

        // 4) type 过滤器接受 expire（列筛选选项后端化，rant 2026-08-25T10:33.26）
        let (s, body) = get(
            st.clone(),
            "/api/transactions?type=expire&page=1&page_size=50",
            &key,
        )
        .await;
        assert_eq!(
            s,
            axum::http::StatusCode::OK,
            "type=expire 应被接受: {body}"
        );
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["total"], 1, "只有一笔过期: {body}");

        // 5) 趋势（expense 列）同样把它算作支出
        let (s, body) = get(
            st.clone(),
            "/api/transactions/trend?type=all&bucket=day",
            &key,
        )
        .await;
        assert_eq!(s, axum::http::StatusCode::OK, "body: {body}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let buckets = v["buckets"].as_array().unwrap();
        let sum_exp: f64 = buckets.iter().map(|b| b["expense"].as_f64().unwrap()).sum();
        assert!(sum_exp >= 1.0, "趋势支出应含过期的 1 点: {sum_exp}");
    }

    #[tokio::test]
    async fn every_accepted_type_filter_value_is_accepted_by_both_transaction_endpoints() {
        // C2052 规格：前端把**同一个** `type` 值同时发给 /api/transactions 与
        // /api/transactions/trend（app.js 的 loadTransactions）⇒ 两个端点必须接受**完全相同**的
        // 取值集合。否则同一个筛选值会在一端 200、另一端 400，而前端把趋势请求的失败吞成
        // `trend = null` ⇒ 列表正确、趋势图只显示「趋势数据加载失败」。C2051 给集合加 `expire`
        // 时只在列表端点补了它，正是这么坏的。
        // 断言打在**单真源**（TX_FILTER_TYPES）上：以后新增类型若只改一个端点，此测试必红。
        let st = test_state("txfilterparity");
        let key = login(st.clone()).await;

        // 1) 集合里的每个成员，两个端点都必须接受
        for t in TX_FILTER_TYPES {
            for uri in [
                format!("/api/transactions?type={t}"),
                format!("/api/transactions/trend?type={t}&bucket=day"),
            ] {
                let (s, body) = get(st.clone(), &uri, &key).await;
                assert_eq!(
                    s,
                    axum::http::StatusCode::OK,
                    "`{t}` 应被两个端点接受（{uri}）: {body}"
                );
            }
        }

        // 2) 缺省与 all（不筛）两端皆 200
        for uri in [
            "/api/transactions",
            "/api/transactions/trend",
            "/api/transactions?type=all",
            "/api/transactions/trend?type=all",
        ] {
            let (s, body) = get(st.clone(), uri, &key).await;
            assert_eq!(s, axum::http::StatusCode::OK, "`{uri}` 应 200: {body}");
        }

        // 3) 阳性对照：集合之外的取值两端皆 400，且文案列出**同一份**清单（文案由集合派生）
        for uri in [
            "/api/transactions?type=refund",
            "/api/transactions/trend?type=refund&bucket=day",
        ] {
            let (s, body) = get(st.clone(), uri, &key).await;
            assert_eq!(
                s,
                axum::http::StatusCode::BAD_REQUEST,
                "`{uri}` 应 400: {body}"
            );
            for t in TX_FILTER_TYPES {
                assert!(body.contains(t), "400 文案应列出 `{t}`（{uri}）: {body}");
            }
        }
    }
}
