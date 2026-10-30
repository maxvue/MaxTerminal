# Implementação 03 — Sincronização de Projeto Ativo, Worktrees e Construtor Sanitizado do Prompt Inspecionar (`spec.md`)

> Escopo da Fase 11: [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md) (§9, §10).
> Depende de: **Implementação 01** e **Implementação 02**.

---

## 1. Contexto, Objetivo e Escopo Exato

Esta é a **terceira unidade de implementação da Fase 11**. Ela completa todos os módulos de domínio do crate `mt-bridge` implementando:
1. Resolução canônica de caminhos de projetos e worktrees Git (`.worktrees/`, `.max-code-worktrees/` ou worktrees externas) com verificação real dos metadados `.git` (`gitdir`, `commondir` e vínculo reverso `<gitdir>/gitdir`), proteção contra nomes de pasta forjados e cache LRU com TTL de 30 s executado via `tokio::task::spawn_blocking` em `src/paths.rs`.
2. Motor de sincronização bidirecional de projeto ativo (`ActiveProjectSync` + `LoopGuard`) com propagação causal de `SyncContext { origin, correlation_id }`, supressão determinística de eco (independente de tempo decorrido ou reconexão), debounce de 250 ms para seleção manual local e confirmação de aplicação pela UI em `src/sync.rs`.
3. Construtor do prompt canônico do botão **"Inspecionar"** em `src/prompt.rs`, gerando exatamente o formato exigido pelo AGENTS.md §6:
   `Ao executar o comando "${comando}" tive o seguinte erro: "${erro}"`
   com remoção prévia de sequências ANSI/OSC e `\r` de barras de progresso, sanitização obrigatória antes do truncamento em fronteira UTF-8 (`${comando}` ≤ 2000 chars; `${erro}` últimas 80 linhas e ≤ 6000 chars com prefixo `[…] `) e verificação final anti-vazamento de segredos.
4. Integração de `ActiveProjectSync`, `paths` e `prompt` nos handlers de `server.rs` (`project.opened`, `project.closed`, `terminal.open_tab`) e no `BridgeHandle` (`src/lib.rs`).

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

| Ação | Caminho Exato | Descrição |
|------|---------------|-----------|
| Criar | `src-tauri/crates/mt-bridge/src/paths.rs` | `canonicalize_project_path`, `ResolvedPath`, `resolve_worktree_root`, `match_project`, `validate_cwd_within_project` e cache LRU de 128 entradas (TTL 30 s). |
| Criar | `src-tauri/crates/mt-bridge/src/sync.rs` | `LoopGuard`, `ActiveProjectSync`, debounce de 250 ms, confirmação de foco da UI e supressão causal de loop. |
| Criar | `src-tauri/crates/mt-bridge/src/prompt.rs` | `INSPECT_TEMPLATE`, `InspectLimits`, `InspectPrompt`, `build_inspect_prompt`, `truncate_utf8_prefix`, `truncate_utf8_suffix` e limpeza ANSI/CR. |
| Modificar | `src-tauri/crates/mt-bridge/src/server.rs` | Integrar despacho de `Message::ProjectOpened`, `Message::ProjectClosed` e `Message::TerminalOpenTab` usando `ActiveProjectSync`, `paths` e `BridgeHost`. |
| Modificar | `src-tauri/crates/mt-bridge/src/lib.rs` | Expor módulos `paths`, `prompt`, `sync`, adicionar `BridgeHandle::sync(&self) -> &ActiveProjectSync` e `BridgeHandle::confirm_active_project`. |
| Modificar | `src-tauri/crates/mt-bridge/tests/server_it.rs` | Adicionar testes de integração de `project.opened` com worktree `.max-code-worktrees/wt-x`, prevenção de loop (CA-24, CA-25, CA-26) e `terminal.open_tab`. |

---

## 3. Assinaturas Exatas e Completas (Rust)

### 3.1. `src-tauri/crates/mt-bridge/src/paths.rs`

