#!/usr/bin/env bash
# Measure worker check throughput and container resource usage for both tools,
# then print a side-by-side comparison table.
#
# Must be called AFTER setup_alon.sh and setup_kuma.sh have seeded monitors
# and workers have had time to run (wait at least one CHECK_INTERVAL).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

if [[ -f "$ROOT_DIR/.env" ]]; then
  set -a; source "$ROOT_DIR/.env"; set +a
fi

MEASURE_DURATION="${MEASURE_DURATION:-300}"
MONITOR_COUNT="${MONITOR_COUNT:-50}"
CHECK_INTERVAL="${CHECK_INTERVAL:-60}"
COMPOSE_PROJECT="benchmark"

ALON_API_CTR="${COMPOSE_PROJECT}-alon-api-1"
ALON_WORKER_CTR="${COMPOSE_PROJECT}-alon-worker-1"
ALON_PG_CTR="${COMPOSE_PROJECT}-postgres-1"
KUMA_CTR="${COMPOSE_PROJECT}-uptime-kuma-1"

log() { echo "[measure] $*"; }

# ── Helpers ───────────────────────────────────────────────────────────────────
container_cpu() {
  docker stats --no-stream --format "{{.CPUPerc}}" "$1" 2>/dev/null \
    | tr -d '%' | head -1 || echo "0"
}

container_mem_mib() {
  docker stats --no-stream --format "{{.MemUsage}}" "$1" 2>/dev/null \
    | cut -d'/' -f1 | tr -d ' ' \
    | awk '{
        v=$1
        if      (v ~ /GiB/) { sub(/GiB/,"",v); printf "%.0f", v*1024 }
        else if (v ~ /MiB/) { sub(/MiB/,"",v); printf "%.0f", v }
        else if (v ~ /kB/)  { sub(/kB/,"",v);  printf "%.0f", v/1024 }
        else                 { printf "%.0f", v/1048576 }
      }' 2>/dev/null || echo "0"
}

# ── Ensure sqlite3 is available in kuma container ────────────────────────────
log "Ensuring sqlite3 available in Kuma container ..."
MSYS_NO_PATHCONV=1 docker exec "$KUMA_CTR" sh -c "which sqlite3 2>/dev/null || apk add --no-cache sqlite3 2>/dev/null || true"

# ── Record start timestamp ────────────────────────────────────────────────────
START_PG=$(docker exec "$ALON_PG_CTR" psql -U alon -d alon_bench_db -tAq \
  -c "SELECT NOW();" 2>/dev/null | tr -d '\n\r')
# Kuma stores heartbeat.time as DATETIME in UTC (e.g. "2026-05-16 14:28:25")
START_KUMA=$(date -u +"%Y-%m-%d %H:%M:%S")
START_MS=$(date +%s%3N)

log "Measuring for ${MEASURE_DURATION}s — collecting resource samples every 15s ..."
log "Start time: $START_PG (Postgres clock)"

# ── Sample resource usage during measurement window ──────────────────────────
ALON_WRK_CPU_SUM=0; ALON_WRK_MEM_SUM=0
ALON_API_CPU_SUM=0; ALON_API_MEM_SUM=0
KUMA_CPU_SUM=0; KUMA_MEM_SUM=0
SAMPLES=0

