# Plano de implementação — Issue #28

## Descrição e Causa Raiz

### Veracidade e escopo

Achado confirmado por inspeção estática da árvore atual: `src/stores/sessions.ts` não possui documentação da store nem das ações públicas. O defeito é documental; não há evidência, nesta análise, de falha funcional em execução. A reprodução consiste em abrir a declaração da store e as três ações citadas e constatar a ausência de comentários que exponham seus contratos. Os comentários existentes no IPC e nos tipos gerados não descrevem a coordenação, os caches e os retornos antecipados implementados pela store.

### Causa Raiz Comprovada

- `src/stores/sessions.ts:L17-28`: declaração pública sem explicação de responsabilidade, chaves dos caches e estado compartilhado `loading`/`error`.
- `src/stores/sessions.ts:L68-81`: `startNewSession` chama a store de abas, aguarda a atualização do histórico com `reset=true` e retorna a aba atualizada. Não documenta os efeitos sobre sessões/PTYS, as duas etapas assíncronas ou a propagação de erros.
- `src/stores/sessions.ts:L83-120`: `loadSessions` distingue substituição e continuação por `reset`; na continuação sem cursor retorna o cache sem IPC, inclusive antes da primeira carga. Deduplica por `TabSessionId`, ordena por `seq` decrescente e retorna a coleção acumulada. Esses contratos estão apenas no corpo da função.
- `src/stores/sessions.ts:L143-186`: `loadScrollback` recebe dois identificadores semanticamente diferentes, pagina por sessão de painel e retorna os chunks acumulados, deduplicados por `seq` em ordem crescente. Não há documentação de parâmetros, pertencimento ou paginação.
- `src/stores/sessions.ts:L238-258`: essas ações são expostas diretamente aos consumidores, tornando a omissão parte da API pública da store.
- Agravante: `SessionId` e `TabSessionId` são aliases de `string` em `src/types/bindings.ts:L11799-11800` e `L12921-12922`. O TypeScript não impede sua troca por engano. O backend verifica a relação, mas o consumidor precisa conhecer a distinção antes de chamar a ação.

### Fluxo causal e rastreamento de dados

1. **Nova sessão — UI → Store → IPC → comando → serviço → SQLite → Store/UI:**
   - `src/components/workspace/TabWorkspaceView.vue:L471-479` passa o ID da aba para `startNewSession`.
   - `src/stores/sessions.ts:L68-81` delega a `src/stores/tabs.ts:L520-529`, que gera o request ID, chama `sessionNew` e aplica o resultado. `applyTabUpsert` atualiza o cache de abas e sincroniza a store de projetos (`src/stores/tabs.ts:L90-115`).
   - `src/ipc/sessions.ts:L17-20` invoca o comando Tauri; `src-tauri/src/commands/sessions.rs:L12-21` delega ao `WorkspaceManager`.
   - `src-tauri/src/workspace/manager.rs:L2309-2329` encerra os PTYs anteriores e solicita a drenagem do recorder. `L2347-2424` fecha sessões de painel, arquiva o agrupamento anterior, cria novos IDs e vínculos e aplica a retenção no SQLite. `L2426-2499` recria os PTYs mantendo o layout de splits e atualiza a aba; `L2519-2525` emite eventos e retorna a nova visão.
   - No retorno, a store de sessões recarrega a primeira página do histórico. Se essa consulta falhar depois da troca bem-sucedida, a Promise rejeita, mas a troca de sessão e a atualização da store de abas já ocorreram: não há rollback coordenado na store de sessões.
2. **Histórico — UI ⇄ Store ⇄ IPC ⇄ serviço ⇄ repositório:**
   - `src/components/workspace/TabWorkspaceView.vue:L482-490` abre o histórico com `reset=true`.
   - `src/stores/sessions.ts:L83-120` controla cursor, deduplicação e cache; `src/ipc/sessions.ts:L22-29` e `src-tauri/src/commands/sessions.rs:L23-33` encaminham os argumentos.
   - `src-tauri/src/workspace/manager.rs:L2528-2541` consulta `tab_sessions().list_by_tab`; `src-tauri/crates/mt-db/src/repos/tab_sessions.rs:L66-75` executa a leitura no banco e retorna a página. A store transforma essa página no histórico acumulado devolvido à UI.
