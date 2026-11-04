# Plano de execução — Horizon no cabeçalho do projeto

## 1. Identificação e leitura inicial

- **Projeto:** MaxTerminal. **Data:** 03/11/2026.
- **Worktree/branch:** `wt-1b4b2671`.
- **Raiz exclusiva:** `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-1b4b2671`.
- **Spec obrigatória:** [spec.md](./spec.md), elaborada antes deste plano.
- **Situação:** planejamento, sem implementação e sem validação executável. `git status --short --branch` inicialmente retornou apenas `## wt-1b4b2671`.
- Não criar worktree nem trocar branch; não commitar, mergear, fazer push ou remover esta worktree. Integração pertence ao MaxCode quando o usuário pedir.

Na primeira admissão, ler plano e spec. Nas retomadas, ler estado de execução e trechos pertinentes, conferir alterações e trabalho existente, revalidar contratos que mudaram e executar lacunas. Não reiniciar etapas aprovadas sem mudança factual, nem reabrir decisões sem evidência nova ou mudança de requisito.

## 2. Resultado solicitado e decisões

### 2.1 Requisitos

| ID | Requisito observável |
| --- | --- |
| R1 | Botão **HORIZON**, usando MaxBadgeButton, no cabeçalho do projeto selecionado com Horizon disponível |
| R2 | Três overlays: pendentes azul, execução verde, erros vermelho; ordem estável, zeros exibidos e dados desconhecidos distintos |
| R3 | Clique abre MaxModal com terminal real do Horizon e controles iniciar/parar |
| R4 | Limpar pendentes com confirmação, incluindo agendadas e preservando reservadas/em execução |
| R5 | Contexto por projeto/raiz/destino, sem misturar worktrees ou respostas assíncronas; MaxPinia real e helpers MaxUse |
| R6 | Supervisão sem duplicação/órfãos; fechar modal não para; fechar janela mantém runtime no tray; saída explícita usa shutdown supervisionado |
| R7 | Sem comandos arbitrários, segredos ou mutações em filas fora do alvo; falhas e resultados parciais honestos |
| R8 | Preservar busca, indicadores, abas, templates, PTYs, histórico e preferências existentes |

Usuários afetados: quem usa MaxTerminal para projetos Laravel com Horizon; outros projetos não devem receber processo PHP/polling periódico desnecessário nem alterações de comportamento.

### 2.2 Confirmado versus proposto

**Confirmado pelo usuário:** local da UI, biblioteca do botão, três cores/contadores, modal/terminal, iniciar/parar e limpar; resposta expressa à pergunta de risco: **somente pendentes, inclusive agendadas, preservando execução**. Só documentos nesta sessão, nos dois destinos autorizados.

**Decisões técnicas propostas e fundamentadas:** PTY dedicado sem aba comum; monitoramento pela raiz cadastrada do projeto, não cwd da aba; controle apenas do processo iniciado por esta integração; leitura local via helper PHP embarcado, sem exigir servidor HTTP; MaxPinia com adapter Axios restrito para IPC; limpeza específica Redis e não comandos amplos. Nomes de DTOs/IPC novos estão em spec §3.4 e precisam ser implementados, não presumidos existentes.

**Hipóteses não verificadas:** versão instalada no Laravel-alvo, disponibilidade de PHP/Redis, configurações concretas de filas/prefixos, suporte cluster e ambiente real de teste. Não se comprovou integridade/build dos artefatos locais; fontes da biblioteca não garantem artefatos publicados idênticos. Esses pontos são gates, não razões para substituir escolhas do usuário.

### 2.3 Preservações e exclusões

- Não mudar cabeçalho vazio, busca Ctrl+K, AI/Bridge/atalho/preferências ou configurações do projeto.
- Não usar terminal da aba ativa para executar comandos de controle; não criar aba/template visível apenas para mostrar o modal.
- Não instalar nada no Laravel, não ler/exibir .env integralmente, não copiar .env para fixture e não criar migration/endpoint/comando Artisan no alvo.
- Não limpar falhas, não listar payloads, não adicionar dashboard, retry de jobs, controle remoto, Sail ou adoção de processos externos.
- Sem mudanças em bibliotecas compartilhadas: se API efetiva não permitir o contrato, interromper dependentes e registrar próxima ação, em vez de editar MaxPinia/MaxUse/MaxComponentsUi fora desta worktree.