```rust
use crate::error::BridgeError;
use crate::handler::ProjectRef;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPath {
    pub canonical: PathBuf,
    pub main_root: PathBuf,
    pub worktree: Option<PathBuf>,
}

/// Executa `std::fs::canonicalize(p)` e normaliza removendo barra final redundante.
/// Retorna `Err(BridgeError::InvalidPath)` se o caminho não for absoluto ou não existir.
pub fn canonicalize_project_path(p: &Path) -> Result<PathBuf, BridgeError>;

/// Sobe a árvore de diretórios a partir de `p` (já canonicalizado) procurando `.git`:
/// - Se `.git` for um diretório: `main_root = dir_pai`, `worktree = None`.
/// - Se `.git` for um arquivo regular contendo `gitdir: <caminho>`:
///   1) Resolve `<gitdir>` (relativo ao diretório que contém `.git`, se não for absoluto) e canonicaliza.
///   2) Verifica o vínculo reverso lendo `<gitdir>/gitdir` e confirmando que aponta para o arquivo `.git`
///      da worktree atual (evita que uma pasta com `.git` forjado aponte para outro repositório).
///   3) Lê `<gitdir>/commondir`, resolve contra `<gitdir>`, canonicaliza o diretório `.git` principal
///      e define `main_root` como seu diretório pai, e `worktree = Some(raiz_da_worktree)`.
///   4) Se os metadados `.git` da suposta worktree forem inválidos/inconsistentes, retorna `Err(BridgeError::InvalidPath)`.
/// - Se nenhum `.git` for encontrado na subida: `main_root = canonical`, `worktree = None`.
/// Nota: nomes `.worktrees` ou `.max-code-worktrees` NUNCA são aceitos como prova de worktree sem validação real do `.git`.
pub fn resolve_worktree_root(p: &Path) -> Result<ResolvedPath, BridgeError>;

/// Versão assíncrona com cache LRU de 128 entradas (TTL 30 s) executada em `tokio::task::spawn_blocking`.
pub async fn resolve_worktree_root_cached(p: PathBuf) -> Result<ResolvedPath, BridgeError>;

/// Faz o match contra a lista de projetos cadastrados na seguinte ordem de prioridade:
/// 1) Projeto cujo `path` canônico é igual a `resolved.worktree` (quando a worktree foi cadastrada diretamente).
/// 2) Projeto cujo `path` canônico é igual a `resolved.main_root`.
/// 3) Projeto cujo `path` canônico é ancestral por componentes (`Path::starts_with`) de `resolved.canonical`
///    (apenas quando `resolved.worktree` é `None`, escolhendo o ancestral mais específico).
pub fn match_project<'a>(
    resolved: &ResolvedPath,
    projects: &'a [ProjectRef],
) -> Option<&'a ProjectRef>;

/// Valida que `cwd` (absoluto ou relativo à raiz do projeto) existe, canonicaliza-o no momento do uso
/// (evitando TOCTOU/escape por symlink) e comprova que pertence a `project_root` ou a uma worktree
/// legitimamente vinculada a `project_root`. Caso contrário retorna `Err(BridgeError::CwdOutsideProject)`.
pub fn validate_cwd_within_project(
    project_root: &Path,
    cwd: &Path,
) -> Result<PathBuf, BridgeError>;
```

### 3.2. `src-tauri/crates/mt-bridge/src/sync.rs`

