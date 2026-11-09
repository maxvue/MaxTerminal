# Issue #8 — Recuperação do ACK de OpenTerminalForIssue em reentregas

### Descrição e Causa Raiz

**Veracidade: bug confirmado por inspeção estática do fluxo executável**, no commit `feb8f520ff5c7a8f9cace41e393a088c24c5fe9c`. A confirmação se apoia nos ramos explícitos abaixo; não foi executada reprodução de rede nesta etapa. O arquivo de plano encontrado na worktree estava vazio. Somente este documento foi preenchido; implementação e execução de testes ficam para a etapa autorizada de execução.

O primeiro `terminal.open_for_issue` cria/foca a aba e produz um `terminal.opened_ack`, mas o servidor conserva apenas a identificação de que a requisição foi vista. Uma reentrega com o mesmo UUID, enquanto ele permanece no cache e dentro do TTL, é descartada sem o resultado. Se o primeiro ACK não chegar ao remetente, a confirmação original não pode ser recuperada. A falha também afeta ACKs negativos: uma recusa original pode ser seguida por HTTP 200 com `accepted: true` genérico, que não representa o resultado da abertura.

**Causa raiz comprovada:**

1. `src-tauri/crates/mt-bridge/src/handler.rs:L273-279`: `check_and_insert(envelope.request_id)` antecede a abertura; ao identificar duplicata, retorna imediatamente `Ok(None)`. O ACK é construído apenas na primeira passagem e não é armazenado.
2. `src-tauri/crates/mt-bridge/src/dedup.rs:L10-19,L38-56`: `RecentRequestSet` guarda somente `Uuid → Instant`, limitado a 512 entradas e 60 segundos. Não há resultado nem estado de operação em andamento. A comparação atual inclui exatamente 60 segundos; acessos não renovam o instante, embora atualizem a ordem LRU. Uma expulsão por capacidade pode encerrar a deduplicação antes do TTL.
3. `src-tauri/crates/mt-bridge/src/server.rs:L381-390`: `Some(reply_env)` é serializado como resposta HTTP, enquanto `None` vira somente `{ "accepted": true }`.
4. `src-tauri/crates/mt-bridge/src/server.rs:L545-554`: o WebSocket escreve uma resposta apenas para `Some(reply)`; `None` não gera frame.
5. `src-tauri/crates/mt-bridge/src/handler.rs:L305-397`: sucesso e falhas são representados por `TerminalOpenedAckPayload`. O efeito de abertura ocorre nas linhas 360-371; foco, evento e resultado positivo nas linhas 373-389. Nenhum desses resultados é recuperado pelo deduplicador.

**Rastreamento reverso e causal, incluindo UI, serviço e banco:**

