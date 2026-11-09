# Plano de implementação — Issue #10

## Descrição e Causa Raiz

**Veracidade: confirmada por inspeção do fluxo executável na worktree `wt-implement-issue-10`.** A listagem continua materializando todas as issues do repositório/estado antes de filtrar e paginar. A implementação solicitada ainda não existe nesse caminho. As referências abaixo correspondem à versão inspecionada; algumas linhas diferem das registradas na auditoria.

### Causa Raiz Comprovada

1. `src-tauri/crates/mt-github/src/cache.rs:L649-690`: as doze combinações de estado/ordenação selecionam todos os números, sem `LIMIT`, `OFFSET` ou os filtros adicionais.
2. `src-tauri/crates/mt-github/src/cache.rs:L692-699`: para cada número, chama `read_issue_summary_and_body_conn` e só depois aplica `matches_issue_filter`.
3. `src-tauri/crates/mt-github/src/cache.rs:L1439-1445`: esse helper delega para `read_issue_full_row_conn` e descarta os campos do plano depois de carregá-los.
4. `src-tauri/crates/mt-github/src/cache.rs:L1323-1375`: o carregador executa uma consulta por issue, incluindo `body_sanitized`, `plan_markdown` e metadados do plano.
5. `src-tauri/crates/mt-github/src/cache.rs:L1402-1427` e `L1276-1296`: cada carregamento dispara outra consulta para as labels e monta o resumo completo.
6. `src-tauri/crates/mt-github/src/cache.rs:L701-705`: somente após materializar todos os resultados filtrados calcula o total e executa `skip(offset).take(limit)` em Rust.
7. `src-tauri/crates/mt-github/src/cache.rs:L707-718` e `L1558-1577`: ainda executa a contagem de abertas e a leitura dos metadados de sincronização.

Para N issues existentes e sem alterações concorrentes, são **1 + 2N consultas do carregamento e mais 2 auxiliares, totalizando 2N + 3 SELECTs por chamada**. `limit=1` não reduz esse trabalho. Corpos e planos são alocados temporariamente para cada issue; os resumos correspondentes ao filtro ficam acumulados até a paginação. Trocar o offset repete o carregamento integral.

Não foram medidos latência, consumo de memória ou trace SQL nesta etapa; a comprovação é estrutural, baseada nas consultas e no laço acima. A classificação de severidade da auditoria não constitui uma medição de desempenho.

### Rastreamento reverso e fluxo de dados

- **DB → repositório:** `github_issues`, `github_issue_labels`, `github_labels` e `github_sync_state` são definidas em `src-tauri/crates/mt-db/migrations/0010_16_github.sql:L33-127`. A relação issue/label possui chave composta e índices existentes. O problema está na estratégia de leitura de `GithubCacheRepo::list_issues`, não na ausência de paginação no contrato.
- **Repositório → service:** `src-tauri/crates/mt-github/src/lib.rs:L206-225` resolve o repositório, consulta o cache e pode sincronizar/refazer a consulta na primeira abertura.
- **Service → comando IPC:** `src-tauri/src/commands/github.rs:L71-83` recebe `project_id` e `IssueFilter` em `github_issues_list` e encaminha ao serviço. Este fluxo usa IPC Tauri; não existe rota/controller HTTP intermediário.
- **IPC → store:** `src/ipc/github.ts:L48-54` chama o binding; `src/stores/githubIssues.ts:L186-209` combina os filtros e grava itens, total, contagem de abertas, stale e metadados retornados.
- **Store → UI:** `src/components/github/IssuesTab.vue:L22-28`, `L104-108` e `L147-157` renderiza os itens/total e inicia o carregamento. `src/composables/useIssueFilters.ts:L25-76` refaz a listagem ao alterar filtros, normalmente reiniciando o offset.
- **Persistência de origem:** `src-tauri/crates/mt-github/src/cache.rs:L1128-1253` sanitiza título/corpo, serializa responsáveis e persiste issue/labels. A busca atual opera sobre o corpo sanitizado.
- **Execução da leitura:** `src-tauri/crates/mt-db/src/lib.rs:L467-475` e `src-tauri/crates/mt-db/src/reader.rs:L92-113` executam a closure em uma conexão somente leitura via `spawn_blocking`. Isso evita executar SQL na thread da UI, mas não elimina o custo nem a espera pela resposta.

Outros consumidores do mesmo caminho: sincronização normal/304/offline em `src-tauri/crates/mt-github/src/sync.rs:L111-179` e prévia do ciclo de bugs em `src-tauri/crates/mt-github/src/bulk.rs:L44-63`.

