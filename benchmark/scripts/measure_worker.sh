#!/usr/bin/env bash
# Measure Alon worker throughput and full-stack resource usage for one scenario.
#
# Required env vars set by caller:
#   SCENARIO
#   MONITOR_COUNT
#   CHECK_INTERVAL
#   MEASURE_DURATION
#   RESULT_FILE
#
# Optional env vars:
#   IS_FAILURE_STORM
#   SEEDED_MONITOR_COUNT
#   SAMPLE_INTERVAL
#   RAW_SAMPLE_FILE
#
# Reads container names from:
#   ALON_PG_CTR, ALON_WORKER_CTR, ALON_API_CTR
set -euo pipefail

MEASURE_DURATION="${MEASURE_DURATION:-600}"
IS_FAILURE_STORM="${IS_FAILURE_STORM:-false}"
SAMPLE_INTERVAL="${SAMPLE_INTERVAL:-15}"
RAW_SAMPLE_FILE="${RAW_SAMPLE_FILE:-${RESULT_FILE%.txt}.samples.csv}"

log() { echo "[measure] $*"; }

pg() {
    docker exec "$ALON_PG_CTR" psql -U alon -d alon_bench_db -tAq -c "$1" 2>/dev/null \
        | tr -d '\n\r' | xargs
}

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

awk_add() { awk "BEGIN{print ($1)+($2)}"; }
awk_avg() { awk "BEGIN{printf \"%.1f\", ($1)/($2)}"; }
awk_max() { awk "BEGIN{v=(($1)>($2))?($1):($2); print v}"; }

command_or_na() {
    "$@" 2>/dev/null || echo "N/A"
}

container_image() {
    docker inspect --format '{{.Config.Image}}' "$1" 2>/dev/null || echo "N/A"
}

container_id_short() {
    docker inspect --format '{{.Id}}' "$1" 2>/dev/null | cut -c1-12 || echo "N/A"
}

host_cpu_model() {
    if command -v lscpu >/dev/null 2>&1; then
        lscpu 2>/dev/null | awk -F: '/Model name/{sub(/^[ \t]+/, "", $2); print $2; exit}'
    elif [[ -r /proc/cpuinfo ]]; then
        awk -F: '/model name/{sub(/^[ \t]+/, "", $2); print $2; exit}' /proc/cpuinfo
    else
        echo "N/A"
    fi
}

host_total_ram_mib() {
    if [[ -r /proc/meminfo ]]; then
        awk '/MemTotal/{printf "%.0f", $2/1024}' /proc/meminfo
    else
        echo "N/A"
    fi
}

START_TS=$(pg "SELECT NOW();")
START_DB_BYTES=$(pg "SELECT pg_database_size('alon_bench_db');")
START_CHECKS=$(pg "SELECT COUNT(*) FROM site_monitor_checks;")
START_INCIDENTS=$(pg "SELECT COUNT(*) FROM site_monitor_incidents;")
ACTIVE_MONITORS_START=$(pg "SELECT COUNT(*) FROM site_monitors WHERE is_active = true;")
START_WALL=$(date +%s)

MEASURED_MONITORS="${SEEDED_MONITOR_COUNT:-${ACTIVE_MONITORS_START:-$MONITOR_COUNT}}"
WORKER_LOG_START_TS=$(date -u +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null || date -u +"%Y-%m-%dT%H:%M:%SZ")

log "Scenario: ${SCENARIO}"
log "Measuring for ${MEASURE_DURATION}s (${SAMPLE_INTERVAL}s samples) ..."
log "Start: ${START_TS}  DB: $(awk "BEGIN{printf \"%.1f MiB\", $START_DB_BYTES/1048576}")"

mkdir -p "$(dirname "$RAW_SAMPLE_FILE")"
printf "elapsed_seconds,api_cpu_pct,api_mem_mib,worker_cpu_pct,worker_mem_mib,postgres_cpu_pct,postgres_mem_mib,app_cpu_pct,app_mem_mib,full_stack_cpu_pct,full_stack_mem_mib\n" > "$RAW_SAMPLE_FILE"

API_CPU_SUM=0; API_CPU_MAX=0
API_MEM_SUM=0; API_MEM_MAX=0
WRK_CPU_SUM=0; WRK_CPU_MAX=0
WRK_MEM_SUM=0; WRK_MEM_MAX=0
PG_CPU_SUM=0; PG_CPU_MAX=0
PG_MEM_SUM=0; PG_MEM_MAX=0
APP_CPU_SUM=0; APP_CPU_MAX=0
APP_MEM_SUM=0; APP_MEM_MAX=0
FULL_CPU_SUM=0; FULL_CPU_MAX=0
FULL_MEM_SUM=0; FULL_MEM_MAX=0
TOTAL_SAMPLES=0

