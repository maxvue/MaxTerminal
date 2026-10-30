# Implementação 02 — Templates Dedicados, Lançamento no Shell, Isolamento em Worktree e Relay (`spec.md`)

> Fase: **13 — Agentes CLI, Telemetria e MaxSkills**
> Referências: [`../instructions.md`](../instructions.md) (I-04, I-06, I-07, I-17; CA-08, CA-09, CA-12, CA-13, CA-14, CA-15, CA-16, CA-21) e [`../spec.md`](../spec.md) (§1.2, §1.3, §2.6–§2.8, §2.13, §3, §7.4, §8.1–§8.4, §9).

---

## 1. Contexto, Objetivo e Escopo

Esta segunda unidade implementa toda a orquestração de lançamento de Agentes CLI dentro do shell do usuário, o isolamento opcional em Git Worktrees e o canal IPC autenticado via Unix domain socket + binário sidecar (`mt-agent-relay`):

1. **Relay Server (`relay/`) e Binário Sidecar (`mt-agent-relay`)**:
   - Servidor Unix domain socket em `$XDG_RUNTIME_DIR/maxterminal/agent-relay.sock` (fallback `$XDG_STATE_HOME/maxterminal/run/agent-relay.sock`) com permissão `0600` (diretório `0700`), verificação `SO_PEERCRED` (`uid == getuid()`), limite de leitura de 64 KiB com timeout de 1 s e autenticação por token de 256 bits (`OsRng`, codificado em `base64url`) comparado por hash SHA-256 em tempo constante (`subtle::ConstantTimeEq`).
   - Binário leve `src-tauri/crates/mt-agents/src/bin/mt-agent-relay.rs` (usando apenas `std`, `serde_json` e `base64`, sem runtime async) que atende `statusline [--chain <b64>]`, `hook <Evento>` (sempre exit 0 e stdout vazio) e `codex-notify [--chain <b64>] <json>`.
2. **Planejador de Lançamento (`launch.rs`) e `AgentResolver` (`src-tauri/src/agents/resolver.rs`)**:
   - Criação idempotente dos 4 templates dedicados por projeto (`agent_templates_ensure`) com rastreamento em `agent_template_seeds` (respeitando exclusões manuais do usuário quando `template_id IS NULL`) e opções em `agent_template_options`.
   - Implementação do `EnvironmentResolver` da Fase 07 (`AgentResolver`) que delega ao `ShellResolver` para iniciar o login shell com OSC 133 e compõe o `initial_input` com quoting específico para `bash`, `zsh` e `fish`.
   - Injeção segura e reversível de `--settings <runtime_dir>/claude-settings.json` para o Claude Code (resolvendo a configuração efetiva antes de encadear o `statusLine` original em `--chain <b64>` — R-04, CA-21) e `-c notify=[...]` para o Codex (encadeando `notify` original).
3. **Isolamento em Git Worktree (`isolation.rs`)**:
   - Proposta somente-leitura (`agent_worktree_propose`) calculando slug, branch (`agent/<short>/<slug>`), caminho (`.worktrees/wt-<slug>` ou `.max-code-worktrees/wt-<slug>`), `base_ref` (OID completo) e checagem de `.gitignore` (`git check-ignore -q`).
   - Abertura (`agent_tab_open`) suportando `WorktreeIsolation::{None, Create, Existing}` com registro durável em `agent_launch_operations` e vínculo em `agent_tab_links`, rollback seguro caso `open_tab` falhe após criar worktree limpa, e reconciliação no startup.
   - Inspeção pós-fechamento (`WorkspaceObserver::on_tab_closed` / `agent_tab_close_check`) e resolução conservadora (`agent_worktree_resolve`: `Keep`, `Remove`, `RemoveAndDeleteBranch`) validada por `inspection_token` sob lock por repositório.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Arquivos Criados
