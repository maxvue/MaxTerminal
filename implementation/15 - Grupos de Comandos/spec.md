# Fase 15 — Grupos de Comandos (`>>`) — Especificação técnica

## 1. Arquitetura e limites

Referências: [instruções](./instructions.md), [contratos comuns](../README.md), AGENTS.md §8.2 e §5.1.
Dependências diretas: **07, 12, 13 e 14**; PTYs (02), persistência (03), input (04), shell integration (05) e
projetos (06) são infraestrutura anterior. A Fase 13 é pré-requisito de persistência: `0008_13_agents_skills_memory.sql`
precede obrigatoriamente `0009_15_command_groups.sql` (D-01). O serviço de memória permanece opcional, sem acoplamento
obrigatório para CRUD/execução/IA. Nenhuma API das Fases 16/17 é necessária.

Fluxo: `>> → store → IPC → CommandGroupService → snapshot SQLite → runner → PTY/WorkspaceManager`.
Eventos pequenos atualizam a store; saída usa Channel limitado. O backend mantém a execução mesmo se o painel Vue
for desmontado. SQLite é a fonte das definições; não há execução/importação automática de arquivos JSON.

`mt-commands::groups` contém domínio, validação, repositório e orquestração testável. As implementações das portas
que acessam serviços do aplicativo ficam em `src-tauri/src/command_groups/adapters.rs`, evitando dependência do
crate de domínio no crate binário. Não criar uma segunda infraestrutura de supervisão ou um segundo cliente IA.

## 2. Contratos consumidos e adaptações

| Origem | Contrato efetivamente usado | Adaptação desta fase |
|---|---|---|
| 02 | `SpawnSpec`, `PtyOwner { tab_id: Option<TabId>, pane_id: Option<PaneId> }`, `PtyManager::{spawn,write,kill,subscribe,add_output_tap}`, `KillMode`, `ExitInfo` | `GroupPtyPort`: headless usa ambos os campos de owner `None`; índice local associa run/item ao `PtyId`. Não existe `PtyOwner::CommandGroupRun`. |
| 03 | Writer/migrations WAL, `sanitize`, `StreamSanitizer`, retenção | Repositório desta fase e sink sanitizado antes de qualquer gravação. |
| 04 | `TriggerRegistry`, provider/context/item, popup, input rico, terminal read-only | Provider `>>` e projeção de ações fixas; nomes finais seguem a fase dona. |
| 05 | PromptStart, início/fim de bloco, exit code, shell do painel, `TabStatus` | `GroupShellPort` correlaciona `(PtyId, SessionId, CommandBlockId, geração)`; status visual não prova prompt disponível. |
| 06 | Projeto e raiz resolvida, remoção de projeto | Validação de escopo/cwd e hook que encerra recursos antes do cascade. |
| 07 | `TabOpenRequest`, `TabOpenOverrides`, `WorkspaceManager::{open_tab,close_tab,restart_pane}`, templates, sessões, observadores | `GroupTabPort`; abertura interna pelo mesmo serviço usado por `tab_open`, sem frontend como coordenador. |
| 12 | `StructuredRequest/Response`, `AiService::generate_structured`, registro de requisições/cancelamento | `GroupAiPort`; schema `command-group.suggestion.v1`; adaptador usa exclusivamente o método público do serviço. |
| 13 | Migration `0008_13_agents_skills_memory.sql` obrigatória; `CommandMemory::suggestions(&query, now_ms)` opcional | Contiguidade D-01; `RecurringCommandsSource` só é registrado se a fonte estiver habilitada. |
| 14 | Crate `mt-commands`, provider Artisan e utilitário fuzzy quando exportado | Registrar `>>` com precedência; não alterar parsing Artisan nem duplicar catálogo. |

Portas `GroupPtyPort`, `GroupShellPort`, `GroupTabPort`, `GroupAiPort` e `RecurringCommandsSource` são interfaces
**novas desta fase**, não APIs alegadamente existentes nas fases anteriores. Seus testes de contrato exercitam
os serviços reais. Falta de um evento confiável de prompt/fim deve produzir `tracking_unavailable`, nunca sucesso presumido.

Lacuna conhecida da Fase 07: `TabOpenOverrides` tem `title`, `cwd`, `initial_command`, `extra_env` e
`cwd_fallback_to_project`, mas não `continuous` nem troca de ambiente. Esta fase seleciona um **template shell
existente** do projeto cujo `continuous` coincida com o item; usa o padrão quando compatível, senão um template
explicitamente escolhido pelo usuário no lançamento. Sem compatível, abre o editor de templates da Fase 07 e
recusa o lançamento inteiro antes de criar recursos. Não inventar campos IPC nem alterar silenciosamente o padrão.
Grupos globais armazenam intenção `continuous`, não um TemplateId pertencente a outro projeto.

## 3. Domínio, módulos e validação

### 3.1. Organização Rust

| Módulo em `mt-commands/src/groups/` | Responsabilidade |
|---|---|
| `model.rs` | DTOs serializáveis, ids, enums e snapshots |
| `validate.rs`, `resolve.rs` | Validação de rascunho, escopo, env, resolução de cwd/contexto |
| `repo.rs` | `CommandGroupRepo`, `SqliteCommandGroupRepo`, revisão otimista e transações |
| `compose.rs`, `sequential.rs` | Quoting por shell, preparo isolado de item e fila no mesmo PTY |
| `runner.rs` | `ParallelRunner`, semáforo global, `RunHandle`, `GroupRunSink` |
| `plan.rs`, `multi_tab.rs` | `MultiTabPlanner`, preflight e abertura parcial rastreável |
| `aggregate.rs`, `events.rs` | Redutor de estados, contadores e payloads versionados |
| `suggest.rs`, `ports.rs`, `service.rs` | IA/memória opcional, interfaces e `CommandGroupService` |

