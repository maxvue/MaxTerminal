# Plano de implementação — Issue #12

### Descrição e Causa Raiz

**Veracidade: confirmada.** Na revisão `feb8f520ff5c7a8f9cace41e393a088c24c5fe9c`, o histórico ainda carrega cada execução completa para produzir um `GroupRunSummary`. A otimização solicitada não está implementada. Este documento é o único arquivo alterado nesta etapa de planejamento.

**Causa Raiz Comprovada:** `src-tauri/crates/mt-commands/src/groups/repo.rs:L1316-1405` seleciona até `limit + 1` IDs, retém a página e chama `load_run_by_id_in_conn` individualmente para cada ID (`L1385-1387`). Esse carregador consulta o registro pai (`L484-514`), desserializa o snapshot do grupo (`L538-552`), consulta todos os itens (`L554-609`), desserializa seus snapshots e constrói `GroupRunItem` (`L611-664`). Depois calcula os contadores (`L666`) e retorna `GroupRun`. O resumo copia os campos do pai e os contadores, descartando o vetor de itens (`L1388-1404`).

Para R execuções efetivamente retornadas, são **1 + 2R SELECTs de domínio**; para 100, são 201. O registro adicional usado como sentinela não passa pelo carregador completo. Além das consultas, há alocação e desserialização proporcionais aos itens das execuções da página. Os contadores exigem conhecer os estados dos itens, mas não seus snapshots, IDs de recursos ou objetos completos. O snapshot do próprio grupo faz parte do contrato e precisa continuar sendo retornado. `output_tail` já não é selecionado pelo carregador atual: sua remoção não é parte do ganho alegado.

**Rastreamento reverso de dados e fluxo causal:**

1. **DB → Repository:** tabelas `command_group_runs` e `command_group_run_items`, definidas em `src-tauri/crates/mt-db/migrations/0009_15_command_groups.sql:L46-90`. O índice `ix_group_runs_project_time` (`L64`) atende ao filtro do projeto; `UNIQUE(run_id, position)` (`L89`) fornece índice para localizar os itens de cada execução. `repo.rs:L242-260` executa a leitura via `Db::read_blocking`, que obtém uma conexão do pool em `src-tauri/crates/mt-db/src/lib.rs:L477-484`.
2. **Repository → Service:** `src-tauri/crates/mt-commands/src/groups/service.rs:L594-608` encaminha projeto, cursor, limite e `active_only`, com padrão `false`. O serviço não elimina o carregamento N+1.
3. **Service → API/Controller:** o comando Tauri `command_group_run_list`, em `src-tauri/src/commands/command_groups.rs:L132-146`, retorna `GroupRunListPage`. A rota é IPC Tauri; não há controller HTTP neste caminho.
4. **IPC → Store:** `src/types/bindings.ts:L398` chama o comando; `src/ipc/commandGroups.ts:L73-82` expõe `listRuns`; `src/stores/commandGroups.ts:L600-628` implementa `loadHistory`, mescla resumos por ID/versão e armazena `nextCursor`. O contrato está em `src-tauri/crates/mt-commands/src/groups/model.rs:L690-772`.
5. **Store → UI:** a store disponibiliza histórico e cursor (`src/stores/commandGroups.ts:L788-789`) e exporta `loadHistory` (`L811`). A busca no frontend não encontrou componente Vue chamando esse carregamento ou consumindo esses mapas. Portanto, não se atribui o problema a uma tela/botão já conectado. A chamada IPC descrita na issue e o método público do repositório são os pontos de reprodução. Há ainda um consumidor nativo de `list_runs(..., 100, true)` na exclusão de projetos (`service.rs:L657-662`).

**Verificação realizada nesta etapa:** experimento Python/SQLite exclusivamente em memória, usando a migration existente e as três consultas extraídas do código atual. Com 101 execuções, três itens por execução e página de 100, o rastreamento contou **201 SELECTs** e a leitura materializou **300 linhas de itens**. Uma consulta candidata com CTE paginada, `LEFT JOIN` e agregação retornou 101 linhas, incluindo sentinela, em **um statement**, preservando IDs/ordem e os contadores parciais exercitados (`total`, `running`, `succeeded`, `failed`). O `EXPLAIN QUERY PLAN` usou `ix_group_runs_project_time` e `sqlite_autoindex_command_group_run_items_2`; também indicou árvores temporárias para ordenação/agrupamento. SQLite do experimento: `3.54.0`.

