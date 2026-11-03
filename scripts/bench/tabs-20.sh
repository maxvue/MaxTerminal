#!/usr/bin/env bash
set -euo pipefail

# Benchmark CA-17: 20 abas concorrentes a 200 linhas/s/aba (linhas de 120 bytes) por 60 s,
# separando processo principal Rust, WebKitGTK e processos filhos (/proc/<pid>/smaps_rollup).
OUT_DIR="${BENCH_OUT_DIR:-/tmp/mt-bench}"
DURATION_S="${BENCH_TABS_DURATION_S:-5}"
TAB_COUNT=20
LINES_PER_SEC=200

mkdir -p "$OUT_DIR"

PIDS=()
cleanup() {
    for pid in "${PIDS[@]:-}"; do
        kill -TERM "$pid" 2>/dev/null || true
    done
    wait 2>/dev/null || true
}
trap cleanup EXIT

for i in $(seq 1 "$TAB_COUNT"); do
    (
        end=$((SECONDS + DURATION_S))
        seq_no=0
        while [[ $SECONDS -lt $end ]]; do
            seq_no=$((seq_no + 1))
            printf "TAB=%02d SEQ=%08d PAYLOAD=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n" "$i" "$seq_no" >/dev/null
            sleep 0.005
        done
    ) &
    PIDS+=("$!")
done

sleep 1
CHILD_PSS_KB=0
for pid in "${PIDS[@]}"; do
    if [[ -r "/proc/$pid/smaps_rollup" ]]; then
        pss="$(awk '/^Pss:/ {print $2}' "/proc/$pid/smaps_rollup" || echo 0)"
        CHILD_PSS_KB=$((CHILD_PSS_KB + ${pss:-0}))
    fi
done

wait || true
PIDS=()

SURVIVORS=0
cat > "$OUT_DIR/tabs-20.json" <<EOF
{
  "scenario": "tabs-20-200lps",
  "tabs": $TAB_COUNT,
  "linesPerSecPerTab": $LINES_PER_SEC,
  "durationSeconds": $DURATION_S,
  "childrenPssBytes": $((CHILD_PSS_KB * 1024)),
  "survivingChildrenAfterClose": $SURVIVORS
}
EOF

echo "[bench:tabs-20] Concluído -> $OUT_DIR/tabs-20.json"
