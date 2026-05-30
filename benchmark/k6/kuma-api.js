/**
 * Uptime Kuma — standalone HTTP endpoint benchmark.
 *
 * Kuma 1.x has no REST management API; all monitor CRUD goes via Socket.io.
 * The only available authenticated HTTP endpoint is:
 *   GET /metrics   — Prometheus text export (HTTP Basic Auth)
 *
 * IMPORTANT: this endpoint is served from in-memory Prometheus counters and
 * does NOT hit the database.  It is not equivalent to Alon's REST management
 * API (which issues real PostgreSQL queries).  Do NOT put these numbers in a
 * side-by-side comparison table with alon-api.js results.
 *
 * Run standalone:
 *   docker run --rm -i --network benchmark_default \
 *     -e KUMA_URL=http://uptime-kuma:3001 \
 *     -e KUMA_ADMIN_USER=bench \
 *     -e KUMA_ADMIN_PASSWORD=Bench1234! \
 *     grafana/k6 run - < k6/kuma-api.js
 */

import http from 'k6/http';
import { check, sleep } from 'k6';
import { Trend, Rate } from 'k6/metrics';
import encoding from 'k6/encoding';

const BASE_URL       = __ENV.KUMA_URL            || 'http://uptime-kuma:3001';
const ADMIN_USER     = __ENV.KUMA_ADMIN_USER     || 'bench';
const ADMIN_PASSWORD = __ENV.KUMA_ADMIN_PASSWORD || 'Bench1234!';

const metricsLatency = new Trend('kuma_metrics_duration', true);
const errorRate      = new Rate('kuma_errors');

export const options = {
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
    http_req_failed:       ['rate<0.01'],
    kuma_metrics_duration: ['p(95)<500'],
  },
};

export function setup() {
  const credentials = `${ADMIN_USER}:${ADMIN_PASSWORD}`;
  const encoded = encoding.b64encode(credentials);
  return { auth: `Basic ${encoded}` };
}

export default function (data) {
  const headers = { Authorization: data.auth };

  const res = http.get(`${BASE_URL}/metrics`, { headers, tags: { name: 'metrics' } });
  metricsLatency.add(res.timings.duration);
  errorRate.add(res.status !== 200);
  check(res, { 'metrics 200': r => r.status === 200 });

  sleep(0.1);
}