### 3.2. Tipos

Contratos abaixo são assinaturas de projeto, não implementações prontas. Structs IPC usam serde camelCase,
enums snake_case e tipos gerados em `bindings.ts`. Ids novos locais (`GroupItemId`, `GroupRunId`, `GroupRunItemId`,
`GroupRequestId`) são newtypes UUID v7; reutilizar `CommandGroupId` de `mt-core`.

```rust
pub enum GroupScope { Local, Global }
pub enum ExecutionMode { Sequential, Parallel, MultiTabs }
pub enum SequentialSeparator { And, Semicolon }
pub enum ShellDialect { Bash, Zsh, Fish }
pub enum ItemStatus { Pending, Starting, Running, Cancelling, Succeeded, Failed, Skipped, Cancelled, Interrupted }
pub enum RunStatus { Queued, Running, Cancelling, Succeeded, Failed, Partial, Cancelled, Interrupted }

pub struct CommandGroupItemInput {
    pub id: Option<GroupItemId>,
    pub title: String,
    pub command: String,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub continuous: bool,
}
pub struct CommandGroupInput {
    pub scope: GroupScope,
    pub project_id: Option<ProjectId>,
    pub name: String,
    pub description: String,
    pub labels: Vec<String>,
    pub mode: ExecutionMode,
    pub separator: SequentialSeparator,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub items: Vec<CommandGroupItemInput>,
}
pub struct GroupRunRequest {
    pub request_id: GroupRequestId,
    pub group_id: CommandGroupId,
    pub expected_revision: u64,
    pub project_id: ProjectId,
    pub origin_pane_id: Option<PaneId>,
    pub mode_override: Option<ExecutionMode>,
    pub template_by_item: BTreeMap<GroupItemId, TemplateId>,
}
pub struct RunContext {
    pub project_id: ProjectId,
    pub root: PathBuf,
    pub origin: Option<(TabId, PaneId, PtyId, SessionId)>,
    pub shell: PathBuf,
    pub dialect: ShellDialect,
    pub generation: u64,
}
```

`CommandGroup` acrescenta `id`, `revision`, `sort_order`, timestamps e ids/posições definitivos dos itens.
`GroupRun` contém id/requestId, snapshot sanitizado do grupo/revisão, contexto, modo, estado, contadores,
`cancel_requested_at`, `started_at`, `finished_at`, `version` e `last_exit_code`. `GroupRunItem` contém snapshot
do comando/título/cwd/env, posição, estado, timestamps, `exit_code`, `signal`, `error_code`, vínculos opcionais
`pty_id/tab_id/pane_id/session_id/command_block_id`, `version` e indicador de saída truncada. DTO de detalhe
inclui itens; listagem é resumo paginado sem saída. Nenhum segredo ou ambiente completo herdado vai nos eventos.

Sequencial usa o shell do painel; background usa esse shell quando há origem, ou o resolvedor shell do template
padrão compatível quando não há painel. Multiabas resolve shell por template selecionado e guarda esse contexto
por item. Exibir o shell efetivo no lançamento: salvar grupo não torna seus comandos portáveis entre dialetos.

### 3.3. Regras

- Nome aparado, 1–80 caracteres; descrição ≤ 2048; 1–50 itens; título 1–80; até 20 labels de 1–32 caracteres.
  `name_key` e chaves de labels usam normalização Unicode NFC + lowercase determinística no Rust, não `NOCASE`
  do SQLite (que não resolve Unicode). Unicidade por escopo/projeto; desempate por posição e id.
- Comando de 1–8192 bytes, não vazio, sem CR/LF/NUL. É código do shell escolhido pelo usuário: não fazer split
  por espaços, regex de operadores, tradução fish→POSIX ou expansão de valores de env no frontend.
- Env: até 64 entradas por nível e 64 KiB mesclados; chaves `[A-Za-z_][A-Za-z0-9_]*`, sem `MAXTERMINAL_*`;
  valores sem NUL. Precedência: herdado/template → grupo → item; identidades internas vêm por último do backend.
  Valores são literais: `$HOME` em um valor env não é expandido pelo aplicativo.
- `cwd` de grupo/item é relativo à raiz efetiva do projeto (worktree incluída), nunca relativo ao campo do outro
  nível. Item > grupo > padrão do modo. Canonicalizar, verificar diretório e confinamento após symlinks a cada
  lançamento; rejeitar caminhos absolutos, escape e inexistência. Global resolve contra o projeto do lançamento.
- Sequencial sem overrides de cwd/env mantém estado do shell ativo, incluindo `cd`/exports de itens anteriores.
  Com override, o item roda em shell filho isolado e não exporta alterações ao pai. Background/multiabas sempre
  resolvem cwd inicial pela raiz quando omitido. UI descreve essa diferença no editor.
- Sanitizar todo texto persistido. Se sanitizer alterar comando ou valor env executável, rejeitar salvamento com
  `secret_detected` e caminho do campo, sem ecoar o valor: não salvar comando mascarado que seria executado errado.
  Títulos, descrição, labels e erros também passam pelo sanitizer. Referências `$TOKEN` permanecem referências;
  tokens herdados ficam somente no ambiente do processo. Nenhum detector garante identificar todo segredo.