## Arquivos afetados

Arquivos a alterar na implementação:

| Arquivo | Alteração prevista |
| --- | --- |
| `src-tauri/crates/mt-github/src/cache.rs` | Consultas paginadas e parametrizadas, predicados compartilhados, projeção de resumo, labels em lote, helper síncrono privado e testes unitários de orçamento SQL. |
| `src-tauri/crates/mt-github/tests/cache.rs` | Fixtures e regressões pela API pública para filtros, paginação, metadados, isolamento e detalhe completo. |
| `src-tauri/crates/mt-github/Cargo.toml` | Habilitar a feature `trace` de `rusqlite` apenas em `dev-dependencies`, herdando a versão do workspace. |

O artefato desta etapa é `docs/issues/10/plan.md`. Os demais caminhos citados são referências de contratos e consumidores. O plano não exige mudança de DTO, bindings, componentes, store, schema ou transporte remoto.

## Execuções propostas

### 1. Fixar os contratos e preparar o bloco de implementação

Trabalhar na worktree dedicada e preparar implementação e testes como um único bloco antes de executar testes, checagem de tipos ou build. Usar os comportamentos de `IssueFilter` (`src-tauri/crates/mt-github/src/model.rs:L251-291`), `matches_issue_filter` (`cache.rs:L1448-1543`) e `extract_canonical_facets` (`src-tauri/crates/mt-github/src/labels.rs:L117-159`) como referência de compatibilidade.

Extrair o corpo síncrono da leitura para um helper privado, por exemplo `list_issues_conn`, chamado diretamente pela closure de `Db::read`. O mesmo helper será exercitado com conexão instrumentada nos testes; não criar uma implementação SQL exclusiva para os testes. Manter a assinatura pública e o tratamento de erros/metadados.

### 2. Construir uma única definição dos filtros SQL

Criar uma representação interna com fragmentos SQL controlados pelo código e parâmetros vinculados. Reutilizar exatamente os mesmos predicados e valores na consulta de contagem e na consulta da página. O `repo_key` deve restringir a consulta principal e todas as relações/subconsultas.

- **Estado:** aplicar `open`, `closed` ou ausência desse predicado para `all`.
- **Plano:** filtrar `has_plan` apenas quando informado.
- **Autor:** aplicar o mesmo `trim` de Rust ao parâmetro; vazio desativa o filtro. Comparação ASCII case-insensitive, compatível com `eq_ignore_ascii_case`.
- **Responsável:** preservar a interpretação de `assignees_json` como `Vec<String>`; JSON inválido, objeto ou array com valores não textuais equivale a lista vazia, como no `serde_json::from_str(...).unwrap_or_default()` atual. Para preservar precisamente essa tolerância, registrar uma função escalar privada/determinística nessa conexão de leitura, que receba JSON e login normalizado e retorne a pertinência usando `serde_json` e `eq_ignore_ascii_case`. A feature `functions` já está habilitada no workspace. Registrar no caminho compartilhado usado pela produção e pelos testes, sem SQL interno ou consultas por elemento. Executar esse predicado somente quando houver filtro de responsável não vazio.
- **Labels:** OR dentro de cada lista de filtros e AND entre grupos distintos. Categorias e customizadas usam existência; `custom_labels` aceita qualquer nome presente, inclusive um nome canônico, como hoje.
- **Estado/encerramento canônicos:** comparar a label **ativa**, isto é, a de menor `order` do catálogo canônico presente na issue. Não substituir por um `EXISTS` de qualquer label do grupo. Obter nomes/grupos/ordens de `canonical_labels_catalog()` em parâmetros/CTE, sem duplicar manualmente as 24 labels. Resolver a label ativa com subconsulta ordenada por essa ordem e `LIMIT 1`, então comparar com a lista solicitada. Não depender exclusivamente de `github_labels.canonical_group`: o helper atual reconhece o nome mesmo quando falta o registro do catálogo do repositório.
- Normalizar parâmetros com `trim`/case ASCII como no comportamento atual. Lista não vazia contendo apenas nomes vazios/desconhecidos não deve virar ausência de filtro. Preservar nomes customizados e a normalização feita por `to_repo_label`.
- Para listas variáveis, usar parâmetros JSON com `json_each` em vez de expandir um número irrestrito de placeholders. Produzir JSON válido com `serde_json`; o SQLite bundled do projeto fornece as funções JSON. Subconsultas correlacionadas devem aproveitar as chaves de repositório/issue existentes.
- **Busca textual:** normalizar com `trim().to_ascii_lowercase()`. Usar busca literal por substring, por exemplo `instr(lower(title), :q)` e `instr(lower(body_sanitized), :q)`, e busca em `CAST(issue_number AS TEXT)` removendo apenas um `#` inicial do parâmetro numérico. Não usar `LIKE` sem preservar `%`/`_` literais nem substituir por FTS tokenizado. Manter a união título/corpo/número e o comportamento de `#` isolado (substring numérica vazia corresponde a todos). A busca continua ASCII case-insensitive, não passa a ignorar diferenças Unicode.
- Vincular todos os valores; somente fragmentos fixos escolhidos pelos enums podem compor o SQL. Manter textos com aspas, percentuais e sublinhados como dados.

