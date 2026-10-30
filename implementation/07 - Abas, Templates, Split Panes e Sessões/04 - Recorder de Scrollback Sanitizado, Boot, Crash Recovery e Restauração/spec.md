# Implementação 04 — Recorder de Scrollback Sanitizado, Boot, Crash Recovery e Restauração

## 1. Contexto, Objetivo e Escopo

Esta implementação entrega o pipeline assíncrono de gravação de scrollback histórico sanitizado (`ScrollbackRecorder` via `OutputTap` da Fase 02) e o coordenador de inicialização, recuperação pós-crash e restauração idempotente do workspace (`workspace/restore.rs`).

### Escopo Exato
1. **Recorder de Scrollback Sanitizado (`workspace/recorder.rs`):**
   - Implementação de `OutputTap` da Fase 02 registrada globalmente antes de qualquer spawn, mantendo o `TapHandle` vivo até o `shutdown`.
   - Cópia não bloqueante no `on_output` para uma fila limitada em bytes (**teto global de 8 MiB** de captura pendente), sem jamais bloquear a thread de leitura do PTY.
   - Worker dedicado com estado isolado por execução `(PtyId, SessionId, TabSessionId, generation)` e orçamento compartilhado por agrupamento de sessão `TabSessionId` (**padrão de 8 MiB por sessão inteira da aba**, somando todos os painéis, configurável via `workspace.scrollback.limit_mib`).
   - Decodificação UTF-8 incremental (preservando sequências multibyte partidas entre frames), parser ANSI/VT incremental (removendo CSI, OSC, DCS e caracteres de controle ativos), omissão de alternate screen (`?47h`, `?1047h`, `?1049h`) com marcador único por entrada (`[tela alternativa omitida]\n`), normalização de `\r` de barras de progresso e proteção de fronteira de segredos em linhas longas (limite de 16 KiB de linha pendente antes do `StreamSanitizer` da Fase 03).
   - Flush em lotes via `BatchWriter` da Fase 03 (200 ms / 512 registros / 1 MiB, chunks `<= 64 KiB` de `SanitizedText`) e barreira `finish_execution(pty_id)` que aguarda EOF, drenagem do worker e `BatchWriter::flush` antes de permitir o arquivamento da sessão.
2. **Sequência de Boot, Crash Recovery e Restauração Single-Flight (`workspace/restore.rs`):**
   - Limpeza de processos remanescentes via supervisor da Fase 02 antes de abrir novos PTYs.
   - Reconciliação de `workspace_operations` interrompidas de boots anteriores (`stage -> 'interrupted'`).
   - Encerramento de blocos de comando órfãos da Fase 05 como desconhecidos/Recovery (`exit_code = NULL`, nunca `0` fictício), fechamento das sessões base órfãs e arquivamento dos agrupamentos `tab_sessions` anteriores com `SessionEndReason::CrashRecovered` e `ended_at = max(started_at, last_persisted_ts)`.
   - Execução single-flight (`tokio::sync::OnceCell<WorkspaceRestoreReport>`) de `restore()`: para cada template com `add_to_workspace = 1` (ordenado por `Project.sort_order`, `Template.position`), localiza ou cria sua única aba `origin = 'workspace'`, reutilizando `TabId`, árvore de `PaneId`s, proporções e `cwd`s válidos, e restaura até 4 abas simultaneamente.
   - Garantia de idempotência em reload da WebView ou chamadas concorrentes no mesmo boot.

---

## 2. Arquivos a Criar ou Modificar

### Criar
- `src-tauri/src/workspace/recorder.rs`
- `src-tauri/src/workspace/restore.rs`

### Modificar
- `src-tauri/src/workspace/mod.rs` (declarar `pub mod recorder;` e `pub mod restore;`; a Implementação 03 não declara `recorder`)
- `src-tauri/src/workspace/manager.rs` (adicionar o import `use crate::workspace::recorder::ScrollbackRecorder;` e o campo `recorder: Arc<ScrollbackRecorder>` à struct `WorkspaceManager` da Implementação 03 e injetá-lo no construtor/builder, integrar `ScrollbackRecorder::finish_execution` no fechamento/nova sessão e expor `restore(&self) -> Result<WorkspaceRestoreReport, MtError>` conforme §3.2)
- `src-tauri/src/lib.rs` (registrar o `OutputTap` global do `ScrollbackRecorder` e acionar a sequência de inicialização do `WorkspaceManager` no startup do Tauri)

---

## 3. Especificação Técnica Detalhada

### 3.1. `ScrollbackRecorder` (`workspace/recorder.rs`)

