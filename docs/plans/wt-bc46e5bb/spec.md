# Especificação técnica — Operação do Horizon pelo badge do projeto

Data: 04/11/2026. Projeto: MaxTerminal. Worktree/branch: `wt-bc46e5bb`.

## 1. Contexto & Objetivo

Ao clicar no badge HORIZON do projeto, abrir um modal com a saída real do Horizon, uma ação para limpar tarefas e o controle de iniciar/parar. A implementação já contém esses elementos; a demanda é completar e tornar confiável o fluxo existente, não criar outro painel.

Evidências da investigação somente-leitura:
- `ProjectWorkspace.vue:22` monta `HorizonBadgeButton`; `App.vue:23` já monta um único `HorizonModal` global.
- `HorizonBadgeButton.vue:2–21` usa dois eventos para alterar a mesma visibilidade. O clique abre, mas o evento de alternância pode fechar e o clique reabrir no mesmo ciclo. Não há reprodução de falha visual nesta sessão.
- `HorizonModal.vue:33–35,51–52,85–114` já contém ações, `TerminalView` com PTY real e confirmação de limpeza.
- `src/stores/horizon.ts` expõe `status` para o daemon, enquanto o plugin MaxPinia também injeta `status` para requisições/cache. Os testes atuais não instalam esse plugin e não cobrem a colisão.
- A store configura `options.get.data` como um computed do objeto inteiro, mas `MaxPinia/src/plugin.ts:122–126` percorre as chaves de um objeto cujos valores são resolvidos com `toValue`. Além disso, a atualização manual usa IPC direto em paralelo à camada GET MaxPinia.
- `HorizonDomainService::snapshot`, no cache recente, substitui status/sessão sem recalcular capacidades. `HorizonRuntimeService::start` consulta o estado armazenado, não necessariamente o PTY vivo; `Stopping` não impede novo spawn. `stop` descarta erros ao sinalizar.
- Tokens de limpeza guardam revisão/escopo, mas não o projeto; `clear_pending` não compara revisão/escopo antes de excluir. Isso deve ser corrigido para expor a ação de modo seguro.

Esses são fatos do código, não comprovação de uma causa única para o comportamento percebido no aplicativo.

## 2. Escopo

### Incluído
- Corrigir a integração badge → store → MaxModal e cobrir a abertura real com os componentes da biblioteca.
- Reutilizar MaxComponentsUi (`MaxBadgeButton`, `MaxModal`, `MaxButton`, `MaxIcon`), MaxPinia para GET/cache e MaxUse para helpers/composables.
- Exibir saída da sessão supervisionada, manter o processo e buffer ao fechar/reabrir e trocar corretamente a identidade do terminal após reinício.
- Oferecer iniciar/parar com estado ocupado e diagnóstico; limpar **apenas prontas e agendadas**, preservando reservadas/em execução e histórico de falhas, conforme contrato existente.
- Proteger ações e prévias contra troca de projeto, respostas antigas, duplicação concorrente e tokens de outro contexto.
- Corrigir apenas as inconsistências nativas necessárias ao ciclo iniciar → parar → iniciar e às capacidades da UI; testes e aceite nativo isolado.

### Fora de escopo
- Novo dashboard, novas métricas, novos comandos IPC, migrações, instalação de Laravel/PHP/Redis/Horizon ou alterações nos repositórios das bibliotecas.
- Limpar falhas, matar tarefas reservadas, usar `horizon:clear` ou esvaziar Redis globalmente.
- Capturar stdout retrospectivo de Horizon externo, encerrá-lo ou assumir controle dele. Não iniciar duplicata de processo externo pelo modal.
- Refatoração geral do terminal, novo histórico persistente, adoção de processos externos, commit/merge/push ou outra worktree.

## 3. Arquitetura & Mudanças Técnicas

```text
ProjectWorkspace → HorizonBadgeButton → isModalOpen → HorizonModal
                                               ├─ TerminalView(paneId, ptyId, generation)
                                               ├─ start / stop → IPC existente
                                               └─ preview / confirmClear → IPC existente
Horizon store → MaxPinia.reload → Axios adapter → horizon_snapshot
                                            → runtime → domínio/probe → PTY/Redis
```

### Frontend
- `src/stores/horizon.ts`: renomear status do daemon para `runtimeStatus`; reservar `status` ao MaxPinia. Ajustar consumidores e testes. `options.get.data` deve ser um objeto `{ projectId: computed(...) }`; definir valor padrão compatível com snapshot nulo e getters defensivos para hidratação inicial.
- GET de snapshot exclusivamente via MaxPinia; `refreshSnapshot` torna-se fachada para `reload` injetado. `onServerData` valida projeto/contexto e impede resposta anterior de reativar capacidades/sessão após mutação. Cache pode informar métricas, mas nunca autorizar ações nem vincular PTY até confirmação do backend nesta ativação.
- Polling com uma requisição lógica por vez, pausado com documento oculto/sem projeto, 2s no modal e 5s fora dele; após erro aguardar antes de tentar novamente, com recuo limitado. Usar helpers MaxUse compatíveis com a biblioteca efetiva e cancelar timers no descarte. A abertura solicita atualização imediata deduplicada, sem esperar o polling.
- `src/ipc/horizonMaxPiniaAdapter.ts`: caminho/método GET exatos, sem casamento por `includes`; limpar listener de cancelamento em `finally`. IPC iniciado não é cancelado pelo AbortSignal: apenas descartar resposta antiga. Não repetir automaticamente mutações.
- `HorizonBadgeButton.vue`: uma única via de comando para abrir o modal, sem toggle concorrente; manter estado visual derivado da store, overlays e acessibilidade.
- `HorizonModal.vue`: manter `visible/update:visible` (API comprovada, não é necessário trocar por outra prop). Remontar terminal por identidade composta de projeto/pane/PTY/geração; resetar confirmação ao mudar projeto/contexto ou fechar. Validar validade temporal e identidade antes de confirmar. Mostrar diagnóstico e resultado parcial/aviso da limpeza, não descartá-los como sucesso pleno.
- Estado parado: ação Iniciar. Ativo supervisionado: ação Parar. Transições: ação correspondente desabilitada/ocupada. Limpar pendentes permanece visível no modal, desabilitada sem capacidade/pendências e com explicação. Estado externo: informação de ausência de terminal capturável e sem controles de iniciar/parar habilitados. Erro: diagnóstico e somente capacidades efetivamente válidas.
- Preservar fechar sem parar, foco/Escape/backdrop e empilhamento do MaxModal. Se necessário impedir fechamento durante mutação, usar também `beforeClose`, pois apenas `dismissable/closeOnEscape` não bloqueiam o X.

