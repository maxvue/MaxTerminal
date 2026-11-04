# Plano de execução — Restaurar projeto e aba selecionados

## Identificação e admissão

- Projeto: MaxTerminal; data: 04/11/2026; worktree/branch: `wt-bb0b4617`.
- Raiz validada por `pwd` e `git rev-parse --show-toplevel`: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-bb0b4617`.
- Requisito: ao fechar e abrir o MaxTerminal, persistir o projeto selecionado e a aba visível.
- Especificação obrigatória: ler `docs/plans/wt-bb0b4617/spec.md` na primeira admissão. Ela define os contratos, precedências, fallbacks e limites deste plano.
- Esta é uma entrega de Plan Mode: nenhuma implementação, teste ou alteração operacional foi executada. Estado inicial do Git estava limpo. Somente documentos autorizados foram criados.
- Executar futuramente exclusivamente nesta worktree; não criar outra, não trocar branch, não editar bibliotecas ligadas por symlink, não fazer commit/merge/push nem remover a worktree. Integração é do MaxCode/usuário.

## 1. Requisitos e preservações

| ID | Requisito observável |
|---|---|
| R1 | Reabrir no projeto anteriormente selecionado, sem depender da velocidade de hidratação. |
| R2 | Reabrir na aba terminal anteriormente visível; lembrar aba por projeto e verificar pertencimento. |
| R3 | Reabrir em Issues se era a aba visível; conservar aba terminal de retorno. |
| R4 | Capturar a última escolha mesmo ao fechar antes dos debounces existentes. |
| R5 | Suportar IDs inválidos/removidos, falha de restauração e falha de armazenamento sem travar ou abrir abas extras. |
| R6 | Não regredir bandeja, restauração de terminais, templates, foco externo, badges/vistos, projeto oculto ou fallback vizinho. |
| R7 | Usar MaxPinia para cache/sincronização, MaxUse para helpers e MaxComponentsUi para eventual feedback. |

Usuários afetados: todos que retomam o aplicativo, inclusive projetos com várias abas e projetos GitHub. Não adicionar opção de preferências ou mudar o layout. Não persistir modais, dados de issues ou conteúdo dos terminais.

## 2. Evidências e diagnóstico

| Arquivo existente | Evidência relevante |
|---|---|
| `src/stores/projects.ts` | `selectedId`, debounce 200 ms, leitura prematura de settings em `init`, `activeTabByProject` volátil; `dispose` cancela timer. |
| `src/stores/settings.ts` | `hydrate`, indicador `hydrated`, `set` retorna booleano, revisão otimista e um retry de conflito. |
| `src/app/App.vue` | Hidrata settings após bootstrap; layout pode iniciar no intervalo. |
| `src/components/layout/AppLayout.vue` | Inicia catálogo/workspace em paralelo e reconcilia vínculos ao fim. |
| `src/stores/tabs.ts` | Hidratação seleciona primeira aba; `focusTab` atualiza localmente antes do IPC e reaplica resposta. Resposta antiga exige proteção. |
| `src/composables/workspace/useWorkspaceEvents.ts` | Buffer de eventos, bootId/eventSeq e resync que precisam ser preservados. |
| `src/stores/githubIssues.ts`, `src/components/workspace/TabWorkspaceView.vue` | Seleção de Issues separada, resolução assíncrona do repositório e retorno a terminal. |
| `src/composables/useActiveProjectSync.ts` | Seleção externa por bridge, com prevenção de eco. Não deve ser substituída por preferência atrasada. |
| `src/stores/window.ts`, `src-tauri/src/lib.rs:573–589` | Quit e fechamento nativo existentes; não garantem flush de debounce frontend. |
| `src-tauri/src/workspace/restore.rs:66–127` | Abas abertas restauradas com IDs preservados, depois templates complementares. |
| `src-tauri/src/workspace/manager.rs:1518–1569` | `tab_focus` persiste painel e marca visto; não invocar apenas para hidratar seleção. |
| `src/app/plugins.ts`, `src/ipc/horizonMaxPiniaAdapter.ts`, `src/stores/horizon.ts` | Padrão concreto de MaxPinia com Axios adapter para IPC, inicialmente limitado ao Horizon. |
| `src-tauri/crates/mt-db/src/settings/registry.rs`, `settings/repo.rs` | Namespace livre `ui.*`, default nulo para chave ausente. |
| `src/types/projectSettings.ts`, `src/types/settings.ts` | Esquema frontend e defaults filtram as chaves disponíveis ao settings store. |
| `package.json`, `vitest.config.ts`, `tests/setup.ts`, `scripts/check.mjs` | Scripts reais, happy-dom, mocks Tauri, aliases/dedupe e gate agregado. |

Testes relacionados inspecionados: `tests/frontend/projects/projects.store.spec.ts` e `tests/frontend/workspace/tabs.store.spec.ts`. Diretórios de testes de bootstrap, plugins, componentes de workspace e eventos também foram localizados. Não foram executados.

Bibliotecas efetivas lidas em `node_modules/@maxvue`: MaxPinia 0.2.0, MaxUse 2.0.1 e MaxComponentsUi 1.1.36. Leitura não comprova integridade de todos os artefatos nem identidade de módulos em runtime. Documentação Context7 de Pinia consultada, mas resultado no ramo v4 não substitui a versão 3.0.1 declarada no projeto. Fonte local prevalece para os contratos utilizados.

Não foi necessária pergunta: o resultado solicitado é concreto. Inclusão de Issues decorre de ser uma aba visível existente, não de uma nova funcionalidade GitHub.

## 3. Decisões e contratos estabilizados

1. **Proposta de implementação:** store dedicada `workspaceSelection`, com MaxPinia ativo, objeto versionado e uma única configuração `ui.workspace.selection`. Contrato completo em `spec.md`.
2. **Fonte durável:** SQLite existente, via settings store. Fonte de recuperação antes do auto-save: pequeno snapshot pendente síncrono via `useStorage` do MaxUse. Cache do MaxPinia é auxiliar, não prova de confirmação SQLite.
3. **Precedência de boot:** pendente local válido → SQLite válido → `projects.selected_id` legado → ausência. IDs só são aplicados após verificar catálogo/workspace. Não usar datas de relógio como comparação de versões.
4. **Captura:** ação de seleção captura snapshot completo sincronamente; mutations têm ID opaco. Na inicialização, supressão explícita de captura de escolhas provisórias. Alteração do usuário/foco externo durante boot tem precedência sobre hidratação.
5. **Persistência:** MaxPinia agenda o auto-save; adapter serializa IPC, valida resultados e limpa pendente somente se a confirmação corresponder ao `mutationId` ainda presente. Não introduzir watcher de gravação backend paralelo ao plugin.
6. **Compatibilidade:** remover writer de debounce `projects.selected_id`, manter leitura legada e chave armazenada. Não migrar ou alterar tabelas.
7. **Reconciliação:** coordenador de navegação aplica seleção uma vez com gates explícitos. Não chamar `focusTab` para restaurar destaque; não marcar resultados como vistos sem apresentação existente.
8. **UI:** nenhuma nova aparência necessária. Feedback de erro sanitizado usa infraestrutura existente MaxComponentsUi, sem adicionar componente artesanal.
9. **Fallbacks:** projeto inexistente no boot → nenhum; aba inexistente → primeira válida; sem abas → vazio; projeto oculto continua elegível; remoções durante execução mantêm vizinho. Falha transitória na identificação GitHub não apaga intenção Issues.
10. **Preservação:** não reabrir debates sobre restauração de processos, bandeja, organização de abas ou sincronização MaxCode. Não introduzir seleção persistida por posição/índice, título, template ou PTY.

Alternativas descartadas: persistir só ao sair (fechamento nativo pode preceder gravação); apenas debounce (perde última escolha); só cache assíncrono (mesmo risco); criar/restaurar novas abas para focar (IDs já são restaurados); plugin genérico novo de persistência (MaxPinia/MaxUse disponíveis); alterar o Rust para uma preferência exclusivamente visual (IPC de settings já atende).

## 4. Etapas executáveis

### T1 — Preflight e revisão dos contratos por leitura

**Arquivos:** documentos deste plano; `AGENTS.md`; `package.json`/lock; `scripts/check.mjs`, `scripts/sync-local-libs.mjs`; configs Vitest/Vite; plugins, adapters e fontes efetivas das três bibliotecas.

- Conferir alterações locais e trabalho já feito. Não sobrescrever trabalho de terceiros.
- Conferir scripts/prehooks, caminhos reais dos symlinks, artefatos exportados e identidade de Vue/Pinia/Axios no harness. O Vitest deduplica Vue/Pinia; conferir Axios e cache LocalForage efetivos também.
- Confirmar `useStorage` com serialização e `flush: 'sync'`, e o ciclo real do plugin: cache pode atribuir `data` diretamente, `onServerData` só rege GET; `saveInServer` captura erros no status e pode resolver sem sucesso.
- Conferir leitura da chave livre e limites do repositório de settings. Confirmar que a preferência pode ser lida sem novo comando ou migração.
- Definir isolamento verificável de SQLite, cache WebView/LocalForage/localStorage e diretórios de dados para testes nativos. Não presumir isolamento só por estar em worktree; não copiar `.env`, usar banco pessoal ou resetar dados.
- Caso bibliotecas estejam inconsistentes, registrar bloqueio e próxima ação. Não contornar via comando direto, remoção de hook ou `ignoreConfig`. Conserto externo exige autorização e está fora desta execução.

**Conclusão:** contratos confirmados por leitura, plano de isolamento e lista fechada de arquivos. Sem executar testes entre leituras.

### T2 — Persistência e recuperação da seleção

**Novos:** `src/types/workspaceSelection.ts`, `src/stores/workspaceSelection.ts`, `src/ipc/workspaceSelectionMaxPiniaAdapter.ts`.
**Existentes:** `src/types/projectSettings.ts`, `src/app/plugins.ts`.
**Testes novos:** `tests/stores/workspaceSelection.spec.ts`, `tests/ipc/workspaceSelectionMaxPiniaAdapter.spec.ts`. Ampliar `tests/app/plugins.spec.ts`.

- Definir contrato/normalização pura e defaults. Adicionar `ui.workspace.selection` ao esquema frontend, sem editar bindings gerados.
- Criar Setup Store com `isCached`, `data`, defaults, rota GET/POST e estado de inicialização/erro. Não importar stores de domínio circularmente.
- Implementar snapshot pendente síncrono e validado via MaxUse. Escrita precisa acontecer no fluxo da ação antes de depender de await, microtask ou debounce.
- Adapter converte exclusivamente a rota interna proposta em IPC de settings; GET reavalia pendente depois da espera. POST serializa writes e confirma ID, respeita falhas/revisões e preserva pendente novo.
- Compor o adapter com Horizon sem mudar sua rota ou comportamento de cancelamento. Nunca enviar a rota interna para rede por fallback acidental.
- Proteger estado de navegação de atribuição tardia de cache e GET: manter snapshot/intenção atual separados do `data` reidratável, reconciliando por geração e pendente. Uma mutação durante carga deve ser reapresentada ao auto-save após o gate/pause do plugin; não pode se perder porque `pauseSave` estava ativo.
- Falha no armazenamento síncrono ou no SQLite não é sucesso silencioso; preservar seleção em memória e informar degradação com mensagem sanitizada. Falha de banco não autoriza recuperação destrutiva.
- Testar contrato vazio/corrompido/versão desconhecida, migração, precedências, cancelamento, conflitos, gravações fora de ordem, cache tardio e status real de save. Harness de integração deve instalar plugin em app/Pinia e simular armazenamento isolado; só `setActivePinia(createPinia())` não prova o ciclo do plugin.

**Conclusão:** bloco completo e seus testes escritos; revisão por leitura da captura síncrona e ordenação de gravações antes de integrar consumidores. Não rodar suíte a cada arquivo.

### T3 — Integrar seleção e restauração coordenadas

**Novo:** `src/composables/workspace/useWorkspaceSelection.ts`.
**Existentes:** `src/stores/projects.ts`, `src/stores/tabs.ts`, `src/stores/githubIssues.ts`, `src/components/layout/AppLayout.vue`; `src/app/App.vue` somente se necessário para controlar montagem/gate.
**Testes:** ampliar stores/componentes citados na spec; novo `tests/frontend/workspace/useWorkspaceSelection.spec.ts`.

- Gate de settings deve terminar com sucesso antes da leitura legada/seleção; impedir que `projects.init` restaure preferência prematura de modo independente.
- Manter listeners de projeto/workspace e buffer de eventos. Ao fim de ambos os snapshots válidos, vincular todas as abas antes de validar e aplicar mapa ativo persistido. Não guardar primeira aba provisória como escolha do usuário.
- Centralizar captura em ações de domínio e observar somente o necessário para orquestração; todas as entradas existentes de seleção, abertura/foco/fechamento de abas e Issues precisam passar pelo contrato sem mudar assinaturas públicas desnecessariamente.
- Distinguir alteração explícita de hidratação/fallback. Atualizar o snapshot completo numa única operação lógica; não registrar misturas transitórias de projeto novo e aba do projeto anterior.
- Preservar as seleções dos outros projetos e suprimir no-op. Limpeza de IDs removidos somente depois de snapshots válidos.
- Proteger `focusTab` contra resposta atrasada: resultado ainda pode atualizar dados da aba, mas não deve torná-la ativa se uma intenção de foco mais nova a substituiu. Aplicar proteção equivalente em chamadas de abertura que focam e no foco externo.
- Integrar aba Issues sem apagar intenção durante resolução assíncrona; resolução antiga de outro projeto não altera a vista atual. Não persistir resultados de rede nem filtros.
- Dispor watchers/listeners ao desmontar; retries e recuperação não reinstalam listeners duplicados nem sobrescrevem interação posterior.
- Completar os testes de todas as permutações de boot, falhas e regressões da matriz abaixo antes da validação completa.

**Conclusão:** todo escopo R1–R7 e seus testes implementados; revisão por risco de boot, cache, captura, foco assíncrono e ausência de novas chamadas que marquem visto.

### T4 — Validação automatizada em lote

- Revalidar integridade e isolamento antes de executar. `npm run check` é o comando agregado confirmado; seu primeiro passo é `node scripts/sync-local-libs.mjs --check`, seguido de lint, Stylelint, tipos TS/Vue, Vitest com cobertura, build, fmt/clippy/test Rust.
- Examinar antes os testes nativos invocados pelo agregado e sua configuração de dados. Se qualquer etapa depender de recurso compartilhado sem prova de isolamento, parar antes dela e registrar pendência; não declarar gate completo aprovado.
- Executar após T2+T3 e todos os testes, não entre microalterações. Não repetir `npm test`, tipos/build/cobertura já cobertos por um agregado aprovado da mesma revisão.
- Se falhar, agrupar diagnóstico do harness/bootstrap e falhas por causa. Corrigir o conjunto autorizado e só então revalidar os checks afetados e o gate final necessário. Repetição sem progresso exige diagnóstico, não loop de comandos.
- Registrar comando real, revisão factual e exit code observado, inclusive falhas da suíte completa. Erro de baseline não autoriza correção fora de escopo.

**Conclusão:** checks vigentes aprovados ou tarefas `blocked`/`validationFailed` com próxima ação, nunca aprovação parcial apresentada como integral.

### T5 — Aceite nativo e entrega

- Usar somente perfil/diretórios isolados comprovados em T1 e projetos/terminais de teste descartáveis. Execução nativa será manual até existir procedimento seguro confirmado; não inventar flag/endpoint de isolamento.
- Selecionar projeto não inicial e sua segunda/terceira aba; encerrar efetivamente e reabrir. Repetir com troca e fechamento imediato antes de 200/300 ms.
- Repetir em Issues, depois voltar ao terminal esperado; testar outra ordem de projetos/abas.
- Separar caso de saída real do caso de esconder na bandeja: neste último, reapresentar deve manter seleção e processos existentes, sem boot/recriação extras.
- Confirmar fallback após fechar aba ativa/última, remoção de projeto em catálogo de teste, preferência legada e ausência/corrupção da nova preferência em storage isolado.
- Confirmar foco externo e que restaurar destaque não adiciona marcação de conclusão como vista nem reexecuta comando inicial indevidamente.
- Evidências manuais descrevem passos, resultado e revisão; sem logs pessoais ou segredos. Se não houver acesso ao binário/perfil isolado, T5 fica `manualPending` com procedimento e próxima ação concretos.
- Revisão final do diff e dos requisitos; entregar resumo e pendências. Não commitar, integrar ou limpar worktree.

## 5. Matriz de aceite por tarefa

| Tarefa / requisito | Evidência exigida | Testes pertinentes | Revisão pelo risco / manual |
|---|---|---|---|
| T1 / R7 | Exports, versões efetivas, identidade de módulos, isolamento documentados | Gate de integridade do agregado em Execute | Bloquear dependentes se biblioteca inválida; não editar repositório externo. |
| T2 / R1,R4,R7 | Snapshot completo pendente antes de await; confirmação SQLite correlacionada | Store + adapter + plugins; save falha/throw/conflito/aborto; journal novo não apagado por ACK velho | Revisar chamada síncrona efetiva e rota sem tráfego HTTP; fechar imediatamente no app nativo. |
| T3 / R1,R2 | Mesmo projeto e aba não iniciais em instância nova | Settings/catalog/workspace em todas as ordens; cache/GET atrasados; resync; seleção durante boot | Conferir uma só aplicação inicial, sem escolher aba por índice; aceite nativo. |
| T3 / R3 | Issues restaurada após resolução GitHub e terminal de retorno preservado | Resolução lenta, erro transitório, não GitHub, troca de projeto durante request | Não apagar preferência em erro transitório; aceite Issues nativo. |
| T3 / R5 | Fallback determinístico, sem abas novas nem travamento | Projeto excluído, aba excluída/outro dono, snapshot vazio, armazenamento inválido/indisponível | Não limpar referência em carregamento incompleto; conferir recuperação sem reset. |
| T3 / R6 | Comportamentos existentes preservados | Aba vizinha/última, reordenação, abertura template/background, split, foco fora de ordem, projeto oculto, bridge | Não criar chamada de `tab_focus` só para boot; preservar buffer de eventos e badges. |
| T4 / todos | Gate automatizado vigente e códigos de saída reais | `npm run check` com ambiente previamente verificado | Revisar falhas em lote; checks omitidos não são aprovação. |
| T5 / R1–R6 | Encerrar/reabrir e bandeja testados separadamente na revisão entregue | Procedimento manual acima, não substituído por mocks | Sem ambiente nativo isolado: `manualPending`, sem conclusão integral. |

## 6. Riscos, operação e reversão

- A proposta adiciona somente uma preferência de UI e um snapshot local pequeno, sem migração. Sanitização/limites existentes do settings continuam ativos. Não duplicar conteúdo de terminal, caminhos ou dados de autenticação.
- O journal síncrono fecha a janela do debounce em funcionamento normal do storage; não garante flush físico de disco ou disponibilidade do WebView. Se localStorage falhar, não afirmar garantia de fechamento imediato.
- Uma instância principal por perfil é a premissa. Não resolver conflito entre várias instâncias por timestamp; se o projeto permitir múltiplas instâncias escritoras no mesmo perfil, reavaliar contrato antes de implementar dependentes.
- A serialização deve ser local à rota de seleção para não bloquear Horizon. Cancelamento lógico não pode permitir reordenação de IPC já iniciado.
- Escritas de navegação são pequenas e agrupadas; não observar scrollback/layout inteiro, não usar polling e não regravar snapshot a cada evento de PTY.
- Se falhar leitura/configuração no boot, preservar painéis de erro/recuperação e permitir retry. Não esperar infinitamente por `hydrated` sem tratar falha e desmontagem.
- Reversão futura remove a integração/novo reader e ignora nova chave, sem limpar banco ou caches globais. A chave antiga preservada pode representar seleção anterior à migração; não prometer retorno exato após downgrade.
- Achado fora de escopo registrado em `docs/notifications/wt-bb0b4617/notifications/tab-focus-marks-seen.md`; este plano apenas evita ampliar o efeito.

## 7. Retomada, progresso e contrato de execução

- Na primeira admissão ler plano/spec e instruções. Nas retomadas ler só trechos pertinentes, conferir trabalho já realizado, diff local e revisão dos arquivos, executar lacunas e manter decisões sem reabertura sem evidência nova.
- Não há delegação autorizada neste planejamento. Se futuramente autorizada: contratos T1/T2 devem estar estáveis antes de consumidores T3; separar escrita de arquivos. T2 e T3 compartilham contratos e não são independentes. Repassar referências, objetivo, revisão, arquivos e critérios sem exigir releitura integral em toda microtarefa.
- Registrar progresso por marcos em `docs/plans/wt-bb0b4617/execution-state.json` **somente durante Execute**, nunca nesta sessão Plan. Esse registro é o único novo destino de progresso previsto para Execute e requer o modo/autorização correspondente.
- Usar formato estrito version 2, sem campos adicionais:
  - Raiz: `{version: 2, planRevision, tasks, verification, updatedAt?, nextAction?}`.
  - Tarefa: `{id, status, files, validations, revision?, nextAction?}`; IDs T1–T5; `validations: []` é obrigatório, podendo permanecer vazio e manter checks em `verification`.
  - `verification`: `{revision, checks}`.
  - Check, somente em `verification.checks`: `{command, revision, result, exitCode?}`.
- `planRevision` é SHA256 real dos bytes de `plan.md`. Revisões factuais são SHA256 reais do conteúdo verificado, não nomes de branch, rótulos ou hashes fictícios. Para múltiplos arquivos, documentar no handoff a composição determinística usada (paths relativos ordenados e conteúdo), sem criar campos extras no JSON.
- Status de tarefa: `pending`, `running`, `completed`, `blocked`, `manualPending`, `validationFailed`. Resultado de validação/check: somente `passed`, `failed`, `pending`; `blocked` não é resultado.
- `passed` exige `exitCode: 0`; `failed` exige inteiro diferente de zero; `pending` não aceita `exitCode`, nem `null`. `updatedAt`, se usado, é inteiro positivo em milissegundos; `nextAction` até 512 caracteres e comandos até 4.096.
- Proibidos: `schemaVersion`, `title`/`evidence` em tarefa, `worktree`/`branch`/`head`/`planPath`/`status`/`checks` soltos na raiz e campos `blocker`, `diagnostic`, `branchSync`, `durableSync`, `followUp` em qualquer nível. Motivos cabem em `nextAction`.
- Registrar todos os comandos de verificação realmente executados e todas as falhas, sem inferir exit code a partir de sucesso da ferramenta. Checks vazios, falha vigente ou ausência de resultado vigente impedem validação integral. Compatibilidade com version 1 não comprova aprovação automaticamente.
- Registro autodeclarado não prova execução nativa nem identifica checks omitidos. Handoff deve manter pendência manual real, mesmo com testes aprovados. Cancelamento preserva resultados parciais e próxima ação. Aguardar tarefas assíncronas por notificação quando disponível, sem polling repetitivo.

## 8. Revisão do planejamento

- Escopo corresponde à retomada de projeto e aba; Issues incluída como vista existente. Sem mudança de política de processos/fechamento.
- Há solução para a leitura prematura, identidade estável das abas, cache/GET tardio e fechamento anterior ao debounce.
- Transporte reutiliza settings e plugin instalado, sem criar API Rust fictícia ou depender de cache como confirmação SQLite.
- Fallbacks preservam comportamento existente onde não há preferência válida e evitam criação de abas extras.
- Requisitos têm evidência automatizada e manual associada; execução nativa permanece gate explícito.
- Não foram executados comandos de instalação, testes, preflight operacional, migração, formatação, build, Git de escrita ou serviços externos. Consulta documental não altera serviços.