- Update exige revisão e ids pertencentes ao grupo; ordem é a ordem do array. Escopo/projeto do grupo são imutáveis
  após criação; mudança de escopo é cópia explícita pelo editor. Excluir definição não cancela execuções existentes.

## 4. Execução, propriedade e estados

### 4.1. Aceitação comum e sequencial (`&&` / `;`)

1. Serializar aceitação por `request_id`; conferir hash canônico do pedido. Mesmo id + mesmo pedido devolve run
   existente (inclusive terminal); mesmo id + pedido diferente retorna `request_conflict`. Consultar isso antes
   de conferir a revisão atual do grupo, permitindo repetir resposta perdida após edição/exclusão.
2. Carregar grupo/revisão, validar escopo e projeto do painel. Resolver shell, cwd, templates e capacidade de
   observação. Persistir snapshot + todos os itens `pending` em uma transação antes de criar recursos.
   Erro de preflight retorna `MtError` sem execução aceita; após aceite, falhas são itens do run consultável.
3. Sequencial adquire lease exclusivo `(pane, pty, session, generation)` com validação backend de prompt pronto,
   sem alt-screen/comando em foreground. Não basta `TabStatus == idle`. Input rico e demais lançadores respeitam
   essa reserva; a UI permanece observadora, não envia o próximo item. Se o contrato de escrita existente não
   permitir reserva coordenada, o adaptador deve implementá-la no ponto comum de submissão da Fase 04/05 antes
   de habilitar o modo; uma checagem somente visual é insuficiente.
4. `SequentialComposer` prepara **um item** para o dialeto real e escreve uma única submissão. Sem overrides,
   usa o texto original e terminador do PTY, preservando funções/builtins/estado do shell. Com overrides, prepara
   `env -C <cwd> <K=V...> <shell-absoluto> -c <comando>` com cada argumento quoted para o shell externo.
   POSIX: apóstrofo interno é representado fechando/reabrindo aspas; fish: escape próprio de apóstrofo e backslash
   dentro de aspas simples. Comando é um único argumento literal, não concatenado com dados de cwd/env.
5. Vincular início/fim de bloco ao item enviado e aguardar também o próximo prompt antes de avançar. `&&` pula
   os restantes após primeira falha; `;` continua mesmo após exit não zero. O último exit é preservado como
   `last_exit_code`, mas falhas anteriores continuam nos contadores/agregado. Cada item gera bloco próprio.
6. Shell `exit`, substituição de PTY, sessão nova, fechamento ou perda de tracking interrompem a fila; não escrever
   na nova geração. Sem hooks confiáveis, não usar temporizador para presumir conclusão. Um comando contínuo
   mantém os próximos pendentes até terminar/cancelar; o editor avisa quando ele não é o último.

Não usar `commands.join(' && ')`: além dos dialetos, comentários, pipelines e `exit` mudam sua semântica. O rótulo
`&&` representa parar após falha; `;`, continuar. O mesmo PTY é mantido em ambos. Shell desconhecido retorna
`unsupported_shell`. `env -C` ausente bloqueia somente o caminho que precisa dele; não simular isolamento fish
com `begin; cd ...; ...; cd -; end`, que altera o estado do pai e falha sob saída precoce.

**Limitação de observabilidade (T-18):** sem OSC 133 confiável, uma linha `cmd1 ; cmd2` só revela o exit do último comando; os resultados intermediários são `unknown`, nunca sucesso presumido. Esta fase recusa a fila sequencial rastreada com `commands.group.tracking_unavailable` e oferece Multi-Abas; não implementa fallback concatenado que marque todos os itens como `succeeded`. Se a cadeia for digitada manualmente em um único item, apenas o resultado da linha completa é conhecido, sem fabricar resultados internos.

### 4.2. Paralelo em background

`ParallelRunner` mantém fila FIFO e semáforo **global** de 16 PTYs de grupos (não 16 por execução). Ao obter vaga,
revalida cancelamento/cwd, persiste `starting`, chama `spawn` com `args = ["-c", command]`, `login = false`, shell
absoluto, cwd/env resolvidos e sink já instalado. Não acrescentar `&`, `nohup` ou `disown`; cada tarefa possui PTY
supervisionado e índice `(run,item) → PtyId`. Registro/observação devem cobrir processo que termine antes do retorno
de spawn: reconciliar snapshot do supervisor e bufferizar eventos sem vínculo temporariamente.

Falha de spawn vira `failed` com código próprio e sem exit code inventado; libera vaga e continua os demais.
Falha de um item não cancela irmãos automaticamente. Sucesso exige exit zero e conclusão da limpeza da árvore
supervisionada; descendentes deixados por daemonização são encerrados pela Fase 02 antes de liberar a vaga.
`continuous` é classificação visual, nunca sucesso prematuro. Tarefas interativas não recebem entrada no modo
headless: UI indica "Use Multi-Abas para interação" e permite cancelar, sem responder prompts automaticamente.

### 4.3. Multi-Abas e falha parcial

Preflight resolve todos os templates shell compatíveis (§2). Cada item vira `TabOpenRequest` real, com projeto,
template escolhido, `focus = false` salvo primeira aba, e overrides de título, cwd canônico, comando inicial,
extra_env mesclado e `cwd_fallback_to_project = false`. Não herdar comando inicial antigo do template.

`MultiTabRunner` abre **uma aba por item**, em ordem, aguardando cada `open_tab`. Antes de chamar, grava `starting`;
após retorno, grava ids de aba/painel/sessão e relaciona o primeiro comando inicial. O adaptador observa eventos
desde antes da abertura, reconciliando eventos que chegaram antes do retorno. A Fase 07 é a única responsável por
enviar o comando inicial uma vez; esta fase nunca escreve uma segunda cópia pelo frontend.

