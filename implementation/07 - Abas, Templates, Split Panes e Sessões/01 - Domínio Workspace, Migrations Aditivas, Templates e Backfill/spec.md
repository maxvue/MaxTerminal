# Implementação 01 — Domínio Workspace, Migrations Aditivas, Templates e Backfill

## 1. Contexto, Objetivo e Escopo

Esta implementação estabelece a fundação de domínio puro em Rust (`mt-core::workspace`), o esquema relacional aditivo no SQLite (`mt-db`), a migração em código de backfill (`m07_backfill`) e o repositório transacional de Templates (`TemplateRepo` + `DefaultTemplateContributor`) da **Fase 07: Abas, Templates, Split Panes e Sessões**.

### Escopo Exato
1. **Identificadores e Códigos de Erro (`mt-core`):**
   - Identificador tipado `TabSessionId` consumido de `mt_core::ids` (gerado pelo `define_id!` da Fase 01; esta fase não o define).
   - Catálogo completo de códigos de erro da Fase 07 em `MtError` com mensagens em `pt-BR` e detalhes sanitizados.
2. **Domínio Puro (`mt-core::workspace`):**
   - Tipos e validações de `TabTemplate`, `TabTemplateInput`, `TemplateConfig`, `TemplateEnvironment`, `EnvironmentKind`, `EnvValue`, `CommandOverride`, `TabOpenOverrides`, `TabOpenRequest` e `LaunchSnapshot`.
   - Árvore binária canônica de painéis (`PaneLayoutNode`, `SplitDirection`, `LayoutSizePatch`), operações puras imutáveis (`split_leaf`, `remove_leaf`, `apply_size_patches`, `validate_layout`, `leaves`, `depth`, `find_node`, seleção de painel ativo vizinho e transferência de `primary_pane_id`) e modelos de estado (`PaneRuntimeState`, `PaneView`, `TabLifecycle`, `TabOrigin`, `TabView`).
   - Tipos de histórico e paginação (`SessionEndReason`, `SessionSummary`, `SessionDetail`, `SessionPaneSummary`, `ScrollbackChunk`, `CursorPage<T>`, `SessionSearchHit`, `WorkspaceRestoreReport`, `WorkspaceSnapshot`).
3. **Migrations SQLite Exclusivamente Aditivas (`mt-db`):**
   - Criação das tabelas `tab_templates`, `tabs`, `panes`, `tab_sessions`, `tab_session_panes`, `session_scrollback` (com índice FTS5 externo `session_scrollback_fts` via `FtsSpec` da Fase 03) e `workspace_operations`.
   - Extensão aditiva da tabela `sessions` criada na Fase 05 (`ALTER TABLE sessions ADD COLUMN ...`), sem recriar `sessions` nem `command_blocks`.
4. **Backfill Idempotente (`m07_backfill.rs`):**
   - Garantia de exatamente 1 template padrão por projeto existente no banco.
   - Vínculo não destrutivo de sessões legadas da Fase 05 (`sessions`) que possuam `tab_id` e `project_id` comprováveis em registros históricos fechados (`tabs`, `panes`, `tab_sessions`, `tab_session_panes`), preservando intactas sessões não vinculáveis.
5. **Repositório de Templates e Contribuidor Atômico (`repos/templates.rs`):**
   - CRUD transacional de templates com validação de segredos literais (`StreamSanitizer` / `sanitize()`), eleição atômica do template padrão e implementação de `DefaultTemplateContributor` integrado ao hook `pub(crate) ProjectInsertContributor` da Fase 06.

---

## 2. Arquivos a Criar ou Modificar

### Criar
- `src-tauri/crates/mt-core/src/workspace/mod.rs`
- `src-tauri/crates/mt-core/src/workspace/template.rs`
- `src-tauri/crates/mt-core/src/workspace/tab.rs`
- `src-tauri/crates/mt-core/src/workspace/layout.rs`
- `src-tauri/crates/mt-core/src/workspace/session.rs`
- `src-tauri/crates/mt-db/migrations/0004_07_workspace_templates_tabs_sessions.sql`
- `src-tauri/crates/mt-db/src/migrations/code/m07_backfill.rs`
- `src-tauri/crates/mt-db/src/repos/templates.rs`

### Modificar
- `src-tauri/crates/mt-core/src/error.rs` (adicionar construtores e códigos de erro da Fase 07)
- `src-tauri/crates/mt-core/src/lib.rs` (exportar `pub mod workspace;`)
- `src-tauri/crates/mt-db/src/migrations/mod.rs` (registrar migration SQL 0004 e code migration `m07_backfill`)
- `src-tauri/crates/mt-db/src/repos/mod.rs` (exportar `templates::TemplateRepo` e registrar `DefaultTemplateContributor` na composição do repositório de projetos da Fase 06)

---

## 3. Especificação Técnica Detalhada

### 3.1. Identificadores e Erros em `mt-core`

`TabSessionId` **não é definido nesta fase**: o `define_id!` de `src-tauri/crates/mt-core/src/ids.rs` (Fase 01) já o gera, com a mesma API dos demais ids (`new()` Uuid v7, `try_from_uuid`, `as_uuid`, `Display`, `FromStr`, `Serialize`/`Deserialize` transparentes e `specta::Type`). Os módulos desta fase apenas o importam:

```rust
use mt_core::ids::TabSessionId;
```

Em `src-tauri/crates/mt-core/src/error.rs`, adicionar os construtores tipados para o catálogo da Fase 07:

```rust
impl MtError {
    pub fn template_invalid(field: &'static str, reason: impl Into<String>) -> Self {
        Self::new(ErrorCode("template.invalid"), format!("Configuração de template inválida no campo '{}': {}", field, reason.into()))
            .with_details(serde_json::json!({ "field": field }))
    }

    pub fn template_default_required(project_id: impl fmt::Display) -> Self {
        Self::new(ErrorCode("template.default_required"), "Todo projeto deve possuir exatamente um template padrão. Não é permitido excluir ou desmarcar o único padrão.")
            .with_details(serde_json::json!({ "projectId": project_id.to_string() }))
    }

    pub fn template_secret_literal(field: &'static str) -> Self {
        Self::new(ErrorCode("template.secret_literal"), "Credenciais, chaves privadas ou tokens literais não podem ser salvos no template. Utilize variáveis de ambiente herdadas.")
            .with_details(serde_json::json!({ "field": field }))
    }

    pub fn template_cwd_not_found(path: impl Into<String>) -> Self {
        Self::new(ErrorCode("template.cwd_not_found"), "O diretório de trabalho configurado para o template não foi encontrado ou não é um diretório válido.")
            .with_details(serde_json::json!({ "path": path.into() }))
    }

    pub fn template_conflict(template_id: impl fmt::Display, expected: i64, actual: i64) -> Self {
        Self::new(ErrorCode("template.conflict"), "O template foi modificado por outra operação. Recarregue os dados antes de salvar.")
            .with_details(serde_json::json!({
                "templateId": template_id.to_string(),
                "expectedRevision": expected,
                "actualRevision": actual
            }))
    }

    pub fn template_not_found(template_id: impl fmt::Display) -> Self {
        Self::new(ErrorCode("template.not_found"), "Template de aba não encontrado.")
            .with_details(serde_json::json!({ "templateId": template_id.to_string() }))
    }

    pub fn workspace_environment_unavailable(agent_id: impl Into<String>) -> Self {
        Self::new(ErrorCode("workspace.environment_unavailable"), "O resolvedor do agente configurado não está disponível neste ambiente.")
            .with_details(serde_json::json!({ "agentId": agent_id.into() }))
    }

    pub fn workspace_request_conflict(request_id: impl Into<String>) -> Self {
        Self::new(ErrorCode("workspace.request_conflict"), "Conflito de idempotência: o mesmo requestId foi reutilizado com parâmetros diferentes.")
            .with_details(serde_json::json!({ "requestId": request_id.into() }))
    }

    pub fn workspace_operation_interrupted(request_id: impl Into<String>) -> Self {
        Self::new(ErrorCode("workspace.operation_interrupted"), "A operação anterior foi interrompida por encerramento abrupto e não pode ser retomada automaticamente.")
            .with_details(serde_json::json!({ "requestId": request_id.into() }))
    }

    pub fn workspace_stop_failed(tab_id: impl fmt::Display, reason: impl Into<String>) -> Self {
        Self::new(ErrorCode("workspace.stop_failed"), "Não foi possível encerrar todos os processos supervisionados da aba dentro do tempo limite.")
            .with_details(serde_json::json!({ "tabId": tab_id.to_string(), "reason": reason.into() }))
    }

    pub fn workspace_persistence_pending(tab_id: impl fmt::Display) -> Self {
        Self::new(ErrorCode("workspace.persistence_pending"), "A drenagem de persistência da sessão ainda está pendente ou falhou.")
            .with_details(serde_json::json!({ "tabId": tab_id.to_string() }))
    }

    pub fn tab_not_found(tab_id: impl fmt::Display) -> Self {
        Self::new(ErrorCode("tab.not_found"), "Aba não encontrada.")
            .with_details(serde_json::json!({ "tabId": tab_id.to_string() }))
    }

    pub fn tab_closing(tab_id: impl fmt::Display) -> Self {
        Self::new(ErrorCode("tab.closing"), "A aba está em processo de encerramento e não aceita novas operações.")
            .with_details(serde_json::json!({ "tabId": tab_id.to_string() }))
    }

    pub fn tab_reorder_mismatch(project_id: impl fmt::Display) -> Self {
        Self::new(ErrorCode("tab.reorder_mismatch"), "A lista de abas informada na reordenação não corresponde exatamente às abas abertas do projeto.")
            .with_details(serde_json::json!({ "projectId": project_id.to_string() }))
    }

    pub fn pane_not_found(pane_id: impl fmt::Display) -> Self {
        Self::new(ErrorCode("pane.not_found"), "Painel não encontrado.")
            .with_details(serde_json::json!({ "paneId": pane_id.to_string() }))
    }

    pub fn pane_limit_reached(max_panes: usize) -> Self {
        Self::new(ErrorCode("pane.limit_reached"), format!("Limite máximo de {} painéis por aba atingido.", max_panes))
            .with_details(serde_json::json!({ "maxPanes": max_panes }))
    }

    pub fn pane_not_exited(pane_id: impl fmt::Display) -> Self {
        Self::new(ErrorCode("pane.not_exited"), "O painel ainda possui processo ativo e não pode ser reiniciado antes do término.")
            .with_details(serde_json::json!({ "paneId": pane_id.to_string() }))
    }

    pub fn layout_invalid(reason: impl Into<String>) -> Self {
        Self::new(ErrorCode("layout.invalid"), format!("Árvore de layout de painéis inválida: {}", reason.into()))
            .with_details(serde_json::json!({}))
    }

    pub fn layout_conflict(tab_id: impl fmt::Display, expected: i64, actual: i64) -> Self {
        Self::new(ErrorCode("layout.conflict"), "A estrutura de painéis mudou. Recarregue o layout atual da aba.")
            .with_details(serde_json::json!({
                "tabId": tab_id.to_string(),
                "expectedLayoutRevision": expected,
                "actualLayoutRevision": actual
            }))
    }

    pub fn session_not_found(id: impl fmt::Display) -> Self {
        Self::new(ErrorCode("session.not_found"), "Sessão não encontrada no histórico.")
            .with_details(serde_json::json!({ "sessionId": id.to_string() }))
    }

    pub fn session_query_invalid(reason: impl Into<String>) -> Self {
        Self::new(ErrorCode("session.query_invalid"), format!("Parâmetros de consulta de histórico inválidos: {}", reason.into()))
            .with_details(serde_json::json!({}))
    }
}
```

