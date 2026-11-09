# Plano de implementação — Issue #9

### Descrição e Causa Raiz

**Veracidade: bug confirmado no código atual.** Base analisada: `feb8f520ff5c7a8f9cace41e393a088c24c5fe9c`, na worktree `wt-implement-issue-9`. A alteração do nome de um grupo agenda uma reconstrução das sete fontes de `search_metadata`, sem transportar o identificador da entidade alterada. O trabalho cresce com o acervo inteiro e ocupa o escritor SQLite compartilhado até o commit. Risco de desempenho alto pela amplificação e serialização comprovadas; não foram medidas latências do aplicativo nem reproduzida indisponibilidade em produção.

**Causa Raiz Comprovada — fluxo e rastreamento reverso:**

1. **UI → composable → store → IPC:** o campo de nome está em `src/components/commandGroups/CommandGroupModal.vue:L33-36`; salvar chama `editor.save()` em `L189-194`. `src/composables/commandGroups/useCommandGroupEditor.ts:L651-665` envia ID, revisão e payload para `store.update`; `src/stores/commandGroups.ts:L446-453` encaminha a operação e atualiza seu cache. `src/ipc/commandGroups.ts:L38-46` chama o binding `command_group_update` (`src/types/bindings.ts:L387-388`). Neste fluxo local não existem rotas HTTP ou controller web: o endpoint é o comando Tauri.
2. **IPC → service → repositório → DB:** `src-tauri/src/commands/command_groups.rs:L58-73` chama o service e, depois do sucesso, dispara a sincronização genérica. `src-tauri/crates/mt-commands/src/groups/service.rs:L136-157` delega ao repositório e emite o evento de domínio. `src-tauri/crates/mt-commands/src/groups/repo.rs:L851-903` valida a revisão dentro da escrita; `L957-994` atualiza o grupo e suas labels por ID. A mutação de domínio é localizada; a amplificação é introduzida depois dela.
3. **Perda de escopo:** `src-tauri/src/search/adapters.rs:L59-69` recebe somente banco, aplicação e motivo textual. Não recebe `group_id`, `run_id`, `repo_key` ou número de issue. Em `L65`, chama `refresh_all_projections` e descarta o erro. Em `L38-43`, o backfill inteiro é executado em uma chamada `Db::write_sized`.
4. **Varredura global:** `src-tauri/crates/mt-db/src/search/projection.rs:L411-499` enumera projetos (`L418`), templates (`L442`), sessões (`L466`), grupos, execuções e issues. `L502-616`, `L619-744` e `L747-975` enumeram todas as entidades e consultam itens, labels e comentários. Há trabalho por entidade e consultas subordinadas, mesmo quando a mutação original alterou apenas um nome.
5. **Amplificação FTS:** `src-tauri/crates/mt-db/src/search/projection.rs:L140-175` usa `ON CONFLICT ... DO UPDATE` sem predicado de diferença, embora persista `source_revision`. `src-tauri/crates/mt-db/migrations/0011_17_search.sql:L55-60` remove e reinsere os termos FTS em qualquer UPDATE, inclusive idêntico ou somente de metadados.
6. **Contenção do escritor:** `src-tauri/crates/mt-db/src/lib.rs:L425-437` encaminha o trabalho ao mesmo writer. `src-tauri/crates/mt-db/src/writer.rs:L232-246` processa um job por vez, e `L527-539` envolve toda a closure em `BEGIN IMMEDIATE`/commit. O spawn assíncrono libera o chamador, mas não divide nem paraleliza a transação; escritas enfileiradas depois aguardam seu término. WAL não elimina essa serialização.
7. **Caminho equivalente de issues:** `src/components/github/IssuesTab.vue:L175-176` → `src/stores/githubIssues.ts:L364-375` → `src/ipc/github.ts:L89-90` → `src-tauri/src/commands/github.rs:L200-214`. O service altera labels em `src-tauri/crates/mt-github/src/lib.rs:L454-510`; `src-tauri/crates/mt-github/src/cache.rs:L511-542` grava somente a issue identificada. Depois disso, o IPC agenda o mesmo backfill global. Refresh e criação também fazem isso em `src-tauri/src/commands/github.rs:L119-135` e `L180-194`.
8. **DB → busca → frontend:** as linhas em `search_metadata` alimentam o FTS pelos triggers; depois do backfill o adaptador invalida snapshots e emite `search://invalidated` em `src-tauri/src/search/adapters.rs:L46-56,65-68`. Esse aviso hoje pode ocorrer mesmo se a transação de projeção falhar.

