# Plano de execução — Modal operacional do Horizon

## Identificação e retomada

- Data: 04/11/2026. Projeto: MaxTerminal. Worktree/branch: `wt-bc46e5bb`.
- Diretório obrigatório: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-bc46e5bb`.
- Spec normativa: `docs/plans/wt-bc46e5bb/spec.md`.
- Não criar outra worktree, trocar branch, modificar bibliotecas externas ou fazer commit/merge/push. Não remover esta worktree; integração e limpeza cabem ao MaxCode quando autorizado.
- Na primeira admissão, ler spec e plano; na retomada, consultar estado e seções pertinentes. Conferir trabalho já feito, diff e revisão factual dos arquivos antes de editar. Não reabrir decisões sem nova evidência ou mudança de requisito. Após falha de patch ou mudança concorrente, reler trechos afetados.

## Pedido e decisões

O badge HORIZON na janela do projeto deve abrir um modal com conteúdo da execução, limpar tarefas e iniciar/parar Horizon. Usuários afetados: projetos Laravel com Horizon detectável. Reutilizar estrutura existente, sem criar painel paralelo.

Decisões de preservação: limpar prontas/agendadas, não reservadas nem falhas; abrir/fechar não controla processo; apenas sessão supervisionada fornece terminal e parada; manter contadores e detecção atuais. Não usar `horizon:clear` nem assumir controle de processos externos. Proposta de segurança: remover a oferta atual de iniciar duplicata quando status for externo. Não há migração nem novo endpoint/DTO planejado.

## Evidências e limites

| Fonte existente | Fato e consequência |
| --- | --- |
| `src/components/projects/ProjectWorkspace.vue`, `src/app/App.vue` | Badge no header e um host global de HorizonModal já existem. Preservar. |
| `src/components/horizon/{HorizonBadgeButton,HorizonModal}.vue` | Dois eventos alteram visibilidade; ações/terminal/limpeza já implementados. Consolidar eventos e garantir comportamento por teste real. |
| `src/stores/horizon.ts`, `src/ipc/{horizon,horizonMaxPiniaAdapter}.ts` | GET direto em paralelo ao opt-in MaxPinia, polling sem descarte explícito, prévia fora de isBusy; troca entre projetos não limpa imediatamente snapshot/modal. |
| `src/app/plugins.ts` | Registra MaxPinia com Axios adapter combinado; não substituir configuração dos demais recursos. |
| `/home/johnattas/GitHub/MaxPinia/src/plugin.ts:112–126,508–549,879–967` | Plugin redefine data, espera objeto de parâmetros, injeta status e reload, possui proteção de identidade e onServerData. Renomear status do daemon para runtimeStatus e testar com plugin instalado. |
| `/home/johnattas/GitHub/MaxComponentsUi/src/components/{MaxModal,MaxBadgeButton}.vue` e `dist/components/MaxModal.vue.d.ts` | visible/update:visible são válidos; beforeClose cobre X/Escape/backdrop; badge emite update antes do click. Não atribuir falha a prop inexistente. |
| `src/components/terminal/TerminalView.vue`, `src/composables/terminal/useTerminal.ts`, `src/terminal/engine/terminalInstance.ts` | attach/detach preservam buffer e stream; composable não remonta automaticamente ao trocar IDs. Usar key composta no consumidor. |
| `src-tauri/src/horizon/mod.rs:57–228` | Snapshot reconcilia PTY, mas start consulta estado armazenado; stop ignora erro de sinal. Corrigir ciclo e concorrência, não reescrever PTY. |
| `src-tauri/crates/mt-commands/src/horizon/{service,probe,cleaner}.rs` | Cache 4s não recalcula capacidades; token 30s não vincula projeto nem revalida contexto; cleaner preserva reserved e limita lote a 10.000. Tratar parcialidade. Probe tem timeout 5s, limpeza 10s: não aumentar genericamente sem evidência. |
| `src-tauri/src/commands/horizon.rs`, `src/types/bindings.ts` | Cinco comandos já disponíveis com revisão/geração/requestId; requestId é ignorado pelo runtime atual, não comprova deduplicação. |
| `src/stores/__tests__/horizon.spec.ts` | Testa ações com Pinia simples e IPC mockado; não cobre MaxPinia real nem interação do modal. |
| `src-tauri/tests/horizon_runtime.rs`, `src-tauri/crates/mt-commands/tests/horizon_tests.rs` | Locais existentes para ampliar testes; runtime atual cobre apenas indisponibilidade básica. |
| `package.json`, `vitest.config.ts`, `tests/setup.ts`, `scripts/{check,sync-local-libs}.mjs`, `.gitignore` | Scripts conhecidos, happy-dom, teleport stub global, dedupe Vue/Pinia/Axios/Max*. Ambas pastas de worktrees ignoradas. |

Estado inicial Git limpo, branch confirmada. Links locais resolvidos por leitura; gate de integridade não executado. Bibliotecas consultadas no código local e tipos publicados, sem garantir que todo dist esteja atualizado. Context7 Vue 3 consultado; versão exata rc.13 não disponível no catálogo. Não houve testes, app aberto, consulta Redis ou Laravel-alvo; nenhuma causa visual única foi comprovada. Não há log/horário de falha reportado para correlacionar; o aceite nativo continua futuro.

## Contratos que o executor deve estabilizar

1. `runtimeStatus: HorizonStatus` é o status do daemon; `store.status` pertence exclusivamente ao MaxPinia. Atualizar todos os consumidores Horizon, incluindo testes, sem alterar o tipo nativo `HorizonSnapshot.status`.
2. `options.get.data = { projectId: computed(() => id.value) }`. `refreshSnapshot(): Promise<void>` delega ao `reload` do MaxPinia, sem GET IPC paralelo/fallback silencioso. Métodos injetados são chamados após instalação do plugin, não dentro do setup antes de existirem.
3. `onServerData` recebe snapshot e metadados de requisição; aceitar somente identidade/revisão ainda válidas. Aplicar resposta de mutação apenas ao projeto/contexto capturado; invalidar leituras iniciadas antes da ação e reconciliar com GET posterior. Uma falha da ação antiga não pode aparecer no novo projeto.
4. Snapshot cacheado nunca prova PTY vivo, capacidades atuais nem disponibilidade. Após seleção/reativação, exigir resposta nativa vigente antes de autorizar mutações/anexação. Limpar snapshot operacional, prévia e erros na troca; manter métricas cacheadas apenas com indicação de atualização. Não gravar tokens de confirmação em cache.
5. `HorizonClearPreview` existente: token, contextRevision, scopeId, queues, estimatedPending, expiresAtMs. Guardar identidade do projeto no fluxo UI e no token nativo interno. `HorizonClearResult` existente: totalCleared, clearedPerQueue, isPartial, warning. Apresentar todos os resultados relevantes e reconciliar contagens.
6. IPC mantém assinaturas: snapshot(projectId); start(projectId, expectedContextRevision, requestId); stop(projectId, expectedGeneration, requestId); preview(projectId); clear(projectId, token, requestId). Sem retry automático de mutação: timeout pode ocultar resultado já aplicado.
7. Terminal usa sessão nativa `{paneId,ptyId,generation}`; key inclui projeto e os três campos. Fechar desanexa sem matar; reabrir reanexa com buffer existente. Não prometer histórico ilimitado ou anterior à captura do PTY.

## Etapas executáveis

### T1 — Preflight e contratos (sem implementação dependente antes do gate)

- Revalidar branch/diff e instruções. Ler scripts/hooks efetivos, exports/dist e symlinks das três libs; conferir identidade única de Vue/Pinia/Axios e configuração do harness.
- No Execute, rodar `node scripts/sync-local-libs.mjs --check` antes das validações dependentes. Se falhar, registrar motivo/ação e interromper, sem instalar, editar bibliotecas, ignorar configuração ou contornar por comando direto.
- Comprovar que Vitest usa cache/storage isolados e que testes Rust não acessam DB/Redis/projeto reais. Preparar harness com app Vue que instale Pinia + createMaxPinia, Axios adapter da aplicação e armazenamento localforage isolado em memória; IPC controlado por mocks. Ler testes de integração antes de liberar suíte completa.
- Definir testes nativos com tempfile/clock e seams internos mínimos para probe/PTY/cleaner, sem alterar contratos públicos nem depender de PHP real para testes determinísticos.
- Saída: contratos da seção anterior confirmados, ambiente apto ou tarefa blocked com próxima ação concreta.

### T2 — Store e fronteira de leitura

Arquivos: `src/stores/horizon.ts`, `src/ipc/horizonMaxPiniaAdapter.ts`, `src/stores/__tests__/horizon.spec.ts`; novos propostos `src/ipc/__tests__/horizonMaxPiniaAdapter.spec.ts` e harness em `tests/` apenas se necessário.

- Implementar runtimeStatus, parâmetros MaxPinia corretos e GET exclusivamente pelo plugin. Integrar diagnóstico de leitura sem apagar sessão válida por uma falha transitória nem autorizar ação com dado antigo.
- Uma atualização em voo por identidade; polling 2s/5s sem sobreposição, suspenso no oculto/indisponível e descartado no dispose. Usar MaxUse para helper de timer/visibilidade; em erro usar espera crescente limitada a 30s e reset após sucesso, sem loop imediato. Preservar dados com indicação de erro, não inventar zero.
- Incluir prévia no bloqueio de ações concorrentes. Capturar projeto/contexto/geração por ação, rejeitar confirmação sem prévia atual e impedir respostas/erros/finally antigos de afetar nova operação.
- Adapter: comparar pathname e GET exatos (query permitida), delegar rotas alheias, liberar listener de abort em todos os resultados; não encaminhar rota Horizon válida à rede.
- Testes: plugin real/status separado, query projectId correta, deduplicação, resposta antiga após mutação/troca A→B→A, hidratação de cache antigo, erro, cancelamento, polling/descarte e prévia concorrente. Usar fake timers e liberar stores/timers em teardown.
- Conclusão de implementação: toda cobertura escrita, revisão factual de identidade/concorrência; executar testes apenas no marco agrupado T5.

### T3 — Badge, modal e saída

Arquivos: `src/components/horizon/{HorizonBadgeButton,HorizonModal}.vue`; novos propostos `src/components/horizon/__tests__/{HorizonBadgeButton,HorizonModal}.spec.ts`.

- Adotar clique único como comando de abrir, removendo o listener de toggle conflitante, com estado ativo derivado de isModalOpen. Abertura atualiza snapshot via store sem criar processo.
- Usar runtimeStatus em todos os ramos. Manter MaxModal existente e ações MaxButton; não alterar biblioteca. Iniciar/Parar conforme capacidades confirmadas, transições visíveis e limpeza desabilitada com motivo quando indisponível.
- Remover CTA externo que sugere iniciar duplicata; explicar que terminal/parada só estão disponíveis para execução supervisionada. Não apresentar terminal fictício.
- Key composta no TerminalView; reset de prévia no fechamento/troca de identidade; verificar expiresAtMs e escopo antes da confirmação. Mostrar erro no diálogo de confirmação ou área acessível acima dele; renovar prévia expirada por nova ação explícita.
- Exibir total removido e warning/isPartial. Não repetir limpeza automaticamente. Se o modal principal estiver bloqueado durante ação, usar beforeClose também para o X e restaurar foco corretamente após confirmação.
- Layout com tokens semânticos, área de terminal dimensionável sem mínimo maior que viewport; respeitar ordem template/script/style, 4 espaços e atributos em linha única.
- Testar badge real + MaxModal real (não somente emitir evento em stub), plugin real, teclado/fechamento/empilhamento. Terminal mockado no teste de UI verifica IDs/visibilidade/key; teste nativo de saída real continua obrigatório. Contornar stub global de teleport no caso que verificar foco/DOM real, sem alterar setup global desnecessariamente.

### T4 — Consistência nativa e confirmação segura

Arquivos: `src-tauri/src/horizon/mod.rs`, `src-tauri/crates/mt-commands/src/horizon/{service,probe}.rs`, `src-tauri/tests/horizon_runtime.rs`, `src-tauri/crates/mt-commands/tests/horizon_tests.rs`. Helpers internos de teste podem ser adicionados junto desses módulos, se necessários.

- Centralizar derivação de capacidades; recalcular ao sobrepor estado/sessão no cache. Não conceder iniciar/parar a externo. Revalidar disponibilidade e revisão não vazia antes de spawn.
- Serializar start/stop por projeto e reservar destino entre projetos de modo atômico; manter reserva em Stopping até saída real. Reconciliar estado armazenado com PtyManager antes de deduplicar e liberar estado encerrado para reinício. Não segurar parking_lot guard durante await. Eventos de saída tardios só afetam o PTY/geração correspondente.
- Reportar falha de signal_process e não deixar UI presa em Stopping quando não foi enviado sinal; stop continua gracioso e por identidade, sem sinais a processo externo/geração divergente. Preservar sessões encerradas para consulta do buffer até substituição/limpeza normal.
- Token interno: project_id + revisão + escopo + filas + expiração. Revalidar snapshot fresco ao consumir, rejeitar identidade diferente ou `now >= expires_at_ms` antes de executar cleaner. Invalidar confirmações na remoção do projeto.
- Tests determinísticos: cache recente após start/stop, ciclo start→stop→start, saída natural, duplo start e projetos com mesmo destino, Stopping bloqueia nova execução, sinal falho, geração divergente, token de outro projeto/contexto/filas/expirado/reutilizado. Dublê do cleaner deve registrar que não foi chamado em rejeições.
- Manter cleaner e DTOs existentes; não ampliar semântica de remoção. Testes e aceite devem provar preservação de reserved/falhas. Revisão de segurança no marco antes de executar qualquer limpeza real.

### T5 — Validação agrupada, aceite e handoff

- Só após T2–T4 completos com testes, executar bateria. Não executar testes/tipos/build a cada microedição. Gate T1 aprovado e sem mudança relevante pode ser reaproveitado.
- `npm run check` é o fluxo canônico: bibliotecas, lint, stylelint, typecheck, typecheck:vue, test:coverage, build, cargo fmt --check, clippy e testes Rust. Ele para na primeira falha. Registrar cada falha/comando/código de saída, corrigir em lote e retomar etapas faltantes. Não burlar o gate para obter um resultado verde.
- Verificações específicas, se necessárias ao diagnóstico após o marco: `npm run test -- src/stores/__tests__/horizon.spec.ts src/components/horizon/__tests__ src/ipc/__tests__/horizonMaxPiniaAdapter.spec.ts`. Validar os novos caminhos após criá-los; não contar comandos planejados como executados.
- O modo de execução deve primeiro comprovar isolamento da suíte. Se houver integração que dependa de serviços reais/credenciais, separar pendência e obter ambiente autorizado, em vez de rodar contra ambiente compartilhado.
- Aceite nativo: aplicação Tauri e projeto Laravel descartável com versões efetivas registradas; Redis separado ou destino/prefixos/filas exclusivos comprovados. Nenhuma cópia ad hoc de .env, migrations/resets ou remoção de filas compartilhadas. Ausência de ambiente deixa manualPending com próxima ação de disponibilizá-lo.
- Preparar apenas nesse ambiente tarefas prontas/agendadas/reservadas e falhas conhecidas; abrir badge, iniciar, observar saída, fechar/reabrir com novas linhas, parar graciosamente, aguardar término por prazo limitado, reiniciar, limpar após confirmar e conferir reserved/falhas preservados. Não usar loops de consulta apertados; espera limitada e recuo. Conferir processo externo sem tentar encerrá-lo.
- Revisão independente nos marcos de segurança e entrega; se delegação futura for autorizada, dividir somente arquivos disjuntos e compartilhar estes contratos. Store/modal têm dependência T2→T3; T4 pode ser paralelo apenas após T1 e sem testes completos durante implementação.
- Entrega descreve arquivos, resultados reais e pendências. Não concluir integralmente sem aceite nativo. Não fazer integração Git nem remover worktree.

## Matriz de aceite

| Tarefa/requisito | Evidência futura | Teste pertinente | Revisão por risco | Manual |
| --- | --- | --- | --- | --- |
| T1 / bibliotecas e isolamento | exports válidos, status do gate e harness isolado | sync-local-libs --check e inspeção do bootstrap | Bloquear bypass e cache real | ambiente Laravel/Redis exclusivo |
| T2 / MaxPinia e identidade | GET via adapter, runtimeStatus separado, nenhuma ação por cache antigo | store com plugin, cancelamento, timers, A→B→A | Corridas e resposta pós-mutação | trocar projeto com modal aberto |
| T3 / clique e modal | Um diálogo, controles corretos e foco | componentes reais + teclado/Escape/X/backdrop | Não parar ao fechar; confirmação acessível | dark/light e viewport pequena |
| T3 / conteúdo da execução | PTY correto e saída nova após reabrir/reiniciar | identidade do TerminalView; terminal existente sem regressão | Sem misturar projetos/gerações | saída PTY real no Tauri |
| T4 / iniciar-parar | Nova geração apenas depois da saída; sem duplicatas | runtime, capacidades com cache e falha de sinal | Concorrência e filhos/externos | processo termina graciosamente e reinicia |
| T4 / limpar | Token válido e resultado explícito; reserved/falhas intactos | domínio com clock/cleaner controlados | Operação irreversível: projeto, contexto e TTL | ready/delayed removidos somente no destino isolado |
| T5 / não regressão | revisão atual passa no fluxo canônico | npm run check | Revisão final do diff e pendências | passos nativos documentados |

## Riscos, operação e recuperação

- Cache persistido pode conter IDs antigos: não confiar nele para anexar PTY/autorizar mutações. Não limpar cache global como correção; restringir invalidação ao recurso afetado.
- Mudança de status para runtimeStatus exige busca de todos os consumidores, sem renomear campo dos DTOs. Testes apenas com Pinia simples esconderiam o problema novamente.
- Falha de probe não deve gerar duplicata, retentativa de limpeza ou números falsos. Recuo de leitura não equivale a cancelamento nativo nem garantia de tempo exato de rede/Redis.
- Contexto validado antes do cleaner reduz erro de alvo; não alegar atomicidade entre configuração PHP/Redis mutável e execução. Se configuração/destino puder mudar durante a operação, bloquear limpeza até demonstrar estabilidade do ambiente; não prometer segurança de ambiente compartilhado.
- Limpeza é irreversível. Não existe rollback de tarefas removidas; confirmação e isolamento são gates. Reversão do código não restaura filas e não substitui parar/limpar recursos de teste com autorização específica.
- Não fazer refatorações gerais do cleaner/terminal nem atualizações de libs. Se testes demonstrarem falha nesses contratos críticos além do escopo previsto, registrar impedimento e reavaliar antes de liberar ação destrutiva.

## Registro de execução — somente na futura sessão Execute

Criar/atualizar `docs/plans/wt-bc46e5bb/execution-state.json` após marcos e no handoff, **não nesta sessão Plan**. Usar hashes SHA256 reais dos bytes de plan.md e da revisão factual dos arquivos das tarefas, com ordem de arquivos determinística. Um registro autodeclarado não prova execução nativa nem detecta verificações omitidas.

Contrato version 2 estrito, sem chaves adicionais:
- Raiz: `{version: 2, planRevision, tasks, verification, updatedAt?, nextAction?}`. `planRevision` = SHA256 real de plan.md. `updatedAt`, se presente, inteiro positivo em milissegundos.
- Tarefa: `{id, status, files, validations, revision?, nextAction?}`. IDs T1–T5; `validations: []` obrigatório, preenchido somente com registros válidos do contrato consumidor confirmado no Execute. Status: `pending|running|completed|blocked|manualPending|validationFailed`. `revision`, quando presente, SHA256 real da revisão factual.
- `verification`: `{revision, checks}`; revision SHA256 real da revisão de conteúdo verificada. Check permitido apenas em `verification.checks`: `{command, revision, result, exitCode?}`. `command` até 4.096 caracteres, realmente executado; `revision` SHA256 real, nunca rótulo.
- Resultado de validação/check: apenas `passed|failed|pending`; `passed` exige exitCode 0; `failed` exige inteiro diferente de 0; `pending` não aceita exitCode. Nunca null. `blocked` não é resultado de validação.
- `nextAction` até 512 caracteres, com motivo e ação concreta para bloqueio/pendência. Não adicionar `blocker`, `diagnostic`, `branchSync`, `durableSync`, `followUp`, `schemaVersion`, `worktree`, `branch`, `head`, `planPath`, `status`/`checks` soltos na raiz nem `title`/`evidence` na tarefa.
- Registrar todas as falhas, inclusive da suíte completa; não inferir exitCode de tool:end.ok. Checks vazios, ausentes, falhos ou obsoletos impedem validação integral. Version 1 continua compatível, mas não prova validação integral automaticamente.
- Conferir lacunas e trabalho existente na retomada; não repetir mesma falha sem diagnóstico/progresso. Cancelamento preserva resultado parcial e próxima ação. Aceite externo pendente continua manualPending, sem alegação de conclusão total.
