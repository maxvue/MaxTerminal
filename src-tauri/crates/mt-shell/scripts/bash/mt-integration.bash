# MaxTerminal — Integração OSC 133 para GNU Bash (Fase 05).
# Carrega os dotfiles do usuário sem modificá-los e instala hooks não intrusivos via PROMPT_COMMAND e PS0.

if [[ -n "${__MT_BASH_INTEGRATION_LOADED:-}" ]]; then
    return 0 2>/dev/null || exit 0
fi
__MT_BASH_INTEGRATION_LOADED=1

__mt_nonce="${MT_SHELL_NONCE:-}"
unset MT_SHELL_NONCE

__mt_login="${MT_SHELL_LOGIN:-0}"
unset MT_SHELL_LOGIN

__mt_runtime_dir="${MT_SHELL_RUNTIME_DIR:-/tmp}"
unset MT_SHELL_RUNTIME_DIR
__mt_state_file="${__mt_runtime_dir}/.mt_executing_$$"

# 1. Carrega dotfiles padrão do usuário conforme modo login vs interativo
if [[ "${__mt_login}" == "1" ]]; then
    if [[ -r /etc/profile ]]; then
        # shellcheck disable=SC1091
        source /etc/profile
    fi
    if [[ -r "${HOME}/.bash_profile" ]]; then
        # shellcheck disable=SC1091
        source "${HOME}/.bash_profile"
    elif [[ -r "${HOME}/.bash_login" ]]; then
        # shellcheck disable=SC1091
        source "${HOME}/.bash_login"
    elif [[ -r "${HOME}/.profile" ]]; then
        # shellcheck disable=SC1091
        source "${HOME}/.profile"
    elif [[ -r "${HOME}/.bashrc" ]]; then
        # shellcheck disable=SC1091
        source "${HOME}/.bashrc"
    fi
else
    if [[ -r /etc/bash.bashrc ]]; then
        # shellcheck disable=SC1091
        source /etc/bash.bashrc
    elif [[ -r /etc/bashrc ]]; then
        # shellcheck disable=SC1091
        source /etc/bashrc
    fi
    if [[ -r "${HOME}/.bashrc" ]]; then
        # shellcheck disable=SC1091
        source "${HOME}/.bashrc"
    fi
fi

# Ajusta HISTCONTROL para que comandos precedidos por espaço também fiquem visíveis para history 1
if [[ "${HISTCONTROL:-}" == *"ignoreboth"* ]]; then
    export HISTCONTROL="ignoredups:${HISTCONTROL//ignoreboth/}"
elif [[ "${HISTCONTROL:-}" == *"ignorespace"* ]]; then
    export HISTCONTROL="${HISTCONTROL//ignorespace/}"
fi

# 2. Helpers internos de codificação Base64 e emissão OSC 133
__mt_b64() {
    printf '%s' "$1" | command base64 -w 0 2>/dev/null || printf '%s' "$1" | command base64 2>/dev/null | command tr -d '\r\n'
}

__mt_emit_osc_d() {
    local exit_code="$1"
    local cwd_b64
    cwd_b64="$(__mt_b64 "${PWD}")"
    printf '\e]133;D;%s;nonce=%s;cwd_b64=%s\a' "${exit_code}" "${__mt_nonce}" "${cwd_b64}"
}

__mt_emit_prompt_markers() {
    local cwd_b64
    cwd_b64="$(__mt_b64 "${PWD}")"
    printf '\e]133;P;cwd_b64=%s;shell=bash\a' "${cwd_b64}"
    printf '\e]133;A;cl=m;aid=1;nonce=%s\a' "${__mt_nonce}"
    printf '\e]133;B;nonce=%s\a' "${__mt_nonce}"
}

# Captura PROMPT_COMMAND pré-existente (string ou array)
__mt_user_prompt_cmds=()
if [[ "$(declare -p PROMPT_COMMAND 2>/dev/null)" == "declare -a"* ]]; then
    for __mt_pc_item in "${PROMPT_COMMAND[@]}"; do
        if [[ -n "${__mt_pc_item}" && "${__mt_pc_item}" != "__mt_prompt_cmd" ]]; then
            __mt_user_prompt_cmds+=("${__mt_pc_item}")
        fi
    done
elif [[ -n "${PROMPT_COMMAND:-}" && "${PROMPT_COMMAND}" != "__mt_prompt_cmd" ]]; then
    __mt_user_prompt_cmds=("${PROMPT_COMMAND}")
fi
unset __mt_pc_item

__mt_restore_exit() {
    return "$1"
}

__mt_prompt_cmd() {
    local last_exit=$?

    if [[ -e "${__mt_state_file}" ]]; then
        command rm -f "${__mt_state_file}" 2>/dev/null
        __mt_emit_osc_d "${last_exit}"
    fi

    local __mt_cmd_entry
    for __mt_cmd_entry in "${__mt_user_prompt_cmds[@]}"; do
        __mt_restore_exit "${last_exit}"
        eval "${__mt_cmd_entry}"
    done

    __mt_emit_prompt_markers
    return "${last_exit}"
}

__mt_ps0() {
    : > "${__mt_state_file}" 2>/dev/null
    local cmd_text=""
    cmd_text="$(LC_ALL=C HISTTIMEFORMAT='' builtin history 1 2>/dev/null | command sed -E 's/^[[:space:]]*[0-9]+\*?[[:space:]]+//')"
    local cmd_b64 cwd_b64
    cmd_b64="$(__mt_b64 "${cmd_text}")"
    cwd_b64="$(__mt_b64 "${PWD}")"
    printf '\e]133;C;nonce=%s;cmd_b64=%s;cwd_b64=%s\a' "${__mt_nonce}" "${cmd_b64}" "${cwd_b64}"
}

# 3. Instala PROMPT_COMMAND unificado e PS0 para início exato de comando
unset PROMPT_COMMAND
PROMPT_COMMAND="__mt_prompt_cmd"
if [[ "${PS0:-}" != *"__mt_ps0"* ]]; then
    PS0="${PS0:-}\$(__mt_ps0)"
fi
