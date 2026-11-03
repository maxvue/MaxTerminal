# MaxTerminal — Shim .zprofile para integração não intrusiva do Zsh (Fase 05).
if [[ -n "${MT_ORIG_ZDOTDIR:-}" ]]; then
    ZDOTDIR="${MT_ORIG_ZDOTDIR}"
    if [[ -r "${ZDOTDIR}/.zprofile" ]]; then
        source "${ZDOTDIR}/.zprofile"
    fi
else
    unset ZDOTDIR
    if [[ -r "${HOME}/.zprofile" ]]; then
        source "${HOME}/.zprofile"
    fi
fi

if [[ -n "${MT_ZSH_SHIM_DIR:-}" ]]; then
    ZDOTDIR="${MT_ZSH_SHIM_DIR}"
fi