Esse experimento confirma a estrutura SQL, mas não executa Rust/IPC nem mede latência real. Os snapshots sintéticos eram JSON válido para o schema, não fixtures completas do domínio. A prova da desserialização desnecessária é o código citado; testes integrados no SQLite embarcado permanecem obrigatórios na implementação.

### Arquivos afetados

| Arquivo | Alteração prevista |
| --- | --- |
| `src-tauri/crates/mt-commands/src/groups/repo.rs` | Substituir a listagem de IDs seguida de carregamento completo por consulta paginada e agregada, com mapeamento direto de resumos. |
| `src-tauri/crates/mt-commands/tests/groups_repo.rs` | Acrescentar fixtures, instrumentação isolada e testes de consultas, projeção, contadores, paginação e compatibilidade. |
| `src-tauri/crates/mt-commands/Cargo.toml` | Habilitar `rusqlite` com `features = ["trace", "hooks"]` exclusivamente em `dev-dependencies`, herdando a versão do workspace. |

O artefato desta etapa é `docs/issues/12/plan.md`. Os arquivos de contrato, store, IPC, serviço e migration são referências para validação, sem alteração funcional prevista. As features de teste não requerem atualização de versão ou novas dependências externas.

### Execuções propostas

1. Trabalhar na worktree dedicada, preservando o estado previamente existente. Manter a assinatura de `CommandGroupRepo::list_runs` e os DTOs. Preparar em conjunto a alteração do repositório, a configuração de instrumentação e todos os testes abaixo antes da primeira rodada de testes/tipos/build.
2. Preservar o parser do cursor e suas mensagens/códigos em `repo.rs:L1300-1314`, `limit.clamp(1, 100)`, o predicado keyset `(created_at < ts OR (created_at = ts AND id < cursor_id))` e a definição de ativo como `finished_at IS NULL`. Não reinterpretar atividade pelo enum `status`.
3. Substituir os quatro caminhos de busca de IDs por uma consulta por chamada, com a seguinte estrutura: CTE `page` selecionando explicitamente os campos necessários de `command_group_runs`, filtrada por projeto/atividade/cursor, ordenada por `created_at DESC, id DESC` e limitada a `clamped_limit + 1`; consulta externa com `LEFT JOIN command_group_run_items i ON i.run_id = page.id`, agrupamento pela chave única `page.id` e ordenação final explícita pelos mesmos campos.
4. Aplicar o `LIMIT` **dentro** da CTE, antes do join. Assim, somente os itens das até 101 execuções candidatas participam da agregação. Não agregar o histórico inteiro nem limitar linhas de itens. Usar variantes de predicados escolhidas pelo programa e parâmetros vinculados para todos os valores externos; nenhum ID/projeto/cursor interpolado no SQL.
5. Projetar todos os campos de `GroupRunSummary`, inclusive `snapshot_json` do pai, mas sem `request_hash` ou colunas de detalhe dos itens. Calcular `COUNT(i.id)` como `total` — nunca `COUNT(*)` no `LEFT JOIN` — e `SUM(CASE WHEN i.status = '<estado>' THEN 1 ELSE 0 END)` para os nove estados: `pending`, `starting`, `running`, `cancelling`, `succeeded`, `failed`, `skipped`, `cancelled`, `interrupted`. Execuções sem itens devem ter todos os contadores zerados.
6. Mapear diretamente as linhas para os resumos, com helper privado se necessário. Manter `status` da execução conforme persistido; não recalculá-lo a partir dos contadores durante a leitura. Preservar validação de IDs opcionais, modos, estados, timestamps, campos nulos, versão e erros de persistência equivalentes aos do carregador atual para os campos efetivamente retornados. Converter contadores sem casts silenciosos que possam truncar valores e verificar sua consistência com o total.
7. Determinar `has_more` pela quantidade de linhas agregadas, remover a sentinela e só então desserializar os snapshots de grupo das linhas devolvidas. Um snapshot inválido exclusivamente na sentinela não deve passar a impedir uma página válida. Gerar `next_cursor` com `created_at:id` do último resumo retornado, exatamente como hoje (`repo.rs:L1407-1413`).
8. Manter `load_run_by_id_in_conn` para detalhe, idempotência, cancelamento e atualizações. O histórico não deve chamá-lo. O resumo continuará carregando o snapshot do grupo; somente a leitura/materialização dos detalhes dos itens será eliminada. A consulta única também evita leituras separadas de pai/itens em instantes diferentes durante atualizações concorrentes.
9. Habilitar instrumentação apenas em testes. Na fixture específica, configurar `DbConfig.reader_pool_size = 1`, criar os dados e instalar `trace_v2`/`authorizer` na conexão leitora via `db.read_blocking`. Liberar o empréstimo da conexão antes de chamar `repo.list_runs`; não chamar o repositório de dentro desse fechamento. Remover os callbacks após o caso, inclusive no caminho de falha por meio de guarda de limpeza.
10. Usar eventos `SQLITE_TRACE_STMT` para contar statements de domínio `SELECT`/`WITH` que consultem as tabelas de execuções, excluindo setup, PRAGMAs e verificações do pool. O callback de trace é ponteiro de função: usar coleta isolada por thread durante a chamada síncrona, sem contador global compartilhado entre testes. No `authorizer`, negar a leitura de qualquer coluna de itens exceto `id`, `run_id` e `status`; isso detecta a reintrodução de snapshots/detalhes mesmo que alguém esconda N+1 em um join. As APIs foram conferidas no rusqlite `0.40.2` disponível no ambiente; a configuração de produção atualmente usa `0.40`.