elapsed=0
while [[ $elapsed -lt $MEASURE_DURATION ]]; do
  sleep 15
  elapsed=$((elapsed + 15))

  # Alon worker
  WRK_CPU=$(container_cpu  "$ALON_WORKER_CTR")
  WRK_MEM=$(container_mem_mib "$ALON_WORKER_CTR")
  ALON_WRK_CPU_SUM=$(awk "BEGIN{print $ALON_WRK_CPU_SUM + ${WRK_CPU:-0}}")
  ALON_WRK_MEM_SUM=$(awk "BEGIN{print $ALON_WRK_MEM_SUM + ${WRK_MEM:-0}}")

  # Alon API
  API_CPU=$(container_cpu  "$ALON_API_CTR")
  API_MEM=$(container_mem_mib "$ALON_API_CTR")
  ALON_API_CPU_SUM=$(awk "BEGIN{print $ALON_API_CPU_SUM + ${API_CPU:-0}}")
  ALON_API_MEM_SUM=$(awk "BEGIN{print $ALON_API_MEM_SUM + ${API_MEM:-0}}")

  # Kuma (single container — API + worker combined)
  K_CPU=$(container_cpu  "$KUMA_CTR")
  K_MEM=$(container_mem_mib "$KUMA_CTR")
  KUMA_CPU_SUM=$(awk "BEGIN{print $KUMA_CPU_SUM + ${K_CPU:-0}}")
  KUMA_MEM_SUM=$(awk "BEGIN{print $KUMA_MEM_SUM + ${K_MEM:-0}}")

  SAMPLES=$((SAMPLES + 1))
  log "  ${elapsed}s — Alon worker: ${WRK_CPU}% / ${WRK_MEM} MiB   Alon API: ${API_CPU}% / ${API_MEM} MiB   Kuma: ${K_CPU}% / ${K_MEM} MiB"
done

END_MS=$(date +%s%3N)
ACTUAL_SECS=$(( (END_MS - START_MS) / 1000 ))

# ── Count Alon checks (Postgres) ──────────────────────────────────────────────
log "Querying Alon check count ..."
ALON_CHECKS=$(docker exec "$ALON_PG_CTR" psql -U alon -d alon_bench_db -tAq \
  -c "SELECT COUNT(*) FROM site_monitor_checks WHERE checked_at >= '$START_PG';" \
  2>/dev/null | tr -d ' ')

EXPECTED=$(( MONITOR_COUNT * ACTUAL_SECS / CHECK_INTERVAL ))
ALON_MISSED=$(( EXPECTED > ALON_CHECKS ? EXPECTED - ALON_CHECKS : 0 ))

# ── Count Kuma heartbeats (SQLite) ────────────────────────────────────────────
log "Querying Kuma heartbeat count ..."
KUMA_CHECKS=$(MSYS_NO_PATHCONV=1 docker exec "$KUMA_CTR" sqlite3 /app/data/kuma.db \
  "SELECT COUNT(*) FROM heartbeat WHERE time >= '${START_KUMA}';" \
  2>/dev/null | tr -d ' ' || echo "N/A")

if [[ "$KUMA_CHECKS" =~ ^[0-9]+$ ]]; then
  KUMA_MISSED=$(( EXPECTED > KUMA_CHECKS ? EXPECTED - KUMA_CHECKS : 0 ))
else
  KUMA_MISSED="N/A"
fi

# ── Compute averages ──────────────────────────────────────────────────────────
if [[ $SAMPLES -gt 0 ]]; then
  ALON_WRK_CPU_AVG=$(awk "BEGIN{printf \"%.1f\", $ALON_WRK_CPU_SUM / $SAMPLES}")
  ALON_WRK_MEM_AVG=$(awk "BEGIN{printf \"%.0f\", $ALON_WRK_MEM_SUM / $SAMPLES}")
  ALON_API_CPU_AVG=$(awk "BEGIN{printf \"%.1f\", $ALON_API_CPU_SUM / $SAMPLES}")
  ALON_API_MEM_AVG=$(awk "BEGIN{printf \"%.0f\", $ALON_API_MEM_SUM / $SAMPLES}")
  ALON_TOT_CPU_AVG=$(awk "BEGIN{printf \"%.1f\", ($ALON_WRK_CPU_SUM + $ALON_API_CPU_SUM) / $SAMPLES}")
  ALON_TOT_MEM_AVG=$(awk "BEGIN{printf \"%.0f\", ($ALON_WRK_MEM_SUM + $ALON_API_MEM_SUM) / $SAMPLES}")
  KUMA_CPU_AVG=$(awk "BEGIN{printf \"%.1f\", $KUMA_CPU_SUM / $SAMPLES}")
  KUMA_MEM_AVG=$(awk "BEGIN{printf \"%.0f\", $KUMA_MEM_SUM / $SAMPLES}")