```rust
use crate::workspace::adapters::{ExecutionOwner, WorkspaceSessionProvider};
use bytes::Bytes;
use dashmap::DashMap;
use mt_core::error::MtError;
use mt_core::ids::{PtyId, SessionId, TabSessionId};
use mt_db::repos::scrollback::{NewScrollbackChunk, ScrollbackChunkKind, ScrollbackRepo};
use mt_db::sanitizer::StreamSanitizer;
use mt_db::BatchWriter;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

pub const GLOBAL_CAPTURE_QUEUE_MAX_BYTES: usize = 8 * 1024 * 1024; // 8 MiB
pub const MAX_PENDING_LINE_BYTES: usize = 16 * 1024; // 16 KiB
pub const MAX_CHUNK_BYTES: usize = 64 * 1024; // 64 KiB
pub const ALT_SCREEN_MARKER: &str = "[tela alternativa omitida]\n";
pub const TRUNCATED_LINE_MARKER: &str = "[linha longa omitida por segurança]\n";
pub const OVERFLOW_GAP_MARKER: &str = "[trecho omitido por sobrecarga de saída]\n";
pub const BUDGET_EXCEEDED_MARKER: &str = "[limite de scrollback da sessão atingido]\n";

pub struct GroupScrollbackBudget {
    pub tab_session_id: TabSessionId,
    pub limit_bytes: usize,
    pub used_bytes: AtomicUsize,
    pub budget_exhausted: std::sync::atomic::AtomicBool,
}

pub struct ScrollbackRecorder {
    session_provider: Arc<WorkspaceSessionProvider>,
    scrollback_repo: ScrollbackRepo,
    batch_writer: Arc<BatchWriter<NewScrollbackChunk>>,
    queued_bytes: Arc<AtomicUsize>,
    group_budgets: DashMap<TabSessionId, Arc<GroupScrollbackBudget>>,
    worker_tx: mpsc::UnboundedSender<RecorderCommand>,
}

impl ScrollbackRecorder {
    pub fn new(
        session_provider: Arc<WorkspaceSessionProvider>,
        scrollback_repo: ScrollbackRepo,
        default_session_limit_bytes: usize,
    ) -> Arc<Self>;

    /// Registra o orçamento compartilhado de bytes para um `TabSessionId` (ex.: 8 MiB para a sessão inteira da aba,
    /// compartilhado entre todos os seus painéis).
    pub fn register_group_budget(&self, tab_session_id: TabSessionId, limit_bytes: usize);

    pub fn release_group_budget(&self, tab_session_id: TabSessionId);

    /// Drena todos os bytes pendentes de `pty_id`, emite o chunk final sanitizado (e marcador de truncamento
    /// se aplicável) e aguarda `BatchWriter::flush()` confirmar a gravação em disco no SQLite.
    pub async fn finish_execution(&self, pty_id: PtyId) -> Result<(), MtError>;
}

impl mt_pty::OutputTap for ScrollbackRecorder {
    fn on_output(&self, pty_id: PtyId, _seq: u64, bytes: &Bytes);
    fn on_exit(&self, pty_id: PtyId);
}
```

#### Pipeline de Processamento por Execução no Worker do `ScrollbackRecorder`
1. **Backpressure e Overflow na Fila Global (`on_output`):**
   - `on_output` verifica atomicamente se `queued_bytes + bytes.len() <= GLOBAL_CAPTURE_QUEUE_MAX_BYTES`.
   - Se houver espaço, incrementa `queued_bytes` e envia `RecorderCommand::Frame { pty_id, owner, bytes, ts_ms }`.
   - Se a fila global de 8 MiB estiver cheia, **não bloqueia o leitor do PTY**: incrementa um flag atômico de `overflow_pending` para aquela execução e descarta o frame bruto. Quando a fila voltar a aceitar dados (ou no `finish_execution`), o worker descarta qualquer linha/token incompleto anterior e posterior até a próxima quebra de linha segura (`\n`), emite um chunk `ScrollbackChunkKind::Gap` com `OVERFLOW_GAP_MARKER` e marca `scrollback_truncated = 1` na sessão base. Nunca concatena fragmentos de antes e depois de uma lacuna (evitando juntar pedaços que ocultem ou corrompam a detecção de segredos).
2. **Decodificação UTF-8 Incremental:**
   - Mantém um buffer de até 3 bytes finais incompletos por `PtyId`. Se um caractere UTF-8 de 2, 3 ou 4 bytes for cortado na fronteira entre dois frames `on_output`, os bytes iniciais aguardam o próximo frame sem gerar `U+FFFD` espúrio.