---

### 3.2. Tipos e Validação de Templates (`mt-core/src/workspace/template.rs`)

```rust
use crate::error::MtError;
use crate::ids::{ProjectId, TemplateId};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const MAX_TEMPLATE_NAME_CHARS: usize = 64;
pub const MAX_ICON_CHARS: usize = 64;
pub const MAX_INITIAL_COMMAND_BYTES: usize = 4096;
pub const MAX_ENV_VARS_COUNT: usize = 64;
pub const MAX_ENV_VALUE_BYTES: usize = 8 * 1024;
pub const MAX_ENV_TOTAL_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentKind {
    Shell,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TemplateEnvironment {
    pub kind: EnvironmentKind,
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EnvValue {
    Literal { value: String },
    Inherited { source: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TabTemplateInput {
    pub name: String,
    pub icon: String,
    pub cwd: Option<String>,
    pub initial_command: Option<String>,
    pub add_to_workspace: bool,
    pub continuous: bool,
    pub environment: TemplateEnvironment,
    pub env_vars: BTreeMap<String, EnvValue>,
    pub make_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TemplateConfig {
    pub name: String,
    pub icon: String,
    pub cwd: Option<String>,
    pub initial_command: Option<String>,
    pub add_to_workspace: bool,
    pub continuous: bool,
    pub environment: TemplateEnvironment,
    pub env_vars: BTreeMap<String, EnvValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TabTemplate {
    pub id: TemplateId,
    pub project_id: ProjectId,
    pub config: TemplateConfig,
    pub is_default: bool,
    pub position: i64,
    pub revision: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandOverride {
    Inherit,
    Clear,
    Replace { command: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TabOpenOverrides {
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub initial_command: CommandOverride,
    pub extra_env: BTreeMap<String, EnvValue>,
    pub continuous: Option<bool>,
    pub cwd_fallback_to_project: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TabOpenRequest {
    pub request_id: String,
    pub project_id: ProjectId,
    pub template_id: Option<TemplateId>,
    pub overrides: Option<TabOpenOverrides>,
    pub focus: bool,
}

/// Snapshot imutável persistido em `tabs.launch_json` na abertura da aba.
/// Nunca armazena valores secretos resolvidos de `Inherited`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LaunchSnapshot {
    pub template_id: Option<TemplateId>,
    pub title: String,
    pub icon: String,
    pub resolved_cwd: String,
    pub initial_command: Option<String>,
    pub continuous: bool,
    pub environment: TemplateEnvironment,
    pub env_vars: BTreeMap<String, EnvValue>,
}
```

#### Funções Puras de Validação em `template.rs`
- `pub fn validate_template_input<F>(input: &TabTemplateInput, is_secret_or_sanitized: F) -> Result<TemplateConfig, MtError> where F: Fn(&str, &str) -> bool`:
  1. `name`: aplica `.trim()`. Deve ter `1..=64` caracteres (`chars().count()`) e nenhum caractere de controle (`c.is_control()`).
  2. `icon`: aplica `.trim()`. Deve ter `1..=64` caracteres, formato Iconify `^[a-z0-9]+(-[a-z0-9]+)*:[a-z0-9]+(-[a-z0-9]+)*$`.
  3. `initial_command`: quando `Some(cmd)`, se `cmd.trim().is_empty()` normaliza para `None`; caso contrário, exige `cmd.len() <= 4096`, sem byte NUL (`\0`). Se `is_secret_or_sanitized("initial_command", cmd)` retornar `true`, retorna ` Err(MtError::template_secret_literal("initialCommand"))`.
  4. `cwd`: quando `Some(raw)`, trim; se vazio vira `None`; não pode conter `\0`.
  5. `environment`:
     - `EnvironmentKind::Shell` exige `agent_id.is_none()`.
     - `EnvironmentKind::Agent` exige `agent_id = Some(slug)` onde `slug` casa com `^[a-z0-9][a-z0-9._-]{0,63}$`.
  6. `env_vars`:
     - No máximo 64 entradas (`MAX_ENV_VARS_COUNT`).
     - Chave deve casar com `^[A-Za-z_][A-Za-z0-9_]*$`.
     - Chave não pode começar com `MAXTERMINAL_` nem `MT_` (reservados internamente).
     - Se `EnvValue::Literal { value }`: sem `\0`, `value.len() <= 8192`. Se o nome da chave for sensível (ex.: `APP_KEY`, `AWS_SECRET_ACCESS_KEY`, `GITHUB_TOKEN`, ou terminar com `_SECRET`, `_TOKEN`, `_PASSWORD`, `_PRIVATE_KEY`, `_API_KEY`) ou `is_secret_or_sanitized(&key, value)` for `true`, retorna `Err(MtError::template_secret_literal("envVars"))`.
     - Se `EnvValue::Inherited { source }`: `source` deve casar com `^[A-Za-z_][A-Za-z0-9_]*$` e não pode começar com `MAXTERMINAL_` nem `MT_`.
     - Soma total de bytes de chaves + valores/sources ≤ 64 KiB (`MAX_ENV_TOTAL_BYTES`).
