#!/usr/bin/env bash
# Sets up Uptime Kuma and seeds MONITOR_COUNT HTTP monitors via Socket.io.
# Delegates to setup_kuma.js (requires node + socket.io-client in benchmark/).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

if [[ -f "$ROOT_DIR/.env" ]]; then
  set -a; source "$ROOT_DIR/.env"; set +a
fi

KUMA_URL="${KUMA_URL:-http://localhost:3101}"
ADMIN_USER="${KUMA_ADMIN_USER:-bench}"
ADMIN_PASSWORD="${KUMA_ADMIN_PASSWORD:-Bench1234!}"
MONITOR_COUNT="${MONITOR_COUNT:-50}"
CHECK_INTERVAL="${CHECK_INTERVAL:-60}"
TARGET_URL="http://target:8088/ok"

log() { echo "[kuma-setup] $*"; }

# ── Wait for Kuma ─────────────────────────────────────────────────────────────
log "Waiting for Uptime Kuma at $KUMA_URL ..."
for i in $(seq 1 40); do
  STATUS=$(curl -s -o /dev/null -w "%{http_code}" "$KUMA_URL/" 2>/dev/null || echo "000")
  if [[ "$STATUS" =~ ^(200|301|302)$ ]]; then
    log "Kuma is up (HTTP $STATUS)."
    break
  fi
  [[ $i -eq 40 ]] && { log "ERROR: Kuma did not become ready."; exit 1; }
  sleep 3
done

mkdir -p "$ROOT_DIR/results"

# ── Delegate to Node.js script ────────────────────────────────────────────────
cd "$ROOT_DIR"
node scripts/setup_kuma.js \
  "$KUMA_URL" \
  "$ADMIN_USER" \
  "$ADMIN_PASSWORD" \
  "$MONITOR_COUNT" \
  "$CHECK_INTERVAL" \
  "$TARGET_URL" \
  "results/kuma_token.txt"
