# Fase 07 — Abas, Templates, Split Panes e Sessões

> Fonte da verdade: [`../../AGENTS.md`](../../AGENTS.md), §4.2. Contratos comuns:
> [`../README.md`](../README.md), §2. Implementação normativa: [`spec.md`](./spec.md).

## 1. Objetivo

Entregar o workspace de terminais de cada projeto: abas abertas por templates, divisões recursivas,
sessões persistidas e restauração automática. A identidade da aba e seu histórico sobrevivem ao reinício
do aplicativo; cada nova execução usa PTYs novos e supervisionados.

Garantias centrais:

1. Cada projeto possui **exatamente um template padrão**, inclusive projetos existentes e importações em lote.
2. Uma aba aberta possui uma sessão lógica ativa (`TabSessionId`), agrupando uma sessão base (`SessionId`)
   por painel; cada painel possui no máximo um PTY vivo.
3. Fechar ou reiniciar só conclui após encerrar os processos supervisionados e drenar a persistência.
4. Recarregar a interface não abre abas, reinicia PTYs nem repete comandos iniciais.
5. O histórico usa a tabela **`sessions` mínima da Fase 05**, sem recriação, descarte de linhas
   antigas ou segunda definição incompatível. Metadados específicos desta fase ficam em tabelas auxiliares.

## 2. Escopo

### Incluído

| Área | Entrega |
|------|---------|
| Templates | CRUD por projeto; nome, ícone, pasta personalizada, comando inicial, adicionar ao workspace, execução contínua, ambiente shell/agente e variáveis de ambiente; eleição atômica do padrão |
| Abas | “Adicionar nova aba” usa o padrão; “Iniciar Aba Template” permite escolher; seleção, ordenação e fechamento; associação ao projeto e integração com badges |
| Painéis | Árvore horizontal/vertical, até oito folhas, foco, proporções persistidas, split, fechar e reiniciar painel encerrado |
| Sessões | “Iniciar nova Sessão” arquiva a atual, preserva layout e cria PTYs limpos; histórico paginado de comandos, timestamps, diretórios, exit codes e scrollback sanitizado |
| Workspace | Uma aba automática estável por template marcado; recuperação após crash; deduplicação por boot e por intenção; reanexação após reload da WebView |
| Persistência | Migrations aditivas, índices, repos transacionais e retenção; reutilização de `Db`, sanitizador, batching e FTS da Fase 03 |
| Integrações | `HybridTerminal` e registry da 04; status/blocos da 05; projetos, placeholder e hook de remoção da 06 |
| Extensões | Resolvedor de ambiente, observadores de ciclo de vida, overrides de abertura, slots de indicadores e aba fixa |

### Fronteiras

Indicadores Git/portas são da 08; tray/atalhos/notificações, da 09; MaxCode, da 11; IA, da 12;
resolvedores concretos de agentes e worktrees, da 13; Artisan/atalhos, da 14; grupos, da 15;
Issues, da 16; busca global, da 17. A 07 oferece contratos para esses consumidores sem depender deles.
Status e `seen` permanecem autoridade da 05; badges e projeto selecionado, da 06.

## 3. Pré-requisitos e compatibilidade

**Dependências diretas: 04, 05 e 06.** A 03 é transitiva por 05/06; a 02, por 04/05; a 01 fornece
ids Uuid v7, erros, geração de bindings e ferramentas.

| Origem | Contrato necessário |
|--------|--------------------|
| 02 | `PtyManager::spawn(spec, sink)`, `kill_and_wait`, `write`, `subscribe`, `OutputTap`, ownership e supervisão Linux; chamadas bloqueantes fora do executor da UI |
| 03 | `Db::write/read`, migrations versionadas, `BatchWriter::flush`, `SanitizedText`, `StreamSanitizer`, `FtsSpec`, infraestrutura de retenção |
| 04 | `HybridTerminal(paneId, ptyId, visible)`, `TerminalInstanceRegistry`, hub de stream, `CommandBlockFeed.clear`, provedor de contexto |
| 05 | Runtime de shell, `SessionProvider`, registry de status, eventos de comandos, `sessions` mínima e `command_blocks`; `useTabStatusStore` |
| 06 | `Project`, `ProjectInsertContributor`, `ProjectActivityGuard`, `ProjectRemovalHook`, `useProjectsStore.attachTab/detachTab/setActiveTab`, `ProjectWorkspace(project: Project \| null)` |