- `src-tauri/crates/mt-agents/src/relay/mod.rs`
- `src-tauri/crates/mt-agents/src/relay/protocol.rs`
- `src-tauri/crates/mt-agents/src/relay/server.rs`
- `src-tauri/crates/mt-agents/src/bin/mt-agent-relay.rs`
- `src-tauri/crates/mt-agents/src/launch.rs`
- `src-tauri/crates/mt-agents/src/isolation.rs`
- `src-tauri/src/agents/mod.rs`
- `src-tauri/src/agents/adapters.rs`
- `src-tauri/src/agents/resolver.rs`
- `src-tauri/crates/mt-agents/tests/relay.rs`
- `src-tauri/crates/mt-agents/tests/launch.rs`
- `src-tauri/crates/mt-agents/tests/isolation.rs`

### 2.2. Arquivos Modificados
- `src-tauri/crates/mt-agents/Cargo.toml` — declaração do target `[[bin]] name = "mt-agent-relay" path = "src/bin/mt-agent-relay.rs"`.
- `src-tauri/crates/mt-agents/src/lib.rs` — exposição dos módulos `relay`, `launch`, `isolation` e estrutura `AgentRuntimeIndex`.
- `src-tauri/src/commands/agents.rs` — adição dos comandos `agent_templates_ensure`, `agent_template_options_get`, `agent_template_options_set`, `agent_worktree_propose`, `agent_tab_open`, `agent_tab_link_get`, `agent_tab_close_check`, `agent_worktree_resolve`, `agent_worktree_pending_list`.
- `src-tauri/src/specta.rs` e `src-tauri/src/lib.rs` — registro dos novos comandos, dos eventos `agent://link-changed` e `agent://worktree-close-pending`, do `AgentResolver` no `EnvironmentRegistry` (Fase 07) e do `AgentWorkspaceObserver`.
- `src-tauri/tauri.conf.json` — adição de `"binaries/mt-agent-relay"` em `bundle.externalBin`.

---

## 3. Especificação Técnica Detalhada

### 3.1. Protocolo e Servidor Relay (`crates/mt-agents/src/relay/`)

```rust
// relay/protocol.rs
use serde::{Deserialize, Serialize};

pub const RELAY_PROTOCOL_VERSION: u8 = 1;
pub const MAX_RELAY_MESSAGE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayEnvelope {
    pub v: u8,
    pub token: String,
    #[serde(flatten)]
    pub msg: RelayMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RelayMessage {
    Statusline {
        payload: serde_json::Value,
    },
    Hook {
        event: String,
        payload: serde_json::Value,
    },
    CodexNotify {
        payload: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayAck {
    pub ok: bool,
    pub error: Option<String>,
}
```

```rust
// relay/server.rs
use std::path::{Path, PathBuf};
use std::sync::Arc;
use dashmap::DashMap;
use mt_core::{AppPaths, MtResult, TabId};
use tokio::sync::mpsc;
use crate::ports::RunBinding;

/// Token opaco de 256 bits codificado em base64url sem padding; Debug nunca imprime o segredo.
#[derive(Clone)]
pub struct RelayToken(String);

impl RelayToken {
    pub fn generate() -> (Self, [u8; 32]); // retorna token base64url + SHA-256 dos bytes decodificados
    pub fn as_str(&self) -> &str;
}

impl std::fmt::Debug for RelayToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RelayToken([REDACTED])")
    }
}

#[derive(Debug, Clone)]
pub struct AuthenticatedRelayEvent {
    pub binding: RunBinding,
    pub message: RelayMessage,
    pub received_at_ms: i64,
}

pub struct RelayServer {
    socket_path: PathBuf,
    tokens: DashMap<[u8; 32], RunBinding>,
    tx: mpsc::Sender<AuthenticatedRelayEvent>,
    shutdown: tokio_util::sync::CancellationToken,
}

impl RelayServer {
    pub const CHANNEL_CAPACITY: usize = 256;

    pub async fn bind(
        paths: &AppPaths,
        tx: mpsc::Sender<AuthenticatedRelayEvent>,
    ) -> MtResult<Arc<Self>>;

    pub fn issue(&self, binding: RunBinding) -> RelayToken;
    pub fn revoke_run(&self, run_id: &str) -> bool;
    pub fn revoke_tab(&self, tab: TabId) -> usize;
    pub fn socket_path(&self) -> &Path;
    pub async fn shutdown(&self);
}
```