Abertura aceita não significa comando concluído. Acompanhar seu bloco/exit; se não houver tracking confiável,
registrar erro de acompanhamento e interromper, sem marcar sucesso só porque a aba existe. Shell permanece
disponível após o comando: comandos posteriores manuais não pertencem ao grupo. `continuous` vem do template.
Falha em uma abertura marca esse item `failed`, continua tentando os outros e mantém abas já abertas. Recursos
parciais de uma abertura que falhou são limpos pelo adaptador/Fase 07 antes de retornar erro; nunca apagar uma aba
bem-sucedida para simular rollback global. Histórico mostra ids das abas abertas e motivo das não abertas.

Cancelamento interrompe novas aberturas. Se uma abertura em andamento retornar depois do pedido, vincular e
encerrar seu recurso antes de finalizar o item. Nunca repetir automaticamente `open_tab` após resposta ambígua.
Crash entre criação e persistência deve reconciliar com o workspace; quando a correlação não for comprovável,
marcar interrompido, não relançar nem fechar abas não identificadas como pertencentes ao run.

### 4.4. Redutor, cancelamento e recuperação

Transições usuais: `pending → starting → running → succeeded|failed`; `pending → skipped` somente por dependência
sequencial falha; cancelamento de pendente → `cancelled`; ativo → `cancelling → cancelled` após confirmação de
encerramento. Saída já finalizada antes do cancelamento conserva `succeeded/failed`; transições terminais são
imutáveis. `interrupted` representa perda de supervisão/tracking/crash, não exit não zero.

Agregado determinístico, calculado na transação da mudança:

| Condição | RunStatus |
|---|---|
| Cancelamento em curso e recurso não terminal | `cancelling` |
| Há algum recurso/abertura ativo ou pendente após início | `running` (antes de iniciar: `queued`) |
| Todos terminais e algum `interrupted` | `interrupted` |
| Todos terminais e algum `cancelled` | `cancelled` |
| Todos terminais, há sucesso e falha | `partial` |
| Todos terminais, há falha e nenhum sucesso | `failed` |
| Todos terminais, todos bem-sucedidos | `succeeded` |

`skipped` não conta como sucesso; só existe após falha, logo não há run composto exclusivamente por skipped.
Contadores expõem todos os estados mesmo se a prioridade agregada for cancelamento/interrupção; não ocultar
falhas anteriores. `finished_at` somente após todos terminais e recursos encerrados ou explicitamente declarados
interrompidos. Não sobrecarregar `TabStatus` com os estados de grupo.

`cancel(run, item?)` é idempotente. Background usa `PtyManager::kill(Graceful)` e aguarda confirmação/limpeza.
Sequencial cancela a fila e interrompe somente o comando de sua geração no painel (Ctrl+C pelo caminho de input
do PTY); aguarda fim/prompt. Sem resposta, mantém `cancelling` e oferece "Encerrar e reiniciar painel" usando
`WorkspaceManager::restart_pane`, que aplica a supervisão da Fase 02; não reportar cancelamento concluído cedo.
Multiabas encerra via `close_tab` somente abas ainda executando o comando inicial do grupo e com identidade
inalterada. Abas concluídas ou reutilizadas não são fechadas. Cancelar item sequencial ativo encerra sua fila
(restantes cancelados); cancelar um item ainda pendente apenas o remove, preservando a política dos restantes.

No boot: executar recuperação de processos da Fase 02 antes da reconciliação de runs; preservar itens terminais,
marcar restantes `interrupted`, recomputar agregado, nunca reiniciar comandos. Não alegar que SIGKILL do app elimina
instantaneamente qualquer processo: a limpeza de órfãos é responsabilidade do supervisor/recuperação existente.

## 5. Persistência SQL e índices

Migration `0009_15_command_groups.sql` (versão 9 da sequência contígua D-01). Abaixo, DDL base; timestamps são epoch ms,
ids TEXT UUID. `snapshot_json` e `env_json` são armazenamento de DTOs validados, **não protocolo de execução**.
JSON é desserializado estritamente com limites antes de uso. Todas as consultas usam parâmetros vinculados.

