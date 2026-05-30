#!/usr/bin/env bash
# Provision Alon Sentinel and seed MONITOR_COUNT HTTP monitors pointing at
# the target service.  Expects the benchmark stack to be running.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# ── Load .env if present ──────────────────────────────────────────────────────
if [[ -f "$ROOT_DIR/.env" ]]; then
  set -a; source "$ROOT_DIR/.env"; set +a
fi

ALON_URL="${ALON_URL:-http://localhost:3100}"
ADMIN_EMAIL="${ALON_ADMIN_EMAIL:-bench@bench.local}"
ADMIN_PASSWORD="${ALON_ADMIN_PASSWORD:-Bench1234!}"
MONITOR_COUNT="${MONITOR_COUNT:-50}"
CHECK_INTERVAL="${CHECK_INTERVAL:-60}"
TARGET_URL="http://target:8088/ok"

DB_URL="${DB_PASSWORD:-benchpw}"
COMPOSE_PROJECT="benchmark"

log() { echo "[alon-setup] $*"; }

# ── Wait for API ──────────────────────────────────────────────────────────────
log "Waiting for Alon API at $ALON_URL ..."
for i in $(seq 1 30); do
  if curl -sf "$ALON_URL/health" -o /dev/null 2>/dev/null; then
    log "API is up."
    break
  fi
  [[ $i -eq 30 ]] && { log "ERROR: Alon API did not become ready."; exit 1; }
  sleep 3
done

# ── Provision admin user ──────────────────────────────────────────────────────
log "Provisioning admin user ($ADMIN_EMAIL) ..."
docker exec \
  -e DATABASE_URL="postgresql://alon:${DB_URL}@postgres/alon_bench_db" \
  -e SEED_ADMIN_EMAIL="$ADMIN_EMAIL" \
  -e SEED_ADMIN_PASSWORD="$ADMIN_PASSWORD" \
  "${COMPOSE_PROJECT}-alon-api-1" /usr/local/bin/provision_admin_user 2>&1 \
  | grep -E "completed|email:|error" || true

# ── Login ─────────────────────────────────────────────────────────────────────
log "Logging in ..."
LOGIN_RESP=$(curl -s -D - -X POST "$ALON_URL/v1/admin/auth/login" \
  -H "Content-Type: application/json" \
  -d "{\"email\":\"$ADMIN_EMAIL\",\"password\":\"$ADMIN_PASSWORD\"}" \
  2>/dev/null)

HTTP_CODE=$(echo "$LOGIN_RESP" | head -1 | grep -o '[0-9][0-9][0-9]' | head -1)
if [[ "$HTTP_CODE" != "200" ]]; then
  log "ERROR: Login returned HTTP $HTTP_CODE"; echo "$LOGIN_RESP"; exit 1
fi

COOKIE=$(echo "$LOGIN_RESP" | grep -i "set-cookie:" | sed -n 's/.*admin_session=\([^;]*\).*/\1/p')
if [[ -z "$COOKIE" ]]; then
  echo "$LOGIN_RESP"
  log "ERROR: Could not extract session cookie."; exit 1
fi
log "Session cookie obtained."

CURL_AUTH=(-b "admin_session=$COOKIE" -H "Content-Type: application/json")

# ── Seed sites + HTTP monitors ────────────────────────────────────────────────
log "Seeding $MONITOR_COUNT sites with HTTP monitors (interval: ${CHECK_INTERVAL}s) ..."
ok=0; skipped=0
for i in $(seq 1 "$MONITOR_COUNT"); do
  # Create site (unique base_url per site; target_url on the monitor is what gets polled)
  SITE_RESP=$(curl -s -X POST "$ALON_URL/v1/sites" \
    "${CURL_AUTH[@]}" \
    -d "{\"name\":\"Bench Site $i\",\"base_url\":\"http://bench-${i}.example.com\"}" 2>/dev/null || true)

  SITE_RAW=$(echo "$SITE_RESP" | grep -o '"id":[0-9]*' || true)
  SITE_ID=$(echo "$SITE_RAW" | head -1 | tr -d '"id:')
  if [[ -z "$SITE_ID" ]]; then
    log "WARNING: Could not parse site ID for site $i — resp: ${SITE_RESP:0:80}"
    skipped=$((skipped + 1))
    continue
  fi

  # Attach HTTP monitor
  MON_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X PUT "$ALON_URL/v1/sites/$SITE_ID/monitoring/http" \
    "${CURL_AUTH[@]}" \
    -d "{
      \"target_url\": \"$TARGET_URL\",
      \"check_interval_seconds\": $CHECK_INTERVAL,
      \"expected_status_code\": 200,
      \"is_active\": true
    }" 2>/dev/null || echo "000")

  if [[ "$MON_CODE" =~ ^2 ]]; then
    ok=$((ok + 1))
  else
    log "WARNING: Monitor creation returned HTTP $MON_CODE for site $SITE_ID"
    skipped=$((skipped + 1))
  fi
done
log "Done. $ok monitors created, $skipped skipped."

# ── Persist cookie for k6 / measure scripts ───────────────────────────────────
echo "$COOKIE" > "$ROOT_DIR/results/alon_session.txt"
log "Session cookie saved to results/alon_session.txt"