**Diagnóstico executado nesta etapa:** script Python somente em memória, usando o SQL real de `upsert_projection` e o trecho real da migration que cria `search_metadata`, FTS e triggers. Inseridas duas projeções sintéticas, uma de grupo e outra de sessão; reaplicados os mesmos valores, observando UPDATEs com trigger temporário. Resultado em SQLite `3.54.0`: `updates_sem_alteracao = [["groups", "grupo-a"], ["sessions", "sessao-b"]]`; incremento de `total_changes = 20`, incluindo escritas internas FTS e o observador. Isso confirma o trabalho de escrita sobre registros inalterados, sem atribuir os 20 incrementos exclusivamente ao FTS. O cenário completo de interface não foi executado; sua cadeia causal está comprovada pelas referências acima. A validação Rust futura deverá usar também a versão SQLite bundled do projeto.

**Limites precisos do achado:** o backfill percorre metadados de sessões e histórico de execuções de grupos; não reconstrói diretamente `command_blocks_fts` nem `session_scrollback_fts`. A ausência de sincronizadores pontuais de grupos/runs/issues, em contraste com os já existentes de projetos/templates/sessões (`projection.rs:L195-405,1010-1032`), demonstra que a correção ainda é necessária. Não basta comparar revisões: o título de um comentário depende do título da issue (`L941`), e sessões usam `started_at`/`ended_at` como revisão apesar de incluir diretório e título de aba (`L371-399`).

### Arquivos afetados

Arquivos previstos para a implementação futura:

| Arquivo | Alteração |
| --- | --- |
| `src-tauri/crates/mt-db/src/search/projection.rs` | Sincronizadores por chave, reconciliação restrita, consultas determinísticas, upsert condicional e reutilização pelos backfills administrativos. |
| `src-tauri/src/search/adapters.rs` | Alvos tipados e executor incremental aguardável/testável; agendamento em background, lotes por repositório e tratamento de falhas. Testes do adaptador no próprio módulo. |
| `src-tauri/src/commands/command_groups.rs` | Transportar IDs de criação, alteração, exclusão e início de execução para os adaptadores. |
| `src-tauri/src/commands/github.rs` | Transportar `repo_key` e número de issue, ou o repositório do refresh. |
| `src-tauri/crates/mt-db/migrations/0013_17_search_projection_noop.sql` — novo | Substituir somente o trigger de UPDATE da projeção por versão condicionada à mudança de texto/rowid. |
| `src-tauri/crates/mt-db/tests/search_projection_incremental.rs` — novo | Regressão de escopo, no-op, exclusão, dependências, FTS, falhas e escalabilidade estrutural. |
| `src-tauri/crates/mt-db/tests/search_index.rs` | Garantir que rebuild administrativo continua completo e sanitizado. |
| `src-tauri/crates/mt-db/tests/migrations.rs` | Upgrade do trigger e atualização das expectativas da versão 12 para a nova versão. |
| `src-tauri/crates/mt-db/tests/upgrade_from_fixtures.rs` | Validar fixtures antigas e nova versão final. |
| `src-tauri/crates/mt-db/benches/search.rs` | Cenários incrementais com acervo alheio crescente e medição de espera da próxima escrita. |

O número `0013` é o próximo disponível na base analisada; revalidá-lo ao implementar se houver migrations integradas posteriormente. O registro usa diretório embarcado (`src-tauri/crates/mt-db/src/migrations/registry.rs:L12`), portanto a inclusão do SQL deve seguir esse mecanismo. A entrega desta etapa é exclusivamente este plano.

