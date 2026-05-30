#!/usr/bin/env bash
# Head-to-head worker scale comparison: Alon Sentinel vs Uptime Kuma.
#
# Runs one scenario at a time (seed → warmup → measure → report).
# Run three times for the full suite:
#   bash scripts/compare_bench.sh --monitors 1000
#   bash scripts/compare_bench.sh --monitors 5000
#   bash scripts/compare_bench.sh --monitors 10000
#
# Flags:
#   --monitors N      number of monitors to seed into each tool (required)
#   --interval S      check interval in seconds              (default 120)
#   --duration S      measurement window in seconds          (default 540)
#   --warmup S        warmup before measurement starts       (default 60)
#   --seed-parallel N parallel curl workers for Alon seeding (default 30)
#   --kuma-parallel N parallel Socket.io conns for Kuma      (default 10)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT_DIR"

if [[ -f ".env" ]]; then set -a; source ".env"; set +a; fi

# ── Defaults ──────────────────────────────────────────────────────────────────
MONITOR_COUNT=""
CHECK_INTERVAL="${CHECK_INTERVAL:-120}"
MEASURE_DURATION="${COMPARE_MEASURE_DURATION:-540}"
WARMUP_SECONDS="${COMPARE_WARMUP:-60}"
SEED_PARALLEL="${SEED_PARALLEL:-30}"
KUMA_PARALLEL="${KUMA_PARALLEL:-5}"

ALON_URL="${ALON_URL:-http://localhost:3100}"
KUMA_SQLITE_URL="${KUMA_SQLITE_URL:-${KUMA_URL:-http://localhost:3101}}"
KUMA_MARIADB_URL="${KUMA_MARIADB_URL:-http://localhost:3102}"
ADMIN_EMAIL="${ALON_ADMIN_EMAIL:-bench@bench.local}"
ADMIN_PASSWORD="${ALON_ADMIN_PASSWORD:-Bench1234!}"
KUMA_USER="${KUMA_ADMIN_USER:-bench}"
KUMA_PASS="${KUMA_ADMIN_PASSWORD:-Bench1234!}"
KUMA_MARIADB_DATABASE="${KUMA_MARIADB_DATABASE:-kuma}"
KUMA_MARIADB_USER="${KUMA_MARIADB_USER:-kuma}"
KUMA_MARIADB_PASSWORD="${KUMA_MARIADB_PASSWORD:-kumapw}"

COMPOSE_PROJECT="benchmark"
ALON_PG_CTR="${COMPOSE_PROJECT}-postgres-1"
ALON_WORKER_CTR="${COMPOSE_PROJECT}-alon-worker-1"
KUMA_SQLITE_CTR="${COMPOSE_PROJECT}-uptime-kuma-1"
KUMA_MARIADB_APP_CTR="${COMPOSE_PROJECT}-uptime-kuma-mariadb-1"
KUMA_MARIADB_CTR="${COMPOSE_PROJECT}-kuma-mariadb-1"
ALON_API_CTR="${COMPOSE_PROJECT}-alon-api-1"

# ── Arg parsing ───────────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
    case "$1" in
        --monitors=*)      MONITOR_COUNT="${1#--monitors=}" ;;
        --interval=*)      CHECK_INTERVAL="${1#--interval=}" ;;
        --duration=*)      MEASURE_DURATION="${1#--duration=}" ;;
        --warmup=*)        WARMUP_SECONDS="${1#--warmup=}" ;;
        --seed-parallel=*) SEED_PARALLEL="${1#--seed-parallel=}" ;;
        --kuma-parallel=*) KUMA_PARALLEL="${1#--kuma-parallel=}" ;;
        --monitors)       shift; MONITOR_COUNT="${1:-}" ;;
        --interval)       shift; CHECK_INTERVAL="${1:-}" ;;
        --duration)       shift; MEASURE_DURATION="${1:-}" ;;
        --warmup)         shift; WARMUP_SECONDS="${1:-}" ;;
        --seed-parallel)  shift; SEED_PARALLEL="${1:-}" ;;
        --kuma-parallel)  shift; KUMA_PARALLEL="${1:-}" ;;
    esac
    shift
done

if [[ -z "$MONITOR_COUNT" ]]; then
    echo "Usage: $0 --monitors N [--interval S] [--duration S]"
    exit 1
fi

SAMPLE_INTERVAL=15
mkdir -p results

