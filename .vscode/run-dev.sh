#!/usr/bin/env bash
# RUN DEV — Inicia o MaxTerminal em modo de desenvolvimento (npm run tauri dev)
# com Vite HMR no frontend e recompilação do backend nativo Rust.
# Inclui sincronização de bibliotecas locais (@maxvue/*), cache gerenciado pelo Vite
# e controle de PID/lockfile para evitar instâncias duplicadas ou portas presas.
set -u
cd -P "$(dirname "$0")/.." || exit 1

export NODE_OPTIONS="--max-old-space-size=8192"

# Assegura ambiente Rust/Cargo disponível mesmo via VS Code GUI
if [ -f "$HOME/.cargo/env" ]; then
  # shellcheck source=/dev/null
  . "$HOME/.cargo/env"
elif [ -d "/home/johnattas/.cargo/bin" ]; then
  export CARGO_HOME="/home/johnattas/.cargo"
  export RUSTUP_HOME="/home/johnattas/.rustup"
  export PATH="/home/johnattas/.cargo/bin:$PATH"
fi

# Se invocado pelo beforeDevCommand do próprio Tauri, sobe apenas o servidor Vite
if [ "${MAXTERMINAL_RUN_DEV_ACTIVE:-0}" = "1" ]; then
  exec node scripts/run-vite.mjs
fi
export MAXTERMINAL_RUN_DEV_ACTIVE=1

# Assegura existência da pasta frontendDist para o macro tauri::generate_context!()
mkdir -p dist

WORKSPACE_HASH=$(echo -n "$PWD" | md5sum | cut -d' ' -f1)
LOCKFILE="${TMPDIR:-/tmp}/.maxterminal-run-dev-${WORKSPACE_HASH}.pid"

process_start() {
  local stat
  local -a fields
  stat=$(cat "/proc/$1/stat" 2>/dev/null) || return 1
  read -r -a fields <<< "${stat##*) }"
  [ "${fields[0]}" != "Z" ] || return 1
  printf '%s\n' "${fields[19]}"
}

matches_project_process() {
  local pid="$1" pattern="$2" cwd exe script
  local -a args
  [[ "$pid" =~ ^[0-9]+$ ]] || return 1
  cwd=$(readlink "/proc/$pid/cwd" 2>/dev/null) || return 1
  case "$cwd" in "$PWD"|"$PWD/src-tauri") ;; *) return 1 ;; esac
  exe=$(readlink "/proc/$pid/exe" 2>/dev/null) || return 1
  exe=${exe% (deleted)}
  if [ "$pattern" = task-child ]; then
    [ "$pid" = "${TASK_PID:-}" ] && [ "$(process_start "$pid")" = "${TASK_START:-}" ]
    return
  fi
  if [ "$pattern" = 'target/debug/maxterminal' ]; then
    local target_dir="${CARGO_TARGET_DIR:-target}"
    case "$target_dir" in /*) ;; *) target_dir="$PWD/src-tauri/$target_dir" ;; esac
    target_dir=$(realpath -m "$target_dir")
    [ "$exe" = "$target_dir/debug/maxterminal" ]
    return
  fi
  [ "$cwd" = "$PWD" ] || return 1
  mapfile -d '' -t args 2>/dev/null < "/proc/$pid/cmdline" || return 1
  script=${args[1]:-}
  script=${script#./}
  script=${script#"$PWD/"}
  case "$pattern" in
    run-dev.sh) [[ "${exe##*/}" = bash && "$script" = '.vscode/run-dev.sh' ]] ;;
    node_modules/.bin/vite) [[ "${exe##*/}" = node && ( "$script" = 'node_modules/.bin/vite' || "$script" = 'node_modules/vite/bin/vite.js' ) ]] ;;
    node_modules/.bin/tauri) [[ "${exe##*/}" = node && ( "$script" = 'node_modules/.bin/tauri' || "$script" = 'node_modules/@tauri-apps/cli/tauri.js' ) ]] ;;
    *) return 1 ;;
  esac
}

stop_project_process() {
  local pid="$1" pattern="$2" start current attempt
  start=$(process_start "$pid") || return 0
  matches_project_process "$pid" "$pattern" || return 0
  current=$(process_start "$pid") || return 0
  [ "$start" = "$current" ] || return 0
  kill -TERM "$pid" 2>/dev/null || return 0
  for ((attempt = 0; attempt < 50; attempt++)); do
    current=$(process_start "$pid") || return 0
    [ "$start" = "$current" ] || return 0
    sleep 0.1
  done
  echo "→ O processo $pid não encerrou; mantendo a supervisão e cancelando a reinicialização." >&2
  return 1
}

