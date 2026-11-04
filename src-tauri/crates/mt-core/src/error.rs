//! Contrato único de erro (`MtError`) e códigos de erro (`ErrorCode`) para fronteira IPC e domínio.

use serde::Serialize;
use serde_json::json;
use std::{borrow::Cow, fmt, io};

/// Código estável legível por máquina no formato `<domínio>.<motivo>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, specta::Type)]
#[serde(transparent)]
#[specta(transparent)]
pub struct ErrorCode(pub &'static str);

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// Códigos canônicos do domínio `core`.
pub mod codes {
    use super::ErrorCode;

    /// Falha interna inesperada.
    pub const INTERNAL: ErrorCode = ErrorCode("core.internal");
    /// Erro de entrada/saída do sistema operacional.
    pub const IO: ErrorCode = ErrorCode("core.io");
    /// Entidade ou recurso não encontrado.
    pub const NOT_FOUND: ErrorCode = ErrorCode("core.not_found");
    /// Argumento de entrada inválido.
    pub const INVALID_ARGUMENT: ErrorCode = ErrorCode("core.invalid_argument");
    /// Conflito de estado ou recurso duplicado.
    pub const CONFLICT: ErrorCode = ErrorCode("core.conflict");
    /// Permissão negada pelo sistema operacional ou política interna.
    pub const PERMISSION_DENIED: ErrorCode = ErrorCode("core.permission_denied");
    /// Recurso ou operação não suportada.
    pub const UNSUPPORTED: ErrorCode = ErrorCode("core.unsupported");
    /// Erro de serialização ou desserialização de dados.
    pub const SERIALIZATION: ErrorCode = ErrorCode("core.serialization");
    /// Impossível resolver diretórios base XDG/HOME da aplicação.
    pub const PATHS_UNRESOLVED: ErrorCode = ErrorCode("core.paths_unresolved");

    /// O recurso de integração de shell não está registrado para o PTY informado.
    pub const SHELL_NOT_REGISTERED: ErrorCode = ErrorCode("shell.not_registered");
    /// O bloco de comando solicitado não foi encontrado.
    pub const SHELL_BLOCK_NOT_FOUND: ErrorCode = ErrorCode("shell.block_not_found");
    /// O bloco solicitado não possui contexto de erro inspecionável.
    pub const SHELL_BLOCK_NOT_ERROR: ErrorCode = ErrorCode("shell.block_not_error");
    /// A aba solicitada não foi encontrada no registro de estados.
    pub const SHELL_TAB_NOT_FOUND: ErrorCode = ErrorCode("shell.tab_not_found");
    /// Falha ao preparar o diretório/arquivos de injeção do shell.
    pub const SHELL_INJECTION_FAILED: ErrorCode = ErrorCode("shell.injection_failed");
    /// Consulta de listagem ou busca de blocos inválida.
    pub const SHELL_QUERY_INVALID: ErrorCode = ErrorCode("shell.query_invalid");

    /// Caminho do projeto não existe no sistema de arquivos.
    pub const PROJECT_PATH_NOT_FOUND: ErrorCode = ErrorCode("project.path_not_found");
    /// Caminho do projeto não é um diretório.
    pub const PROJECT_PATH_NOT_DIRECTORY: ErrorCode = ErrorCode("project.path_not_directory");
    /// Caminho proibido para cadastro de projeto (`/` ou `$HOME`).
    pub const PROJECT_PATH_FORBIDDEN: ErrorCode = ErrorCode("project.path_forbidden");
    /// Caminho contém bytes não representáveis em UTF-8.
    pub const PROJECT_PATH_NOT_UTF8: ErrorCode = ErrorCode("project.path_not_utf8");
    /// Já existe um projeto cadastrado para este caminho canônico.
    pub const PROJECT_DUPLICATE_PATH: ErrorCode = ErrorCode("project.duplicate_path");
    /// Título de projeto inválido.
    pub const PROJECT_INVALID_TITLE: ErrorCode = ErrorCode("project.invalid_title");
    /// Ícone Iconify inválido.
    pub const PROJECT_INVALID_ICON: ErrorCode = ErrorCode("project.invalid_icon");
    /// Cor hexadecimal inválida.
    pub const PROJECT_INVALID_COLOR: ErrorCode = ErrorCode("project.invalid_color");
    /// Patch de atualização vazio ou inválido.
    pub const PROJECT_INVALID_PATCH: ErrorCode = ErrorCode("project.invalid_patch");
    /// Argumento de domínio de projeto inválido.
    pub const PROJECT_INVALID_ARGUMENT: ErrorCode = ErrorCode("project.invalid_argument");
    /// Projeto não encontrado.
    pub const PROJECT_NOT_FOUND: ErrorCode = ErrorCode("project.not_found");
    /// Remoção de projeto bloqueada por abas em execução ou hook ativo.
    pub const PROJECT_REMOVAL_BLOCKED: ErrorCode = ErrorCode("project.removal_blocked");
    /// Conjunto de IDs de reordenação diverge do catálogo atual.
    pub const PROJECT_REORDER_MISMATCH: ErrorCode = ErrorCode("project.reorder_mismatch");
    /// Sugestão de descoberta não encontrada.
    pub const PROJECT_DISCOVERY_NOT_FOUND: ErrorCode = ErrorCode("project.discovery_not_found");
    /// Diretório sugerido deixou de existir antes do aceite.
    pub const PROJECT_DISCOVERY_STALE: ErrorCode = ErrorCode("project.discovery_stale");

    /// Já existe uma pasta-mãe cadastrada para este caminho canônico.
    pub const PROJECT_ROOT_DUPLICATE_PATH: ErrorCode = ErrorCode("project_root.duplicate_path");
    /// Pasta-mãe não encontrada.
    pub const PROJECT_ROOT_NOT_FOUND: ErrorCode = ErrorCode("project_root.not_found");
    /// Diretório da pasta-mãe está ausente no disco.
    pub const PROJECT_ROOT_MISSING: ErrorCode = ErrorCode("project_root.missing");
    /// Falha ao varrer subpastas da pasta-mãe.
    pub const PROJECT_ROOT_SCAN_FAILED: ErrorCode = ErrorCode("project_root.scan_failed");
    /// Falha ao registrar observador inotify na pasta-mãe.
    pub const PROJECT_ROOT_WATCH_FAILED: ErrorCode = ErrorCode("project_root.watch_failed");

    /// Parâmetros inválidos de template de aba.
    pub const TEMPLATE_INVALID: ErrorCode = ErrorCode("template.invalid");
    /// Template de aba não encontrado.
    pub const TEMPLATE_NOT_FOUND: ErrorCode = ErrorCode("template.not_found");
    /// Não é permitido remover ou desmarcar o único template padrão do projeto.
    pub const TEMPLATE_DEFAULT_REQUIRED: ErrorCode = ErrorCode("template.default_required");
    /// Literal de variável ou comando contém segredo em texto plano.
    pub const TEMPLATE_SECRET_LITERAL: ErrorCode = ErrorCode("template.secret_literal");
    /// Diretório de trabalho configurado para o template/aba não foi encontrado.
    pub const TEMPLATE_CWD_NOT_FOUND: ErrorCode = ErrorCode("template.cwd_not_found");
    /// Conflito de revisão ao atualizar ou excluir template.
    pub const TEMPLATE_CONFLICT: ErrorCode = ErrorCode("template.conflict");

    /// Ambiente ou resolvedor de agente CLI indisponível para iniciar a aba.
    pub const WORKSPACE_ENVIRONMENT_UNAVAILABLE: ErrorCode =
        ErrorCode("workspace.environment_unavailable");
    /// Reutilização de `request_id` com argumentos divergentes.
    pub const WORKSPACE_REQUEST_CONFLICT: ErrorCode = ErrorCode("workspace.request_conflict");
    /// Operação interrompida em execução anterior do aplicativo.
    pub const WORKSPACE_OPERATION_INTERRUPTED: ErrorCode =
        ErrorCode("workspace.operation_interrupted");
    /// Falha ou timeout ao encerrar árvore de processos supervisionados.
    pub const WORKSPACE_STOP_FAILED: ErrorCode = ErrorCode("workspace.stop_failed");
    /// Persistência de comandos ou scrollback ainda pendente de drenagem.
    pub const WORKSPACE_PERSISTENCE_PENDING: ErrorCode = ErrorCode("workspace.persistence_pending");

    /// Aba solicitada não encontrada.
    pub const TAB_NOT_FOUND: ErrorCode = ErrorCode("tab.not_found");
    /// A aba está em processo de fechamento e não aceita novas mutações.
    pub const TAB_CLOSING: ErrorCode = ErrorCode("tab.closing");
    /// Conjunto de IDs de abas para reordenação diverge das abas abertas do projeto.
    pub const TAB_REORDER_MISMATCH: ErrorCode = ErrorCode("tab.reorder_mismatch");

    /// Painel solicitado não encontrado.
    pub const PANE_NOT_FOUND: ErrorCode = ErrorCode("pane.not_found");
    /// Limite máximo de 8 painéis por aba atingido.
    pub const PANE_LIMIT_REACHED: ErrorCode = ErrorCode("pane.limit_reached");
    /// Reinício de painel permitido apenas quando o painel está encerrado ou com falha.
    pub const PANE_NOT_EXITED: ErrorCode = ErrorCode("pane.not_exited");

    /// Árvore de layout de painéis inválida.
    pub const LAYOUT_INVALID: ErrorCode = ErrorCode("layout.invalid");
    /// Conflito de revisão ao atualizar proporções de layout de painéis.
    pub const LAYOUT_CONFLICT: ErrorCode = ErrorCode("layout.conflict");

    /// Sessão de aba ou painel não encontrada.
    pub const SESSION_NOT_FOUND: ErrorCode = ErrorCode("session.not_found");
    /// Parâmetros de listagem, cursor ou busca de sessão inválidos.
    pub const SESSION_QUERY_INVALID: ErrorCode = ErrorCode("session.query_invalid");

    /// Caminho inválido, inexistente ou fora do diretório monitorado no subsistema Git.
    pub const GIT_INVALID_PATH: ErrorCode = ErrorCode("git.invalid_path");
    /// Binário `git` ausente no `$PATH` do sistema.
    pub const GIT_BINARY_MISSING: ErrorCode = ErrorCode("git.binary_missing");
    /// Execução do subprocesso `git` excedeu o tempo limite configurado.
    pub const GIT_COMMAND_TIMEOUT: ErrorCode = ErrorCode("git.command_timeout");
    /// Subprocesso `git` retornou código de saída diferente de zero.
    pub const GIT_COMMAND_FAILED: ErrorCode = ErrorCode("git.command_failed");
    /// Caminho informado não pertence a um repositório Git válido.
    pub const GIT_NOT_REPOSITORY: ErrorCode = ErrorCode("git.not_repository");
    /// Nome de worktree ou branch inválido para criação gerenciada.
    pub const GIT_WORKTREE_NAME_INVALID: ErrorCode = ErrorCode("git.worktree_name_invalid");
    /// Diretório de destino ou branch da worktree já existe.
    pub const GIT_WORKTREE_CONFLICT: ErrorCode = ErrorCode("git.worktree_conflict");
    /// Worktree fora de `.worktrees/` ou `.max-code-worktrees/`, ou é a worktree principal.
    pub const GIT_WORKTREE_NOT_MANAGED: ErrorCode = ErrorCode("git.worktree_not_managed");
    /// Worktree possui alterações locais e `force=false`.
    pub const GIT_WORKTREE_DIRTY: ErrorCode = ErrorCode("git.worktree_dirty");
    /// Worktree está em uso por aba/painel aberto no `MaxTerminal`.
    pub const GIT_WORKTREE_IN_USE: ErrorCode = ErrorCode("git.worktree_in_use");
    /// Falha ao registrar observador inotify sobre metadados `.git`.
    pub const GIT_WATCHER_FAILED: ErrorCode = ErrorCode("git.watcher_failed");

    /// `/proc/net` inacessível no sistema operacional atual.
    pub const PORTS_PROCFS_UNAVAILABLE: ErrorCode = ErrorCode("ports.procfs_unavailable");
    /// Porta ativa não encontrada para o painel solicitado.
    pub const PORTS_NOT_FOUND: ErrorCode = ErrorCode("ports.not_found");
    /// Identidade `(pid, start_time_ticks)` diverge do proprietário atual do socket.
    pub const PORTS_PROCESS_MISMATCH: ErrorCode = ErrorCode("ports.process_mismatch");
    /// Permissão negada (`EPERM`) ao enviar sinal ao processo da porta.
    pub const PORTS_KILL_DENIED: ErrorCode = ErrorCode("ports.kill_denied");
    /// Processo não encerrou após escalonamento `SIGTERM -> SIGKILL`.
    pub const PORTS_KILL_TIMEOUT: ErrorCode = ErrorCode("ports.kill_timeout");
    /// Falha ao abrir URL `http://127.0.0.1:<port>/` no navegador padrão.
    pub const PORTS_OPEN_FAILED: ErrorCode = ErrorCode("ports.open_failed");

    /// Configuração de integração com o SO inválida ou fora das faixas permitidas.
    pub const OS_SETTINGS_INVALID: ErrorCode = ErrorCode("os.settings_invalid");
    /// Especificação de acelerador de atalho global inválida.
    pub const OS_SHORTCUT_INVALID: ErrorCode = ErrorCode("os.shortcut_invalid");
    /// Atalho global em conflito com outro aplicativo ou sessão gráfica.
    pub const OS_SHORTCUT_CONFLICT: ErrorCode = ErrorCode("os.shortcut_conflict");
    /// Registro de atalho global não suportado no ambiente gráfico atual.
    pub const OS_SHORTCUT_UNSUPPORTED: ErrorCode = ErrorCode("os.shortcut_unsupported");
    /// Portal XDG (`GlobalShortcuts`) ausente ou indisponível no D-Bus.
    pub const OS_PORTAL_UNAVAILABLE: ErrorCode = ErrorCode("os.portal_unavailable");
    /// Bandeja do sistema (`StatusNotifierWatcher`) indisponível na sessão atual.
    pub const OS_TRAY_UNAVAILABLE: ErrorCode = ErrorCode("os.tray_unavailable");
    /// Arquivo referenciado por link do terminal não foi encontrado no disco.
    pub const OS_LINK_NOT_FOUND: ErrorCode = ErrorCode("os.link_not_found");
    /// Candidato de link rejeitado (não é arquivo regular, tamanho/lote inválido ou fora dos limites).
    pub const OS_LINK_REJECTED: ErrorCode = ErrorCode("os.link_rejected");
    /// Binário do editor configurado não foi encontrado no `$PATH`.
    pub const OS_EDITOR_NOT_FOUND: ErrorCode = ErrorCode("os.editor_not_found");
    /// Falha ao iniciar o processo ou URI do editor externo.
    pub const OS_EDITOR_LAUNCH_FAILED: ErrorCode = ErrorCode("os.editor_launch_failed");
    /// Falha ao despachar notificação nativa para o servidor de notificações.
    pub const OS_NOTIFY_FAILED: ErrorCode = ErrorCode("os.notify_failed");
    /// Falha ao ler ou gravar o arquivo `.desktop` de inicialização automática (`autostart`).
    pub const OS_AUTOSTART_FAILED: ErrorCode = ErrorCode("os.autostart_failed");
    /// Tempo limite esgotado ao encerrar processos supervisionados durante o desligamento.
    pub const OS_SHUTDOWN_TIMEOUT: ErrorCode = ErrorCode("os.shutdown_timeout");
    /// Aba solicitada não encontrada no subsistema de integração com o SO.
    pub const OS_TAB_NOT_FOUND: ErrorCode = ErrorCode("os.tab_not_found");
    /// Contexto de painel/PTY/época de CWD tornou-se obsoleto antes ou durante a resolução do link.
    pub const OS_LINK_CONTEXT_STALE: ErrorCode = ErrorCode("os.link_context_stale");
    /// Fila de ativação (`deep-link` / `single-instance`) saturada.
    pub const OS_ACTIVATION_BUSY: ErrorCode = ErrorCode("os.activation_busy");
    /// Rota de ativação sem consumidor registrado ou expirada.
    pub const OS_ACTIVATION_UNAVAILABLE: ErrorCode = ErrorCode("os.activation_unavailable");
    /// URI ou payload de ativação malformado ou inválido.
    pub const OS_ACTIVATION_INVALID: ErrorCode = ErrorCode("os.activation_invalid");
    /// Subsistema de integração com o SO ainda não concluiu a inicialização.
    pub const OS_NOT_READY: ErrorCode = ErrorCode("os.not_ready");

    /// Cofre de credenciais bloqueado exigindo senha mestra.
    pub const VAULT_LOCKED: ErrorCode = ErrorCode("vault.locked");
    /// Cofre não configurado (nenhuma senha mestra cadastrada).
    pub const VAULT_UNCONFIGURED: ErrorCode = ErrorCode("vault.unconfigured");
    /// Senha mestra incorreta para desbloqueio do cofre.
    pub const VAULT_WRONG_PASSWORD: ErrorCode = ErrorCode("vault.wrong_password");
    /// Credencial não encontrada no cofre.
    pub const VAULT_CREDENTIAL_NOT_FOUND: ErrorCode = ErrorCode("vault.credential_not_found");
    /// Conexão SSH falhou por erro de rede ou autenticação.
    pub const SSH_CONNECTION_FAILED: ErrorCode = ErrorCode("ssh.connection_failed");
    /// Autenticação SSH bloqueada aguardando desbloqueio do cofre.
    pub const CREDENTIAL_LOCKED: ErrorCode = ErrorCode("credential.locked");
}

/// Erro único retornado por todos os comandos IPC do `MaxTerminal`.
#[derive(Debug, Serialize, specta::Type, thiserror::Error)]
#[error("[{code}] {message}")]
pub struct MtError {
    /// Código estável legível por máquina.
    pub code: ErrorCode,
    /// Mensagem descritiva em português do Brasil (pt-BR) segura para exibição na UI.
    pub message: Cow<'static, str>,
    /// Metadados estruturados opcionais (< 4 KiB, sem dados sensíveis).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub details: Option<serde_json::Value>,
    /// Causa original interna mantida apenas em memória (nunca serializada ou logada bruta).
    #[serde(skip)]
    #[specta(skip)]
    #[source]
    pub source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
}

/// Alias padrão de resultado para operações de domínio e comandos IPC.
pub type MtResult<T> = Result<T, MtError>;

impl Clone for MtError {
    fn clone(&self) -> Self {
        Self {
            code: self.code,
            message: self.message.clone(),
            details: self.details.clone(),
            source: None,
        }
    }
}

impl PartialEq for MtError {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code && self.message == other.message && self.details == other.details
    }
}