#### Regras de Segurança e Ciclo de Vida do `RelayServer`:
1. Antes de criar o `UnixListener`, cria o diretório pai com modo `0700`. Se `socket_path` já existir:
   - Verifica `symlink_metadata`: se não for socket (`FileType::is_socket`), recusa remover e retorna `agents.relay_unavailable`.
   - Tenta conectar com timeout de 100 ms: se houver listener ativo (outra instância do app), **não** remove o socket existente e usa um sufixo por PID (`agent-relay-<pid>.sock`); se retornar `ConnectionRefused` / `NotFound`, remove o socket órfão e faz `bind`, ajustando permissão para `0600`.
2. Para cada conexão aceita:
   - Verifica `stream.peer_cred()?.uid() == nix::unistd::geteuid().as_raw()`. Se divergir, fecha imediatamente.
   - Lê até `\n` com `AsyncReadExt::take(64 * 1024 + 1)` e timeout de 1 s. Se exceder 64 KiB, responde `RelayAck { ok: false, error: Some("payload_too_large".into()) }`.
   - Decodifica o token `base64url`, calcula `Sha256::digest` e busca na tabela `tokens` comparando o digest com `subtle::ConstantTimeEq`. Token desconhecido ou já revogado responde `RelayAck { ok: false, error: None }` sem vazar detalhes (R-05).
   - Envia `AuthenticatedRelayEvent` no `mpsc::Sender` com `try_send`. Se a fila estiver cheia e a mensagem for `Hook` ou `CodexNotify`, registra sinal de reconciliação para que o detector de atenção não presuma resolução silenciosa.
3. Tokens são revogados imediatamente em `on_pane_exited`, `on_session_archived` e `on_tab_closed`.

---

### 3.2. Binário Sidecar (`crates/mt-agents/src/bin/mt-agent-relay.rs`)

Interface de linha de comando:
1. `mt-agent-relay statusline [--chain <base64_cmd>]`
   - Lê todo o `stdin` (limitado a 64 KiB).
   - Se `MAXTERMINAL_AGENT_TOKEN` e `MAXTERMINAL_RELAY_SOCK` estiverem presentes e não vazios, parseia o `stdin` como `serde_json::Value` e envia `RelayEnvelope { v: 1, token, msg: RelayMessage::Statusline { payload } }` ao `UnixStream` com timeout total (connect + write + read) de 150 ms. Qualquer falha no socket é silenciosamente ignorada.
   - Se `--chain <b64>` foi fornecido: decodifica a string em UTF-8, invoca `sh -c "<cmd>"` passando exatamente os bytes lidos do `stdin`, aguarda até 1 s (matando o filho em timeout) e copia o `stdout` resultante para o `stdout` do relay.
   - Se `--chain` não foi fornecido: extrai `model.display_name` (fallback `"Claude"`) e `context_window.used_percentage` do JSON e imprime `"<display_name> · ctx <pct>%"` (ou apenas `"<display_name>"` quando `used_percentage` for nulo/ausente).
   - Sempre encerra com código de saída `0`.
2. `mt-agent-relay hook <EventName>`
   - Lê `stdin` (até 64 KiB), parseia como JSON (fallback `Value::Null`) e envia `RelayMessage::Hook { event, payload }` ao socket (timeout 150 ms).
   - **Sempre** termina com exit code `0` e `stdout` completamente vazio (CA-21 / spec §2.8: exit 2 bloquearia a ferramenta no Claude Code e qualquer texto no stdout de `UserPromptSubmit` contaminaria o prompt do usuário).
