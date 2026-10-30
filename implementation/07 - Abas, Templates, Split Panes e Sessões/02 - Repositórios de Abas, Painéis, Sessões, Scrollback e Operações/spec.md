# Implementação 02 — Repositórios de Abas, Painéis, Sessões, Scrollback e Operações

## 1. Contexto, Objetivo e Escopo

Esta implementação constrói toda a camada de persistência transacional do workspace em `mt-db` sobre o schema criado na Implementação 01, compondo o repositório base `SessionsRepo` da Fase 05 sem substituí-lo, além de registrar as configurações e políticas de retenção na infraestrutura da Fase 03.

### Escopo Exato
1. **Repositórios de Abas e Painéis (`repos/tabs.rs` e `repos/panes.rs`):**
   - Criação transacional de aba + painel inicial + snapshot de lançamento (`LaunchSnapshot`) + validação de posse do projeto e invariantes da árvore de layout.
   - Reordenação atômica de abas abertas (`reorder`), atualização de foco (`touch_focus`), atualização otimista da árvore de layout (`update_layout` com `expected_layout_revision`), adição de folha em split (`insert_split_pane_tx`), remoção de folha intermediária (`close_pane_tx`) e fechamento completo de aba (`close_tab_tx`).
   - Reutilização da identidade única `origin = 'workspace'` por `template_id` para abas automáticas do workspace.
2. **Repositório de Agrupamentos e Vínculos de Sessão (`repos/tab_sessions.rs`):**
   - Gerenciamento de `tab_sessions` (`start_tx`, `archive_tx`, `get_active`, `get_detail`, `list_by_tab` paginado por cursor opaco `(tab_id, seq)`).
   - Gerenciamento de `tab_session_panes` (`attach_pane_session_tx`, `close_pane_session_tx`, máquina de estados de `initial_dispatch`: `none -> pending -> waiting -> claimed -> sent`).
   - Extensão de arquivamento sobre `SessionsRepo` da Fase 05 (`archive_closed_base_session_tx`), exigindo sessão `closed`, `ended_at` preenchido e zero blocos `running` em `command_blocks`.
3. **Repositório de Scrollback e Busca FTS5 (`repos/scrollback.rs`):**
   - Gravação em lote de chunks sanitizados (`insert_chunks_batch`), alocando `seq` a partir de `sessions.next_scrollback_seq` e atualizando `scrollback_bytes` e `scrollback_truncated` na mesma transação do `BatchWriter`.
   - Leitura paginada (`list_chunks`) por cursor `(tab_session_id, session_id, seq)` com limite de até 200 itens e teto estrito de 1 MiB (1.048.576 bytes) por resposta.
   - Busca textual sanitizada (`search_in_tab`) em `session_scrollback_fts` usando `build_match_query` da Fase 03, restringindo `session_id` via `JOIN` com `tab_session_panes` e `tab_sessions` antes do `LIMIT`.
4. **Ledger de Idempotência e Saga (`repos/workspace_operations.rs`):**
   - Registro e transição de estágios (`prepared`, `stopping`, `committed`, `completed`, `blocked`, `interrupted`) com hash SHA-256 canônico dos argumentos (`args_hash`, nunca persistindo env ou segredos brutos) e reconciliação de operações interrompidas no boot.
5. **Configurações e Retenção (`settings` + `retention`):**
   - Registro das chaves `workspace.sessions.keep_per_tab=50` (1–1000), `workspace.sessions.max_age_days=90` (1–3650) e `workspace.scrollback.limit_mib=8` (1–256) no `SettingsSchema` da Fase 03.
   - Regra de retenção por idade (`max_age_days`) e job em lotes por quantidade (`keep_per_tab`) que apaga sessões arquivadas excedentes, limpa abas manuais fechadas sem histórico e preserva abas `origin = 'workspace'` enquanto o template existir.
   - Limpeza em cascata explícita de sessões, grupos, scrollback, abas e painéis na remoção de projeto (`delete_project_workspace_data_tx`).

---

## 2. Arquivos a Criar ou Modificar

### Criar
- `src-tauri/crates/mt-db/src/repos/tabs.rs`
- `src-tauri/crates/mt-db/src/repos/panes.rs`
- `src-tauri/crates/mt-db/src/repos/tab_sessions.rs`
- `src-tauri/crates/mt-db/src/repos/scrollback.rs`
- `src-tauri/crates/mt-db/src/repos/workspace_operations.rs`