- `pub fn resolve_and_validate_cwd(project_root: &Path, template_cwd: Option<&str>, override_cwd: Option<&str>, fallback_to_project: bool, home_dir: Option<&Path>) -> Result<PathBuf, MtError>`:
  - Ordem de precedência: `override_cwd` → `template_cwd` → `project_root`.
  - Expansão de `~` ou `~/` usando `home_dir` (sem invocar shell).
  - Caminhos relativos são resolvidos a partir de `project_root` (`project_root.join(rel)`).
  - Executa `std::fs::canonicalize(&candidate)`: se falhar ou `!canonical.is_dir()` ou `canonical.to_str().is_none()`:
    - Se `fallback_to_project` for `true`, tenta `std::fs::canonicalize(project_root)` e verifica `is_dir()` + UTF-8;
    - Caso contrário, retorna `Err(MtError::template_cwd_not_found(candidate.display().to_string()))`.

---

### 3.3. Árvore Binária Canônica e Identidade de Abas/Painéis (`mt-core/src/workspace/layout.rs` e `tab.rs`)

Em `src-tauri/crates/mt-core/src/workspace/layout.rs`:

```rust
use crate::error::MtError;
use crate::ids::PaneId;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeSet;

pub const MAX_PANES_PER_TAB: usize = 8;
pub const MAX_LAYOUT_DEPTH: usize = 7;
pub const MIN_PANE_SIZE_PERCENT: f64 = 5.0;
pub const SIZE_SUM_TOLERANCE: f64 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SplitDirection {
    /// Divisor horizontal: painéis empilhados acima e abaixo.
    Horizontal,
    /// Divisor vertical: painéis dispostos lado a lado.
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneLayoutNode {
    Leaf {
        #[serde(rename = "paneId")]
        pane_id: PaneId,
    },
    Split {
        #[serde(rename = "nodeId")]
        node_id: String,
        direction: SplitDirection,
        children: Vec<PaneLayoutNode>,
        sizes: Vec<f64>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LayoutSizePatch {
    pub node_id: String,
    pub sizes: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveLeafResult {
    pub new_root: Option<PaneLayoutNode>,
    pub next_active_pane_id: Option<PaneId>,
    pub next_primary_pane_id: Option<PaneId>,
}
```

#### Operações Puras Obrigatórias em `layout.rs`
1. `pub fn leaves(&self) -> Vec<PaneId>`: retorna as folhas em ordem em-ordem (esquerda para direita / topo para base).
2. `pub fn depth(&self) -> usize`: folha tem profundidade `0`; `Split` tem `1 + max(children.depth())`.
3. `pub fn validate_against_open_panes(&self, open_panes: &BTreeSet<PaneId>) -> Result<(), MtError>`:
   - Profundidade ≤ `MAX_LAYOUT_DEPTH` (7).
   - Quantidade de folhas entre `1` e `MAX_PANES_PER_TAB` (8).
   - Todas as folhas são únicas (sem `PaneId` duplicado na árvore) e o conjunto de folhas é exatamente igual a `open_panes`.
   - Todos os `node_id` de nós `Split` são strings não vazias e únicas na árvore.
   - Cada nó `Split` é estritamente binário (`children.len() == 2` e `sizes.len() == 2`), com `sizes.iter().all(|s| s.is_finite() && *s >= MIN_PANE_SIZE_PERCENT)` e `|(sizes[0] + sizes[1]) - 100.0| <= SIZE_SUM_TOLERANCE`.
4. `pub fn split_leaf(&self, target_pane_id: PaneId, new_pane_id: PaneId, new_node_id: String, direction: SplitDirection) -> Result<PaneLayoutNode, MtError>`:
   - Verifica se `self.leaves().len() < MAX_PANES_PER_TAB` (caso contrário retorna `MtError::pane_limit_reached(MAX_PANES_PER_TAB)`).
   - Verifica se `new_pane_id` não existe na árvore e `new_node_id` não colide.
   - Substitui a folha `target_pane_id` por `PaneLayoutNode::Split { node_id: new_node_id, direction, children: vec![Leaf { pane_id: target_pane_id }, Leaf { pane_id: new_pane_id }], sizes: vec![50.0, 50.0] }`.
   - Não funde nós de mesma orientação (preserva `node_id` e proporções existentes).
   - Valida profundidade ≤ 7 após a divisão.
