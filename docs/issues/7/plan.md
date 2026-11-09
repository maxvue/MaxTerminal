# Plano de implementação — Issue #7

### Descrição e Causa Raiz

**Veracidade: bug confirmado por inspeção estática do fluxo executável.** A implementação atual ainda contém a conversão de erro remoto em sucesso local. O histórico consultado de `src-tauri/crates/mt-github/src/lib.rs` apresenta o commit `09d5171`; o código presente na branch `wt-implement-issue-7` não incorpora uma correção desse comportamento. Não foi executada reprodução dinâmica nesta etapa de planejamento.

Quando os transportes falham ao substituir labels, o serviço fabrica labels a partir do estado desejado, atualiza o cache SQLite, grava a resposta como mutação concluída e emite evento de sucesso. Com `clientMutationId` preenchido, repetir a mesma solicitação retorna essa resposta antes de consultar o GitHub. Sem chave, a falsa confirmação e a divergência local também ocorrem, embora não haja bloqueio do retry pelo registro idempotente.

**Causa Raiz Comprovada:** `src-tauri/crates/mt-github/src/lib.rs:L490-499`: o `unwrap_or_else` descarta qualquer `GithubError` de `replace_issue_labels` e constrói `Vec<RepoLabel>` com `next_names`. Esse resultado é indistinguível de uma confirmação remota para o restante da função:

1. `lib.rs:L501-510` grava as labels fabricadas no cache.
2. `lib.rs:L512-527` tenta registrar `client_mutation_id` como concluído, com `kind = "update_labels"`. Quando essa gravação é bem-sucedida, o retry fica comprometido.
3. `lib.rs:L529-535` emite atualização da issue e retorna `Ok(updated_summary)`.
4. Na repetição, `lib.rs:L457-465` recupera o JSON e retorna `Ok` imediatamente, sem nova escrita remota.

**Rastreamento reverso dos dados e fluxo de retorno:**

| Camada | Evidência e papel causal |
| --- | --- |
| UI | `src/components/github/IssueLabelEditor.vue:L73-115` monta `LabelUpdatePatch` e emite `update-labels`; `src/components/github/IssuesTab.vue:L175-177` encaminha ao store. O editor atual envia `clientMutationId: null`: a falsa confirmação afeta essa UI, mas a variante idempotente exige chamada IPC/serviço com chave preenchida. |
| Store | `src/stores/githubIssues.ts:L364-376` aguarda o IPC e só então atualiza a lista. O `finally` libera a trava mesmo em rejeição. O falso `Ok` do backend é tratado como confirmação legítima. |
| IPC / command | `src/ipc/github.ts:L89-91` chama `github_issue_labels_update`; `src-tauri/src/commands/github.rs:L197-214` propaga erro via `MtError` e só sincroniza projeções de busca após sucesso. Não há rota HTTP interna nesse caminho: trata-se de comando Tauri. |
| Serviço | `src-tauri/crates/mt-github/src/lib.rs:L454-499` resolve repositório, verifica idempotência, lê a issue e calcula labels; o erro remoto é descartado no ponto causal acima. |
| Transportes / API externa | `src-tauri/crates/mt-github/src/client/fallback.rs:L211-229` tenta `gh` e depois REST, propagando o erro final. `src-tauri/crates/mt-github/src/client/rest.rs:L338-352` envia `PUT /repos/{owner}/{repo}/issues/{number}/labels`; `rest.rs:L410-416,L446-479` transforma erros de rede e HTTP em `GithubError`. |
| DB / retorno | `src-tauri/crates/mt-github/src/cache.rs:L511-550` substitui `github_issue_labels` e atualiza timestamps de `github_issues`. `cache.rs:L927-970` lê/grava a resposta em `github_mutations`. `src-tauri/crates/mt-db/migrations/0010_16_github.sql:L168-178` define a chave primária e os campos desse registro. O resumo persistido retorna pelo serviço e IPC e também segue via evento ao frontend. |

**Cenário determinístico a automatizar:** preparar issue local/remota com `Waiting Plan`; solicitar `Planned` com chave `issue-7-labels-retry`; fazer `gh` e REST falharem antes de aplicar a escrita; recuperar o transporte e repetir exatamente o patch. Hoje a primeira chamada pode retornar sucesso local e a segunda nem chegar ao transporte. Após a correção, a primeira deve retornar erro sem efeitos locais e a segunda deve efetivamente atualizar o remoto.

### Arquivos afetados

Alterações previstas na implementação:

1. `src-tauri/crates/mt-github/src/lib.rs` — propagar o erro remoto e retirar o import de `to_repo_label`, que é usado nesse arquivo apenas pela fabricação de labels.
2. `src-tauri/crates/mt-github/tests/label_updates.rs` — novo teste de integração do `GithubService`, incluindo fixtures isoladas, transportes controlados, captura de eventos e verificação do SQLite.

Artefato desta etapa: `docs/issues/7/plan.md`. As referências de UI, IPC, cache, transportes e migration acima documentam contratos existentes; a correção proposta se concentra no serviço e na nova suíte. As dependências de teste necessárias já constam em `src-tauri/crates/mt-github/Cargo.toml`.