### Modificar
- `src-tauri/crates/mt-db/src/repos/mod.rs` (exportar novos repositórios e integrar limpeza de remoção de projeto)
- `src-tauri/crates/mt-db/src/settings.rs` (registrar as 3 settings de workspace no `SettingsSchema`)
- `src-tauri/crates/mt-db/src/retention.rs` (registrar regra de idade de sessões arquivadas e expor executor de retenção por contagem `keep_per_tab`)

---

## 3. Especificação Técnica Detalhada

### 3.1. `TabsRepo` e `PanesRepo` (`repos/tabs.rs`, `repos/panes.rs`)

```rust
use crate::Db;
use mt_core::error::MtError;
use mt_core::ids::{PaneId, ProjectId, TabId, TabSessionId, TemplateId};
use mt_core::workspace::layout::{LayoutSizePatch, PaneLayoutNode};
use mt_core::workspace::tab::TabOrigin;
use mt_core::workspace::template::LaunchSnapshot;
use rusqlite::Transaction;

#[derive(Debug, Clone)]
pub struct PersistedPaneRow {
    pub id: PaneId,
    pub tab_id: TabId,
    pub cwd: String,
    pub generation: i64,
    pub created_at: i64,
    pub closed_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct PersistedTabRow {
    pub id: TabId,
    pub project_id: ProjectId,
    pub template_id: Option<TemplateId>,
    pub origin: TabOrigin,
    pub title: String,
    pub icon: String,
    pub position: i64,
    pub continuous: bool,
    pub launch: LaunchSnapshot,
    pub layout: PaneLayoutNode,
    pub active_pane_id: PaneId,
    pub primary_pane_id: PaneId,
    pub revision: i64,
    pub layout_revision: i64,
    pub last_session_seq: i64,
    pub opened_at: i64,
    pub closed_at: Option<i64>,
    pub last_focused_at: Option<i64>,
    pub active_tab_session_id: Option<TabSessionId>,
    pub open_panes: Vec<PersistedPaneRow>,
}

#[derive(Clone)]
pub struct TabsRepo {
    db: Db,
}

impl TabsRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Cria uma nova aba manual (ou reabre/cria aba `origin = 'workspace'`) junto com seu painel raiz
    /// na mesma transação. Valida que `template_id` (quando presente) pertence ao mesmo `project_id`.
    pub fn create_tab_with_root_pane_tx(
        tx: &Transaction<'_>,
        tab_id: TabId,
        root_pane_id: PaneId,
        project_id: ProjectId,
        template_id: Option<TemplateId>,
        origin: TabOrigin,
        launch: &LaunchSnapshot,
        now_ms: i64,
    ) -> Result<PersistedTabRow, MtError>;

    /// Localiza a aba `origin = 'workspace'` associada a `template_id` (mesmo que `closed_at IS NOT NULL`),
    /// permitindo reutilizar `TabId`, layout e folhas válidas durante o restore do boot.
    pub fn find_workspace_tab_by_template_tx(
        tx: &Transaction<'_>,
        template_id: TemplateId,
    ) -> Result<Option<PersistedTabRow>, MtError>;

    pub async fn get_open_tab(&self, tab_id: TabId) -> Result<PersistedTabRow, MtError>;
    pub async fn get_tab_any_state(&self, tab_id: TabId) -> Result<PersistedTabRow, MtError>;
    pub async fn list_open_by_project(&self, project_id: ProjectId) -> Result<Vec<PersistedTabRow>, MtError>;
    pub async fn list_all_open(&self) -> Result<Vec<PersistedTabRow>, MtError>;

    /// Reordena todas as abas abertas de um projeto. Falha com `tab.reorder_mismatch` se o conjunto de IDs
    /// não coincidir exatamente com as abas abertas (`closed_at IS NULL`) de `project_id`.
    pub async fn reorder(
        &self,
        project_id: ProjectId,
        ordered_tab_ids: &[TabId],
        now_ms: i64,
    ) -> Result<Vec<TabId>, MtError>;

    /// Atualiza `active_pane_id` (validando que pertence aos painéis abertos da aba) e `last_focused_at`.
    pub async fn update_focus(
        &self,
        tab_id: TabId,
        pane_id: Option<PaneId>,
        now_ms: i64,
    ) -> Result<PersistedTabRow, MtError>;

    /// Aplica patches de redimensionamento validando `expected_layout_revision`.
    /// Incrementa `revision` e `layout_revision`.
    pub async fn apply_layout_patches(
        &self,
        tab_id: TabId,
        expected_layout_revision: i64,
        patches: &[LayoutSizePatch],
    ) -> Result<PersistedTabRow, MtError>;
}

#[derive(Clone)]
pub struct PanesRepo {
    db: Db,
}

impl PanesRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Insere novo painel em split e atualiza `tabs.layout_json`, `tabs.layout_revision`, `tabs.revision`
    /// e `tabs.active_pane_id` na mesma transação.
    pub fn split_pane_tx(
        tx: &Transaction<'_>,
        tab_id: TabId,
        target_pane_id: PaneId,
        new_pane_id: PaneId,
        new_node_id: String,
        direction: mt_core::workspace::layout::SplitDirection,
        expected_layout_revision: i64,
        inherited_cwd: &str,
        now_ms: i64,
    ) -> Result<PersistedTabRow, MtError>;

    /// Incrementa `panes.generation` e atualiza `panes.cwd` para restart ou nova sessão.
    pub fn bump_generation_tx(
        tx: &Transaction<'_>,
        pane_id: PaneId,
        cwd: &str,
    ) -> Result<i64, MtError>;

    /// Marca um painel intermediário como fechado (`closed_at = now_ms`), atualizando `layout_json`,
    /// `active_pane_id` e `primary_pane_id` na mesma transação.
    pub fn close_intermediate_pane_tx(
        tx: &Transaction<'_>,
        tab_id: TabId,
        pane_id: PaneId,
        now_ms: i64,
    ) -> Result<PersistedTabRow, MtError>;
}
```

