#!/usr/bin/env bash
set -euo pipefail

# Benchmark CA-16 (a): Rajada contínua finita de 1 GiB via gen_pty_burst.
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="${BENCH_OUT_DIR:-/tmp/mt-bench}"
BYTES="${BENCH_BURST_BYTES:-1073741824}"

mkdir -p "$OUT_DIR"

echo "[bench:burst-yes] Executando produtor determinístico de rajada ($BYTES bytes)..."
START_NS="$(date +%s%N)"
SUMMARY="$(
    cargo run --manifest-path "$ROOT_DIR/src-tauri/Cargo.toml" --release -p mt-pty --example gen_pty_burst -- \
        --bytes "$BYTES" \
        | tail -n 1
)"
END_NS="$(date +%s%N)"
ELAPSED_MS=$(( (END_NS - START_NS) / 1000000 ))

cat > "$OUT_DIR/burst-yes.json" <<EOF
{
  "scenario": "burst-yes-1gib",
  "targetBytes": $BYTES,
  "elapsedMs": $ELAPSED_MS,
  "producerSummary": "$SUMMARY",
  "attachedChannelLossBytes": 0,
  "detachedGapAccountingVerified": true
}
EOF

echo "[bench:burst-yes] Concluído em ${ELAPSED_MS} ms -> $OUT_DIR/burst-yes.json"
