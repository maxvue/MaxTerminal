# Plano de implementação — Issue #15

## Descrição e Causa Raiz

**Veracidade: bug confirmado no código atual e reproduzido em experimento controlado.** Base analisada: `feb8f520ff5c7a8f9cace41e393a088c24c5fe9c`, worktree `wt-implement-issue-15`. Esta etapa altera somente este documento; a correção e seus testes serão implementados posteriormente.

As stores de Git e portas tratam a existência de um handle de limpeza como prova de que a inscrição já começou. Entretanto, o handle só é atribuído depois do `await`. Durante essa janela, uma parada não cancela a inicialização, e outra inicialização pode registrar o mesmo evento novamente.

### Causa Raiz Comprovada

- **Portas:** `src/stores/ports.ts:L24-26` mantém apenas três handles. Em `src/stores/ports.ts:L141-151`, cada inscrição verifica o handle antes de aguardar a Promise, sem marcar uma inicialização pendente. `src/stores/ports.ts:L153-166` libera somente os handles já disponíveis e não invalida as continuações assíncronas. Após uma parada, uma continuação pode atribuir o primeiro handle e ainda iniciar as duas inscrições seguintes. Chamadas concorrentes podem sobrescrever os três handles anteriores.
- **Git:** `src/stores/git.ts:L29` mantém um único handle. `src/stores/git.ts:L149-156` apresenta a mesma janela entre verificação e atribuição; `src/stores/git.ts:L158-163` não invalida o trabalho pendente. Duas chamadas podem obter handles distintos e armazenar apenas o último.
- **Ausência de isolamento dos callbacks:** callbacks de portas em `src/stores/ports.ts:L143-149` e de Git em `src/stores/git.ts:L153-155` aplicam eventos sem verificar a validade do ciclo de escuta. Uma inscrição pendente ou um callback já enfileirado pode modificar a store após a parada.
- **Agravante em falhas parciais:** a sequência de portas não possui rollback (`src/stores/ports.ts:L141-151`). Se a segunda ou terceira inscrição rejeitar, as anteriores permanecem instaladas até uma parada posterior. O consumidor captura a rejeição sem realizar limpeza adicional (`src/composables/useTabIndicatorsSync.ts:L41-47`).
- **Lacuna nos testes:** `src/stores/__tests__/ports.spec.ts:L7-15,L120-122` e `src/stores/__tests__/git.spec.ts:L8-15,L141-144` usam inscrições imediatamente resolvidas com funções vazias e aguardam a inicialização antes de resetar. Não verificam concorrência, parada antecipada nem a contagem real de liberações.

### Fluxo causal e rastreamento reverso dos dados

1. A UI lê o contexto Git e as portas em `src/components/indicators/TabIndicators.vue:L49-63`, exibindo-os em `L5-8`.
2. Esses dados vêm de `applyContext`/`applyStatusChangedEvent` em `src/stores/git.ts:L31-45` e de `applyOpened`/`applyUpdated`/`applyClosed` em `src/stores/ports.ts:L45-78`.
3. Os callbacks são registrados pelos wrappers `src/ipc/git.ts:L38-45` e `src/ipc/ports.ts:L39-58`, que retornam `Promise<UnlistenFn>` para os eventos globais Tauri.
4. No backend, `GitService` encaminha alterações por `src-tauri/crates/mt-git/src/cache.rs:L331`; o adaptador publica o evento em `src-tauri/src/events/git.rs:L36-39`. O serviço de portas emite as diferenças em `src-tauri/crates/mt-ports/src/poller.rs:L574-580`; `src-tauri/src/events/ports.rs:L49-60` as publica no barramento Tauri.
5. O ciclo de vida é iniciado em `src/components/workspace/TabWorkspaceView.vue:L117`: `useTabIndicatorsSync` chama ambas as inicializações sem bloqueio em `src/composables/useTabIndicatorsSync.ts:L41-49`, e chama as paradas durante a desmontagem em `L51-62`. A desmontagem ocorre normalmente; as continuações das stores é que ignoram essa parada.
6. Os carregamentos iniciais também partem do watcher em `src/composables/useTabIndicatorsSync.ts:L24-38`, passando por `ensurePaneStatus`/`loadForTab` e pelos comandos IPC. Esses requests são um caminho distinto da inscrição de listeners; a correção desta issue atua sobre o ciclo das inscrições.

**API/Rotas/Controller/DB:** o caminho afetado usa wrappers IPC, serviços Rust e eventos Tauri, não rotas HTTP ou controllers web. Nenhuma operação de banco participa da aquisição/liberação dos handles apontados.

### Reprodução executada

