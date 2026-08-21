# AITokenPool — Shared AI Token Pool

> **Don't let your token plan go to waste.**
> Subscribed to Claude / ChatGPT / GLM / DeepSeek and can't use it all? Share your quota to earn points — and spend them on models from others when you need to.

AITokenPool is an open-source **AI token sharing platform / multi-model gateway** that pools API quota from multiple model providers behind a single entry point with fair per-point allocation. The enterprise edition (internal key pool + per-employee point quotas) and the public edition (users share idle keys to earn points and spend points on others' models) share the same core platform.

## Features

- **Multi-protocol gateway** — bidirectional conversion between OpenAI Chat / OpenAI Responses / Anthropic Messages, for both non-streaming and SSE streaming requests; clients only need one OpenAI-compatible endpoint
- **Transparent billing** — every call shows an input / cache-hit / output token breakdown; built-in DeepSeek official CNY pricing with peak-hour pricing support
- **Point system** — daily gift, 90/10 sharing split, admin top-up, self-registration with email verification
- **Enterprise controls** — departments, member credits, usage reports, and a dedicated operator view
- **Security** — upstream keys encrypted with AES-256-GCM, argon2 password hashing, email verification, admin role isolation
- **Lightweight** — a single Rust binary + SQLite + one-command Docker deploy, no external services
- **Bilingual web UI included** — full dashboard (market / wallet / transactions / sharing / admin), no separate frontend deployment

## Use Cases

| Scenario | Description |
|----------|-------------|
| Enterprises / teams | Buy multiple model plans centrally into a key pool and allocate points to employees — one entry point for all models, controlled cost |
| Individual subscribers | Share idle subscription quota to earn points, then spend them on other models — subscriptions never go to waste |
| Developers / tinkerers | One OpenAI-compatible API to call every model, with dashboard, reports, and permission system out of the box |

## Quick Start

### Docker (recommended)

```bash
docker compose up -d --build
# or use the published image
docker pull ghcr.io/argszero/aitokenpool:latest
docker run -p 8080:8080 -v "$PWD/atp-data:/data" \
  -e ATP_MASTER_KEY=$(openssl rand -hex 32) \
  ghcr.io/argszero/aitokenpool:latest
```

### From source

```bash
cargo run   # creates ./data/ and config on first start; open http://localhost:8080/
```

### First start

- A unified data directory is created automatically (`<data>/`: `config.toml` + `aitokenpool.db` + `logs/`)
- An **initial admin** `admin@aitokenpool.local` is created with a random 16-character password printed to the startup log (first run only) — change it right after login
- Regular users can **self-register** from the login page (email verification code; unverified emails can't log in)
- ⚠️ **Set the master key `ATP_MASTER_KEY` in production** (`openssl rand -hex 32`) — it encrypts upstream API keys; without it, previously listed keys can't be decrypted after a restart

See comments in `config/config.example.toml` for the full configuration.

### Gateway access

OpenAI-compatible endpoints: `POST /v1/chat/completions`, `POST /v1/responses`, `POST /anthropic/v1/messages`, `GET /v1/models`. The base URL is shown on the settings page and built from `[server].public_url` (set it to your real domain in production). Full API reference: [docs/architecture.md](docs/architecture.md).

## Documentation

| Doc | Description |
|-----|-------------|
| [docs/architecture.md](docs/architecture.md) | Architecture, API reference, database schema |
| [docs/user-stories.md](docs/user-stories.md) | User stories & scenarios |
| [docs/plan-api-matrix.md](docs/plan-api-matrix.md) | Plans & API matrix |
| [CHANGELOG.md](CHANGELOG.md) | Version history |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Contribution guide |

## License

[MIT](LICENSE)