### Execuções propostas

1. **Definir escopos internos explícitos.** Substituir a entrada genérica sem chave por um tipo interno semelhante a `ProjectionSyncTarget::{Group, GroupRun, GithubIssue, GithubRepo}`. Transportar IDs próprios do domínio e strings canônicas de repositório; não adicionar tipos de `mt-commands`/`mt-github` como dependências de `mt-db`, evitando ciclos. Validar a conversão de números `u64` para as representações SQL e `SearchTarget`, sem truncamento silencioso.

2. **Extrair a construção de projeções mantendo a semântica existente.** Implementar em `projection.rs` funções por `group_id`, `run_key` e `(repo_key, issue_number)`, com variantes `if_ready` quando necessário. Consultas devem conter predicados de chave desde o SELECT inicial, incluindo filhos. Reutilizar construção, sanitização e serialização de alvo no backfill administrativo. Ordenar labels de forma determinística e itens pela ordem canônica, evitando diferenças artificiais no corpo concatenado. Propagar erros de leitura de linhas, sem transformar erro SQL em ausência da entidade.
   - Grupo: ler somente seu registro, itens e labels. Se não existir, excluir apenas `(source = 'groups', entity_key = ID)`; preservar histórico de runs.
   - Run: ler somente sua execução e itens; preservar snapshot, primeira chave de item, saída sanitizada, truncamento, status e código de saída. Ausência remove somente sua projeção.
   - Issue: consultar a chave composta, suas labels e comentários e os vínculos de `github_project_repos` apenas desse repositório. Replicar para todos os projetos vinculados, não apenas o projeto da chamada; manter o fallback atual quando nenhum vínculo existir. Título da issue alterado exige atualizar os títulos dos seus comentários. Preservar plano, autor, estado, `fetched_at`, revisão e alvo de navegação.
   - Reconciliar comentários removidos e chaves antigas somente dentro da issue e dos vínculos correspondentes. Não usar limpeza global nem `LIKE` com caracteres de repositório sem escape; preferir chaves exatas e intervalos de prefixo seguros apoiados no índice `(source, entity_key)`. Projeções de outra issue/repositório devem ficar intactas.

3. **Fazer o upsert verdadeiramente idempotente.** Acrescentar `WHERE` ao ramo `DO UPDATE`, comparando com `excluded` todos os campos persistidos: IDs/contexto, `exit_code`, `source_truncated`, `fetched_at`, `title`, `body`, `target_json`, `occurred_at` e `source_revision`. Usar comparações nulas-seguras (`IS NOT`, combinadas com OR). Mesma revisão não autoriza ignorar conteúdo ou navegação diferentes; timestamp de sincronização deve continuar sendo atualizado quando mudou. Uma projeção totalmente igual deve resultar em zero UPDATEs e nenhum trigger FTS.

4. **Restringir o trigger ao texto efetivamente indexado.** Criar a migration aditiva descrita na seção Banco de dados. Uma mudança apenas em revisão, `fetched_at` ou contexto deve atualizar os metadados necessários sem reindexar texto idêntico. Preservar a manutenção FTS para alterações reais, inserções e exclusões. Não substituir o upsert por `INSERT OR REPLACE`, que mudaria rowid/semântica de exclusão.

