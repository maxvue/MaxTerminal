# Fase 07 — Especificação Técnica: Abas, Templates, Split Panes e Sessões

> Objetivos e aceite: [`instructions.md`](./instructions.md). Contratos gerais: [`../README.md`](../README.md).
> Dependências diretas **04, 05, 06**; persistência **03 transitiva**. Nenhuma dependência de fase posterior.

## 1. Autoridades e invariantes

`WorkspaceManager` no crate binário orquestra; `mt-core::workspace` contém tipos/funções puras;
`mt-db` persiste; `mt-pty` supervisiona; `mt-shell` captura comandos e calcula status. Não há novo crate.

```text
Project → TabTemplate
Project → TabId → árvore de PaneId
TabId → TabSessionId atual + histórico de agrupamentos
TabSessionId → SessionId por execução de painel
(TabId, PaneId, SessionId, generation) → PtyId vivo → processos supervisionados
```

- Uma aba pertence a um único projeto. Template escolhido deve pertencer ao mesmo projeto.
- Uma aba aberta tem exatamente um agrupamento ativo `TabSessionId`; cada painel/stream tem seu `SessionId`
  da 05. Painéis encerrados permanecem identificáveis no histórico. O agrupamento pode sobreviver à saída
  espontânea de todos os PTYs, mas suas sessões base são fechadas pela captura.
- `PtyId` identifica uma execução, nunca é reutilizado nem restaurado do SQLite. `PaneId` identifica uma folha
  persistente; `TabId` identifica a aba e seu histórico. Nova sessão preserva aba/folhas e troca grupo/sessões/PTYs.
- `SessionId` refere-se à **mesma tabela `sessions` mínima da 05**, usada pela captura de comandos.
  Não há sessão concorrente exclusiva do frontend e não há `CREATE TABLE sessions` nesta fase.
- Só o manager abre/encerra PTYs do workspace. Montar um componente e ler histórico não criam processos.
- Status (`running`, `continuous`, `finished`, `error`, `idle`) e `seen` são calculados pela 05; estados
  operacionais (`opening`, `ready`, `restarting`, `closing`, `blocked`) são separados.

## 2. Contratos consumidos e adaptadores

| Origem | Contrato efetivo | Uso |
|--------|------------------|-----|
| 01 | Ids Uuid v7, `MtError`, bindings `specta` | Identidade, erros pt-BR e IPC tipado |
| 02 | `spawn(SpawnSpec, Option<Arc<dyn OutputSink>>)`, `write(PtyId, Bytes)` | Spawn sem view: sink `None`, ring da 02 até `pty_attach` |
| 02 | `kill_and_wait(PtyId, KillMode, Duration) -> ExitInfo` | Chamada bloqueante em `spawn_blocking`; não confundir com `kill`, que apenas agenda |
| 02 | `PtyOwner { tab_id: Option<TabId>, pane_id: Option<PaneId> }`, `subscribe`, `process_tree`, `pids` | Ownership e saída; nunca matar PID não validado pelo supervisor |
| 02 | `add_output_tap(Arc<dyn OutputTap>) -> TapHandle`; `on_output(pty_id, seq, bytes)`, `on_exit` | Tap global registrado antes dos spawns, handle mantido até shutdown |
| 03 | `Db::write/read`, `DbSetup`, migrations, `BatchWriter`, `SanitizedText`, `StreamSanitizer`, `FtsSpec` | Infraestrutura WAL, batching, sanitização e manutenção |
| 04 | `HybridTerminal(paneId, ptyId, visible)`, `TerminalInstanceRegistry::{getOrCreate,dispose}`, `CommandBlockFeed.clear` | Render/stream e limpeza por painel |
| 05 | `RuntimeBinding`, `SessionProvider::{prepare,close,abort}`, lifecycle de spawn | Sessão por painel preparada antes do reader; hooks registrados uma vez |
| 05 | Eventos de prompt confiável, registry de status e Reset | Prontidão, status e `seen`; geração rejeita eventos antigos |
| 05 | `SessionsRepo::{prepare,close,abort,get}`, `CommandBlocksRepo`, `sessions`, `command_blocks` | Base de sessões por painel e blocos persistidos |
| 06 | `ProjectInsertContributor::after_insert(tx, project)`, `ProjectActivityGuard`, `ProjectRemovalHook::before_remove` | Padrão atômico e encerramento pré-remoção |
| 06 | `selectedId`, `attachTab(tabId, projectId)`, `detachTab`, `setActiveTab`; prop `project` do workspace | Seleção e badges; assinatura preservada |

`workspace/adapters.rs` contém `PtyLifecycleAdapter`, `ShellWorkspaceAdapter`, `BaseSessionAdapter`;
`mt-db/src/repos/templates.rs` implementa `DefaultTemplateContributor` dentro de `mt-db`, onde a trait
`ProjectInsertContributor` da 06 é `pub(crate)`. O registro ocorre internamente na composição dos
repositórios, antes de liberar cadastros/watchers; o binário não recebe conexão SQL nem implementa essa
trait privada. Os adapters são nomes **novos desta fase**, não métodos supostamente existentes em 02–06.

Reconciliações obrigatórias antes de implementar:

1. A 05 é proprietária de `sessions` mínima e `command_blocks`, via
   `NNNN_05_shell_sessions_command_blocks.sql`; a 03 fornece infraestrutura. Nenhum adapter altera
   silenciosamente schema ou semântica. O agrupamento da 07 não se passa por uma sessão base.
2. A 05 deve disponibilizar barreira de fim de captura/commit por PTY e sequenciamento dentro de cada sessão.
   Os adaptadores abaixo exigem essas capacidades; se ausentes, a integração é bloqueada até implementá-las.