take_over() {
  [ -f "$LOCKFILE" ] || return 0
  local old_pid old_start current
  read -r old_pid old_start < "$LOCKFILE" || [ -n "${old_pid:-}" ] || return 0
  [[ "$old_pid" =~ ^[0-9]+$ ]] || return 0
  [ "$old_pid" != "$$" ] || return 0
  matches_project_process "$old_pid" run-dev.sh || return 0
  current=$(process_start "$old_pid") || return 0
  [ -z "$old_start" ] || [ "$old_start" = "$current" ] || return 0
  echo "→ Instância anterior do RUN DEV (PID $old_pid) ainda ativa. Encerrando..."
  stop_project_process "$old_pid" run-dev.sh
}

# Confere diretório, executável/argumento de entrada e identidade antes de sinalizar.
# Nunca encerra um processo só porque ele menciona o caminho em outro argumento.
kill_in_project() {
  local pattern="$1" pid
  # pgrep apenas reduz os candidatos; a autorização continua sendo a identidade exata.
  for pid in $(pgrep -u "$UID" -f 'maxterminal|vite|tauri|run-dev.sh' 2>/dev/null); do
    [ "$pid" = "$$" ] || [ "$pid" = "$PPID" ] && continue
    if matches_project_process "$pid" "$pattern"; then
      stop_project_process "$pid" "$pattern" || return 1
    fi
  done
  return 0
}

cleanup() {
  echo "→ Encerrando processos filhos do projeto..."
  # O backend precisa liberar o banco antes de encerrar seu supervisor Tauri.
  kill_in_project 'target/debug/maxterminal' || return 1
  kill_in_project 'node_modules/.bin/vite' || return 1
  kill_in_project 'node_modules/.bin/tauri' || return 1
  if [ -n "${TASK_PID:-}" ] && [ -n "${TASK_START:-}" ]; then
    stop_project_process "$TASK_PID" task-child
  fi
}

finish() {
  local status=$?
  trap - EXIT SIGINT SIGTERM
  # Uma task que perdeu a identidade não pode limpar os processos da sucessora.
  if [ "$(cat "$LOCKFILE" 2>/dev/null)" = "$TASK_IDENTITY" ]; then
    cleanup || status=1
    rm -f "$LOCKFILE"
  fi
  exit "$status"
}

TASK_PID=''
TASK_START=''
# Serializa a troca de proprietário e toda a preparação. O arquivo .lock permanece
# para que processos concorrentes sempre bloqueiem o mesmo inode.
exec 9>"${LOCKFILE}.lock" || exit 1
flock -x 9 || exit 1
take_over || exit 1
TASK_IDENTITY="$$ $(process_start "$$")"
echo "$TASK_IDENTITY" > "$LOCKFILE"
trap finish EXIT
trap 'exit 130' SIGINT
trap 'exit 143' SIGTERM

# Sincroniza bibliotecas locais (@maxvue/*) se o script existir
if [ -f "scripts/sync-local-libs.mjs" ]; then
  echo "→ Sincronizando bibliotecas locais (@maxvue/*)..."
  node scripts/sync-local-libs.mjs
  SYNC_CODE=$?
  if [ "$SYNC_CODE" -ne 0 ]; then
    echo "→ Falha ao sincronizar bibliotecas locais (código $SYNC_CODE); o Tauri não será iniciado." >&2
    exit "$SYNC_CODE"
  fi
fi

# O cacheDir está em vite.config.ts. Não apaga node_modules, que pode ser compartilhado.
echo "→ Preservando cache gerenciado pelo Vite (.tmp/vite)..."

# Limpa instâncias antigas presas antes de subir
cleanup || exit 1

# Não deixa os filhos herdarem o bloqueio de preparação.
flock -u 9
exec 9>&-

echo "→ Iniciando MaxTerminal Desktop (npm run tauri dev)..."
npm run tauri dev &
TASK_PID=$!
TASK_START=$(process_start "$TASK_PID") || TASK_START=''
wait "$TASK_PID"
EXIT_CODE=$?

echo "→ MaxTerminal finalizado (código de saída: $EXIT_CODE)."
exit "$EXIT_CODE"
