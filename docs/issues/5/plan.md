# Issue #5 — Impedir criação duplicada no fallback GitHub

### Descrição e Causa Raiz

**Veracidade: confirmada por evidência estática do fluxo executável.** A implementação atual pode efetuar dois POSTs de criação para uma única chamada: primeiro pelo `gh api`, depois pelo cliente REST, se o primeiro transporte retornar qualquer erro. A confirmação não depende de reproduzir a falha contra um repositório real. Nesta etapa não foram criadas issues remotas nem executados testes de implementação; a reprodução determinística fica especificada abaixo.

**Causa Raiz Comprovada:** `src-tauri/crates/mt-github/src/client/fallback.rs:L188-208`. Quando `should_try_gh()` retorna verdadeiro, o cliente executa `gh.create_issue`; somente `Ok` encerra a função. O braço `Err` em L203 apenas invalida a autenticação, quando aplicável, e segue para `rest.create_issue` em L206-208. Não há distinção entre falha anterior ao envio e resultado remoto desconhecido.

Evidências complementares e sequência causal:

1. `src-tauri/crates/mt-github/src/client/gh_cli.rs:L443-462` constrói e executa `gh api --method POST repos/{owner}/{repo}/issues`, com título, corpo, labels e responsáveis.
2. `src-tauri/crates/mt-github/src/client/gh_cli.rs:L139-144` converte timeout em `NetworkUnavailable`. Encerrar o subprocesso não desfaz uma criação já aceita pelo servidor.
3. `src-tauri/crates/mt-github/src/client/gh_cli.rs:L462-477` verifica o exit code e interpreta a resposta **depois** da execução. Mesmo com exit code zero, JSON truncado ou ausência dos campos obrigatórios produz `Internal`. Esse erro não comprova ausência de efeito remoto.
4. `src-tauri/crates/mt-github/src/client/rest.rs:L305-335` emite outro POST com o mesmo conteúdo. Os dois adaptadores não transmitem um identificador compartilhado de deduplicação.
5. `src-tauri/crates/mt-github/src/lib.rs:L325-333,L387-435` consulta uma resposta local anterior, mas só grava a resposta da mutação após o retorno remoto e a persistência da issue. Esse cache não protege as duas tentativas dentro da mesma chamada. Além disso, `src-tauri/crates/mt-github/src/client/mod.rs:L127-135` não recebe `client_mutation_id` no contrato remoto.

**Rastreamento UI ⇄ Store ⇄ IPC ⇄ Service ⇄ remoto/DB:**

- `src/components/github/IssueCreateModal.vue:L78-94`: constrói `CreateIssueInput`, atualmente com `clientMutationId: null`.
- `src/components/github/IssuesTab.vue:L210-217`: chama a store e controla o estado de envio do modal.
- `src/stores/githubIssues.ts:L355-361`: chama o IPC e atualiza lista, seleção e contador somente após sucesso.
- `src/ipc/github.ts:L86-87`: encaminha para `commands.githubIssueCreate` usando `unwrap`.
- `src-tauri/src/commands/github.rs:L177-194`: comando Tauri `github_issue_create` chama o serviço, converte erros para `MtError` e sincroniza projeções após sucesso. Não há rota HTTP/controller próprio entre UI e serviço; a fronteira é IPC Tauri.
- `src-tauri/crates/mt-github/src/lib.rs:L307-333,L387-450`: valida a entrada, consulta a mutação concluída, chama o fallback, persiste a issue, registra a resposta e emite eventos.
- `src-tauri/crates/mt-github/src/client/fallback.rs:L188-208`: ponto da repetição indevida; chama os adaptadores `gh_cli.rs` e `rest.rs` acima.
- `src-tauri/crates/mt-github/src/cache.rs:L926-970`: lê/grava `github_mutations` por `client_mutation_id` no SQLite. No retorno normal, o serviço usa `upsert_issues_batch` e `get_issue_detail` antes de publicar o resultado. Quando o primeiro POST é efetivado mas sua resposta se perde, o banco local ainda não registra esse sucesso e não bloqueia o segundo POST.

**Agravantes:** o segundo transporte pode retornar sucesso e ocultar a duplicação; títulos/corpos iguais não identificam unicamente uma intenção; classificar o erro só por `GithubErrorKind` também não prova que o POST não ocorreu. `gh_cli.rs:L520-542` infere classes inclusive por texto de stderr, e `Internal` cobre erros em fases diferentes.

**Pré-existência:** o código consultado ainda contém o ramo defeituoso; o histórico consultado do arquivo aponta o commit `09d5171`. A idempotência local já existente não corrige esse fluxo.

**Contrato externo consultado:** documentação oficial de criação de issues, via Context7 em 2026-11-09: <https://docs.github.com/en/rest/issues/issues#create-an-issue>. O endpoint é POST e retorna 201; os parâmetros apresentados não documentam uma chave de idempotência. A solução não deve presumir deduplicação remota por header inventado ou por igualdade de conteúdo.

