# MaxTerminal — Shim .zshrc para integração não intrusiva do Zsh (Fase 05).
typeset -g __mt_shim_dir="${MT_ZSH_SHIM_DIR:-}"

if [[ -n "${MT_ORIG_ZDOTDIR:-}" ]]; then
    export ZDOTDIR="${MT_ORIG_ZDOTDIR}"
    unset MT_ORIG_ZDOTDIR
    if [[ -r "${ZDOTDIR}/.zshrc" ]]; then
        source "${ZDOTDIR}/.zshrc"
    fi
else
    unset ZDOTDIR
    unset MT_ORIG_ZDOTDIR
    if [[ -r "${HOME}/.zshrc" ]]; then
        source "${HOME}/.zshrc"
    fi
fi

if [[ -n "${__mt_shim_dir}" && -r "${__mt_shim_dir}/mt-integration.zsh" ]]; then
    source "${__mt_shim_dir}/mt-integration.zsh"
fi
unset MT_ZSH_SHIM_DIR
unset __mt_shim_dir
