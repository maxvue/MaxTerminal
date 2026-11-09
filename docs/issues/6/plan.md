# Plano de implementação — Issue #6

## Descrição e Causa Raiz

**Veracidade: bug confirmado por inspeção estática do fluxo atual.** A consulta de idempotência e a criação remota não pertencem à mesma seção crítica. Duas invocações simultâneas com o mesmo `clientMutationId` não vazio podem criar duas issues e receber números diferentes. A chave primária local preserva apenas uma resposta, sem desfazer a segunda criação no GitHub.

**Causa raiz comprovada:** em `src-tauri/crates/mt-github/src/lib.rs:L323-333`, `create_issue` consulta uma resposta anterior sem reservar a chave. Em `L387-396`, chama o cliente remoto; somente em `L420-435` registra a resposta. `GithubService` não contém coordenador de mutações (`L67-77`). O mutex de sincronização existente protege `refresh_repo`, não essa criação (`src-tauri/crates/mt-github/src/sync.rs:L48-64`).

O banco também não fecha a janela: `src-tauri/crates/mt-github/src/cache.rs:L926-944` faz um SELECT independente e `L947-971` executa um INSERT posterior com `ON CONFLICT ... DO UPDATE`. A tabela possui chave primária global por `client_mutation_id`, sem estado de reserva (`src-tauri/crates/mt-db/migrations/0010_16_github.sql:L168-175`). Serializar cada operação SQLite isoladamente não serializa a sequência que inclui a rede.

### Rastreamento UI ⇄ Store ⇄ IPC ⇄ Serviço ⇄ GitHub/DB

1. `src/components/github/IssueCreateModal.vue:L78-94` emite `CreateIssueInput`. Hoje o modal envia `clientMutationId: null`; portanto, ele não demonstra sozinho o cenário com chave repetida.
2. `src/components/github/IssuesTab.vue:L210-217` encaminha a criação para a store; `src/stores/githubIssues.ts:L355-361` chama IPC e atualiza lista, seleção e contador.
3. `src/ipc/github.ts:L86-88` e `src/types/bindings.ts:L431-432` encaminham o input para `github_issue_create`.
4. `src-tauri/src/commands/github.rs:L177-194` aguarda `state.service.create_issue(input)` sem exclusão mútua. O estado compartilha uma instância por `Arc<GithubService>` (`src-tauri/src/github/mod.rs:L29-38,L68-78`).
5. O serviço consulta SQLite, chama `FallbackGithubClient::create_issue` (`src-tauri/crates/mt-github/src/client/fallback.rs:L188-209`) e atualiza o cache. O POST REST transmite título, corpo, labels e responsáveis, sem a chave local (`src-tauri/crates/mt-github/src/client/rest.rs:L305-322`).
6. A resposta volta pelo resumo lido do cache; o serviço registra a mutação e emite eventos (`src-tauri/crates/mt-github/src/lib.rs:L398-450`). A constraint local é aplicada depois do efeito externo.

**Interleaving que comprova a possibilidade:** A consulta K e não encontra; B consulta K e não encontra; A cria issue N; B cria issue N+1; A grava K→N; B sobrescreve K→N+1. Todos esses passos são permitidos pelo código atual. Não foi executada criação real nem reprodução dinâmica nesta etapa de planejamento.

**Agravantes e limites:** erros de serialização/gravação da resposta são atualmente ignorados (`lib.rs:L420-435`); erro local após sucesso remoto pode permitir nova tentativa duplicada. O fallback também pode tentar REST após erro do CLI. Esses caminhos exigem política própria de reconciliação e não são corrigidos apenas por um mutex. A correção proposta cobre a concorrência na instância compartilhada, com conclusão remota e persistência local bem-sucedidas; não promete exactly-once entre processos, reinícios ou resultados remotos ambíguos.

## Arquivos afetados

| Arquivo | Alteração prevista |
| --- | --- |
| `src-tauri/crates/mt-github/src/lib.rs` | Registro privado de locks por chave, integração em `create_issue` e testes unitários do ciclo de vida dos locks. |
| `src-tauri/crates/mt-github/tests/create_issue.rs` (novo) | Testes do serviço completo com SQLite temporário, transporte local controlado e eventos capturados. |
| `src-tauri/crates/mt-github/tests/cache.rs` | Testes de persistência e recuperação da resposta de mutação usados pelo replay. |