impl Eq for MtError {}

impl MtError {
    /// Cria um novo `MtError` com código e mensagem em pt-BR.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
            source: None,
        }
    }

    /// Anexa detalhes estruturados seguros ao erro.
    #[must_use]
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    /// Anexa a causa interna em memória (omitida na serialização).
    #[must_use]
    pub fn with_source(mut self, err: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(err));
        self
    }

    /// Constrói erro interno genérico (`core.internal`).
    #[must_use]
    pub fn internal(message: impl Into<Cow<'static, str>>) -> Self {
        Self::new(codes::INTERNAL, message)
    }

    /// Constrói erro de recurso não encontrado (`core.not_found`) com `entity` e `id` em `details`.
    #[must_use]
    pub fn not_found(entity: &'static str, id: impl fmt::Display) -> Self {
        Self::new(
            codes::NOT_FOUND,
            format!("Recurso '{entity}' não encontrado."),
        )
        .with_details(json!({
            "entity": entity,
            "id": id.to_string(),
        }))
    }

    /// Constrói erro de argumento inválido (`core.invalid_argument`) com `field` em `details`.
    #[must_use]
    pub fn invalid_argument(field: &'static str, reason: impl Into<Cow<'static, str>>) -> Self {
        Self::new(codes::INVALID_ARGUMENT, reason).with_details(json!({
            "field": field,
        }))
    }

    /// Constrói erro de operação não suportada (`core.unsupported`).
    #[must_use]
    pub fn unsupported(feature: &'static str) -> Self {
        Self::new(
            codes::UNSUPPORTED,
            format!("Recurso não suportado: {feature}."),
        )
        .with_details(json!({
            "feature": feature,
        }))
    }
}