3. A 07 usa o tracker integrado 05/06 já montado no `AppLayout`: foco/visibilidade, render da conclusão
   e `SeenRequest` com identidade/revisão conforme 05. Dwell de 300 ms, quando aplicado pela 06, apenas
   adia esse ACK; nunca marca snapshot inteiro nem confirma geração antiga. `tab_focus` não marca visto.

## 3. Domínio Rust e contrato de serialização

DTOs públicos derivam `Serialize`, `Deserialize` quando entrada e `specta::Type`. **Campos em camelCase no
wire**, enums/tag `kind` em snake_case. Todos os tipos TS são gerados em `src/types/bindings.ts`; amostras abaixo
definem domínio, não cópias manuais. Datas Unix ms e revisões são inteiros restritos ao intervalo seguro do JS;
sequências de stream `u64` seguem a representação da 02 (string), sem conversão para `number`.

### 3.1. Templates e configuração de lançamento

```rust
pub enum EnvironmentKind { Shell, Agent }
pub struct TemplateEnvironment {
    pub kind: EnvironmentKind,
    pub agent_id: Option<String>,
}
// Enum tagged por `kind`: literal ou inherited.
pub enum EnvValue {
    Literal { value: String },
    Inherited { source: String },
}
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
// TemplateConfig contém todos os campos de TabTemplateInput exceto make_default.
pub enum CommandOverride { Inherit, Clear, Replace { command: String } }
pub struct TabOpenOverrides {
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub initial_command: CommandOverride,
    pub extra_env: BTreeMap<String, EnvValue>,
    pub continuous: Option<bool>,
    pub cwd_fallback_to_project: bool,
}
pub struct TabOpenRequest {
    pub request_id: String,
    pub project_id: ProjectId,
    pub template_id: Option<TemplateId>,
    pub overrides: Option<TabOpenOverrides>,
    pub focus: bool,
}
```

`request_id` é UUID v7 de intenção, reutilizado em retry; mesmo id com argumentos diferentes retorna
`workspace.request_conflict`. Dois cliques deliberados geram ids diferentes. `focus` é intenção de UI;
não altera a regra de `seen`. `None` no template seleciona o padrão dentro da transação.

Validações no backend:

- Nome trim, 1–64 caracteres, sem controles; ícone Iconify válido com limite de 64 caracteres.
- Comando até 4096 bytes UTF-8, sem NUL; `Clear` remove comando herdado, diferente de `Inherit`.
- Cwd: `overrides.cwd` → template → projeto; `~`/`~/` expandidos; relativos ancorados no projeto;
  `canonicalize` fora da UI; exigir diretório UTF-8. Falha retorna `template.cwd_not_found`; fallback somente
  quando explicitamente solicitado, validando também a raiz. Não executar shell para expandir caminhos.
- Shell exige `agent_id = None`; agente exige slug `^[a-z0-9][a-z0-9._-]{0,63}$`. Salvar agente é permitido;
  iniciar sem resolvedor retorna `workspace.environment_unavailable`.
- Até 64 envs, chave/source `^[A-Za-z_][A-Za-z0-9_]*$`, valor até 8 KiB, conjunto até 64 KiB, sem NUL.
  Prefixos internos `MAXTERMINAL_` e `MT_` não podem ser sobrescritos pelo usuário.
- Literais de chaves sensíveis ou texto alterado por `sanitize()` são recusados com
  `template.secret_literal`; usar `Inherited { source }` ou variável no comando (`$TOKEN`). Nunca salvar
  a máscara como comando executável. Mesma política para overrides, snapshots e recibos de operação.
- Ambiente efetivo = ambiente permitido pela 02 + env do template + overrides + ids internos; `Inherited`
  consulta o ambiente permitido do processo, apenas em memória. Referência ausente gera erro de campo.
- `continuous` gera `ContinuousHint` da 05 ligado ao comando inicial, ou primeiro comando quando ausente;
  não pinta uma aba ociosa como em execução contínua.

`LaunchSnapshot` guarda a configuração validada usada ao abrir a aba (sem valores secretos resolvidos).
Editar/apagar template não altera silenciosamente execuções abertas. Nova sessão e restart usam esse snapshot.
Restauração automática aplica a configuração **atual** do template, mantendo layout, ids e cwd por painel
quando válidos; override manual não cria nem substitui uma aba automática.

### 3.2. Árvore e identidade

```rust
pub enum SplitDirection { Horizontal, Vertical }
// Horizontal = divisor horizontal, acima/abaixo; Vertical = divisor vertical, lado a lado.
pub enum PaneLayoutNode {
    Leaf { pane_id: PaneId },
    Split {
        node_id: String,
        direction: SplitDirection,
        children: Vec<PaneLayoutNode>,
        sizes: Vec<f64>,
    },
}
pub struct LayoutSizePatch { pub node_id: String, pub sizes: Vec<f64> }
pub enum PaneRuntimeState { Starting, Live, Exited, Failed, Stopping }
pub struct PaneView {
    pub id: PaneId,
    pub pty_id: Option<PtyId>,
    pub generation: i64,
    pub cwd: String,
    pub state: PaneRuntimeState,
    pub exit: Option<ExitInfo>,
    pub error: Option<MtError>,
}
pub enum TabLifecycle { Opening, Ready, Restarting, Closing, Blocked }
pub enum TabOrigin { Manual, Workspace }
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
```