### 3. Contar e paginar antes da hidratação

Executar, na mesma conexão e em transação curta de leitura diferida para um snapshot consistente:

1. `SELECT COUNT(*)` sobre `github_issues` com todos os predicados, sem limite/offset, para `total_matching`.
2. `SELECT` explícito apenas das colunas de `IssueSummary`, com os mesmos predicados, ordenação, `LIMIT` e `OFFSET` vinculados.
3. Consulta única de labels apenas para os números retornados, conforme o passo seguinte.
4. Contagem de abertas do repositório, independente dos filtros adicionais e da página.
5. Leitura dos metadados de sincronização pelo helper existente.

Preservar `limit.unwrap_or(50).clamp(1, 200)` e offset padrão zero; converter os valores `u32` para `i64` sem truncamento. Página vazia por offset alto deve continuar retornando o total correto, contagem de abertas e metadados.

Ordenações:

- `UpdatedDesc`: `updated_at_ms DESC, issue_number DESC`.
- `CreatedDesc`: `created_at_ms DESC, issue_number DESC`.
- `CreatedAsc`: `created_at_ms ASC, issue_number ASC`.
- `CommentsDesc`: `comments_count DESC, updated_at_ms DESC, issue_number DESC`. O último campo passa a explicitar um desempate que hoje não está definido, necessário para páginas determinísticas.

Não projetar `body_sanitized`, `plan_markdown`, `plan_source`, `plan_comment_id` ou `plan_updated_at_ms` para os objetos de listagem. Apenas a busca textual pode ler o corpo dentro do SQLite. `has_plan` permanece no resumo. Contagem exata, busca por substring, ordenações sem índice adequado e offsets altos ainda podem percorrer muitos registros no SQLite; o objetivo é eliminar round-trips N+1 e materialização integral em Rust, não prometer custo total O(1).

### 4. Carregar labels da página em lote

Usar um único SELECT com `repo_key` e os números da página, `LEFT JOIN github_labels` pelas mesmas chaves atuais e ordenação por `issue_number, label_lower`. Agrupar em mapa por número; para cada resumo da página, aplicar `to_repo_label` e `extract_canonical_facets` sobre suas labels. Não fazer `LIMIT` diretamente em um JOIN de issues com labels, pois isso paginaria linhas da relação em vez de issues.

Preservar labels sem metadados, ordem alfabética atual das labels, cores/descrições de fallback, precedência canônica e issues sem labels. Se a página estiver vazia, omitir a consulta do lote. O mapa deve conter somente labels dos itens paginados.

Manter o carregamento completo usado por `get_issue_detail` e por outros consumidores dos helpers existentes. Remover `matches_issue_filter` do caminho de produção da listagem; sua lógica pode permanecer sob `cfg(test)` como oráculo de paridade, sem introduzir um fallback que volte a carregar o repositório inteiro.

### 5. Instrumentar e revisar o contrato de desempenho

Adicionar `rusqlite = { workspace = true, features = ["trace"] }` às dependências de desenvolvimento da crate. Usar `Connection::trace_v2` com eventos de statement e de linha na conexão que executa o helper real. A API de rusqlite 0.40 aceita `&self` e callback `fn(TraceEvent<'_>)`; isolar o coletor por teste/thread ou serializar os testes instrumentados para impedir contagem cruzada.

Instalar o trace somente depois de criar o schema e popular a fixture. Contar os SELECTs de aplicação, incluindo os iniciados por `WITH`, separando `BEGIN`/`COMMIT` e preparação da fixture. Não confundir linhas internas da execução SQL com linhas de resultados retornadas à aplicação. Desabilitar o trace ao terminar e não registrar valores reais de usuários.

Orçamento esperado: **5 SELECTs para página não vazia e no máximo 4 para página vazia**, independentemente de N e do tamanho da página. A consulta de issues deve devolver no máximo o limite; a de labels deve devolver somente associações da página. Contagens/metadados podem produzir suas linhas agregadas normalmente. Revisar os planos com `EXPLAIN QUERY PLAN` para a consulta de página e as subconsultas de labels, usando os índices existentes.