```rust
use crate::client::MaxCodeClient;
use crate::error::BridgeError;
use crate::handler::{BridgeHost, FocusRequest, ProjectRef, SyncContext};
use crate::protocol::{AckPayload, Envelope, PeerApp, ProjectPathPayload};
use mt_core::ProjectId;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

pub const LOOP_GUARD_TTL: Duration = Duration::from_secs(300);
pub const LOOP_GUARD_LIMIT: usize = 1024;
pub const LOCAL_SELECTION_DEBOUNCE: Duration = Duration::from_millis(250);
pub const UI_FOCUS_CONFIRM_TIMEOUT: Duration = Duration::from_secs(3);

pub struct LoopGuard {
    recent: Mutex<HashMap<Uuid, (SyncContext, std::time::Instant)>>,
    ttl: Duration,
    limit: usize,
}

impl LoopGuard {
    pub fn new(limit: usize, ttl: Duration) -> Self;

    /// Registra o `SyncContext` no histórico recente (expurgando entradas > `ttl` ou mais antigas se cheio).
    pub fn record(&self, context: SyncContext);

    /// Retorna `true` se o envio para `outgoing_to` deve ser suprimido:
    /// - Se `context.origin == outgoing_to` (ex.: seleção originada por ` PeerApp::Maxcode` nunca é enviada de volta para `Maxcode`,
    ///   independentemente de quanto tempo passou — 1500 ms, 10 s ou após reconexão).
    /// - Ou se `context.correlation_id` já foi processado com origem remota correspondente.
    pub fn should_suppress(&self, context: &SyncContext, outgoing_to: PeerApp) -> bool;

    /// Verifica se uma mensagem recebida carrega um `correlation_id` que foi originado localmente por `PeerApp::Maxterminal`.
    pub fn is_own_correlation(&self, correlation_id: &Uuid) -> bool;
}

pub struct ActiveProjectSync {
    guard: Arc<LoopGuard>,
    enabled: Arc<AtomicBool>,
    debounce: Duration,
    // Task de debounce cancelável por geração + mapa de confirmações pendentes da UI
}

impl ActiveProjectSync {
    pub fn new(
        enabled: bool,
        debounce: Duration,
        client: Arc<MaxCodeClient>,
    ) -> Self;

    pub fn is_enabled(&self) -> bool;

    /// Habilita ou desabilita a sincronização imediatamente; ao desabilitar, incrementa a geração,
    /// cancela qualquer timer de debounce em andamento e rejeita pendências em voo.
    pub fn set_enabled(&self, on: bool);

    /// Processa `project.opened` vindo do MaxCode:
    /// 1) Se `!is_enabled()`, retorna `AckPayload { handled: false, reason: Some("sync_disabled".into()), .. }`.
    /// 2) Se `env.origin != PeerApp::Maxcode`, retorna erro de validação.
    /// 3) Se `guard.is_own_correlation(&env.correlation_id)`, reconhece eco e retorna `AckPayload { handled: true, .. }` sem reaplicar foco.
    /// 4) Resolve caminho e worktree via `resolve_worktree_root_cached` e busca match em `host.list_projects()`.
    ///    Se não encontrar projeto, retorna `AckPayload { handled: false, reason: Some("project_not_found".into()), .. }`.
    /// 5) Registra `SyncContext { origin: PeerApp::Maxcode, correlation_id: env.correlation_id }` no `LoopGuard`,
    ///    chama `host.focus_project(FocusRequest { project, worktree_path, context })` e aguarda confirmação
    ///    do consumidor da UI (`confirm_ui_selection`) em até 3 s antes de retornar `AckPayload { handled: true, .. }`.
    pub async fn on_remote_opened(
        &self,
        host: &dyn BridgeHost,
        env: &Envelope,
        payload: &ProjectPathPayload,
    ) -> Result<AckPayload, BridgeError>;

    /// Chamado quando a UI (ou o comando `bridge_notify_active_project`) informa seleção de projeto:
    /// - Se `context.origin == PeerApp::Maxcode`: resolve a confirmação pendente de `on_remote_opened` (se mesmo `project_id` e `correlation_id`)
    ///   e **nunca** agenda envio de `project.focus` para o MaxCode.
    /// - Se `context.origin == PeerApp::Maxterminal` e `is_enabled()`: agenda envio com debounce de 250 ms
    ///   (colapsando seleções rápidas consecutivas na última) e chama `client.notify_focus(project_path, context)`.
    pub fn on_local_selected(&self, project: &ProjectRef, context: SyncContext);
}
```

### 3.3. `src-tauri/crates/mt-bridge/src/prompt.rs`