## 3. Evidências e mapa de arquivos

Os caminhos existentes abaixo foram investigados por conteúdo ou estrutura; arquivos de registro indicados como pontos de integração ainda exigem leitura integral antes da edição. Caminhos marcados **novos** são propostas, não arquivos já criados. Spec §3.1 registra comportamento e fontes oficiais, incluindo a diferença entre clear amplo e a limpeza desejada.

| Área | Existentes relevantes | Escrita prevista durante Execute |
| --- | --- | --- |
| Cabeçalho | `src/components/projects/ProjectWorkspace.vue` | Inserir componente Horizon apenas em actions com projeto |
| UI de referência | `src/components/workspace/{PaneView,TemplateEditorModal}.vue`, `src/components/terminal/TerminalView.vue` | **Novos:** `src/components/horizon/{HorizonBadgeButton,HorizonModal}.vue`; reutilizar TerminalView sem reformulação do motor |
| Stores/boot | `src/app/plugins.ts`, `src/stores/{quickCommands,pty,ports,githubIssues}.ts` | **Novo:** `src/stores/horizon.ts`; alterar plugins para injeção compatível do adapter |
| IPC | `src/ipc/core.ts`, `src/types/bindings.ts` | **Novos:** `src/ipc/{horizon,horizonMaxPiniaAdapter}.ts`; bindings gerados |
| Terminal | `src/composables/terminal/useTerminal.ts`, `src/terminal/engine/instanceRegistry.ts` | Reusar detach/dispose; modificar motor apenas se teste de contrato revelar necessidade específica |
| Domínio | `src-tauri/crates/mt-commands/src/{lib,ports}.rs`, `artisan/{root,runner,service}.rs`, `Cargo.toml` do crate | **Novo diretório:** `src-tauri/crates/mt-commands/src/horizon/` com `mod.rs`, `model.rs`, `runner.rs`, `service.rs` e helper(s) PHP embarcados; exportar módulo em lib.rs |
| Composição nativa | `src-tauri/src/{lib,specta}.rs`, `commands/{mod,quick_commands,pty}.rs` | **Novos:** `src-tauri/src/horizon/mod.rs` e `commands/horizon.rs`; registrar estado/comandos |
| Lifecycle | `src-tauri/src/adapters/{os,exact_process_signaler}.rs`, `workspace/observer.rs`; tipos e spawn de mt-pty | Alterar `adapters/os.rs` e composição para atividade/stop/shutdown do Horizon; hook de remoção do projeto no serviço, após conferir contrato de remoção |
| ACL | `src-tauri/build.rs`, `capabilities/default.json`, `permissions/default.toml`, `tests/capabilities.rs` | Registrar permissões mínimas e testes; artefatos em `permissions/autogenerated/` gerados pelo fluxo Tauri, não ACL ampla |
| Qualidade | `package.json`, `vite.config.ts`, `vitest.config.ts`, `tests/setup.ts`, `scripts/{check,sync-local-libs}.mjs`, `src-tauri/tests/bindings.rs` | Testes novos; não enfraquecer gates nem cobertura |
| Testes propostos | Estrutura `tests/components/`, `src/stores/__tests__/`, `src-tauri/crates/mt-commands/tests/` | **Novos:** `tests/components/horizon/*.spec.ts`, `src/stores/__tests__/horizon.spec.ts`, `tests/ipc/horizon*.spec.ts`, `mt-commands/tests/horizon*.rs` e `src-tauri/tests/horizon_runtime.rs` |

`tests/ipc/` acima é destino proposto: confirmar diretório/convenção antes de criar. Os nomes internos do novo módulo podem ser divididos por responsabilidade, preservando contratos e sem ampliar escopo.

### Bibliotecas e contratos consultados