5. `pub fn remove_leaf(&self, target_pane_id: PaneId, current_active: PaneId, current_primary: PaneId) -> Result<RemoveLeafResult, MtError>`:
   - Localiza `target_pane_id`. Se for a única folha da raiz, retorna `RemoveLeafResult { new_root: None, next_active_pane_id: None, next_primary_pane_id: None }`.
   - Se estiver dentro de um `Split`, remove a folha e colapsa o pai diretamente em seu irmão sobrevivente (preservando a subárvore do irmão intacta).
   - Se `current_active == target_pane_id`, seleciona a folha vizinha imediata (a primeira folha da subárvore irmã ou, na ausência, a folha anterior na ordem de `leaves()`).
   - Se `current_primary == target_pane_id`, transfere `next_primary_pane_id` para a primeira folha sobrevivente (`new_root.leaves()[0]`).
6. `pub fn apply_size_patches(&self, patches: &[LayoutSizePatch]) -> Result<PaneLayoutNode, MtError>`:
   - Para cada patch em `patches`, exige que `node_id` exista na árvore (caso contrário retorna `MtError::layout_invalid("node_id inexistente no layout atual")`).
   - Normaliza/valida `sizes`: 2 elementos finitos, cada um `>= 5.0`, soma `100.0 ± 0.01` (ajustando `sizes[1] = 100.0 - sizes[0]` quando dentro da tolerância de arredondamento de ponto flutuante do frontend).

Em `src-tauri/crates/mt-core/src/workspace/tab.rs`:

```rust
use crate::error::MtError;
use crate::ids::{PaneId, ProjectId, PtyId, TabId, TabSessionId, TemplateId};
use crate::pty::ExitInfo;
use crate::workspace::layout::PaneLayoutNode;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PaneRuntimeState {
    Starting,
    Live,
    Exited,
    Failed,
    Stopping,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PaneView {
    pub id: PaneId,
    pub pty_id: Option<PtyId>,
    pub generation: i64,
    pub cwd: String,
    pub state: PaneRuntimeState,
    pub exit: Option<ExitInfo>,
    pub error: Option<MtError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TabLifecycle {
    Opening,
    Ready,
    Restarting,
    Closing,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TabOrigin {
    Manual,
    Workspace,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TabView {
    pub id: TabId,
    pub project_id: ProjectId,
    pub template_id: Option<TemplateId>,
    pub title: String,
    pub icon: String,
    pub position: i64,
    pub origin: TabOrigin,
    pub continuous: bool,
    pub lifecycle: TabLifecycle,
    pub revision: i64,
    pub layout_revision: i64,
    pub layout: PaneLayoutNode,
    pub panes: Vec<PaneView>,
    pub active_pane_id: PaneId,
    pub tab_session_id: TabSessionId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneCloseOutcome {
    PaneClosed {
        tab: TabView,
    },
    TabClosed {
        #[serde(rename = "tabId")]
        tab_id: TabId,
    },
}
```

---

### 3.4. Tipos de Sessão, Histórico e Snapshot (`mt-core/src/workspace/session.rs`)

```rust
use crate::error::MtError;
use crate::ids::{PaneId, ProjectId, SessionId, TabId, TabSessionId, TemplateId};
use crate::workspace::tab::TabView;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SessionEndReason {
    NewSession,
    TabClosed,
    AppExit,
    CrashRecovered,
    ProjectRemoved,
    SpawnFailed,
}

impl SessionEndReason {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::NewSession => "new_session",
            Self::TabClosed => "tab_closed",
            Self::AppExit => "app_exit",
            Self::CrashRecovered => "crash_recovered",
            Self::ProjectRemoved => "project_removed",
            Self::SpawnFailed => "spawn_failed",
        }
    }

    pub fn from_db_str(raw: &str) -> Result<Self, MtError> {
        match raw {
            "new_session" => Ok(Self::NewSession),
            "tab_closed" => Ok(Self::TabClosed),
            "app_exit" => Ok(Self::AppExit),
            "crash_recovered" => Ok(Self::CrashRecovered),
            "project_removed" => Ok(Self::ProjectRemoved),
            "spawn_failed" => Ok(Self::SpawnFailed),
            other => Err(MtError::session_query_invalid(format!(
                "end_reason desconhecido no banco: {}",
                other
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum InitialDispatchState {
    None,
    Pending,
    Waiting,
    Claimed,
    Sent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: TabSessionId,
    pub tab_id: TabId,
    pub seq: i64,
    pub archived: bool,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub end_reason: Option<SessionEndReason>,
    pub command_count: i64,
    pub failed_command_count: i64,
    pub scrollback_bytes: i64,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionPaneSummary {
    pub session_id: SessionId,
    pub pane_id: PaneId,
    pub cwd: String,
    pub closed_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    pub summary: SessionSummary,
    pub panes: Vec<SessionPaneSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScrollbackChunk {
    pub session_id: SessionId,
    pub seq: i64,
    pub ts: i64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CursorPage<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionSearchHit {
    pub tab_session_id: TabSessionId,
    pub session_id: SessionId,
    pub pane_id: PaneId,
    pub seq: i64,
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRestoreFailure {
    pub project_id: ProjectId,
    pub template_id: Option<TemplateId>,
    pub tab_id: Option<TabId>,
    pub error: MtError,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRestoreReport {
    pub boot_id: String,
    pub opened_tabs_count: usize,
    pub crash_recovered_sessions_count: usize,
    pub interrupted_operations_count: usize,
    pub failures: Vec<WorkspaceRestoreFailure>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub boot_id: String,
    pub event_seq: i64,
    pub restored: bool,
    pub tabs: Vec<TabView>,
    pub restore_report: Option<WorkspaceRestoreReport>,
}
```

