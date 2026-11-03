//! Códigos de erro e construtores de `MtError` do subsistema `mt-os` (Fase 09).

use std::borrow::Cow;

use mt_core::error::{ErrorCode, MtError};

/// Códigos de erro canônicos do domínio `os.*`.
pub mod codes {
    /// Configuração de integração com o SO inválida ou fora das faixas permitidas.
    pub const SETTINGS_INVALID: &str = "os.settings_invalid";
    /// Especificação de acelerador de atalho global inválida.
    pub const SHORTCUT_INVALID: &str = "os.shortcut_invalid";
    /// Atalho global em conflito com outro aplicativo ou sessão gráfica.
    pub const SHORTCUT_CONFLICT: &str = "os.shortcut_conflict";
    /// Registro de atalho global não suportado no ambiente gráfico atual.
    pub const SHORTCUT_UNSUPPORTED: &str = "os.shortcut_unsupported";
    /// Portal XDG (`GlobalShortcuts`) ausente ou indisponível no D-Bus.
    pub const PORTAL_UNAVAILABLE: &str = "os.portal_unavailable";
    /// Bandeja do sistema (`StatusNotifierWatcher`) indisponível na sessão atual.
    pub const TRAY_UNAVAILABLE: &str = "os.tray_unavailable";
    /// Arquivo referenciado por link do terminal não foi encontrado no disco.
    pub const LINK_NOT_FOUND: &str = "os.link_not_found";
    /// Candidato de link rejeitado (não é arquivo regular, tamanho/lote inválido ou fora dos limites).
    pub const LINK_REJECTED: &str = "os.link_rejected";
    /// Binário do editor configurado não foi encontrado no `$PATH`.
    pub const EDITOR_NOT_FOUND: &str = "os.editor_not_found";
    /// Falha ao iniciar o processo ou URI do editor externo.
    pub const EDITOR_LAUNCH_FAILED: &str = "os.editor_launch_failed";
    /// Falha ao despachar notificação nativa para o servidor de notificações.
    pub const NOTIFY_FAILED: &str = "os.notify_failed";
    /// Falha ao ler ou gravar o arquivo `.desktop` de inicialização automática (`autostart`).
    pub const AUTOSTART_FAILED: &str = "os.autostart_failed";
    /// Tempo limite esgotado ao encerrar processos supervisionados durante o desligamento.
    pub const SHUTDOWN_TIMEOUT: &str = "os.shutdown_timeout";
    /// Aba solicitada não encontrada no subsistema de integração com o SO.
    pub const TAB_NOT_FOUND: &str = "os.tab_not_found";
    /// Contexto de painel/PTY/época de CWD tornou-se obsoleto antes ou durante a resolução do link.
    pub const LINK_CONTEXT_STALE: &str = "os.link_context_stale";
    /// Fila de ativação (`deep-link` / `single-instance`) saturada.
    pub const ACTIVATION_BUSY: &str = "os.activation_busy";
    /// Rota de ativação sem consumidor registrado ou expirada.
    pub const ACTIVATION_UNAVAILABLE: &str = "os.activation_unavailable";
    /// URI ou payload de ativação malformado ou inválido.
    pub const ACTIVATION_INVALID: &str = "os.activation_invalid";
    /// Subsistema de integração com o SO ainda não concluiu a inicialização.
    pub const NOT_READY: &str = "os.not_ready";
}

/// Constrói um `MtError` com código estático de `mt-os` e mensagem em `pt-BR`.
#[must_use]
pub fn os_error(code: &'static str, message: impl Into<Cow<'static, str>>) -> MtError {
    MtError::new(ErrorCode(code), message)
}