impl From<io::Error> for MtError {
    fn from(err: io::Error) -> Self {
        let (code, msg) = match err.kind() {
            io::ErrorKind::PermissionDenied => (
                codes::PERMISSION_DENIED,
                "Permissão negada ao acessar recurso do sistema.",
            ),
            io::ErrorKind::NotFound => (
                codes::NOT_FOUND,
                "Arquivo ou diretório não encontrado no sistema.",
            ),
            _ => (
                codes::IO,
                "Falha de entrada/saída ao acessar o sistema de arquivos.",
            ),
        };
        Self::new(code, msg).with_source(err)
    }
}

impl From<serde_json::Error> for MtError {
    fn from(err: serde_json::Error) -> Self {
        Self::new(
            codes::SERIALIZATION,
            "Falha ao processar dados em formato JSON.",
        )
        .with_source(err)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn serializes_mt_error_contract_and_omits_source() {
        let err = MtError::not_found("project", "proj-123");
        let json_value =
            serde_json::to_value(&err).unwrap_or_else(|e| panic!("Erro ao serializar: {e}"));

        assert_eq!(json_value["code"], "core.not_found");
        assert_eq!(json_value["message"], "Recurso 'project' não encontrado.");
        assert_eq!(json_value["details"]["entity"], "project");
        assert_eq!(json_value["details"]["id"], "proj-123");
        assert!(json_value.get("source").is_none());

        // Erro sem details não deve conter o campo 'details'
        let err_no_details = MtError::new(codes::INTERNAL, "Erro interno de teste");
        let json_no_details = serde_json::to_string(&err_no_details)
            .unwrap_or_else(|e| panic!("Erro ao serializar: {e}"));
        assert!(!json_no_details.contains("\"details\""));

        // Com source anexado, source() existe mas 'source' nunca é serializado no JSON
        let io_err = io::Error::other("erro de baixo nivel");
        let err_with_src = MtError::internal("Erro com causa").with_source(io_err);
        assert!(err_with_src.source().is_some());

        let json_with_src = serde_json::to_string(&err_with_src)
            .unwrap_or_else(|e| panic!("Erro ao serializar: {e}"));
        assert!(!json_with_src.contains("\"source\""));
    }

    #[test]
    fn maps_io_and_serde_errors() {
        let perm_err = io::Error::new(io::ErrorKind::PermissionDenied, "sem permissao");
        let mt_perm: MtError = perm_err.into();
        assert_eq!(mt_perm.code, codes::PERMISSION_DENIED);
        assert!(mt_perm.source.is_some());

        let not_found_err = io::Error::new(io::ErrorKind::NotFound, "arquivo nao existe");
        let mt_not_found: MtError = not_found_err.into();
        assert_eq!(mt_not_found.code, codes::NOT_FOUND);
        assert!(mt_not_found.source.is_some());

        let other_io = io::Error::new(io::ErrorKind::BrokenPipe, "pipe quebrado");
        let mt_io: MtError = other_io.into();
        assert_eq!(mt_io.code, codes::IO);
        assert!(mt_io.source.is_some());

        let serde_err = serde_json::from_str::<serde_json::Value>("invalido{").unwrap_err();
        let mt_serde: MtError = serde_err.into();
        assert_eq!(mt_serde.code, codes::SERIALIZATION);
        assert!(mt_serde.source.is_some());
    }

    #[test]
    fn semantic_constructors_and_display() {
        let invalid = MtError::invalid_argument("nome", "deve ser preenchido");
        assert_eq!(invalid.code, codes::INVALID_ARGUMENT);
        assert_eq!(invalid.message, "deve ser preenchido");
        assert_eq!(invalid.details, Some(json!({ "field": "nome" })));

        let unsupported = MtError::unsupported("gpu_vulkan");
        assert_eq!(unsupported.code, codes::UNSUPPORTED);
        assert_eq!(unsupported.message, "Recurso não suportado: gpu_vulkan.");
        assert_eq!(
            unsupported.details,
            Some(json!({ "feature": "gpu_vulkan" }))
        );

        let code = codes::INTERNAL;
        assert_eq!(code.to_string(), "core.internal");
        let display_msg = format!("{invalid}");
        assert_eq!(display_msg, "[core.invalid_argument] deve ser preenchido");
    }
}
