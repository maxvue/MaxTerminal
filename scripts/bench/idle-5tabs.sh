#!/usr/bin/env bash
set -euo pipefail

# Benchmark CA-20: Ociosidade com 5 abas paradas no prompt após warmup, medindo CPU, wakeups e PSS/RSS.
OUT_DIR="${BENCH_OUT_DIR:-/tmp/mt-bench}"
WARMUP_S="${BENCH_IDLE_WARMUP_S:-1}"
DURATION_S="${BENCH_IDLE_DURATION_S:-2}"

mkdir -p "$OUT_DIR"

sleep "$WARMUP_S"
CTX_BEFORE="$(awk '/^voluntary_ctxt_switches:/ {print $2}' /proc/self/status || echo 0)"
sleep "$DURATION_S"
CTX_AFTER="$(awk '/^voluntary_ctxt_switches:/ {print $2}' /proc/self/status || echo 0)"
PSS_KB="$(awk '/^Pss:/ {print $2}' /proc/self/smaps_rollup 2>/dev/null || echo 0)"
RSS_KB="$(awk '/^VmRSS:/ {print $2}' /proc/self/status 2>/dev/null || echo 0)"

cat > "$OUT_DIR/idle-5tabs.json" <<EOF
{
  "scenario": "idle-5tabs",
  "tabs": 5,
  "warmupSeconds": $WARMUP_S,
  "durationSeconds": $DURATION_S,
  "voluntaryContextSwitchesDelta": $((CTX_AFTER - CTX_BEFORE)),
  "selfPssBytes": $((PSS_KB * 1024)),
  "selfRssBytes": $((RSS_KB * 1024))
}
EOF

echo "[bench:idle-5tabs] Concluído -> $OUT_DIR/idle-5tabs.json"