Árvore canônica binária: cada `Split` tem dois filhos e tamanhos finitos ≥ 5%, soma 100 ± 0,01;
até oito folhas, profundidade até sete, ids de nós/folhas únicos, conjunto de folhas = painéis abertos.
Split substitui a folha por `[folha, nova]`, 50/50; não funde nós de mesma orientação (preserva ids/tamanhos).
Fechar remove a folha e colapsa o pai em seu irmão; último painel fecha a aba. `active_pane_id` sempre pertence
ao conjunto, escolhendo folha vizinha se a ativa for removida. `primary_pane_id` persistido indica quem recebe
comando inicial em nova sessão; se fechado, transfere-se à primeira folha sobrevivente, sem executar comando.

Resize envia `expected_layout_revision` e patches por `node_id`, nunca caminhos por índice sujeitos a corrida.
Backend valida conjunto e tipos, aplica atomicamente, incrementa revisão e devolve `TabView`.
`layout.conflict` exige snapshot novo; não reaplicar tamanhos a nós diferentes.

### 3.3. Histórico

```rust
pub enum SessionEndReason { NewSession, TabClosed, AppExit, CrashRecovered, ProjectRemoved, SpawnFailed }
// Novo newtype desta fase, Uuid v7, exportado por mt-core::workspace.
pub struct TabSessionId(pub uuid::Uuid);
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
pub struct SessionDetail {
    pub summary: SessionSummary,
    pub panes: Vec<SessionPaneSummary>,
}
pub struct SessionPaneSummary { pub session_id: SessionId, pub pane_id: PaneId, pub cwd: String, pub closed_at: Option<i64> }
pub struct ScrollbackChunk { pub session_id: SessionId, pub seq: i64, pub ts: i64, pub text: String }
// Comandos reutilizam CommandBlockDto da 05.
pub struct CursorPage<T> { pub items: Vec<T>, pub next_cursor: Option<String> }
pub struct SessionSearchHit { pub tab_session_id: TabSessionId, pub session_id: SessionId, pub pane_id: PaneId, pub seq: i64, pub snippet: String }
```

Detalhe não embute comandos ilimitados: usa `command_block_list/get` da 05 e scrollback paginado da 07.
`seq` de agrupamento é alocado transacionalmente por aba; `command_blocks.seq` e `session_scrollback.seq` são
por `SessionId` de painel, não contadores de PTY. Reinício de painel cria outra sessão base no mesmo grupo.

## 4. Orquestração e concorrência

```rust
pub struct WorkspaceManager {
    db: Db,
    pty: Arc<PtyLifecycleAdapter>,
    shell: Arc<ShellWorkspaceAdapter>,
    environments: Arc<EnvironmentRegistry>,
    tabs: DashMap<TabId, Arc<TabActor>>,
    pty_index: DashMap<PtyId, ExecutionOwner>,
    restore: tokio::sync::OnceCell<WorkspaceRestoreReport>,
}
pub struct ExecutionOwner {
    pub project_id: ProjectId,
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub session_id: SessionId,
    pub tab_session_id: TabSessionId,
    pub generation: i64,
}
pub struct TabActor { /* mailbox limitada, runtime exclusivo, operações serializadas */ }
pub struct WorkspaceSessionProvider { /* mapa preparado de owners e sessões persistidas */ }
```

Locks/filas: barreira de boot → `ProjectActivityGuard` da 06 → actor por aba → transação curta no writer.
Remoção usa gate exclusivo da 06; seu hook não readquire a lease, evitando deadlock. Não criar segundo gate.
Não manter transação SQLite enquanto aguarda PTY, evento, diálogo ou E/S. Actor não espera callback que
precise da própria mailbox: callbacks de saída completam promises separadas; eventos carregam geração.
Cancelamento de `invoke` abandona somente a resposta; tarefas e guards de compensação pertencem ao manager.

Adaptador de shell:

- `SessionProvider::prepare` valida sessão já commitada, aba e painel; retorna o `SessionId` preparado, sem
  duplicar sessão. `close` fecha a sessão do painel após drain; não arquiva o agrupamento inteiro da aba.
- `prepare(owner, request)` associa contexto **antes de iniciar a leitura do PTY**, usando o decorador de spawn
  da 05. Tap global recebe owner desde o primeiro byte; não há janela “spawn, depois attach do recorder”.
- `drain_execution(pty_id)` fecha blocos pendentes, aguarda worker e commits da 05, incluindo saída final.
  `recorder.finish_execution` drena texto e aguarda `BatchWriter::flush`. Só então pode fechar
  e arquivar cada sessão base e, por último, seu agrupamento.
- Cada sessão base possui um produtor por vez; sequência de blocos parte de `sessions.next_seq` da 05;
  não criar outro sequenciador compartilhado entre painéis. Scrollback usa `next_scrollback_seq` separado,
  acrescentado pela 07, e atualizado junto com o INSERT no writer da 03.
- Prontidão correlacionada de prompt A/B da 05 é associada a `(PtyId,generation)`. Antes do spawn, o manager deve estar inscrito;
  prontidão fica retida para não perder prompt que chega antes da resposta do spawn.

`EnvironmentRegistry::resolve(SpawnContext) -> ResolvedSpawn` é a extensão da 13; `ShellResolver` entrega
`SpawnSpec` da 02, `ShellIntegrationRequest` e input inicial separado. `SpawnContext` inclui owner, snapshot,
cwd e `is_primary`. Agente sem resolvedor falha antes de criar PTY; não há dependência do gateway de IA.

### 4.1. Abrir aba

1. Aguardar boot, validar pedido, projeto/template/ambiente/cwd. Reservar `request_id` no ledger de operações.
2. Transação: criar aba, folha, agrupamento `tab_sessions` ativo e snapshot. Criar sessão base por
   `SessionsRepo::prepare` da 05, associá-la ao painel/grupo e preparar owner/contexto antes do spawn. Ledger cobre
   falha entre essas etapas; nenhuma variável resolvida sensível chega ao disco.
