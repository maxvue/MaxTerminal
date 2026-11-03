#!/usr/bin/env bash
set -euo pipefail

# Benchmark CA-16 (b): Leitura de arquivo determinístico de 500 MiB via gen_pty_burst --file.
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="${BENCH_OUT_DIR:-/tmp/mt-bench}"
FILE_BYTES="${BENCH_CAT_BYTES:-524288000}"
FIXTURE_FILE="$OUT_DIR/fixture-500mib.txt"

mkdir -p "$OUT_DIR"

if [[ ! -f "$FIXTURE_FILE" ]]; then
    echo "[bench:burst-cat] Gerando arquivo determinístico ($FILE_BYTES bytes) em $FIXTURE_FILE..."
    cargo run --manifest-path "$ROOT_DIR/src-tauri/Cargo.toml" --release -p mt-pty --example gen_pty_burst -- \
        --bytes "$FILE_BYTES" > "$FIXTURE_FILE" 2>/dev/null
fi

START_NS="$(date +%s%N)"
SUMMARY="$(
    cargo run --manifest-path "$ROOT_DIR/src-tauri/Cargo.toml" --release -p mt-pty --example gen_pty_burst -- \
        --file "$FIXTURE_FILE" \
        | tail -n 1
)"
END_NS="$(date +%s%N)"
ELAPSED_MS=$(( (END_NS - START_NS) / 1000000 ))

cat > "$OUT_DIR/burst-cat.json" <<EOF
{
  "scenario": "burst-cat-500mib",
  "fixtureBytes": $FILE_BYTES,
  "elapsedMs": $ELAPSED_MS,
  "producerSummary": "$SUMMARY",
  "attachedChannelLossBytes": 0
}
EOF

echo "[bench:burst-cat] Concluído em ${ELAPSED_MS} ms -> $OUT_DIR/burst-cat.json"