5. **Implementar a orquestração incremental em `adapters.rs`.** Separar um executor assíncrono aguardável, que recebe `Db` e alvo e retorna `DbResult`, do wrapper Tauri que faz spawn e notifica. Para grupo, run ou issue, executar apenas o grafo da chave em uma transação curta, lendo o estado atual dentro dela; não indexar um DTO antigo capturado antes de outra mutação.
   - Para `GithubRepo`, enumerar issues somente de `repo_key`, por keyset de `issue_number`, em páginas limitadas (por exemplo, 100 IDs). Fazer a leitura de IDs no read pool, liberar a conexão e enviar trabalhos restritos ao writer; não manter transação única para todo o repositório. Os commits podem ser por issue, tornando cada unidade limitada ao grafo daquela issue. Não materializar todas as issues/comentários do acervo em memória.
   - Refresh deve alcançar todas as issues locais daquele repositório, incluindo fechadas por reconciliação e comentários associados. Não usar somente `page.items`: a resposta é paginada (`mt-github/src/model.rs:L429-447`) e `cache.rs:L349-368` pode fechar issues fora da página. Essa atualização por repositório é deliberada; mutações pontuais continuam por chave.
   - Jobs concorrentes devem reler a origem no writer, de modo que um job atrasado não sobrescreva uma projeção com estado antigo. Exclusão seguida de sincronização atrasada deve continuar excluída.
   - Preservar os motivos existentes do evento e invalidar snapshots após commit. Em refresh com vários commits, invalidar/notificar após cada lote publicado, para não manter snapshots anteriores se um lote subsequente falhar. Fazer a notificação antes de propagar a falha subsequente, quando houver commits prévios. Não anunciar sucesso do lote que sofreu rollback.
   - Substituir `let _ = ...` por registro estruturado de erro com motivo/chave/código, sem conteúdo de comandos ou segredos. Manter o resultado de domínio já confirmado; falha de indexação em background não deve incentivar repetir a mutação de domínio. Rebuild explícito permanece disponível para recuperação.

6. **Substituir os sete disparos que hoje causam backfill global.**

   | Comando | Alvo de sincronização |
   | --- | --- |
   | `command_group_create` | ID de `created`. |
   | `command_group_update` | ID de `updated`. |
   | `command_group_delete` | ID de `receipt`, com remoção pontual se a origem não existir. |
   | `command_group_run` | ID de `run`, preservando idempotência. |
   | `github_issue_create` | `created.repo_key` e `created.number`. |
   | `github_issue_labels_update` | `updated.repo_key` e `updated.number`. |
   | `github_refresh` | `page.repo_key`, percorrido em lotes restritos. |

   Os DTOs GitHub já carregam essas chaves (`mt-github/src/model.rs:L345-349,429-433`), dispensando nova consulta remota. Remover o helper genérico de refresh global de `adapters.rs` depois de substituir seus consumidores. Não mudar assinaturas IPC, bindings, fluxo de edição ou controle de revisão.

7. **Preservar os usos intencionais de reconstrução completa.** Migration inicial em `src-tauri/crates/mt-db/src/migrations/runner.rs:L300-307`, restore em `src-tauri/crates/mt-db/src/maintenance/backup.rs:L766-783` e rebuild solicitado em `src-tauri/crates/mt-db/src/search/mod.rs:L448-474` continuam chamando `backfill_all_projections`. A otimização não pode deixar um índice vazio após criação, restauração ou reconstrução. O recorte é eliminar amplificação nos sete disparos identificados; atualizações de domínio que já não agendavam sincronização não serão usadas como justificativa para manter o backfill incidental.

8. **Implementar todo o bloco e todos os testes autorizados antes de executar testes, checagens de tipos ou build.** Em seguida executar a validação em lote, reunir falhas, corrigir o conjunto e repetir somente as validações afetadas e as necessárias à conclusão.

### Testes de regressão e validação em lote

Criar fixtures com pelo menos dois projetos/repositórios, grupos locais e globais, itens, runs, sessões, templates, issues e comentários. Desabilitar manutenção periódica nas fixtures para evitar ruído. Estabelecer a projeção inicial com o backfill administrativo e zerar os contadores antes de cada ação.

