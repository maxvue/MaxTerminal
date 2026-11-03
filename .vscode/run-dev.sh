#!/usr/bin/env bash
# RUN DEV — Inicia o MaxTerminal em modo de desenvolvimento (npm run tauri dev)
# com Vite HMR no frontend e recompilação do backend nativo Rust.
# Inclui sincronização de bibliotecas locais (@maxvue/*), limpeza de cache
# e controle de PID/lockfile para evitar instâncias duplicadas ou portas presas.
set -u
cd "$(dirname "$0")/.." || exit 1

export NODE_OPTIONS="--max-old-space-size=8192"

WORKSPACE_HASH=$(echo -n "$PWD" | md5sum | cut -d' ' -f1)
LOCKFILE="/tmp/.maxterminal-run-dev-${WORKSPACE_HASH}.pid"

take_over() {
  [ -f "$LOCKFILE" ] || return 0
  local old_pid
  old_pid=$(cat "$LOCKFILE" 2>/dev/null) || return 0
  [ -n "$old_pid" ] || return 0
  kill -0 "$old_pid" 2>/dev/null || return 0

  echo "→ Instância anterior do RUN DEV (PID $old_pid) ainda ativa. Encerrando..."
  kill -TERM -- "-$old_pid" 2>/dev/null || true
  kill -TERM "$old_pid" 2>/dev/null || true
  for _ in 1 2 3 4 5; do
    kill -0 "$old_pid" 2>/dev/null || break
    sleep 0.5
  done
  kill -KILL -- "-$old_pid" 2>/dev/null || true
  kill -KILL "$old_pid" 2>/dev/null || true
}

take_over
echo $$ > "$LOCKFILE"

# Mata apenas processos cujo diretório de execução (cwd) é ESTE projeto/worktree.
kill_in_project() {
  local pattern="$1"
  local pid
  for pid in $(pgrep -f "$pattern" 2>/dev/null); do
    if [ "$(readlink -f "/proc/$pid/cwd" 2>/dev/null)" = "$PWD" ]; then
      kill "$pid" 2>/dev/null || true
    fi
  done
  return 0
}

cleanup() {
  echo "→ Encerrando processos filhos do projeto..."
  kill_in_project 'node_modules/.bin/vite'
  kill_in_project 'vite'
  kill_in_project 'target/debug/maxterminal'
  kill_in_project 'tauri'
}

trap 'cleanup; rm -f "$LOCKFILE"; exit 0' SIGINT SIGTERM

# Sincroniza bibliotecas locais (@maxvue/*) se o script existir
if [ -f "scripts/sync-local-libs.mjs" ]; then
  echo "→ Sincronizando bibliotecas locais (@maxvue/*)..."
  node scripts/sync-local-libs.mjs >/dev/null 2>&1 || true
fi

# Limpa o cache de pré-bundling do Vite ao iniciar
echo "→ Limpando cache de dependências do Vite (node_modules/.vite/deps)..."
rm -rf node_modules/.vite/deps >/dev/null 2>&1 || true

# Limpa instâncias antigas presas antes de subir
cleanup
sleep 0.5

echo "→ Iniciando MaxTerminal Desktop (npm run tauri dev)..."
npm run tauri dev
EXIT_CODE=$?

echo "→ MaxTerminal finalizado (código de saída: $EXIT_CODE)."
cleanup
rm -f "$LOCKFILE"
exit "$EXIT_CODE"