### Backend
- `src-tauri/src/horizon/mod.rs`: reconciliar sessões com estado real do PTY antes de iniciar/parar/verificar conflitos; serializar mutações concorrentes e reserva de destino entre projetos. Não manter lock síncrono durante await. Uma sessão em parada continua ocupando o destino até término real. Reinício só depois do término, com nova geração/pane/PTY.
- Não mascarar erro de sinalização como parada bem-sucedida. Manter parada graciosa do master por identidade de processo, sem SIGKILL automático de workers. Rever tratamento de saída em paralelo e preservar saída final da sessão encerrada enquanto o buffer existir.
- `src-tauri/crates/mt-commands/src/horizon/{service.rs,probe.rs}`: centralizar cálculo de capacidades e aplicá-lo tanto no cache quanto no probe fresco; `external` não autoriza iniciar/parar. Revalidar disponibilidade/contexto também no comando nativo, sem confiar no frontend.
- Confirmação de limpeza deve guardar `project_id`, revisão, escopo, filas e validade. Consumir uma vez; rejeitar projeto diferente, tempo `now >= expires_at`, revisão/escopo/filas alterados após nova leitura, antes de chamar o cleaner. Invalidação por remoção de projeto. Não expor segredos na revisão/diagnóstico.
- Preservar contratos de `horizon_snapshot/start/stop/clear_pending_preview/clear_pending` e DTOs em `src/types/bindings.ts`. Nenhuma alteração manual nos bindings gerados é prevista.

## 4. Plano de Execução Faseado

1. Preflight somente-leitura de revisão, libs efetivas/harness e isolamento. Estabilizar nomes e contratos acima; impedir implementação dependente se biblioteca estiver inválida.
2. Corrigir store/adapter MaxPinia e testes com plugin real, isolamento de cache e IPC simulado.
3. Ajustar badge/modal/terminal e testes de interação com componentes reais; nenhum segundo host de modal.
4. Corrigir reconciliação/capacidades/serialização nativas e validação de tokens; testes sem atingir aplicações/Redis reais.
5. Com todo escopo e testes escritos, executar validações agrupadas, revisar falhas em lote, depois aceite nativo em ambiente comprovadamente descartável. Registrar pendências manuais, sem alegar conclusão integral.

## 5. Critérios de Aceite & Validação

- Clique, Enter e Espaço abrem um único modal; fechar/reabrir não inicia nem para o daemon.
- Modal apresenta saída real e contínua do PTY correto, inclusive após reabrir; reinício troca identidade e não mistura projetos.
- Iniciar/parar funciona repetidamente; cliques simultâneos não duplicam daemon; estado/capacidades coerentes mesmo no cache recente; erro de parada é visível.
- Limpeza exige confirmação de escopo e validade; remove somente pendentes e comunica parcialidade; tokens de outro projeto/contexto/expirados não provocam exclusão.
- MaxPinia real preserva seu próprio status, faz GET pela rota IPC e não envia essa rota à rede; cache antigo não habilita ação nem terminal obsoleto.
- Frontend passa pelos comandos existentes `npm run lint`, `npm run lint:style`, `npm run typecheck`, `npm run typecheck:vue`, `npm run test:coverage`, `npm run build`, sujeitos ao gate de bibliotecas. `npm run check` é o orquestrador canônico completo; ler segurança dos testes antes de executá-lo.
- Aceite com PHP/Horizon/Redis real é manual e somente em projeto descartável, com Redis/prefixos/filas isolados comprovados e autorização. Ausência desse ambiente deixa `manualPending`.

Nesta sessão: apenas investigação e documentos. Sem testes, inicialização de app, comandos Horizon, exclusão de tarefas ou alterações externas. Links locais resolvidos para `/home/johnattas/GitHub/{MaxComponentsUi,MaxPinia,MaxUse}`; MaxComponentsUi declara 1.1.36 e exports em dist. Tipos dist do MaxModal confirmam visible e beforeClose. Context7 consultado para Vue 3; catálogo não ofereceu a versão exata 3.6.0-rc.13, portanto não constitui validação dessa versão. Nenhum projeto Laravel-alvo ou evento/log temporal foi indicado; não foi reproduzida a interface nativa nem comprovada disponibilidade de Redis/Horizon.
