# Issue #11 — Carregamento em lote do catálogo de grupos

## Descrição e Causa Raiz

**Veracidade: confirmada no código atual e em reprodução SQL isolada.** A implementação solicitada ainda é necessária: a listagem seleciona os identificadores e carrega cada grupo individualmente, sem carregamento em lote.

### Causa Raiz Comprovada

- `src-tauri/crates/mt-commands/src/groups/repo.rs:L431-454`: `list_catalog_in_conn` consulta os IDs locais e, para cada ID, chama `load_group_by_id`.
- `src-tauri/crates/mt-commands/src/groups/repo.rs:L459-475`: repete a estratégia para os grupos globais.
- `src-tauri/crates/mt-commands/src/groups/repo.rs:L285-327`: o helper executa um SELECT dos atributos do grupo, embora seu ID já tenha sido selecionado.
- `src-tauri/crates/mt-commands/src/groups/repo.rs:L365-410`: o mesmo helper executa um SELECT das labels e outro dos itens para cada grupo.

Com projeto informado, o custo é `2 + 3 × (locais do projeto + globais)` SELECTs. Sem projeto, é `1 + 3 × globais`. O custo aparece mesmo quando os grupos não têm labels, pois a consulta da coleção vazia também é executada. Não há uma requisição IPC por grupo: a multiplicação ocorre dentro de uma única leitura do repositório SQLite.

### Rastreamento UI ⇄ Store ⇄ IPC ⇄ Service ⇄ DB

1. `src/components/workspace/TabWorkspaceView.vue:L215-237`: o observador imediato do projeto solicita `commandGroupsStore.load(projectId)`.
2. `src/stores/commandGroups.ts:L323-349`: o cache e a deduplicação evitam algumas chamadas; quando a leitura é necessária, `L365-398` chama `commandGroupsIpc.list`, recebe o catálogo e atualiza os caches local/global.
3. `src/ipc/commandGroups.ts:L28-31` e `src/types/bindings.ts:L381-382`: o wrapper chama o comando Tauri `command_group_list` com `projectId`.
4. `src-tauri/src/commands/command_groups.rs:L19-27`: o handler encaminha para `state.service.list`.
5. `src-tauri/crates/mt-commands/src/groups/service.rs:L105-108`: o service delega diretamente a `repo.list`.
6. `src-tauri/crates/mt-commands/src/groups/repo.rs:L691-699` e `L242-260`: `list` executa `list_catalog_in_conn` por `run_read`; os erros de domínio são preservados.
7. `src-tauri/crates/mt-db/src/lib.rs:L477-484`: `read_blocking` empresta uma conexão do pool. Nela, o catálogo consulta `command_groups`, `command_group_labels` e `command_group_items` conforme os laços acima.
8. No retorno, `CommandGroupCatalog` preserva `project_id`, `local_groups` e `global_groups` (`src-tauri/crates/mt-commands/src/groups/model.rs:L481-491`); o wrapper IPC entrega o resultado à store, que disponibiliza as coleções à interface.

**Agravante comprovado:** `reorder` também chama `list_catalog_in_conn` dentro da transação de escrita (`src-tauri/crates/mt-commands/src/groups/repo.rs:L1099-1185`, especialmente `L1184`). Assim, o N+1 prolonga também o trabalho de reconstrução do catálogo nessa transação. A leitura síncrona é chamada diretamente pelo service assíncrono; este plano corrige a multiplicação de consultas sem alterar o modelo de execução do service.

### Reprodução realizada nesta etapa

Foi executado um script Python via entrada padrão, sem gravar arquivos, que:

1. Extraiu do `repo.rs` atual os dois SELECTs de IDs e os três SELECTs do helper.
2. Criou um SQLite em memória, uma tabela mínima `projects` e aplicou o SQL existente de `0009_15_command_groups.sql`.
3. Inseriu 50 grupos locais de um projeto e 50 globais, cada qual com uma label e um item.
4. Instalou `sqlite3.set_trace_callback` depois da preparação e reproduziu os dois laços usando as consultas extraídas.
5. Contou **302 SELECTs**, com asserção de igualdade.

