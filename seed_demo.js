#!/usr/bin/env node
// Alon Sentinel — demo data seed
// Usage: node seed_demo.js
//   SEED_ADMIN_EMAIL=admin@localhost SEED_ADMIN_PASSWORD=change-me-now node seed_demo.js

const API      = process.env.SENTINEL_API_URL  || "http://localhost:3000";
const EMAIL    = process.env.SEED_ADMIN_EMAIL   || "admin@localhost";
const PASSWORD = process.env.SEED_ADMIN_PASSWORD || "change-me-now";

let sessionCookie = null;

// ── HTTP helpers ──────────────────────────────────────────────────────────────

async function request(method, path, body) {
  const headers = { "Content-Type": "application/json", Accept: "application/json" };
  if (sessionCookie) headers["Cookie"] = sessionCookie;

  const resp = await fetch(`${API}${path}`, {
    method,
    headers,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });

  const setCookie = resp.headers.get("set-cookie");
  if (setCookie) {
    const m = setCookie.match(/admin_session=([^;]+)/);
    if (m) sessionCookie = `admin_session=${m[1]}`;
  }

  if (!resp.ok) {
    const text = await resp.text().catch(() => "");
    let msg = text;
    try { msg = JSON.parse(text).error || text; } catch { /* ok */ }
    throw new Error(`${method} ${path} → HTTP ${resp.status}: ${msg}`);
  }

  return resp.json().catch(() => null);
}

const get  = (path)       => request("GET",  path);
const post = (path, body) => request("POST", path, body);
const put  = (path, body) => request("PUT",  path, body);

// ── Domain helpers ────────────────────────────────────────────────────────────

async function createSite(name, baseUrl) {
  const site = await post("/v1/sites", { name, base_url: baseUrl });
  return site.id;
}

function http(siteId, opts) {
  return put(`/v1/sites/${siteId}/monitoring/http`, {
    is_active: true,
    check_interval_seconds: 60,
    expected_status_code: 200,
    ...opts,
  });
}

function ssl(siteId, targetUrl, opts = {}) {
  return put(`/v1/sites/${siteId}/monitoring/ssl`, {
    target_url: targetUrl,
    check_interval_seconds: 300,
    ssl_expiry_warning_days: 14,
    is_active: true,
    ...opts,
  });
}

function tcp(siteId, host, port, opts = {}) {
  return put(`/v1/sites/${siteId}/monitoring/tcp`, {
    target_host: host,
    target_port: port,
    check_interval_seconds: 60,
    is_active: true,
    ...opts,
  });
}

function dns(siteId, hostname, recordType, opts = {}) {
  return put(`/v1/sites/${siteId}/monitoring/dns`, {
    hostname,
    record_type: recordType,
    check_interval_seconds: 120,
    is_active: true,
    ...opts,
  });
}

function heartbeat(siteId, opts = {}) {
  return put(`/v1/sites/${siteId}/monitoring/heartbeat`, {
    check_interval_seconds: 60,
    heartbeat_grace_seconds: 30,
    is_active: true,
    ...opts,
  });
}

// ── Site catalogue ────────────────────────────────────────────────────────────
//
// Mix of public services that should stay up + services purpose-built for
// monitoring/testing, to show all monitor types in the screenshots.