### Testes de regressão e validação em lote

Adicionar casos com prefixo `list_runs_` em `groups_repo.rs`, usando banco temporário com migrations reais e snapshots completos do domínio:

- **Quantidade constante de consultas:** páginas com 0, 1, 25 e 100 execuções, variando itens por execução; exigir exatamente **um statement de leitura de domínio** por chamada válida. Incluir 101 execuções para comprovar lookahead e retorno de somente 100. A implementação atual reprovaria o cenário de 100 por executar 201 SELECTs.
- **Ausência de materialização dos itens:** com `authorizer` restringindo as colunas de itens, a listagem deve funcionar. Em caso independente, persistir `snapshot_json = '{}'` em um item, preservando o CHECK de JSON válido: o histórico deve continuar retornando o resumo, enquanto `get_run` deve manter o erro de desserialização do detalhe. Não desabilitar constraints para esse teste.
- **Paridade de contrato:** para dados válidos, comparar cada campo do resumo com a projeção do `GroupRun` de `get_run`, inclusive o snapshot do grupo, versão, códigos de saída e timestamps. Efetuar essa comparação fora da janela de contagem de consultas. Cobrir os três modos, IDs opcionais nulos e grupo excluído com `group_id = NULL`, mantendo o snapshot histórico.
- **Contadores:** distribuir itens pelos nove estados e comparar com `aggregate::compute_counts` (`aggregate.rs:L8-27`). Incluir uma execução sem itens e outra com múltiplos itens no mesmo estado; verificar `total == soma dos estados`.
- **Paginação e isolamento:** várias páginas, timestamps iguais com desempate por ID, projeto vazio e dois projetos intercalados; demonstrar ausência de duplicações/omissões e cursor da última linha devolvida. Cobrir `limit = 0`, `1`, `100` e acima de `100`, página exatamente cheia sem próxima linha, última página parcial e página após o fim.
- **Quatro variantes de filtro:** com/sem cursor e `active_only` verdadeiro/falso. Usar registros controlados para provar que a atividade depende de `finished_at`, inclusive quando o estado isoladamente sugeriria outro resultado. Manter os erros atuais para cursor sem separador e timestamp inválido; cursor vazio/em branco segue sendo tratado como ausente.
- **Sentinela e erros:** snapshot de grupo estruturalmente inválido em uma linha efetivamente devolvida deve produzir erro de persistência; o mesmo snapshot apenas na linha extra não deve invalidar a página anterior. Snapshot de item inválido não deve contaminar o resumo.
- **Plano de execução:** executar `EXPLAIN QUERY PLAN` da consulta efetivamente usada com fixtures de vários projetos/páginas. Verificar acesso aos itens por `run_id` e ausência de agregação prévia de todos os itens do banco. Evitar comparar strings completas do plano, pois variam por versão SQLite. Árvores temporárias já observadas não justificam migration sem medição adicional.