Nesta etapa, o único artefato gravado é `docs/issues/6/plan.md`, na worktree já dedicada `wt-implement-issue-6`.

## Execuções propostas

1. Adicionar ao `GithubService` um registro privado `parking_lot::Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>`, inicializado em `new`. As dependências já existem no crate. A chave será exatamente o identificador persistido, coerente com a PK global atual.
2. Implementar obtenção atômica do lock: sob o mutex curto do mapa, remover entradas cujo `Weak` não tenha referências fortes, tentar `upgrade` da chave e, se necessário, criar um `Arc` e armazenar seu `Weak`. Retornar a referência forte antes de liberar o mapa. Nenhum guard síncrono deve atravessar `.await`.
3. Cada chamada que aguarda deve manter sua referência forte. Nunca remover a entrada de um lock ainda referenciado por executor ou aguardantes: isso criaria dois locks para K. A limpeza oportunista de entradas mortas evita acumular todas as chaves históricas. Incluir testes privados para essa propriedade.
4. Em `create_issue`, manter validação de título/corpo e resolução de repositório. Logo antes da primeira consulta de mutação, identificar `Some(id)` cujo `trim()` não seja vazio e adquirir `lock_owned().await`. Usar `trim()` apenas para detectar vazio: não normalizar o valor usado no lock ou no banco, preservando o contrato atual.
5. Fazer a consulta existente ao cache somente após adquirir o guard. Se encontrar `IssueSummary` válido, retornar o replay sem chamada remota ou reemissão dos eventos do serviço. A chamada que estava esperando deve obrigatoriamente consultar de novo após obter o lock.
6. Manter o guard em escopo durante validação das labels, criação remota, atualização/leitura do cache, registro da resposta e emissão dos eventos. A liberação ocorre por RAII em sucesso, erro ou cancelamento. `None`, string vazia e apenas espaços continuam no caminho sem deduplicação.
7. Preservar assinaturas do serviço, DTOs, IPC, formatos de resposta e comportamento das labels. Manter paralelismo entre chaves diferentes. Evitar transação SQLite aberta durante rede e mutex global mantido pela operação inteira.
8. Construir a fixture de integração usando `Db::open` em diretório temporário, cadastro prévio de projeto/repositório, portas falsas e coletor de eventos. Usar `GhProcessRunner` falso para declarar CLI indisponível e `RestClient::with_base_url` com servidor loopback, conforme os mecanismos existentes em `tests/rest_client.rs:L33-109`. Não usar credenciais ou GitHub reais.
9. Implementar todo o bloco acima e os testes abaixo antes da primeira rodada de testes, tipos ou build. Em seguida validar em lote, corrigir conjuntamente as falhas encontradas e repetir as verificações afetadas.

## Testes de regressão e validação em lote

### Concorrência e replay no serviço

- `concurrent_same_mutation_id_creates_one_remote_issue`: disparar duas chamadas com K e input idêntico. Reter a primeira resposta HTTP enquanto a segunda chamada é exercitada. O servidor deve aceitar conexões concorrentemente e contar POSTs, em vez de serializá-los acidentalmente. Usar canais/semáforos para coordenar etapas e timeout limitado para detectar uma segunda requisição enquanto a primeira está retida; liberar a primeira resposta mesmo na falha para não deixar tarefas presas. Ao terminar ambas, exigir exatamente um POST, mesmo número/URL/resumo nas duas respostas, uma issue no cache e uma resposta persistida para K. Sem a seção crítica, o servidor pode receber dois POSTs e o teste deve falhar pela contagem.
- Complementar a prova temporal com teste unitário determinístico do registro: obter o primeiro guard, exercer o futuro de aquisição da mesma chave até `Pending`, verificar que o lock continua compartilhado, liberar o primeiro e exigir progresso do segundo. Evitar depender apenas de sleeps ou de uma rodada de stress.
- `repeated_mutation_id_replays_persisted_response`: após sucesso, repetir K; reconstruir também o serviço sobre o mesmo banco e repetir. Exigir zero POST adicional e resposta equivalente. Verificar um único par de eventos de criação no cenário concorrente.
- `different_mutation_ids_remain_concurrent`: reter a resposta de K1 e exigir chegada de K2 antes de liberar K1. Esperar duas issues e duas respostas de mutação, provando ausência de serialização global.
- `missing_or_blank_mutation_id_keeps_independent_creations`: cobrir `None`, `""` e espaços; duas chamadas válidas produzem duas criações, sem linha de idempotência. Testar ainda que `" K "` e `"K"` continuam chaves distintas.

