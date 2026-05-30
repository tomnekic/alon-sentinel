/**
 * Alon Sentinel — authenticated API benchmark.
 *
 * Mirrors the same ramp profile as kuma-api.js so results are directly
 * comparable. Exercises the two most expensive read endpoints:
 *   GET /v1/sites          — list with DB scan
 *   GET /v1/sites/1/summary — multi-join aggregate
 *
 * Run inside the benchmark network:
 *   docker run --rm -i --network benchmark_default \
 *     -e ALON_URL=http://alon-api:3000 \
 *     -e ALON_ADMIN_EMAIL=bench@bench.local \
 *     -e ALON_ADMIN_PASSWORD=Bench1234! \
 *     grafana/k6 run - < k6/alon-api.js
 */

import http from 'k6/http';
import { check, sleep } from 'k6';
import { Trend, Rate } from 'k6/metrics';

const BASE_URL       = __ENV.ALON_URL            || 'http://alon-api:3000';
const ADMIN_EMAIL    = __ENV.ALON_ADMIN_EMAIL    || 'bench@bench.local';
const ADMIN_PASSWORD = __ENV.ALON_ADMIN_PASSWORD || 'Bench1234!';

const listLatency    = new Trend('alon_list_sites_duration',    true);
const summaryLatency = new Trend('alon_site_summary_duration',  true);
const errorRate      = new Rate('alon_errors');

export const options = {
  summaryTrendStats: ['avg', 'min', 'med', 'max', 'p(90)', 'p(95)', 'p(99)'],
  scenarios: {
    ramp: {
      executor:  'ramping-vus',
      startVUs:  0,
      stages: [
        { duration: '30s',  target: 10  },
        { duration: '60s',  target: 25  },
        { duration: '30s',  target: 50  },
        { duration: '60s',  target: 50  },
        { duration: '15s',  target: 0   },
      ],
    },
  },
  thresholds: {
    http_req_failed:              ['rate<0.01'],
    alon_list_sites_duration:     ['p(95)<200'],
    alon_site_summary_duration:   ['p(95)<300'],
  },
};

export function setup() {
  const res = http.post(
    `${BASE_URL}/v1/admin/auth/login`,
    JSON.stringify({ email: ADMIN_EMAIL, password: ADMIN_PASSWORD }),
    { headers: { 'Content-Type': 'application/json' } },
  );

  if (res.status !== 200) {
    throw new Error(`Alon login failed: ${res.status} ${res.body}`);
  }

  const setCookie = res.headers['Set-Cookie'] || '';
  const match     = setCookie.match(/admin_session=([^;]+)/);
  if (!match) {
    throw new Error(`No admin_session cookie in response: ${setCookie}`);
  }

  return { cookie: match[1] };
}

export default function (data) {
  const headers = {
    Cookie:          `admin_session=${data.cookie}`,
    'Content-Type':  'application/json',
  };

  // ── List sites ────────────────────────────────────────────────────────────
  const list = http.get(`${BASE_URL}/v1/sites`, { headers, tags: { name: 'list_sites' } });
  listLatency.add(list.timings.duration);
  errorRate.add(list.status !== 200);
  check(list, { 'list sites 200': r => r.status === 200 });

  // ── Site summary (first site) ─────────────────────────────────────────────
  const summary = http.get(`${BASE_URL}/v1/sites/1/summary`, { headers, tags: { name: 'site_summary' } });
  summaryLatency.add(summary.timings.duration);
  errorRate.add(summary.status !== 200);
  check(summary, { 'site summary 200': r => r.status === 200 });

  sleep(0.1);
}
