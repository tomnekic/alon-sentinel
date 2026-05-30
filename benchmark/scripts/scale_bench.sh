#!/usr/bin/env bash
# Alon Sentinel worker scale benchmark.
#
# Examples:
#   bash scripts/scale_bench.sh --scenario 5
#   bash scripts/scale_bench.sh --monitors 3000 --interval 60 --duration 900 --warmup 60
#   bash scripts/scale_bench.sh --monitors 1000 --target-path fail --name "1000 monitors / fail"
#   bash scripts/scale_bench.sh --skip-storm
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT_DIR"

if [[ -f ".env" ]]; then set -a; source ".env"; set +a; fi

ALON_URL="${ALON_URL:-http://localhost:3100}"
ADMIN_EMAIL="${ALON_ADMIN_EMAIL:-bench@bench.local}"
ADMIN_PASSWORD="${ALON_ADMIN_PASSWORD:-Bench1234!}"
CHECK_INTERVAL="${CHECK_INTERVAL:-60}"
MEASURE_DURATION="${SCALE_MEASURE_DURATION:-540}"
WARMUP_SECONDS="${WARMUP_SECONDS:-15}"
SEED_PARALLEL="${SEED_PARALLEL:-20}"
SAMPLE_INTERVAL="${SAMPLE_INTERVAL:-15}"

COMPOSE_PROJECT="benchmark"
export ALON_PG_CTR="${COMPOSE_PROJECT}-postgres-1"
export ALON_WORKER_CTR="${COMPOSE_PROJECT}-alon-worker-1"
export ALON_API_CTR="${COMPOSE_PROJECT}-alon-api-1"

ONLY_STORM=false
SKIP_STORM=false
SCENARIO_NUM=""
PRINT_SUMMARY=false
CUSTOM_MONITORS=""
CUSTOM_TARGET_PATH="ok"
CUSTOM_NAME=""
CUSTOM_STORM=false

log() { echo "[scale-bench] $*"; }
hdr() {
    echo ""
    printf '%*s\n' 72 '' | tr ' ' '='
    echo "  $*"
    printf '%*s\n' 72 '' | tr ' ' '='
}

usage() {
    cat <<'USAGE'
Usage:
  bash scripts/scale_bench.sh [--scenario N]
  bash scripts/scale_bench.sh --monitors N [--target-path ok|fail] [--interval S] [--duration S] [--warmup S]
  bash scripts/scale_bench.sh --skip-storm
  bash scripts/scale_bench.sh --only-storm
  bash scripts/scale_bench.sh --print-summary

Fixed scenarios:
  1 = 100 monitors / ok
  2 = 500 monitors / ok
  3 = 1000 monitors / ok
  4 = 1000 monitors / fail storm
  5 = 5000 monitors / ok
  6 = 10000 monitors / ok
  7 = 10000 monitors / fail storm
USAGE
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --only-storm) ONLY_STORM=true ;;
        --skip-storm) SKIP_STORM=true ;;
        --print-summary) PRINT_SUMMARY=true ;;
        --scenario) shift; SCENARIO_NUM="${1:-}" ;;
        --scenario=*) SCENARIO_NUM="${1#--scenario=}" ;;
        --monitors) shift; CUSTOM_MONITORS="${1:-}" ;;
        --monitors=*) CUSTOM_MONITORS="${1#--monitors=}" ;;
        --target-path) shift; CUSTOM_TARGET_PATH="${1:-}" ;;
        --target-path=*) CUSTOM_TARGET_PATH="${1#--target-path=}" ;;
        --name) shift; CUSTOM_NAME="${1:-}" ;;
        --name=*) CUSTOM_NAME="${1#--name=}" ;;
        --fail|--storm) CUSTOM_TARGET_PATH="fail"; CUSTOM_STORM=true ;;
        --interval) shift; CHECK_INTERVAL="${1:-}" ;;
        --interval=*) CHECK_INTERVAL="${1#--interval=}" ;;
        --duration) shift; MEASURE_DURATION="${1:-}" ;;
        --duration=*) MEASURE_DURATION="${1#--duration=}" ;;
        --warmup) shift; WARMUP_SECONDS="${1:-}" ;;
        --warmup=*) WARMUP_SECONDS="${1#--warmup=}" ;;
        --seed-parallel) shift; SEED_PARALLEL="${1:-}" ;;
        --seed-parallel=*) SEED_PARALLEL="${1#--seed-parallel=}" ;;
        --sample-interval) shift; SAMPLE_INTERVAL="${1:-}" ;;
        --sample-interval=*) SAMPLE_INTERVAL="${1#--sample-interval=}" ;;
        --help|-h) usage; exit 0 ;;
        *) log "Unknown argument: $1"; usage; exit 1 ;;
    esac
    shift
done

mkdir -p results

