# Changelog

All notable changes are recorded here. Versions follow [SemVer](https://semver.org/).

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
