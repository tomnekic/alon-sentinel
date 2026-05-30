# Changelog

All notable changes to Alon Sentinel are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
Alon Sentinel follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The `/v1` API surface carries documented stability guarantees: additive changes
ship within `v1`; breaking changes require a new major version (`/v2`).

---

## [1.0.0] — 2026-05-23

Initial public release.

### Added

**Monitor types**
- HTTP — status code, response body, JSON path assertions, header assertions, response time threshold, and SSL expiry checks
- SSL — certificate validity and days-until-expiry threshold
- DNS — record resolution and expected-value matching
- TCP — port reachability
- Heartbeat — passive ping endpoint for cron jobs and background services

**Site-centric model**
- Sites group monitors around the service they protect
- Per-site operational timeline across 24 h, 7 d, 30 d, and 90 d windows
- Shared incident history, notifications, and public status output per site

**Incident lifecycle**
- Failures open incidents automatically; recoveries resolve them
- Configurable check history retention

**Public status pages**
- Per-site public pages reflecting current health and open incidents
- Degraded and outage states surfaced to end users or customers

**Notifications**
- Slack, Discord, webhook, and email channels
- Per-site channel overrides for routing alerts independently

**API and authentication**
- Versioned REST API under `/v1` with OpenAPI specification
- Machine-to-machine API clients with scoped bearer tokens (`sites:read`, `sites:write`)
- Role-based admin users: viewer, operator, admin
- In-process per-IP rate limiting on authentication endpoints

**Deployment**
- Docker Compose stack: PostgreSQL, API server, background worker, React admin UI behind nginx
- Trusted-proxy mode for `X-Forwarded-For` support behind a reverse proxy
- Lease-based worker coordination — no external queue required

---

[1.0.0]: https://github.com/tomnekic/alon-sentinel/releases/tag/v1.0.0