```sql
CREATE TABLE command_groups (
    id TEXT PRIMARY KEY,
    scope TEXT NOT NULL CHECK(scope IN ('local','global')),
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 80),
    name_key TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    mode TEXT NOT NULL CHECK(mode IN ('sequential','parallel','multi_tabs')),
    separator TEXT NOT NULL CHECK(separator IN ('and','semicolon')),
    cwd TEXT,
    env_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(env_json)),
    sort_order INTEGER NOT NULL CHECK(sort_order >= 0),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision > 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK((scope = 'local' AND project_id IS NOT NULL) OR (scope = 'global' AND project_id IS NULL))
);
CREATE UNIQUE INDEX ux_groups_local_name ON command_groups(project_id,name_key) WHERE scope='local';
CREATE UNIQUE INDEX ux_groups_global_name ON command_groups(name_key) WHERE scope='global';
CREATE INDEX ix_groups_order ON command_groups(scope,project_id,sort_order,id);

CREATE TABLE command_group_labels (
    group_id TEXT NOT NULL REFERENCES command_groups(id) ON DELETE CASCADE,
    label_key TEXT NOT NULL,
    label TEXT NOT NULL,
    PRIMARY KEY(group_id,label_key)
);
CREATE INDEX ix_group_labels_lookup ON command_group_labels(label_key,group_id);

CREATE TABLE command_group_items (
    id TEXT PRIMARY KEY,
    group_id TEXT NOT NULL REFERENCES command_groups(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position >= 0),
    title TEXT NOT NULL,
    command TEXT NOT NULL CHECK(length(command) > 0),
    cwd TEXT,
    env_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(env_json)),
    continuous INTEGER NOT NULL DEFAULT 0 CHECK(continuous IN (0,1)),
    UNIQUE(group_id,position)
);

CREATE TABLE command_group_runs (
    id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL UNIQUE,
    request_hash TEXT NOT NULL,
    group_id TEXT REFERENCES command_groups(id) ON DELETE SET NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    origin_session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    snapshot_json TEXT NOT NULL CHECK(json_valid(snapshot_json)),
    mode TEXT NOT NULL CHECK(mode IN ('sequential','parallel','multi_tabs')),
    status TEXT NOT NULL CHECK(status IN ('queued','running','cancelling','succeeded','failed','partial','cancelled','interrupted')),
    version INTEGER NOT NULL DEFAULT 1 CHECK(version > 0),
    last_exit_code INTEGER,
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    cancel_requested_at INTEGER,
    finished_at INTEGER
);
CREATE INDEX ix_group_runs_project_time ON command_group_runs(project_id,created_at DESC,id);
CREATE INDEX ix_group_runs_group_time ON command_group_runs(group_id,created_at DESC,id);
CREATE INDEX ix_group_runs_origin ON command_group_runs(origin_session_id);
CREATE INDEX ix_group_runs_active ON command_group_runs(status) WHERE finished_at IS NULL;

CREATE TABLE command_group_run_items (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES command_group_runs(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    snapshot_json TEXT NOT NULL CHECK(json_valid(snapshot_json)),
    status TEXT NOT NULL CHECK(status IN ('pending','starting','running','cancelling','succeeded','failed','skipped','cancelled','interrupted')),
    version INTEGER NOT NULL DEFAULT 1 CHECK(version > 0),
    pty_id TEXT,
    tab_id TEXT REFERENCES tabs(id) ON DELETE SET NULL,
    pane_id TEXT REFERENCES panes(id) ON DELETE SET NULL,
    session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    command_block_id TEXT REFERENCES command_blocks(id) ON DELETE SET NULL,
    exit_code INTEGER,
    signal INTEGER,
    error_code TEXT,
    error_message TEXT,
    started_at INTEGER,
    finished_at INTEGER,
    output_tail TEXT NOT NULL DEFAULT '',
    output_truncated INTEGER NOT NULL DEFAULT 0 CHECK(output_truncated IN (0,1)),
    UNIQUE(run_id,position)
);
CREATE INDEX ix_group_run_items_pty ON command_group_run_items(pty_id);
CREATE INDEX ix_group_run_items_session ON command_group_run_items(session_id);
CREATE INDEX ix_group_run_items_tab ON command_group_run_items(tab_id);
CREATE INDEX ix_group_run_items_pane ON command_group_run_items(pane_id);
CREATE INDEX ix_group_run_items_block ON command_group_run_items(command_block_id);
```

`command_blocks` e demais FKs usam os nomes finais das migrations anteriores; conferir `foreign_key_check` em
banco incremental e limpo. `PtyId` não tem FK porque é recurso runtime. Snapshots históricos não têm FK para
itens da definição: edição/exclusão não destrói evidência. Contadores são derivados dos itens na mesma leitura
transacional do run, evitando duas fontes de verdade.

CRUD é transacional: validar tudo, conferir revisão, substituir itens/labels e incrementar revisão com CAS.
Falha em qualquer inserção desfaz tudo. Reordenação de grupos recebe escopo/projeto, lista completa de ids
sem repetição e revisões esperadas; lista incompleta, grupo estrangeiro ou conflito falha atomicamente.
Reordenação de itens usa substituição transacional para evitar colisões transitórias do índice de posição.
Transações não ficam abertas durante spawn, espera de prompt, chamada IA ou encerramento de processos.

Persistir transições antes de emitir eventos; falha de gravação após spawn exige manter supervisão, suspender
novos lançamentos e encerrar recursos recém-criados se não for possível registrar a propriedade. Nunca deixá-los
rodando sem handle. Remoção de projeto cancela/aguarda todos os seus runs antes do cascade, inclusive globais
executados naquele projeto. Retenção: histórico terminal 30 dias configurável, nunca runs ativos; remover filhos
via cascade. Busca FTS global fica para a Fase 17; fuzzy de grupos usa catálogo compacto, não FTS obrigatório.

## 6. IPC e erros

Todos retornam `Result<T, MtError>` e são registrados em `commands/command_groups.rs`, `specta` e permissões
do aplicativo (janela principal). DTOs com campos desconhecidos são recusados. Não aceitar comandos arbitrários
em `command_group_run`: somente id/revisão de uma definição salva pelo CRUD explícito.

