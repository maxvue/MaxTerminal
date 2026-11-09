# Plano de implementação — Issue #16

### Descrição e Causa Raiz

**Veracidade: bug confirmado no código atual e reproduzido isoladamente.** A inicialização das dez inscrições do workspace perde os handles das inscrições bem-sucedidas quando qualquer outra inscrição rejeita. O efeito é a retenção de listeners e closures mesmo após `stop()`/descarte do escopo. Antes do descarte, eventos desses listeners também podem alcançar stores sem que a inicialização tenha terminado com sucesso.

**Causa Raiz Comprovada:**

- `src/composables/workspace/useWorkspaceEvents.ts:L184-223`: `localUnlisteners` começa vazio; os dez registros são iniciados em `Promise.all`; os handles só são copiados para a lista após o sucesso integral. `Promise.all` rejeita antecipadamente, mas não cancela as outras inscrições.
- `src/composables/workspace/useWorkspaceEvents.ts:L225-239`: a publicação em `unlisteners` também depende do sucesso integral. Na rejeição, o `catch` tenta liberar a lista local vazia e zera a lista compartilhada. Inscrições que resolvem depois da rejeição igualmente ficam sem responsável pela liberação.
- `src/composables/workspace/useWorkspaceEvents.ts:L250-265`: `stop()` e `onScopeDispose(stop)` só alcançam os handles publicados em `unlisteners`. O bloqueio por `disposed` em `L97-104` impede processamento após o descarte, mas não remove listeners nativos.
- Agravante: `catch` em `L235-239` não esvazia `preHydrationBuffer`; eventos recebidos durante a tentativa fracassada podem sobreviver até uma nova tentativa. As callbacks atuais não distinguem a geração que as criou.

**Fluxo causal e rastreamento reverso UI ⇄ Store ⇄ IPC ⇄ Service ⇄ DB:**

1. UI: `src/components/layout/AppLayout.vue:L45-60` cria o composable e chama `workspaceEvents.start()` no mount, absorvendo a rejeição. O descarte do escopo é registrado pelo próprio composable.
2. Inscrição: o composable chama seis wrappers em `src/ipc/tabs.ts:L87-139`, dois em `src/ipc/sessions.ts:L58-74`, um em `src/ipc/templates.ts:L57-64` e um em `src/ipc/workspace.ts:L31-41`. Todos retornam o `Promise<UnlistenFn>` de `listen` da API Tauri; não mantêm um registro alternativo para cleanup.
3. Retorno dos eventos: os wrappers repassam `ev.payload` para `handleIncoming` e `dispatchItem`, em `src/composables/workspace/useWorkspaceEvents.ts:L61-125`, atualizando tabs, sessões e templates. O handle perdido é o recurso necessário para interromper esse caminho na origem.
4. Hidratação normal: após as inscrições, `resync()` (`L128-163`, chamado em `L230-231`) usa `src/stores/tabs.ts:L253-256` → `src/ipc/workspace.ts:L13-16` → comando Tauri `src-tauri/src/commands/workspace.rs:L10-17` → `WorkspaceManager::workspace_snapshot`, em `src-tauri/src/workspace/manager.rs:L509-534`.
5. Service/DB: o snapshot lê abas vivas em memória e pode iniciar restauração; `ensure_restored`, em `src-tauri/src/workspace/manager.rs:L486-506`, passa `self.inner.db` para a reconciliação. No cenário reproduzido, a rejeição ocorre antes de `resync()`: snapshot e banco não são necessários para produzir a falha. A propriedade dos handles no frontend é a origem comprovada, sem necessidade de alteração de persistência.

**Reprodução realizada nesta etapa, sem editar fontes ou testes:** Node v24.18.0 carregou o arquivo real, transpilou TypeScript em memória e executou o composable em `node:vm`, substituindo somente Vue, stores e wrappers IPC por doubles. Uma das dez inscrições rejeitou com um erro conhecido; as outras nove retornaram funções contadoras. Após aguardar a rejeição de `start()`, foi chamado `stop()`. Foram exercitadas resoluções anteriores à rejeição e resoluções posteriores à rejeição/stop.

```json
{"cenario":"Sucessos antes da rejeição","inscricoesBemSucedidas":9,"startRejeitouComErroOriginal":true,"chamadasUnlisten":0,"chamadasSnapshot":0}
{"cenario":"Sucessos após rejeição e stop","inscricoesBemSucedidas":9,"startRejeitouComErroOriginal":true,"chamadasUnlisten":0,"chamadasSnapshot":0}
```

O esperado é uma chamada para cada um dos nove handles após sua resolução. A reprodução comprova a perda de handles, sem afirmar medição de heap ou reprodução em um processo Tauri real. A cobertura existente em `tests/frontend/workspace/useWorkspaceEvents.spec.ts:L99-139` verifica hidratação/bufferização no caminho de sucesso, mas usa unlisteners vazios e não verifica falha parcial.