3. `mt-agent-relay codex-notify [--chain <base64_argv_json>] <json_payload>`
   - Envia `RelayMessage::CodexNotify { payload }` ao socket (timeout 150 ms).
   - Se `--chain <b64>` foi passado (contendo `Vec<String>` serializado em JSON e codificado em base64 do `notify` original do usuário), executa o programa original acrescentando `<json_payload>` como último argumento (timeout 1 s).
   - Sempre termina com exit code `0`.

---

### 3.3. Lançamento e Templates Dedicados (`crates/mt-agents/src/launch.rs` e `src-tauri/src/agents/resolver.rs`)

```rust
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use mt_core::{AppPaths, MtResult, ProjectId, TabId, TemplateId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::detect::AgentInstallation;
use crate::isolation::AgentTabLinkDraft;
use crate::ports::RunBinding;
use crate::registry::{AgentDescriptor, AgentKind};
use crate::relay::server::RelayToken;
use crate::settings::{AgentsSettings, WorktreeLocationDto};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateOptions {
    pub model: Option<String>,
    pub extra_args: Vec<String>,
    pub isolation_default: bool,
    pub location: WorktreeLocationDto,
}

impl Default for AgentTemplateOptions {
    fn default() -> Self {
        Self {
            model: None,
            extra_args: Vec::new(),
            isolation_default: false,
            location: WorktreeLocationDto::Project,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Bash,
    Zsh,
    Fish,
    Posix,
}

#[derive(Debug, Clone)]
pub struct PendingLaunch {
    pub launch_id: Uuid,
    pub agent: AgentKind,
    pub model: Option<String>,
    pub extra_args: Vec<String>,
    pub link: Option<AgentTabLinkDraft>,
}

#[derive(Debug, Clone)]
pub struct LaunchPlan {
    pub kind: AgentKind,
    pub run_binding: RunBinding,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub token: RelayToken,
    pub runtime_dir: PathBuf,
    pub configured_model: Option<String>,
}

pub struct LaunchPlanner {
    paths: AppPaths,
    relay_bin: PathBuf,
}

impl LaunchPlanner {
    pub fn new(paths: AppPaths, relay_bin: PathBuf) -> Self;

    /// Valida e traduz um ID de modelo selecionado (do template ou do catálogo da Fase 12)
    /// para o argumento CLI local do agente. Retorna `Err(agents.model_unsupported)` se o agente
    /// não suportar seleção de modelo por flag ou se o modelo for incompatível.
    pub fn resolve_cli_model(
        descriptor: &AgentDescriptor,
        requested_model: Option<&str>,
    ) -> MtResult<Option<String>>;

    pub fn build_plan(
        &self,
        descriptor: &AgentDescriptor,
        installation: &AgentInstallation,
        opts: &AgentTemplateOptions,
        pending: Option<&PendingLaunch>,
        binding: RunBinding,
        token: RelayToken,
        relay_sock: &Path,
        settings: &AgentsSettings,
    ) -> MtResult<LaunchPlan>;
}

/// Monta a linha de comando inicial para enviar ao login shell após `PromptStart`.
pub fn command_line(
    plan: &LaunchPlan,
    initial_command: Option<&str>,
    shell: ShellKind,
) -> MtResult<String>;
```

#### Regras de Quoting por Shell (`command_line`) e Injeção de Configurações:
1. **Serialização segura de argumentos por shell**:
   - `Bash` / `Zsh` / `Posix`: cada argumento é escapado com aspas simples `'...'`, substituindo `'` interno por `'\''` (ou via `shell_words::quote`).
   - `Fish`: em `fish`, dentro de `'...'` apenas `\` e `'` são especiais (`\'` e `\\`). Implementar `quote_fish_arg(arg)` dedicado para garantir que metacaracteres (`$`, `*`, espaços, aspas duplas, `~`, `;`) nunca sejam expandidos indevidamente.
   - Se `initial_command` existir e não for vazio: agrupar em bloco `{ <initial_command>; } && <agent_cmd>` no `bash`/`zsh` e `begin; <initial_command>; end; and <agent_cmd>` no `fish`, preservando que o agente só inicia se o comando preparatório terminar com código 0.
