# Fase 15 — Grupos de Comandos (`>>`) (`instructions.md`)

> Fonte da verdade: [`../../AGENTS.md`](../../AGENTS.md) §8.2 (Grupos de Comandos) e §5.1 ("Geração Assistida —
> sugestão de Grupos de Comandos"). Contratos compartilhados: [`../README.md`](../README.md) §2.1–§2.6.
> Especificação técnica detalhada: [`./spec.md`](./spec.md).

---

## 1. Objetivo da Fase

Entregar o recurso completo de **Grupos de Comandos**: conjuntos nomeados e ordenados de comandos shell, com escopo
**Local** (do projeto) ou **Global** (todos os projetos), acionados pelo gatilho **`>>`** do input rico e executados em
um de três modos:

1. **Sequencial na mesma aba** — fila no PTY do painel ativo, com política **`&&`** (parar na primeira falha)
   ou **`;`** (continuar após falha). Cada item é submetido após o prompt e o término correlacionado do anterior;
   não concatenar strings arbitrárias como se bash, zsh e fish fossem intercambiáveis.
2. **Paralelo em Background** — executa todos os itens concorrentemente em PTYs ocultos (headless) supervisionados
   pela Fase 02, com painel de progresso, status/exit por item, cancelamento sem órfãos e registro em histórico.
3. **Multi-Abas** — abre uma aba por item via `tab_open` (Fase 07), aplicando título, pasta, comando inicial e flag
   de Execução Contínua (ideal para subir todo o ambiente de desenvolvimento).

A fase também entrega o editor `CommandGroupModal.vue` (itens reordenáveis, escolha de modo/separador), acessado pelas
opções fixas **"Criar grupo de comandos local"** / **"Criar grupo de comandos global"**, com **sugestão assistida**
via `ai_generate_structured` (Fase 12). Sugestões da memória contextual (Fase 13) são integração opcional,
sem transformar essa fase em pré-requisito obrigatório.

Todo o backend vive no módulo `groups` do crate `mt-commands` (criado na Fase 14, que já hospeda o módulo `artisan`).

---

## 2. Escopo

### 2.1. Incluído

| # | Item | Detalhe |
|---|------|---------|
| I-01 | Modelo de domínio | `CommandGroup`, `CommandGroupItem`, `GroupScope`, `ExecutionMode`, `SequentialSeparator`, `GroupRunId` (newtype local do crate), DTOs de entrada/saída (spec §3). |
| I-02 | Migration | `0009_15_command_groups.sql` com `command_groups`, `command_group_labels`, `command_group_items`, `command_group_runs`, `command_group_run_items`, `CHECK` de escopo/projeto, índices únicos por nome (spec §5; sequência contígua D-01). |
| I-03 | Repositório | `CommandGroupRepo` (trait) + `SqliteCommandGroupRepo` sobre `mt-db`; CRUD transacional, reordenação, substituição atômica de itens. |
| I-04 | Validação | Nome (1–80), ≤ 50 itens, comando 1–8192 bytes sem `\n`/`\r`/NUL, `cwd` relativo e confinado, chaves de env POSIX, detecção de segredos literais via `mt_db::sanitizer` (bloqueia gravação). |
| I-05 | Sequencial | `SequentialComposer` prepara cada item para o shell real; `SequentialRunner` aguarda término + prompt no mesmo PTY; `&&`/`;` são políticas de fila. Overrides isolados em shell filho com `env -C`; shell desconhecido ou ausência de correlação confiável bloqueiam o modo (spec §4). |
| I-06 | Executor paralelo | `ParallelRunner` sobre `mt_pty::PtyManager` (PTYs headless com `PtyOwner` sem aba/painel; índice de propriedade local a esta fase), limite global de concorrência (16, fila), status por item, cancelamento sem órfãos, saída sanitizada e recuperação no boot. |
| I-07 | Multi-Abas | `MultiTabPlanner` gera `TabOpenRequest` + `TabOpenOverrides` reais da Fase 07; `MultiTabRunner` abre via adaptador Rust do `WorkspaceManager`. Template shell compatível fornece `continuous`, que não existe em `TabOpenOverrides`; preflight e falhas parciais descritos na spec §4. |
| I-08 | Histórico de execuções | Tabelas `command_group_runs`/`command_group_run_items` vinculadas à sessão de origem (`origin_session_id`, Fase 07), saída sanitizada e truncada (256 KiB/item). |
| I-09 | Comandos IPC | `command_group_list`, `command_group_get`, `command_group_create`, `command_group_update`, `command_group_delete`, `command_group_reorder`, `command_group_run`, `command_group_run_cancel`, `command_group_run_list`, `command_group_run_get`, `command_group_run_item_attach`, `command_group_run_item_detach`, `command_group_suggest`, `command_group_recurring_commands` (spec §6). |
| I-10 | Eventos | `command-group://changed`, `command-group://run-started`, `command-group://run-item-changed`, `command-group://run-finished`; saída ao vivo de item somente via `Channel` (spec §7). |
| I-11 | Gatilho `>>` | `commandGroupsTrigger.ts` registrado no `TriggerRegistry` (Fase 04), com regra exata de desambiguação frente ao `>` da Fase 14 (spec §8.1), seções "Deste projeto" → "Globais", filtro fuzzy, setas, etapa 2 de ações, itens fixados ao final. |
| I-12 | Store | `useCommandGroupsStore` (`src/stores/commandGroups.ts`): cache por projeto + globais, execuções ativas, assinatura de eventos, ações de CRUD/execução/sugestão. |
| I-13 | Componentes | `CommandGroupModal.vue`, `CommandGroupItemRow.vue`, `CommandGroupSuggestPanel.vue`, `GroupRunPanel.vue`, `GroupRunItemRow.vue`, `GroupRunOutputView.vue`, `GroupRunIndicator.vue` (spec §9.4). |
| I-14 | Sugestão assistida | `command_group_suggest` usa o gerador estruturado da Fase 12 e schema de rascunho; resultado validado, revisável e nunca executado/salvo automaticamente. Memória da Fase 13 é capacidade opcional (spec §10). |
| I-15 | Consistência de execuções | Snapshot imutável, `request_id` idempotente, revisões de edição, estados por item e agregado (`partial`, `cancelled`, `interrupted`), falhas de lançamento e reconciliação após perda de eventos (spec §§3–7). |

### 2.2. Fora de escopo (e fase responsável)

| Item | Fase |
|------|------|
| Infra do `TriggerRegistry`, `CompletionPopup.vue`, `completionSession`, input rico | 04 (consumida) |
| Spawn/kill/supervisão de PTY, replay, `pty_attach`, varredura de órfãos | 02 (consumida) |
| `TabStatus`, OSC 133 e blocos de comando da aba (cada item sequencial possui seu bloco correlacionado) | 05 (consumida) |
| `tab_open`, sessões, `useTabsStore`, "Iniciar nova Sessão" | 07 (consumida) |
| Gatilho `>` (Artisan), `/` (atalhos universais), módulo `artisan` do `mt-commands` | 14 (coexistência apenas) |
| Cliente `MaxAiManager`, `ai_generate_structured`, seleção de Harness/modelo | 12 (consumida) |
| Memória contextual de comandos recorrentes (coleta e ranqueamento) | 13 (consumida, somente leitura) |
| Contadores dos badges da coluna de projetos (execuções em background **não** são abas e não entram nos badges §4.1) | 06 (inalterado) |
| Bandeja e decisão de fechar janela | 09; esta fase participa do shutdown supervisionado, sem presumir `KeepAliveProvider` inexistente (spec §11.4). |
| Importação/exportação ou execução de `.max-code/commands/groups.json` | Adiada; nenhum arquivo JSON é fonte executável nesta fase. |
| Busca FTS5 global no histórico de execuções de grupos, E2E completos, benchmarks formais | 17 |
| Grupos gerados a partir de issues / "Corrigir todos os Bugs" | 16 (não usa grupos) |
| Promover um item paralelo a uma aba visível | Não previsto no AGENTS.md — apenas ponto de extensão (`GroupRunItemAction`, spec §9.5) |

---

## 3. Pré-requisitos

### 3.1. Fases (contratos consumidos — nomes reais prevalecem conforme `src/types/bindings.ts`)

| Fase | Contrato consumido |
|------|--------------------|
| 01 | `mt-core` (`MtError`, `MtResult`, `ProjectId`, `TabId`, `PaneId`, `PtyId`, `SessionId`, `CommandGroupId`), `tauri-specta`, `AppPaths`, lint/test. |
| 02 | `mt_pty::PtyManager::{spawn, write, kill, subscribe, add_output_tap}`, `SpawnSpec`, `PtyOwner { tab_id, pane_id }`, `KillMode`, `ExitInfo`, `OutputSink`, replay e shutdown sem órfãos. |
| 03 | `mt-db` (pool WAL, migrations, `sanitizer::sanitize()`), padrão de repositórios. |
| 04 | `TriggerRegistry` (prefixo mais longo, multi-etapa, seções, `pinned: 'bottom'`), `CompletionPopup.vue`, `ptyClient.write`, `TerminalView.vue` (modo somente leitura). |
| 05 | `TabStatus` para apresentação; prompt pronto, término de bloco/exit code e identidade PTY/sessão para correlação. Status da aba sozinho não autoriza escrita. |
| 06 | Tabela `projects(id)`, `useProjectsStore` (projeto ativo, pasta raiz, stack). |
| 07 | `TabOpenRequest`, `TabOpenOverrides`, `WorkspaceManager::{open_tab, close_tab, restart_pane}`, templates shell e flag contínua, `useTabsStore`, sessões e observadores. |
| 12 | `ai_generate_structured`, `StructuredRequest`, `StructuredResponse`, `AiService::generate_structured`, schema `command-group.suggestion.v1`, cancelamento via registro da Fase 12 e configuração padrão. |
| 13 | Pré-requisito de persistência obrigatório: migration `0008_13_agents_skills_memory.sql` antes de `0009_15_command_groups.sql` (D-01). Serviço de comandos recorrentes opcional via `RecurringCommandsSource`; ausência não bloqueia CRUD/execução/IA. |
| 14 | Crate `mt-commands` existente, `ErrorCode` com prefixo `commands.*`, gatilho `>` registrado, utilitário `fuzzyScore` (se exportado). |

### 3.2. Ferramentas
- Rust stable (edição 2024), `cargo clippy`, `cargo test`; Node ≥ 22, `tsgo`, Vitest, ESLint/Stylelint (Fase 01).
- GNU coreutils ≥ 8.28 (`env -C`) — presente no CachyOS (`env --version`).
- Shells para testes: `bash`, `zsh`, `fish`; testar sintaxe/quoting próprios, sem traduzir comandos do usuário.
- Reordenação acessível por botões/teclado; arrastar pode reutilizar dependência já instalada, sem impor biblioteca nova.

---

## 4. Entregáveis

### 4.1. Backend
- `src-tauri/crates/mt-commands/src/groups/mod.rs` — reexports do módulo.
- `src-tauri/crates/mt-commands/src/groups/model.rs` — tipos de domínio e DTOs.
- `src-tauri/crates/mt-commands/src/groups/validate.rs` — validação + detecção de segredos.
- `src-tauri/crates/mt-commands/src/groups/repo.rs` — `CommandGroupRepo` + `SqliteCommandGroupRepo`.
- `src-tauri/crates/mt-commands/src/groups/compose.rs` — `ShellDialect`, `SequentialComposer`.
- `src-tauri/crates/mt-commands/src/groups/resolve.rs` — `CwdResolver`, `RunContext`.
- `src-tauri/crates/mt-commands/src/groups/runner.rs` — `ParallelRunner`, `RunHandle`, `GroupRunSink`.
- `src-tauri/crates/mt-commands/src/groups/{sequential,multi_tab,aggregate,ports}.rs` — filas, estados e portas desta fase.
- `src-tauri/crates/mt-commands/src/groups/plan.rs` — `MultiTabPlanner`.
- `src-tauri/crates/mt-commands/src/groups/suggest.rs` — `GroupSuggester` (IA + memória).
- `src-tauri/crates/mt-commands/src/groups/events.rs` — payloads + trait `GroupEventSink`.
- `src-tauri/crates/mt-commands/src/groups/service.rs` — `CommandGroupService` (orquestração).
- `src-tauri/src/command_groups/adapters.rs` — ligação com PTYs, shell integration, workspace e IA existentes.
- `src-tauri/crates/mt-commands/tests/groups_{repo,validate,compose,runner,service}.rs`.
- `src-tauri/crates/mt-db/migrations/0009_15_command_groups.sql` (versão 9 fixa; D-01).
- `src-tauri/src/commands/command_groups.rs` — comandos Tauri; registro no builder `tauri-specta`.
- `src-tauri/src/events/command_groups.rs` — `TauriGroupEventSink`, `ChannelRunObserver`.
- `src-tauri/capabilities/default.json` — permissões `command_group_*` restritas à janela `main`.

### 4.2. Frontend
- `src/ipc/commandGroups.ts` — wrappers tipados + `listen` dos eventos.
- `src/types/commandGroups.ts` — apenas tipos derivados de UI (re-exports de `bindings.ts`).
- `src/stores/commandGroups.ts` — `useCommandGroupsStore`.
- `src/composables/commandGroups/useCommandGroupRun.ts`, `useCommandGroupEditor.ts`, `useGroupRunOutput.ts`.
- `src/terminal/triggers/providers/commandGroupsTrigger.ts` (+ registro em `src/terminal/triggers/providers/index.ts`).
- `src/components/commandGroups/{CommandGroupModal,CommandGroupItemRow,CommandGroupSuggestPanel,GroupRunPanel,GroupRunItemRow,GroupRunOutputView,GroupRunIndicator}.vue`.
- `tests/commandGroups/*.spec.ts` (spec §13.2).

---

## 5. Critérios de Aceite

### 5.1. Automatizados (bloqueantes)
- [ ] CA-01 `cargo fmt --check`, `cargo clippy --workspace -- -D warnings` e `cargo test -p mt-commands` passam.
- [ ] CA-02 `groups_repo.rs`: inserir grupo `local` com `project_id = NULL` falha no `CHECK`; `global` com `project_id` preenchido falha; nome duplicado (case-insensitive) no mesmo projeto falha com `commands.group.name_conflict`, mas o mesmo nome em outro projeto e em `global` é aceito; excluir o projeto (FK) remove seus grupos e itens em cascata.
- [ ] CA-03 `groups_repo.rs`: `update` substitui itens atomicamente (falha no meio ⇒ estado anterior intacto); `reorder` com ids de outro escopo retorna `commands.group.reorder_mismatch`; `list(project)` retorna locais por `sort_order` seguidos de globais por `sort_order`.
- [ ] CA-04 `groups_validate.rs`: rejeita comando com `\n`, `cwd` absoluto, `cwd` que escapa da raiz (`../..`), chave de env `1ABC`, > 50 itens, e comando contendo `Bearer eyJ…`/`APP_KEY=base64:…` (`commands.group.secret_detected`, `details.fields` aponta o campo).
- [ ] CA-05 Shells reais bash/zsh/fish: fila `[false, printf ok]` com `&&` não inicia o segundo item; com `;` inicia e mantém a falha no agregado. Comentários, aspas, pipes, `cd`, `exit`, env com espaços/apóstrofos/backslash e override de cwd têm testes sem concatenação textual universal; override não vaza ao item seguinte. Shell desconhecido é erro explícito.
- [ ] CA-06 `groups_runner.rs` (com `PtyManager` real): 3 itens `sleep 0.2; exit 0`, `exit 3`, `sleep 30` ⇒ eventos `running` → `succeeded(0)`, `failed(3)`; `command_group_run_cancel` encerra o terceiro com status `cancelled` em < 3,5 s e **nenhum PID** da árvore permanece em `/proc`; execução final `cancelled` com `finished_at` gravado.
- [ ] CA-07 `groups_runner.rs`: 20 itens com limite 16 ⇒ nunca mais de 16 PTYs simultâneos; 4 itens ficam `pending` até liberar vaga.
- [ ] CA-08 `groups_runner.rs`: saída de 2 MiB de um item resulta em `output_tail` ≤ 256 KiB, sanitizado (token `ghp_…` impresso vira `[REDACTED]`), sem bloquear o leitor do PTY.
- [ ] CA-09 `groups_service.rs`: ao iniciar com linhas `status = 'running'` órfãs (simulação de crash), `recover_interrupted()` marca execução e itens como `interrupted`.
- [ ] CA-10 `npm run typecheck`, `npm run typecheck:vue`, `npm run lint`, `npm run lint:style` sem erros; `src/types/bindings.ts` regenerado e sem drift (`git diff --exit-code`).
- [ ] CA-11 `npx --no-install vitest run tests/commandGroups` verde com cobertura ≥ 90% de linhas em `src/stores/commandGroups.ts` e `commandGroupsTrigger.ts`, ≥ 80% em `src/components/commandGroups/**`.
- [ ] CA-12 `commandGroupsTrigger.spec.ts`: buffer `>>` abre o provider de grupos mesmo com o provider `>` registrado; `>` sozinho continua no Artisan; `>>` → Backspace → `>` volta ao Artisan com a sessão de grupos abortada (`AbortSignal.aborted === true`); `\>>` não abre popup; ` >>` (espaço inicial) não abre; `>>>dev` usa a query `>dev`; em modo `passthrough` nada abre.
- [ ] CA-13 `commandGroupsTrigger.spec.ts`: seção "Deste projeto" sempre antes de "Globais"; filtro fuzzy `dvup` encontra "Dev Up"; os itens "Criar grupo de comandos local" e "Criar grupo de comandos global" estão sempre presentes no fim (inclusive com 0 resultados); sem projeto ativo, a opção local fica desabilitada com dica.
- [ ] CA-14 `commandGroupsStore.spec.ts`: eventos `command-group://changed` invalidam somente o cache do projeto afetado (ou globais); `run-item-changed` fora de ordem (`seq` menor) é descartado.

### 5.2. Comportamento observável (`npm run tauri dev`)
- [ ] CA-15 Em um projeto, digitar `>>` abre o popup com "Deste projeto" e "Globais"; ↑/↓ navegam com wrap-around; `Enter` executa no modo do grupo; `Tab`/`→` abre a etapa de ações (Executar em modo Sequencial / Paralelo / Multi-Abas, Editar, Excluir); `Escape` fecha mantendo o texto literal.
- [ ] CA-16 "Criar grupo de comandos local" abre `CommandGroupModal` com escopo local pré-selecionado; é possível adicionar 3 itens, arrastar para reordenar (e reordenar por teclado `Alt+↑/↓`), escolher modo e separador; salvar fecha o modal e o grupo aparece imediatamente no `>>`.
- [ ] CA-17 Modo **Sequencial**: `[npm ci, npm run build]` usa o mesmo PTY; `build` somente inicia após término bem-sucedido + novo prompt de `ci` com política `&&`. Cada item tem bloco OSC 133 próprio e vínculo à execução. Painel ocupado, alt-screen, prompt não confirmado ou outra fila detentora do painel recusam escrita; oferece-se "Executar em nova aba".
- [ ] CA-18 Modo **Paralelo**: o `GroupRunPanel` mostra cada item com cor semântica (`--max-success-500` executando, `--max-info-500` contínuo, `--max-warning-500` finalizado, `--max-danger-500` erro, neutro pendente/cancelado), duração e exit code; "Ver saída" exibe a saída ao vivo (replay + live) em terminal somente leitura; "Cancelar" (item ou tudo) encerra sem deixar processos (`pgrep -f <marcador>` vazio).
- [ ] CA-19 Modo **Multi-Abas**: com template shell contínuo compatível, grupo `npm run dev` ("Vite") e `php artisan horizon` ("Horizon") abre 2 abas em ordem com cwd/env corretos e status `continuous`. Falha na segunda abertura preserva a primeira; ausência de template compatível falha no preflight antes de abrir qualquer aba.
- [ ] CA-20 Botão "Sugerir com IA" produz rascunho revisável via Fase 12 sem salvar/executar; resposta tardia não sobrescreve edição. Erro/cancelamento preservam o formulário. "Do histórico" somente fica disponível com fonte opcional registrada.
- [ ] CA-21 Sair explicitamente cancela os grupos e aguarda supervisão sem órfãos; minimizar mantém execução. Após crash, reconciliação marca itens não terminais como `interrupted`, preserva os concluídos e não relança comandos.
- [ ] CA-22 Todos os textos de UI, mensagens de `MtError` e comentários em pt-BR; SFCs na ordem template → script → style, tags de abertura em linha única.
- [ ] CA-23 Repetir `request_id` com a mesma requisição devolve a execução existente sem novo PTY/aba; payload diferente retorna conflito. Edição/exclusão do grupo durante a execução preserva seu snapshot.
- [ ] CA-24 Falha de spawn, falha após abrir parte das abas, mistura sucesso/erro e cancelamento concorrente a exit resultam nos estados e contadores da spec §4.4; cancelamento repetido não mata recursos reaproveitados por outra sessão.
- [ ] CA-25 Busca por nome, descrição e labels preserva locais antes de globais; labels não substituem as duas ações finais, inclusive com lista vazia ou erro de carregamento.
- [ ] CA-26 Eventos perdidos/fora de ordem e reconexão se reconciliam pelo snapshot versionado; atualização concorrente do editor retorna conflito de revisão sem perder o rascunho.

---

## 6. Riscos e Mitigação

| # | Risco | Impacto | Mitigação |
|---|-------|---------|-----------|
| R-01 | `>>` no início da linha é redirecionamento legítimo (`>> arquivo`). | Médio | Escape `\>>`; `Escape` fecha e insere literal; gatilho só em `rich`, posição 0, sem espaço inicial (spec §8.1). |
| R-02 | Conflito de sessão entre `>` (Artisan) e `>>` ao digitar rápido. | Médio | Re-resolução por tecla no `TriggerRegistry` (prefixo mais longo) com abort da sessão anterior; teste dedicado (CA-12). |
| R-03 | Quoting incorreto entre bash/zsh/fish gera comando quebrado. | Alto | Dialeto real, fila por item e quoting específico; shell desconhecido é recusado, sem fallback POSIX silencioso (CA-05). |
| R-04 | Escrever no PTY enquanto um programa está em primeiro plano injeta teclas no programa. | Alto | Reserva coordenada do painel, identidade da geração e prompt confirmado no backend; status visual sozinho não autoriza escrita (spec §4.1). |
| R-05 | Processos órfãos de execuções paralelas (daemons, `setsid`). | Alto | Supervisão da Fase 02; índice local `(run_id, item_id) → PtyId`; conclusão exige limpeza dos descendentes, sem variante nova de `PtyOwner`; teste CA-06. |
| R-06 | Saída volumosa de itens em background consome memória ou trava o leitor. | Médio | `GroupRunSink` com ring buffer de 256 KiB, sanitização por bloco de linhas, sem Channel anexado ⇒ sem backpressure bloqueante; Channel só ao abrir "Ver saída". |
| R-07 | Segredos persistidos em definições de grupo (env/comandos) ou enviados à IA. | Alto | Bloqueio na gravação via `sanitizer` (`secret_detected`); orientação para referenciar variáveis já exportadas (`$TOKEN`); entrada da IA sanitizada. |
| R-08 | Configuração de arquivo ser confundida com execução. | Alto | Definições vêm exclusivamente do CRUD SQLite; nenhum watcher/import/runner de JSON nesta entrega. |
| R-09 | Divergência de nomes com contratos das Fases 04/07/12/13 ainda não especificados. | Médio | Adaptadores finos isolados (`TabOpener`, `AiStructuredClient`, `RecurringCommandsSource`, provider do gatilho); nomes reais conforme `bindings.ts` prevalecem. |
| R-10 | `env -C` indisponível (coreutils antigo/BusyBox). | Baixo | Verificar capacidade; overrides sequenciais retornam `commands.group.env_cwd_unavailable`, sem fallback que altere o cwd do shell fish pai. |
| R-11 | Muitas abas abertas de uma vez em Multi-Abas sobrecarregam WebGL/IPC. | Baixo | Abertura sequencial (await por aba), limite de 50 itens, WebGL só na aba visível (Fase 04). |
