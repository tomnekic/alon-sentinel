# Sentinel HTTP API

This directory defines the public HTTP contract for Alon Sentinel.

- OpenAPI contract: [openapi-v1.yaml](openapi-v1.yaml)
- Current stable version: `v1`
- Base path: `/v1`

## Design Position

Sentinel is a single-tenant, self-hosted infrastructure service, not a framework-specific backend.

That means the API contract itself is the primary integration surface. Any UI, CLI, automation workflow, or external service should be able to use Sentinel without relying on Laravel, PHP, or any other project-specific adapter.

One Sentinel installation serves one owner and can monitor many sites. The API is scoped to that installation.

## Versioning Policy

Sentinel uses path-based versioning for its public API.

- Stable versioned endpoints live under `/v1`
- Breaking changes require a new versioned path such as `/v2`
- Additive changes may be shipped within the current version

Breaking changes include:

- removing an endpoint
- renaming an endpoint
- removing a documented field
- changing a documented field type
- changing the meaning of a documented field
- changing authentication requirements for an existing endpoint
- changing a documented success status code for an existing operation

Non-breaking changes within `v1` include:

- adding new endpoints
- adding new optional request fields
- adding new optional response fields
- adding new scopes
- improving error messages without changing the error envelope

## Authentication Model

Sentinel supports two bearer-token flows.

### API Clients

Use this flow for automation, CLIs, and service-to-service integrations.

1. Create or provision an API client in Sentinel.
2. Exchange `client_id` and `client_secret` at `POST /v1/auth/token`.
3. Send the resulting access token in the `Authorization` header:

```http
Authorization: Bearer <access_token>
```

These tokens are installation-scoped through the API client that issued them.

### Sentinel Admin Users

Use this flow for human operators and UI logins.

1. Create or provision an admin user in Sentinel.
2. Exchange `email` and `password` at `POST /v1/admin/auth/login`.
3. Send the resulting access token in the `Authorization` header.

Admin tokens carry the permissions implied by the user’s assigned roles.

All protected endpoints operate within the authenticated Sentinel installation.

## Authorization

### API Client Scopes

Current documented client scopes:

- `sites:read`
  Allows reads for sites, HTTP monitor configuration, and effective notification channel configuration.
- `sites:write`
  Allows writes for sites, HTTP monitor configuration, installation notification channels, and site notification overrides.

### Admin Permissions

Current admin-user permissions are role-derived and include keys such as:

- `sites.read`
- `sites.create`
- `sites.update`
- `sites.delete`
- `site_monitors.read`
- `site_monitors.create`
- `site_monitors.update`
- `site_monitors.delete`
- `site_checks.read`
- `site_incidents.read`
- `notification_channels.read`
- `notification_channels.create`
- `notification_channels.update`
- `notification_channels.delete`
- `site_notification_channel_overrides.read`
- `site_notification_channel_overrides.create`
- `site_notification_channel_overrides.update`
- `site_notification_channel_overrides.delete`
- `notification_deliveries.read`

If a token is valid but missing a required scope or permission, Sentinel returns `403 Forbidden`.

## Error Semantics

The stable error envelope for `v1` is:

```json
{
  "error": "human-readable message"
}
```

Current documented status code meanings:

- `400 Bad Request`
  Request validation failed or the request payload is incomplete.
- `401 Unauthorized`
  Client credentials are invalid, the bearer token is invalid, expired, revoked, or the authorization header is malformed or missing.
- `403 Forbidden`
  The token is valid but the client is not allowed to perform the operation, usually because a required scope is missing or the client is not bound to the Sentinel installation.
- `404 Not Found`
  The addressed resource does not exist within the authenticated installation scope.
- `500 Internal Server Error`
  Sentinel failed while processing the request.

Important `v1` rule:

- clients should rely on the HTTP status code and the top-level `error` field
- clients should not parse internal implementation details from the message text

## Serialization Rules

- All timestamps are RFC 3339 strings in UTC
- Enum values are serialized in `snake_case`
- IDs are numeric `int64` values
- Boolean flags are explicit booleans, not `0` or `1`

## Source of Truth

`openapi-v1.yaml` is the public contract for external clients.

When the implementation changes, the OpenAPI contract should be updated in the same change set if the public API surface changed.