elapsed=0
while (( elapsed < MEASURE_DURATION )); do
    sleep "$SAMPLE_INTERVAL"
    elapsed=$(( elapsed + SAMPLE_INTERVAL ))

    A_CPU=$(container_cpu "$ALON_API_CTR")
    A_MEM=$(container_mem_mib "$ALON_API_CTR")
    W_CPU=$(container_cpu "$ALON_WORKER_CTR")
    W_MEM=$(container_mem_mib "$ALON_WORKER_CTR")
    P_CPU=$(container_cpu "$ALON_PG_CTR")
    P_MEM=$(container_mem_mib "$ALON_PG_CTR")

    APP_CPU=$(awk_add "${A_CPU:-0}" "${W_CPU:-0}")
    APP_MEM=$(awk_add "${A_MEM:-0}" "${W_MEM:-0}")
    FULL_CPU=$(awk_add "$APP_CPU" "${P_CPU:-0}")
    FULL_MEM=$(awk_add "$APP_MEM" "${P_MEM:-0}")

    API_CPU_SUM=$(awk_add "$API_CPU_SUM" "${A_CPU:-0}")
    API_MEM_SUM=$(awk_add "$API_MEM_SUM" "${A_MEM:-0}")
    API_CPU_MAX=$(awk_max "${A_CPU:-0}" "$API_CPU_MAX")
    API_MEM_MAX=$(awk_max "${A_MEM:-0}" "$API_MEM_MAX")

    WRK_CPU_SUM=$(awk_add "$WRK_CPU_SUM" "${W_CPU:-0}")
    WRK_MEM_SUM=$(awk_add "$WRK_MEM_SUM" "${W_MEM:-0}")
    WRK_CPU_MAX=$(awk_max "${W_CPU:-0}" "$WRK_CPU_MAX")
    WRK_MEM_MAX=$(awk_max "${W_MEM:-0}" "$WRK_MEM_MAX")

    PG_CPU_SUM=$(awk_add "$PG_CPU_SUM" "${P_CPU:-0}")
    PG_MEM_SUM=$(awk_add "$PG_MEM_SUM" "${P_MEM:-0}")
    PG_CPU_MAX=$(awk_max "${P_CPU:-0}" "$PG_CPU_MAX")
    PG_MEM_MAX=$(awk_max "${P_MEM:-0}" "$PG_MEM_MAX")

    APP_CPU_SUM=$(awk_add "$APP_CPU_SUM" "$APP_CPU")
    APP_MEM_SUM=$(awk_add "$APP_MEM_SUM" "$APP_MEM")
    APP_CPU_MAX=$(awk_max "$APP_CPU" "$APP_CPU_MAX")
    APP_MEM_MAX=$(awk_max "$APP_MEM" "$APP_MEM_MAX")

    FULL_CPU_SUM=$(awk_add "$FULL_CPU_SUM" "$FULL_CPU")
    FULL_MEM_SUM=$(awk_add "$FULL_MEM_SUM" "$FULL_MEM")
    FULL_CPU_MAX=$(awk_max "$FULL_CPU" "$FULL_CPU_MAX")
    FULL_MEM_MAX=$(awk_max "$FULL_MEM" "$FULL_MEM_MAX")

    TOTAL_SAMPLES=$(( TOTAL_SAMPLES + 1 ))
    printf "%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n" \
        "$elapsed" "${A_CPU:-0}" "${A_MEM:-0}" "${W_CPU:-0}" "${W_MEM:-0}" \
        "${P_CPU:-0}" "${P_MEM:-0}" "$APP_CPU" "$APP_MEM" "$FULL_CPU" "$FULL_MEM" \
        >> "$RAW_SAMPLE_FILE"

    log "  ${elapsed}s - worker CPU: ${W_CPU}% mem: ${W_MEM} MiB; full stack CPU: ${FULL_CPU}% mem: ${FULL_MEM} MiB"
done

END_WALL=$(date +%s)
ACTUAL_SECS=$(( END_WALL - START_WALL ))

log "Querying final metrics ..."

END_DB_BYTES=$(pg "SELECT pg_database_size('alon_bench_db');")
END_CHECKS=$(pg "SELECT COUNT(*) FROM site_monitor_checks;")
END_INCIDENTS=$(pg "SELECT COUNT(*) FROM site_monitor_incidents;")
ACTIVE_MONITORS_END=$(pg "SELECT COUNT(*) FROM site_monitors WHERE is_active = true;")