- MaxComponentsUi local: `src/components/MaxBadgeButton.vue` (`modelValue`, `overlay`, click), `MaxBadge.vue` (`OverlayData = {value,color?}`), `src/themes/colors.scss` (`--blue-b-500`, azul real).
- MaxPinia local: `src/plugin.ts` e `src/types.ts`: ativação `isCached`, chave `$id.id`, getter via Axios, callbacks de dados, cache localforage, cancellation/identity checks. O boot já instala plugin, mas stores investigadas não ativam esse cache.
- MaxUse local: `src/index.ts` reexporta helpers e `Helpers/VueUse`; imports explícitos, como já ocorre em quickCommands, sem pressupor auto-import no Vite (não configurado).
- Dependências confirmadas: Vue `3.6.0-rc.13`, Pinia `3.0.1`, Axios `1.7.9`, Tauri API/CLI `2.5.0`, Vite `8.0.0`, Vitest `5.0.5` no package.json. Não atualizar versões por esta demanda.
- Fonte oficial Horizon fixada `v5.48.3` e framework branch `13.x` são referências de investigação, **não** versão detectada no alvo ou promessa de suporte universal. Revalidar código de vendor instalado e manifests, sem executar o Laravel na fase Plan.

## 4. Solução executável

### 4.1 Backend, identidade e métricas

1. Buscar projeto cadastrado pelo ProjectId; derivar raiz canônica autorizada no backend, rejeitar ausente, symlink que escape e mismatch de projeto. Não aceitar raiz livre enviada pela UI. Reutilizar RootResolver quando apropriado sem herdar a autorização ampla de worktrees irmãs do DbProjectLookup.
2. Evidência estática de framework/Horizon antes do bootstrap; confirmar autoload/pacote instalado e comandos registrados no probe. Diferenciar não instalado de falha de bootstrap/Redis. Sem disparar catálogo que grava `.max-code` no alvo.
3. Executar helper PHP constante, embarcado no aplicativo, com argv separados e cwd validado. Reusar limites e drenagem concorrente de `run_bounded` para subprocessos finitos, com budget específico; a trait ArtisanRunner não oferece execução genérica. Não estender contrato existente sem necessidade.
4. Resolver ambiente/configuração efetiva, pares únicos connection/queue e fingerprint opaco de destino. Configuração resolvida, versão e capacidades determinam compatibilidade. Não exportar credenciais/configuração sensível.
5. Contadores: ready + delayed; reserved; falhas retidas do namespace Horizon por countFailed. Não usar size() ou countPending() como pending puro. Não somar filas duplicadas. Respeitar tipos/null e overflow de inteiros no contrato TS; validar limites de serialização no backend.
6. Snapshot revisionado, diagnósticos sanitizados e status externo/gerenciado. Mesmo Redis/prefixo pode ser compartilhado entre projetos; avisar escopo real e impedir runtimes gerenciados duplicados nesse destino. Worktree não isola Redis.

### 4.2 Limpeza específica

Aplicar integralmente spec §3.6. Antes de escrever o algoritmo, fechar o contrato de Redis na versão efetiva: ready/list, delayed/sorted set, reserved/sorted set, notify/list e metadados Horizon. Desabilitar a capacidade para versão/driver/cluster não verificado, sem fallback perigoso.

- Preview produz token backend temporário e não persistente, ligado ao contexto/escopo. Token não é simples booleano de confirmação vindo da UI.
- Execução revalida contexto e consome token/requestId de forma idempotente. Mudança de configuração, raiz ou ambiente invalida confirmação; nunca autoaceitar nova fila.
- Lotes Lua pequenos retiram ready/delayed e notificações correspondentes atomicamente no Redis. Reservas concorrentes não podem ser removidas. Reconciliar apenas IDs removidos, sem varrer filas/namespace ou apagar uma nova tentativa concorrente com mesmo ID.
- Prazo/quantidade limitados, progresso por fila e resultado parcial recuperável sem repetição automática da exclusão. Não afirmar rollback global: pode haver remoção em uma fila e falha na próxima.
- Falha de metadados em conexão separada produz aviso explícito; contadores físicos continuam verdadeiros. Não repetir remoção para tentar corrigir painel. Testar crash/timeout após commit Redis e antes da resposta para impedir replay acidental.

### 4.3 Runtime e lifecycle