---

### 3.2. `TabSessionsRepo` e Adaptação com `SessionsRepo` da Fase 05 (`repos/tab_sessions.rs`)

```rust
use crate::Db;
use mt_core::error::MtError;
use mt_core::ids::{PaneId, ProjectId, SessionId, TabId, TabSessionId};
use mt_core::workspace::session::{
    CursorPage, InitialDispatchState, SessionDetail, SessionEndReason, SessionSummary,
};
use rusqlite::Transaction;

#[derive(Debug, Clone)]
pub struct PreparedPaneExecution {
    pub tab_session_id: TabSessionId,
    pub session_id: SessionId,
    pub pane_id: PaneId,
    pub generation: i64,
    pub cwd: String,
    pub initial_dispatch: InitialDispatchState,
}

#[derive(Clone)]
pub struct TabSessionsRepo {
    db: Db,
}

impl TabSessionsRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Aloca `seq = tabs.last_session_seq + 1`, atualiza `tabs.last_session_seq` e cria um novo
    /// agrupamento ativo em `tab_sessions` (`archived_at = NULL`).
    pub fn start_group_tx(
        tx: &Transaction<'_>,
        tab_id: TabId,
        tab_session_id: TabSessionId,
        now_ms: i64,
    ) -> Result<i64, MtError>;

    /// Prepara uma sessão base na tabela `sessions` (via primitivo de `SessionsRepo` da Fase 05)
    /// e cria o vínculo ativo correspondente em `tab_session_panes`.
    pub fn prepare_pane_session_tx(
        tx: &Transaction<'_>,
        tab_session_id: TabSessionId,
        session_id: SessionId,
        project_id: ProjectId,
        tab_id: TabId,
        pane_id: PaneId,
        generation: i64,
        shell: &str,
        cwd: &str,
        initial_dispatch: InitialDispatchState,
        now_ms: i64,
    ) -> Result<PreparedPaneExecution, MtError>;

    /// Transiciona atomicamente `initial_dispatch`:
    /// - `claim_initial_dispatch`: exige estado `pending` ou `waiting` para `(tab_session_id, pane_id, generation)`
    ///   e grava `claimed` retornando `true` (idempotente: retorna `false` se já `claimed` ou `sent`).
    /// - `mark_initial_dispatch_waiting`: transiciona `pending -> waiting`.
    /// - `mark_initial_dispatch_sent`: transiciona `claimed -> sent`.
    pub async fn claim_initial_dispatch(
        &self,
        tab_session_id: TabSessionId,
        pane_id: PaneId,
        generation: i64,
    ) -> Result<bool, MtError>;

    pub async fn mark_initial_dispatch_waiting(
        &self,
        tab_session_id: TabSessionId,
        pane_id: PaneId,
        generation: i64,
    ) -> Result<(), MtError>;

    pub async fn mark_initial_dispatch_sent(
        &self,
        tab_session_id: TabSessionId,
        pane_id: PaneId,
        generation: i64,
    ) -> Result<(), MtError>;

    /// Arquiva uma sessão base individual já encerrada pela Fase 05:
    /// Exige `sessions.status = 'closed'`, `sessions.ended_at IS NOT NULL` e zero linhas em
    /// `command_blocks` com `status = 'running'` para este `session_id`.
    /// Grava `sessions.status = 'archived'`, `sessions.archived_at = now_ms` e `tab_session_panes.closed_at = now_ms`.
    /// Idempotente caso a sessão já esteja `archived`.
    pub fn archive_closed_pane_session_tx(
        tx: &Transaction<'_>,
        session_id: SessionId,
        now_ms: i64,
    ) -> Result<(), MtError>;

    /// Arquiva o agrupamento `tab_sessions` após confirmar que todos os vínculos em `tab_session_panes`
    /// estão fechados e todas as sessões base estão `archived`.
    pub fn archive_group_tx(
        tx: &Transaction<'_>,
        tab_session_id: TabSessionId,
        end_reason: SessionEndReason,
        now_ms: i64,
    ) -> Result<SessionSummary, MtError>;

    /// Lista agrupamentos de sessão de uma aba (aberta ou fechada) em ordem `seq DESC` com cursor opaco.
    pub async fn list_by_tab(
        &self,
        tab_id: TabId,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<CursorPage<SessionSummary>, MtError>;

    /// Obtém o resumo e a lista de painéis (`SessionPaneSummary`) de um `TabSessionId`.
    pub async fn get_detail(&self, tab_session_id: TabSessionId) -> Result<SessionDetail, MtError>;
}
```