Essa reprodução confirma a contagem do caminho SQL identificado; não executou o aplicativo, o IPC ou o repositório Rust. Não foram medidas latências, congelamentos de interface ou ganhos de tempo. A comprovação automatizada sobre o helper Rust real faz parte da implementação proposta abaixo.

## Arquivos afetados

| Arquivo | Alteração prevista na implementação |
| --- | --- |
| `src-tauri/crates/mt-commands/src/groups/repo.rs` | Carregamento em lote, reaproveitamento dos decodificadores de linhas e testes unitários privados do catálogo com contagem real de statements. |
| `src-tauri/crates/mt-commands/tests/groups_repo.rs` | Regressões de contrato por `SqliteCommandGroupRepo::list` e `reorder`, usando SQLite e migrations reais. |
| `src-tauri/crates/mt-commands/Cargo.toml` | Habilitar `rusqlite = { workspace = true, features = ["trace"] }` em `dev-dependencies` para a instrumentação dos testes. |

Nesta etapa, somente `docs/issues/11/plan.md` é preenchido. Os arquivos de UI, store, IPC, modelos e migrations citados acima são referências do fluxo; o contrato existente será mantido pela implementação.

## Execuções propostas

1. **Manter o escopo na leitura do catálogo.** Preservar as assinaturas de `CommandGroupRepo`, `CommandGroupCatalog` e os comandos Tauri. Manter `load_group_by_id` para leituras pontuais e os retornos de `create`/`update`.
2. **Extrair a decodificação reutilizável.** Separar a leitura/conversão dos atributos do grupo e dos itens da execução dos SELECTs. Usar os mesmos decodificadores na leitura pontual e em lote, preservando `parse_id`, `parse_opt_id`, validação de enums, `PersistenceFailed`, fallback atual de `env_json`, conversão de revisão e de `continuous`. O `NotFound` deve continuar próprio da busca pontual. Os decodificadores não podem executar SQL.
3. **Substituir os SELECTs de IDs por um SELECT de grupos completos.** Selecionar as mesmas 13 colunas do helper com alias `g` e filtro parametrizado:

   ```sql
   WHERE g.scope = 'global'
      OR (g.scope = 'local' AND g.project_id = ?1)
   ORDER BY g.sort_order ASC, g.id ASC
   ```

   Vincular o ID opcional como texto ou `NULL`. Com `NULL`, somente globais são elegíveis. Separar o resultado nos vetores local/global sem perder a ordem relativa de cada vetor. Inicializar `labels` e `items` vazios e manter um mapa de ID para posição/coleção, evitando uma busca linear nos grupos para cada filho.
4. **Carregar labels em uma única consulta.** Fazer JOIN de `command_group_labels AS l` com `command_groups AS g ON g.id = l.group_id`, aplicar exatamente o filtro de elegibilidade anterior e selecionar `l.group_id, l.label`. Ordenar por `l.group_id, l.label_key ASC`; anexar ao grupo pelo mapa de posições. Grupos sem labels continuam presentes, com coleção vazia.
5. **Carregar itens em uma única consulta.** Fazer JOIN de `command_group_items AS i` com `command_groups AS g ON g.id = i.group_id`, repetir o filtro e selecionar o ID proprietário mais todas as colunas atualmente lidas dos itens. Ordenar por `i.group_id, i.position ASC` e anexar ao grupo correto. Preservar IDs, comandos, títulos, diretórios, ambiente e execução contínua.
6. **Fixar o orçamento em três SELECTs por catálogo**, inclusive vazio e sem projeto. As consultas dos filhos devem ocorrer fora dos laços de montagem. Evitar listas dinâmicas `IN` com um parâmetro por grupo, leituras de filhos de todos os projetos e JOIN simultâneo de labels com itens, que multiplicaria linhas por produto cartesiano. O número de statements será constante; o processamento dos dados ainda cresce com o volume retornado.
7. **Manter uma visão consistente na montagem em lote.** Em `list`, dentro de `run_read`, abrir uma transação de leitura diferida com a API compatível com `&Connection` (`unchecked_transaction`), passar a conexão transacional ao helper e concluir a transação após a montagem. Propagar falhas e permitir rollback por descarte. O helper `list_catalog_in_conn` deve continuar sem abrir ou fechar transações: `reorder` já o chama dentro de sua transação de escrita e deve enxergar suas próprias alterações. Não criar transação aninhada nem alterar o modo WAL.
8. **Adicionar toda a instrumentação e as regressões do próximo bloco.** Restringir callbacks e contadores a testes, usar dados sintéticos e contar execução real de SQL. Não alterar o pool de produção apenas para permitir medição.
9. **Validar em lote.** Concluir o bloco autorizado de implementação e testes antes de executar testes, checagens de tipos ou build. Executar as verificações indicadas, reunir as falhas, corrigi-las em lote e repetir as verificações afetadas até passarem.