const SITES = [
  {
    name: "GitHub",
    url: "https://github.com",
    note: "HTTP + SSL",
    async setup(id) {
      await http(id, {
        target_url: "https://github.com",
        check_interval_seconds: 60,
        max_response_time_ms: 3000,
      });
      await ssl(id, "https://github.com", { ssl_expiry_warning_days: 21 });
    },
  },
  {
    name: "Cloudflare",
    url: "https://www.cloudflare.com",
    note: "HTTP + DNS",
    async setup(id) {
      await http(id, {
        target_url: "https://www.cloudflare.com",
        check_interval_seconds: 60,
      });
      await dns(id, "cloudflare.com", "A", { check_interval_seconds: 120 });
    },
  },
  {
    name: "Google",
    url: "https://www.google.com",
    note: "HTTP + TCP",
    async setup(id) {
      await http(id, {
        target_url: "https://www.google.com",
        check_interval_seconds: 60,
        max_response_time_ms: 2000,
      });
      await tcp(id, "google.com", 443, { check_interval_seconds: 60 });
    },
  },
  {
    name: "Hacker News",
    url: "https://news.ycombinator.com",
    note: "HTTP",
    async setup(id) {
      await http(id, {
        target_url: "https://news.ycombinator.com",
        check_interval_seconds: 60,
      });
    },
  },
  {
    name: "GitHub API",
    url: "https://api.github.com",
    note: "HTTP (JSON path) + SSL",
    async setup(id) {
      // Verify the root API returns expected structure
      await http(id, {
        target_url: "https://api.github.com",
        check_interval_seconds: 120,
        json_path_exists: ["$.documentation_url", "$.current_user_url"],
      });
      await ssl(id, "https://api.github.com", { check_interval_seconds: 300 });
    },
  },
  {
    name: "JSONPlaceholder",
    url: "https://jsonplaceholder.typicode.com",
    note: "HTTP (JSON path assertions) — fake REST API for testing",
    async setup(id) {
      await http(id, {
        target_url: "https://jsonplaceholder.typicode.com/posts/1",
        check_interval_seconds: 120,
        json_path_exists: ["$.userId", "$.id", "$.title"],
        json_path_equals: [{ path: "$.id", value: "1" }],
      });
    },
  },
  {
    name: "httpbin",
    url: "https://httpbin.org",
    note: "HTTP (JSON path + response time) — purpose-built HTTP testing service",
    async setup(id) {
      await http(id, {
        target_url: "https://httpbin.org/get",
        check_interval_seconds: 120,
        max_response_time_ms: 6000,
        json_path_equals: [{ path: "$.url", value: '"https://httpbin.org/get"' }],
      });
    },
  },
  {
    name: "Cloudflare DNS (1.1.1.1)",
    url: "https://1.1.1.1",
    note: "HTTP + TCP + DNS — purpose-built public DNS resolver",
    async setup(id) {
      await http(id, {
        target_url: "https://1.1.1.1",
        check_interval_seconds: 60,
        max_response_time_ms: 1500,
      });
      await tcp(id, "1.1.1.1", 53, {
        check_interval_seconds: 60,
        max_connect_time_ms: 500,
      });
      await dns(id, "cloudflare.com", "A", {
        check_interval_seconds: 60,
        expected_value: "104.16.132.229",
      });
    },
  },
  {
    name: "My App",
    url: "https://myapp.example.com",
    note: "Heartbeat — simulates a self-hosted service that pings in",
    async setup(id) {
      await heartbeat(id, {
        check_interval_seconds: 60,
        heartbeat_grace_seconds: 30,
      });
    },
  },
  {
    name: "My App (Staging)",
    url: "https://staging.myapp.example.com",
    note: "No monitor — demonstrates coverage gap",
    async setup() {
      // Intentionally left unconfigured.
    },
  },
];

// ── Main ──────────────────────────────────────────────────────────────────────

async function main() {
  console.log("\n  ┌─────────────────────────────────────────┐");
  console.log("  │   Alon Sentinel — Demo Data Seed        │");
  console.log("  └─────────────────────────────────────────┘\n");
  console.log(`  API  : ${API}`);
  console.log(`  User : ${EMAIL}\n`);

  // Guard: skip if sites already exist
  const loginCheck = await post("/v1/admin/auth/login", { email: EMAIL, password: PASSWORD });
  console.log(`  Signed in as ${loginCheck.user.display_name}\n`);

  const existing = await get("/v1/sites");
  const existingCount = existing?.totalCount ?? (Array.isArray(existing?.items) ? existing.items.length : 0);
  if (existingCount > 0) {
    console.log(`  ⚠  ${existingCount} site(s) already exist — aborting to avoid duplicates.`);
    console.log("     Drop and recreate the database volume to start fresh:\n");
    console.log("       docker compose down -v && docker compose up -d\n");
    process.exit(0);
  }

  for (let i = 0; i < SITES.length; i++) {
    const { name, url, note, setup } = SITES[i];
    process.stdout.write(`  [${String(i + 1).padStart(2)}/${SITES.length}] ${name.padEnd(28)} ${note}\n`);
    const id = await createSite(name, url);
    await setup(id);
  }

  console.log(`\n  ✓ Seeding complete — ${SITES.length} sites created.\n`);
  console.log("  Open  http://localhost:8080  to explore the dashboard.");
  console.log("  Wait ~30 seconds for the worker to run the first checks.\n");
}

main().catch((err) => {
  console.error("\n  ✗ Seed failed:", err.message);
  process.exit(1);
});