log()  { echo "[compare] $*"; }
hdr()  { echo ""; printf '%*s\n' 72 '' | tr ' ' '='; echo "  $*"; printf '%*s\n' 72 '' | tr ' ' '='; }
pg()   { docker exec "$ALON_PG_CTR" psql -U alon -d alon_bench_db -tAq -c "$1" 2>/dev/null | tr -d '\n\r' | xargs; }
kuma_sqlite() { MSYS_NO_PATHCONV=1 docker exec "$KUMA_SQLITE_CTR" sqlite3 /app/data/kuma.db "$1" 2>/dev/null | tr -d '\n\r' | xargs; }
kuma_mariadb() {
    docker exec "$KUMA_MARIADB_CTR" mariadb \
        -u"$KUMA_MARIADB_USER" "-p$KUMA_MARIADB_PASSWORD" "$KUMA_MARIADB_DATABASE" \
        -Nse "$1" 2>/dev/null | tr -d '\r' | xargs
}
kuma_sqlite_file_mib() {
    MSYS_NO_PATHCONV=1 docker exec "$KUMA_SQLITE_CTR" sh -c \
        "du -cm /app/data/kuma.db /app/data/kuma.db-wal /app/data/kuma.db-shm 2>/dev/null | awk '/total/{print \$1}'" \
        2>/dev/null | tr -d '\n\r' | xargs
}
kuma_mariadb_size_mib() {
    kuma_mariadb "SELECT ROUND(COALESCE(SUM(data_length + index_length),0) / 1024 / 1024, 1) FROM information_schema.tables WHERE table_schema='${KUMA_MARIADB_DATABASE}';"
}
container_image() {
    docker inspect --format '{{.Config.Image}}' "$1" 2>/dev/null || echo "N/A"
}

container_stats() {
    # Returns "cpu,memMiB" for the named container
    local raw cpu mem
    raw=$(docker stats --no-stream --format "{{.CPUPerc}},{{.MemUsage}}" "$1" 2>/dev/null | head -1 || echo "0,0/0")
    cpu=$(echo "$raw" | cut -d',' -f1 | tr -d '%')
    mem=$(echo "$raw" | cut -d',' -f2 | cut -d'/' -f1 | tr -d ' ' \
        | awk '{v=$1; if(v~/GiB/){sub(/GiB/,"",v);printf "%.0f",v*1024}
               else if(v~/MiB/){sub(/MiB/,"",v);printf "%.0f",v}
               else if(v~/kB/){sub(/kB/,"",v);printf "%.0f",v/1024}
               else{printf "%.0f",v/1048576}}' 2>/dev/null || echo "0")
    echo "${cpu:-0},${mem:-0}"
}

awk_avg() { awk "BEGIN{printf \"%.1f\", ($1)/($2)}"; }
awk_max() { awk "BEGIN{v=(($1)>($2))?($1):($2); print v}"; }

# ── Wait for APIs ─────────────────────────────────────────────────────────────
log "Waiting for Alon API at $ALON_URL ..."
for i in $(seq 1 20); do
    if curl -sf "$ALON_URL/health" -o /dev/null 2>/dev/null; then log "Alon API ready."; break; fi
    [[ $i -eq 20 ]] && { log "ERROR: Alon API not ready"; exit 1; }
    sleep 3
done

log "Waiting for Kuma SQLite at $KUMA_SQLITE_URL ..."
for i in $(seq 1 20); do
    STATUS=$(curl -s -o /dev/null -w "%{http_code}" "$KUMA_SQLITE_URL/" 2>/dev/null || echo "000")
    if [[ "$STATUS" =~ ^(200|301|302)$ ]]; then log "Kuma SQLite ready (HTTP $STATUS)."; break; fi
    [[ $i -eq 20 ]] && { log "ERROR: Kuma SQLite not ready"; exit 1; }
    sleep 3
done

log "Waiting for Kuma MariaDB at $KUMA_MARIADB_URL ..."
for i in $(seq 1 20); do
    STATUS=$(curl -s -o /dev/null -w "%{http_code}" "$KUMA_MARIADB_URL/" 2>/dev/null || echo "000")
    if [[ "$STATUS" =~ ^(200|301|302)$ ]]; then log "Kuma MariaDB ready (HTTP $STATUS)."; break; fi
    [[ $i -eq 20 ]] && { log "ERROR: Kuma MariaDB not ready"; exit 1; }
    sleep 3
done

# ── Reset Alon ────────────────────────────────────────────────────────────────
log "Resetting Alon DB (TRUNCATE sites CASCADE) ..."
if ! docker exec "$ALON_PG_CTR" psql -U alon -d alon_bench_db -q \
        -c "TRUNCATE sites CASCADE;" 2>&1; then
    log "WARNING: TRUNCATE failed, trying DELETE ..."
    docker exec "$ALON_PG_CTR" psql -U alon -d alon_bench_db -q \
        -c "DELETE FROM site_monitor_checks; DELETE FROM site_monitors; DELETE FROM sites;" 2>&1 || true
fi
log "Alon DB reset done."

# ── Reset Kuma ────────────────────────────────────────────────────────────────
log "Resetting Kuma SQLite monitors ..."
kuma_sqlite "DELETE FROM monitor;" || true
kuma_sqlite "DELETE FROM heartbeat;" || true
log "Kuma SQLite reset done."