---

### 3.5. Migration SQLite (`0004_07_workspace_templates_tabs_sessions.sql`)

O arquivo `src-tauri/crates/mt-db/migrations/0004_07_workspace_templates_tabs_sessions.sql` deve conter exatamente o DDL abaixo (incluindo o índice FTS5 externo gerado segundo a `FtsSpec` da Fase 03):

```sql
-- Fase 07: Templates, Abas, Painéis, Agrupamentos de Sessões, Scrollback FTS5 e Operações

CREATE TABLE tab_templates (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 64),
    icon TEXT NOT NULL,
    cwd TEXT,
    initial_command TEXT CHECK (initial_command IS NULL OR length(CAST(initial_command AS BLOB)) <= 4096),
    add_to_workspace INTEGER NOT NULL DEFAULT 0 CHECK (add_to_workspace IN (0,1)),
    continuous INTEGER NOT NULL DEFAULT 0 CHECK (continuous IN (0,1)),
    environment_kind TEXT NOT NULL CHECK (environment_kind IN ('shell','agent')),
    agent_id TEXT,
    env_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(env_json) AND json_type(env_json) = 'object'),
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0,1)),
    position INTEGER NOT NULL DEFAULT 0,
    revision INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK ((environment_kind = 'agent') = (agent_id IS NOT NULL))
) STRICT;

CREATE UNIQUE INDEX ux_tab_templates_default ON tab_templates(project_id) WHERE is_default = 1;
CREATE INDEX ix_tab_templates_project ON tab_templates(project_id, position, id);
CREATE INDEX ix_tab_templates_workspace ON tab_templates(project_id, id) WHERE add_to_workspace = 1;

CREATE TABLE tabs (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    template_id TEXT REFERENCES tab_templates(id) ON DELETE SET NULL,
    origin TEXT NOT NULL CHECK (origin IN ('manual','workspace')),
    title TEXT NOT NULL,
    icon TEXT NOT NULL,
    position INTEGER NOT NULL,
    continuous INTEGER NOT NULL CHECK (continuous IN (0,1)),
    launch_json TEXT NOT NULL CHECK (json_valid(launch_json)),
    layout_json TEXT NOT NULL CHECK (json_valid(layout_json)),
    active_pane_id TEXT NOT NULL,
    primary_pane_id TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1,
    layout_revision INTEGER NOT NULL DEFAULT 1,
    last_session_seq INTEGER NOT NULL DEFAULT 0,
    opened_at INTEGER NOT NULL,
    closed_at INTEGER,
    last_focused_at INTEGER
) STRICT;

CREATE INDEX ix_tabs_project_open ON tabs(project_id, position, id) WHERE closed_at IS NULL;
CREATE UNIQUE INDEX ux_tabs_workspace_template ON tabs(template_id)
    WHERE origin = 'workspace' AND template_id IS NOT NULL;

CREATE TABLE panes (
    id TEXT PRIMARY KEY NOT NULL,
    tab_id TEXT NOT NULL REFERENCES tabs(id) ON DELETE CASCADE,
    cwd TEXT NOT NULL,
    generation INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    closed_at INTEGER
) STRICT;

CREATE INDEX ix_panes_tab ON panes(tab_id, id) WHERE closed_at IS NULL;

-- Extensão aditiva da tabela sessions criada na Fase 05
ALTER TABLE sessions ADD COLUMN archived_at INTEGER;
ALTER TABLE sessions ADD COLUMN next_scrollback_seq INTEGER NOT NULL DEFAULT 1 CHECK (next_scrollback_seq > 0);
ALTER TABLE sessions ADD COLUMN scrollback_bytes INTEGER NOT NULL DEFAULT 0 CHECK (scrollback_bytes >= 0);
ALTER TABLE sessions ADD COLUMN scrollback_truncated INTEGER NOT NULL DEFAULT 0 CHECK (scrollback_truncated IN (0,1));
CREATE INDEX ix_sessions_archived ON sessions(archived_at, id) WHERE archived_at IS NOT NULL;

CREATE TABLE tab_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    tab_id TEXT NOT NULL REFERENCES tabs(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL CHECK (seq > 0),
    started_at INTEGER NOT NULL,
    archived_at INTEGER,
    end_reason TEXT CHECK (end_reason IN
        ('new_session','tab_closed','app_exit','crash_recovered','project_removed','spawn_failed')),
    CHECK ((archived_at IS NULL) = (end_reason IS NULL)),
    UNIQUE (tab_id, seq)
) STRICT;

CREATE UNIQUE INDEX ux_tab_sessions_active ON tab_sessions(tab_id) WHERE archived_at IS NULL;
CREATE INDEX ix_tab_sessions_history ON tab_sessions(tab_id, seq DESC);

CREATE TABLE tab_session_panes (
    session_id TEXT PRIMARY KEY NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    tab_session_id TEXT NOT NULL REFERENCES tab_sessions(id) ON DELETE CASCADE,
    pane_id TEXT NOT NULL REFERENCES panes(id) ON DELETE RESTRICT,
    generation INTEGER NOT NULL,
    joined_at INTEGER NOT NULL,
    closed_at INTEGER,
    initial_dispatch TEXT NOT NULL DEFAULT 'none'
        CHECK (initial_dispatch IN ('none','pending','waiting','claimed','sent')),
    UNIQUE (tab_session_id, pane_id, generation)
) STRICT;

CREATE UNIQUE INDEX ux_tab_session_pane_active ON tab_session_panes(tab_session_id, pane_id)
    WHERE closed_at IS NULL;
CREATE INDEX ix_tab_session_panes_group ON tab_session_panes(tab_session_id, pane_id, generation);

CREATE TABLE session_scrollback (
    rowid INTEGER PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL CHECK (seq > 0),
    ts INTEGER NOT NULL,
    text TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('output','gap')),
    byte_len INTEGER NOT NULL CHECK (byte_len >= 0 AND byte_len <= 65536),
    UNIQUE (session_id, seq)
) STRICT;

CREATE VIRTUAL TABLE session_scrollback_fts USING fts5(
    text,
    content='session_scrollback',
    content_rowid='rowid',
    tokenize='unicode61 remove_diacritics 2',
    prefix='2 3'
);

CREATE TRIGGER trg_session_scrollback_ai AFTER INSERT ON session_scrollback BEGIN
    INSERT INTO session_scrollback_fts(rowid, text) VALUES (new.rowid, new.text);
END;

CREATE TRIGGER trg_session_scrollback_ad AFTER DELETE ON session_scrollback BEGIN
    INSERT INTO session_scrollback_fts(session_scrollback_fts, rowid, text)
    VALUES ('delete', old.rowid, old.text);
END;

CREATE TRIGGER trg_session_scrollback_au AFTER UPDATE ON session_scrollback BEGIN
    INSERT INTO session_scrollback_fts(session_scrollback_fts, rowid, text)
    VALUES ('delete', old.rowid, old.text);
    INSERT INTO session_scrollback_fts(rowid, text) VALUES (new.rowid, new.text);
END;

CREATE TABLE workspace_operations (
    request_id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL,
    args_hash TEXT NOT NULL,
    tab_id TEXT REFERENCES tabs(id) ON DELETE CASCADE,
    boot_id TEXT NOT NULL,
    stage TEXT NOT NULL CHECK (stage IN ('prepared','stopping','committed','completed','blocked','interrupted')),
    result_json TEXT CHECK (result_json IS NULL OR json_valid(result_json)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE INDEX ix_workspace_operations_pending ON workspace_operations(stage, updated_at);
```