2. **Injeção no Claude Code (`--settings <runtime_dir>/claude-settings.json`)**:
   - Quando `settings.inject[Claude] == true` e `extra_args` do usuário não já contiver `--settings`:
   - Inspeciona a configuração efetiva do Claude Code (`~/.claude/settings.json`, `<cwd>/.claude/settings.json`, `<cwd>/.claude/settings.local.json`). Se existir política gerenciada (`managed-settings.json`) ou formato indecifrável que torne ambígua a precedência, **não** injeta `statusLine` conflitante (R-04).
   - Caso exista um `statusLine.command` configurado pelo usuário, codifica o comando em Base64 (`STANDARD`) e gera `"<relay_bin> statusline --chain <b64>"`; caso contrário gera `"<relay_bin> statusline"`.
   - Grava `<runtime_dir>/claude-settings.json` com permissão `0600` (diretório `0700`) contendo `statusLine` e os hooks observacionais `Notification`, `Stop`, `UserPromptSubmit` e `PreToolUse`.
3. **Injeção no Codex (`-c notify=[...]`)**:
   - Quando `settings.inject[Codex] == true`: lê `~/.codex/config.toml` se existir; se houver `notify = [...]` válido, serializa o array original em JSON + Base64 e adiciona `-c notify=["<relay_bin>","codex-notify","--chain","<b64>"]`; se não houver, adiciona `-c notify=["<relay_bin>","codex-notify"]`. Se o `config.toml` do usuário não puder ser parseado com segurança, omite a injeção para não sobrescrever preferências do usuário.
4. **Variáveis de ambiente injetadas no `SpawnSpec` do shell**:
   - `MAXTERMINAL_AGENT = descriptor.kind.as_ref_id()`
   - `MAXTERMINAL_AGENT_TOKEN = plan.token.as_str()` *(nunca gravado em `args`, logs ou SQLite)*
   - `MAXTERMINAL_RELAY_SOCK = relay_sock.display().to_string()`
   - `MAXTERMINAL_RELAY_BIN = relay_bin.display().to_string()`
   - A chave transitória `MAXTERMINAL_AGENT_LAUNCH` usada em `overrides.extra_env` para correlacionar `open_tab` ao `PendingLaunch` é **obrigatoriamente removida** da cópia final de `base.spec.env` antes do spawn do PTY.

---

### 3.4. Isolamento em Git Worktree (`crates/mt-agents/src/isolation.rs`)