1. **Regressão central:** renomear somente o grupo A e executar o mesmo alvo usado pelo adaptador. Trigger observador temporário em `search_metadata` deve registrar somente `groups/A`; nenhuma escrita nas projeções do projeto B, sessões, templates, runs ou issues. Confirmar novo termo em `MATCH` e ausência do termo antigo. Testar também criação e exclusão, incluindo exclusão sem apagar histórico da execução.
2. **Idempotência e FTS:** reaplicar a mesma projeção e exigir zero mudanças em `search_metadata` e delta zero de `total_changes` na conexão isolada. Mudar apenas `fetched_at`/revisão e verificar alteração do metadado, sem escritas nas tabelas internas FTS; medir o delta isolado e conferir o trigger instalado. Mudar título/corpo, mesmo com revisão igual, deve alterar `MATCH` corretamente. Exercitar NULL ↔ valor nos campos opcionais.
3. **Issues e dependências:** atualizar uma label e assegurar escopo na issue; renomear a issue e verificar seus títulos de comentários; remover comentário da origem e verificar sua remoção na busca. Testar dois projetos ligados ao mesmo repo, outro repo isolado, fallback sem vínculo e manutenção de chaves/targets. Um refresh com mais de 100 issues deve alcançar páginas posteriores e issues fechadas pela reconciliação do cache.
4. **Runs:** indexar início da execução e repetir idempotentemente, preservando snapshot, truncamento, código de saída e target. Reexecutar o sincronizador após alterar a origem e confirmar leitura da versão atual; não depender de um DTO capturado anteriormente.
5. **Ordem, rollback e eventos:** testes no módulo do adaptador devem aguardar o executor, sem sleeps. Cobrir duas atualizações enfileiradas, exclusão seguida de job antigo e falha SQL injetada antes do commit. Verificar que falha não emite sucesso, que o erro é observável e que lotes já confirmados invalidam snapshots mesmo se um posterior falhar. Conservar os motivos e o payload externo de `search://invalidated`.
6. **Ausência de varredura global:** além de observar escritas, examinar as consultas dos helpers e seus `EXPLAIN QUERY PLAN` com valores reais, confirmando busca por ID/chave composta e índices nos filhos. Não aceitar apenas igualdade do resultado final: ela não detectaria uma varredura global com upserts no-op. Os testes do executor devem incluir origem alheia ainda sem projeção e garantir que ela não seja materializada incidentalmente. A revisão deve verificar que nenhum dos sete handlers alcança `backfill_all_projections`.
7. **Escalabilidade:** repetir o caso pontual com acervo alheio pequeno e ampliado (por exemplo, 100 e 10.000 entidades), mantendo o grupo-alvo e seus filhos constantes. O número de projeções tocadas permanece um; registrar tempo do sync e espera de uma escrita sentinela enfileirada depois. Usar barreiras/confirmadores para determinar a ordem. Latência é evidência de benchmark, não limiar rígido e instável de CI. O critério obrigatório é escopo/volume de trabalho independente do acervo alheio; refresh cresce apenas com o repositório afetado.
8. **Compatibilidade e recuperação:** banco novo e upgrade de schema 12 devem instalar o novo trigger. Conferir `PRAGMA foreign_key_check` e FTS5 `integrity-check` com comparação ao conteúdo externo. Rodar regressões de rebuild, restore, fixtures, fontes e snapshots. Testar sanitização de APP_KEY/Bearer nas projeções extraídas; nenhum novo caminho pode contornar `SanitizedText`.

### Banco de dados

**Migration necessária:** `src-tauri/crates/mt-db/migrations/0013_17_search_projection_noop.sql`, aditiva e executada pelo runner transacional existente. Remover e recriar somente `search_metadata_fts_au` com o mesmo corpo delete/insert e condição:

```sql
WHEN old.title IS NOT new.title
  OR old.body IS NOT new.body
  OR old.rowid IS NOT new.rowid
```

A inclusão de rowid mantém a coerência caso uma atualização explícita altere a chave física; o upsert normal preserva essa chave. Inserção/exclusão continuam usando seus triggers existentes. Não editar a migration `0011`, seus checksums ou dados de origem; não é necessário backfill ou rebuild automático para essa mudança, pois apenas o critério de futuras atualizações é alterado. Aplicar em banco novo e existente e atualizar as expectativas fixas de schema nos dois arquivos de testes listados. Os índices existentes devem ser verificados com `EXPLAIN QUERY PLAN` antes de considerar qualquer índice adicional.