log "Resetting Kuma MariaDB monitors ..."
kuma_mariadb "SET FOREIGN_KEY_CHECKS=0; DELETE FROM monitor; DELETE FROM heartbeat; SET FOREIGN_KEY_CHECKS=1;" || true
log "Kuma MariaDB reset done."

# ── Login to Alon ─────────────────────────────────────────────────────────────
log "Logging in to Alon ..."
_resp=$(curl -s -D - -X POST "$ALON_URL/v1/admin/auth/login" \
    -H "Content-Type: application/json" \
    -d "{\"email\":\"$ADMIN_EMAIL\",\"password\":\"$ADMIN_PASSWORD\"}" 2>/dev/null)
_code=$(echo "$_resp" | head -1 | grep -o '[0-9][0-9][0-9]' | head -1)
[[ "$_code" != "200" ]] && { log "ERROR: Alon login returned HTTP $_code"; exit 1; }
COOKIE=$(echo "$_resp" | grep -i "set-cookie:" | sed -n 's/.*admin_session=\([^;]*\).*/\1/p' | tr -d '\r')
[[ -z "$COOKIE" ]] && { log "ERROR: No Alon session cookie"; exit 1; }
log "Alon login OK (cookie length: ${#COOKIE})."

# Pre-flight: verify cookie works before spawning parallel jobs
log "Pre-flight cookie check ..."
_pf_resp=$(curl -s -w "\n__STATUS__:%{http_code}" "$ALON_URL/v1/sites" \
    -b "admin_session=$COOKIE" 2>/dev/null || true)
_pf_code=$(printf '%s' "$_pf_resp" | grep '__STATUS__:' | cut -d: -f2 | tr -d '\r')
if [[ "$_pf_code" != "200" ]]; then
    _pf_body=$(printf '%s' "$_pf_resp" | grep -v '__STATUS__:')
    log "ERROR: Cookie check failed — GET /v1/sites returned HTTP ${_pf_code}"
    log "Response body: ${_pf_body}"
    exit 1
fi
log "Cookie verified OK (GET /v1/sites → 200)."

# ── Seed Alon (parallel curl) ─────────────────────────────────────────────────
hdr "Seeding: $MONITOR_COUNT monitors -> Alon + Kuma SQLite + Kuma MariaDB"

TARGET_URL_DOCKER="http://target:8088/ok"

log "Seeding Alon ($MONITOR_COUNT monitors, ${SEED_PARALLEL} parallel) ..."
_tmpdir=$(mktemp -d)
_pids=()

seed_one_alon() {
    local i="$1" out="$2" cookie="$3" alon_url="$4" interval="$5" target="$6"
    local site_resp site_id mon_code
    site_resp=$(curl -s -X POST "$alon_url/v1/sites" \
        -b "admin_session=$cookie" \
        -H "Content-Type: application/json" \
        -d "{\"name\":\"Compare Site $i\",\"base_url\":\"http://compare-$i.bench.local\"}" \
        2>/dev/null || true)
    site_id=$(printf '%s' "$site_resp" | grep -o '"id":[0-9]*' | head -1 | grep -o '[0-9]*' || true)
    if [[ -z "$site_id" ]]; then echo skip > "$out"; return; fi
    mon_code=$(curl -s -o /dev/null -w "%{http_code}" \
        -X PUT "$alon_url/v1/sites/$site_id/monitoring/http" \
        -b "admin_session=$cookie" \
        -H "Content-Type: application/json" \
        -d "{\"target_url\":\"$target\",\"check_interval_seconds\":$interval,\"expected_status_code\":200,\"is_active\":true}" \
        2>/dev/null || echo 000)
    [[ "$mon_code" =~ ^2 ]] && echo ok > "$out" || echo skip > "$out"
}