3. Spawn no worker com guard: até a publicação, qualquer falha encerra o PTY criado e drena workers.
4. Publicar `TabView` e evento após atualizar runtime. Falha sem PTY cria estado de erro recuperável; falha
   após spawn só libera owner quando encerramento for confirmado. Erro de banco após spawn exige compensação.
5. No primeiro prompt confiável, para painel primário, reservar `initial_dispatch = claimed` em persistência
   antes de escrever comando + `\r`. Atualizar para `sent` após aceitação. Mesmo prompt repetido não reenvia.

Sem prompt confiável após 5 s: estado `waiting` e ação “Executar comando inicial” explícita, validando a
mesma geração; timeout não é prova de que o shell está pronto. Erro de escrita com aceitação ambígua conserva
`claimed` e informa usuário; retry automático é proibido. A garantia é **no máximo uma tentativa automática
por lançamento**, não “exactly once” de efeitos externos através de crash.

### 4.2. Split e reinício de painel

Split valida limite e revisão, herda cwd confirmado pela 05 ou `/proc` da 02 (fallback ao último cwd válido),
cria folha e sessão base vinculada ao agrupamento atual e lança novo PTY sem comando inicial. Após spawn
publica nova árvore; falha compensa folha e processo. Layout antigo permanece utilizável.

`pane_restart` só aceita `Exited`/`Failed`, após confirmar limpeza de descendentes. Mantém grupo/painel,
fecha/arquiva sessão base anterior, cria novo `SessionId`, incrementa geração e troca `PtyId`; não repete
comando inicial. Ação de fallback de pasta é argumento explícito.
Eventos/frames antigos são ignorados pelo par PTY/geração, inclusive updates de status da 05.

### 4.3. Fechar e iniciar nova sessão

1. Actor marca `Closing`/`Restarting`, cancela dispatch pendente e captura os owners da geração corrente.
2. Encerra PTYs em paralelo via `kill_and_wait(Graceful)` fora da thread assíncrona. Aguarda também supervisão
   dos descendentes; orçamento e escalada vêm da 02. Timeout pode tentar `Force`, mas falha final mantém
   ownership, mostra `Blocked` e **não** conclui fechamento nem cria substitutos.
3. Drena captura/commits da 05 e recorder. Fecha via repo05 e arquiva pela extensão07; falha mantém grupo recuperável,
   mesmo sem PTYs. Pode repetir flush/close/archive idempotentes sem repetir comandos ou abrir novos processos.
4. Fechamento: transação arquiva agrupamento, marca aba/painéis fechados. Fechar painel intermediário fecha
   somente sua sessão base e reduz árvore; agrupamento continua ativo.
5. Nova sessão: após todas as sessões base encerradas/arquivadas, uma transação arquiva agrupamento anterior
   e cria sucessor `seq+1` com recibo da operação. Preparar sessões base novas e vínculos antes dos spawns.
   A base e o agrupamento usam saga registrada, não promessa de transação única entre APIs assíncronas da 05.
   Falha intermediária deixa estado recuperável sem duas gerações de PTY; retomar não cria segundo grupo.
6. Nova sessão lança PTYs novos com gerações novas. Cada falha fica em `PaneView.error` sem impedir outros
   painéis. Emite snapshots; reset da 05 ocorre antes de eventos da nova execução, nunca depois de um comando
   já começar. Comando inicial volta a ser elegível apenas no painel primário.

`SessionEndReason::AppExit` no shutdown. A 09 decide quando sair; chama `WorkspaceManager::shutdown` antes
de fechar banco/runtime. `ProjectRemovalHook` usa o mesmo fluxo com `ProjectRemoved`; DELETE só ocorre se
todas as abas estiverem encerradas. Nenhum fallback “registrar erro e prosseguir” pode abandonar processos.

## 5. Persistência SQLite: migrations exclusivamente aditivas

Usar `DbSetup`/runner da 03, `foreign_keys=ON`, WAL, backup pré-migration e número global `NNNN` atribuído na
integração. Não desligar FKs dentro de transação nem reescrever migrations já aplicadas.

### 5.1. Templates e padrão

```sql
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
```

`TemplateRepo::ensure_default_tx(tx, project_id)` é o primitivo de todas as entradas:

- Padrão existente: retornar sem alteração; sem padrão e com templates: eleger menor `(position,created_at,id)`;
  sem templates: criar “Terminal”, ícone `mdi:console`, shell, cwd/comando nulos, env vazio, flags falsas.
- `DefaultTemplateContributor::after_insert(tx, project)` chama esse primitivo na transação da 06;
  implementação e registro permanecem internos a `mt-db`, antes de aceitar comandos/watchers.
  Importação mantém atomicidade; `SqliteProjectStore` consome o resultado pela fachada tipada da 06.
  Não usar evento pós-commit nem uma segunda `Db::write` como garantia do padrão.
- Code migration faz backfill dos projetos existentes; startup verifica/repara antes de expor workspace.
  Ids são Uuid v7 gerados no Rust. Projeto novo não fica visível sem template, mesmo se o app cair.
- `set_default`: validar alvo; `UPDATE ... SET is_default=0 WHERE project_id=? AND is_default=1`;
  depois `UPDATE ... SET is_default=1 WHERE id=? AND project_id=?`, verificar uma linha, commit.
  Não usar UPDATE único com expressão `(id=?)`: unicidade pode ser verificada linha a linha.