```rust
pub const INSPECT_TEMPLATE: &str =
    "Ao executar o comando \"{comando}\" tive o seguinte erro: \"{erro}\"";

pub const TRUNCATION_PREFIX: &str = "[…] ";

#[derive(Debug, Clone, Copy)]
pub struct InspectLimits {
    pub command_chars: usize,
    pub error_lines: usize,
    pub error_chars: usize,
}

impl Default for InspectLimits {
    fn default() -> Self {
        Self {
            command_chars: 2000,
            error_lines: 80,
            error_chars: 6000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectPrompt {
    pub text: String,
    pub truncated: bool,
}

/// Remove sequências de escape ANSI/OSC (`strip_ansi_escapes::strip`), normaliza `\r\n` para `\n`,
/// processa `\r` isolados de barras de progresso mantendo apenas o texto após o último `\r` de cada linha,
/// e remove caracteres de controle exceto `\n` e `\t`.
pub fn clean_terminal_text(raw: &str) -> String;

/// Trunca o início da string mantendo o sufixo final com no máximo `max_chars` escalares Unicode (`char`),
/// prefixando com `"[…] "` quando truncado (incluindo os 4 chars do prefixo na contagem total `max_chars`).
pub fn truncate_utf8_keep_end(text: &str, max_chars: usize) -> (String, bool);

/// Trunca o final da string mantendo o prefixo inicial com no máximo `max_chars` escalares Unicode (`char`),
/// adicionando `" […]"` ao final quando truncado (incluindo os 4 chars do sufixo na contagem total `max_chars`).
pub fn truncate_utf8_keep_start(text: &str, max_chars: usize) -> (String, bool);

/// Constrói o prompt canônico do botão "Inspecionar" seguindo rigorosamente os 5 passos:
/// 1) Limpa ANSI/OSC e normaliza `\r\n` / `\r` em `command` e `output` completos.
/// 2) Aplica `sanitize(command_clean)` e `sanitize(output_clean)` ANTES de qualquer recorte (evita cortar delimitador de segredo).
/// 3) Faz `trim()` externo:
///    - Se o comando sanitizado exceder `limits.command_chars` (2000), trunca mantendo o início (`truncate_utf8_keep_start`).
///    - Se a saída sanitizada estiver vazia após `trim()`, usa `format!("código de saída {exit_code}")`.
///    - Caso contrário, retém as últimas `limits.error_lines` (80) linhas (preservando linhas vazias internas)
///      e, se exceder `limits.error_chars` (6000) caracteres, trunca mantendo o final com prefixo `"[…] "` (`truncate_utf8_keep_end`).
/// 4) Interpola no template exato: `Ao executar o comando "{comando}" tive o seguinte erro: "{erro}"`
///    (sem escapar aspas internas nem quebras de linha).
/// 5) Executa verificação final passando os valores recortados por `sanitize` novamente; se houver diferença,
///    re-aplica o recorte sobre os valores re-sanitizados e reconstrói a frase fixa exata.
pub fn build_inspect_prompt(
    command: &str,
    output: &str,
    exit_code: i32,
    limits: &InspectLimits,
    sanitize: &dyn Fn(&str) -> String,
) -> InspectPrompt;
```

---

## 4. Regras de Negócio, Sanitização e Casos de Borda

1. **Fidelidade Estrita ao Prompt do `AGENTS.md` §6 (`CA-29`, `CA-30`, `CA-31`)**:
   - O template é literalmente `Ao executar o comando "${comando}" tive o seguinte erro: "${erro}"`.
   - Para `command = "ls /naoexiste"` e `output = "ls: não foi possível acessar '/naoexiste': Arquivo ou diretório inexistente\n"`, o texto produzido é exatamente:
     `Ao executar o comando "ls /naoexiste" tive o seguinte erro: "ls: não foi possível acessar '/naoexiste': Arquivo ou diretório inexistente"`.
   - Se um token `Bearer eyJ...` ou `APP_KEY=base64:...` estiver exatamente na fronteira da linha 80 ou do caractere 6000, como a etapa 2 sanitiza o texto **completo** antes do corte das 80 linhas / 6000 chars, o segredo já estará redigido antes da divisão, e a etapa 5 garante que nenhum resíduo jamais vaze.
2. **Prevenção de Loop Causal (`CA-24`, `CA-25`, `CA-26`, `CA-27`)**:
   - Quando o MaxCode envia `project.opened { path: "<repo>/.max-code-worktrees/wt-x" }`, `resolve_worktree_root` identifica `main_root = <repo>` e `worktree = Some("<repo>/.max-code-worktrees/wt-x")`, faz o match com o projeto `<repo>` e emite `FocusRequest` com `SyncContext { origin: PeerApp::Maxcode, correlation_id }`.
   - Quando a store/UI confirma a seleção chamando `bridge_notify_active_project`, o `SyncContext` com `origin: PeerApp::Maxcode` chega em `on_local_selected`: a confirmação libera o `AckPayload { handled: true }` para o MaxCode e **nunca** dispara `project.focus` de volta — nem imediatamente, nem após 1500 ms, nem após reconexão.
   - Se o usuário selecionar manualmente o **mesmo** projeto 100 ms depois, a UI gera um novo `SyncContext { origin: PeerApp::Maxterminal, correlation_id: Uuid::now_v7() }` que não é suprimido pelo `LoopGuard` e é enviado após o debounce de 250 ms.
3. **Validação de Worktree e `cwd` (`R-10`, `CA-19`, `CA-24`)**:
   - Criar uma pasta arbitrária chamada `/tmp/.max-code-worktrees/falso` sem um arquivo `.git` válido apontando bidirecionalmente para o `.git/worktrees/<nome>` do repositório registrado **não** associa o caminho ao projeto.
   - `Path::starts_with` compara componentes de caminho (logo `/home/user/repo-2` nunca faz match com `/home/user/repo`).
