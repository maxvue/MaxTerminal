#!/usr/bin/env bash
set -euo pipefail
seq 1 3000000
yes "MaxTerminal-Burst-Line-0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ" | head -c 200M || true
printf "\n=== BURST FINALIZADO: 3000000 ===\n"