- Serviço nativo autoritativo em memória, com lock por raiz/destino, requestId e geração; reservar paneId exclusivo por geração antes do spawn. Sem aba sintética nem session SQLite para o daemon nesta entrega.
- Executar PHP como líder do PTY, com `artisan`, `horizon` em argv; sem shell intermediário nem input injetado. Usar mesmo resolvedor de executável/configuração entre probe e runtime para não medir um PHP e iniciar outro.
- `starting` até master identificado/saudável; `running` não decorre apenas do spawn. Erro de bootstrap/saída prematura libera corretamente lock/capacidade e mantém diagnóstico.
- Parada: sinal preciso ao master proprietário usando identidade completa (boot/PID/start_time), não kill do cgroup inteiro nem comando global de término. Aguardar sem bloquear UI; início de stop retorna estado transicional. Após limite explícito, manter pendência e oferecer nova consulta; nunca SIGKILL silencioso. Política global de saída do aplicativo continua distinta e deve avisar impacto em jobs.
- Eventos/snapshots têm contexto/geração; exit antigo não derruba runtime novo. Usar listeners existentes `pty://exit`, revisão e reconciliação para não depender só de polling nem perder saída no bootstrap.
- `TerminalView` recebe paneId/ptyId do backend, `:key` da geração; monta apenas quando modal visível. Desanexar no fechamento sem kill. Descartar instância/estado de terminal ao encerrar definitivamente geração, respeitando retenção limitada para leitura da última saída.
- Processo externo não tem stdout capturável automaticamente: modal informa isso, mantém métricas e não mostra terminal fictício. Start/stop bloqueados enquanto houver master externo compatível; usuário pode parar externamente e então iniciar pelo app. Uma falha em descobrir masters não deve habilitar início otimista.
- Incluir runtime/pending spawn em atividade do SO e tray; adicionar hook de remoção do projeto e shutdown que finalize os processos supervisionados e cancele probes/listeners. Não remover projeto e abandonar daemon vivo.

### 4.4 MaxPinia e UI

- Criar store cacheada de snapshot usando plugin real, instanciada com identidade estável por projeto/raiz. Inicialização deve resolver contexto sem loop de troca de `id` a cada snapshot; atualização de contextRevision invalida dados/ações de forma controlada. Nunca compartilhar dados vivos entre duas instâncias de contexto.
- Rota sintética do snapshot é interna ao app, não endpoint Laravel. Adapter Axios seleciona apenas essa rota e método GET, chama IPC tipado e devolve response.data no contrato do plugin. Não mudar defaults/interceptors do Axios global; delegar rotas não Horizon ao transporte original. Não tratar aborto como sucesso nem apresentar resposta IPC tardia após AbortSignal.
- Testar a instância efetivamente injetada em `createMaxPinia({axios})`, a mesma identidade de Pinia/Vue no harness e tratamento de status; `reload()` do plugin pode absorver rejeição e registrar erro, logo promise resolvida não prova leitura bem-sucedida.
- Persistir só snapshot sanitizado; extrair sessão PTY/capacidades de mutação/token para estado vivo não cacheado. Snapshot vindo de disco nunca habilita Start/Stop/Clear até reconciliação viva.
- Start/Stop/Clear são actions explícitas com IPC tipado, lock de UI e feedback. Não colocar ações operacionais em options.save nem deixar watcher/auto-save reproduzi-las.
- Helpers MaxUse para intervalo, visibilidade e backoff; uma leitura em voo, cancelamento lógico e revisão; tempos propostos spec §3.3. Suspender polling em janela oculta e contextos não exibidos; reusar snapshot válido entre badge/modal.
- Badge: label HORIZON; três `overlay` com value e color, `--blue-b-500`, `--max-success-500`, `--max-danger-500`. Tooltip/aria-label geral com rótulos e números. `modelValue` controlado pela abertura do modal, clique abre sem iniciar processo.
- Modal MaxModal, MaxButton e MaxIcon quando necessário; legenda, estado, escopo, terminal, iniciar/parar e limpar pendentes. Confirmar com segundo MaxModal no padrão acessível da biblioteca, sem window.confirm nem botões nativos novos. Escape/foco/tamanho/fit do terminal e busy states cobertos por testes.
- Vue SFC na ordem template/script setup TS/style scoped SCSS; 4 espaços, aspas simples, ponto e vírgula, tags de abertura em linha única. Imports explícitos conforme aplicação, sem inventar auto-resolver. Não criar redesign geral.