- `update(make_default=false)` mantém um padrão existente; exclusão do padrão é recusada com
  `template.default_required`. Todas as mutações fazem verificação final de exatamente um padrão no projeto.

### 5.2. Abas e folhas

```sql
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
```

Árvore/active/primary são validados pelo repositório na mesma transação que altera `panes`;
não há FK circular de `tabs` para uma folha ainda não criada. Correspondência template→projeto também é
validada transacionalmente. `PtyId`/PIDs são runtime, nunca restaurados destas tabelas.

### 5.3. Sessões: extensão por vínculo, não tabela concorrente

`BaseSessionAdapter` usa `SessionsRepo` da 05. Preservar todas as colunas existentes:
`id,tab_id,pane_id,project_id,pty_id,shell,cwd,status,started_at,ended_at,exit_code,exit_signal,next_seq`.
Uma linha corresponde a um painel/stream. `status` admite active/closed e passa a admitir archived no repo07,
conforme extensão prevista pela 05, sem alterar CHECK inexistente. `cwd` histórico é sanitizado; pasta
operacional e ambiente para restauração vêm do snapshot validado da aba/painel. `pty_id` antigo não é handle.

```sql
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
```

`TabSessionRepo::{start_tx,archive_tx}` altera agrupamentos; `tabs.last_session_seq` evita reutilizar seq
após retenção. `SessionsRepo::prepare/close/abort` da 05 controla ciclo base; `archive_closed_tx` da 07
exige status closed, `ended_at` preenchido, nenhum bloco running e flush confirmado; grava archived_at/status
archived preservando exit_code/signal e ids. Operação repetida é idempotente, nunca refecha sessão archived.
Cada sessão vinculada deve ter tab/pane/project corretos, e cada painel pertencer à aba do agrupamento.
Campo `closed_at` do vínculo só é preenchido após close confirmado da base; consulta repara vínculo atrasado
em recuperação. Novo vínculo ativo não substitui geração ainda viva.

Backfill de histórico: somente quando `tab_id` já possui identidade/projeto conhecidos, associar sessão
sem alterar seu id, `command_blocks`, chunks ou texto. Criar registros fechados de aba/painel quando o vínculo ao
projeto for comprovável; caso não seja, conservar a sessão base intacta e reportar contagem de não vinculadas.
Sem evidência de agrupamento antigo, cada sessão base vira um grupo histórico separado, ordenado por
`(started_at,id)`; não inferir simultaneidade nem fundir ids. `PRAGMA foreign_key_check` deve ficar vazio.
Base ausente/incompatível
retorna erro de migration; não existe “cenário A criar sessions” nesta fase.

Remover projeto: após fechamento confirmado, excluir sessões base vinculadas (cascade de blocos/FTS e
scrollback), vínculos/grupos, abas/painéis e projeto na transação de remoção. Histórico de projetos
removidos é apagado por essa ação; pasta no disco permanece. Sessões legadas sem associação comprovada
ficam intactas. Retenção é política separada e só alcança sessões arquivadas; `RESTRICT` impede apagar
painel antes de liberar seus vínculos. Nunca colocar NULL em tab_id/pane_id obrigatórios da base05.

### 5.4. Scrollback/FTS e operações