## Testes de regressão e validação em lote

### Orçamento de consultas no helper real

Adicionar módulo `#[cfg(test)]` em `repo.rs`, com acesso direto ao helper privado. Criar banco temporário por `mt_db::Db::open`, semear por APIs existentes e obter uma conexão via `read_blocking`. Instalar nessa conexão `Connection::trace_v2`, com `TraceEventCodes::SQLITE_TRACE_STMT`, imediatamente antes de chamar o helper real e desativar ao final, inclusive em caminhos de erro por um guard de limpeza.

O callback deve contar statements SELECT efetivamente iniciados, com contador por thread e execução síncrona dentro da closure; não usar contador global compartilhado entre testes paralelos. Não contar preparação de fixtures, `BEGIN`, `COMMIT`, PRAGMAs ou assertions posteriores. A feature `trace` deve ser apenas de desenvolvimento. A API `trace_v2(&self, ...)` existe em rusqlite 0.40.2 e permite instrumentar `&Connection`; não usar a API antiga `trace`, depreciada.

- Teste `catalog_query_budget_is_constant`: executar cenários com 0, 1, 10 e 100 grupos elegíveis e afirmar **exatamente 3 SELECTs** por montagem. O caso de 100 grupos deve conter 50 locais e 50 globais. Inserir também grupos de outro projeto, que não podem aparecer no catálogo.
- Teste `catalog_query_budget_without_project`: repetir para `None`, incluindo catálogo vazio; locais devem estar ausentes e a contagem continuar em 3.
- A implementação atual reprovaria o cenário de 100 grupos informados com 302 SELECTs, conforme a análise e a reprodução SQL. A asserção pós-correção deve exigir o orçamento otimizado, nunca cristalizar 302 como resultado esperado.
- Avaliar `EXPLAIN QUERY PLAN` das três consultas com os mesmos filtros em fixture representativa, para confirmar que a escolha do plano não carrega desnecessariamente filhos de projetos alheios. Registrar a saída para análise; não fixar textos do planner em assertions frágeis dependentes da versão SQLite.

### Contrato e integridade pelo repositório público

Estender `tests/groups_repo.rs`, aproveitando `setup_db_with_projects` e `make_input` (`L27-83`):

- Comparar cada grupo de `list(Some(project))` com o resultado completo de `get(id)`, incluindo campos opcionais, labels, itens, envs, revisões e timestamps; executar as leituras de comparação fora de qualquer janela de contagem.
- Cobrir somente locais, somente globais, ambos, catálogo vazio, `None`, projeto sem grupos e ID de projeto válido porém inexistente. Preservar o retorno dos globais sem acrescentar erro de projeto inexistente.
- Confirmar isolamento entre dois projetos, inclusive filhos com valores semelhantes; ausência de filhos de grupos excluídos ou não elegíveis.
- Cobrir empate de `sort_order` e garantir desempate por ID no backend; labels ordenadas por `label_key`, não pelo texto exibido; itens por `position`, independentemente da ordem de inserção.
- Cobrir grupos sem labels e, por fixture SQL, sem itens, preservando os vetores vazios. Cobrir várias labels e vários itens no mesmo grupo para detectar duplicação cartesiana.
- Exercitar decodificação de dados válidos e falhas já tratadas, como ID persistido inválido; preservar o comportamento de JSON válido mas incompatível com o mapa de ambiente. Não relaxar constraints do schema de produção para esses testes.
- Ampliar a regressão de `reorder` (`L163-253`): verificar ordem, revisão incrementada, conteúdo completo e globais; cobrir reordenação global com `None`. Confirmar que não há falha por transação aninhada, que o retorno vê as atualizações e que conflito de revisão preserva atomicidade.
- Testar a transação de leitura: durante sua vigência, efetuar uma escrita por outra conexão em WAL com sincronização explícita e verificar que o catálogo segue o snapshot estabelecido; após concluir, nova listagem deve refletir a escrita. Evitar sleeps e limiares de tempo como mecanismo de coordenação.
- Manter as regressões já existentes de CRUD, unicidade Unicode, exclusão em cascata, idempotência e histórico.