```rust
use std::path::PathBuf;
use std::sync::Arc;
use dashmap::DashMap;
use mt_core::{MtResult, ProjectId, TabId, TemplateId};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use crate::ports::{GitPort, ProjectRecord, PtyPort, WorkspacePort};
use crate::registry::AgentKind;
use crate::settings::{AgentsSettings, WorktreeLocationDto};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum WorktreeIsolation {
    None,
    Create {
        location: WorktreeLocationDto,
        slug: String,
        branch: String,
        base: Option<String>,
        ensure_gitignore: bool,
    },
    Existing {
        path: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeProposal {
    pub repo_root: String,
    pub slug: String,
    pub name: String,
    pub branch: String,
    pub path: String,
    pub location: WorktreeLocationDto,
    pub root_ignored: bool,
    pub warnings: Vec<String>,
    pub base_ref: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentTabOpenRequest {
    pub project_id: ProjectId,
    pub agent: AgentKind,
    pub template_id: Option<TemplateId>,
    pub isolation: WorktreeIsolation,
    pub model: Option<String>,
    pub title: Option<String>,
    pub focus: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeResolution {
    Keep,
    Remove,
    RemoveAndDeleteBranch,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentTabLink {
    pub id: i64,
    pub tab_id: TabId,
    pub project_id: ProjectId,
    pub agent: AgentKind,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub base_ref: Option<String>,
    pub created_by_app: bool,
    pub opened_at_ms: i64,
    pub tab_closed_at_ms: Option<i64>,
    pub resolution: Option<WorktreeResolution>,
    pub resolved_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentCloseInspection {
    pub link: AgentTabLink,
    pub exists: bool,
    pub dirty: bool,
    pub changed_files: Vec<String>,
    pub changed_files_truncated: bool,
    pub ahead_of_base: u32,
    pub has_ignored_files: bool,
    pub inspection_complete: bool,
    pub in_use: bool,
    pub branch_merged: bool,
    pub inspection_token: String,
    pub allowed: Vec<WorktreeResolution>,
    pub requires_confirmation: bool,
}

pub struct IsolationService {
    git: Arc<dyn GitPort>,
    pty: Arc<dyn PtyPort>,
    workspace: Arc<dyn WorkspacePort>,
    db: mt_db::Db,
    settings: Arc<RwLock<AgentsSettings>>,
    repo_locks: DashMap<PathBuf, Arc<Mutex<()>>>,
}

impl IsolationService {
    pub async fn propose(
        &self,
        project: &ProjectRecord,
        agent: AgentKind,
        hint: Option<&str>,
    ) -> MtResult<WorktreeProposal>;

    pub async fn prepare(
        &self,
        project: &ProjectRecord,
        agent: AgentKind,
        iso: &WorktreeIsolation,
    ) -> MtResult<Option<PreparedWorktree>>;

    pub async fn rollback(&self, prepared: &PreparedWorktree) -> MtResult<()>;
    pub async fn mark_tab_closed(&self, tab_id: TabId, now_ms: i64) -> MtResult<Option<AgentCloseInspection>>;
    pub async fn inspect(&self, link_id: i64) -> MtResult<AgentCloseInspection>;
    pub async fn inspect_by_tab(&self, tab_id: TabId) -> MtResult<Option<AgentCloseInspection>>;
    pub async fn resolve(
        &self,
        link_id: i64,
        action: WorktreeResolution,
        inspection_token: &str,
    ) -> MtResult<AgentTabLink>;
    pub async fn list_pending(&self, project_id: Option<ProjectId>) -> MtResult<Vec<AgentCloseInspection>>;
    pub async fn reconcile_startup(&self) -> MtResult<u32>;
}
```

#### Regras de Negócio de Isolamento, Inspeção e Resolução:
1. **`propose` (somente leitura)**:
   - Descobre o repositório Git via `mt_git::discover(&project.path)` (se não for repo Git, retorna `Err(agents.not_a_repo)`).
   - Resolve `HEAD` para OID completo de 40 caracteres (`git rev-parse --verify HEAD`).
   - Gera `slug = slugify(hint)` (ASCII `[a-z0-9-]`, máx. 40 chars) ou `<agent.short()>-<YYYYMMDD-HHmm>`. Se `<repo_root>/<location_dir>/wt-<slug>` ou `refs/heads/<branch_prefix>/<agent.short()>/<slug>` já existir, incrementa sufixo `-2`, `-3`, ...
   - Executa `git check-ignore -q -- <location_dir>/` no `repo_root`: se exit != 0, `root_ignored = false` e adiciona aviso em pt-BR: `"A pasta de worktrees (.worktrees/) não está no .gitignore deste repositório."` (CA-13).
2. **`prepare` e `agent_launch_operations`**:
   - Para `WorktreeIsolation::None`: retorna `Ok(None)`.
   - Para `WorktreeIsolation::Existing { path }`: valida com `mt_git::discover(path)` que pertence ao mesmo `common_dir` canônico do projeto; caso contrário retorna `Err(agents.worktree_foreign)`. Grava `created_by_app = false` (CA-15).
   - Para `WorktreeIsolation::Create { ... }`: grava registro `stage = 'reserved'` em `agent_launch_operations`, chama `GitPort::create_worktree`, atualiza `stage = 'worktree_ready'` e retorna `PreparedWorktree` com `created_by_app = true`.