- O remetente MaxCode envia um `RpcEnvelope` com UUID; o protocolo real utiliza `requestId`, `projectId` e `tabId` em JSON, por `serde(rename_all = "camelCase")` (`src-tauri/crates/mt-bridge/src/protocol.rs:L215-249,L317-352`). O UUID do pedido fica no payload do ACK; o envelope de resposta possui UUID e timestamp próprios. A reentrega deve conservar ambos os níveis do ACK original.
- HTTP `/v1/rpc` e WebSocket `/v1/ws` convergem no mesmo `dispatch_inbound`. O `BridgeService` cria um único `Arc<InboundContext>` em `src-tauri/crates/mt-bridge/src/client.rs:L178-201`; por isso a recuperação precisa ser compartilhada entre conexões e transportes, não vinculada a um socket.
- `dispatch_inbound → handle_open_terminal_for_issue → BridgeHost`: normaliza diretórios, procura/cria o projeto e solicita a aba. A implementação concreta está em `src-tauri/src/bridge/host.rs:L196-241,L258-334`.
- `TauriBridgeHost → WorkspaceManager.tab_open → DB`: o host cria um novo UUID interno em `src-tauri/src/bridge/host.rs:L312-329`. O workspace consulta `workspace_operations` em `src-tauri/src/workspace/manager.rs:L868-901` e grava o resultado da operação em `L1281-1297`. Essa idempotência preexistente não recupera o ACK da ponte: usa outro UUID e a reentrega nem chega ao workspace. Remover apenas a deduplicação poderia criar outra aba.
- Retorno `TabView.id → OpenedTerminalInfo.tab_id → TerminalOpenedAckPayload.tab_id`: `src-tauri/src/bridge/host.rs:L331-334` e `src-tauri/crates/mt-bridge/src/handler.rs:L373-389` comprovam a origem do identificador perdido.
- Em paralelo ao ACK de rede, o sink emite `bridge://terminal-opened` (`src-tauri/src/events/bridge.rs:L77-85`); `src/ipc/bridge.ts:L111-117` o recebe e `src/stores/maxCodeBridge.ts:L146-151` atualiza `lastOpenedTerminal`. Esse estado da interface não funciona como armazenamento de respostas RPC. No workspace, a abertura também emite os eventos de sessão/aba em `src-tauri/src/workspace/manager.rs:L1299-1301`.
- A falha está na fronteira de despacho/resposta, não na persistência ou na apresentação da aba. Reproduzir o ACK não deve repetir abertura de PTY, criação de projeto, foco ou eventos de UI.

**Cenário determinístico para a execução:** enviar um envelope fixo, conservar o primeiro resultado apenas como referência do teste e descartá-lo do ponto de vista do remetente; reenviar o mesmo envelope imediatamente. Hoje o primeiro despacho retorna `Some(TerminalOpenedAck)` e o segundo retorna `None`. Em HTTP isso vira resposta genérica; em WebSocket, ausência de frame. O teste existente em `src-tauri/crates/mt-bridge/tests/server_it.rs:L206-228` cobre somente o primeiro envio, sem reentrega.

### Arquivos afetados

| Arquivo | Alteração prevista na implementação |
| --- | --- |
| `src-tauri/crates/mt-bridge/src/dedup.rs` | Adicionar cache limitado de envelopes de resposta e testes determinísticos de TTL/LRU; manter `RecentRequestSet` para os eventos sem ACK. |
| `src-tauri/crates/mt-bridge/src/handler.rs` | Acrescentar estado compartilhado ao contexto e substituir o descarte de `OpenTerminalForIssue` pela consulta/execução/armazenamento atômicos. |
| `src-tauri/crates/mt-bridge/src/client.rs` | Inicializar o novo estado uma vez por `BridgeService`. |
| `src-tauri/crates/mt-bridge/tests/server_it.rs` | Acrescentar regressões HTTP, WebSocket, concorrência, erros e contagem dos efeitos, ampliando os mocks existentes. |
| `docs/issues/8/plan.md` | Documento desta etapa e registro da validação posterior. |

Os módulos de transporte, protocolo, frontend, host e workspace citados acima são pontos de rastreamento e validação. A solução não exige alteração de seus contratos de rede ou banco. O módulo `dedup` já é público (`src-tauri/crates/mt-bridge/src/lib.rs:L14`), permitindo utilizar o novo tipo sem acrescentar uma reexportação na raiz.

### Execuções propostas