```sql
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

`FtsSpec { content_table: "session_scrollback", fts_table: "session_scrollback_fts", columns: &["text"],
tokenizer: FtsTokenizer::Text, prefix: &[2,3] }` da 03 gera DDL de índice externo/triggers em migration07.
Registrar para manutenção sem criar duas vezes. `command_blocks_fts` continua o da 05, sem redefinição.
Busca restringe SessionId pelo JOIN com `tab_session_panes`/`tab_sessions` antes de LIMIT; query parametrizada
via `build_match_query`. Paginar chunks por `(session_id,seq)`, no máximo 200 itens/1 MiB por resposta;
snippet é texto, nunca `v-html`. `byte_len` é calculado do UTF-8 sanitizado no Rust.

Ledger não contém comando/env bruto: hash de argumentos canônicos validados e resultado sanitizado com ids.
Operações duplicadas em andamento compartilham future; concluídas retornam o mesmo resultado/ids. Resultado
antigo usa sua revisão e não deve ressuscitar aba fechada. Registros interrompidos no boot seguinte nunca são
reexecutados silenciosamente; consulta do mesmo request retorna `workspace.operation_interrupted`.

### 5.5. Recorder e retenção

Tap somente copia para fila **limitada em bytes** e retorna; worker faz decode UTF-8 incremental, parser ANSI
incremental e sanitização. Um estado por execução/geração, contador e orçamento compartilhados por sessão.
Não confundir o `seq` recebido pelo tap com a sequência persistida dos chunks.

- Alt-screen `47/1047/1049` é omitido com marcador único por entrada; não reproduzir TUIs no histórico.
- Remover CSI/OSC/DCS e controles ativos; tratar CR de progresso sem acumular linhas ilimitadas.
- Não inserir newline artificial numa linha longa antes do sanitizador: isso pode partir o contexto de segredo.
  Acima do limite seguro (16 KiB de linha pendente), omitir a linha inteira até delimitador e marcar truncamento.
  PEM incompleto permanece suprimido; fronteiras de UTF-8/escape são preservadas.
- Cada chunk só aceita `SanitizedText`, ≤ 64 KiB; flush do batch em 200 ms/512 registros/1 MiB conforme 03.
  Seq é reservado por sessão base e `next_scrollback_seq` atualizado junto com INSERT. Restart abre outra sessão.
- Limite total padrão 8 MiB por sessão incluindo todos os painéis; marcador final reservado dentro desse
  orçamento do agrupamento `TabSessionId`. Fila de captura até 8 MiB global, batch até 8 MiB; backpressure
  não bloqueia leitor do PTY. A 05 persiste cabeça/cauda por comando; a 07 persiste stream histórico em chunks,
  um único produtor por execução. Nenhum desses caminhos recaptura bytes de replay da WebView.
- Overflow invalida a linha/token pendente e descarta até fronteira segura; não concatenar fragmentos de ambos
  os lados da lacuna. Sinalizar truncamento por estado fora da fila cheia e persistir no flush final.
- Flush espera EOF/worker e commits; eventos de saída não significam que todo texto já foi gravado.

Settings registradas: `workspace.sessions.keep_per_tab=50` (1–1000), `workspace.sessions.max_age_days=90`
(1–3650), `workspace.scrollback.limit_mib=8` (1–256). Registrar regra de idade de sessões arquivadas na
infra da 03; limite por quantidade de grupos usa job dedicado do repo07. Nunca tocar sessão ativa.
Registrar extensão de `SettingsSchema`; não inventar suporte de contagem no `RetentionRule`.
Após excluir sessões, apagar abas manuais fechadas sem histórico; conservar identidade automática enquanto
seu template existir. Rotina em lotes, com cascades/FTS testados.

## 6. IPC e eventos

Todos retornam `Result<T,MtError>`, tipos gerados, capabilities restritas à janela `main`. Args do Rust abaixo
aparecem camelCase no wrapper TS. Limites são validados no Rust; ids sempre verificados contra ownership.

| Comando | Argumentos | Retorno |
|---------|------------|---------|
| `template_list` | `project_id` | `Vec<TabTemplate>` |
| `template_get` | `template_id` | `TabTemplate` |
| `template_create` | `project_id, input` | `TabTemplate` |
| `template_update` | `template_id, expected_revision, input` | `TabTemplate` |
| `template_delete` | `template_id, expected_revision` | `()` |
| `template_set_default` | `template_id` | `Vec<TabTemplate>` |
| `tab_list` | `project_id` | `Vec<TabView>` abertas |
| `tab_open` | `request: TabOpenRequest` | `TabView` |
| `tab_close` | `tab_id, request_id` | `()` idempotente |
| `tab_reorder` | `project_id, ordered_tab_ids` | ordem confirmada completa |
| `tab_focus` | `tab_id, pane_id?` | `TabView` |
| `pane_split` | `pane_id, direction, expected_layout_revision, request_id` | `TabView` |
| `pane_close` | `pane_id, request_id` | `PaneCloseOutcome` (`pane_closed { tab }` ou `tab_closed { tab_id }`) |
| `pane_restart` | `pane_id, cwd_fallback_to_project, request_id` | `TabView` |
| `pane_layout_update` | `tab_id, expected_layout_revision, patches` | `TabView` |
| `session_new` | `tab_id, request_id` | `TabView` |
| `session_list` | `tab_id, cursor?, limit` (1–100) | `CursorPage<SessionSummary>` (agrupamentos) |
| `session_get` | `tab_session_id: TabSessionId` | `SessionDetail` |
| `session_scrollback_get` | `tab_session_id, session_id: SessionId, cursor?, limit` (1–200) | `CursorPage<ScrollbackChunk>`, teto 1 MiB |
| `session_search` | `tab_id, query` (1–256), `limit` (1–50) | `Vec<SessionSearchHit>` |
| `workspace_snapshot` | — | `WorkspaceSnapshot` |
| `workspace_initial_command_run` | `tab_id, pane_id, generation, request_id` | `TabView` |

`WorkspaceSnapshot { bootId, eventSeq, restored, tabs, restoreReport }`; relatório tem contagens de abertas,
recuperadas e falhas por template/aba. `workspace_snapshot` é somente leitura, nunca chama restore/spawn.
Cursor opaco de grupos contém `(tabId,seq)`; de chunks `(tabSessionId,sessionId,seq)`, validando escopo.
Histórico não depende de a aba estar aberta; `session_list` aceita identidade de aba fechada conhecida.

Eventos usam envelope `WorkspaceEvent<T> { bootId, eventSeq, payload }`; sequência monotônica por boot,
emitida após commit/runtime coerente, baixo volume. Saída de PTY permanece no Channel da 02/04.

| Evento | Payload |
|--------|---------|
| `tab://opened`, `tab://layout-changed` | `TabView` |
| `tab://closed` | `{ tabId, projectId, revision }` (tombstone) |
| `tab://reordered` | `{ projectId, orderedTabIds }` |
| `pane://pty-replaced` | `{ tabId, paneId, sessionId, generation, oldPtyId, newPtyId }` |
| `pane://exited` | `{ tabId, paneId, sessionId, generation, ptyId, exit }` |
| `session://started`, `session://archived` | `SessionSummary` |
| `template://changed` | `{ projectId, templateIds }` (invalidação, sem comando/env) |
| `workspace://restored` | `WorkspaceRestoreReport` |

Falha de spawn tem `newPtyId=null`, sinalizada pelo snapshot completo; não emitir substituição fictícia.
Observer interno recebe owner completo, revisão e spawn/exit/session-archive; callbacks fora do actor e em
fila ordenada por aba. Consumidor da 08/13 valida geração, evitando monitor reativado por callback atrasado.

Erros novos: `template.invalid`, `template.default_required`, `template.secret_literal`,
`template.cwd_not_found`, `template.conflict`, `workspace.environment_unavailable`, `workspace.request_conflict`,
`workspace.operation_interrupted`, `workspace.stop_failed`, `workspace.persistence_pending`, `tab.closing`,
`tab.reorder_mismatch`, `pane.limit_reached`, `pane.not_exited`, `layout.invalid`, `layout.conflict`,
`session.query_invalid`, mais `*.not_found` e erros 02/03. Mensagens pt-BR, detalhes sanitizados, sem env resolvida.