### Arquivos afetados

- `src/composables/workspace/useWorkspaceEvents.ts`: gerenciamento incremental e descarte dos handles por tentativa de inicialização.
- `tests/frontend/workspace/useWorkspaceEvents.spec.ts`: regressões de falha parcial, resoluções tardias, cancelamento e nova tentativa, preservando o teste de hidratação existente.
- `docs/issues/16/plan.md`: este documento de planejamento, único arquivo alterado nesta etapa.

### Execuções propostas

1. Manter a API pública `start(): Promise<void>`, `stop(): void` e `resync(): Promise<void>`, as dez inscrições concorrentes e o protocolo de inscrição antes do snapshot. Preservar o erro original e a rejeição antecipada de `start()`.
2. Dar a cada tentativa um estado local de cancelamento/falha, além de `myGen` e da coleção de handles. Disponibilizar essa mesma coleção para `stop()` desde o início da tentativa, permitindo liberar imediatamente handles já obtidos enquanto outros registros ainda estão pendentes.
3. Encapsular cada inscrição em um helper local tipado que receba uma factory `() => Promise<UnlistenFn>`. Executar cada factory em uma cadeia de promessa com rejeição observada; assim, uma eventual exceção síncrona também participa do mesmo tratamento. Passar as dez promessas encapsuladas ao `Promise.all`.
4. No sucesso individual, assumir a propriedade do handle imediatamente: se a tentativa continua válida, armazená-lo na coleção local; se houve falha, `disposed` ou mudança de geração, chamar seu unlistener de forma protegida naquele momento. Não depender do array de resultados do `Promise.all` para descobrir recursos adquiridos.
5. Tornar a liberação consumidora da coleção: retirar os handles antes de invocá-los, mantendo o isolamento de exceções por handle de `safeUnlistenAll`. `catch`, `stop()` e o ramo de geração inválida devem usar o mesmo mecanismo para não executar um unlistener duas vezes. Uma exceção síncrona de cleanup não pode impedir a liberação dos demais nem substituir o erro da inscrição.
6. No `catch`, marcar a tentativa como encerrada antes da limpeza; drenar os handles disponíveis. Somente se ainda for a geração atual, desativar hidratação, limpar o buffer daquela tentativa e desvincular a coleção compartilhada. Relançar o erro original. Os sucessos tardios ficam responsáveis apenas por liberar seus próprios handles, sem modificar estado compartilhado de uma nova tentativa.
7. Encaminhar as callbacks das dez inscrições por uma guarda da tentativa: ignorar eventos se houve falha/cancelamento, descarte ou divergência de geração. Enquanto válida, continuar chamando `handleIncoming`, inclusive durante hidratação. Isso impede que eventos tardios de uma tentativa fracassada repovoem o buffer ou atinjam stores numa nova inicialização.
8. Preservar a deduplicação de chamadas concorrentes a `start()` e a limpeza de `startPromise` condicionada à geração. Após falha, permitir nova tentativa na mesma instância; após `stop()`, preservar o descarte definitivo já existente. O sucesso integral deve executar um snapshot e manter os dez handles até o descarte.
9. Implementar o bloco completo acima e todos os testes abaixo antes de executar testes, checagens de tipos ou build. Corrigir falhas em lote e repetir as validações afetadas.

### Testes de regressão e validação em lote

Usar a suíte existente e seus mocks IPC, com um spy independente por handle e promessas controláveis por `resolve`/`reject`, sem sleeps, timers reais ou backend Tauri. Restaurar implementações dos mocks entre cenários; `vi.clearAllMocks()` sozinho não desfaz implementações alteradas. Sempre observar a rejeição esperada antes de provocá-la e encerrar o composable/escopo ao final.