**Ordem de trabalho exigida:** implementar todo o bloco autorizado e seus testes; depois executar a rodada abaixo. Consolidar as falhas, corrigir o conjunto e repetir apenas as validações afetadas e os checks obrigatórios. Não fazer ciclos de build a cada edição. Na etapa atual não foram executados Cargo, Vitest, typecheck ou build; somente inspeção e experimento SQL em memória.

### Banco de dados

**Nenhuma migration.** As tabelas e índices necessários já existem na migration `0009_15_command_groups.sql`. A alteração é exclusivamente de consulta/projeção. Manter WAL, constraints e política de sanitização existentes. Não persistir contadores redundantes nem criar índices especulativos para esta correção.

### Riscos de quebra e Não-Regressão

| Risco | Medida e prova de não-regressão |
| --- | --- |
| Join multiplicar resultados ou `LIMIT` cortar itens, alterando páginas/contadores | Limitar pais primeiro, agregar por ID e testar páginas com múltiplos itens, empates e sentinela. |
| `COUNT(*)` contar uma linha em execução sem itens | Usar `COUNT(i.id)` e testar todos os contadores zerados. |
| Omitir um dos nove estados ou recalcular incorretamente o estado do pai | Comparar contadores com `compute_counts`; comparar `status` e demais campos com o detalhe. |
| Quebra dos filtros, cursor ou isolamento por projeto | Exercitar as quatro variantes e limites, com projetos intercalados e timestamps iguais. |
| Mudança acidental em snapshots e validação | Preservar snapshot do grupo e erros dos campos retornados. Documentar como mudança intencional que corrupção restrita ao detalhe de um item deixa de bloquear o histórico; `get_run` continua validando esse detalhe. |
| Regressão em cancelamento, recuperação, idempotência ou exclusão de projeto | Preservar o carregador completo e executar as suítes `groups_repo`, `groups_runner` e `groups_service`; assegurar paridade da seleção `active_only`. |
| Eliminar N+1 mas agregar todas as execuções do banco | CTE limitada antes do join, inspeção de `EXPLAIN QUERY PLAN` e fixture com dados fora da página. O número de statements será constante; o custo de leitura continuará proporcional aos itens das execuções selecionadas. |
| Instrumentação introduzir interferência ou custo em produção | Features em `dev-dependencies`, callbacks exclusivos dos testes, conexão controlada e coleta isolada. |

Não se promete redução de latência em milissegundos sem benchmark no aplicativo. O critério determinístico é eliminar os 201 statements e a leitura dos detalhes de itens mantendo os resultados do contrato para dados válidos.

### Validação

Após concluir implementação e testes, executar na raiz da worktree:

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p mt-commands --test groups_repo list_runs_
cargo test --manifest-path src-tauri/Cargo.toml -p mt-commands --test groups_repo --test groups_service --test groups_runner
cargo check --manifest-path src-tauri/Cargo.toml -p mt-commands
npm exec -- vitest run tests/commandGroups/commandGroupsIpcAndComposables.spec.ts tests/commandGroups/commandGroupsStore.spec.ts
git diff --check
```

**Prova conclusiva:** o primeiro comando deve passar os casos instrumentados, incluindo 100 resumos e um único statement de domínio com bloqueio de leitura das colunas de detalhe, além da paridade de contadores e paginação. As suítes adicionais comprovam compatibilidade do repositório/serviço/runner e dos consumidores IPC/store existentes; mocks de frontend, isoladamente, não provam desempenho SQL. Revisar o diff final para confirmar o escopo e preservar os contratos públicos.

### Skills Aplicáveis

- `database-optimizer`: eliminação de N+1, projeção mínima, agregação restrita à página, análise de índices e plano de execução. Aplicada na investigação deste planejamento.
- `code-review`: revisão da implementação e dos testes antes de integrar, com foco em contrato, contadores, paginação e regressões nos consumidores do repositório.

As skills devem ser carregadas quando aplicáveis à execução; esta correção está concentrada no backend Rust/SQLite.