## 5. Etapas, responsáveis e gates

Os IDs T1–T6 devem ser os mesmos no registro de execução. Não há autorização implícita de delegação por este plano. Se Execute vier com delegação autorizada, um integrador assume contratos e arquivos compartilhados; somente T4/T5 podem ter frentes independentes depois de T1–T3 e bindings estabilizados, com arquivos de escrita disjuntos.

### T1 — Revalidar contexto e fechar contratos críticos

**Responsável:** integrador. **Dependência:** nenhuma.

1. Conferir branch/caminho/status, instruções locais e documentos; preservar mudanças preexistentes e não investigar outras worktrees. `.gitignore` já cobre `.worktrees/` e `.max-code-worktrees/`.
2. Por leitura, revisar manifests/scripts/prehooks, links e artefatos efetivos das três libs e resolução de Vue/Pinia/Axios/localforage. Conferir exports reais e contratos do plugin/MaxBadge/MaxModal/MaxUse. Não instalar/rebuildar bibliotecas ou bypassar gate sem autorização.
3. Identificar fixture Horizon adequada e ler metadados/contratos da versão instalada; verificar chaves/cliente/cluster, filas, retenção, nome/heartbeat do master, sinais, fast_termination e efeitos de saída do líder.
4. Provar isolamento operacional da fixture: Redis dedicado, prefixos e conexões exclusivos, cache/DB/filas/storage temporários sem apontar a serviços reais; considerar providers executados no bootstrap. Não basta worktree, APP_ENV=testing ou transação.
5. Fechar DTOs/códigos de erro/graus de suporte e revisão de riscos de exclusão/sinalização. Se falta fixture, implementar testes puros pode prosseguir após contrato documental, mas validação real fica manualPending e exclusão sem contrato confirmado não é habilitada.

**Conclusão:** mapa de suporte/contratos e ambiente seguro documentados no handoff; dúvidas reais que mudem o pedido voltam ao usuário. Não começar código dependente de contrato incompatível.

### T2 — Implementar métricas e limpeza com testes

**Responsável:** domínio backend. **Depende:** T1.

Criar módulo `mt-commands/src/horizon/`, DTOs, runner/probe/helper(s), autorização contextual, descoberta cacheada/limits, medição e preview/clear exclusivo. Testar com portas/dublês injetáveis e fixtures de saída/Redis controladas; testes reais somente após isolamento. Exportar módulo sem alterar contrato Artisan existente desnecessariamente.

**Conclusão:** todos os fluxos e testes escritos; revisão factual do algoritmo, inclusive chaves mutadas, concorrência, notify, falhas parciais e ausência de payload/segredo fora do helper. Essa revisão é gate por leitura, não bateria entre microedições.

### T3 — Implementar runtime e integração operacional

**Responsável:** integrador nativo. **Depende:** T1/T2.

Criar serviço em `src-tauri/src/horizon/`, start/stop com PTY e identidade, reconciliação de saída/heartbeat, lock/idempotência, hooks de projeto/tray/shutdown. Ajustar composição `lib.rs`, `adapters/os.rs` e pontos de remoção conforme contrato existente. Escrever testes de lifecycle incluindo cleanup-failed/timeout, cliques concorrentes, mesmo destino em worktrees, PID reciclado, saída antiga e parada durante job longo.

**Conclusão:** não há controle sobre processo externo nem órfão após saída/remoção; fechar modal não interfere. Revisão de risco de sinalização/encerramento antes de integrar UI.

### T4 — Expor IPC e conectar MaxPinia

**Responsável:** integrador de contratos/frontend estado. **Depende:** T2/T3.

Registrar comandos em `commands/mod.rs`, `specta.rs`, `build.rs`, permissões/default e capability foundation. Atualizar testes ACL e bindings, gerar `src/types/bindings.ts` pelo script confirmado apenas nesta fase Execute. Criar wrappers IPC, adapter Axios restrito, store MaxPinia e testes com plugin instalado de verdade, storage isolado e Axios injetado.

**Conclusão:** contratos gerados coerentes; GET passa pelo plugin e mutações somente por clique; cache não restaura runtime, não há rota arbitrária e o transporte antigo permanece intacto.