## Testes de regressão e validação em lote

Implementar todo o bloco acima e todos os testes abaixo antes da primeira rodada de testes, tipos ou build. Depois executar a validação dirigida, agrupar as falhas por causa, corrigir o lote e repetir os comandos afetados. Não executar suites entre alterações parciais do bloco.

### Testes determinísticos de desempenho

Criar no módulo de testes de `cache.rs` o teste `list_issues_query_budget_is_constant`:

- Popular bancos temporários com N=10 e N=1.000 issues, com múltiplas labels e corpos/planos grandes; ao popular pela API pública, persistir o conjunto de cada repositório em lote para não acionar remoção de ausentes entre chamadas parciais.
- Chamar o helper real com `limit=1`, offsets zero, intermediário e além do total; repetir com limites 50 e 200.
- Asserir o orçamento de SELECTs, quantidade de linhas da projeção de issues e correspondência do lote de labels aos números da página. Não aceitar apenas `items.len()==1`, pois o código atual já passa nessa asserção.
- Asserir que a projeção da listagem não contém colunas de corpo/plano. Sem busca, nenhum SELECT de aplicação deve ler essas colunas; com busca, o corpo pode aparecer no predicado, mas nunca como resultado materializado. Usar os statements efetivamente executados e as colunas da projeção, não apenas uma contagem de queries.
- Comparar os resultados com o cenário antigo demonstrado pela análise: 2N+3 SELECTs e N linhas de detalhe. Registrar evidência antes/depois na execução futura, sem afirmar que um trace da implementação antiga já foi coletado neste planejamento.
- Não usar limite de tempo absoluto como critério de aprovação. Tempo/volume de memória podem ser registrados como evidência complementar em máquina identificada.

### Matriz funcional pela API pública

Expandir `tests/cache.rs`, reutilizando `Db::open`, `TempDir`, `GithubCacheRepo` e inserção de fixtures:

1. **Paginações e totais:** limites ausente/0/1/50/200/acima de 200; offsets zero, intermediário, último e além do fim; banco vazio e repositório inexistente. Filtros devem reduzir o conjunto antes de paginar, inclusive quando os primeiros registros não correspondem.
2. **Ordenação:** quatro modos, datas iguais, empates de comentários e datas, várias páginas sem sobreposição ou lacunas em snapshot fixo.
3. **Filtros:** três estados; `has_plan` verdadeiro/falso/ausente; autor/responsável com variação ASCII de caixa, espaços no parâmetro e valor vazio; JSON de responsáveis válido, inválido, objeto e array heterogêneo.
4. **Labels:** OR intragrupo/AND entre grupos, duplicatas nos parâmetros, nomes desconhecidos, lista vazia versus lista contendo string vazia, labels customizadas e canônicas, catálogo de metadados ausente, duas labels de estado/encerramento na mesma issue. Uma label de menor prioridade presente não deve satisfazer o filtro quando outra é a ativa.
5. **Busca:** título, corpo sanitizado, número parcial e `#numero`; `#` isolado, vazio, espaços, aspas, `%`, `_`, barra invertida, termos não ASCII e ausência de resultados. Verificar que texto exclusivo de `plan_markdown` de comentário não passa a participar da busca.
6. **Isolamento:** dois repositórios com os mesmos números e nomes de labels, cores diferentes e estados diferentes; garantir que itens, labels, totais e metadados não se misturem.
7. **Resposta:** `total_matching` independente do offset/limite; `open_issues_count` independente dos filtros adicionais; preservação de `stale`, transporte, timestamps, labels faltantes e rate limit, inclusive com página vazia.
8. **Detalhe e sanitização:** manter o teste atual `cache_sanitizes_secrets_and_filters_by_canonical_labels_and_plan`; adicionar leitura de detalhe após listagem para confirmar corpo, comentários, plano e labels completos e sanitizados.
9. **Paridade:** comparar uma matriz de filtros e fixtures com o comportamento antigo mantido apenas como oráculo de testes; para empates de `CommentsDesc`, usar explicitamente o novo desempate por número.
10. **Snapshot:** em banco WAL temporário, testar leitura e escrita em conexões distintas com sincronização determinística por barreiras, sem sleeps, confirmando que total, itens e labels da chamada pertencem ao mesmo snapshot. A transação deve terminar também nos caminhos de erro.