**Contrato canônico:** a migration `NNNN_05_shell_sessions_command_blocks.sql` da 05 define `sessions`
mínima e `command_blocks`. Cada sessão base corresponde a um painel/stream. A 07 acrescenta colunas de
arquivamento, agrupamento por aba e scrollback, sem redefinir as tabelas existentes. A 03 fornece apenas
infraestrutura WAL/migrations/sanitização/FTS. Ver `spec.md` §5.3.

Também devem estar verificadas as barreiras de término da captura e persistência da 05 e a alocação de
sequência contínua de comandos/chunks por sessão de painel (`spec.md` §4). Eventos visuais não substituem
essas barreiras. Adaptadores desta fase traduzem tipos e nomes; não fabricam garantias ausentes.

## 4. Entregáveis

### Rust e SQLite

- `mt-core/src/workspace/{mod,template,tab,layout,session}.rs`: DTOs, validação e operações puras da árvore.
- `mt-db/migrations/NNNN_07_*.sql`: templates, abas, painéis, extensão aditiva de sessões05,
  agrupamentos/vínculos, scrollback/FTS e recibos; numeração global após migrations anteriores.
- `mt-db/src/migrations/code/m07_backfill.rs`: templates padrão para projetos existentes e vínculo não destrutivo
  do histórico identificável; sem modificar migrations já aplicadas.
- `mt-db/src/repos/{templates,tabs,panes,tab_sessions,scrollback,workspace_operations}.rs`:
  compõem o repositório base de sessões, sem substituí-lo.
- `src-tauri/src/workspace/{mod,manager,runtime,environment,recorder,restore,observer,adapters,errors}.rs`:
  orquestração, single-flight de boot, idempotência, compensação e supervisão.
- `src-tauri/crates/mt-db/src/repos/templates.rs`: `DefaultTemplateContributor` implementa internamente a trait `pub(crate)` `ProjectInsertContributor` e cria padrão na transação do
  projeto, inclusive importação/auto-descoberta; usa o hook fornecido pela 06.
- `src-tauri/src/commands/{templates,tabs,panes,sessions,workspace}.rs` e `events/workspace.rs`:
  IPC/eventos tipados e geração de `src/types/bindings.ts`.

### Frontend

- `src/ipc/{templates,tabs,sessions,workspace}.ts`: wrappers gerados e listeners tipados.
- `src/stores/{templates,tabs,sessions}.ts`: stores de dados serializáveis, revisão e cache paginado.
- `src/composables/workspace/{useWorkspaceEvents,useActiveTabSeen,usePaneLayout,layoutTree}.ts`.
- `ProjectWorkspace.vue`: mantém a prop da 06; componentes internos `TabBar`, `TabBarItem`,
  `TemplateLaunchPopover`, `TemplateEditorModal`, `TabWorkspaceView`, `PaneLayout`, `PaneView`,
  `SessionHistoryDrawer`, `SessionReadonlyViewer`.
- Testes de contrato Rust/SQLite, integração PTY e frontend descritos abaixo; evidências dos comandos
  de validação quando a fase for implementada.

## 5. Critérios de aceite

### Templates e projetos

- [ ] Projeto avulso, importado, aceito da descoberta ou autoimportado já é visível com exatamente um padrão;
  falha ao inserir o padrão desfaz também a criação do projeto/lote.
- [ ] Backfill é idempotente: mantém padrão existente; elege template existente em ordem estável se necessário;
  cria “Terminal” somente quando não há templates. Nenhuma aba/PTY é aberta pelo backfill.
- [ ] Índice parcial impede dois padrões. Troca executa desmarcar e marcar em uma transação; falha intermediária
  preserva o anterior. Exclusão do padrão é recusada; exclusão de outro preserva abas e histórico.
- [ ] Todos os parâmetros do AGENTS.md são editáveis e validados. Comando/env sensíveis não aparecem no banco,
  WAL, backups, eventos ou logs; referências a env herdadas permitem execução sem gravar o segredo.
- [ ] Template de agente pode ser salvo; abertura sem resolvedor retorna erro claro. Nenhuma integração da 13
  é requisito para abrir templates shell.

### Árvore, identidade e processos

- [ ] Horizontal significa divisor horizontal, painéis acima/abaixo; vertical significa lado a lado.
  Folhas únicas, conjunto de folhas igual ao de painéis abertos, profundidade ≤ 7, até oito painéis.
- [ ] Split cria um novo `PaneId`/`PtyId` na sessão atual, herda cwd do painel e não repete o comando inicial.
  O fechamento do último painel usa exatamente o fluxo de fechar aba.