CHECKS_DONE=$(pg "SELECT COUNT(*) FROM site_monitor_checks WHERE checked_at >= '${START_TS}';")
INCIDENTS_OPENED=$(pg "SELECT COUNT(*) FROM site_monitor_incidents WHERE status='open' AND opened_at >= '${START_TS}';")
OPEN_INCIDENTS=$(pg "SELECT COUNT(*) FROM site_monitor_incidents WHERE status='open';")
DUP_INCIDENTS=$(pg "SELECT COUNT(*) FROM (
    SELECT site_monitor_id FROM site_monitor_incidents WHERE status='open'
    GROUP BY site_monitor_id HAVING COUNT(*) > 1
) t;")

WORKER_ERRORS=$(docker logs "$ALON_WORKER_CTR" --since "$WORKER_LOG_START_TS" 2>&1 \
    | grep -c 'ERROR\|error\b' || true)

MEASURED_MONITORS="${SEEDED_MONITOR_COUNT:-${ACTIVE_MONITORS_END:-$MEASURED_MONITORS}}"
EXPECTED=$(awk "BEGIN{printf \"%d\", int($MEASURED_MONITORS * $ACTUAL_SECS / $CHECK_INTERVAL)}")
MISSED=$(awk "BEGIN{m=$EXPECTED - $CHECKS_DONE; print (m>0)?m:0}")
MISS_PCT=$(awk "BEGIN{e=$EXPECTED; m=$MISSED; if(e>0) printf \"%.1f%%\", m/e*100; else printf \"0.0%%\"}")

DB_GROWTH_BYTES=$(awk "BEGIN{print $END_DB_BYTES - $START_DB_BYTES}")
DB_GROWTH_HUMAN=$(awk "BEGIN{
    g=$DB_GROWTH_BYTES
    if(g>=1048576) printf \"%.1f MiB\", g/1048576
    else printf \"%.0f KiB\", g/1024
}")
DB_SIZE_END_HUMAN=$(awk "BEGIN{printf \"%.1f MiB\", $END_DB_BYTES/1048576}")

WRITES=$(awk "BEGIN{print ($END_CHECKS - $START_CHECKS) + ($END_INCIDENTS - $START_INCIDENTS)}")
WRITES_SEC=$(awk "BEGIN{printf \"%.1f\", $WRITES / $ACTUAL_SECS}")
INCIDENT_RATE=$(awk "BEGIN{printf \"%.1f/min\", $INCIDENTS_OPENED / ($ACTUAL_SECS/60)}")

if (( TOTAL_SAMPLES > 0 )); then
    API_CPU_AVG=$(awk_avg "$API_CPU_SUM" "$TOTAL_SAMPLES")
    API_MEM_AVG=$(awk_avg "$API_MEM_SUM" "$TOTAL_SAMPLES")
    WRK_CPU_AVG=$(awk_avg "$WRK_CPU_SUM" "$TOTAL_SAMPLES")
    WRK_MEM_AVG=$(awk_avg "$WRK_MEM_SUM" "$TOTAL_SAMPLES")
    PG_CPU_AVG=$(awk_avg "$PG_CPU_SUM" "$TOTAL_SAMPLES")
    PG_MEM_AVG=$(awk_avg "$PG_MEM_SUM" "$TOTAL_SAMPLES")
    APP_CPU_AVG=$(awk_avg "$APP_CPU_SUM" "$TOTAL_SAMPLES")
    APP_MEM_AVG=$(awk_avg "$APP_MEM_SUM" "$TOTAL_SAMPLES")
    FULL_CPU_AVG=$(awk_avg "$FULL_CPU_SUM" "$TOTAL_SAMPLES")
    FULL_MEM_AVG=$(awk_avg "$FULL_MEM_SUM" "$TOTAL_SAMPLES")
else
    API_CPU_AVG="N/A"; API_MEM_AVG="N/A"
    WRK_CPU_AVG="N/A"; WRK_MEM_AVG="N/A"
    PG_CPU_AVG="N/A"; PG_MEM_AVG="N/A"
    APP_CPU_AVG="N/A"; APP_MEM_AVG="N/A"
    FULL_CPU_AVG="N/A"; FULL_MEM_AVG="N/A"
fi

DOCKER_VERSION=$(command_or_na docker version --format '{{.Server.Version}}')
COMPOSE_VERSION=$(command_or_na docker compose version --short)
HOST_CPU_MODEL=$(host_cpu_model)
HOST_CPU_COUNT=$(command_or_na nproc)
HOST_RAM_MIB=$(host_total_ram_mib)
POSTGRES_VERSION=$(pg "SHOW server_version;")
API_IMAGE=$(container_image "$ALON_API_CTR")
WORKER_IMAGE=$(container_image "$ALON_WORKER_CTR")
POSTGRES_IMAGE=$(container_image "$ALON_PG_CTR")
API_CONTAINER_ID=$(container_id_short "$ALON_API_CTR")
WORKER_CONTAINER_ID=$(container_id_short "$ALON_WORKER_CTR")
POSTGRES_CONTAINER_ID=$(container_id_short "$ALON_PG_CTR")

