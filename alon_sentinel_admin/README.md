# Alon Sentinel Admin

Standalone React admin for `alon_sentinel`.

## Development

1. Start Sentinel on `http://127.0.0.1:3000`
2. Provision an admin user with:

```bash
cargo run --bin provision_admin_user
```

3. Start the admin:

```bash
npm run dev
```

Create `alon_sentinel_admin/.env` with:

```env
VITE_SENTINEL_API_BASE_URL=http://127.0.0.1:3000
```

The UI uses that env value as the default Sentinel API base URL, stores manual base-URL overrides in browser local storage, and keeps the issued admin session in browser session storage so it clears with the browser session.

## Routing and Deployment

The admin UI uses hash-based routing (`react-router-dom` with `HashRouter`), so routes are represented as `/#/dashboard`, `/#/sites`, and `/#/access`.

That keeps browser refreshes working even when the UI is hosted as plain static files, because the server only needs to serve `index.html` for `/`.