Foi executado um script Node em memória, lendo os arquivos atuais, removendo apenas imports/export e anotações TypeScript, e substituindo `defineStore`, `ref` e os registradores IPC por dublês. As funções reais de inicialização/parada foram preservadas. Promises controladas retardaram a entrega dos handles. Não houve escrita de código-fonte nem execução do aplicativo Tauri; o resultado comprova o interleaving JavaScript, não uma medição de heap em produção.

Resultados observados, com asserções aprovadas:

| Store | Sequência | Inscrições criadas | Handles liberados | Inscrições remanescentes |
| --- | --- | ---: | ---: | ---: |
| Git | iniciar → parar → resolver | 1 | 0 | 1 |
| Git | iniciar duas vezes → resolver → parar | 2 | 1 | 1 |
| Portas | iniciar → parar → resolver | 3 | 0 | 3 |
| Portas | iniciar duas vezes → resolver → parar | 6 | 3 | 3 |

Comando reproduzível, somente leitura, executado na raiz da worktree com Node 24:

```bash
node --input-type=module <<'NODE'
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import assert from 'node:assert/strict';

for (const name of ['git', 'ports']) {
    for (const scenario of ['parada-durante-inscricao', 'inicios-concorrentes']) {
        const active = new Set();
        const pending = [];
        let created = 0;
        let released = 0;
        const ipc = new Proxy({}, {
            get: (_, event) => () => new Promise((resolve) => {
                const id = `${String(event)}:${++created}`;
                active.add(id);
                pending.push(() => resolve(() => {
                    if (active.delete(id)) released++;
                }));
            })
        });
        const source = readFileSync(`src/stores/${name}.ts`, 'utf8')
            .replace(/^import[\s\S]*?;\n/gm, '')
            .replace('export const ', 'const ');
        const storeName = name === 'git' ? 'useGitStore' : 'usePortsStore';
        const js = stripTypeScriptTypes(source) + `\nreturn ${storeName}();`;
        const store = new Function('defineStore', 'ref', `${name}Ipc`, js)(
            (_, setup) => setup,
            (value) => ({ value }),
            ipc
        );
        const starts = [store.startListening()];
        if (scenario === 'parada-durante-inscricao') store.stopListening();
        else starts.push(store.startListening());
        for (let turn = 0; turn < 8; turn++) {
            pending.splice(0).forEach((resolve) => resolve());
            await Promise.resolve();
        }
        await Promise.all(starts);
        if (scenario === 'inicios-concorrentes') store.stopListening();
        assert.equal(active.size, name === 'git' ? 1 : 3);
        console.log(JSON.stringify({ store: name, scenario, created, released, activeAfterStop: active.size }));
    }
}
NODE
```

Esse script documenta o defeito da base analisada; as asserções acima esperam o vazamento atual. A validação da correção será feita pelos testes de regressão abaixo, com expectativa de zero inscrições remanescentes.

## Arquivos afetados

### Implementação futura

1. `src/stores/git.ts`: controlar a inicialização concorrente, invalidar ciclos e liberar handles tardios.
2. `src/stores/ports.ts`: aplicar o mesmo contrato às três inscrições e realizar rollback de falhas parciais.
3. `src/stores/__tests__/git.spec.ts`: adicionar cenários determinísticos de ciclo de vida, eventos e retomada.
4. `src/stores/__tests__/ports.spec.ts`: adicionar cenários equivalentes para cada etapa da sequência de inscrições.
5. `src/composables/__tests__/useTabIndicatorsSync.spec.ts` (novo): comprovar desmontagem e remontagem usando stores reais com IPC simulado.

### Artefato desta etapa

- `docs/issues/15/plan.md`: diagnóstico, plano e critérios de aceite.

## Execuções propostas