for i in $(seq 1 "$MONITOR_COUNT"); do
    seed_one_alon "$i" "$_tmpdir/$i" "$COOKIE" "$ALON_URL" "$CHECK_INTERVAL" "$TARGET_URL_DOCKER" &
    _pids+=($!)
    if (( ${#_pids[@]} >= SEED_PARALLEL )); then
        wait "${_pids[0]}"; _pids=("${_pids[@]:1}")
    fi
done
wait "${_pids[@]:-}"

_alon_ok=0; _alon_skip=0
for i in $(seq 1 "$MONITOR_COUNT"); do
    r=$(cat "$_tmpdir/$i" 2>/dev/null || echo "skip")
    [[ "$r" == "ok" ]] && _alon_ok=$((_alon_ok+1)) || _alon_skip=$((_alon_skip+1))
done
rm -rf "$_tmpdir"
log "Alon seeding done: $_alon_ok created, $_alon_skip skipped."
(( _alon_ok == 0 )) && { log "ERROR: No Alon monitors created"; exit 1; }

# ── Seed Kuma SQLite (parallel Socket.io) ────────────────────────────────────
log "Seeding Kuma SQLite ($MONITOR_COUNT monitors, ${KUMA_PARALLEL} sockets) ..."
KUMA_URL="$KUMA_SQLITE_URL" \
KUMA_USER="$KUMA_USER" \
KUMA_PASS="$KUMA_PASS" \
MONITOR_COUNT="$MONITOR_COUNT" \
SEED_PARALLEL="$KUMA_PARALLEL" \
CHECK_INTERVAL="$CHECK_INTERVAL" \
TARGET_URL="$TARGET_URL_DOCKER" \
    node "$SCRIPT_DIR/seed_kuma_parallel.js"
log "Kuma SQLite seeding done."

# ── Seed Kuma MariaDB (parallel Socket.io) ───────────────────────────────────
log "Seeding Kuma MariaDB ($MONITOR_COUNT monitors, ${KUMA_PARALLEL} sockets) ..."
KUMA_URL="$KUMA_MARIADB_URL" \
KUMA_USER="$KUMA_USER" \
KUMA_PASS="$KUMA_PASS" \
MONITOR_COUNT="$MONITOR_COUNT" \
SEED_PARALLEL="$KUMA_PARALLEL" \
CHECK_INTERVAL="$CHECK_INTERVAL" \
TARGET_URL="$TARGET_URL_DOCKER" \
    node "$SCRIPT_DIR/seed_kuma_parallel.js"
log "Kuma MariaDB seeding done."

# ── Warmup ────────────────────────────────────────────────────────────────────
log "Warmup: ${WARMUP_SECONDS}s (both tools claim and run first check cycle) ..."
sleep "$WARMUP_SECONDS"

# ── Pre-measurement baselines ─────────────────────────────────────────────────
hdr "Measuring: $MONITOR_COUNT monitors / ${CHECK_INTERVAL}s interval / ${MEASURE_DURATION}s window"

START_TS=$(pg "SELECT NOW();")
START_CHECKS_ALON=$(pg "SELECT COUNT(*) FROM site_monitor_checks;")
START_WALL=$(date +%s)
START_KUMA=$(date -u +"%Y-%m-%d %H:%M:%S")

log "Alon clock: $START_TS"
log "Kuma clocks: $START_KUMA"

# ── Sampling loop ─────────────────────────────────────────────────────────────
AW_CPU_SUM=0; AW_CPU_MAX=0; AW_MEM_SUM=0; AW_MEM_MAX=0  # Alon worker
AA_CPU_SUM=0; AA_CPU_MAX=0; AA_MEM_SUM=0; AA_MEM_MAX=0  # Alon API
PG_CPU_SUM=0; PG_CPU_MAX=0; PG_MEM_SUM=0; PG_MEM_MAX=0  # Alon Postgres
KS_CPU_SUM=0; KS_CPU_MAX=0; KS_MEM_SUM=0; KS_MEM_MAX=0  # Kuma SQLite container
KM_CPU_SUM=0; KM_CPU_MAX=0; KM_MEM_SUM=0; KM_MEM_MAX=0  # Kuma MariaDB app
KDB_CPU_SUM=0; KDB_CPU_MAX=0; KDB_MEM_SUM=0; KDB_MEM_MAX=0 # Kuma MariaDB database
SAMPLES=0
elapsed=0

while (( elapsed < MEASURE_DURATION )); do
    sleep "$SAMPLE_INTERVAL"
    elapsed=$(( elapsed + SAMPLE_INTERVAL ))

    # Alon worker
    aw_raw=$(container_stats "$ALON_WORKER_CTR")
    aw_cpu=$(echo "$aw_raw" | cut -d',' -f1)
    aw_mem=$(echo "$aw_raw" | cut -d',' -f2)
    AW_CPU_SUM=$(awk "BEGIN{print $AW_CPU_SUM + ${aw_cpu:-0}}")
    AW_MEM_SUM=$(awk "BEGIN{print $AW_MEM_SUM + ${aw_mem:-0}}")
    AW_CPU_MAX=$(awk_max "$AW_CPU_MAX" "${aw_cpu:-0}")
    AW_MEM_MAX=$(awk_max "$AW_MEM_MAX" "${aw_mem:-0}")

    # Alon API
    aa_raw=$(container_stats "$ALON_API_CTR")
    aa_cpu=$(echo "$aa_raw" | cut -d',' -f1)
    aa_mem=$(echo "$aa_raw" | cut -d',' -f2)
    AA_CPU_SUM=$(awk "BEGIN{print $AA_CPU_SUM + ${aa_cpu:-0}}")
    AA_MEM_SUM=$(awk "BEGIN{print $AA_MEM_SUM + ${aa_mem:-0}}")
    AA_CPU_MAX=$(awk_max "$AA_CPU_MAX" "${aa_cpu:-0}")
    AA_MEM_MAX=$(awk_max "$AA_MEM_MAX" "${aa_mem:-0}")

    # Alon Postgres
    pg_raw=$(container_stats "$ALON_PG_CTR")
    pg_cpu=$(echo "$pg_raw" | cut -d',' -f1)
    pg_mem=$(echo "$pg_raw" | cut -d',' -f2)
    PG_CPU_SUM=$(awk "BEGIN{print $PG_CPU_SUM + ${pg_cpu:-0}}")
    PG_MEM_SUM=$(awk "BEGIN{print $PG_MEM_SUM + ${pg_mem:-0}}")
    PG_CPU_MAX=$(awk_max "$PG_CPU_MAX" "${pg_cpu:-0}")
    PG_MEM_MAX=$(awk_max "$PG_MEM_MAX" "${pg_mem:-0}")

    # Kuma SQLite (single container: app + embedded SQLite)
    ks_raw=$(container_stats "$KUMA_SQLITE_CTR")
    ks_cpu=$(echo "$ks_raw" | cut -d',' -f1)
    ks_mem=$(echo "$ks_raw" | cut -d',' -f2)
    KS_CPU_SUM=$(awk "BEGIN{print $KS_CPU_SUM + ${ks_cpu:-0}}")
    KS_MEM_SUM=$(awk "BEGIN{print $KS_MEM_SUM + ${ks_mem:-0}}")
    KS_CPU_MAX=$(awk_max "$KS_CPU_MAX" "${ks_cpu:-0}")
    KS_MEM_MAX=$(awk_max "$KS_MEM_MAX" "${ks_mem:-0}")

    # Kuma MariaDB app
    km_raw=$(container_stats "$KUMA_MARIADB_APP_CTR")
    km_cpu=$(echo "$km_raw" | cut -d',' -f1)
    km_mem=$(echo "$km_raw" | cut -d',' -f2)
    KM_CPU_SUM=$(awk "BEGIN{print $KM_CPU_SUM + ${km_cpu:-0}}")
    KM_MEM_SUM=$(awk "BEGIN{print $KM_MEM_SUM + ${km_mem:-0}}")
    KM_CPU_MAX=$(awk_max "$KM_CPU_MAX" "${km_cpu:-0}")
    KM_MEM_MAX=$(awk_max "$KM_MEM_MAX" "${km_mem:-0}")

    # Kuma MariaDB database
    kdb_raw=$(container_stats "$KUMA_MARIADB_CTR")
    kdb_cpu=$(echo "$kdb_raw" | cut -d',' -f1)
    kdb_mem=$(echo "$kdb_raw" | cut -d',' -f2)
    KDB_CPU_SUM=$(awk "BEGIN{print $KDB_CPU_SUM + ${kdb_cpu:-0}}")
    KDB_MEM_SUM=$(awk "BEGIN{print $KDB_MEM_SUM + ${kdb_mem:-0}}")
    KDB_CPU_MAX=$(awk_max "$KDB_CPU_MAX" "${kdb_cpu:-0}")
    KDB_MEM_MAX=$(awk_max "$KDB_MEM_MAX" "${kdb_mem:-0}")

    SAMPLES=$(( SAMPLES + 1 ))
    log "  ${elapsed}s - Alon worker: ${aw_cpu}%/${aw_mem}MiB  API: ${aa_cpu}%/${aa_mem}MiB  Postgres: ${pg_cpu}%/${pg_mem}MiB  Kuma SQLite: ${ks_cpu}%/${ks_mem}MiB  Kuma MariaDB app: ${km_cpu}%/${km_mem}MiB  MariaDB: ${kdb_cpu}%/${kdb_mem}MiB"
done

END_WALL=$(date +%s)
ACTUAL_SECS=$(( END_WALL - START_WALL ))

# ── Post-measurement queries ───────────────────────────────────────────────────
log "Querying final metrics ..."

END_CHECKS_ALON=$(pg "SELECT COUNT(*) FROM site_monitor_checks;")
ALON_DONE=$(pg "SELECT COUNT(*) FROM site_monitor_checks WHERE checked_at >= '${START_TS}';")
ALON_INCIDENTS=$(pg "SELECT COUNT(*) FROM site_monitor_incidents WHERE status='open';")
ALON_DUPS=$(pg "SELECT COUNT(*) FROM (
    SELECT site_monitor_id FROM site_monitor_incidents WHERE status='open'
    GROUP BY site_monitor_id HAVING COUNT(*) > 1) t;")

KUMA_SQLITE_DONE=$(kuma_sqlite "SELECT COUNT(*) FROM heartbeat WHERE time >= '${START_KUMA}';" || echo "N/A")
KUMA_MARIADB_DONE=$(kuma_mariadb "SELECT COUNT(*) FROM heartbeat WHERE time >= '${START_KUMA}';" || echo "N/A")
KUMA_SQLITE_SIZE_MIB=$(kuma_sqlite_file_mib || echo "N/A")
KUMA_MARIADB_SIZE_MIB=$(kuma_mariadb_size_mib || echo "N/A")
KUMA_SQLITE_IMAGE=$(container_image "$KUMA_SQLITE_CTR")
KUMA_MARIADB_IMAGE=$(container_image "$KUMA_MARIADB_APP_CTR")

EXPECTED=$(awk "BEGIN{printf \"%d\", int($MONITOR_COUNT * $ACTUAL_SECS / $CHECK_INTERVAL)}")
ALON_MISSED=$(awk "BEGIN{m=$EXPECTED - $ALON_DONE; print (m>0)?m:0}")
ALON_MISS_PCT=$(awk "BEGIN{e=$EXPECTED; c=$ALON_DONE; if(e>0 && c<e) printf \"%.1f%%\",(e-c)/e*100; else print \"0.0%\"}")

KUMA_SQLITE_MISSED="N/A"; KUMA_SQLITE_MISS_PCT="N/A"
if [[ "$KUMA_SQLITE_DONE" =~ ^[0-9]+$ ]]; then
    KUMA_SQLITE_MISSED=$(awk "BEGIN{m=$EXPECTED - $KUMA_SQLITE_DONE; print (m>0)?m:0}")
    KUMA_SQLITE_MISS_PCT=$(awk "BEGIN{e=$EXPECTED; c=$KUMA_SQLITE_DONE; if(e>0 && c<e) printf \"%.1f%%\",(e-c)/e*100; else print \"0.0%\"}")
fi

KUMA_MARIADB_MISSED="N/A"; KUMA_MARIADB_MISS_PCT="N/A"
if [[ "$KUMA_MARIADB_DONE" =~ ^[0-9]+$ ]]; then
    KUMA_MARIADB_MISSED=$(awk "BEGIN{m=$EXPECTED - $KUMA_MARIADB_DONE; print (m>0)?m:0}")
    KUMA_MARIADB_MISS_PCT=$(awk "BEGIN{e=$EXPECTED; c=$KUMA_MARIADB_DONE; if(e>0 && c<e) printf \"%.1f%%\",(e-c)/e*100; else print \"0.0%\"}")
fi

if (( SAMPLES > 0 )); then
    AW_CPU_AVG=$(awk_avg "$AW_CPU_SUM" "$SAMPLES")
    AW_MEM_AVG=$(awk_avg "$AW_MEM_SUM" "$SAMPLES")
    AA_CPU_AVG=$(awk_avg "$AA_CPU_SUM" "$SAMPLES")
    AA_MEM_AVG=$(awk_avg "$AA_MEM_SUM" "$SAMPLES")
    PG_CPU_AVG=$(awk_avg "$PG_CPU_SUM" "$SAMPLES")
    PG_MEM_AVG=$(awk_avg "$PG_MEM_SUM" "$SAMPLES")
    # App only = Alon worker + API.
    A_APP_CPU_AVG=$(awk "BEGIN{printf \"%.1f\", ($AW_CPU_SUM + $AA_CPU_SUM) / $SAMPLES}")
    A_APP_MEM_AVG=$(awk "BEGIN{printf \"%.1f\", ($AW_MEM_SUM + $AA_MEM_SUM) / $SAMPLES}")
    A_APP_CPU_MAX=$(awk "BEGIN{print $AW_CPU_MAX + $AA_CPU_MAX}")
    A_APP_MEM_MAX=$(awk "BEGIN{print $AW_MEM_MAX + $AA_MEM_MAX}")
    # Full stack = Alon worker + API + Postgres.
    A_FULL_CPU_AVG=$(awk "BEGIN{printf \"%.1f\", ($AW_CPU_SUM + $AA_CPU_SUM + $PG_CPU_SUM) / $SAMPLES}")
    A_FULL_MEM_AVG=$(awk "BEGIN{printf \"%.1f\", ($AW_MEM_SUM + $AA_MEM_SUM + $PG_MEM_SUM) / $SAMPLES}")
    A_FULL_CPU_MAX=$(awk "BEGIN{print $AW_CPU_MAX + $AA_CPU_MAX + $PG_CPU_MAX}")
    A_FULL_MEM_MAX=$(awk "BEGIN{print $AW_MEM_MAX + $AA_MEM_MAX + $PG_MEM_MAX}")
    KS_CPU_AVG=$(awk_avg "$KS_CPU_SUM" "$SAMPLES")
    KS_MEM_AVG=$(awk_avg "$KS_MEM_SUM" "$SAMPLES")
    KM_CPU_AVG=$(awk_avg "$KM_CPU_SUM" "$SAMPLES")
    KM_MEM_AVG=$(awk_avg "$KM_MEM_SUM" "$SAMPLES")
    KDB_CPU_AVG=$(awk_avg "$KDB_CPU_SUM" "$SAMPLES")
    KDB_MEM_AVG=$(awk_avg "$KDB_MEM_SUM" "$SAMPLES")
    KM_FULL_CPU_AVG=$(awk "BEGIN{printf \"%.1f\", ($KM_CPU_SUM + $KDB_CPU_SUM) / $SAMPLES}")
    KM_FULL_MEM_AVG=$(awk "BEGIN{printf \"%.1f\", ($KM_MEM_SUM + $KDB_MEM_SUM) / $SAMPLES}")
    KM_FULL_CPU_MAX=$(awk "BEGIN{print $KM_CPU_MAX + $KDB_CPU_MAX}")
    KM_FULL_MEM_MAX=$(awk "BEGIN{print $KM_MEM_MAX + $KDB_MEM_MAX}")
else
    AW_CPU_AVG="N/A"; AW_MEM_AVG="N/A"
    AA_CPU_AVG="N/A"; AA_MEM_AVG="N/A"
    PG_CPU_AVG="N/A"; PG_MEM_AVG="N/A"
    A_APP_CPU_AVG="N/A"; A_APP_MEM_AVG="N/A"
    A_APP_CPU_MAX="N/A"; A_APP_MEM_MAX="N/A"
    A_FULL_CPU_AVG="N/A"; A_FULL_MEM_AVG="N/A"
    A_FULL_CPU_MAX="N/A"; A_FULL_MEM_MAX="N/A"
    KS_CPU_AVG="N/A"; KS_MEM_AVG="N/A"
    KM_CPU_AVG="N/A"; KM_MEM_AVG="N/A"
    KDB_CPU_AVG="N/A"; KDB_MEM_AVG="N/A"
    KM_FULL_CPU_AVG="N/A"; KM_FULL_MEM_AVG="N/A"
    KM_FULL_CPU_MAX="N/A"; KM_FULL_MEM_MAX="N/A"
fi

ALON_ERR=$(docker logs "$ALON_WORKER_CTR" --since "$START_KUMA" 2>&1 \
    | grep -c 'ERROR\|error\b' || true)

# ── Print comparison table ─────────────────────────────────────────────────────
LABEL="${MONITOR_COUNT} monitors / ${CHECK_INTERVAL}s interval"
RESULT_FILE="results/compare_${MONITOR_COUNT}m_$(date +%Y%m%d_%H%M%S).txt"

{
echo ""
printf '%*s\n' 78 '' | tr ' ' '='
printf "  %-74s\n" "Alon Sentinel vs Uptime Kuma variants - ${LABEL}"
printf "  %-74s\n" "Window: ${ACTUAL_SECS}s  Expected checks/tool: ${EXPECTED}"
printf '%*s\n' 78 '' | tr ' ' '='
echo ""
echo "  Workload"
printf "  %-24s  %-18s  %-18s  %-18s\n" "Metric" "Alon/Postgres" "Kuma/SQLite" "Kuma/MariaDB"
printf "  %-24s  %-18s  %-18s  %-18s\n" "------------------------" "------------------" "------------------" "------------------"
printf "  %-24s  %-18s  %-18s  %-18s\n" "Image" "local build" "$KUMA_SQLITE_IMAGE" "$KUMA_MARIADB_IMAGE"
printf "  %-24s  %-18s  %-18s  %-18s\n" "Checks completed" "$ALON_DONE" "$KUMA_SQLITE_DONE" "$KUMA_MARIADB_DONE"
printf "  %-24s  %-18s  %-18s  %-18s\n" "Expected checks" "$EXPECTED" "$EXPECTED" "$EXPECTED"
printf "  %-24s  %-18s  %-18s  %-18s\n" "Missed checks" "$ALON_MISSED ($ALON_MISS_PCT)" "$KUMA_SQLITE_MISSED ($KUMA_SQLITE_MISS_PCT)" "$KUMA_MARIADB_MISSED ($KUMA_MARIADB_MISS_PCT)"
echo ""

echo "  Alon component split"
printf "  %-24s  %-18s  %-18s\n" "Component" "CPU avg/max" "RAM avg/max"
printf "  %-24s  %-18s  %-18s\n" "------------------------" "------------------" "------------------"
printf "  %-24s  %-18s  %-18s\n" "API" "${AA_CPU_AVG}% / ${AA_CPU_MAX}%" "${AA_MEM_AVG} / ${AA_MEM_MAX} MiB"
printf "  %-24s  %-18s  %-18s\n" "Worker" "${AW_CPU_AVG}% / ${AW_CPU_MAX}%" "${AW_MEM_AVG} / ${AW_MEM_MAX} MiB"
printf "  %-24s  %-18s  %-18s\n" "Postgres" "${PG_CPU_AVG}% / ${PG_CPU_MAX}%" "${PG_MEM_AVG} / ${PG_MEM_MAX} MiB"
printf "  %-24s  %-18s  %-18s\n" "App subtotal" "${A_APP_CPU_AVG}% / ${A_APP_CPU_MAX}%" "${A_APP_MEM_AVG} / ${A_APP_MEM_MAX} MiB"
printf "  %-24s  %-18s  %-18s\n" "Full stack" "${A_FULL_CPU_AVG}% / ${A_FULL_CPU_MAX}%" "${A_FULL_MEM_AVG} / ${A_FULL_MEM_MAX} MiB"
echo ""

echo "  Kuma SQLite split"
printf "  %-24s  %-18s  %-18s\n" "Component" "CPU avg/max" "RAM avg/max"
printf "  %-24s  %-18s  %-18s\n" "------------------------" "------------------" "------------------"
printf "  %-24s  %-18s  %-18s\n" "App + SQLite" "${KS_CPU_AVG}% / ${KS_CPU_MAX}%" "${KS_MEM_AVG} / ${KS_MEM_MAX} MiB"
printf "  %-24s  %-18s  %-18s\n" "SQLite storage" "N/A" "${KUMA_SQLITE_SIZE_MIB:-N/A} MiB"
echo ""

echo "  Kuma MariaDB split"
printf "  %-24s  %-18s  %-18s\n" "Component" "CPU avg/max" "RAM avg/max"
printf "  %-24s  %-18s  %-18s\n" "------------------------" "------------------" "------------------"
printf "  %-24s  %-18s  %-18s\n" "Kuma app" "${KM_CPU_AVG}% / ${KM_CPU_MAX}%" "${KM_MEM_AVG} / ${KM_MEM_MAX} MiB"
printf "  %-24s  %-18s  %-18s\n" "MariaDB" "${KDB_CPU_AVG}% / ${KDB_CPU_MAX}%" "${KDB_MEM_AVG} / ${KDB_MEM_MAX} MiB"
printf "  %-24s  %-18s  %-18s\n" "Full stack" "${KM_FULL_CPU_AVG}% / ${KM_FULL_CPU_MAX}%" "${KM_FULL_MEM_AVG} / ${KM_FULL_MEM_MAX} MiB"
printf "  %-24s  %-18s  %-18s\n" "MariaDB storage" "N/A" "${KUMA_MARIADB_SIZE_MIB:-N/A} MiB"
echo ""

echo "  Full-stack summary"
printf "  %-24s  %-18s  %-18s  %-18s\n" "Metric" "Alon/Postgres" "Kuma/SQLite" "Kuma/MariaDB"
printf "  %-24s  %-18s  %-18s  %-18s\n" "------------------------" "------------------" "------------------" "------------------"
printf "  %-24s  %-18s  %-18s  %-18s\n" "CPU avg/max" "${A_FULL_CPU_AVG}% / ${A_FULL_CPU_MAX}%" "${KS_CPU_AVG}% / ${KS_CPU_MAX}%" "${KM_FULL_CPU_AVG}% / ${KM_FULL_CPU_MAX}%"
printf "  %-24s  %-18s  %-18s  %-18s\n" "RAM avg/max" "${A_FULL_MEM_AVG} / ${A_FULL_MEM_MAX} MiB" "${KS_MEM_AVG} / ${KS_MEM_MAX} MiB" "${KM_FULL_MEM_AVG} / ${KM_FULL_MEM_MAX} MiB"
printf "  %-24s  %-18s  %-18s  %-18s\n" "DB storage size" "N/A" "${KUMA_SQLITE_SIZE_MIB:-N/A} MiB" "${KUMA_MARIADB_SIZE_MIB:-N/A} MiB"
printf "  %-24s  %-18s  %-18s  %-18s\n" "Open incidents" "$ALON_INCIDENTS" "N/A" "N/A"
printf "  %-24s  %-18s  %-18s  %-18s\n" "Duplicate incidents" "$ALON_DUPS (want 0)" "N/A" "N/A"
printf "  %-24s  %-18s  %-18s  %-18s\n" "Worker errors" "$ALON_ERR" "N/A" "N/A"
echo ""
echo "  Notes:"
echo "  - App-only Alon = worker + API."
echo "  - Full-stack Alon = worker + API + Postgres."
echo "  - Alon API memory includes its configured database connection pool."
echo "  - Kuma/SQLite stores SQLite inside the application container; app and DB RAM cannot be split."
echo "  - Kuma/MariaDB full-stack = Kuma application container + MariaDB container."
echo ""
} | tee "$RESULT_FILE"

log "Results saved to $RESULT_FILE"