3. **Scrollback — origem e validação dos dois IDs:**
   - `src/components/workspace/TabWorkspaceView.vue:L504-512` obtém `TabSessionId` da sessão selecionada e `SessionId` de `detail.panes[0].sessionId`; `L544-550` solicita continuação com `reset=false`.
   - `src/types/bindings.ts:L11776-11782` registra a relação entre resumo do agrupamento e sessões dos painéis.
   - `src/stores/sessions.ts:L143-186` → `src/ipc/sessions.ts:L36-46` → `src-tauri/src/commands/sessions.rs:L45-58` → `src-tauri/src/workspace/manager.rs:L2561-2575` → `src-tauri/crates/mt-db/src/repos/scrollback.rs:L139-157`.
   - No banco, `src-tauri/crates/mt-db/src/repos/scrollback.rs:L210-223` exige o vínculo entre ambos os IDs em `tab_session_panes`. `L225-239` usa o cursor para consultar `session_scrollback` por sequência crescente; `L286-296` devolve o cursor seguinte. A store guarda cache/cursor por `SessionId`, não por `TabSessionId` nem por `PaneId`.

A causa da issue é a ausência de documentação na fronteira pública que reúne esses contratos distribuídos. A correção deve explicitar o comportamento observado, sem inventar garantias de atomicidade, concorrência ou retenção ilimitada.

## Arquivos afetados

- **Implementação:** `src/stores/sessions.ts` — adicionar somente comentários JSDoc em pt-BR na declaração da store e em seus métodos públicos.
- **Artefato desta etapa:** `docs/issues/28/plan.md` — este plano.
- **Validação existente, sem edição prevista:** `tests/frontend/workspace/sessions.store.spec.ts`.

Os arquivos de UI, IPC, tipos gerados, Rust e repositórios citados acima são referências para a verificação dos contratos.

## Execuções propostas

1. Trabalhar na worktree dedicada e conferir o estado inicial do Git. Registrar a revisão-base para a comparação documental; preservar alterações pré-existentes.
2. Adicionar JSDoc à declaração de `useSessionsStore` descrevendo sua finalidade: coordenar histórico de sessões de aba, detalhes, scrollback por sessão de painel, busca e eventos. Explicar as chaves `TabId`, `TabSessionId` e `SessionId` e que `loading`/`error` são compartilhados entre operações, sem representar um contador de requisições concorrentes.
3. Documentar `startNewSession` com parâmetro, retorno e falhas:
   - Recebe `TabId`, não um ID de sessão.
   - Delega à store de abas o arquivamento da sessão anterior e a criação de sessões/PTYS novos para os painéis, preservando o layout; o histórico persistido continua sujeito à retenção.
   - Atualiza a store de abas e recarrega o histórico com `loadSessions(tabId, true)` antes de resolver com `TabView_Serialize`.
   - Explicar a falha parcial: rejeição na atualização do histórico não implica desfazer a sessão recém-criada.
   - A ação não limpa explicitamente todos os caches de detalhes, scrollback e busca da store.
4. Documentar `loadSessions`:
   - `reset=true` por padrão: consulta a primeira página com cursor nulo e substitui o histórico da aba após sucesso.
   - `reset=false`: utiliza o cursor salvo e agrega a página ao cache. Sem cursor, retorna o cache ou `[]`, sem chamada IPC; não serve como primeira carga.
   - `limit=30` é o tamanho solicitado da página, não o máximo da coleção retornada.
   - Deduplicação pelo ID da sessão de aba, prevalecendo a última ocorrência; ordenação por `seq` decrescente; cursor atualizado a partir da resposta.
   - Retorna a coleção consolidada, não apenas os itens da última página. Reiniciar a paginação não esvazia previamente o cache nem reinicia todos os caches da store.