1. **Trabalhar na worktree dedicada da issue.** Conferir novamente o estado inicial e preservar alterações alheias. Implementar a correção completa e todos os testes abaixo antes de executar testes, checagem de tipos ou build; depois corrigir eventuais falhas em lote e repetir as validações afetadas.
2. **Criar `RecentResponseCache` em `dedup.rs`.** Guardar `Uuid → (Instant, RpcEnvelope)` em `LruCache`, com capacidade padrão 512 e TTL de 60 segundos, reutilizando as constantes existentes. Oferecer construção com capacidade/TTL e operações determinísticas com `Instant` explícito para testes. Remover/ignorar entradas vencidas, atualizar recência ao consultar e devolver clone do envelope original. Não armazenar o comando inicial, credenciais ou o payload do pedido.
3. **Fixar a semântica temporal.** Para respostas, contar o TTL a partir da conclusão/armazenamento do ACK, sem renovação em reentregas; considerar válido `elapsed <= ttl`, como a fronteira atual. Isso garante uma janela completa de recuperação após uma abertura lenta. Documentar a diferença em relação ao conjunto atual, cujo relógio começa antes da execução. A garantia permanece limitada à vida do serviço, à residência LRU e ao TTL, sem promessa de durabilidade após reinício.
4. **Acoplar exclusão assíncrona ao estado de respostas.** Adicionar a `InboundContext` um campo `terminal_responses: tokio::sync::Mutex<RecentResponseCache>`, inicializado em `BridgeService::new`. O próprio contexto já é compartilhado por `Arc`; não criar cache por conexão. Preservar `dedup: Arc<RecentRequestSet>` para `ProjectOpened` e os demais usos existentes.
5. **Trocar somente o ramo `OpenTerminalForIssue`.** Após a validação de versão já existente, adquirir o mutex assíncrono; consultar pelo UUID do pedido; se houver resposta válida, retornar `Ok(Some(cached.clone()))`. Em falta, executar `handle_open_terminal_for_issue`, construir uma única vez `RpcEnvelope::new(RpcPayload::TerminalOpenedAck(ack))`, armazenar o envelope antes de entregá-lo ao transporte e retornar o mesmo envelope. Não chamar `check_and_insert` neste ramo: manter um marcador independente criaria a possibilidade de pedido marcado sem resposta recuperável.
6. **Eliminar a corrida consulta/execução.** Manter o guard do mutex Tokio durante a consulta, abertura assíncrona e gravação. Uma reentrega concorrente aguarda a primeira operação e recebe o ACK concluído. Não manter `parking_lot::MutexGuard` através de `.await`. Nesta correção cirúrgica, serializar apenas as aberturas recebidas pela ponte, inclusive UUIDs distintos; pings, sincronização de projeto e aberturas locais não adquirem esse mutex. Não introduzir tarefas destacadas ou mapa de operações pendentes nesta entrega.
7. **Reproduzir resultados positivos e negativos integralmente.** Preservar `accepted`, `projectId`, `tabId`, `error`, UUID do pedido no payload, UUID externo e timestamp do envelope de resposta. Não recriar um ACK a partir de `tabId` nem reexecutar a operação quando o primeiro resultado foi uma recusa. Depois de expiração/expulsão, o pedido volta a ser elegível, assim como hoje.
8. **Definir escopo da chave.** O novo cache é exclusivo de `OpenTerminalForIssue`: um `ProjectOpened` com o mesmo UUID não deve suprimir seu ACK. Dentro desse tipo, o UUID identifica o resultado original durante a janela; reusar o UUID com conteúdo diferente não executa uma segunda operação e devolve o primeiro resultado. Um novo comando requer novo UUID. Cobrir ambos os comportamentos em testes.
9. **Conservar a adaptação de transporte.** Os ramos `Some` de HTTP/WS já serializam envelopes; ao corrigir o dispatcher, ambos passam a devolver o ACK original automaticamente. Os ramos `None` continuam necessários para mensagens sem resposta. Não trocar globalmente `accepted: true` nem alterar o protocolo v1.
10. **Preparar mocks observáveis.** Fazer o host de teste contar aberturas, criações de projeto e foco de janela; usar sink contador de `terminal-opened`. Permitir falha controlada e bloqueio assíncrono da abertura. Gerar IDs de aba distintos por chamada no mock para que uma abertura duplicada não fique escondida pelo `tab-issue-42` constante atual. Evitar esperas arbitrárias: coordenar concorrência com canais/barreiras e proteger leituras de rede com timeout.