### Arquivos afetados

Na execução da correção:

1. `src-tauri/crates/mt-github/src/client/fallback.rs` — política de transporte exclusiva para criação de issue e documentação dessa exceção ao fallback de leitura.
2. `src-tauri/crates/mt-github/tests/fallback.rs` — novo arquivo de testes de integração do cliente combinado, com runner falso e servidor HTTP local contabilizando chamadas.

Nesta etapa, apenas `docs/issues/5/plan.md` é escrito. Os demais arquivos citados são evidências ou referências de infraestrutura; não precisam ser modificados para a correção delimitada.

### Execuções propostas

1. Executar na worktree dedicada da issue, preservando alterações preexistentes. Manter o escopo na troca automática de transporte dentro de uma chamada de criação.
2. Adotar a invariável: **uma chamada de `FallbackGithubClient::create_issue` escolhe um único transporte de criação**. Se a sondagem inicial considerar o `gh` indisponível ou não autenticado, usar REST diretamente. Se escolher `gh`, retornar seu resultado sem executar criação REST após erro.
3. Em `fallback.rs`, preservar o retorno de sucesso e, no braço de erro do `gh`, chamar `invalidate_gh_on_auth_error(&err)` e retornar `Err(err)` imediatamente. Preservar `kind`, mensagem, detalhes sanitizados e `retry_after_ms`, evitando mascarar a causa com um erro secundário REST.
4. Manter o POST REST exclusivamente no caminho em que `should_try_gh()` foi falso. Não criar allowlist de erros que autorize uma segunda criação: os tipos atuais não expressam com segurança a fase do envio. A política conservadora inclui falha de spawn após sondagem positiva e rejeição de autenticação durante a tentativa; uma nova operação poderá selecionar REST após a invalidação de autenticação.
5. Documentar no método que timeout, erro de processo e falha de parsing podem ocorrer após o efeito remoto. Não aplicar essa mudança mecanicamente às consultas, criação de labels ou substituição de labels.
6. Implementar os testes de integração com `GhProcessRunner`, `GhCliClient::new`, `RestClient::with_base_url`, `TokenStore::in_memory` e `RateLimitTracker`, seguindo os exemplos de `tests/gh_cli.rs:L20-60` e `tests/rest_client.rs:L33-109`. Não é necessária nova dependência ou abstração de produção.
7. Para resultados ambíguos, retornar erro em vez de afirmar sucesso ou inexistência da issue. A correção elimina a segunda tentativa automática nesta chamada; não oferece garantia de exatamente uma criação entre novas submissões, concorrência ou reinícios. Reconciliação remota e deduplicação persistente entre solicitações são escopos adicionais, não pré-requisitos para eliminar este fallback.

### Testes de regressão e validação em lote

**Infraestrutura determinística:** runner falso deve responder separadamente a `--version`, `auth status` e `api --method POST`, registrar argumentos e contar apenas chamadas de criação. O servidor REST deve ler cabeçalhos e corpo completo, contar POSTs para o endpoint exato e responder com uma issue válida quando acionado. Usar token fictício válido para o armazenamento local, endereço `127.0.0.1:0` e encerramento explícito do servidor. Nunca depender de token real, binário `gh` instalado ou rede externa. Não usar `sleep` como prova de ausência de chamada: sincronizar a conclusão do cliente e o encerramento do servidor, recolhendo contadores e aguardando tarefas; colocar limite de tempo na infraestrutura para evitar travamentos.

Matriz obrigatória no novo alvo `fallback`:

| Cenário | Evidência/verificação esperada |
| --- | --- |
| `gh` registra criação remota simulada e retorna timeout (`NetworkUnavailable`) | Uma criação simulada via `gh`, zero POST REST, erro original retornado; na versão atual o REST seria acionado |
| `gh` registra criação e retorna exit code zero com JSON truncado | Uma criação simulada, zero POST REST, `Internal` com a mensagem de JSON inválido |
| `gh` registra criação e retorna JSON válido sem campos obrigatórios | Zero POST REST e erro de interpretação preservado |
| `gh` retorna falha de processo/exit code não zero depois da tentativa | Zero POST REST; erro propagado sem troca de transporte |
| `gh` retorna erro com `details` sanitizados e `retry_after_ms` | Campos preservados integralmente, sem segunda criação |
| `gh` autenticado retorna issue válida | Uma chamada de criação `gh`, zero POST REST, número e conteúdo corretos |
| Sondagem informa `gh` ausente | Zero chamadas de criação `gh`, exatamente um POST REST com título, corpo, labels e responsáveis corretos |
| Sondagem informa `gh` instalado mas não autenticado | Mesmo comportamento REST direto, preservando o caminho disponível sem autenticação CLI |
| Sondagem positiva seguida de `AuthInvalid` ou `AuthRequired` na criação | Nenhum POST REST nessa chamada; status em cache fica não autenticado; chamada posterior pode escolher REST |
| Sondagem positiva seguida de erro ambíguo não relacionado à autenticação | Cache de autenticação continua válido; nenhum POST REST |
| REST selecionado diretamente retorna erro | Erro REST propagado, sem tentativa de criação via `gh` |
| Consulta `fetch_issues` via `gh` falha | Consulta REST continua funcionando, sem emissão de POST de criação |