#### Formato de Cursores Opacos
- Cursor de `session_list`: JSON codificado em Base64 URL-safe sem padding (`base64url`) contendo `{"tabId":"<uuid>","seq":<i64>}`.
  - Ao receber `cursor`, decodifica e valida que `cursor.tab_id == tab_id` e `cursor.seq > 0`; caso contrário retorna `MtError::session_query_invalid("cursor inválido para esta aba")`.
  - Query SQL usa `WHERE ts.tab_id = ?1 AND (?2 IS NULL OR ts.seq < ?2) ORDER BY ts.seq DESC LIMIT ?3`.
- Cálculo de agregados em `SessionSummary`:
  - `command_count`: `COUNT(cb.id)` das sessões base vinculadas em `tab_session_panes`.
  - `failed_command_count`: `COUNT(cb.id) FILTER (WHERE cb.exit_code IS NOT NULL AND cb.exit_code != 0)`.
  - `scrollback_bytes`: ` COALESCE(SUM(s.scrollback_bytes), 0)` das sessões base vinculadas.
  - `truncated`: `MAX(s.scrollback_truncated) == 1`.
  - `ended_at`: para grupos arquivados, `ts.archived_at`.

---

### 3.3. `ScrollbackRepo` e Busca FTS5 (`repos/scrollback.rs`)