## 7. Boot, crash e restauração sem duplicação

1. Inicializar DB/migrations/backfill e supervisor 02. Finalizar a limpeza dos processos remanescentes antes
   de abrir novos; falha bloqueia somente owners afetados, reportada sem abandono da supervisão.
2. Instalar adapter de sessões, captura e taps antes de qualquer spawn. Recuperar operações interrompidas;
   fechar blocos pendentes da 05 como desconhecidos, sem inventar exit 0. Drenar commits.
3. A 05 fecha blocos pendentes como unknown/Recovery e sessões antigas como closed, sem exit fictício.
   A 07 arquiva essas sessões e encerra grupos anteriores com `crash_recovered`; não executar segundo
   recovery concorrente. `ended_at` é ≥ início e último timestamp persistido.
   Marcar abas anteriores fechadas; nenhum `PtyId` salvo é reutilizado.
4. `restore()` single-flight executa uma vez por boot, retendo relatório. Para cada template marcado, buscar
   sua única aba `origin=workspace` (índice único também cobre fechadas). Reutilizar `TabId`, folhas/layout,
   foco e ordem quando válidos; criar apenas quando não existe. `Project.sort_order` determina ordem de projetos.
5. Transação por aba cria novo grupo, atualiza snapshot e reabre folhas; preparar sessões base por painel
   com ledger antes de spawn. Cwd persistido ausente
   gera painel `Failed` com fallback explícito; árvore inválida é preservada para diagnóstico e substituída
   por folha nova, sem apagar histórico antigo. Restaurar até quatro abas simultâneas.
6. Comando inicial elegível somente no primário da nova execução. Fechar aba automática durante o boot não
   dispara reabertura; será aberta novamente no próximo boot se a flag continuar marcada.
7. Template desmarcado/deletado não restaura; edição da flag não abre aba durante a sessão corrente.
   Abertura manual do mesmo template cria aba manual independente e não ocupa a identidade automática.
8. Publicar relatório e `workspace://restored`. Frontend assina eventos **antes** de pedir snapshot, guarda
   buffer, aplica snapshot e só eventos posteriores ao `eventSeq`; lacuna/boot novo solicita ressincronização.

Reload da WebView apenas recebe snapshot e chama attach via 04. Restore repetido retorna o relatório anterior;
não percorre templates de novo. Garantia de não duplicar se aplica ao mesmo boot/intenção: reinício real do
aplicativo inicia uma nova execução automática, comportamento previsto pela flag de workspace.

## 8. Frontend Vue, stores e composables

### Stores e contratos TS

```ts
import type { PaneId, ProjectId, TabId, TabView, TabTemplateInput } from '@/types/bindings';

export interface TemplateEnvRow {
    key: string;
    kind: 'literal' | 'inherited';
    value: string;
    source: string;
}
export interface TemplateFormState {
    input: TabTemplateInput;
    envRows: TemplateEnvRow[];
    errors: Record<string, string>;
}
export interface WorkspaceSelection {
    projectId: ProjectId | null;
    tabId: TabId | null;
    paneId: PaneId | null;
}
export type TabBarIndicatorSlotProps = { tab: TabView };
```

| Store | Estado | Ações/garantias |
|-------|--------|----------------|
| `useTabsStore` | `tabsById`, `orderByProject`, `pending`, revisões/tombstones, `bootId/eventSeq` | loadSnapshot/open/close/reorder/focus/split/closePane/restartPane/updateLayout; upsert só com revisão mais nova; mesmo id de intenção em retry |
| `useTemplatesStore` | templates por projeto, loading/erro/revisão | list/get/create/update/delete/setDefault; evento invalida cache; leitura via store; request atrasado não sobrescreve revisão nova |
| `useSessionsStore` | páginas por aba, detalhe/scrollback por sessão/painel, busca | loadPage/loadDetail/loadScrollback/startNew/search; LRU de três sessões e teto de bytes; resultados de busca descartados por token de pedido obsoleto |

Stores de domínio não persistem cópias em localStorage; SQLite é autoridade. Objetos xterm/Channel/Promise
não entram no estado serializável. Projeto/aba ativa vêm de `useProjectsStore.activeTabByProject`, sem segunda
fonte concorrente; `attachTab` também é feito na hidratação. `remove` remove status da 05, vínculos, seleção
e registry; escolhe vizinha ou `null`. Resposta atrasada com revisão menor que tombstone é ignorada.

Composables:

- `useWorkspaceEvents`: apenas no `AppLayout`, init idempotente, assinatura antes de snapshot, buffer/replay
  ordenado e ressync em lacuna; dispose dos listeners inclusive se sua Promise resolver após unmount.
- `useActiveTabSeen`: sincroniza seleção/foco com a 06, sem chamar `markSeen` diretamente; um único tracker
  aplica visibilidade/foco/dwell e cancela timer ao mudar aba/estado. Não duplicar foco imediato da 05.
- `usePaneLayout`: normaliza payload `resized` da versão instalada; debounce 300 ms, patches coalescidos por
  `nodeId`, envio serializado com revisão; conflito recarrega. Mudança de estrutura cancela patches antigos.
- `layoutTree`: leitura pura (`leaves`, `findPane`, `findNode`), sem criar ids/painéis no frontend.

### Componentes e lifecycle