| Comando | Entrada → saída |
|---|---|
| `command_group_list` | `project_id?` → locais do projeto + globais, revisões; sem projeto, somente globais |
| `command_group_get` | `group_id` → `CommandGroup` completo |
| `command_group_create` | `input: CommandGroupInput` → `CommandGroup` |
| `command_group_update` | `group_id, expected_revision, input` → `CommandGroup` |
| `command_group_delete` | `group_id, expected_revision` → recibo `{ id, revision }` |
| `command_group_reorder` | `scope, project_id?, entries[{id,revision}]` → catálogo revisado |
| `command_group_run` | `request: GroupRunRequest` → `GroupRun` aceito/existente, sem aguardar término |
| `command_group_run_cancel` | `run_id, item_id?` → snapshot atual, podendo estar `cancelling` |
| `command_group_run_list` | `project_id, cursor?, limit` (1–100), `active_only?` → página de resumos |
| `command_group_run_get` | `run_id` → detalhe + itens/versão/contadores |
| `command_group_run_item_attach` | `run_id, item_id, after_seq?, Channel<GroupOutputFrame>` → `subscription_id`, limites do replay |
| `command_group_run_item_detach` | `subscription_id` → `()` idempotente |
| `command_group_suggest` | `request_id: AiRequestId, project_id?, pane_id?, scope, goal, draft?, draft_revision` → rascunho validado + revisão de origem |
| `command_group_recurring_commands` | `project_id, limit` → `{ available, items }`, vazio se fonte opcional ausente |

Sugestão é cancelada pelo `ai_cancel` existente (Fase 12), não por kill de processos. `run_cancel` valida
pertencimento do item ao run. Projeto/painel/template são revalidados no backend contra o contexto real.

Prefixo `commands.group.*`: `validation`, `not_found`, `name_conflict`, `revision_conflict`, `reorder_mismatch`,
`project_required`, `scope_mismatch`, `cwd_outside_project`, `cwd_not_found`, `secret_detected`, `unsupported_shell`,
`env_cwd_unavailable`, `pane_busy`, `tracking_unavailable`, `template_incompatible`, `request_conflict`,
`spawn_failed`, `tab_open_failed`, `persistence_failed`. `details.fields` contém caminhos, nunca valores secretos.
Erros da IA preservam códigos da Fase 12; lista de runs mostra falhas de item, sem convertê-las em rejeição tardia
da chamada original de lançamento. Cancelamento não promete conclusão apenas pelo retorno IPC.

## 7. Eventos e saída

Eventos emitidos **após commit**, sem comandos/env/output: `command-group://changed` contém
`{ groupId, scope, projectId, revision, operation }`; eventos `run-started`, `run-item-changed` e `run-finished`
contêm `{ runId, projectId, seq, status, counts, item? }`. `seq` é a versão monotônica **do run**, incrementada
por qualquer transição; `item` inclui somente id/status/exit/signal/timestamps/vínculos e sua versão.
`run-finished` somente quando terminal. Payloads contêm estado, não incrementos de contadores.

Store assina antes de buscar snapshot, bufferiza eventos durante a leitura e aplica os de versão maior.
Descarta duplicados/antigos; salto de `seq` solicita `run_get`, pois o evento perdido poderia referir-se a outro
item. Reabertura/foco/reconexão consultam runs ativos e reconciliam eventos terminais perdidos. Perda de evento
nunca é tratada como perda do processo supervisionado.

`GroupOutputFrame = { subscriptionId, itemId, seq, kind: replay|data|gap|end, text?, droppedBytes? }` é DTO gerado.
Attach captura watermark e assina live atomicamente para não duplicar/perder bytes na fronteira replay/live.
Ring buffer de saída **sanitizada** ≤ 256 KiB/item; cortar em limites UTF-8. Sanitização incremental cobre segredo
partido entre chunks; linhas sem terminador têm limite e são redigidas/truncadas conservadoramente, nunca
despejadas brutas. Replay não deve usar diretamente o buffer bruto de `pty_attach` para esse painel.

Fila de Channel limitada, envio em lotes, `gap` explícito quando consumidor lento; nunca bloquear o leitor PTY.
Persistência da cauda pelo writer em lotes; `run-finished` inclui flush sanitizado final. Detach/desmontagem remove
somente a assinatura, não mata tarefa. Sequencial/multiabas observam a saída apenas durante o item correlacionado,
sem atribuir à execução comandos manuais posteriores. Terminal de saída read-only não aceita teclado/colagem.

## 8. Gatilho `>>`, pesquisa e ações

### 8.1. Seleção do provider

Somente input `rich`, prefixo literal na posição zero e fora de escape: prefixo mais longo `>>` ganha de `>`.
Digitar o segundo `>` aborta sessão/requisições Artisan; apagar de `>>` para `>` aborta grupos e reavalia Artisan
(apenas Laravel). `>>>dev` é query `>dev`; ` >>`, `\>>` e passthrough não ativam. `Escape` fecha sem modificar
o texto e suprime reabertura até nova edição; enviar depois pode executar o redirecionamento literal no shell.

### 8.2. Busca e ordem

Pesquisar nome, descrição e labels normalizados, com pontuação fuzzy e bônus para prefixo do nome. Primeiro
filtrar/ordenar **cada escopo**, depois concatenar: seção "Deste projeto", seção "Globais". Nunca misturar escopos
por score; desempates por `sort_order`, nome e id. Sem query, usar ordem salva. Labels aparecem como chips curtos,
com nome/descrição acessíveis. Troca de projeto aborta a busca anterior e impede resultado tardio do outro projeto.

Sempre acrescentar no final, fora do filtro e sem entrar no limite de resultados:
1. **Criar grupo de comandos local**;
2. **Criar grupo de comandos global**.

Mesmo com catálogo vazio, nenhum resultado, carregamento ou erro, ambas permanecem visíveis. Sem projeto, local
fica desabilitada com motivo e global continua disponível. Executar global exige escolher projeto (cwd e abas
precisam desse contexto). As ações de criação podem aproveitar a query como nome do rascunho, nunca como comando.