Referência técnica consultada: documentação do Tokio para `tokio::sync::Mutex`, cuja guarda suporta `.await` e cujo bloqueio é assíncrono/FIFO: <https://github.com/tokio-rs/tokio/blob/master/tokio/src/sync/mutex.rs>.

### Testes de regressão e validação em lote

Implementar todo este bloco junto com a correção antes da primeira execução das verificações:

- **HTTP — ACK perdido:** enviar duas vezes o mesmo envelope por `send_http_rpc_envelope`; ambas as respostas devem ser 200 e conter `TerminalOpenedAck`. Comparar os envelopes completos por igualdade; conferir UUID de correlação, `projectId`, `tabId` e uma única abertura/foco/evento. No código anterior, a segunda resposta não contém envelope de ACK.
- **WebSocket — ACK perdido:** completar upgrade e `Hello`, enviar o pedido e ler/descartar a primeira confirmação; reenviar exatamente o envelope. Dentro de timeout, receber o mesmo ACK completo. A falta do segundo frame deve fazer o teste falhar, nunca passar silenciosamente.
- **Reconexão e troca de transporte:** concluir por WS, fechar a conexão, reenviar por HTTP e depois por uma nova conexão WS com handshake. Recuperar o mesmo resultado sem criar outra aba. Não registrar dois peers WS simultâneos neste teste.
- **Reentrega durante a execução:** bloquear o mock na abertura; iniciar dois despachos/requests HTTP do mesmo UUID em tarefas distintas; liberar o mock e verificar duas respostas iguais e uma única execução. As requisições HTTP usam conexões distintas. Um WS isolado processa frames sequencialmente e, sozinho, não prova concorrência.
- **Recusas preservadas:** caminho inexistente e falha controlada de `BridgeHost::open_terminal_for_issue`. Em ambas as reentregas, esperar o mesmo ACK com `accepted: false`, mesmos campos opcionais e erro; nenhuma segunda tentativa no host. O caso de erro do host deve registrar exatamente uma chamada.
- **TTL determinístico:** consultar em `t0`, `t0 + 60s` e após `t0 + 60s`; verificar inclusão da fronteira, expiração e que consultas intermediárias não renovam o prazo. Confirmar relógio de conclusão com operação simulada demorada e instantes injetados, sem esperar 60 segundos reais.
- **Capacidade/LRU:** cache pequeno, inserir A/B, consultar A, inserir C; comprovar expulsão de B, retenção de A e ausência de crescimento além do limite. Confirmar que falta/expiração autoriza novo processamento e novo resultado, sem prometer replay de item expulso.
- **UUIDs e isolamento:** UUIDs distintos produzem aberturas/resultados distintos; mesmo UUID com conteúdo diferente conserva o resultado original; `ProjectOpened` com o mesmo UUID não interfere no cache de respostas. O teste existente de `RecentRequestSet` deve continuar passando.
- **Efeitos e mensagens adjacentes:** repetição não recria projeto, não chama novamente o host, não refoca a janela e não reemite evento. Repetição de `ProjectOpened` continua sem resposta e sem novo efeito. `Hello`, `Ping`, validação de versão, autenticação, origem e mensagens de ACK preservam os contratos cobertos pela suíte.
- **Progresso independente:** enquanto uma abertura estiver bloqueada, enviar ping por outra conexão e exigir resposta antes de liberar a abertura; cancelar uma tarefa que apenas aguarda o mutex e comprovar que a operação dona do lock e uma reentrega posterior continuam funcionando.

Executar a suíte do crate em lote; reunir todas as falhas, corrigir o conjunto e repetir a suíte. Não alterar testes para aceitar o ACK genérico ou ausência de frame. Artefatos e comandos devem permanecer dentro da worktree.

### Banco de dados

**Nenhuma migration.** O cache é volátil e limitado, no serviço da ponte. As tabelas de projetos, abas, sessões e `workspace_operations` mantêm seu contrato. Não persistir envelopes nem comandos para corrigir este relato. A idempotência durável entre reinícios e a propagação do UUID externo até o workspace constituem outro escopo.

