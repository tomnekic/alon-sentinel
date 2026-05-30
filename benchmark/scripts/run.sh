#!/usr/bin/env bash
# Full Alon Sentinel vs Uptime Kuma benchmark.
#
# Usage:
#   ./scripts/run.sh              — full run (build + setup + measure + api bench)
#   ./scripts/run.sh --skip-build — reuse existing Docker images
#   ./scripts/run.sh --api-only   — skip worker measurement, run API bench only
#   ./scripts/run.sh --worker-only — skip API bench, measure worker only
#   ./scripts/run.sh --teardown   — stop and remove the benchmark stack
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT_DIR"

if [[ -f ".env" ]]; then
  set -a; source ".env"; set +a
fi

SKIP_BUILD=false
API_ONLY=false
WORKER_ONLY=false
TEARDOWN=false

for arg in "$@"; do
  case "$arg" in
    --skip-build)   SKIP_BUILD=true  ;;
    --api-only)     API_ONLY=true    ;;
    --worker-only)  WORKER_ONLY=true ;;
    --teardown)     TEARDOWN=true    ;;
  esac
done

MONITOR_COUNT="${MONITOR_COUNT:-50}"
CHECK_INTERVAL="${CHECK_INTERVAL:-60}"
MEASURE_DURATION="${MEASURE_DURATION:-300}"
ALON_URL="${ALON_URL:-http://localhost:3100}"
KUMA_URL="${KUMA_URL:-http://localhost:3101}"

COMPOSE_PROJECT="benchmark"
NETWORK="${COMPOSE_PROJECT}_default"

log()  { echo ""; echo "▶ $*"; }
hdr()  { echo ""; echo "════════════════════════════════════════════════════"; echo "  $*"; echo "════════════════════════════════════════════════════"; }
die()  { echo "ERROR: $*" >&2; exit 1; }

# ── Teardown ──────────────────────────────────────────────────────────────────
if $TEARDOWN; then
  log "Tearing down benchmark stack ..."
  docker compose -p "$COMPOSE_PROJECT" down -v
  exit 0
fi

mkdir -p results

# ── Start stack ───────────────────────────────────────────────────────────────
hdr "Starting benchmark stack"

if $SKIP_BUILD; then
  log "Skipping build (--skip-build)"
  docker compose -p "$COMPOSE_PROJECT" up -d --no-build
else
  log "Building images and starting services ..."
  docker compose -p "$COMPOSE_PROJECT" up -d --build
fi

log "Waiting for alon-init to complete ..."
for i in $(seq 1 60); do
  STATUS=$(docker inspect --format='{{.State.Status}}' "${COMPOSE_PROJECT}-alon-init-1" 2>/dev/null || echo "missing")
  [[ "$STATUS" == "exited" ]] && break
  [[ $i -eq 60 ]] && die "alon-init did not complete in time."
  sleep 3
done

# ── Seed both tools ───────────────────────────────────────────────────────────
hdr "Seeding Alon Sentinel"
bash "$SCRIPT_DIR/setup_alon.sh"

hdr "Seeding Uptime Kuma"
bash "$SCRIPT_DIR/setup_kuma.sh"

# ── Worker throughput measurement ─────────────────────────────────────────────
if ! $API_ONLY; then
  hdr "Worker throughput — waiting one check cycle before measuring ..."
  log "Sleeping ${CHECK_INTERVAL}s for workers to complete their first cycle ..."
  sleep "$CHECK_INTERVAL"

  log "Measuring for ${MEASURE_DURATION}s ..."
  bash "$SCRIPT_DIR/measure.sh"
fi

# ── API benchmark (k6) ────────────────────────────────────────────────────────
if ! $WORKER_ONLY; then
  ALON_COOKIE=$(cat results/alon_session.txt 2>/dev/null || true)
  KUMA_TOKEN=$(cat results/kuma_token.txt  2>/dev/null || true)

  hdr "API benchmark — Alon Sentinel"
  docker run --rm -i \
    --network "$NETWORK" \
    -e ALON_URL="http://alon-api:3000" \
    -e ALON_ADMIN_EMAIL="${ALON_ADMIN_EMAIL:-bench@bench.local}" \
    -e ALON_ADMIN_PASSWORD="${ALON_ADMIN_PASSWORD:-Bench1234!}" \
    grafana/k6 run \
      - < "$ROOT_DIR/k6/alon-api.js" \
    2>&1 | tee results/alon_k6.log || true

  # ── API benchmark note ────────────────────────────────────────────────────
  # Uptime Kuma 1.x has no REST management API — all monitor CRUD goes through
  # Socket.io.  Its only authenticated HTTP endpoint (GET /metrics) serves
  # in-memory Prometheus counters without touching the database.  Putting that
  # number next to Alon's DB-backed REST endpoints would be a false comparison,
  # so we report Alon's results only.
  #
  # To see Kuma's raw HTTP throughput independently, run:
  #   docker run --rm -i --network "$NETWORK" \
  #     -e KUMA_URL=http://uptime-kuma:3001 \
  #     -e KUMA_ADMIN_USER=bench \
  #     -e KUMA_ADMIN_PASSWORD=Bench1234! \
  #     grafana/k6 run - < "$ROOT_DIR/k6/kuma-api.js"

  hdr "API Performance — Alon Sentinel"

  ALON_RPS=$(awk '/http_reqs[.]*:/ { for (i=1; i<=NF; i++) if ($i ~ /\/s$/) { sub("/s", "", $i); print $i } }' results/alon_k6.log 2>/dev/null | tail -1)
  ALON_P95=$(grep "http_req_duration" results/alon_k6.log 2>/dev/null | sed -n 's/.*p(95)=\([^ ,}]*\).*/\1/p' | head -1)
  ALON_P99=$(grep "http_req_duration" results/alon_k6.log 2>/dev/null | sed -n 's/.*p(99)=\([^ ,}]*\).*/\1/p' | head -1)
  ALON_ERR=$(awk '/http_req_failed[.]*:/ { print $2 }' results/alon_k6.log 2>/dev/null | tail -1)

  ALON_RPS="${ALON_RPS:-N/A}"
  ALON_P95="${ALON_P95:-N/A}"
  ALON_P99="${ALON_P99:-N/A}"
  ALON_ERR="${ALON_ERR:-N/A}"

  {
    echo ""
    printf "  %-28s  %-18s\n" "Metric" "Alon Sentinel"
    printf "  %-28s  %-18s\n" "----------------------------" "------------------"
    printf "  %-28s  %-18s\n" "Req / sec"   "$ALON_RPS"
    printf "  %-28s  %-18s\n" "p95 latency" "$ALON_P95"
    printf "  %-28s  %-18s\n" "p99 latency" "$ALON_P99"
    printf "  %-28s  %-18s\n" "Error rate"  "$ALON_ERR"
    echo ""
    echo "  Endpoints: GET /v1/sites (list) + GET /v1/sites/1/summary (aggregate)"
    echo "  Kuma 1.x: no REST API — no comparable measurement available"
    echo "  Full k6 log: results/alon_k6.log"
    echo ""
  } | tee -a results/api_summary.txt
fi

hdr "Benchmark complete"
echo "  All results in: $ROOT_DIR/results/"
echo ""
echo "  To tear down the stack:  ./scripts/run.sh --teardown"
echo ""