5. Documentar `loadScrollback`:
   - `tabSessionId` identifica o agrupamento/sessão da aba; `sessionId` identifica a sessão persistida de um painel pertencente ao agrupamento, obtida de `loadSessionDetail(tabSessionId).panes`. `sessionId` não é `PaneId` ou ID de PTY.
   - Incluir exemplo curto em JSDoc usando `detail.panes[0]?.sessionId`, com guarda para lista vazia, e os dois argumentos na ordem correta.
   - O pertencimento é validado no backend; cache e cursor locais usam `SessionId` como chave.
   - `reset=true` e `limit=100` são os padrões. A primeira página substitui o cache após sucesso; continuação agrega, deduplica por `seq` e ordena crescentemente. Sem cursor na continuação, devolve cache/`[]` sem IPC.
   - Retorna todos os chunks consolidados. O limite refere-se à página solicitada; não garante esse número de itens na resposta.
6. Completar de forma breve a documentação dos demais métodos públicos para coerência da API:
   - `sessionsForTab`, `detailFor`, `scrollbackFor`: consultas locais, sem IPC, com retornos vazios/`null` em ausência de cache.
   - `loadSessionDetail`: cache por `TabSessionId` e atualização do resumo correspondente.
   - `searchInTab`: busca com texto aparado, limite padrão 50; entrada vazia limpa resultados da aba e retorna `[]` sem IPC.
   - `handleSessionStarted` e `handleSessionArchived`: atualização/inserção de resumo e sincronização do resumo no detalhe já carregado; não são comandos para criar ou arquivar sessões no backend.
   - `resetForTests`: limpeza do estado local para isolamento de testes, sem apagar dados persistidos.
7. Explicar o tratamento compartilhado de erros das operações assíncronas: ao iniciar a requisição, limpam `error` e ativam `loading`; na falha registram a mensagem de `useIpcError`, relançam o erro original e desativam `loading` em `finally`. Retornos antecipados de paginação/busca vazia não percorrem esse ciclo.
8. Concluir todo o bloco documental antes de validar. Revisar cada afirmação contra as referências deste plano, preservando assinaturas, valores padrão, ordem de chamadas, exports, comportamento e formatação do código.

## Testes de regressão e validação em lote

Esta é uma alteração reversível, exclusivamente documental. Reutilizar os testes existentes e uma comparação automatizada de código emitido; não criar testes que procurem frases específicas nos comentários nem adicionar testes funcionais sem alteração de comportamento.

- A suíte `tests/frontend/workspace/sessions.store.spec.ts:L40-85` já exercita ordenação do histórico, detalhe e scrollback com IDs distintos. `L87-115` cobre busca, consulta vazia, eventos e limpeza de estado.
- Limite da cobertura existente: ela não prova falha parcial de `startNewSession` nem todos os ramos de paginação. Essas afirmações documentais devem ser confrontadas com os trechos exatos da implementação; a comparação do código emitido comprova que esta mudança não altera esses ramos.
- Depois de terminar todos os comentários, executar o bloco de validação da próxima seção: comparação documental, suíte direcionada, lint do arquivo e tipos. Reunir falhas, corrigir em lote e repetir somente as verificações afetadas. Não executar esses checks durante a elaboração deste plano.
- Não é necessário criar migration, executar build nativo ou iniciar PTYs para comprovar uma mudança apenas de comentários.

## Banco de dados

Nenhuma migration. A documentação descreve os contratos já existentes de `tab_sessions`, `sessions`, `tab_session_panes` e `session_scrollback`.

## Riscos de quebra e Não-Regressão