### Execuções propostas

1. Trabalhar na worktree dedicada e preservar quaisquer alterações preexistentes. Conferir o fluxo citado antes de aplicar o bloco de implementação, pois as linhas podem mudar em integrações posteriores.
2. Em `GithubService::update_issue_labels`, substituir o encadeamento `.await.unwrap_or_else(...)` por `.await?`, mantendo `remote_labels` como o resultado confirmado do cliente. Preservar `GithubError` com tipo, mensagem, detalhes e `retry_after_ms`, sem convertê-lo em sucesso ou em erro genérico.
3. Remover o import de `to_repo_label` de `lib.rs`. Manter a ordem causal: resolver/cache idempotente → ler estado atual → validar/calcular patch → escrita remota confirmada → atualizar cache → registrar resposta idempotente → emitir evento → retornar resumo.
4. Implementar a suíte inteira em `tests/label_updates.rs` usando a API pública do serviço e o `FallbackGithubClient` real. Aproveitar os pontos existentes de injeção, sem mudar construtores de produção:
   - `GhCliClient::new(Arc<dyn GhProcessRunner>)`, conforme `tests/gh_cli.rs:L20-60`, para simular autenticação, falha e sucesso por argumentos recebidos.
   - `RestClient::with_base_url`, com servidor HTTP local em porta efêmera, conforme `tests/rest_client.rs:L33-109`.
   - `Db::open(DbConfig::for_path(...))`, `GithubCacheRepo` e `TempDir`, conforme `tests/cache.rs:L22-85`. Criar diretórios temporários dentro da worktree, preparar vínculo projeto/repositório via `save_repo_resolution` e inserir a issue com as labels iniciais.
   - Implementações mínimas das portas exigidas por `GithubService::new` e um `GithubEventSink` que capture eventos. Portas sem uso nesse fluxo devem falhar explicitamente se forem acionadas.
5. Controlar o estado remoto simulado e contar apenas operações de substituição de labels, separadamente dos probes de autenticação. O servidor deve conferir método, caminho e corpo JSON completo, lendo `Content-Length`; respostas de falha não alteram esse estado. Usar tokens fictícios em memória, sem GitHub real, shell externo ou credenciais do ambiente.
6. Nos testes, comparar labels e timestamps persistidos antes/depois, consultar `get_mutation_response` e verificar o conteúdo dos eventos. A prova do retry deve observar a nova escrita e o estado remoto, não apenas o retorno do serviço.
7. Implementar todo o bloco autorizado de correção e testes antes de executar testes, tipos ou build. Em seguida validar em lote pelos comandos abaixo; agrupar correções de falhas e repetir somente as verificações afetadas. Esta etapa de planejamento não executa nem implementa esse bloco.

### Testes de regressão e validação em lote

A nova suíte deverá conter os seguintes casos, com nomes identificáveis:

| Teste proposto | Preparação e asserções obrigatórias |
| --- | --- |
| `failed_label_update_can_retry_same_mutation_id` | `gh` falha na escrita e REST retorna HTTP 503 antes de aplicar a alteração. Assertar `Err(NetworkUnavailable)`, labels e timestamps locais intactos, remoto intacto, ausência de resposta idempotente e ausência de evento de sucesso. Restaurar REST para 200, repetir o mesmo patch e a mesma chave; assertar segunda tentativa de escrita, remoto/cache com `Planned`, JSON idempotente correspondente e um evento. Repetir pela terceira vez: mesmo resumo e nenhuma nova escrita ou evento. |
| `failed_label_update_without_mutation_id_preserves_cache` | Executar a falha com `None`, chave vazia e chave contendo apenas espaços. Todos os casos devem retornar erro, manter cache/timestamps, não criar registro idempotente e não emitir atualização da issue. |
| `label_update_preserves_remote_error_metadata` | Cobrir queda de conexão antes da escrita e HTTP 401, 403 de permissão, 422 e 429 com `Retry-After`. Conferir o tipo de erro e metadados aplicáveis, além de ausência de efeitos locais. Cada caso usa clientes e banco próprios para não herdar cooldown. |
| `label_update_gh_failure_rest_success_is_committed` | `gh` falha e REST confirma a alteração com JSON válido. Conferir payload calculado, persistência, registro idempotente e um evento. A correção não deve interromper o fallback legítimo. |
| `label_update_gh_success_skips_rest` | `gh` confirma labels com JSON válido. Assertar resultado confirmado, cache e evento; o contador REST permanece zero. |
| `label_update_validation_failure_has_no_side_effects` | Usar um patch inválido para as regras canônicas. Assertar `ValidationFailed`, nenhuma escrita remota, nenhum registro idempotente, nenhum evento e cache intacto. |

Incluir no payload dos casos positivos uma categoria e uma label personalizada preservadas, além da troca de estado, para verificar a integração com `compute_next_issue_labels`. Os testes existentes de `labels.rs` continuam exercitando exclusividade de estado/encerramento e normalização.