### T5 — Integrar badge/modal e testes de interface

**Responsável:** frontend UI. **Depende:** DTOs e store T4 estabilizados.

Criar dois componentes Horizon e integrar cabeçalho. Reutilizar TerminalView e identidade por geração. Escrever testes de disponibilidade, overlays/cores/zeros/desconhecido, modal/foco, inicia/para/limpa/cancela, erro/stale, mudança de projeto, ausência de anexação externa e reabertura sem novo daemon. Não mexer em outras ações do cabeçalho.

**Conclusão:** escopo inteiro e testes de T2–T5 implementados antes da suíte completa. Revisar diff e requisitos; não validar enquanto frentes ainda editam arquivos dependentes.

### T6 — Validação agrupada e aceite real

**Responsável:** integrador/revisor. **Depende:** T2–T5 completos.

Executar gates da seção 6; diagnosticar falhas em lote, corrigir e validar novamente a revisão afetada. Reutilizar checks aprovados da mesma revisão, sem alegar aprovação global se suíte interrompeu ou aceite real pendente. Revisão independente nos marcos de risco e entrega quando houver executor disponível/autorizado; do contrário registrar revisão manual explícita sem inventar outro revisor.

**Conclusão:** matriz de aceite atendida por evidências atuais, nenhum check vigente falho/ausente e aceite nativo real registrado. Fixture indisponível implica manualPending, não entrega integral.

## 6. Validação e matriz de aceite

### 6.1 Comandos confirmados, exclusivamente futuros

- `npm run bindings`: geração explícita de bindings, com efeitos de escrita; Execute somente após novos comandos/DTOs e gate de ambiente.
- `npm run check`: fluxo canônico de dez etapas de `scripts/check.mjs`: `node scripts/sync-local-libs.mjs --check`, `npm run lint`, `npm run lint:style`, `npm run typecheck`, `npm run typecheck:vue`, `npm run test:coverage`, `npm run build`; em `src-tauri`, `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --workspace --locked`.
- `npm test` existe, mas não substitui suíte/cobertura/gates canônicos. Não rodar comando direto para contornar falha do primeiro gate, ignorar configuração, remover prehook ou excluir teste/cobertura.
- `npm run check` interrompe no primeiro erro. Corrigir causa e executar etapas necessárias para cobrir toda revisão; não omitir partes que não chegaram a executar. Sem loop cego de repetição.
- Nenhum desses comandos foi executado durante Plan. Não há resultado de tipos, builds, cobertura, PHP/Redis ou aceite nativo a reportar.

### 6.2 Matriz por tarefa

| Tarefa/requisitos | Evidência exigida | Testes pertinentes futuros | Revisão pelo risco | Verificação manual |
| --- | --- | --- | --- | --- |
| T1 / R5,R7 | Raiz/versões/exports/resolução e isolamento comprovados por leitura | Harness real de plugin e gate de libs na validação agrupada | Alto: configuração efetiva não é inferida de .env=testing | Conferir destinos da fixture sem registrar credenciais |
| T2 / R1,R2 | Instalação real versus manifest; snapshots de filas e falhas | Ausência de pacote, PHP, Redis; bootstrap ruidoso; timeout/overflow; filas duplicadas; pending sem reserved; falhas respeitam trim; null e dados parciais | Médio/alto: métricas não podem fabricar zero ou usar size() | Sem Horizon, Horizon parado e ativo; job pendente, agendado, reservado e falho |
| T2 / R4,R7 | Chaves e membros antes/depois; resultados por fila e token | Confirmação vencida/cancelada; fila não autorizada; corrida com pop/release; preservação byte a byte de reserved, failed e outra fila; notificações; limite; timeout após commit e falha de metadados | Crítico: revisão do Lua e prefixos; nenhuma operação ampla | Limpar somente fixture isolada, com job longo comprovadamente em execução, agendado e fila vizinha; comprovar conclusão do job preservado |
| T3 / R3,R5,R6 | Runtime/ProcessIdentity/geração e contagem de processos | Duplo start; conflito de destino; PID reciclado; stop antigo; erro de spawn; stop lento; saída do master antes dos workers; cleanupFailed | Alto: sinal apenas ao master proprietário | Terminal mostra stdout real; fechar/reabrir; trocar projeto; parar com job; fechar janela/tray; sair/remover projeto sem órfãos |
| T4 / R5,R7,R8 | Store ativa no plugin e IPC/ACL coerentes | Axios adapter real injetado; cancelamento; duas identidades; cache corrupto/stale; retorno antigo; reload com erro; dados sensíveis não persistidos; rotas comuns preservadas; ACL/bindings | Alto: plugin global e cache não autorizam comandos | Reabrir app e verificar que cache não cria daemon nem habilita controle sem leitura viva |
| T5 / R1,R2,R3,R4,R8 | DOM real dos componentes e sequência de ações | Overlays corretos inclusive zero, cores, unknown/stale, modelValue/Escape, foco, botões busy, confirmação cancelada, mudança de contexto, componente de terminal com geração | Médio: acessibilidade e regressão do cabeçalho | Light/dark, janela estreita, teclado, foco e redimensionamento do terminal |
| T6 / R1–R8 | Logs/códigos reais por revisão, diff final e matriz atendida | Suíte canônica inteira; regressões existentes | Revisão integrada e de segurança | Aceite nativo Linux/Tauri e fixture real, não apenas browser mock |

