#!/usr/bin/env bash
# Verifica a reprodutibilidade determinística do build do MaxTerminal (CA-41).
# Fixa SOURCE_DATE_EPOCH, TZ=UTC, LC_ALL=C.UTF-8 e --remap-path-prefix,
# executa dois builds independentes em diretórios distintos e compara o SHA-256.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-1735689600}"
export TZ="UTC"
export LC_ALL="C.UTF-8"
export CARGO_INCREMENTAL="0"

if [[ "${1:-}" == "--dry-run" ]]; then
  echo "[reproducible-build] Dry-run OK (SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH}, TZ=${TZ}, LC_ALL=${LC_ALL})"
  exit 0
fi

TMP_A="$(mktemp -d)"
TMP_B="$(mktemp -d)"
trap 'rm -rf "${TMP_A}" "${TMP_B}"' EXIT

build_once() {
  local target_dir="$1"
  RUSTFLAGS="--remap-path-prefix=${ROOT_DIR}=/build/maxterminal --remap-path-prefix=${HOME}=/build/home" \
    CARGO_TARGET_DIR="${target_dir}" \
    cargo build --manifest-path "${ROOT_DIR}/src-tauri/Cargo.toml" --locked --release --bin maxterminal
  sha256sum "${target_dir}/release/maxterminal" | awk '{print $1}'
}

HASH_A="$(build_once "${TMP_A}")"
HASH_B="$(build_once "${TMP_B}")"

echo "[reproducible-build] Build A SHA-256: ${HASH_A}"
echo "[reproducible-build] Build B SHA-256: ${HASH_B}"

if [[ "${HASH_A}" != "${HASH_B}" ]]; then
  echo "[reproducible-build] ERRO: hashes divergem entre builds independentes!" >&2
  exit 1
fi

echo "[reproducible-build] OK — binário 100% reproduzível."
