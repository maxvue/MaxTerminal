# MaxTerminal — Shim .zshenv para integração não intrusiva do Zsh (Fase 05).
if [[ -n "${MT_ORIG_ZDOTDIR:-}" ]]; then
    ZDOTDIR="${MT_ORIG_ZDOTDIR}"
    if [[ -r "${ZDOTDIR}/.zshenv" ]]; then
        source "${ZDOTDIR}/.zshenv"
    fi
else
    unset ZDOTDIR
    if [[ -r "${HOME}/.zshenv" ]]; then
        source "${HOME}/.zshenv"
    fi
fi

if [[ -n "${MT_ZSH_SHIM_DIR:-}" ]]; then
    ZDOTDIR="${MT_ZSH_SHIM_DIR}"
fi