else
  ALON_WRK_CPU_AVG="N/A"; ALON_WRK_MEM_AVG="N/A"
  ALON_API_CPU_AVG="N/A"; ALON_API_MEM_AVG="N/A"
  ALON_TOT_CPU_AVG="N/A"; ALON_TOT_MEM_AVG="N/A"
  KUMA_CPU_AVG="N/A";     KUMA_MEM_AVG="N/A"
fi

ALON_CPM=$(awk "BEGIN{printf \"%.1f\", $ALON_CHECKS / ($ACTUAL_SECS / 60)}")
KUMA_CPM="N/A"
if [[ "$KUMA_CHECKS" =~ ^[0-9]+$ ]]; then
  KUMA_CPM=$(awk "BEGIN{printf \"%.1f\", $KUMA_CHECKS / ($ACTUAL_SECS / 60)}")
fi

# ── Print report ──────────────────────────────────────────────────────────────
# Resource note: Kuma runs API + worker in one container (fair total cost).
# Alon splits them; we report worker, API, and combined so the comparison is
# apples-to-apples at the "combined" row.  Postgres is a shared dependency and
# is excluded from both sides.
RESULT_FILE="$ROOT_DIR/results/worker_$(date +%Y%m%d_%H%M%S).txt"

{
  echo ""
  echo "╔══════════════════════════════════════════════════════════════════╗"
  echo "║       Alon Sentinel vs Uptime Kuma — Worker Throughput           ║"
  echo "╚══════════════════════════════════════════════════════════════════╝"
  echo ""
  printf "  %-24s  %-12s\n" "Monitors seeded"    "$MONITOR_COUNT"
  printf "  %-24s  %-12s\n" "Check interval"     "${CHECK_INTERVAL}s"
  printf "  %-24s  %-12s\n" "Measurement window" "${ACTUAL_SECS}s"
  printf "  %-24s  %-12s\n" "Expected checks"    "$EXPECTED"
  echo ""
  printf "  %-32s  %-18s  %-18s\n" "Metric" "Alon Sentinel" "Uptime Kuma"
  printf "  %-32s  %-18s  %-18s\n" "--------------------------------" "------------------" "------------------"
  printf "  %-32s  %-18s  %-18s\n" "Checks completed"       "$ALON_CHECKS"          "$KUMA_CHECKS"
  printf "  %-32s  %-18s  %-18s\n" "Checks / min"           "$ALON_CPM"             "$KUMA_CPM"
  printf "  %-32s  %-18s  %-18s\n" "Missed checks"          "$ALON_MISSED"          "$KUMA_MISSED"
  printf "  %-32s  %-18s  %-18s\n" "Worker CPU avg"         "${ALON_WRK_CPU_AVG}%"  "(combined below)"
  printf "  %-32s  %-18s  %-18s\n" "Worker RAM avg"         "${ALON_WRK_MEM_AVG} MiB" ""
  printf "  %-32s  %-18s  %-18s\n" "API CPU avg"            "${ALON_API_CPU_AVG}%"  "(combined below)"
  printf "  %-32s  %-18s  %-18s\n" "API RAM avg"            "${ALON_API_MEM_AVG} MiB" ""
  printf "  %-32s  %-18s  %-18s\n" "Combined CPU avg"       "${ALON_TOT_CPU_AVG}%"  "${KUMA_CPU_AVG}%"
  printf "  %-32s  %-18s  %-18s\n" "Combined RAM avg"       "${ALON_TOT_MEM_AVG} MiB" "${KUMA_MEM_AVG} MiB"
  echo ""
  echo "  Note: Postgres not counted on either side (Kuma uses embedded SQLite)."
  echo ""
} | tee "$RESULT_FILE"

log "Results saved to $RESULT_FILE"