do_login() {
    local resp code
    resp=$(curl -s -D - -X POST "$ALON_URL/v1/admin/auth/login" \
        -H "Content-Type: application/json" \
        -d "{\"email\":\"$ADMIN_EMAIL\",\"password\":\"$ADMIN_PASSWORD\"}" 2>/dev/null)
    code=$(echo "$resp" | head -1 | grep -o '[0-9][0-9][0-9]' | head -1)
    if [[ "$code" != "200" ]]; then
        echo "$resp"
        log "ERROR: Login returned HTTP $code"
        exit 1
    fi
    COOKIE=$(echo "$resp" | grep -i "set-cookie:" | sed -n 's/.*admin_session=\([^;]*\).*/\1/p' | tr -d '\r')
    if [[ -z "$COOKIE" ]]; then
        log "ERROR: No session cookie"
        exit 1
    fi
    log "Logged in."
}

reset_db() {
    log "Resetting DB (TRUNCATE sites CASCADE) ..."
    if ! docker exec "$ALON_PG_CTR" psql -U alon -d alon_bench_db -q \
            -c "TRUNCATE sites CASCADE;" 2>&1; then
        log "WARNING: TRUNCATE failed, trying DELETE fallback ..."
        docker exec "$ALON_PG_CTR" psql -U alon -d alon_bench_db -q \
            -c "DELETE FROM site_monitor_checks; DELETE FROM site_monitors; DELETE FROM sites;" 2>&1 || true
    fi
    log "DB reset complete."
}

seed_monitors() {
    local count="$1" target_path="$2"
    local target_url="http://target:8088/${target_path}"

    log "Seeding $count monitors -> $target_url (${SEED_PARALLEL} parallel) ..."

    local ok=0 skipped=0
    local -a pids=()
    local tmpdir
    tmpdir=$(mktemp -d)

    seed_one() {
        local i="$1" out_file="$2"
        local site_resp site_id mon_code

        site_resp=$(curl -s -X POST "$ALON_URL/v1/sites" \
            -b "admin_session=$COOKIE" \
            -H "Content-Type: application/json" \
            -d "{\"name\":\"Scale Site $i\",\"base_url\":\"http://scale-$i.bench.local\"}" \
            2>/dev/null || true)

        site_id=$(echo "$site_resp" | grep -o '"id":[0-9]*' | head -1 | grep -o '[0-9]*' || true)
        if [[ -z "$site_id" ]]; then
            echo "skip" > "$out_file"
            return
        fi

        mon_code=$(curl -s -o /dev/null -w "%{http_code}" \
            -X PUT "$ALON_URL/v1/sites/$site_id/monitoring/http" \
            -b "admin_session=$COOKIE" \
            -H "Content-Type: application/json" \
            -d "{
              \"target_url\": \"$target_url\",
              \"check_interval_seconds\": $CHECK_INTERVAL,
              \"expected_status_code\": 200,
              \"is_active\": true
            }" 2>/dev/null || echo "000")

        if [[ "$mon_code" =~ ^2 ]]; then
            echo "ok" > "$out_file"
        else
            echo "skip" > "$out_file"
        fi
    }
    export -f seed_one
    export ALON_URL COOKIE CHECK_INTERVAL

    for i in $(seq 1 "$count"); do
        local out_file="$tmpdir/$i"
        seed_one "$i" "$out_file" &
        pids+=($!)

        if (( ${#pids[@]} >= SEED_PARALLEL )); then
            wait "${pids[0]}"
            pids=("${pids[@]:1}")
        fi
    done
    wait "${pids[@]}"

    for i in $(seq 1 "$count"); do
        local result
        result=$(cat "$tmpdir/$i" 2>/dev/null || echo "skip")
        if [[ "$result" == "ok" ]]; then
            ok=$(( ok + 1 ))
        else
            skipped=$(( skipped + 1 ))
        fi
    done
    rm -rf "$tmpdir"

    log "Seeding done: $ok created, $skipped skipped."
    if (( ok == 0 )); then
        log "ERROR: No monitors were created!"
        exit 1
    fi
    SEEDED_MONITOR_COUNT="$ok"
}

run_scenario() {
    local name="$1" count="$2" target_path="$3" is_storm="${4:-false}"

    hdr "Scenario: $name"
    reset_db
    do_login
    seed_monitors "$count" "$target_path"

    log "Waiting ${WARMUP_SECONDS}s for workers to claim and run first cycle ..."
    sleep "$WARMUP_SECONDS"

    local label result_file
    label=$(echo "$name" | tr ' /,' '---' | tr -s '-' | tr -cd '[:alnum:]_.-')
    result_file="results/scale_${label}_$(date +%Y%m%d_%H%M%S).txt"

    SCENARIO="$name" \
    MONITOR_COUNT="$count" \
    SEEDED_MONITOR_COUNT="$SEEDED_MONITOR_COUNT" \
    CHECK_INTERVAL="$CHECK_INTERVAL" \
    MEASURE_DURATION="$MEASURE_DURATION" \
    SAMPLE_INTERVAL="$SAMPLE_INTERVAL" \
    IS_FAILURE_STORM="$is_storm" \
    RESULT_FILE="$result_file" \
        bash "$SCRIPT_DIR/measure_worker.sh"
}

if ! $PRINT_SUMMARY; then
    log "Waiting for Alon API at $ALON_URL ..."
    for i in $(seq 1 20); do
        if curl -sf "$ALON_URL/health" -o /dev/null 2>/dev/null; then
            log "API ready."
            break
        fi
        [[ $i -eq 20 ]] && { log "ERROR: API not ready"; exit 1; }
        sleep 3
    done
fi

run_s1() { run_scenario "100 monitors / ok" 100 "ok"; }
run_s2() { run_scenario "500 monitors / ok" 500 "ok"; }
run_s3() { run_scenario "1000 monitors / ok" 1000 "ok"; }
run_s4() { run_scenario "1000 monitors / fail (storm)" 1000 "fail" "true"; }
run_s5() { run_scenario "5000 monitors / ok" 5000 "ok"; }
run_s6() { run_scenario "10000 monitors / ok" 10000 "ok"; }
run_s7() { run_scenario "10000 monitors / fail (storm)" 10000 "fail" "true"; }

if $PRINT_SUMMARY; then
    :
elif [[ -n "$CUSTOM_MONITORS" ]]; then
    if [[ -z "$CUSTOM_NAME" ]]; then
        CUSTOM_NAME="${CUSTOM_MONITORS} monitors / ${CUSTOM_TARGET_PATH}"
        $CUSTOM_STORM && CUSTOM_NAME="${CUSTOM_NAME} (storm)"
    fi
    run_scenario "$CUSTOM_NAME" "$CUSTOM_MONITORS" "$CUSTOM_TARGET_PATH" "$CUSTOM_STORM"
    exit 0
elif [[ -n "$SCENARIO_NUM" ]]; then
    case "$SCENARIO_NUM" in
        1) run_s1 ;;
        2) run_s2 ;;
        3) run_s3 ;;
        4) run_s4 ;;
        5) run_s5 ;;
        6) run_s6 ;;
        7) run_s7 ;;
        *) log "Unknown scenario $SCENARIO_NUM (use 1-7)"; exit 1 ;;
    esac
    exit 0
