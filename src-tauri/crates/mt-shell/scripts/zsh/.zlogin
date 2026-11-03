# MaxTerminal — Shim .zlogin para integração não intrusiva do Zsh (Fase 05).
if [[ -n "${MT_ORIG_ZDOTDIR:-}" ]]; then
    export ZDOTDIR="${MT_ORIG_ZDOTDIR}"
    unset MT_ORIG_ZDOTDIR
    if [[ -r "${ZDOTDIR}/.zlogin" ]]; then
        source "${ZDOTDIR}/.zlogin"
    fi
else
    unset ZDOTDIR
    if [[ -r "${HOME}/.zlogin" ]]; then
        source "${HOME}/.zlogin"
    fi
fi