### Liberação, memória e contratos existentes

- Falha de validação de labels com chave: nenhuma criação remota; nova entrada válida com a mesma chave consegue prosseguir.
- Falha remota inequívoca antes da criação: aguardante progride após o erro e não ocorre deadlock. Não usar falha ambígua pós-criação como evidência de retry seguro.
- Cancelar um aguardante e verificar que outro continua usando o mesmo lock; cancelar o detentor em teste unitário sem efeito remoto e verificar liberação. Testar remoção de entradas mortas e ausência de divisão do lock quando há aguardantes vivos.
- Em `tests/cache.rs`, verificar ausência inicial, gravação e replay exato do JSON após reabrir o banco. Preservar a semântica atual de conflito; não transformar o UPSERT em mecanismo de proteção remota.
- Executar os testes existentes de cache, labels, transportes e sincronização junto aos novos. O fixture deve encerrar servidor, tarefas e banco mesmo quando uma asserção falhar.

## Banco de dados

**Nenhuma migration.** Manter `github_mutations`, sua chave primária e o modo WAL existente. A exclusão mútua ocorre antes do SELECT e cobre a operação inteira no serviço compartilhado. Não alterar migrations já aplicadas nem introduzir reserva persistente nesta correção.

## Riscos de quebra e Não-Regressão

- **Bloqueio excessivo:** lock global por toda a operação impediria criações independentes; cobrir com o teste de K1/K2.
- **Limpeza incorreta:** descartar um lock ainda em uso permitiria a corrida original; cobrir aguardantes vivos e cancelamento.
- **Mudança de identidade:** normalização ou composição com `repo_key` divergiria da PK atual; manter a chave literal. Colisões de IDs entre repositórios/tipos já são uma limitação do contrato global e ficam registradas para evolução separada.
- **Falha após efeito remoto:** mutex não reconcilia POST confirmado com falha de cache, queda de processo ou resposta perdida. Manter esse limite explícito; não declarar retry seguro nesses cenários nem ampliar a correção para alterar o fallback.
- **Eventos e frontend:** replay retorna antes dos eventos, como atualmente. O incremento incondicional da store (`githubIssues.ts:L359-360`) não é prova de quantidade de issues no GitHub; validar a contagem real pelo transporte/cache. O modal atual usa chave nula.
- **Múltiplas instâncias:** o lock pertence ao `GithubService` compartilhado do Tauri. Duas instâncias independentes ou processos exigiriam coordenação persistente adicional.
- **Contrato e sanitização:** preservar a leitura do resumo via cache, suas regras existentes de sanitização, retorno do IPC e validação de labels. Testes existentes devem continuar aprovados.

## Validação

Executar a partir da raiz da worktree de implementação, após concluir o bloco de código e testes:

```sh
cargo test --manifest-path src-tauri/Cargo.toml -p mt-github --test create_issue
cargo test --manifest-path src-tauri/Cargo.toml -p mt-github
cargo check --manifest-path src-tauri/Cargo.toml -p mt-github --all-targets
git diff --check
```

Critério conclusivo do relato: duas chamadas concorrentes com a mesma chave retornam a mesma issue, o servidor observa **um único POST**, o banco conserva uma única criação/resposta e o serviço emite um único par de eventos. Chaves diferentes devem atingir o transporte simultaneamente. Esses testes são propostas para a execução; nenhum teste de correção ou build foi executado no planejamento.

## Skills Aplicáveis

- `code-review`: revisar a seção crítica, cancelamento, ciclo de vida das referências e cobertura antes da integração.
- `security-and-hardening`: preservar validação e sanitização na fronteira com o transporte externo; testes exclusivamente locais.
- `git-worktree`: executar e validar em worktree dedicada e integrar/limpar conforme autorização.

Referência técnica consultada: documentação Tokio sobre `Mutex::lock_owned`, guards mantidos entre awaits e liberação por drop, via Context7 (`/tokio-rs/tokio`). Não é necessário aplicar skills de Vue para esta alteração restrita ao backend.
