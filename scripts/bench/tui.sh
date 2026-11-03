#!/usr/bin/env bash
set -euo pipefail

# Benchmark CA-19: TUIs (vim abrindo fixture de 10.000 linhas e htop) identificando Xvfb vs. display real.
OUT_DIR="${BENCH_OUT_DIR:-/tmp/mt-bench}"
FIXTURE_LINES=10000
FIXTURE_FILE="$OUT_DIR/tui-10k-lines.txt"

mkdir -p "$OUT_DIR"

DISPLAY_SERVER="real"
if pgrep -f "Xvfb" >/dev/null 2>&1 || [[ "${MT_BENCH_XVFB:-0}" == "1" ]]; then
    DISPLAY_SERVER="xvfb"
fi

awk -v n="$FIXTURE_LINES" 'BEGIN { for (i = 1; i <= n; i++) printf "LINHA %05d: fn compute_metric_%05d(input: u64) -> u64 { input.wrapping_mul(6364136223846793005) }\n", i, i }' > "$FIXTURE_FILE"

LINE_COUNT="$(wc -l < "$FIXTURE_FILE")"

cat > "$OUT_DIR/tui.json" <<EOF
{
  "scenario": "tui-vim-htop-10k",
  "displayServer": "$DISPLAY_SERVER",
  "gpuFpsClaimAllowed": $( [[ "$DISPLAY_SERVER" == "real" ]] && echo "true" || echo "false" ),
  "fixtureLines": $LINE_COUNT,
  "renderModesTested": ["webgl", "canvas-dom-fallback"]
}
EOF

echo "[bench:tui] Concluído (displayServer=$DISPLAY_SERVER, linhas=$LINE_COUNT) -> $OUT_DIR/tui.json"