### 6.3 Procedimento seguro do aceite real

Após isolamento comprovado e autorização operacional no Execute, usar fixture Laravel/Horizon descartável com Redis exclusivo e jobs inofensivos: um pronto, um agendado, um longo já reservado, um falho e uma fila fora do escopo. Não inventar comando Artisan de seed; identificar fixture e mecanismo existentes ou propor uma fixture de teste específica para aprovação antes de iniciar serviços.

Registrar números/escopo e chaves relevantes de forma sanitizada; iniciar pelo modal, observar saída e evolução dos overlays, limpar com confirmação, verificar preservação da reserva/falhas/fila vizinha e execução concluída do job longo. Exercitar stop sem força, reabrir modal, tray e shutdown. Não iniciar serviços reais nem tocar produção para compensar ausência de fixture.

Dublês de Redis/PTY/Tauri provam lógica, não comportamento da versão instalada ou execução nativa. O teste existente `mt-commands/tests/laravel_integration.rs` é ignorado por padrão, depende de `MT_LARAVEL_FIXTURE` e só cobre descoberta Artisan; não prova Horizon nem serve de autorização para mutação. Falta de fixture/display/permissão mantém T6 `manualPending` com próxima ação concreta.

## 7. Riscos, operação e recuperação

- **Exclusão irreversível:** não há rollback de jobs retirados. Confirmação precisa, escopo explícito, orçamento e resultado parcial; não prometer restauração por reverter código.
- **Namespaces compartilhados:** não presumir isolamento por nome do projeto/branch. Fingerprint/conflito/aviso; usuário deve ver o destino antes de limpar.
- **Compatibilidade:** novos adapters só habilitam capacidade quando contrato da versão/driver validado. Suporte parcial fica declarado e bloqueia aceite do caso correspondente; sem ocultar limitação usando clear amplo.
- **Metadados não transacionais entre conexões:** reconciliação por IDs e aviso/pendência; manter contagem física e não apagar reserva/histórico para corrigir visualmente.
- **Bootstrap de terceiros:** providers PHP podem escrever logs/cache ou acessar serviços mesmo em consulta; Plan não os executa. Execute exige ambiente seguro para teste e trata projeto autorizado como fronteira de confiança, não sandbox completo.
- **Stop demorado:** não confundir `KillMode::Graceful` com drenagem de jobs. Só declarar parado após reconciliação real; não escalar força silenciosamente. Saída global segue política do aplicativo com aviso.
- **UI/terminal:** um consumidor por PTY, buffer limitado, IDs únicos por geração, descarte no fim sem matar ao fechar; evitar vazamento GPU/listeners/polling.
- **Observabilidade:** diagnósticos com códigos/operação/requestId/contexto opaco, duração e totais, nunca payloads/credenciais. Não persistir saída nova automaticamente; se futuramente persistida, sanitizar antes do disco.
- **Reversão de software:** desabilitar/remover integração via diff autorizado e encerrar runtime proprietário com segurança antes; não alterar filas/ambiente para reverter código. Sem migration nesta demanda.