### Riscos de quebra e Não-Regressão

| Risco | Garantia e teste |
| --- | --- |
| Índice ficar desatualizado porque a revisão não mudou | Comparar todos os campos projetados; testar sessão com diretório alterado e comentário cujo título depende da issue. |
| Atualizar apenas um projeto quando o repo é compartilhado | Resolver vínculos por `repo_key`, preservar multiplicidade e testar dois projetos ligados ao mesmo repo. |
| Apagar projeções vizinhas por prefixo incorreto | Predicados parametrizados e chaves completas; testar números 9/90, repositórios parecidos e nomes contendo `_`. |
| Excluir histórico ao apagar grupo | Remover somente a projeção do grupo; validar que runs e seus snapshots permanecem pesquisáveis. |
| Refresh parcial por usar somente a página retornada | Percorrer o cache do repo por keyset; testar mais de uma página e issues fechadas fora da resposta. |
| Alteração tardia sobrescrever dados recentes | Ler origem no writer; testar ordem invertida de jobs e origem já removida. |
| Falha parcial manter snapshots antigos | Invalidar após lotes confirmados e registrar falha; testar sucesso parcial seguido de rollback. |
| Fragmentação introduzir inconsistência entre issue e comentários | Manter o grafo de uma issue na mesma transação; commits separados somente entre unidades/lotes. Uma issue com muitos comentários ainda pode custar mais, mas não deve carregar o restante do acervo. |
| SQL novo reduzir compatibilidade de bancos existentes | Migration aditiva, checksum original intacto, testes de upgrade/restore e FTS integrity-check. |
| Regressão de build/contratos entre crates | Nenhuma dependência reversa em `mt-db`; IDs e DTOs externos preservados; compilar todos os alvos afetados. |
| Otimização modificar ordenação ou sanitização | Comparar projeções incrementais com backfill para as mesmas entidades, verificar target, filtros, termos e segredos. |

### Validação

Executar a partir da raiz da worktree, somente após implementar o bloco completo e seus testes:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked -p mt-db --test search_projection_incremental
cargo test --manifest-path src-tauri/Cargo.toml --locked -p maxterminal --lib search::adapters::tests
cargo test --manifest-path src-tauri/Cargo.toml --locked -p mt-db -p mt-commands -p mt-github
cargo check --manifest-path src-tauri/Cargo.toml --locked -p maxterminal --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml --locked -p mt-db -p mt-commands -p mt-github -p maxterminal --all-targets
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo bench --manifest-path src-tauri/Cargo.toml --locked -p mt-db --bench search
git diff --check
```

**Prova de aceite:** os testes devem demonstrar simultaneamente que uma mutação pontual toca somente sua entidade e dependências, nenhuma projeção alheia é criada/atualizada/excluída, reaplicação idêntica gera zero escrita, atualização só de metadados não reindexa FTS, termos de busca refletem mudanças e rebuild/upgrade continuam íntegros. A matriz de sete handlers deve usar exclusivamente alvos restritos. O benchmark complementa a prova estrutural com medidas, sem apresentar latência não medida como fato. Nesta etapa foram executados apenas inspeção e diagnóstico SQL em memória; a correção e sua suíte ficam para a execução autorizada do plano.

### Skills Aplicáveis

- `database-optimizer`: escopo de consultas, amplificação de escrita, planos de execução e benchmark com escritor compartilhado; aplicada na análise deste plano.
- `security-and-hardening`: preservar sanitização de projeções e parametrização das consultas de chaves externas durante a implementação.
- `code-review`: revisar a correção antes de integração, especialmente grafo de dependências, SQL, migrations e ausência de backfill nos sete handlers.

Referências técnicas consultadas: documentação SQLite sobre `UPSERT ... DO UPDATE ... WHERE` (<https://www.sqlite.org/lang_upsert.html>) e manutenção FTS5 external-content por triggers (<https://www.sqlite.org/fts5.html>), via Context7. Nenhuma skill inexistente ou específica de outra stack é pressuposta pelo plano.