Evitar sleeps e dependência de relógio real: usar fixtures com timestamps iniciais conhecidos, contadores e sincronização explícita. Limitar esperas de rede com timeout e finalizar tarefas do servidor e o banco de cada fixture. Uma repetição que não deva fazer I/O deve ser verificada por contador, sem deixar o servidor esperando indefinidamente uma conexão inexistente.

### Banco de dados

**Nenhuma migration.** O esquema já permite registrar somente respostas bem-sucedidas; a falha é a decisão do serviço sobre quando gravar. Preservar o modo WAL e o caminho transacional do repositório de cache.

**Dados legados:** a tabela atual não registra evidência de confirmação remota; portanto, não permite distinguir automaticamente sucessos reais de respostas fabricadas anteriormente. A correção impede novas entradas incorretas, mas não repara chaves já contaminadas. Para um caso legado comprovado, reconciliar a issue por atualização remota forçada e reaplicar a intenção com uma nova chave; não prometer que o retry da chave antiga será recuperado pelo patch. Uma limpeza global descartaria deduplicação válida e não integra esta correção cirúrgica.

### Riscos de quebra e Não-Regressão

- **Erro antes tratado como sucesso passa a rejeitar o IPC:** é a mudança esperada de contrato comportamental. As assinaturas já são `Result`/`Promise`; `github_issue_labels_update` propaga `MtError` e o store libera a trava no `finally`. `IssuesTab.vue:L175-177` não apresenta tratamento local explícito de erro; validar que a UI mantém as labels anteriores e permite nova tentativa, sem pressupor que exista aviso amigável.
- **Fallback legítimo:** falha de `gh` com sucesso REST deve continuar funcionando. Coberto por teste específico com ambos os transportes.
- **Idempotência bem-sucedida:** o terceiro envio da mesma chave deve retornar o sucesso registrado sem duplicar escrita/eventos. Coberto pelo teste principal.
- **Regressão de regras de labels:** preservar `compute_next_issue_labels` e a exclusividade canônica. Validar a suíte existente e o payload dos casos positivos.
- **Cache, eventos e projeções de busca:** após erro remoto, nenhum resumo fabricado deve ser persistido ou emitido. O retorno antecipado por erro no command também impede a sincronização de projeções subsequente; não mudar esse contrato.
- **Falha de banco após sucesso remoto / resposta perdida após escrita:** a correção não garante transação distribuída ou execução exatamente uma vez. Reaplicar o conjunto de labels pode ser necessário; não registrar confirmação quando o cliente remoto retorna erro. O cenário central simula falha antes da escrita para que a prova seja inequívoca.
- **Registros antigos, escopo global da chave e concorrência entre chamadas:** permanecem limitações distintas; os testes devem usar chaves únicas por cenário e não reivindicar correção desses contratos.
- **Resposta HTTP bem-sucedida com JSON inválido:** `rest.rs:L353-362` possui outro fallback para nomes desejados. É um caso diferente de `Err` remoto no serviço; este plano usa JSON válido nos sucessos e não atribui ao patch a correção de erros de parsing de transportes.
- **Isolamento:** fixtures locais, portas efêmeras e estado por teste evitam dependência de rede externa e interferência entre execuções paralelas.

### Validação

Executar a partir da raiz da worktree, depois de concluir o bloco de implementação e testes:

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p mt-github --test label_updates
cargo test --manifest-path src-tauri/Cargo.toml -p mt-github
cargo check --manifest-path src-tauri/Cargo.toml -p mt-github --all-targets
git diff --check
```

**Critério conclusivo:** o teste `failed_label_update_can_retry_same_mutation_id` deve provar a sequência erro sem efeitos → retry com a mesma chave efetivamente escrito no remoto → deduplicação somente após sucesso confirmado. A suíte completa deve terminar com código zero. Restaurar apenas o antigo `unwrap_or_else` em um experimento isolado deve fazer o teste falhar na primeira expectativa de erro, comprovando que detecta o defeito original; esse experimento é uma validação futura, não executada neste planejamento.

Na revisão da entrega, conferir que a alteração de produção se limita à propagação do erro e ao import removido, e que a suíte verifica serviço, transportes e banco reais do domínio. Registrar eventuais bloqueios de ambiente separadamente de falhas de asserção. Nenhum teste, build ou validação funcional da futura correção foi executado nesta etapa documental.

### Skills Aplicáveis

- `code-review` — revisar a alteração de controle de fluxo e os testes antes da integração, com atenção a falso sucesso, efeitos colaterais e cobertura da mesma chave.
- `security-and-hardening` — ao implementar fixtures de integração externa e persistência, preservar isolamento de credenciais, ausência de chamadas reais e contratos de armazenamento.
- `vue-debugging-best-practices` — aplicável à validação da propagação da rejeição e liberação do estado de mutação no frontend, se a verificação integrada exigir diagnóstico de reatividade.

As skills listadas estão disponíveis no ambiente. A implementação deve respeitar a validação em lote solicitada e as diretrizes canônicas do `AGENTS.md`.