O teste existente (`src-tauri/crates/mt-github/tests/cache.rs:L119-132`) cobre apenas uma listagem com `has_plan`; não cobre orçamento de consultas nem paginação.

## Banco de dados

**Migration: Nenhuma.** Usar as tabelas, chaves e índices de `0010_16_github.sql`, especialmente `github_issues(repo_key, issue_number)`, `idx_github_issues_repo_state_updated`, `idx_github_issues_repo_has_plan` e a chave `github_issue_labels(repo_key, issue_number, label_lower)`.

Manter WAL, conexões somente leitura e sanitização na escrita. A função escalar de responsáveis é registrada em conexão, não persistida no schema. Não alterar migrations históricas nem adicionar FTS para uma busca cuja semântica atual é substring literal. Índices adicionais para outras ordenações exigem evidência específica de `EXPLAIN QUERY PLAN`/medição e uma migration posterior; não são pré-requisito para remover este N+1.

## Riscos de quebra e Não-Regressão

| Risco concreto | Proteção exigida |
| --- | --- |
| Filtros SQL diferirem do filtro Rust, especialmente ASCII, curingas literais e JSON inválido | Matriz funcional e comparação com oráculo; função de responsáveis com a mesma desserialização. |
| Selecionar qualquer label em vez da label canônica ativa | Fixtures com múltiplas labels do mesmo grupo e ordens diferentes; catálogo canônico como fonte de prioridade. |
| JOIN multiplicar issues e corromper limite/contagem | Paginar issues primeiro, labels em segundo SELECT; testar múltiplas labels e issues sem labels. |
| Contagem e itens usarem predicados diferentes ou snapshots diferentes | Builder compartilhado e transação curta de leitura; teste de concorrência determinístico. |
| Paginação instável em empate de comentários | Desempate final por número, com cobertura de páginas adjacentes. Essa é a única ordenação antes não especificada que será explicitada. |
| Regressão em detalhe, sincronização offline/304 ou prévia de bugs | Preservar helpers de detalhe e DTOs; rodar a suite inteira de `mt-github`, além das regressões de metadados e limite 200. |
| Labels ou metadados cruzarem repositórios | Todas as consultas relacionais restringem `repo_key`; fixture de números iguais em repositórios distintos. |
| Testes instrumentados flutuarem por concorrência, inicialização ou uso de outra conexão | Trace no helper real, após fixture, coletor isolado e controle explícito das instruções de transação. |
| Confundir número constante de consultas com custo constante de CPU/I/O | Inspecionar plano de execução e registrar leituras/linhas; reconhecer varreduras de contagem/busca e o custo de offsets altos. |
| Snapshot prolongar retenção do WAL ou função escalar faltar em conexão do pool | Transação limitada à leitura da página; registrar a função na conexão efetivamente usada pelo helper; testes públicos pelo pool. |

## Validação

Comandos da **etapa futura de implementação**, executados a partir da raiz da worktree depois de concluir o bloco de código e testes:

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p mt-github --lib list_issues_query_budget_is_constant
cargo test --manifest-path src-tauri/Cargo.toml -p mt-github --test cache
cargo test --manifest-path src-tauri/Cargo.toml -p mt-github
cargo check --manifest-path src-tauri/Cargo.toml -p mt-github --all-targets
git diff --check
```

Critério conclusivo de aceite: orçamento SQL constante (até 5 SELECTs) para N=10 e N=1.000, hidratação limitada à página e suas labels, nenhuma projeção de corpo/plano na listagem, resultados/contagens/metadados compatíveis e todos os testes da crate aprovados. Uma listagem que retorne um item após hidratar N, ou que elimine o N+1 mas continue carregando todos os corpos/resumos, não atende ao aceite.

Esta etapa realizou inspeção de código e documentação e produziu somente o plano. Não executou testes, build, migração ou medição de desempenho. A documentação de rusqlite 0.40.2 foi consultada para confirmar a feature de desenvolvimento `trace` e a API `trace_v2` (`https://github.com/rusqlite/rusqlite/blob/v0.40.2/src/trace.rs`); o workspace declara rusqlite 0.40 em `src-tauri/Cargo.toml:L74`.

## Skills Aplicáveis

- **`database-optimizer`**: eliminação de N+1, projeção mínima, uso de índices existentes e comprovação por consultas/planos de execução; aplicada ao planejamento.
- **`security-and-hardening`**: revisar composição SQL parametrizada e preservação da sanitização ao implementar os filtros recebidos pelo IPC.
- **`code-review`**: revisão final da implementação e dos testes, com foco em paridade de filtros, isolamento por repositório e orçamento de consultas antes de integração.