```rust
use crate::sanitizer::SanitizedText;
use crate::Db;
use mt_core::error::MtError;
use mt_core::ids::{SessionId, TabId, TabSessionId};
use mt_core::workspace::session::{CursorPage, ScrollbackChunk, SessionSearchHit};

pub const MAX_SCROLLBACK_PAGE_ITEMS: usize = 200;
pub const MAX_SCROLLBACK_PAGE_BYTES: usize = 1024 * 1024; // 1 MiB
pub const MAX_SCROLLBACK_CHUNK_BYTES: usize = 65_536; // 64 KiB

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollbackChunkKind {
    Output,
    Gap,
}

#[derive(Debug, Clone)]
pub struct NewScrollbackChunk {
    pub session_id: SessionId,
    pub ts: i64,
    pub text: SanitizedText,
    pub kind: ScrollbackChunkKind,
}

#[derive(Clone)]
pub struct ScrollbackRepo {
    db: Db,
}

impl ScrollbackRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Insere um lote de chunks sanitizados em uma única transação do writer:
    /// - Para cada chunk, lê e incrementa `sessions.next_scrollback_seq`, `sessions.scrollback_bytes`
    ///   e `sessions.scrollback_truncated` (quando `kind == Gap` ou `mark_truncated` estiver ativo).
    /// - Garante `byte_len == text.as_str().len() <= 65536`.
    pub async fn insert_batch(
        &self,
        chunks: Vec<NewScrollbackChunk>,
        truncated_sessions: Vec<SessionId>,
    ) -> Result<(), MtError>;

    /// Retorna página de chunks em ordem `seq ASC` para `(tab_session_id, session_id)`.
    /// Valida que `session_id` pertence a `tab_session_id` em `tab_session_panes`.
    /// Interrompe o preenchimento da página ao atingir `limit` (1..=200) OU quando a soma de `byte_len`
    /// atingir `MAX_SCROLLBACK_PAGE_BYTES` (1 MiB), emitindo `next_cursor` para o último `seq` entregue.
    pub async fn get_scrollback_page(
        &self,
        tab_session_id: TabSessionId,
        session_id: SessionId,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<CursorPage<ScrollbackChunk>, MtError>;

    /// Pesquisa FTS5 no scrollback de todas as sessões de `tab_id` usando `build_match_query` da Fase 03.
    /// Restringe `session_id` via JOIN com `tab_session_panes` e `tab_sessions` antes do `LIMIT` (1..=50).
    /// O snippet retornado é texto puro sanitizado (sem HTML).
    pub async fn search_in_tab(
        &self,
        tab_id: TabId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SessionSearchHit>, MtError>;
}
```

- Cursor de `session_scrollback_get`: JSON codificado em `base64url` contendo `{"tabSessionId":"<uuid>","sessionId":"<uuid>","seq":<i64>}`. Valida igualdade exata de `tab_session_id` e `session_id` antes de consultar `seq > cursor.seq`.

---

### 3.4. `WorkspaceOperationsRepo` (`repos/workspace_operations.rs`)

```rust
use crate::Db;
use mt_core::error::MtError;
use mt_core::ids::TabId;
use serde::{de::DeserializeOwned, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationStage {
    Prepared,
    Stopping,
    Committed,
    Completed,
    Blocked,
    Interrupted,
}

#[derive(Debug, Clone)]
pub struct WorkspaceOperationRecord {
    pub request_id: String,
    pub kind: String,
    pub args_hash: String,
    pub tab_id: Option<TabId>,
    pub boot_id: String,
    pub stage: OperationStage,
    pub result_json: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub enum AcquireOperationOutcome<T> {
    /// Novo request registrado como `prepared`.
    Acquired,
    /// Já concluído anteriormente com os mesmos argumentos: retorna o resultado serializado.
    AlreadyCompleted(T),
    /// Em andamento no mesmo boot com os mesmos argumentos.
    InProgress(OperationStage),
}

#[derive(Clone)]
pub struct WorkspaceOperationsRepo {
    db: Db,
}

impl WorkspaceOperationsRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Calcula SHA-256 hex sobre a representação JSON canônica dos argumentos (sem valores secretos resolvidos).
    pub fn compute_args_hash<A: Serialize>(kind: &str, args: &A) -> Result<String, MtError>;

    /// Registra ou consulta `request_id`:
    /// - Se `request_id` já existir com `args_hash` diferente -> `Err(MtError::workspace_request_conflict(request_id))`.
    /// - Se `stage == Interrupted` -> `Err(MtError::workspace_operation_interrupted(request_id))`.
    /// - Se `stage == Completed` -> desserializa `result_json` e retorna `AlreadyCompleted(T)`.
    pub async fn acquire_or_inspect<T: DeserializeOwned>(
        &self,
        request_id: &str,
        kind: &str,
        args_hash: &str,
        tab_id: Option<TabId>,
        boot_id: &str,
        now_ms: i64,
    ) -> Result<AcquireOperationOutcome<T>, MtError>;

    pub async fn update_stage(
        &self,
        request_id: &str,
        stage: OperationStage,
        tab_id: Option<TabId>,
        now_ms: i64,
    ) -> Result<(), MtError>;

    pub async fn mark_completed<T: Serialize>(
        &self,
        request_id: &str,
        tab_id: Option<TabId>,
        result: &T,
        now_ms: i64,
    ) -> Result<(), MtError>;

    /// No boot, marca como `interrupted` todas as operações de `boot_id != current_boot_id`
    /// que ficaram em `prepared`, `stopping` ou `committed`, retornando a quantidade afetada.
    pub async fn mark_stale_boots_interrupted(
        &self,
        current_boot_id: &str,
        now_ms: i64,
    ) -> Result<usize, MtError>;
}
```