---

### 3.6. Repositório de Templates (`mt-db/src/repos/templates.rs`) e Backfill (`m07_backfill.rs`)

#### Assinaturas de `TemplateRepo` e `DefaultTemplateContributor`
```rust
use crate::{Db, DbError};
use crate::repos::projects::StoredProject;
use mt_core::error::MtError;
use mt_core::ids::{ProjectId, TemplateId};
use mt_core::workspace::template::{TabTemplate, TabTemplateInput};
use rusqlite::Transaction;

#[derive(Clone)]
pub struct TemplateRepo {
    db: Db,
}

impl TemplateRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Primitivo transacional idempotente que garante que `project_id` possui exatamente 1 template padrão.
    /// 1. Se já existir linha com `is_default = 1`, retorna seu `TemplateId`.
    /// 2. Se não houver padrão mas existirem templates no projeto, elege o menor `(position, created_at, id)`.
    /// 3. Se não houver nenhum template no projeto, insere "Terminal" (`mdi:console`, shell, cwd=NULL,
    ///    initial_command=NULL, add_to_workspace=0, continuous=0, env_json='{}', is_default=1, position=0).
    /// Primitivo interno a `mt-db`: retorna `DbError` (convertido para `MtError` apenas nos métodos públicos `async`).
    pub fn ensure_default_tx(tx: &Transaction<'_>, project_id: ProjectId, now_ms: i64) -> Result<TemplateId, DbError>;

    pub async fn list(&self, project_id: ProjectId) -> Result<Vec<TabTemplate>, MtError>;
    pub async fn get(&self, template_id: TemplateId) -> Result<TabTemplate, MtError>;
    pub async fn create(&self, project_id: ProjectId, input: TabTemplateInput) -> Result<TabTemplate, MtError>;
    pub async fn update(
        &self,
        template_id: TemplateId,
        expected_revision: i64,
        input: TabTemplateInput,
    ) -> Result<TabTemplate, MtError>;
    pub async fn delete(&self, template_id: TemplateId, expected_revision: i64) -> Result<(), MtError>;
    pub async fn set_default(&self, template_id: TemplateId) -> Result<Vec<TabTemplate>, MtError>;
}

/// Contribuidor registrado internamente em `mt-db` no `SqliteProjectStore` da Fase 06.
pub(crate) struct DefaultTemplateContributor;

impl crate::repos::projects::ProjectInsertContributor for DefaultTemplateContributor {
    /// Assinatura idêntica a `ProjectInsertContributor::after_insert` da Fase 06
    /// (`06/02/spec.md` §3.2): recebe `&StoredProject` e lê `project.id` e `project.created_at` do próprio registro.
    fn after_insert(&self, tx: &Transaction<'_>, project: &StoredProject) -> Result<(), DbError> {
        let project_id: ProjectId = project.id.parse().map_err(|_| DbError::Constraint("project.id inválido".into()))?;
        TemplateRepo::ensure_default_tx(tx, project_id, project.created_at).map(|_| ())
    }
}
```

