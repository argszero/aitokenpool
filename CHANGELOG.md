# Changelog

All notable changes are recorded here. Versions follow [SemVer](https://semver.org/).

## v0.7.22 (2026-09-11)

- **补齐全局 reset 层并让表单/按钮几何对齐原型（rant 2026-09-11T22:01:43，PR #159 f46882c）** — 纯前端改动（`ui/` 内，后端契约与部署结构不变，数据库仍留 NAS、不启用 WAL）：
  - **整层全局 reset（根因）**：原型 `docs/prototype/aitokenpool-console.html` 第 53–72 行的全局 reset 层在整站 UI 重设计迁移时**整层漏掉**，导致所有**未带 class 的表单控件**直接裸露浏览器默认外观 —— 这是登录页 / 共享管理 / 设置页与原型视觉脱节的单一根因。本次补齐 `button { font: inherit; cursor: pointer; border: 0; background: none; color: inherit }`、`input, select, textarea { font: inherit }`、`a { color: inherit; text-decoration: none }`、`img, svg { display: block; max-width: 100% }`、`p { text-wrap: pretty }`、`h1, h2, h3 { text-wrap: balance }` 等条目（均为元素选择器，权重低于既有 `.btn` / `.input` 类选择器）。原型那行 `:focus-visible` 未重复：实现早有等价规则，仅圆角沿用既有 `--radius-sm`（已在代码注释记录该决策）。
  - **补齐依赖 reset 的缺失组件类**：`.num`（此前只有 `.table .num`，表格外的 `.num` 丢失等宽与 tabular-nums 对齐）、`.row`（markup 里 6 处 `class="row"` 此前**完全没有规则**，按钮贴边且不换行）、`.divider`（原型类名，与历史别名 `.login-divider` 合并同规格）、独立 `.avatar`（此前仅 `.user-chip .avatar` 后代选择器）。
  - **几何与原型对齐**：`.btn`（min-height 40px / `padding: 0 16px` / 13.5px / inline-flex）、`.btn-sm`（32px / `0 11px`）、`.input`（`padding: 10px 12px` / 13.5px）、`.auth-tabs button` 显式写出 `border: 0` + `background: none` —— 消除未选中 tab 的浏览器原生 `2px outset` 立体边框，输入框高度 25px → 44px。
  - **12 个裸 input 补 `class="input"`**：登录 / 注册 / 验证码 / 找回密码四族（与全站其余 47 个已有 class 的 input 一致）。
  - **cache-bust** `?v=20260911-4` → `?v=20260911-5`（5 处引用）。
  - **实测证据**（headless computed-style 探针，同视口 1912×836，9 个视图）：浏览器默认样式控件 **108 → 0**、原生 3D（inset/outset）边框 **0**、`.row` 按钮行 18 组 0 异常；identity-keyed A/B 对 310 个元素 × 10 个 computed 字段比对显示 reset 的 `img,svg{display:block}` **未改变任何可见图标尺寸**、`border-radius` 0 处变化，39 处几何变化全为「向原型靠拢」的预期修复。
  - **质量门禁**：`cargo test` 148/148 + `cargo fmt --check` + `cargo clippy` + `node --check` ×4 + i18n ZH/EN 键集精确相等（762×2）。

## v0.7.21 (2026-09-11)

- **全站 UI 按新原型完整重设计（rant 2026-09-11T16:23:43，PR #152 → #157）** — 纯前端改动（后端契约不变），6 片独立可部署提交：
  - **设计 token 层（#152 bafd223）**：`:root` 与浅色主题全面迁移到 OKLCH 色彩系统（`--bg` / `--surface` / `--fg` / `--muted` / `--border` / `--accent` 等），26 个 token 与原型逐字一致；旧 token 名保留为别名，视觉零跳变；原型基线入库 `docs/prototype/aitokenpool-console.html`。
  - **登录页与侧边栏（#153 e33bf2e）**：登录页改左右分栏（左侧品牌叙事 + 三行端点卡片含 OpenAI Responses，右侧表单 + 登录/注册 tab，复用既有 `showAuthForm` 故验证码/找回密码全保留）；侧边栏重构为 brand → 分组导航 → 底部用户 chip（头像/主题切换/退出），游客态同样可达。
  - **共享组件层（#154 75dcb2c）**：补齐此前缺失的 `.btn-secondary` / `.btn-sm`，迁移 stat-card / pill / tag / trend / bar-list / toolbar / search / empty 等组件（全部使用 OKLCH token，明暗主题免覆盖），原型组件覆盖 48/48。
  - **8 视图全部对齐原型**：仪表盘 + 模型市场 + 共享管理（#155 7e094e0）、钱包 + 交易记录（#156 41bb6d1）、设置 + 管理 + 运营（#157 09e4121）。
  - **保留的强功能**：i18n 中英双语（键集 762×2，精确等集）、交易页列筛选整行（焦点保持 + IME 保护）、24h + 自定义时间段、后端真分页、tokenBrk 换行、CSV 导出、withdraw/gift 类型。
  - **新增后端支持**：`/api/ops/runtime` 追加 `today_hours`（今日按小时调用量，0–23 全量补零）与 `key_health`（按厂商聚合上游 key 健康度）。
  - **质量门禁**：`cargo test` 148/148 + `cargo fmt --check` + `cargo clippy` + `node --check` ×4 + i18n ZH/EN 键集精确相等 + headless 明暗双主题 DOM 标记与几何探针校验。

## v0.7.20 (2026-08-25)

- **交易页性能修复（rant 2026-08-25T12:02:13，PR #150）** — dev 库在 NFS 导致的页面慢（浏览器实测 /api/transactions 1.7s、刷新 ~5s）三层修复：① `db.rs open()` 设 `PRAGMA cache_size=-65536`（64MB）+ `mmap_size=67108864`，整库常驻进程内存，NFS 只首读一次（dev 实测 COUNT 2.39s → 0.40s）；② v12 迁移为 transactions 建 4 个索引 `(user_id)` / `(user_id, id DESC)` / `(user_id, time)` / `(user_id, type)`，消除 summary/COUNT/list 全表扫描；③ 前端 `loadTransactions` 用 `Promise.all` 并行拉列表与趋势图，去掉一次串行 ~0.9s 等待。不启用 WAL（网络文件系统不支持）、数据库仍留在 NAS。

## v0.7.19 (2026-08-25)

- **筛选输入不再丢焦点（rant 2026-08-25T11:15:16，PR #148）** — 修复 v0.7.18 服务端列筛选回归：每输入一个字母触发刷新并失去焦点。方案（宿主确认 B）：表头筛选行改为**只渲染一次、重建时保持存活**——buildDataTable 仅重建 tbody 与分页器，筛选输入框 DOM 永不销毁，焦点天然保留；聚焦时不做 fallback 恢复。

## v0.7.18 (2026-08-25)

- **交易列筛选改服务端全量（rant 2026-08-25T10:33:26，PR #146）** — 修复 v0.7.15 后端翻页引入的回归：表格内部列筛选（用户/模型/Key/状态/点数区间等）此前只过滤当前加载页（几十条），全量几千条不在范围内。现改为：后端 `/api/transactions` 支持列筛选参数（model/user_name/key_name LIKE、status 精确、pts_min/pts_max 区间，与 type/时间段叠加），前端筛选变化时重新向后端拉全量子集（page 重置 1）；summary/trend 同步叠加列筛选口径，各区域结果一致。

## v0.7.17 (2026-08-24)

- **趋势图 1:1 渲染修复（rant 2026-08-24T14:29:57，PR #144）** — 根因：SVG viewBox 640 在宽容器被等比放大 ~2.4x（线 1.4px 视觉 ≈3.4px、字 8px ≈19px，v0.7.16 调细"看不出来"即被放大吞掉）。修复：viewBox 宽动态 = 容器宽（1 viewBox 单位 ≈ 1 物理像素），线宽/字号按 CSS 值真实呈现；窄屏保持等比缩小不变形；窗口 resize 防抖重渲染。

## v0.7.16 (2026-08-24)

- **趋势图视觉比例精修（rant 2026-08-24T13:31:02，PR #142）** — 交易页"点数趋势"图更精致：折线 stroke-width 1.8→1.4、坐标轴/时间标签字号 9px→8px、标题 12px→11px、图例 11px→10px、指标切换按钮 12px→11px 并收紧间距；仅调视觉比例，渐变面积/平滑曲线/指标切换/悬停 tooltip 功能不变

## v0.7.15 (2026-08-24)

- **趋势图渐变修复（rant 2026-08-24T12:32:18，PR #138）** — 交易趋势图填充面积不再显示黑色实心：`<stop>` 的 stop-color/stop-opacity 改为内联属性（复用仪表盘 sparkline 写法）+ 每实例唯一渐变 id，删除不可靠的 CSS class 方案；4 个指标切换均显示对应色 0.35→0 渐变面积
- **交易列表时间精确显示（rant 2026-08-24T12:38:44，PR #139）** — 交易记录列表时间列由相对时间（"3 小时前"）改为精确本地时间（`2026-08-24 12:36:12`），相对时间移入悬停提示
- **API Key 最近使用真实数据（rant 2026-08-24T12:41:25，PR #140）** — 设置页 API Key「最近使用」不再硬编码"从未"：api_keys 表新增 last_used 字段，网关计费时更新，list 接口返回真实时间；未使用过的 key 才显示"从未"

## v0.7.14 (2026-08-24)

- **交易记录页真分页（rant 2026-08-24T10:51:57，PR #135）** — 修复假分页：改为后端翻页，`/api/transactions` 支持 `page`/`pageSize`，前端按表格分页参数拉取并显示真实总数（7320 条全部可达），紧凑省略号分页器 + 翻页滚动回顶部
- **点数趋势图 sparkline 化（rant 2026-08-24T10:51:57，PR #136）** — 参考仪表盘「本月点数变化」样式重做：渐变面积填充、连续平滑曲线（去数据点断口）、极简坐标（去虚线网格）、teal 主题配色；**图高减半**（viewBox 190→85），保留指标切换 + 悬停 tooltip

## v0.7.13 (2026-08-24)

- **交易趋势图优化（rant 2026-08-23T16:17:18，PR #133）** —
  - 美观度提升：平滑曲线、渐变面积填充、主题色系配色、自适应坐标轴刻度、悬停数据提示（tooltip）、精致图例
  - **指标可切换**：趋势图上方增加指标选择器（消费点数 / 收入点数 / 净变化 / Token 用量），默认只展示「消费点数」变化

## v0.7.12 (2026-08-23)

- **交易页 UI 改进（rant 2026-08-23T16:01:07）** —
  - 移除交易列表内 time 列的内部筛选（外部时间段筛选已覆盖，两套并存冗余且易混淆）
  - 列表上方新增**趋势图**（手写 SVG 折线图，无外部依赖）：新增 `GET /api/transactions/trend`，按时间桶（hour/day/week）聚合收入/支出点数，口径与 summary 一致；趋势图跟随 tab + 时间段筛选联动
  - **修复统计指标未联动表格内部筛选**：内部筛选（类型/模型/用户/Key 等）变化后，汇总条基于筛选后可见行本地加总（含 Token 总/输入/缓存/输出四指标），无筛选时仍用后端全量 SQL 聚合
  - **修复筛选输入框逐字符刷新**：`.th-filter` 的 input 事件由立即重建改为 300ms 防抖，刷新后恢复焦点并置光标到末尾（此前输入 "sh" 会因中间刷新变成 "hs"）

## v0.7.11 (2026-08-23)

- **P0 cache-billing fix (passthrough path)** — PR #128 (v0.7.10) only patched the cross-protocol SSE conversion path; the same-protocol passthrough path (`UsageCapture::finish()` in `src/gateway.rs`, used when client and upstream speak the same protocol, e.g. openai→openai) still passed the **full** `prompt_tokens` as input (including cache-hit tokens) and only recognized OpenAI's `prompt_tokens_details.cached_tokens` spelling — so DeepSeek responses double-counted cached tokens into the total (~2× tokens, per-call uncached showed 241,776 instead of 112). `finish()` now extracts cached via all three spellings (DeepSeek native `prompt_cache_hit_tokens` → OpenAI `prompt_tokens_details.cached_tokens` → Anthropic `cache_read_input_tokens`) and returns `(input − cached).max(0.0)` disjoint, matching the `sse.rs` logic; tests extended with cached cases for all three spellings (rant 2026-08-23T14:05:02, PR #130)

## v0.7.10 (2026-08-23)

- **Request body limit raised to 70MB** — axum's default 2MB body limit rejected long LLM contexts (~1M token) and large image-base64 payloads with 413; gateway now applies `RequestBodyLimitLayer::new(70 * 1024 * 1024)` (rant 2026-08-22T23:20:00)
- **P0 cache-billing fix** — DeepSeek's native top-level `prompt_cache_hit_tokens` was silently dropped (cached=0 → cache hits billed at full miss price, ~30x overcharge); all three spellings (DeepSeek `prompt_cache_hit_tokens` / OpenAI `prompt_tokens_details.cached_tokens` / Anthropic `cache_read_input_tokens`) are now extracted with DeepSeek priority, and all 6 `record_usage` sites disjoint input (`prompt_tokens − cached`) so cached tokens are never double-billed; downstream `input_tokens` forwarding is disjoint too (rant 2026-08-23T08:20:38)

## v0.7.9 (2026-08-22)

- **API Key 名称持久化** — `POST /api/api-keys` now stores the submitted name (was hardcoded empty); new `PATCH /api/api-keys/:id` renames a key (owner-only); settings-page rename now calls the API and reloads instead of faking it in memory (rant 2026-08-22T17:21:39)
- **Transaction table columns** — added a 用户 (user) column (JOIN users) and the api-key name column (api_keys.name via new `api_key_id` on transactions, migration v11 + settle writes); CSV export and i18n updated to match (rant 2026-08-22T17:21:39)

## v0.7.8 (2026-08-22)

- **Transaction-table filter row fix** — filter row no longer stretches to 236px (constrained to ~48px via fixed-height/th-top-aligned filter inputs); the four token columns (input/cached/output/tokens) drop their number-range filters, keeping sort and right-align (rants 2026-08-22T10:11:48/10:12:57)

## v0.7.7 (2026-08-22)

- **Transaction-page UE/UI** — token columns use K/M abbreviation with exact-value hover tooltip; summary bar values right-aligned; Key column shows a transaction-type label for non-keyed rows (topup/gift/withdraw) instead of a bare dash (rant 2026-08-22T08:58:54)
- **Docs refresh** — README zh/en language switcher links, "Powered by EMRG" section, and concise current-state docs (architecture / plan-api-matrix / user-stories rewritten to describe present + roadmap instead of history) (rants 2026-08-22T07:46:46/07:49:58/07:51:36)

## v0.7.6 (2026-08-22)

- **Docker publishing is tag-driven** — `docker-publish.yml` now builds GHCR images only on version tags (`v*`), plus `workflow_dispatch` manual trigger; `latest` follows the newest release tag (rant 2026-08-22T07:14:15)
- **Transaction table column overhaul** — 「模型 / Key」split into two readable columns (`model` + `key_label` from the `keys` table: note > provider/plan), and token usage split into four columns (input non-cache / input cache / output / total) (rants 2026-08-22T06:36:54/06:37:50/06:39:04)
- **Transaction summary fixes** — income whitelist (earn/topup/gift) positive, consume negative; daily gift now writes a `transactions` row; dashboard net/series treats topup as positive (rants 2026-08-22T00:04:21/00:07:08/06:34:37)
- **Dynamic user nickname** — sidebar chip + settings form show the real nickname (rant 2026-08-22T00:01:52)
- **SMTP send retry** — 3 attempts × 2s with fresh transports; 502 with a clear error when verification-code sending fails (rant 2026-08-21T23:52:17)

## v0.7.5 (2026-08-22)

- **New model** `deepseek-v4-flash-vision-exp` (provider=deepseek, same pricing as deepseek-v4-flash: 1.5/0.05/4.5, peak 3.0/0.1/9.0, context 1M, vision=true)

## v0.7.4 (2026-08-21)

- **Per-call token usage breakdown** — settle now writes the split usage (input / cache-hit / output) into `transactions` and `usage_records` (idempotent migration v10; input = total − cache − output, not double-stored)
- `GET /api/transactions` returns `input_tokens / cached_tokens / output_tokens` (older records default to 0)
- Transaction table shows a sub-line with the token breakdown (bilingual i18n, cache/output color-coded + tooltip)

## v0.7.3 (2026-08-20)

- **DeepSeek peak-hour pricing** — optional `peak_input_per_m / peak_output_per_m / peak_cache_hit_input_per_m` fields on the models table and config `[[models]]` (default 0 = peak pricing disabled)
- Peak hours judged in Beijing time (09:00–12:00, 14:00–18:00, fixed Asia/Shanghai, independent of server timezone)
- DeepSeek official peak prices written into the example config; market shows a "peak ×N" badge + detail panel; admin model form supports peak prices; migration v9

## v0.7.2 (2026-08-20)

- **Seed sync-delete** — `seed_models` now also deletes rows for models removed from config (config `[[models]]` fully authoritative; no more ghost market models)
- New test `seed_models_deletes_config_removed_rows`

## v0.7.1 (2026-08-20)

- **Simplified model config** — all model info (provider / price / context / vision…) defined in config.toml `[[models]]` (single source of truth), upserted into the DB on startup
- Removed `data/models.example.json` and the `price_overrides` double-layer mechanism; 10 models moved into config with DeepSeek official CNY prices; `data.js` visitor fallback prices aligned

## v0.7.0 (2026-08-20)

- **Separate billing for cache hits and misses** — usage parsing splits cached tokens (OpenAI `prompt_tokens_details.cached_tokens` / Anthropic `cache_read_input_tokens` / Responses `input_tokens_details.cached_tokens`)
- Billing = miss × input_per_m + hit × cache_hit_input_per_m (default 0 = free cache hits)
- `usage_records.cached_tokens` column (migration v8); DeepSeek official CNY pricing; admin model form gets the cache-hit input price; SSE (converted + passthrough) split the same way

## v0.6.7 (2026-08-19)

- **Logging system** — log4rs replaces env_logger: logs written to `<data-dir>/logs/aitokenpool.log` + stdout
- Size-based rolling (`[log].max_file_size`, default 10MB) with auto-pruning (`[log].max_backups`, default 7); `[log]` config section (dir / level / file_pattern)

## v0.6.6 (2026-08-19)

- **Unified data directory** — `ATP_DATA_DIR` (default `./data`; `--data-dir` > env > default) holds config.toml (auto-copied from the example on first start) + aitokenpool.db + logs/
- DB path always derived from the data dir (config `db_path` ignored); Docker single volume `./atp-data:/data`

## v0.6.5 (2026-08-19)

- **Timezone fix** — all backend JSON time fields normalized to UTC ISO with `Z`; frontend `timeAgo()` parses UTC, dashboard sparkline buckets by local day, absolute-time titles localized

## v0.6.4 (2026-08-19)

- **Admin model-info CRUD** — models table gains context_length / max_output / vision / cache_hit_input_per_m (migration v7); `GET|POST /api/admin/models` + `PATCH|DELETE /api/admin/models/:id` (admin-only, 409 on unique conflict, deleted models billed at 0)
- Management tab in the admin view (search / add / edit / delete); `GET /api/models` exposes the new fields

## v0.6.3 (2026-08-19)

- **Configurable public URL** — `[server].public_url` (default `http://localhost:8080`) + `GET /api/config`; the frontend builds gateway endpoints from it with a same-origin fallback

## v0.6.2 (2026-08-19)

- **Self-registration + email verification** — `POST /api/auth/register` / `verify` / `resend-code`; 6-digit code (10 min validity, 5 wrong attempts invalidate, 60 s resend rate limit); unverified emails can't log in (403)
- Registration form + verification page on the login page (bilingual); SMTP delivery via the `[mail]` config (dev mode prints the code to logs/response when unset)

## v0.6.1 (2026-08-19)

- **First-start initial admin** — an empty DB creates `admin@aitokenpool.local` with a random 16-character password printed to the startup log (first run only) + a zero-balance quota account; idempotent
- `POST /api/auth/change-password` endpoint (old-password check + argon2 update)

## v0.6.0 (2026-08-19)

- **Remove all demo seed data** — first deploy is a clean empty DB (schema only); no demo/admin/ops accounts, balances, or placeholder keys
- Tests use a `#[cfg(test)]`-only `seed_test_users` helper; UI login/settings pages drop the demo-account prefill and hints

## v0.5.2 (2026-08-19)

- **Bugfix: time-sensitive tests** — tests with hardcoded dates switched to SQLite dynamic dates (`datetime('now')` / `strftime('%Y-%m-%d 23:59:59', 'now')`); no more periodic failures when the day rolls over

## v0.5.1 (2026-08-18)

- **Docker publish** — GitHub Actions workflow builds and pushes the GHCR image `ghcr.io/argszero/aitokenpool` on `main` push / `v*` tag (buildx + gha cache)

## v0.5.0 (2026-08-18)

- **P3-B: streaming SSE cross-protocol conversion** — `stream:true` requests from any protocol (openai/anthropic/responses) forward to any protocol upstream with real-time event conversion (openai delta ↔ anthropic content_block_delta ↔ responses output_text.delta, incl. tool-call / thinking deltas and in-stream usage metering; responses→anthropic streaming deferred)

## v0.4.1 (2026-08-18)

- **P3-A follow-up: `GET /v1/models`** — OpenAI-compatible model list (optional auth; Bearer adds `available_keys`; `/models` alias)

## v0.4.0 (2026-08-18)

- **P3-A: three-protocol gateway conversion** — OpenAI Chat / OpenAI Responses / Anthropic: any endpoint can call plans exposing other protocols (auto-convert, zero-loss passthrough on the same protocol) + new `/v1/responses` endpoint

## v0.3.4 (2026-08-18)

- **Integration fixes** — `GET /api/plans` single source of truth (frontend form wired to the API), `data.js` PLANS aligned to the 12 plan ids, idempotent seed placeholder keys, master-key documentation

## v0.3.3 (2026-08-18)

- **P2-C: departments / raise requests / usage / operator** — departments CRUD + member re-assignment, raise-request approval flow, usage reports (users/models/departments aggregations), operator view (runtime/credits/users); schema v4

## v0.3.2 (2026-08-18)

- **P2-B: frontend wired to real APIs** — market / sharing / transactions / dashboard / API-key management all backed by the real backend; org/ops views keep mock placeholders

## v0.3.1 (2026-08-18)

- **P2-A: frontend integration** — backend statically serves `ui/`, API client layer, login/session integration, admin view gated by role; `GET /api/me`

## v0.3.0 (2026-08-18)

- **P1: point rules refined** — new-user daily gift (10-day window, valid same day), deduct earliest-expiring gift first then permanent, admin top-up API (role=admin); gift_balance/gift_grants migration v3

## v0.2.2 (2026-08-18)

- **P0-C: SSE streaming + key encryption + sharing APIs** — streaming with usage metered at the stream tail (no charge on disconnect), upstream keys AES-256-GCM encrypted, sharing / wallet / transaction APIs

## v0.2.1 (2026-08-18)

- **P0-B: gateway forwarding + failover + metering** — OpenAI Chat Completions + Anthropic Messages forwarding, sticky routing with health cooldown (3-switch cap), metering ledger (point calculation, 90/10 split, transactional settle)

## v0.2.0 (2026-08-18)

- **P0-A: backend skeleton** — axum server, TOML config (`Config::validate`: points_per_unit>0 / plan→provider exists / endpoints≥1 / protocol enum), SQLite data layer (idempotent migrations, production empty DB seeds nothing), auth (argon2 + Bearer API key), API-key endpoints