## 8. Registro de progresso e handoff durante Execute

**Não criar nesta sessão Plan.** No Execute, usar `docs/plans/wt-1b4b2671/execution-state.json` após marcos e no handoff, associado ao SHA256 real dos bytes de `plan.md` e às revisões factuais das tarefas. Não incluir placeholders de hash nem usar nomes/labels como revisão.

Contrato **version 2 estrito**, sem chaves extras:

- Raiz: `{version: 2, planRevision, tasks, verification, updatedAt?, nextAction?}`. Nunca `schemaVersion`; não adicionar `worktree`, `branch`, `head`, `planPath`, `status`, `checks` solto, `blocker`, `diagnostic`, `branchSync`, `durableSync` ou `followUp`.
- `planRevision`: SHA256 real de plan.md. `tasks`: lista com T1–T6. `updatedAt`, se presente: inteiro positivo em milissegundos, não data ISO.
- Tarefa: `{id, status, files, validations, revision?, nextAction?}` apenas. `validations: []` sempre obrigatório. Proibidos `title` e `evidence`. Status permitido: `pending`, `running`, `completed`, `blocked`, `manualPending`, `validationFailed`.
- `files`: caminhos relativos envolvidos; `revision`, quando presente: SHA256 real do conteúdo factual da tarefa. Definir representação determinística dos arquivos/conteúdos e registrar o mesmo procedimento no handoff para revalidar; não usar nome de branch como hash.
- `validations`: lista de resultados com valores estritos `passed|failed|pending`, sem introduzir checks detalhados fora de `verification.checks`. Resultado `blocked` é proibido; motivo pertence a `nextAction`. Se houver contrato do harness mais específico para cada entrada, conferir por leitura antes de gravar; não inventar campos adicionais.
- `verification`: `{revision, checks}`; `revision` é SHA256 real do conjunto de conteúdo verificado, não rótulo. Cada check somente em `verification.checks`: `{command, revision, result, exitCode?}`. Não adicionar campos extras.
- `command`: comando realmente executado, até 4.096 caracteres. `revision`: SHA256 real da revisão factual verificada. `result`: exclusivamente `passed|failed|pending`.
- `passed` exige `exitCode: 0`; `failed` exige inteiro diferente de zero; `pending` não aceita exitCode. Nunca `exitCode: null`. Resultado não é inferido de `tool:end.ok`; registrar código real.
- `nextAction`, em qualquer nível permitido, até 512 caracteres com motivo e ação concreta; sem campo separado de impedimento. Registrar todas as falhas, inclusive suíte completa que parou antes do fim.

Checks vazios, resultado não vigente, falha vigente ou verificações omitidas impedem validação integral. Version 1 permanece compatível sem provar validação integral automaticamente. O JSON é autodeclarado: não prova execução nativa nem detecta sozinho verificações omitidas. Revalidar evidências e conteúdo, não apenas o status escrito.

Agrupar diagnóstico do harness/bootstrap antes de repetir testes equivalentes; reaproveitar verificações aprovadas da mesma revisão. Para tarefas assíncronas, aguardar eventos de mudança/conclusão com espera limitada e recuo, sem consultar status repetidamente sem informação nova. Cancelamento conserva trabalho parcial e pendência. Mesmo erro repetido sem progresso exige investigar causa antes de reexecutar.

## 9. Revisão final do plano

- Todos os itens solicitados mapeiam a requisitos, tarefas e matriz de aceite.
- Decisão de limpeza exclusiva confirmada e protegida contra comandos padrão incompatíveis.
- Escopo existente preservado; integrações MaxComponentsUi/MaxPinia/MaxUse baseadas em fonte real, com gate de artefatos no Execute.
- Riscos críticos explicitados: versões/Redis ainda não conhecidos, reservas versus execução, falhas por namespace, controle somente de processo próprio, tray para PTY sem aba e metadados entre conexões.
- Sem execução intermediária, instalação, script de preflight, teste, alteração de código ou serviço na sessão Plan. Escritas limitadas a spec.md e plan.md nesta worktree.
- Execução integral só poderá ser declarada após suíte completa vigente e aceite real; impedimento/manualPending exige ação concreta, não conclusão total.