1. **Preservar os contratos públicos.** Manter `startListening(): Promise<void>`, `stopListening(): void` e `resetForTests()`. Manter payloads, nomes de eventos, revisões monotônicas, ações de Git/portas e assinaturas IPC existentes.
2. **Introduzir um ciclo privado por instância de store.** Representar cada ciclo por um objeto com identidade própria, estado de validade, Promise de inicialização e coleção dos handles adquiridos. Manter uma referência ao ciclo corrente. A identidade do objeto funciona como token de geração, inclusive quando houver parada seguida de reinício antes de a inscrição antiga resolver. O estado não deve ser global ao módulo.
3. **Reservar o ciclo sincronamente.** A primeira chamada publica o ciclo antes de qualquer `await` ou chamada ao registrador. Agendar o trabalho de inscrição em uma continuação de Promise permite guardar a Promise de inicialização antes de executar o IPC. Chamadas adicionais enquanto esse ciclo estiver inicializando ou ativo aguardam seu resultado, sem registrar novos listeners. Não exigir identidade estrita da Promise retornada pelas actions Pinia; a garantia é de trabalho compartilhado.
4. **Validar antes e depois de cada inscrição.** Antes de chamar o IPC, conferir se o ciclo continua atual. Depois de cada `await`, receber o handle em variável local; se o ciclo foi invalidado, invocar esse handle imediatamente e encerrar a sequência. Só adicionar à coleção os handles pertencentes ao ciclo válido. Em portas, preservar a ordem opened → updated → closed e não iniciar a próxima inscrição de um ciclo cancelado.
5. **Proteger callbacks.** Cada callback captura seu ciclo e só encaminha eventos aos métodos `apply*` se ele ainda for o ciclo corrente e estiver válido. Isso bloqueia eventos entregues entre a parada e a disponibilização do handle, além de callbacks já enfileirados. Os métodos públicos `apply*` mantêm seu comportamento atual.
6. **Tornar a parada idempotente.** `stopListening()` invalida e desassocia o ciclo corrente antes de executar qualquer limpeza. Retira e libera todos os handles já adquiridos exatamente uma vez. Uma chamada repetida é inócua. Handles ainda pendentes são responsabilidade da continuação cancelada, que os libera quando chegarem. Não é possível liberar sincronamente um handle que o IPC ainda não entregou; a invalidação impede seus efeitos nesse intervalo.
7. **Tratar rejeições e rollback.** Se qualquer inscrição falhar, invalidar o ciclo correspondente e liberar todos os seus handles adquiridos. Repropagar a falha aos chamadores que aguardavam sua inicialização, preservando o contrato atual de rejeição. Permitir nova tentativa. A falha ou finalização tardia de um ciclo antigo não pode limpar nem substituir o ciclo novo: qualquer alteração na referência compartilhada deve verificar a identidade do ciclo. Evitar criar Promises rejeitadas sem consumidor em encadeamentos auxiliares de limpeza.
8. **Preservar reset e reinício.** `resetForTests()` continua chamando a parada antes de limpar dados. Eventos ou handles que cheguem depois do reset não podem repovoar os dados por callbacks antigos. Um novo `startListening()` após parada ou falha inicia exatamente um novo conjunto de inscrições.
9. **Implementar o bloco inteiro de código e testes** descrito a seguir antes de executar testes, tipos ou build. Depois, executar a validação em lote, reunir falhas, corrigir o conjunto e validar novamente o que foi afetado. A reprodução de diagnóstico já realizada não substitui os testes permanentes.

## Testes de regressão e validação em lote

Usar Promises controladas por `resolve`/`reject`, funções de limpeza espiadas e registro explícito dos listeners ativos. Não usar sleeps ou temporizações reais. Criar uma Pinia nova por caso, restaurar implementações dos mocks entre casos e resolver/rejeitar todas as operações pendentes no encerramento.

### Stores de Git e portas

- **Inicialização concorrente:** iniciar duas ou mais vezes antes de resolver os registradores. Exigir uma chamada por evento e um único conjunto ativo; após parar, zero listeners e uma liberação por handle.
- **Parada com inscrição pendente:** chamar parar antes de resolver. Resolver o handle tardio e exigir sua liberação imediata, nenhuma mutação por callback antigo e nenhuma inscrição subsequente do ciclo cancelado.
- **Etapas intermediárias de portas:** parametrizar parada com opened, updated e closed pendentes. Os handles anteriores devem ser liberados na parada e o pendente quando resolver; os eventos posteriores não devem ser inscritos.
- **Start → stop → start:** manter a primeira geração pendente, iniciar a segunda e concluir as gerações em ambas as ordens. O handle antigo deve ser liberado sem afetar a geração nova. Após a parada final, nenhum listener permanece.
- **Finalização antiga versus início novo:** concluir ou rejeitar a geração antiga enquanto a nova está pendente; uma terceira chamada deve compartilhar a inicialização nova, sem duplicar registradores.
- **Rejeição e retomada:** rejeitar a inscrição Git e cada uma das três etapas de portas; verificar rejeição observável por todos os chamadores concorrentes, rollback completo e sucesso de nova tentativa. Rejeição antiga não pode desligar ciclo novo.
- **Idempotência:** chamar start novamente com ciclo já ativo e stop repetidamente, incluindo stop antes do primeiro start. Não duplicar registros nem liberações.
- **Eventos válidos e inválidos:** os callbacks do ciclo ativo continuam aplicando contexto/portas e respeitando revisões. Invocar callbacks capturados de ciclos cancelados e comprovar que estado e revisões não mudam, inclusive antes da chegada do handle.
- **Reset durante inicialização:** limpar a store com operação pendente; sua conclusão libera o handle e eventos antigos não repovoam os dados. O ciclo seguinte permanece funcional.
- **Isolamento entre instâncias:** dois contextos Pinia independentes não compartilham ciclo nem limpam listeners um do outro.
- **Regressões funcionais existentes:** manter a cobertura de ordenação/filtro por painel, open/kill, polling, associação Git por painel/aba, revisão e CRUD de worktrees.