- **Documentar semântica incorreta de `reset`:** não afirmar que `reset=false` inicia a carga nem que `reset=true` limpa todos os caches. Conferir os retornos antecipados e as atribuições em `sessions.ts:L88-112` e `L149-177`.
- **Inverter identidades:** explicitar origem e ordem dos IDs, mantendo o exemplo alinhado com `TabWorkspaceView.vue:L504-512` e com a validação de pertencimento no banco.
- **Prometer atomicidade ponta a ponta:** registrar a falha possível da segunda etapa de `startNewSession` depois da troca efetiva. Não descrever uma rejeição como prova de que a sessão anterior permaneceu ativa.
- **Confundir sessão limpa com apagamento de histórico:** descrever arquivamento, renovação dos PTYs e retenção; não prometer eliminação dos registros anteriores.
- **Sugerir segurança de concorrência inexistente:** não tratar `loading` como contador de chamadas nem prometer cancelamento, serialização ou proteção contra respostas fora de ordem.
- **Alteração acidental de contrato:** a comparação do código emitido e a revisão do diff devem garantir que a implementação altera apenas comentários; a suíte existente, lint e checagem de tipos preservam a integração frontend.

## Validação

Executar na worktree de implementação após concluir o bloco autorizado. O script abaixo pressupõe que `HEAD` seja a revisão-base de `src/stores/sessions.ts` anterior à implementação; se houver mudanças prévias nesse arquivo, usar a revisão-base registrada e revisar separadamente essas diferenças.

```bash
node --input-type=module <<'NODE'
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

const path = 'src/stores/sessions.ts';
const before = execFileSync('git', ['show', `HEAD:${path}`], { encoding: 'utf8' });
const after = readFileSync(path, 'utf8');
const emit = (source) => ts.transpileModule(source, {
    fileName: path,
    compilerOptions: {
        target: ts.ScriptTarget.ESNext,
        module: ts.ModuleKind.ESNext,
        removeComments: true
    }
}).outputText;
assert.notEqual(after, before, 'A documentação precisa ter sido acrescentada.');
assert.equal(emit(after), emit(before), 'A alteração deve preservar o código executável.');
const source = ts.createSourceFile(path, after, ts.ScriptTarget.Latest, true);
const required = new Set(['useSessionsStore', 'startNewSession', 'loadSessions', 'loadScrollback']);
function visit(node) {
    const name = node.name?.getText(source);
    if (required.has(name)) {
        assert.ok(ts.getJSDocCommentsAndTags(node).length > 0, `JSDoc ausente: ${name}`);
        required.delete(name);
    }
    ts.forEachChild(node, visit);
}
visit(source);
assert.equal(required.size, 0, 'Todas as declarações essenciais precisam ser encontradas.');
console.log('Documentação essencial presente e código executável preservado.');
NODE
npm run test -- tests/frontend/workspace/sessions.store.spec.ts
npx --no-install eslint --max-warnings 0 src/stores/sessions.ts
npm run typecheck
git diff --check
git diff -- src/stores/sessions.ts
```

Critérios cumulativos de aceite: comandos concluídos com sucesso; diff da implementação restrito a comentários; JSDoc em pt-BR visível na store e nas ações; efeitos de nova sessão, falha parcial, paginação por `reset`, valores padrão, retorno acumulado e distinção dos IDs descritos corretamente. O script verifica presença e invariância executável; a revisão semântica dos contratos é indispensável para comprovar a qualidade do conteúdo documental.

## Skills Aplicáveis

- `code-review` — revisão final da precisão dos contratos, do escopo documental e da ausência de mudanças executáveis.
- `vue-typescript-best-practices` — referência para preservar a tipagem e distinguir semanticamente os aliases existentes, sem alterar bindings gerados.
- `vitest-skill` — ao executar e interpretar a suíte direcionada já existente; não exige introduzir novos testes para esta alteração documental.

Não pressupor skills ausentes do catálogo nem aplicar padrões de outros projetos que substituam a implementação Pinia atual.