↑/↓ navegam ignorando cabeçalhos/desabilitados, Enter executa o modo salvo apenas na ação de execução selecionada;
Enter na criação abre editor. Tab/→ abre ações do grupo: executar nos três modos, editar e excluir. ← volta,
Esc fecha. Enter segurado não cria runs duplicados: pendência + request_id estável por gesto de lançamento.
Após aceite limpa input; erro de preflight preserva query e foco. Enter não seleciona automaticamente o primeiro
comando após retorno assíncrono se o usuário já mudou o texto.

## 9. Frontend Vue/TypeScript

### 9.1. IPC e store

`src/ipc/commandGroups.ts` encapsula bindings e listen/Channel. `src/types/commandGroups.ts` contém apenas tipos
de UI/reexports, sem reproduzir enums Rust. Store `useCommandGroupsStore` em `src/stores/commandGroups.ts`:

- Catálogo global único e locais por `ProjectId`, estado loading/error por chave, revisões e single-flight.
- `load`, `get`, `create`, `update`, `remove`, `reorder`, `startRun`, `cancelRun`, `loadRun`, `loadHistory`,
  `suggest`, `loadRecurring`; GETs de página passam pela store do padrão `@maxvue/max-pinia`.
- Runs por id e itens por id; catálogos invalidados por changed local somente naquele projeto, global em todas
  as projeções sem recarregar todos os projetos imediatamente. Deleted mantém tombstone de revisão no cache.
- Assinaturas globais únicas no bootstrap; HMR/desmontagem global descarregam listeners. Componentes não assinam
  eventos por linha. Não manter output ilimitado na store nem guardar handlers Channel serializáveis nela.

### 9.2. Composables

`useCommandGroupEditor` mantém clone do rascunho, revisão base, validações, dirty, reordenação, aplicar/desfazer
sugestão; conflito de revisão não sobrescreve edição. `useCommandGroupRun` captura projeto/painel/geração no clique,
requestId por tentativa e template compatível; oferece estado agregado e cancelamento. `useGroupRunOutput`
anexa/destaca assinatura ao abrir/fechar, limita replay e limpa recursos. Provider usa store e AbortSignal;
`registerCommandGroupsTrigger` é chamado no bootstrap depois de registrar os providers da Fase 14.

### 9.3. Contratos Vue representativos

```ts
import type { CommandGroup, GroupRun } from '@/types/bindings';

interface CommandGroupModalProps {
    modelValue: boolean;
    group: CommandGroup | null;
    scope: 'local' | 'global';
    projectId: string | null;
}
interface GroupRunPanelProps {
    run: GroupRun;
}
```

Usar `defineProps<...>()`/`defineEmits<...>()` tipados; ids/escopos no código final derivados dos bindings, não
novos tipos de domínio manuais. Modal emite `update:modelValue` e `saved`; painel emite cancelamento por run/item
e pedido de saída. Handlers assíncronos expõem pending/errors e impedem duplo submit.

### 9.4. Componentes e fluxos do editor

`CommandGroupModal.vue`: nome, descrição, labels, escopo/projeto de criação, modo, política `&&`/`;`, cwd/env do
grupo, itens ordenados, validação por campo, Salvar/Cancelar/Excluir e sugestão IA. Escopo existente é somente
leitura; "Copiar para…" cria outro grupo. Salvar nunca executa. Reordenação por drag quando disponível e sempre
por botões/Alt+↑/↓, mantendo foco e anúncio acessível da posição.

`CommandGroupItemRow.vue`: título, comando, cwd/env opcionais, contínua, remover/duplicar/mover; duplicar gera id
novo. Env usa pares chave/valor e mascara valores sensíveis. Editor diferencia env literal de referência dentro
do comando. `CommandGroupSuggestPanel.vue` exibe resultado separado e ações Aplicar/Desfazer, sem apagar rascunho.

`GroupRunPanel.vue`, `GroupRunItemRow.vue`, `GroupRunIndicator.vue`: contadores, duração, modo, estado agregado,
falhas parciais e motivo de itens skipped/interrupted; link para aba/sessão existente e cancelamento disponível
apenas quando aplicável. `GroupRunOutputView.vue` usa terminal read-only e mostra lacunas/truncamento.
Tokens semânticos para executando/contínuo/finalizado/erro; texto e ícone acompanham cor. SFC na ordem
template → script setup TS → style SCSS scoped, quatro espaços e tags de abertura em linha única.

### 9.5. Ações e estados vazios

`GroupRunItemAction` é união de UI `show_output | focus_tab | cancel`; não inclui promoção headless→aba nesta
entrega. Estado sem grupos mantém criação local/global; sem execução, painel orienta executar um grupo; erro de
lista oferece tentar novamente. Toast resume falha de aceite; falhas posteriores ficam no painel sem tempestade
de toasts. Nenhum comando é disparado por watcher de campo, montagem de componente ou atualização de catálogo.

## 10. Sugestão assistida e dados externos

### 10.1. IA via Fase 12

`command_group_suggest` delega ao `TauriGroupAiAdapter`, que chama `AiService::generate_structured` (o serviço da Fase 12 registra `AiRequestId`) com
`schema_id = command-group.suggestion.v1`, schema fechado de `CommandGroupInput` sem ids/campos de execução,
objetivo em pt-BR e contexto sanitizado do projeto/shell. Escopo/projeto vêm do editor, não do modelo. Saída
deve respeitar 1–50 itens, modos, labels, limites de cwd/env e o mesmo validador do CRUD. Nenhum campo arbitrário
de JSON pode selecionar um executável/API interna ou injetar um run.

