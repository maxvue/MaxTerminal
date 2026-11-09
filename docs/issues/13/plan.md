# Issue #13 — Encerramento seguro dos listeners de useShellCommandBlocks

## Descrição e Causa Raiz

**Veracidade: bug confirmado no código e por reprodução determinística.** Base analisada: `feb8f520ff5c7a8f9cace41e393a088c24c5fe9c`. O encerramento do adaptador enquanto uma inscrição está pendente deixa listeners registrados depois de `stop()`. Fechar o terminal ou substituir seu vínculo de painel/PTY pode disparar essa sequência. Repetições acumulam callbacks e referências ao adaptador, às opções e às stores.

### Causa Raiz Comprovada

- `src/composables/useShellCommandBlocks.ts:L68-73`: os handles começam como `null`; `disposed` é o indicador de encerramento.
- `src/composables/useShellCommandBlocks.ts:L285-295`: `start()` verifica `disposed` somente antes de aguardar as inscrições. As atribuições dos handles ocorrem depois dos `await`, sem revalidar o ciclo de vida nem liberar o recurso recém-adquirido.
- `src/composables/useShellCommandBlocks.ts:L310-323`: `stop()` marca `disposed` e libera somente os handles já disponíveis naquele instante. Não alcança o handle que ainda será retornado pela Promise.
- `src/ipc/shell.ts:L49-65`: as duas inscrições delegam a `listen`, retornando `Promise<UnlistenFn>`. Ignorar eventos não equivale a remover a inscrição.
- `src/composables/useShellCommandBlocks.ts:L297-306`: a continuação ainda pode iniciar a consulta de hidratação após o encerramento.

Sequência causal: `start()` → primeira inscrição pendente → `stop()` encontra os handles nulos → a Promise resolve → `start()` guarda o handle tardio, registra o segundo listener e consulta o histórico → nenhum novo encerramento é garantido pelo consumidor. Se a parada acontecer durante a segunda inscrição, somente o primeiro listener será liberado.

**Limite do impacto comprovado:** `applyStarted`, `applyFinished` e `applyDto` já verificam `disposed` (`src/composables/useShellCommandBlocks.ts:L99-105`, `L155-161`, `L228-231`). Portanto, o achado comprova retenção de inscrições e trabalho desnecessário, não corrupção das stores depois da parada.

### Rastreamento reverso e fluxo de dados

1. **UI e ciclo de vida:** `src/components/terminal/HybridTerminal.vue:L80-87` encerra a instância anterior, cria outra e inicia `start()` sem bloquear. O terminal pronto chama esse fluxo em `L142-151`; mudanças de `paneId`/`ptyId` repetem-no em `L162-176`; a desmontagem chama `stop()` e elimina a referência em `L179-186`. O descarte do escopo também chama `stop()` em `src/composables/useShellCommandBlocks.ts:L325-329`.
2. **UI ⇄ stores:** o adaptador encaminha eventos ao histórico e ao feed em `src/composables/useShellCommandBlocks.ts:L113-144` e `L169-224`. `src/stores/shellHistory.ts:L189-227` e `L231-270` convertem eventos em DTOs; `src/stores/commandBlocks.ts:L288-332` e `L389-421` criam e finalizam os blocos exibidos pela camada `CommandBlockLayer` de `HybridTerminal.vue:L10-14`.
3. **Eventos/API:** os callbacks chegam por `src/ipc/shell.ts:L49-65`, a partir das emissões Rust em `src-tauri/src/events/shell.rs:L103-128`. O handle de remoção pertence ao adaptador frontend. Não há rota HTTP neste fluxo.
4. **Hidratação ⇄ serviço ⇄ DB:** `src/composables/useShellCommandBlocks.ts:L297-305` → `src/ipc/shell.ts:L25-30` → comando Tauri `src-tauri/src/commands/shell.rs:L27-35` → `src-tauri/crates/mt-shell/src/service.rs:L396-434` → repositório `src-tauri/crates/mt-db/src/repos/command_blocks.rs:L295-299` e consulta parametrizada de `command_blocks` em `L611-625`. Os DTOs voltam a `applyDto` e às stores. O serviço também consulta seu cache; nenhuma dessas camadas é responsável pelo descarte do listener frontend.

### Reprodução executada nesta análise

Foi executado um comando Node em memória, lendo o arquivo real, removendo apenas os imports via AST e transpilando-o com TypeScript 5.8.3 para `node:vm`. As dependências foram substituídas por doubles; cada inscrição retorna uma Promise controlável e um handle que incrementa um contador e remove o listener de um conjunto de inscrições ativas. Não foram criados arquivos de teste nem modificados fontes.