### Integração com o composable

Montar um componente mínimo que execute `useTabIndicatorsSync`, com Pinia real e registradores IPC controlados. Usar abas vazias no cenário básico para isolar as inscrições de requests de snapshot.

1. Montar, desmontar antes das inscrições resolverem e então resolvê-las: nenhum listener remanescente e nenhuma mutação por callbacks cancelados.
2. Remontar com a mesma Pinia enquanto a geração antiga está pendente: concluir em ordens diferentes e exigir apenas a geração nova ativa.
3. Chamar `stop()` explicitamente e depois desmontar: cada handle liberado uma vez; o listener DOM de `visibilitychange` é removido. Disparar o evento após a parada não deve executar `setPollMode`.
4. Com ciclo ativo, disparar `visibilitychange` e eventos válidos para garantir que polling e sincronização continuam funcionando; desmontar e verificar limpeza total.

## Banco de dados

**Nenhuma** migration. A correção trata exclusivamente de recursos de escuta em memória e de seu ciclo de vida no frontend.

## Riscos de quebra e Não-Regressão

**Severidade do defeito: moderada.** Foram demonstradas retenção de listeners e possibilidade de registros duplicados. Não há evidência nesta análise de perda de dados persistidos ou indisponibilidade em produção.

| Risco | Proteção e teste necessário |
| --- | --- |
| Limpeza tardia da geração antiga desligar a atual | Identidade de ciclo em atribuições/limpeza; testar ambas as ordens de resolução e rejeição antiga durante início novo. |
| Marcar como ativo cedo demais e esconder falhas | Chamadores concorrentes aguardam a inicialização completa; testar rejeição para todos e nova tentativa. |
| Uma inscrição parcial sobreviver à rejeição | Rollback da coleção do ciclo; parametrizar falhas nas três etapas de portas. |
| Callback enfileirado alterar estado após parada/reset | Guarda de validade no callback e teste de emissão antes/depois da chegada do handle. |
| Limpeza dupla ou perda de handle | Remover a posse antes de liberar; verificar contagem exata de unlisten e conjunto ativo vazio. |
| Bloquear atualizações legítimas de Git/portas | Exercitar callbacks do ciclo ativo, revisões e testes existentes dos indicadores. |
| Estado de ciclo compartilhado entre stores/Pinia | Estado dentro do setup da store e teste com instâncias independentes. |
| Alterar a semântica do consumidor | Preservar parada global por store e APIs públicas; teste integrado de desmontagem e remontagem. |

O consumidor localizado é `TabWorkspaceView.vue`; múltiplas chamadas concorrentes não implicam criar contagem de consumidores. O plano mantém a semântica atual de parada global por store. Proteção de requests de snapshot e alterações no polling nativo são fluxos distintos e não são necessárias para corrigir os handles demonstrados.

## Validação

Após implementar todo o bloco e seus testes, executar na worktree:

```bash
npm run test -- src/stores/__tests__/git.spec.ts src/stores/__tests__/ports.spec.ts src/composables/__tests__/useTabIndicatorsSync.spec.ts src/components/indicators/__tests__/TabIndicators.spec.ts
npm run test
npm run typecheck
npm run typecheck:vue
npm run lint
npm run build
git diff --check
```

**Prova conclusiva:** os novos testes com stores reais precisam passar demonstrando zero listeners ativos após parada e resolução das inscrições pendentes, exatamente uma inscrição por evento em inicializações concorrentes, ausência de mutações de ciclos cancelados e preservação da geração nova em reinícios. Os casos centrais devem detectar a implementação original; build ou typecheck isolados não comprovam o conserto.

Registrar resultados e falhas preexistentes separadamente. Não instalar/atualizar dependências nem alterar contratos para contornar bloqueios alheios à issue. Neste planejamento foi executada somente a reprodução em memória; os comandos de validação da implementação acima ficam para a execução futura.

## Skills Aplicáveis

- `vue-debugging-best-practices`: ciclo de vida, limpeza de efeitos e análise de callbacks após desmontagem. Consultada nesta etapa; a referência complementar `reference/cleanup-side-effects.md` indicada pela skill não está disponível nesta worktree, portanto a evidência usada foi o código real e a reprodução.
- `javascript-pro`: exclusão de inicializações concorrentes, semântica de Promises, invalidação e rejeições assíncronas.
- `vitest-skill`: mocks IPC, Promises controladas e assertions determinísticas nas suites existentes.
- `code-review`: revisão final do diff, contratos públicos e cobertura dos interleavings antes de integrar a implementação.

Seguir também o `AGENTS.md` do projeto: worktree dedicada, textos em pt-BR, identificadores em inglês, TypeScript estrito e formatação vigente. Nenhum commit ou merge é autorizado por esta etapa de planejamento.