3. **Inspeção (`inspect`) e `inspection_token`**:
   - Verifica se a aba já foi fechada (`tab_closed_at_ms.is_some()`), se não há processos vivos na aba (`!pty.has_active_processes(link.tab_id)`) e se nenhuma outra aba aberta usa `worktree_path` (`workspace.tabs_using_cwd`). Caso contrário, `in_use = true`.
   - Se `!link.created_by_app` ou se `worktree_path` for o checkout principal do repositório: `allowed = vec![WorktreeResolution::Keep]`.
   - Para worktree criada pelo app (`created_by_app == true`) e `exists == true`:
     - Executa `git status --porcelain=v1 -z --ignored=matching --untracked-files=all` na worktree:
       - Entradas `!!` marcam `has_ignored_files = true`.
       - Demais entradas compõem `changed_files` (até 50 itens; se > 50, `changed_files_truncated = true`) e marcam `dirty = true`.
     - Verifica submódulos sujos e presença de `.git/index.lock` na worktree.
     - Calcula `ahead_of_base` via `git rev-list --count <base_ref>..HEAD`.
     - Calcula `branch_merged`: verifica se `HEAD` da worktree é ancestral da branch padrão/base de integração (`git merge-base --is-ancestor HEAD <target_ref>`) e se a branch da worktree contém commits ou se está no mesmo commit da base.
     - Só permite `WorktreeResolution::Remove` quando `inspection_complete && !in_use && !dirty && !has_ignored_files`.
     - Só permite `WorktreeResolution::RemoveAndDeleteBranch` quando `Remove` for permitido **e** `branch_merged == true` (sem commits exclusivos não mesclados — CA-14).
     - Gera `inspection_token` como SHA-256 hex de `(link.id, worktree_path, head_oid, dirty, has_ignored_files, in_use, branch_merged)`.
4. **Resolução (`resolve`)**:
   - Adquire o `Mutex` do `repo_root` em `repo_locks`.
   - Reexecuta `inspect(link_id)` internamente e compara o `inspection_token` recém-calculado com o token enviado pelo cliente: se diferir, retorna `Err(agents.inspection_stale)`.
   - Se `in_use`: retorna `Err(git.worktree_in_use)` / `Err(agents.execution_in_use)`.
   - Se a ação for `Remove` ou `RemoveAndDeleteBranch` e `(dirty || has_ignored_files || !inspection_complete)`: retorna `Err(agents.worktree_dirty)`.
   - Para `Remove`: chama `GitPort::remove_worktree(repo_root, worktree_path, false)` (sempre `force = false`).
   - Para `RemoveAndDeleteBranch`: remove a worktree com `force = false` e em seguida executa `git branch -d -- <branch>` no `repo_root` (nunca `-D`).
   - Grava `resolution` e `resolved_at` em `agent_tab_links` e emite `agent://link-changed`.

---

## 4. Casos de Borda e Tratamento de Erros

1. **Exclusão manual de template de agente pelo usuário**:
   - `agent_template_seeds` possui `template_id TEXT NULL REFERENCES tab_templates(id) ON DELETE SET NULL`. Se o usuário excluir o template "Claude Code", a linha em `agent_template_seeds` permanece com `template_id = NULL`. Chamadas subsequentes a `agent_templates_ensure` **não** recriam o template excluído automaticamente (`spec.md` §8.1).
2. **Falha de `WorkspaceManager::open_tab` após criar a worktree**:
   - `IsolationService::rollback` verifica se a worktree recém-criada continua limpa (sem arquivos criados por hooks `post-checkout` e `HEAD == base_oid`). Se estiver limpa, remove a worktree e a branch recém-criada e marca a operação como `resolved`; se um hook escreveu arquivos, preserva a worktree, marca `stage = 'recovery_required'` e inclui na lista de pendências para decisão do usuário (`spec.md` §2.7 e §8.2).
3. **Fechamento da aba com processo do agente ainda rodando (CA-16)**:
   - A decisão sobre a worktree nunca ocorre em um interceptador antes do fechamento, e sim no `WorkspaceObserver::on_tab_closed` **após** a Fase 02/07 ter encerrado a árvore de processos do PTY.