W=$(printf '=%.0s' {1..70})
{
echo ""
echo "+${W}+"
printf "|  %-67s|\n" "Scenario: ${SCENARIO}"
printf "|  %-67s|\n" "Window: ${ACTUAL_SECS}s  Monitors: ${MEASURED_MONITORS}  Interval: ${CHECK_INTERVAL}s"
echo "+${W}+"
echo ""
printf "  %-32s  %s\n"  "Metric"             "Value"
printf "  %-32s  %s\n"  "--------------------------------"  "--------------------"
printf "  %-32s  %s\n"  "Checks completed"    "$CHECKS_DONE"
printf "  %-32s  %s\n"  "Expected checks"     "$EXPECTED"
printf "  %-32s  %s\n"  "Requested monitors"  "$MONITOR_COUNT"
printf "  %-32s  %s\n"  "Measured monitors"   "$MEASURED_MONITORS"
printf "  %-32s  %s  (%s)\n" "Missed checks"  "$MISSED" "$MISS_PCT"
printf "  %-32s  %s\n"  "Open incidents"      "$OPEN_INCIDENTS"
printf "  %-32s  %s\n"  "Incidents opened"    "$INCIDENTS_OPENED"
printf "  %-32s  %s\n"  "Incident rate"       "$INCIDENT_RATE"
printf "  %-32s  %s\n"  "Duplicate incidents" "$DUP_INCIDENTS  (should be 0)"
printf "  %-32s  %s\n"  "DB writes"           "$WRITES  (${WRITES_SEC}/sec)"
printf "  %-32s  %s\n"  "DB size (end)"       "$DB_SIZE_END_HUMAN  (+${DB_GROWTH_HUMAN})"
printf "  %-32s  %s\n"  "Worker errors"       "$WORKER_ERRORS"
printf "  %-32s  %s\n"  "Raw samples CSV"     "$RAW_SAMPLE_FILE"
echo ""
printf "  %-32s  %-18s  %-18s\n" "Component" "CPU avg/max" "RAM avg/max"
printf "  %-32s  %-18s  %-18s\n" "--------------------------------" "------------------" "------------------"
printf "  %-32s  %-18s  %-18s\n" "API"        "${API_CPU_AVG}% / ${API_CPU_MAX}%"      "${API_MEM_AVG} / ${API_MEM_MAX} MiB"
printf "  %-32s  %-18s  %-18s\n" "Worker"     "${WRK_CPU_AVG}% / ${WRK_CPU_MAX}%"      "${WRK_MEM_AVG} / ${WRK_MEM_MAX} MiB"
printf "  %-32s  %-18s  %-18s\n" "Postgres"   "${PG_CPU_AVG}% / ${PG_CPU_MAX}%"        "${PG_MEM_AVG} / ${PG_MEM_MAX} MiB"
printf "  %-32s  %-18s  %-18s\n" "App subtotal" "${APP_CPU_AVG}% / ${APP_CPU_MAX}%"  "${APP_MEM_AVG} / ${APP_MEM_MAX} MiB"
printf "  %-32s  %-18s  %-18s\n" "Full stack" "${FULL_CPU_AVG}% / ${FULL_CPU_MAX}%"  "${FULL_MEM_AVG} / ${FULL_MEM_MAX} MiB"
echo ""
printf "  %-32s  %s\n"  "Environment"         "Value"
printf "  %-32s  %s\n"  "--------------------------------"  "--------------------"
printf "  %-32s  %s\n"  "Host CPU"            "$HOST_CPU_MODEL"
printf "  %-32s  %s\n"  "Host CPU count"      "$HOST_CPU_COUNT"
printf "  %-32s  %s MiB\n" "Host RAM"           "$HOST_RAM_MIB"
printf "  %-32s  %s\n"  "Docker server"       "$DOCKER_VERSION"
printf "  %-32s  %s\n"  "Docker Compose"      "$COMPOSE_VERSION"
printf "  %-32s  %s\n"  "Postgres version"    "$POSTGRES_VERSION"
printf "  %-32s  %s (%s)\n" "API image/id"       "$API_IMAGE" "$API_CONTAINER_ID"
printf "  %-32s  %s (%s)\n" "Worker image/id"    "$WORKER_IMAGE" "$WORKER_CONTAINER_ID"
printf "  %-32s  %s (%s)\n" "Postgres image/id"  "$POSTGRES_IMAGE" "$POSTGRES_CONTAINER_ID"
echo ""
} | tee "${RESULT_FILE}"

log "Results saved to ${RESULT_FILE}"
log "Raw samples saved to ${RAW_SAMPLE_FILE}"