## Banco de dados

**Nenhuma migration.** A correção modifica a estratégia de leitura. O schema existente já oferece:

- `ix_groups_order(scope, project_id, sort_order, id)` em `src-tauri/crates/mt-db/migrations/0009_15_command_groups.sql:L21-23`.
- Chave primária de labels `(group_id, label_key)` em `L25-30`.
- Unicidade de itens `(group_id, position)` em `L34-44`.

Preservar WAL, constraints, chaves estrangeiras e a sanitização existente no caminho de escrita. Nenhum dado persistido precisa ser transformado.

## Riscos de quebra e Não-Regressão

| Risco concreto | Garantia necessária |
| --- | --- |
| Misturar grupos locais de projetos distintos ou incluir locais quando o projeto for `None` | Reutilizar o mesmo predicado parametrizado nas três consultas; testes com dois projetos e ausência de projeto. |
| Trocar a ordenação de grupos, labels ou itens | Assertions de empate por ID, `label_key` e `position`; preservar os vetores do DTO. |
| Duplicar ou eliminar filhos durante a montagem | Consultas separadas para labels e itens; casos sem filhos e com múltiplos filhos de cada tipo. |
| Divergência entre o decoder pontual e o decoder em lote | Reuso dos decodificadores e equivalência completa entre `list` e `get`, incluindo erros e fallbacks existentes. |
| Abrir transação aninhada em `reorder` ou ocultar seus writes | Transação apenas no ponto de entrada da leitura; teste público de reordenação com revisões e catálogo completo. |
| Misturar versões de dados entre os três SELECTs | Snapshot da transação de leitura e teste com duas conexões em WAL. |
| A instrumentação passar sem contar a consulta real ou sofrer interferência de outros testes | Callback na conexão efetivamente utilizada pelo helper, contador por thread, janela delimitada e cleanup garantido. |
| Reduzir statements mas aumentar trabalho por varredura indiscriminada | JOINs filtrados, mapa de associação por ID e inspeção de `EXPLAIN QUERY PLAN`; não afirmar ganho de latência sem medi-lo. |

O cache da store permanece consumidor do mesmo DTO. A ordenação adicional por nome existente em `src/stores/commandGroups.ts:L62-72` não é a causa do N+1; o teste do contrato de ordenação desta correção deve observar diretamente o resultado Rust.

## Validação

Executar a partir da worktree dedicada, após implementar todo o bloco e seus testes:

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p mt-commands --lib catalog_query_budget
cargo test --manifest-path src-tauri/Cargo.toml -p mt-commands --test groups_repo
cargo test --manifest-path src-tauri/Cargo.toml -p mt-commands
cargo check --manifest-path src-tauri/Cargo.toml -p mt-commands --all-targets
git diff --check
```

**Critério conclusivo:** o helper real retorna os 100 grupos elegíveis completos com exatamente 3 SELECTs, independentemente de G; `list(None)` mantém somente globais; a equivalência de conteúdo, o snapshot de leitura e a reordenação transacional passam nas regressões. Os comandos são verificações propostas para a implementação, não resultados alegadamente obtidos nesta etapa de planejamento.

## Skills Aplicáveis

- `database-optimizer`: orientar análise das consultas, índices e planos de execução ao otimizar a persistência SQLite.
- `code-review`: revisar o diff antes da integração, com foco em contratos, precisão da instrumentação e reuso de `list_catalog_in_conn` por `reorder`.
- `security-and-hardening`: aplicar ao revisar o SQL parametrizado, isolamento de dados por projeto e ausência de logs de parâmetros sensíveis na instrumentação.

Usar as skills disponíveis e efetivamente pertinentes ao bloco executado. Não presumir a existência de skills citadas apenas como exemplo no pedido. A execução deve respeitar a validação em lote solicitada, alterações exclusivas na worktree e a confirmação exigida pelo projeto antes de commit/merge.