Incluir testes parametrizados para outras classes de `GithubErrorKind`, impedindo que uma futura allowlist restabeleça repetição automática. Os testes principais devem medir efeitos e requisições, não somente verificar se foi retornado `Err`.

**Reprodução controlada:** no runner, registrar a primeira criação antes de devolver a falha; no mock REST, registrar qualquer criação recebida. Antes da correção, esse cenário permite duas criações e a asserção `REST POSTs == 0` falha; após a correção, apenas a primeira permanece. Isso demonstra o defeito sem gerar issues reais.

**Ordem de execução:** implementar todo o bloco autorizado e todos os testes acima antes de rodar testes, tipos ou build. Em seguida executar a validação em lote, reunir as falhas, corrigi-las em conjunto e repetir apenas os comandos afetados e a suíte necessária. Não executar build frontend nem regenerar bindings para uma alteração interna de política sem mudança de contrato.

### Banco de dados

Nenhuma. A correção não altera schema, migrations, modo WAL ou sanitização. `github_mutations` continua representando respostas concluídas, sem ser apresentada como mecanismo de idempotência remoto ou registro de operações em andamento.

### Riscos de quebra e Não-Regressão

- **Disponibilidade versus duplicação:** após escolher `gh`, erros antes ou depois do envio passam a encerrar a operação, mesmo se houver token REST. Essa redução deliberada do fallback é necessária enquanto o erro não informa uma fase comprovadamente anterior ao envio. Cobrir ausência/desautenticação na sondagem para manter REST direto funcional.
- **Autenticação em cache:** retornar cedo sem invalidar `AuthRequired`/`AuthInvalid` causaria repetição de escolhas inválidas. Testar o estado após a primeira falha e a seleção REST na operação posterior.
- **Operações de leitura:** a política geral de consultas não deve ser alterada. Testar fallback GET e executar a suíte existente de REST, cache e sincronização.
- **Contratos públicos:** nenhuma alteração em `GithubRemoteClient`, `GithubErrorKind`, DTOs IPC, bindings ou schema. Verificar preservação do erro e sucesso com os mesmos dados.
- **Resultado remoto desconhecido:** uma issue pode existir mesmo quando a criação retorna erro. O serviço não deve fabricar sucesso, persistir issue inventada ou emitir eventos de criação nesse caminho. O `?` de `lib.rs:L396` já interrompe a persistência e os eventos.
- **Novas submissões:** o modal atualmente envia ID nulo e o cache registra apenas sucesso. Uma nova solicitação manual ou concorrente ainda pode criar duplicatas; o aceite desta issue é impedir o segundo POST automático do fallback, não deduplicar solicitações distintas.
- **Testes falsamente verdes:** usar REST funcional com token fictício, contador de requisições e resposta válida; um REST indisponível poderia ocultar a tentativa indevida. Registrar a mutação simulada antes de produzir timeout/parsing inválido.

### Validação

Executar da raiz da worktree, após implementar integralmente o bloco:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked -p mt-github --test fallback
cargo test --manifest-path src-tauri/Cargo.toml --locked -p mt-github
cargo clippy --manifest-path src-tauri/Cargo.toml --locked -p mt-github --all-targets -- -D warnings
git diff --check
```

**Prova conclusiva de aceite:** o alvo `fallback` passa com uma criação remota simulada pelo `gh` e zero POSTs REST para timeout, JSON inválido e resposta incompleta; REST direto permanece capaz de criar exatamente uma issue quando a sondagem não seleciona `gh`. A suíte completa de `mt-github` deve passar. Falhas de ambiente/dependências devem ser registradas como bloqueios, nunca como validação aprovada.

Na etapa atual foi realizada somente análise estática e documentação do plano. Os comandos e o novo alvo de teste acima são para a implementação futura.

### Skills Aplicáveis

- `code-review` — revisar a política não idempotente, a preservação de contratos e a cobertura antes de integrar a correção.
- `security-and-hardening` — aplicar à interação com GitHub e aos testes de transporte, mantendo sanitização, tokens fictícios e isolamento de credenciais.
- `git-worktree` — seguir o isolamento obrigatório e preservar trabalho de outras sessões; utilizar a worktree dedicada já disponível.

As skills Vue e Vitest não são necessárias para este bloco Rust. Aplicar a ordem de implementação e validação em lote exigida nesta solicitação. Commit, integração e remoção da worktree dependem da autorização e da etapa de execução correspondente.