O transporte, header `X-Agent-Name: MaxTerminal`, seleção padrão, timeout, erro e tentativa de reparo pertencem
à Fase 12; não inventar rota HTTP nova nem chamar gateway do Vue. Requisição cancelada ou resposta de revisão
antiga é descartada. Resposta válida vai para preview; Aplicar modifica o rascunho, Salvar persiste, Executar é
gesto posterior separado. Erro de schema/gateway não altera dados digitados. Fase 15 não classifica a existência
de resposta JSON como autorização de execução.

### 10.2. Memória opcional

`RecurringCommandsSource` recebe projeto/limite e devolve comandos sanitizados já disponíveis na Fase 13.
Sem implementação, `available = false`; CRUD, execução e sugestão IA continuam completos. Selecionar chip do
histórico apenas adiciona item editável; não encadear execução nem persistir sem Salvar.

### 10.3. Arquivos de configuração

`.max-code/commands/groups.json` não faz parte desta entrega: nenhum watcher, parser executável, trust flag ou
dependência de adoção futura pelo MaxCode. JSON de schema IA e colunas SQLite são dados tipados com validação,
não uma API genérica de executar ações JSON. Não consumir funcionalidades das Fases 16/17 para concluir esta fase.

## 11. Ciclo de vida e garantias de integração

### 11.1. Propriedade

Somente runs aceitos criam recursos; cada recurso possui handle e id/generation antes de poder ser cancelado.
Editor aberto, filtro, sugestão e detach nunca encerram execução. Reexecutar após término gera requestId novo;
retry de transporte usa o anterior. Exclusão de definição preserva run/snapshot; remoção do projeto encerra antes
de excluir dados. Limite global evita explosão de PTYs com vários grupos simultâneos.

### 11.2. Concorrência

Lock por run para transições, lease por painel para sequencial, writer único para DB. Não segurar lock de banco
enquanto aguarda PTY/IA. Compare identidade completa antes de kill/restart; reutilização de PID não é autorização
para matar processo. Abas reutilizadas e sessões reiniciadas invalidam vínculos anteriores.

### 11.3. Histórico

Sequencial e multiabas vinculam sessão/bloco reais da Fase 07/05; background mantém saída e timestamps no run,
sem fabricar aba/sessão nem incrementar badges de abas. Grupo global executado em dois projetos tem histórico
independente por contexto. IDs nulos após retenção de sessão não impedem leitura do snapshot histórico.

### 11.4. Shutdown e bandeja

Fechar visualizador não cancela. Decisão de minimizar fica na Fase 09; quando apenas minimiza, os handles vivem.
Saída explícita do processo chama shutdown dos runners antes de liberar DB/PTY manager: interrompe filas,
cancela tarefas, aguarda supervisão e persiste estados finais. Registrar esse coordenador no bootstrap desta
fase, usando o fluxo de shutdown existente, sem alegar um `KeepAliveProvider` inexistente. Background não vira
aba fictícia só para influenciar badges/bandeja. Crash segue recuperação §4.4.

## 12. Sequência de implementação

1. Validar contratos reais 02–07/12/14, especialmente correlação de bloco, reserva de input e templates contínuos.
2. Implementar tipos/validador/migration/repo com testes de transação, revisões e retenção.
3. Implementar redutor e runners sobre portas fake; depois adaptadores reais e cancelamento/recuperação.
4. Registrar IPC/eventos/bindings e testar races de evento antes da resposta e resposta perdida.
5. Store, provider `>>`, editor, painel/saída e sugestão IA com validação compartilhada.
6. Validar shells reais, processos descendentes, falha parcial e contratos de dependência; entregar sem alterar
   especificações de outras fases para esconder incompatibilidades.

## 13. Verificação

### 13.1. Rust/SQL e integração

Além dos CA das instruções: migrations em banco limpo/incremental, `foreign_key_check`, Unicode nos nomes,
rollback de itens/labels, reorder incompleto, revisão concorrente, symlink fora da raiz, env literal e segredo
dividido no stream. Runners: exit antes de registrar, spawn que falha, falha de persistência após spawn,
cancelamento durante fila/abertura, request repetido após edição/exclusão e crash antes de gravar vínculo.

Matriz bash/zsh/fish: comandos com aspas/apóstrofos/backslashes/pipe/comentário, alteração de cwd entre itens,
override isolado, `false` seguido de sucesso com ambas as políticas, `exit`, foreground que ignora interrupção,
shell não suportado e falta de tracking. Assertar efeitos e códigos, não somente snapshot da string composta.
Paralelo: duas execuções somadas nunca excedem 16 PTYs; descendentes/setsid encerrados pela infraestrutura 02.
Multiabas: template agente incompatível, flag contínua correta, falha no item 2 sem apagar item 1, sem comando
inicial duplicado e sem fechar aba reutilizada. Checar estados agregados com tabela de casos incluindo partial.

### 13.2. Frontend e comportamento

`tests/commandGroups/*.spec.ts`: precedência `>>`/`>`, abort e resposta obsoleta, busca por labels, ordem entre
escopos, duas ações sempre finais, acessibilidade de reorder, conflito de edição, IA sem execução, invalidação
local/global, snapshot/evento fora de ordem, perda de evento terminal, detach sem cancelamento e requestId estável.
Fluxo manual em app Tauri: criar/editar/excluir grupos locais/globais, testar três modos, cancelar um/todos,
observar logs sanitizados e reiniciar após execução interrompida. Lint/typecheck/bindings/Cargo conforme Fase 01;
executar somente quando a implementação existir. Esta entrega é especificação, não evidência de testes de runtime.