#### Regras de Negócio de `TemplateRepo` e `m07_backfill`
- **Validação de Segredos:** `TemplateRepo::create` e `TemplateRepo::update` invocam `validate_template_input(&input, |_, text| crate::sanitizer::sanitize(text).was_redacted)`. Se o sanitizador da Fase 03 alterar qualquer caractere do comando inicial ou valor de variável de ambiente literal, a mutação falha imediatamente com `template.secret_literal`.
- **Troca Atômica do Padrão (`set_default`, `create` com `make_default=true`, `update` com `make_default=true`):**
  - Executa dois comandos `UPDATE` separados dentro da mesma transação:
    1. `UPDATE tab_templates SET is_default = 0, updated_at = ?1, revision = revision + 1 WHERE project_id = ?2 AND is_default = 1 AND id != ?3;`
    2. `UPDATE tab_templates SET is_default = 1, updated_at = ?1, revision = revision + 1 WHERE id = ?3 AND project_id = ?2;`
  - Nunca usar `SET is_default = (id = ?)` em um único `UPDATE` para não violar o índice único parcial `ux_tab_templates_default` durante a avaliação linha a linha do SQLite.
  - Ao final de toda mutação (`create`, `update`, `delete`, `set_default`), verifica `SELECT COUNT(*) FROM tab_templates WHERE project_id = ?1 AND is_default = 1` == `1`; caso contrário aborta a transação retornando `MtError::template_default_required(project_id)`.
- **Recusa de Exclusão do Padrão:** `delete` verifica `is_default == 1` na linha alvo e retorna `Err(MtError::template_default_required(project_id))` antes de executar `DELETE`.
- **Backfill (`m07_backfill.rs`):**
  1. Verifica que a tabela `sessions` da Fase 05 existe e contém as colunas esperadas (`id, tab_id, pane_id, project_id, pty_id, shell, cwd, status, started_at, ended_at, exit_code, exit_signal, next_seq`). Se ausente ou incompatível, retorna erro fatal de migration.
  2. Para cada `id` em `SELECT id FROM projects ORDER BY created_at ASC, id ASC`, chama `TemplateRepo::ensure_default_tx(tx, project_id, now_ms)`.
  3. Seleciona sessões de `sessions` que ainda não possuem vínculo em `tab_session_panes`, ordenadas por `(started_at ASC, id ASC)`.
  4. Para cada sessão legado cujo `project_id` exista em `projects` e possua `tab_id` e `pane_id` válidos (UUID):
     - Garante registro fechado em `tabs` (`origin = 'manual'`, `closed_at = COALESCE(ended_at, started_at)`) e em `panes` (`closed_at = COALESCE(ended_at, started_at)`).
     - Incrementa `tabs.last_session_seq` e cria um `tab_sessions` individual arquivado (`archived_at = COALESCE(ended_at, started_at)`, `end_reason = 'tab_closed'`).
     - Cria o vínculo em `tab_session_panes` (`generation = 0`, `closed_at = COALESCE(ended_at, started_at)`).
     - Atualiza `sessions.archived_at = COALESCE(ended_at, started_at)` e `status = 'archived'` se a sessão já estava fechada.
  5. Sessões legadas cujo `project_id` não exista em `projects` permanecem intactas na tabela `sessions` (nenhuma linha é deletada). Ao final, executa `PRAGMA foreign_key_check` e falha a transação caso retorne qualquer linha.

---

## 4. Casos de Borda e Tratamento de Erros

1. **Conflito de Revisão Otimista (`template.conflict`):** `update` e `delete` comparam `expected_revision` com `revision` atual no banco. Se divergirem, retornam `MtError::template_conflict(template_id, expected_revision, actual_revision)`.
2. **Importação em Lote de Projetos (Fase 06):** Se a inserção do template padrão falhar para qualquer projeto do lote dentro de `DefaultTemplateContributor::after_insert`, toda a transação SQLite da Fase 06 sofre rollback automático, impedindo projetos órfãos sem template padrão.
3. **Desmarcar `make_default` no `update`:** Se o template editado já é o padrão (`is_default == 1`) e o usuário envia `make_default: false`, a requisição é recusada com `MtError::template_default_required(project_id)` (para trocar o padrão, o usuário deve promover outro template como padrão).
4. **Exclusão de Template Não-Padrão:** `tabs.template_id` possui `ON DELETE SET NULL`, preservando todas as abas abertas e todo o histórico de sessões quando um template secundário é removido.