1. **Falha parcial parametrizada nas dez inscrições:** nove sucessos e uma rejeição; conferir identidade do erro original, zero chamadas de snapshot e uma chamada por handle bem-sucedido. Após `stop()` repetido, a contagem deve continuar sendo um.
2. **Resoluções tardias:** rejeitar uma inscrição enquanto algumas já resolveram e outras permanecem pendentes. `start()` deve rejeitar sem esperar as pendentes; handles prontos são liberados no rollback e os demais ao resolverem, antes e depois de `stop()`.
3. **Cancelamento durante registro:** obter alguns handles, manter os demais pendentes e chamar `stop()`. Os prontos devem ser liberados imediatamente e os tardios assim que disponíveis. O snapshot não deve iniciar. Cobrir também rejeição posterior ao stop e descarte via `effectScope().stop()`.
4. **Nova tentativa com trabalho antigo pendente:** falhar a primeira inicialização, iniciar outra e resolver handles antigos durante a nova hidratação. Verificar que eles são liberados, não limpam os novos handles e suas callbacks não alteram stores/buffer. Após sucesso e `stop()`, cada handle novo deve ter exatamente uma chamada de cleanup.
5. **Buffer da tentativa fracassada:** emitir evento antes da rejeição e callback antiga após a falha. No retry, usar snapshot com sequência inferior à do evento descartado para provar que nenhum evento da tentativa anterior é reproduzido.
6. **Cleanup resiliente:** fazer um unlistener lançar sincronicamente; os demais ainda devem ser chamados uma vez e a rejeição de `start()` deve continuar sendo o erro original da inscrição.
7. **Exceção síncrona de registro:** uma factory lança antes de retornar a promessa; os handles das outras inscrições devem continuar rastreados e ser liberados, inclusive os tardios.
8. **Sucesso e idempotência:** todas as inscrições resolvem; snapshot ocorre só depois das dez; duas chamadas concorrentes de `start()` não duplicam inscrições nem snapshot; nova chamada após sucesso não registra novamente; `stop()` repetido libera cada handle uma vez; `start()` após descarte não faz nova inscrição.
9. **Protocolo existente:** conservar o teste de bufferização e descarte de eventos cobertos pelo snapshot. Validar ordenação por `event_seq` e reconciliação por salto de sequência/mudança de `boot_id`, para garantir que a guarda da tentativa aceita eventos legítimos após o sucesso.

Executar a sequência da seção Validação somente após concluir a implementação e os testes. Agrupar os diagnósticos, corrigir o conjunto e repetir os comandos pertinentes; não executar ciclos de build por alteração individual.

### Banco de dados

Nenhuma migration. O defeito ocorre no ciclo de vida de inscrições Tauri no frontend, antes do snapshot no cenário de falha parcial.

### Riscos de quebra e Não-Regressão

- **Duplo cleanup por listas compartilhadas:** coleção consumida na liberação e spies por handle em falha, sucesso e descarte repetido.
- **Espera indefinida por inscrição pendente:** preservar `Promise.all` com coleta incremental; não trocar simplesmente por `Promise.allSettled`, pois isso atrasaria a rejeição e o rollback até todas as inscrições terminarem. Testar rejeição com pendências ainda abertas.
- **Estado antigo interferindo em retry:** guardar geração e cancelamento nas callbacks e na conclusão de cada inscrição; testar sobreposição de resoluções antigas com uma tentativa nova.
- **Interrupção indevida de eventos legítimos:** manter bufferização, sequência e resync durante a tentativa válida e após sucesso; cobrir o protocolo existente.
- **Mudança de contrato de erro ou descarte:** preservar erro original, `stop()` síncrono/idempotente e descarte definitivo; testar explicitamente.
- **Falha do próprio unlistener:** manter tratamento síncrono existente, sem introduzir uma nova API assíncrona de cleanup. Os testes demonstram que cada handle recebido é acionado; não prometem remoção nativa quando o próprio mecanismo Tauri falha.

### Validação

Executar no worktree dedicado, com as dependências do projeto disponíveis:

```bash
npm run test -- tests/frontend/workspace/useWorkspaceEvents.spec.ts
npm run test -- tests/frontend/workspace
npm run typecheck
npm run lint
npm run build
git diff --check
```

**Prova conclusiva da correção:** a suíte específica passa com rejeição em cada uma das dez posições, nove handles liberados exatamente uma vez, incluindo sucessos tardios, nenhum snapshot em inicialização fracassada e nenhuma interferência de gerações antigas em retry. O caminho integralmente bem-sucedido conserva hidratação/eventos e libera os dez handles no descarte. Tipos, lint e build complementam essa prova comportamental.

Nesta etapa foi executada apenas a reprodução isolada descrita acima; implementação e validações pós-correção ficam para a execução do plano.

### Skills Aplicáveis

- `vue-debugging-best-practices`: ciclo de vida, descarte de efeitos e rastreamento da causa com evidências. A skill está disponível; o guia referenciado `reference/cleanup-side-effects.md` não está presente neste checkout, portanto as evidências locais e a documentação oficial devem fundamentar a execução.
- `javascript-pro`: concorrência, promessas e tratamento de resoluções tardias sem perder recursos.
- `vitest-skill` e `javascript-testing-patterns`: mocks tipados, promessas controláveis e testes de comportamento assíncrono.
- `code-review`: revisar a implementação antes de integração, com atenção à propriedade dos handles, rollback e gerações.

Referência oficial consultada via Context7: documentação Tauri v2, `develop/_sections/frontend-listen.mdx`, que confirma o retorno assíncrono do handle e a necessidade de chamá-lo para remover a inscrição durante cleanup. Não são necessárias dependências novas.