---

### 3.5. Settings, Retenção e Remoção de Projeto (`settings.rs`, `retention.rs`)

1. **Settings Registradas no `SettingsSchema` da Fase 03:**
   - `workspace.sessions.keep_per_tab`: inteiro, padrão `50`, intervalo `1..=1000`.
   - `workspace.sessions.max_age_days`: inteiro, padrão `90`, intervalo `1..=3650`.
   - `workspace.scrollback.limit_mib`: inteiro, padrão `8`, intervalo `1..=256`.
2. **Retenção de Sessões Arquivadas:**
   - Regra de idade registrada na infraestrutura de retenção da Fase 03: remove em lotes sessões com `archived_at IS NOT NULL AND archived_at < cutoff_ms`.
   - Job dedicado `TabSessionsRepo::enforce_keep_per_tab(&self, keep_per_tab: usize, batch_size: usize) -> Result<usize, MtError>`:
     - Nunca toca sessões nem grupos com `archived_at IS NULL`.
     - Para cada `tab_id` com mais de `keep_per_tab` grupos arquivados em `tab_sessions`, identifica os grupos mais antigos (`seq` menores), apaga as linhas correspondentes em `sessions` (disparando cascade para `command_blocks`, `session_scrollback`, triggers FTS5 e `tab_session_panes`) e apaga os `tab_sessions` excedentes.
     - Em seguida, apaga painéis fechados (`panes.closed_at IS NOT NULL`) que não possuam mais referências em `tab_session_panes`, e apaga abas manuais fechadas (`tabs.origin = 'manual' AND tabs.closed_at IS NOT NULL`) que não possuam mais nenhum `tab_sessions`. Abas `origin = 'workspace'` com `template_id IS NOT NULL` são preservadas para manter seu `TabId` e layout estáveis.
3. **Limpeza na Remoção de Projeto (`delete_project_workspace_data_tx`):**
   - Como `tab_session_panes.pane_id` tem `ON DELETE RESTRICT`, antes de deletar o projeto (ou dentro do hook transacional de remoção após encerrar todos os PTYs):
     1. `DELETE FROM sessions WHERE project_id = ?1;` (remove em cascata `command_blocks`, `session_scrollback`, FTS5 e `tab_session_panes`).
     2. `DELETE FROM tab_sessions WHERE tab_id IN (SELECT id FROM tabs WHERE project_id = ?1);`
     3. `DELETE FROM panes WHERE tab_id IN (SELECT id FROM tabs WHERE project_id = ?1);`
     4. `DELETE FROM tabs WHERE project_id = ?1;`

---

## 4. Casos de Borda e Tratamento de Erros

1. **Restrição `ON DELETE RESTRICT` em `tab_session_panes(pane_id)`:** Garante que nenhum painel com histórico de sessão possa ser apagado da tabela `panes` enquanto houver sessões vinculadas a ele; fechar um painel em runtime apenas preenche `panes.closed_at`, nunca faz `DELETE FROM panes`.
2. **Preservação de Sequência após Retenção (`tabs.last_session_seq`):** Mesmo que sessões antigas sejam expurgadas pela política de retenção, `tabs.last_session_seq` garante que o próximo `seq` de `tab_sessions` seja estritamente monotônico e nunca reutilize números antigos.
3. **Teto de 1 MiB em `session_scrollback_get`:** Se o usuário solicitar `limit = 200` e cada chunk tiver 64 KiB (total potencial de 12,8 MiB), `get_scrollback_page` interrompe a leitura aos 16 chunks (1 MiB) e devolve `next_cursor` apontando para o chunk 16, protegendo o canal IPC contra payloads excessivos.