3. **Filtro Incremental de ANSI / Alternate Screen / Carriage Return:**
   - Máquina de estados ANSI por execução que preserva sequências de escape partidas entre frames (`\x1b[...`, `\x1b]...`, `\x1bP...`).
   - Ao detectar entrada em alternate screen (`\x1b[?47h`, `\x1b[?1047h`, `\x1b[?1049h`), ativa `in_alt_screen = true` e emite uma única vez `ALT_SCREEN_MARKER`. Enquanto `in_alt_screen == true`, descarta todo o conteúdo da TUI (`vim`, `htop`, `claude`, `agy`) até receber a saída (`\x1b[?47l`, `\x1b[?1047l`, `\x1b[?1049l`).
   - Remove todas as sequências CSI, OSC, DCS e controles C0 exceto `\n` e `\t`.
   - Para `\r` não seguido de `\n` (barras de progresso no terminal), substitui a linha pendente atual em memória desde o último `\n`, evitando acumular milhares de linhas falsas de progresso no histórico.
4. **Proteção de Linhas Longas e Sanitização (`StreamSanitizer`):**
   - Nunca insere `\n` artificial no meio de uma linha longa antes de passar pelo `StreamSanitizer` da Fase 03 (pois quebrar uma linha no meio poderia partir um token `Bearer`, chave privada PEM ou `APP_KEY`).
   - Se a linha pendente sem `\n` ultrapassar `MAX_PENDING_LINE_BYTES` (16 KiB), entra em modo `discarding_long_line = true`, descarta tudo até o próximo `\n`, emite `ScrollbackChunkKind::Gap` com `TRUNCATED_LINE_MARKER` e marca `scrollback_truncated = 1`.
   - Caso contrário, alimenta `StreamSanitizer::push_str` (que também retém blocos PEM multi-linha incompletos até `-----END ...-----` ou suprime blocos PEM truncados no EOF).
5. **Orçamento de 8 MiB por `TabSessionId` e Empacotamento em Chunks (≤ 64 KiB):**
   - O orçamento de bytes (`limit_bytes`, padrão 8 MiB) pertence ao `GroupScrollbackBudget` do `TabSessionId`, reservando espaço para `BUDGET_EXCEEDED_MARKER` no final.
   - Se a soma de bytes de todos os painéis daquele `TabSessionId` atingir o orçamento, emite `ScrollbackChunkKind::Gap` com `BUDGET_EXCEEDED_MARKER`, marca `scrollback_truncated = 1` e ignora bytes subsequentes daquela sessão.

---

### 3.2. Inicialização, Crash Recovery e Restauração Idempotente (`workspace/restore.rs`)

```rust
use crate::workspace::manager::WorkspaceManager;
use mt_core::error::MtError;
use mt_core::workspace::session::WorkspaceRestoreReport;

impl WorkspaceManager {
    /// Executa a recuperação pós-crash e a restauração automática das abas marcadas com `add_to_workspace = 1`.
    /// Single-flight garantido por `OnceCell`: chamadas concorrentes ou subsequentes no mesmo boot
    /// aguardam e recebem exatamente o mesmo `WorkspaceRestoreReport` sem reabrir abas ou repetir comandos.
    pub async fn restore(&self) -> Result<WorkspaceRestoreReport, MtError>;
}
```

#### Etapas Determinísticas do Boot e `restore()`
0. **Composição antes do boot:** a lista de `WorkspaceSpawnHook` já foi fixada por `with_spawn_hooks`
   antes do manager ser compartilhado; na composição da Fase 08 inclui o adaptador Git. Restauração
   usa o mesmo pipeline reserva → spawn → registro confirmado → publicação de `07/03` §3.4 em cada
   painel, sem caminho especial que ignore hooks; não instala hooks durante `restore()`.
1. **Limpeza de Processos Remanescentes (Fase 02) e Instalação de Taps:**
   - Antes de qualquer spawn, aguarda o supervisor da Fase 02 concluir o expurgo de processos órfãos de execuções anteriores registradas.
   - Registra o `ScrollbackRecorder` via `pty_manager.add_output_tap(...)` e os hooks do `WorkspaceSessionProvider` na Fase 05.
