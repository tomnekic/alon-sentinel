/**
 * Alon Sentinel — standalone authenticated API benchmark.
 *
 * Runs against a locally running Alon instance (default: host.docker.internal:3000).
 * Use this script from within the alon_sentinel/benchmark/ docker-compose context
 * when you want to benchmark Alon in isolation (not the side-by-side comparison).
 *
 * Scenarios:
 *   list-sites    — GET /v1/sites  (constant 25 VUs, 60s)
 *   site-summary  — GET /v1/sites/1/summary  (constant 25 VUs, 60s)
 *   ramp          — mixed ramp 0→50→0 VUs across 195s
 *
 * Usage:
 *   docker run --rm -i \
 *     -e ALON_URL=http://host.docker.internal:3000 \
 *     -e ALON_ADMIN_EMAIL=bench@bench.local \
 *     -e ALON_ADMIN_PASSWORD=Bench1234! \
 *     grafana/k6 run - < k6/api-authenticated.js
 *
 *   # Run only the ramp scenario:
 *     ... grafana/k6 run --env SCENARIO=ramp - < k6/api-authenticated.js
 */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Trend, Rate, Counter } from 'k6/metrics';

const BASE_URL       = __ENV.ALON_URL            || 'http://host.docker.internal:3000';
const ADMIN_EMAIL    = __ENV.ALON_ADMIN_EMAIL    || 'bench@bench.local';
const ADMIN_PASSWORD = __ENV.ALON_ADMIN_PASSWORD || 'Bench1234!';
const SCENARIO       = __ENV.SCENARIO            || 'ramp';

const listDuration    = new Trend('list_sites_ms',    true);
const summaryDuration = new Trend('site_summary_ms',  true);
const errors          = new Rate('error_rate');
const requests        = new Counter('total_requests');

const SCENARIOS = {
  'list-sites': {
    executor:  'constant-vus',
    vus:       25,
    duration:  '60s',
    exec:      'listSites',
  },
  'site-summary': {
    executor:  'constant-vus',
    vus:       25,
    duration:  '60s',
    exec:      'siteSummary',
  },
  'ramp': {
    executor:  'ramping-vus',
    startVUs:  0,
    stages: [
      { duration: '30s',  target: 10  },
      { duration: '60s',  target: 25  },
      { duration: '30s',  target: 50  },
      { duration: '60s',  target: 50  },
      { duration: '15s',  target: 0   },
    ],
    exec: 'mixed',
  },
};

export const options = {
  scenarios: SCENARIO === 'all'
    ? SCENARIOS
    : { [SCENARIO]: SCENARIOS[SCENARIO] || SCENARIOS['ramp'] },
  thresholds: {
    error_rate:       ['rate<0.01'],
    list_sites_ms:    ['p(95)<200', 'p(99)<400'],
    site_summary_ms:  ['p(95)<300', 'p(99)<500'],
  },
};

// ── Auth setup ────────────────────────────────────────────────────────────────
export function setup() {
  const res = http.post(
    `${BASE_URL}/v1/admin/auth/login`,
    JSON.stringify({ email: ADMIN_EMAIL, password: ADMIN_PASSWORD }),
    { headers: { 'Content-Type': 'application/json' } },
  );

  if (res.status !== 200) {
    throw new Error(`Login failed: HTTP ${res.status} — ${res.body}`);
  }

  const setCookie = res.headers['Set-Cookie'] || '';
  const match     = setCookie.match(/admin_session=([^;]+)/);
  if (!match) {
    throw new Error(`No admin_session cookie in: ${setCookie}`);
  }

  return { cookie: match[1] };
}

// ── Scenario: list sites ──────────────────────────────────────────────────────
export function listSites(data) {
  const res = http.get(`${BASE_URL}/v1/sites`, {
    headers: { Cookie: `admin_session=${data.cookie}` },
    tags:    { endpoint: 'list_sites' },
  });
  listDuration.add(res.timings.duration);
  errors.add(res.status !== 200);
  requests.add(1);
  check(res, { '200 OK': r => r.status === 200 });
}

// ── Scenario: site summary ────────────────────────────────────────────────────
export function siteSummary(data) {
  const res = http.get(`${BASE_URL}/v1/sites/1/summary`, {
    headers: { Cookie: `admin_session=${data.cookie}` },
    tags:    { endpoint: 'site_summary' },
  });
  summaryDuration.add(res.timings.duration);
  errors.add(res.status !== 200);
  requests.add(1);
  check(res, { '200 OK': r => r.status === 200 });
}

// ── Scenario: mixed (used by ramp) ────────────────────────────────────────────
export function mixed(data) {
  const headers = { Cookie: `admin_session=${data.cookie}` };

  group('list sites', () => {
    const res = http.get(`${BASE_URL}/v1/sites`, { headers, tags: { endpoint: 'list_sites' } });
    listDuration.add(res.timings.duration);
    errors.add(res.status !== 200);
    requests.add(1);
    check(res, { '200 OK': r => r.status === 200 });
  });

  group('site summary', () => {
    const res = http.get(`${BASE_URL}/v1/sites/1/summary`, { headers, tags: { endpoint: 'site_summary' } });
    summaryDuration.add(res.timings.duration);
    errors.add(res.status !== 200);
    requests.add(1);
    check(res, { '200 OK': r => r.status === 200 });
  });

  sleep(0.1);
}

// default export required even though scenarios use named functions
export default function (data) { mixed(data); }