elif $ONLY_STORM; then
    run_s4
elif $SKIP_STORM; then
    run_s1; run_s2; run_s3
else
    run_s1; run_s2; run_s3; run_s4
fi

hdr "Scale Benchmark Complete"

SUMMARY_FILE="results/scale_summary_$(date +%Y%m%d_%H%M%S).txt"
{
echo ""
echo "Alon Sentinel - Worker Scale Benchmark Summary"
echo "$(date)"
echo ""
printf "  %-28s  %9s  %9s  %9s  %6s  %9s  %9s  %9s  %9s\n" \
    "Scenario" "Completed" "Expected" "Missed" "Miss%" "W-CPU%" "W-RAM" "FullCPU%" "FullRAM"
printf "  %-28s  %9s  %9s  %9s  %6s  %9s  %9s  %9s  %9s\n" \
    "----------------------------" "---------" "---------" "---------" "------" "---------" "---------" "---------" "---------"

for f in results/scale_*.txt; do
    [[ -f "$f" ]] || continue
    [[ "$f" == results/scale_summary_* ]] && continue
    SCEN=$(grep -m1 "Scenario:" "$f" 2>/dev/null | sed 's/.*Scenario: //' | sed 's/ *|//' | xargs || echo "?")
    DONE=$(grep "Checks completed" "$f" 2>/dev/null | awk '{print $NF}' || echo "?")
    EXP=$(grep "Expected checks" "$f" 2>/dev/null | awk '{print $NF}' || echo "?")
    MISS=$(grep "Missed checks" "$f" 2>/dev/null | awk '{print $3}' || echo "?")
    PCT=$(grep "Missed checks" "$f" 2>/dev/null | grep -o '([0-9.]*%)' | tr -d '()' || echo "?")
    WCPU=$(awk '$1=="Worker"{print $2; exit}' "$f" 2>/dev/null || echo "?")
    WRAM=$(awk '$1=="Worker"{print $5; exit}' "$f" 2>/dev/null || echo "?")
    FCPU=$(awk '$1=="Full" && $2=="stack"{print $3; exit}' "$f" 2>/dev/null || echo "?")
    FRAM=$(awk '$1=="Full" && $2=="stack"{print $6; exit}' "$f" 2>/dev/null || echo "?")
    printf "  %-28s  %9s  %9s  %9s  %6s  %9s  %9s  %9s  %9s\n" \
        "${SCEN:0:28}" "$DONE" "$EXP" "$MISS" "$PCT" "$WCPU" "$WRAM" "$FCPU" "$FRAM"
done
echo ""
echo "Full per-scenario reports: results/scale_*.txt"
echo "Raw sample CSV files: results/scale_*.samples.csv"
echo ""
} | tee "$SUMMARY_FILE"

log "Summary saved to $SUMMARY_FILE"