2. **Recuperação de Operações e Sessões Órfãs (Crash Recovery):**
   - Chama `WorkspaceOperationsRepo::mark_stale_boots_interrupted(&self.boot_id, now_ms)` para transicionar qualquer operação deixada em `prepared`, `stopping` ou `committed` de outro `boot_id` para `interrupted`.
   - Chama a rotina de recovery da Fase 05 para fechar `command_blocks` pendentes como desconhecidos (`status = 'unknown'`, `exit_code = NULL`) e fechar sessões base abertas (`status = 'closed'`).
   - Em transação da Fase 07, arquiva todas as sessões base fechadas que ainda estavam com `archived_at IS NULL`, fecha todos os vínculos `tab_session_panes` abertos e arquiva todos os `tab_sessions` ativos de boots anteriores com `end_reason = 'crash_recovered'` e `archived_at = max(started_at, ultimo_ts_persistido)`.
   - Marca `tabs.closed_at = now_ms` em todas as abas que haviam ficado abertas no boot anterior (garantindo que abas manuais não sejam reabertas e que nenhum `PtyId` antigo seja reutilizado).
   - Repara defensivamente templates padrão faltantes em todos os projetos (`TemplateRepo::ensure_default_tx`).
3. **Restauração Automática de Templates com `add_to_workspace = 1`:**
   - Consulta todos os templates com `add_to_workspace = 1` ordenados por `(projects.sort_order ASC, projects.created_at ASC, tab_templates.position ASC, tab_templates.id ASC)`.
   - Limita a concorrência de abertura a no máximo **4 abas em paralelo** (`futures::stream::StreamExt::buffer_unordered(4)`).
   - Para cada template:
     - Busca a aba `origin = 'workspace'` vinculada àquele `template_id` (`TabsRepo::find_workspace_tab_by_template_tx`).
     - Se existir:
       - Reutiliza o mesmo `TabId`.
       - Valida a árvore `layout_json` e as folhas em `panes`. Se a árvore persistida for válida, preserva todos os `PaneId`s, proporções, `active_pane_id`, `primary_pane_id` e o `cwd` de cada painel; se a árvore estiver corrompida, preserva o registro antigo em log de diagnóstico e substitui o layout por uma folha única nova sem apagar o histórico de sessões da aba.
       - Atualiza `title`, `icon`, `continuous` e `launch_json` com a configuração **atual** do template.
       - Reabre a aba (`closed_at = NULL`, `revision += 1`), cria um novo `TabSessionId` (`seq = last_session_seq + 1`), incrementa `generation` de cada painel e prepara um novo `SessionId` base para cada painel.
     - Se não existir:
       - Cria a aba `origin = 'workspace'` com seu `TabId`, folha raiz `PaneId`, `TabSessionId` (`seq = 1`) e `SessionId`.
     - Executa o spawn supervisionado de cada painel:
       - Se o `cwd` de um painel não existir mais no disco, aquele painel não aborta a aba inteira: entra em `PaneRuntimeState::Failed` com `MtError::template_cwd_not_found(...)` (permitindo que o usuário acione o botão de fallback para a pasta raiz do projeto na UI).
       - O comando inicial do template é elegível **somente** no `primary_pane_id` da nova execução.
4. **Publicação e Reload da WebView:**
   - Armazena o `WorkspaceRestoreReport` no `OnceCell` e emite o evento `workspace://restored`.
   - Quando a WebView recarrega (`location.reload()` / HMR), ela chama `workspace_snapshot()` (somente leitura) e reconecta os terminais existentes via `pty_attach` da Fase 04, sem reexecutar `restore()`, sem criar novos PTYs e sem repetir comandos iniciais.

---

## 4. Casos de Borda e Tratamento de Erros

1. **Segredo ou PEM Fragmentado entre Frames no Recorder:** O teste de corte byte a byte deve alimentar um token `Bearer eyJ...` e uma chave `-----BEGIN OPENSSH PRIVATE KEY-----` em fatias de 1 a 7 bytes; o buffer de linha pendente + `StreamSanitizer` garante que 0 bytes do segredo cheguem a `session_scrollback` ou `session_scrollback_fts`.
2. **Fechamento de Aba Automática Durante o Próprio Boot:** Se o usuário fechar uma aba `origin = 'workspace'` logo após ela abrir enquanto outras abas ainda estão restaurando, o fechamento não dispara nova tentativa de abertura no mesmo boot; ela só voltará a abrir no próximo boot se `add_to_workspace` continuar `true`.
3. **Abertura Manual do Mesmo Template:** Se o usuário clicar em "Iniciar Aba Template" escolhendo um template que possui `add_to_workspace = true`, `tab_open` cria uma aba com `origin = 'manual'` e novo `TabId`, sem colidir com o índice único parcial `ux_tabs_workspace_template` (`WHERE origin = 'workspace'`).
