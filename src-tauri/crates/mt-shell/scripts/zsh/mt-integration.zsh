# MaxTerminal — Integração OSC 133 para Zsh (Fase 05).
if [[ -n "${__MT_ZSH_INTEGRATION_LOADED:-}" ]]; then
    return 0
fi
typeset -g __MT_ZSH_INTEGRATION_LOADED=1

typeset -g __mt_nonce="${MT_SHELL_NONCE:-}"
unset MT_SHELL_NONCE
unset MT_SHELL_LOGIN

typeset -gi __mt_executing=0

__mt_b64() {
    printf '%s' "$1" | command base64 -w 0 2>/dev/null || printf '%s' "$1" | command base64 2>/dev/null | command tr -d '\r\n'
}

__mt_precmd() {
    local exit_code=$?
    local cwd_b64
    cwd_b64="$(__mt_b64 "${PWD}")"

    if (( __mt_executing )); then
        __mt_executing=0
        printf '\e]133;D;%d;nonce=%s;cwd_b64=%s\a' "${exit_code}" "${__mt_nonce}" "${cwd_b64}"
    fi

    printf '\e]133;P;cwd_b64=%s;shell=zsh\a' "${cwd_b64}"
    printf '\e]133;A;cl=m;aid=1;nonce=%s\a' "${__mt_nonce}"
    printf '\e]133;B;nonce=%s\a' "${__mt_nonce}"
}

__mt_preexec() {
    __mt_executing=1
    local cmd_text="${1:-}"
    local cmd_b64 cwd_b64
    cmd_b64="$(__mt_b64 "${cmd_text}")"
    cwd_b64="$(__mt_b64 "${PWD}")"
    printf '\e]133;C;nonce=%s;cmd_b64=%s;cwd_b64=%s\a' "${__mt_nonce}" "${cmd_b64}" "${cwd_b64}"
}

autoload -Uz add-zsh-hook
add-zsh-hook precmd __mt_precmd
add-zsh-hook preexec __mt_preexec