- [ ] `TabId` e `PaneId` permanecem em nova sessão; `TabSessionId`, todos os `SessionId` e `PtyId` mudam. Evento tardio da geração
  antiga não modifica status, layout, contadores nem terminal da geração atual.
- [ ] Fechar aba/projeto, reiniciar painel ou nova sessão aguarda os descendentes rastreados, inclusive processo
  desacoplado por `setsid`; `/proc` não contém os pares `(pid,start_time)` terminados.
- [ ] Falha/timeout de encerramento mantém ownership e estado recuperável, bloqueia remoção do projeto e impede
  spawn substituto. Nunca retorna sucesso descartando uma árvore de processos ainda viva.
- [ ] Concorrência split × close × nova sessão × remoção de projeto não cria PTYs sem dono; cancelamento do
  `invoke` não cancela a supervisão nem deixa operação pela metade.

### Sessões, histórico e restauração

- [ ] Migração de banco preenchido preserva ids, comandos, scrollback e FKs. Dados legados não vinculáveis
  permanecem na base; nenhuma linha é descartada por falta de aba/projeto.
- [ ] Um agrupamento ativo por aba; cada painel tem sua sessão base e sequência independente, sem colisões
  durante execução simultânea. Reiniciar painel encerra/arquiva sua sessão e cria outra vinculada ao mesmo grupo.
- [ ] Nova sessão drena comandos e scrollback, arquiva anterior e cria sucessora atomicamente; layout preservado,
  xterm/blocos limpos. Falha de spawn afeta só o painel correspondente e é exibida em `TabView`.
- [ ] Histórico é somente leitura, com paginação e filtros por painel; sessão antiga não executa bytes como
  escapes ativos nem reenvia comandos ao PTY. Busca só indexa conteúdo sanitizado.
- [ ] Segredo dividido entre frames, UTF-8 fragmentado, linha longa, alt-screen, overflow e término abrupto não
  vazam conteúdo. Limite de 8 MiB é por sessão inteira, não multiplicado pelo número de painéis.
- [ ] Dois pedidos de restore no mesmo boot ou reload da WebView produzem os mesmos ids e nenhum novo comando.
  Templates marcados abrem uma aba automática cada, preservando `TabId`/layout; abas manuais não são restauradas.
- [ ] Crash deixa sessões antigas arquivadas e blocos pendentes concluídos como desconhecidos antes do restore;
  processos remanescentes são tratados pela 02 antes de novos spawns.
- [ ] Comando inicial é enviado no máximo uma vez por lançamento raiz. Timeout sem prompt confiável não injeta
  texto em processo desconhecido; UI oferece tentativa explícita. Falha ambígua de escrita não gera retry automático.

### UI e verificações

- [ ] Resposta IPC atrasada e evento duplicado/fora de ordem não ressuscitam aba fechada; snapshot e assinatura
  não perdem eventos durante o boot; foco e seleção permanecem por projeto.
- [ ] Troca de aba apenas oculta/reanexa a view; instâncias xterm e PTYs continuam; nova sessão descarta o xterm
  antigo antes de anexar o novo. Um único consumidor confirma bytes por PTY.
- [ ] `splitpanes` aceita seu payload real de `resized`; resize obsoleto retorna conflito, sem alterar outro nó.
  Tamanho provoca `fit` pela 04; foco/teclado, `tablist` e divisores permanecem acessíveis.
- [ ] Badges e `seen` usam a política única 05/06; não são recalculados por uma segunda máquina de estados.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, testes Rust afetados,
  geração de bindings sem drift, typecheck `tsgo`, lint e testes frontend afetados passam.
- [ ] Medições de abertura/restore e rajada de 200 MB registram ambiente, tempos e memória; metas não substituem
  garantias de término. Testes usam limites determinísticos das filas em vez de depender somente de RSS.

## 6. Ordem recomendada de implementação

1. Fixar contratos base de sessões por painel, captura/flush e agrupamento com 03/05; testes de compatibilidade.
2. Domínio, migrations aditivas e criação transacional do padrão; backfill e integridade em banco preenchido.
3. Orquestração e ownership: abrir, split, fechar, nova sessão, falhas parciais e remoção de projeto.
4. Recorder, histórico e busca; restauração idempotente e recuperação de operações interrompidas.
5. IPC, stores, componentes e integração com 04/05/06; validação estática, integração e roteiro manual.