Para cada inscrição (`started` e `finished`): iniciar o adaptador com `ptyId`, aguardar a entrada na inscrição controlada, executar `stop()`, resolver a Promise e aguardar `start()`. As asserções confirmaram que o handle tardio não foi chamado e que a inscrição permanecia ativa. Saída observada:

```json
{"pending":"started","calls":{"started":1,"finished":1,"list":1},"cleaned":{"started":0,"finished":0},"active":["started","finished"]}
{"pending":"finished","calls":{"started":1,"finished":1,"list":1},"cleaned":{"started":1,"finished":0},"active":["finished"]}
```

Essa é uma reprodução do controle assíncrono do código real com dependências simuladas; não representa uma medição de heap nem execução do aplicativo Tauri completo. A documentação oficial de eventos Tauri v2 confirma que a limpeza deve consumir o handle retornado pela Promise, inclusive se ela resolver depois da desmontagem: <https://github.com/tauri-apps/tauri-docs/blob/v2/src/content/docs/develop/_sections/frontend-listen.mdx>.

## Arquivos afetados

Arquivos a alterar durante a implementação:

1. `src/composables/useShellCommandBlocks.ts` — aquisição e descarte dos handles assíncronos; interrupção da continuação após parada.
2. `src/composables/__tests__/useShellCommandBlocks.spec.ts` — regressões determinísticas de inscrições pendentes e preservação dos contratos existentes.

Artefato desta etapa: `docs/issues/13/plan.md`. Os componentes, stores, IPC, bindings e arquivos Rust acima são referências do rastreamento, sem alteração necessária para corrigir esta issue.

## Execuções propostas

1. Trabalhar na worktree dedicada da issue, preservando alterações preexistentes. Manter os identificadores em inglês, os comentários e nomes descritivos de testes em pt-BR e a formatação do projeto.
2. Preparar os testes de regressão no arquivo existente. Usar Promises controladas, callbacks capturados e handles individuais por inscrição. Controlar o mock de `listen` por nome do evento, evitando depender da ordem de inscrições de outras stores. Restaurar mocks e encerrar as instâncias ao final de cada caso.
3. Em `start()`, receber o resultado de `onCommandStarted` em uma variável local. Imediatamente após o `await`, verificar `disposed`: se verdadeiro, chamar o handle local uma única vez e retornar, sem guardá-lo no estado nem iniciar `onCommandFinished`.
4. Se a instância estiver ativa, transferir esse handle para `unlistenStarted`. Aplicar exatamente a mesma aquisição local e verificação após o `await` de `onCommandFinished`; em caso de parada, liberar o handle recém-retornado e retornar. O primeiro handle já terá sido removido por `stop()`.
5. Preservar a ordem de inscrição antes da hidratação. A saída antecipada após a segunda inscrição deve impedir `commandBlockList` quando a parada já ocorreu. Depois do `await` da consulta, verificar `disposed` antes de ordenar e aplicar a página. A consulta já iniciada pode terminar, mas seu resultado deve ser descartado.
6. Preservar `stop(): void`, seu encerramento definitivo e a limpeza dos callbacks de flush. Preservar o contrato `start(): Promise<void>`, a rejeição de erros de inscrição/consulta e a possibilidade de chamadas repetidas de `stop()` sem dupla liberação. Não redefinir `disposed` para reusar a instância.
7. Manter os filtros, deduplicação, mapeamento de status e guardas existentes em `applyStarted`, `applyFinished` e `applyDto`. O objetivo é liberar recursos tardios além de impedir efeitos dos callbacks.
8. Concluir todo o bloco de implementação e testes autorizado antes de executar a bateria de testes, tipos ou build. Depois executar os comandos da seção Validação, corrigir as falhas identificadas em lote e repetir as verificações afetadas.

**Delimitação técnica:** o guard de `L286` também não serializa chamadas simultâneas de `start()` na mesma instância. Os consumidores analisados iniciam cada instância uma vez; adicionar mutex, compartilhar Promises de inicialização ou alterar a recuperação de falhas é um contrato adicional, fora da correção cirúrgica desta corrida com `stop()`. As chamadas sequenciais após início completo e após parada continuam cobertas.

## Testes de regressão e validação em lote

Os dois testes atuais (`src/composables/__tests__/useShellCommandBlocks.spec.ts:L39-126` e `L128-288`) exercitam projeção, deduplicação e hidratação, mas o mock de `listen` resolve imediatamente (`L10-12`) e não controla o encerramento durante uma inscrição.

Adicionar os seguintes casos no mesmo arquivo:

| Cenário | Asserções obrigatórias |
| --- | --- |
| Parada durante `onCommandStarted` | Resolver o handle após `stop()` e aguardar `start()`; unlisten de started chamado exatamente uma vez; finished e hidratação não iniciados; conjunto de listeners ativos vazio. |
| Parada durante `onCommandFinished` | Started já registrado; `stop()` o remove uma vez; resolver finished e aguardar `start()`; handle de finished removido uma vez; hidratação não iniciada; zero listeners ativos. |
| Descarte de escopo com inscrição pendente | Criar o adaptador em `effectScope`, interromper o escopo e resolver a inscrição; obter as mesmas garantias, sem precisar de um segundo `stop()`. |
| Parada durante hidratação | Resolver as duas inscrições, manter `commandBlockList` pendente, parar e devolver uma página não vazia; ambos os handles removidos uma vez e nenhum DTO aplicado às stores. |
| Callbacks tardios | Invocar os callbacks capturados depois de `stop()` com eventos válidos; nenhuma alteração de blocos, histórico ou agendamento de flush. A contagem de handles deve ser verificada separadamente. |
| Fluxo normal | Inscrições ativas antes da consulta, hidratação ordenada por `commandSeq`, eventos funcionando e `stop()` removendo cada listener uma vez. |
| Idempotência e encerramento definitivo | Repetir `stop()` antes/depois da resolução tardia; não duplicar unlisten. Chamar `start()` depois de parado; não registrar eventos nem consultar o backend. Repetir `start()` após início completo; não duplicar inscrições. |
| Rejeição depois de parada | Rejeitar a inscrição pendente após `stop()`; a Promise de `start()` rejeita de forma observável, sem iniciar próximas etapas; handles previamente adquiridos continuam liberados. |

Usar sinais explícitos para saber que cada inscrição/consulta foi iniciada; evitar sleeps e dependência de temporização real. Os testes devem medir a remoção do recurso, não apenas ausência de mutação reativa. Reutilizar os DTOs/eventos válidos existentes e isolar o estado Pinia por caso.

## Banco de dados

Nenhuma migration. Nenhuma alteração de schema, consultas, WAL, FTS5 ou persistência. A hidratação utiliza os contratos existentes; o resultado de uma consulta já em andamento é ignorado após a parada.

## Riscos de quebra e Não-Regressão

- **Perder eventos no início:** manter started → finished → hidratação e testar a ordem observável das chamadas.
- **Remover o mesmo handle duas vezes:** separar a aquisição local da transferência para o estado; testar parada repetida e descarte do escopo junto da parada explícita.
- **Reativar uma instância encerrada:** conservar `disposed` monotônico e testar `start()` depois de `stop()`.
- **Alterar o histórico projetado:** conservar ordenação, filtros, deduplicação, metadados e status; manter os testes existentes e executar a suíte completa.
- **Ocultar falhas de IPC:** manter a rejeição da Promise de `start()`; não introduzir captura silenciosa de erros no composable.
- **Teste falso-positivo:** os callbacks já ignoram eventos após a parada no código defeituoso. Exigir contagem exata de unlisteners e zero inscrições ativas para provar a correção.
- **Cancelamento inexistente:** não presumir cancelamento nativo das Promises de inscrição/consulta. Liberar o handle quando disponível e descartar respostas tardias.

## Validação

Executar na worktree, somente após implementar o bloco completo e seus testes:

```bash
npm run test -- src/composables/__tests__/useShellCommandBlocks.spec.ts
npm run test
npm run typecheck
npm run lint
npm run build
git diff --check
```

**Prova conclusiva da correção:** os dois primeiros cenários da tabela devem terminar com zero inscrições ativas, cada handle adquirido removido exatamente uma vez, nenhuma hidratação iniciada após a parada e nenhuma necessidade de uma segunda chamada de `stop()`. O comando focal acima deve passar junto com os testes funcionais existentes. No código analisado, a reprodução já demonstrou o resultado inverso para ambas as janelas de corrida.

Nesta etapa foi executada apenas a reprodução em memória descrita acima. A bateria de implementação é futura; não se declara a correção aplicada ou os testes futuros aprovados.

## Skills Aplicáveis

- `vue-debugging-best-practices` — diagnóstico do ciclo de vida, efeitos colaterais e descarte assíncrono. A skill foi consultada nesta análise; seu guia referenciado `reference/cleanup-side-effects.md` não está presente na cópia local, portanto a evidência utilizada é o código e a reprodução, complementados pela documentação oficial do Tauri.
- `vitest-skill` — mocks, Promises controladas e asserções dos testes de regressão.
- `javascript-testing-patterns` — isolamento, testes determinísticos de concorrência assíncrona e verificação comportamental dos recursos.
- `code-review` — revisão final da correção, contratos públicos e cobertura das duas janelas de inscrição antes de integrar.

Não é necessária delegação nem criação de abstração genérica para este ajuste de dois arquivos.