### Riscos de quebra e Não-Regressão

| Risco | Controle e validação |
| --- | --- |
| Duas aberturas para reentrega simultânea | Seção crítica assíncrona envolvendo consulta, execução e gravação; teste com host bloqueado e contador de chamadas. |
| Recriação do envelope mudar correlação ou timestamp | Armazenar `RpcEnvelope` completo e comparar igualdade em HTTP e WS. |
| ACK negativo virar sucesso aparente | Cachear também recusas e verificar todos os campos em testes de falha. |
| Cache ou mutex criado por conexão | Instância única no `InboundContext`; teste de reconexão e troca de transporte. |
| Regressão na sincronização de projetos | Manter `RecentRequestSet` e a máquina anti-eco; testar duplicatas de `ProjectOpened`. |
| Latência entre aberturas independentes da ponte | O mutex serializa este tipo de comando; registrar essa escolha. Testar que ping/serviço continuam responsivos em outra conexão. Uma abertura travada retém as seguintes: não mascarar isso com timeout que cancele efeitos já iniciados. |
| Cancelamento da operação dona do mutex após efeito e antes de guardar ACK | A guarda é liberada, mas não há garantia de exatamente uma execução nessa interrupção. O problema planejado é perda de ACK de operação concluída; queda do processo, aborto da tarefa ou idempotência durável exigem integração adicional com `workspace_operations`. Não afirmar cobertura desses cenários. |
| Mudança do ponto inicial do TTL | Explicitar prazo desde a conclusão, fronteira inclusiva e ausência de renovação; testes com relógio injetado. |
| Expulsão antecipada ou reinício do serviço | Limite de 512 resultados e retenção volátil documentados; após perda da entrada, nova execução é possível, como no deduplicador atual. |
| Crescimento de memória e exposição de dados | Cache limitado; guardar somente resultados, sem payload original, logging de comandos ou persistência adicional. |
| Mudança de construção de `InboundContext` | Atualizar o construtor em `client.rs` e fixtures. A busca nesta revisão encontrou somente a declaração e esse construtor; compilação de todos os targets do crate confirma consumidores de teste. |

### Validação

Na raiz da worktree, após implementar toda a correção e os testes:

```sh
cargo test --manifest-path src-tauri/Cargo.toml -p mt-bridge --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml -p mt-bridge --all-targets -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml -p mt-bridge -- --check
git diff --check
```

**Prova conclusiva:** a suíte de integração precisa obter o segundo `terminal.opened_ack` idêntico ao primeiro, por HTTP e WebSocket, com o mesmo `tabId` e uma única chamada ao host. Também deve obter o mesmo ACK negativo em reentrega de falha. A aprovação isolada de lint, compilação ou teste do LRU não prova a correção.

Critério de entrega: todos os cenários acima aprovados, evidências de execução registradas e revisão do diff final. Se dependências ou ambiente impedirem comandos, registrar o impedimento e não declarar a correção validada. Nesta etapa de planejamento, os comandos são prescritos, não executados.

### Skills Aplicáveis

- **`security-and-hardening`**: durante a implementação do fluxo RPC externo, preservar autenticação, tratamento de entradas e limite do cache.
- **`code-review`**: revisar o diff de implementação, contratos de resposta, efeitos duplicados e concorrência antes da integração.
- **`git-worktree`**: se for necessário criar ou gerenciar a worktree da execução; a sessão atual já está isolada em `.max-code-worktrees/wt-implement-issue-8`, diretório ignorado pelo Git.

As skills de Vue não são necessárias para esta correção Rust. A ordem de trabalho é a prescrita pela issue: implementação e testes completos primeiro, validação em lote depois. Não introduzir migrations, alteração de UI ou refatoração ampla da ponte neste plano.
