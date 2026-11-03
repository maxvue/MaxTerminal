# MaxTerminal — Integração OSC 133 para Fish Shell (Fase 05).
if set -q __MT_FISH_INTEGRATION_LOADED
    exit 0
end
set -g __MT_FISH_INTEGRATION_LOADED 1

set -g __mt_nonce "$MT_SHELL_NONCE"
set -e MT_SHELL_NONCE
set -e MT_SHELL_LOGIN

set -g __mt_executing 0

function __mt_b64
    printf '%s' "$argv[1]" | command base64 -w 0 2>/dev/null; or printf '%s' "$argv[1]" | command base64 2>/dev/null | command tr -d '\r\n'
end

function __mt_on_postexec --on-event fish_postexec
    set -l last_status $status
    if test "$__mt_executing" = "1"
        set -g __mt_executing 0
        set -l cwd_b64 (__mt_b64 "$PWD")
        printf '\e]133;D;%d;nonce=%s;cwd_b64=%s\a' $last_status "$__mt_nonce" "$cwd_b64"
    end
end

function __mt_on_prompt --on-event fish_prompt
    set -l cwd_b64 (__mt_b64 "$PWD")
    printf '\e]133;P;cwd_b64=%s;shell=fish\a' "$cwd_b64"
    printf '\e]133;A;cl=m;aid=1;nonce=%s\a' "$__mt_nonce"
    printf '\e]133;B;nonce=%s\a' "$__mt_nonce"
end

function __mt_on_preexec --on-event fish_preexec
    set -g __mt_executing 1
    set -l cmd_b64 (__mt_b64 "$argv[1]")
    set -l cwd_b64 (__mt_b64 "$PWD")
    printf '\e]133;C;nonce=%s;cmd_b64=%s;cwd_b64=%s\a' "$__mt_nonce" "$cmd_b64" "$cwd_b64"
end
