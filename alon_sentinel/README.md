# Alon Sentinel

Alon Sentinel is a single-tenant, self-hosted, API-first monitoring service written in Rust.

One Sentinel installation serves one owner and can monitor many sites.

The Rust service is the product boundary. It owns:

- client-credentials authentication and bearer token issuance
- Sentinel admin user authentication and role-based permissions
- installation-scoped site management
- HTTP, SSL, TCP, DNS, and Heartbeat monitor configuration
- background execution of site checks
- incident lifecycle management (open on failure, resolve on recovery)
- public status page serving
- notification channel configuration and delivery
- Prometheus metrics endpoint

Client applications are expected to integrate with Sentinel over HTTP. A UI can be built in any stack as long as it follows the public API contract.

## Public API

The stable public HTTP API is versioned under `/v1`.

- Contract overview: [docs/api/README.md](docs/api/README.md)
- Machine-readable contract: [docs/api/openapi-v1.yaml](docs/api/openapi-v1.yaml)

The `/v1` contract documents:

- authentication flow
- supported scopes
- request and response schemas
- status code and error semantics
- versioning and compatibility rules

## Local Setup

### Prerequisites

- Rust stable (2024 edition) — install via [rustup](https://rustup.rs)
- PostgreSQL 14+

### 1. Create a database

```sql
CREATE DATABASE alon_sentinel_db;
```

### 2. Configure the environment

```bash
cp .env.example .env
```

Open `.env` and set the two required values:

```env
DATABASE_URL=postgresql://user:password@localhost/alon_sentinel_db

# 64 hex characters — generate with: openssl rand -hex 32
WEBHOOK_SECRET_ENCRYPTION_KEY=your_64_hex_chars_here
```

Everything else has a working default. See `.env.example` for the full reference.

### 3. Run database migrations

```bash
cargo run --bin migrate
```

This applies all pending migrations and seeds the built-in roles (`viewer`, `operator`,
`admin`) and their permission sets.

### 4. Provision an admin user

```bash
cargo run --bin provision_admin_user
```

Creates or updates the admin user and prints the credentials for
`POST /v1/admin/auth/login`. The command uses these environment variables (all optional —
defaults are shown):

| Variable | Default |
|---|---|
| `SEED_ADMIN_EMAIL` | `admin@localhost` |
| `SEED_ADMIN_PASSWORD` | `change-me-now` |
| `SEED_ADMIN_NAME` | `Sentinel Admin` |
| `SEED_ADMIN_ROLE` | `admin` |

Set them in `.env` or inline to override:

```bash
SEED_ADMIN_EMAIL=you@example.com SEED_ADMIN_PASSWORD=strongpassword cargo run --bin provision_admin_user
```

### 5. Provision an API client (optional)

Skip this step if you only need the admin UI. For programmatic or service-to-service
access, provision an API client:

```bash
cargo run --bin provision_client
```

Prints the `client_id` and `client_secret` needed for `POST /v1/auth/token`. Defaults:

| Variable | Default |
|---|---|
| `SEED_CLIENT_ID` | `sentinel-client` |
| `SEED_CLIENT_SECRET` | `sentinel-local-client-secret` |
| `SEED_CLIENT_NAME` | `Sentinel API Client` |

### 6. Start the processes

Start the API server and the worker (each in its own terminal — see
[Runtime Entry Points](#runtime-entry-points) below for all options):

```bash
cargo run --bin api
```

```bash
cargo run --bin worker
```

## Runtime Entry Points

Run the HTTP API with:

```bash
cargo run
```

or explicitly:

```bash
cargo run --bin api
```

When Sentinel is deployed behind a trusted reverse proxy, build and run the API with the
`trusted-proxy` feature so auth audit IPs are taken from the proxy-appended
`X-Forwarded-For` hop:

```bash
cargo run --bin api --features trusted-proxy
```

Run background site checks and notification delivery workers with:

```bash
cargo run --bin worker
```

Workers also prune historical `site_monitor_checks` rows in the background. The retention
window and sweep behavior are configurable with:
`SITE_MONITOR_CHECK_RETENTION_DAYS`,
`SITE_MONITOR_CHECK_RETENTION_INTERVAL_SECONDS`, and
`SITE_MONITOR_CHECK_RETENTION_BATCH_SIZE`.

API and worker processes already run with separate `sqlx` pools. To reserve database
capacity for checks under API load, size them independently with
`API_DB_MAX_CONNECTIONS` / `API_DB_MIN_CONNECTIONS` and
`WORKER_DB_MAX_CONNECTIONS` / `WORKER_DB_MIN_CONNECTIONS`.
If you do not set them, both services fall back to the shared `DB_MAX_CONNECTIONS` and
`DB_MIN_CONNECTIONS` defaults.

Auth endpoints are also protected by an in-process per-IP rate limiter. Tune it with
`AUTH_RATE_LIMIT_MAX_REQUESTS` and `AUTH_RATE_LIMIT_WINDOW_SECONDS`.

## Contract Stability

`/v1` is the current stable API line.

Within `v1`, Sentinel may add:

- new endpoints
- new optional response fields
- new optional request fields
- new scopes

Within `v1`, Sentinel will not make breaking changes such as:

- removing an existing endpoint
- removing a documented field
- changing the type or meaning of a documented field
- changing authentication requirements for an existing endpoint
- changing a successful response code for an existing operation

Breaking changes require a new versioned path such as `/v2`.