| Componente | Contrato |
|------------|----------|
| `ProjectWorkspace` | **Mantém `project: Project \| null` da 06**; preenche slot com toolbar/abas ou estado vazio |
| `TabBar`/`TabBarItem` | tabs/activeTabId/templates; emits select, add, open-template, edit-template, close, reorder, new-session, show-history; slots `#pinned`, `#indicators="{ tab }"`, `#actions` |
| `TemplateLaunchPopover` | Lista padrão primeiro, demais por posição/nome; ações “Novo template” e editar |
| `TemplateEditorModal` | Campos de §3.1, seletor de cwd, pares env literal/herdada, erros por campo; “Padrão”; aviso de resolvedor indisponível sem impedir cadastro |
| `TabWorkspaceView` | tab/visible; renderiza árvore; emite foco de painel; não chama spawn |
| `PaneLayout` | node/tab; recursivo com keys estáveis `nodeId`/`paneId`; splitpanes horizontal iff direction=horizontal |
| `PaneView` | pane/tabId/active/visible; terminal quando houver PTY, overlay de saída/erro e ações split/close/restart/fallback |
| `SessionHistoryDrawer` | tabId/open; resumo paginado, seleção de sessão/painel, busca e truncamento |
| `SessionReadonlyViewer` | sessionId/paneId; comandos paginados e texto virtualizado; sem PTY, sem input, sem interpretar escapes/HTML |

SFCs seguem template → script setup TS → style SCSS scoped, quatro espaços, tags de abertura em uma linha,
aspas simples e `;`. Tokens semânticos `--max-*`/`--background-*`, textos pt-BR e MaxIcon para ícones.

`Splitpanes` `horizontal=true` empilha painéis; evento `resized` deve ser normalizado conforme versão fixada
(v3 array, v4 objeto com `panes`), sem cast cego. Usar adapter testado e a estrutura real do lockfile.
`Pane` recebe `size` e `min-size=5`; resize aciona `ResizeObserver`/fit da 04. Remontagem da árvore não pode
remontar o PTY. `v-memo` não deve congelar foco/estado: omitir ou incluir todas as dependências relevantes.

Ocultar aba chama detach/setVisible(false), preservando instância pelo registry da 04; limite de `KeepAlive`
não autoriza dispose de PTY. Ao trocar execução: disposar instância antiga, limpar `CommandBlockFeed`/input,
ignorar callbacks antigos, montar xterm limpo e attach ao novo `PtyId`. Manter uma única cadeia de ACK por PTY.

Teclado: `role=tablist/tab`, roving tabindex, setas/Home/End, Enter/Space, foco restaurado após fechar;
reordenação também por teclado. Divisores focáveis e rótulos “Dividir horizontalmente/verticalmente”.
Botões “Adicionar nova aba”, “Iniciar Aba Template”, “Iniciar nova Sessão”, “Histórico de Sessões”.
Durante operação pendente, desabilitar ações conflitantes e mostrar erro recuperável por painel/aba.

## 9. Verificação e pontos de extensão

### Matriz de testes de implementação

| Camada | Casos necessários |
|--------|-------------------|
| SQL real + migrations | infraestrutura03 + sessions/command_blocks05 preenchidos; nenhuma perda na migration; padrão de existentes/novos/importação; rollback; grupo ativo único; cascades explícitos ao remover projeto |
| Domínio | propriedades split/remove/resize; ids únicos, soma/tamanho/profundidade; patches obsoletos; posse cruzada de projeto/painel |
| PTY + shell | prompt antes da resposta; prompt duplicado; timeout sem injeção; split sem comando; sequências concorrentes; shutdown/setsid; erro ao encerrar bloqueia spawn e exclusão |
| Sessões | EOF e commits antes de arquivo; flush falha; DB falha após spawn; spawn parcial; eventos tardios; restart cria nova sessão base; duas session_new com mesmo request não duplicam |
| Restore | chamadas simultâneas, reload, crash em cada etapa do ledger, comando claimed, template editado/apagado, cwd ausente, árvore inválida e abas manuais |
| Recorder | todos os cortes de segredo/UTF-8/ANSI, PEM, linha >16 KiB, alt-screen, overflow entre partes de token, limite global multi-painel, lote cheio e EOF |
| Vue/stores | evento antes/depois de resposta, tombstone, snapshot com buffer, revisão antiga, seleção/badges, unmount durante listen, resized v3/v4, geração de xterm e histórico sem input |

Metas observadas: primeiro prompt típico <500 ms; restore de dez abas <2 s em ambiente de referência;
registrar distribuição e hardware. Segurança de processo e integridade prevalecem sobre prazo artificial.
Validar cargo fmt/clippy/test, bindings, tsgo, lint e suítes frontend afetadas.

| Consumidor | Contrato entregue |
|------------|-------------------|
| 08 | Slot de indicadores; observer com owner completo e geração; aba/painel/PTY/cwd consultáveis |
| 09 | `WorkspaceManager::shutdown`, consulta de abas/processos vivos e status da 05 |
| 11/12 | `tab_open`, snapshot e seleção por projeto; contexto da 04 e sessão/blocos da 05 |
| 13 | `EnvironmentResolver`, `TemplateEnvironment`, snapshot de lançamento e overrides de cwd/env |
| 14/15 | Aberturas com `request_id`, overrides e ownership; chamadas multi-abas não confundem identidade automática |
| 16 | Slot `#pinned`, abertura na pasta/worktree por override, histórico por aba |
| 17 | FTS sanitizado, paginação e métricas/limites de captura |

Referência de biblioteca: tentativa de Context7 bloqueada por cota; documentação oficial alternativa:
<https://github.com/antoniandre/splitpanes> (orientação, nesting e mudança de eventos na v4).
Confirmar versão instalada antes da implementação; a documentação desta fase não impõe atualização da biblioteca.
