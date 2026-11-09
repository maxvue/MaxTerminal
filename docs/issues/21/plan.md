# Plano de implementação — Issue #21

## Descrição e Causa Raiz

**Veracidade: confirmada por inspeção estática do fluxo executável**, na revisão `0a769a40c4bfe80abc2f4ffc68a44a49019d5a0b`. A correção ainda é necessária. Não foi executada reprodução interativa nem suíte de testes nesta etapa de planejamento; a evidência é a ordem das operações e a ausência de correlação no código atual.

Um item Multi-Abas deve representar exclusivamente o comando inicial de sua aba. Atualmente, pode permanecer `running` depois de esse comando terminar e, posteriormente, registrar o código de saída e o bloco de outro comando da mesma sessão. A consequência inclui histórico persistido incorreto e status agregado incorreto do grupo. Severidade avaliada: alta, pois há possibilidade de registrar sucesso para uma execução que falhou, sem recuperação automática confiável.

### Causa raiz comprovada

1. **Despacho anterior à observação:** `src-tauri/src/command_groups/adapters.rs:L572-627` abre a aba, chama `workspace_initial_command_run` nas linhas 611–619 e só então retorna. O retorno traz `initial_block_id: None` na linha 626. No Workspace, `src-tauri/src/workspace/manager.rs:L2641-2666` reclama o despacho no banco, escreve no PTY nas linhas 2654–2660 e ainda aguarda uma gravação antes de retornar; o shell pode terminar nesse intervalo.
2. **Assinatura tardia sem replay:** `src-tauri/crates/mt-commands/src/groups/multi_tab.rs:L185-230` espera `open_tab` antes de chamar `subscribe_session_events`. Em `src-tauri/src/command_groups/adapters.rs:L456-488`, a assinatura cria um receptor novo de `broadcast`, filtra por sessão e não recupera eventos anteriores. `Sender::subscribe` só recebe valores enviados depois da assinatura, conforme a documentação de Tokio (`tokio/src/sync/broadcast.rs`). Marcar `Running` depois de assinar não corrige o despacho que já ocorreu.
3. **Identidade mutável e conclusão sem filtro:** `src-tauri/crates/mt-commands/src/groups/multi_tab.rs:L309-350` sobrescreve `command_block_id` em qualquer `BlockStarted` recebido e aceita qualquer `BlockFinished`, ignorando identidade do bloco, PTY e geração. Assim, perder o término inicial permite concluir o item com o próximo comando.
4. **Persistência efetiva do resultado errado:** `src-tauri/crates/mt-commands/src/groups/repo.rs:L1543-1575` aceita os dados do patch; `L1601-1688` grava bloco/código/status em `command_group_run_items` e recalcula `command_group_runs`. A validação de transição de status não comprova a procedência do comando.

**Intercalação que comprova a falha:** `open_tab → write(false) → BlockStarted(A) → BlockFinished(A, 1) → retorno de open_tab → subscribe → Running → usuário executa true → BlockStarted(B) → BlockFinished(B, 0) → Succeeded, bloco B`. O receptor tardio não tem A; o monitor aceita B. Sem B, o canal segue aberto e o item pode permanecer `running`.

### Rastreamento de dados e agravantes

- UI `>>`: `src/terminal/triggers/providers/commandGroupsTrigger.ts:L368-396` → store `src/stores/commandGroups.ts:L572-578` → IPC `src/ipc/commandGroups.ts:L64-65` → comando Tauri `src-tauri/src/commands/command_groups.rs:L105-118` → serviço `src-tauri/crates/mt-commands/src/groups/service.rs:L513-562` → registro SQLite e `MultiTabRunner` → adaptador de abas → Workspace/PTY.
- Retorno: eventos `CommandStarted/CommandFinished` → `src-tauri/src/command_groups/adapters.rs:L350-381` → monitor → `update_run_item` → SQLite → `multi_tab.rs:L461-467` → eventos Tauri em `src-tauri/src/events/command_groups.rs:L59-64` → assinaturas da store em `src/stores/commandGroups.ts:L723-726` → atualização/reconsulta do snapshot em `L516-562`. Reconsultar o banco não reconstitui a conclusão inicial perdida.
- **Outro disparador do mesmo comando:** o Workspace publica a aba em `manager.rs:L1299-1301`; o frontend pode executar `src/stores/tabs.ts:L532-557`. A proteção deve existir antes dessa publicação e cobrir o ponto único de escrita. Apenas retirar o despacho de `open_tab` e movê-lo ao runner deixa uma corrida com esse handshake.
- A assinatura atual ignora `Lagged` e descarta `GenerationInvalidated` (`adapters.rs:L477-484`). O fallback `multi_tab.rs:L399-425` pode atribuir ao item a saída do shell interativo inteiro. Esses caminhos não podem produzir sucesso sem evidência do bloco inicial.
- O teste existente não detecta o problema: `src-tauri/crates/mt-commands/tests/groups_runner.rs:L124-146` fabrica eventos apenas ao assinar, com IDs independentes daqueles da aba (`L232-238`). O cenário `L659-739` valida falha parcial, sem intercalação anterior à assinatura.

## Arquivos afetados

1. `src-tauri/crates/mt-commands/src/groups/ports.rs`: contrato de observação do comando inicial e geração do runtime aberto.
2. `src-tauri/crates/mt-commands/src/groups/multi_tab.rs`: consumir observação pré-instalada, fixar bloco inicial e finalizar somente com resultado correlacionado.
3. `src-tauri/src/command_groups/initial_tracking.rs` — novo: estado compacto de acompanhamento inicial, registro/cancelamento e testes unitários determinísticos.
4. `src-tauri/src/command_groups/adapters.rs`: instalação da observação, encaminhamento de eventos e propagação de falhas.
5. `src-tauri/src/command_groups/mod.rs`: declarar módulo e compartilhar o rastreador entre adaptadores de shell e abas.
6. `src-tauri/src/workspace/manager.rs`: ponto de extensão interno para preparar observação antes da publicação e armá-la no despacho canônico; testes locais desse contrato.
7. `src-tauri/crates/mt-commands/tests/groups_runner.rs`: substituir os doubles incoerentes e acrescentar os cenários da issue.
8. `src-tauri/crates/mt-commands/tests/groups_service.rs`: adequar o double de abertura ao contrato revisado e preservar idempotência do serviço.

Os arquivos de UI, IPC e repositório acima são referências do rastreamento. A solução utiliza os DTOs públicos e as colunas existentes; o novo contrato é interno Rust. Se necessário declarar o ponto de extensão fora de `manager.rs`, usar um módulo interno de Workspace e registrá-lo em `src-tauri/src/workspace/mod.rs`, mantendo a mesma responsabilidade restrita.

## Execuções propostas

1. **Definir o contrato antes de alterar o fluxo.** A abertura deve entregar os IDs e a geração, juntamente com um handle exclusivo de observação já instalado, cujo estado sobrevive à conclusão anterior ao retorno de `open_tab`. Separar metadados clonáveis do receptor/guard de posse única. O handle retém somente o início e o resultado do comando inicial, não um histórico ilimitado da sessão.
2. **Instalar a observação antes de expor a aba.** Acrescentar uma variante interna da abertura no Workspace que aceite um preparador/observador de despacho. Depois de obter os IDs reais do painel e antes de publicar/disponibilizar a aba aos caminhos de despacho, instalar o acompanhamento. A abertura normal permanece usando o fluxo sem observador. Usar o lock de aba e o fluxo canônico já existente; não duplicar a escrita de comando nem a lógica de `claim_initial_dispatch`.
3. **Armar no ponto real de execução.** Em `workspace_initial_command_run`, imediatamente antes da escrita efetiva, notificar o observador com `(tab_id, pane_id, session_id, pty_id, generation)`. A mesma instrumentação deve valer quando o frontend ganha a corrida do handshake. Antes de armar, estabelecer que o shell está no prompt e não há bloco anterior em execução; eventos de inicialização do shell não são candidatos. Se não for possível obter essa condição ou houver disputa de submissão, retornar erro de acompanhamento em vez de associar um bloco arbitrário. A verificação, o armamento e a submissão precisam ser serializados: testar explicitamente uma entrada concorrente antes do despacho e não aceitar heurística por texto do comando.
4. **Capturar diretamente no sink.** O adaptador de shell deve alimentar o rastreador específico de Multi-Abas nos callbacks `on_command_started/on_command_finished`, antes de distribuir os eventos gerais. Usar uma pequena máquina de estados: preparado → armado → bloco inicial identificado → concluído/inválido. O primeiro início válido após o armamento fixa definitivamente o `block_id`; inícios anteriores ao armamento e eventos de outra sessão/PTY/geração são descartados. Somente o término daquele bloco encerra a observação. Retenção de resultado e entrega ao consumidor devem compartilhar sincronização, sem janela entre consultar estado e assinar.
5. **Eliminar dependência da assinatura tardia.** `open_tab` entrega a observação preparada, mesmo quando o resultado já foi capturado durante sua execução. `MultiTabRunner` deixa de chamar `subscribe_session_events` depois da abertura para acompanhar o comando inicial. Persistir os IDs e o estado de execução antes de consumir o resultado retido, preservando a ordem de eventos do grupo e impedindo que um `Running` posterior reverta a conclusão. Nunca substituir o bloco inicial por outro `BlockStarted`.
6. **Tratar perda de evidência explicitamente.** Propagar falhas de instalação e escrita hoje ocultadas por `.ok()`/`let _`. Ausência de OSC/rastreamento deve resultar em erro explícito ou `Interrupted`, conforme o comando tenha sido enviado; não aguardar indefinidamente por um evento que não pode ser produzido. Não usar código de saída do shell como resultado do comando. Encerramento do PTY, reinício de sessão e término do canal invalidam a observação ainda pendente; `wait_exit` deve participar concorrentemente como sinal de interrupção, sem sobrescrever um resultado inicial já retido. A captura direta evita que `broadcast::Lagged` apague a evidência inicial.
7. **Finalizar uma única vez.** Remover registro, receptor, tap e sink externo em sucesso, falha, cancelamento, erro de abertura e invalidação. Conservar a precedência de cancelamento definida pelo domínio e a preservação das demais abas em falha parcial. O guard deve liberar a observação também quando `open_tab` falhar antes de retornar. Não fechar uma aba já reutilizada por outro comando como efeito colateral do acompanhamento.
8. **Manter o escopo cirúrgico.** Utilizar um ponto de extensão neutro no Workspace; manter associação item/bloco no subsistema de grupos. Não adicionar replay global de eventos, polling do banco, atraso artificial ou correspondência por comando textual. Instalação de captura deve estar pronta antes da primeira escrita; anexar o tap antes do despacho quando usado pelo novo handle, preservando sanitização existente.

## Testes de regressão e validação em lote

Implementar todo o bloco autorizado e seus testes antes de executar testes, tipos ou build. Depois, executar a matriz em lote, corrigir conjuntamente as falhas encontradas e repetir as validações afetadas. Os cenários abaixo são testes a implementar, não resultados já obtidos.

- **Conclusão antes do retorno:** double coordenado por barreiras emite início A e fim A/1 durante `open_tab`, antes de devolver o handle. O item deve terminar `Failed`, código 1 e bloco A, sem fechar o shell e sem precisar de outro evento.
- **Resultado alheio:** em seguida, emitir início B e fim B/0 na mesma sessão. Estado, bloco, código, `finished_at` e quantidade de eventos terminais do item devem permanecer os de A. Testar também a ordem inversa (`true` inicial e `false` posterior).
- **Identidade imutável:** com A ainda ativo, emitir início/fim de B, eventos de outro PTY/sessão e geração antiga. Nenhum deles pode alterar a identidade ou concluir A; fim A deve finalizar normalmente. Um fim sem início correlacionável nunca deve ser adotado como comando inicial.
- **Handshake real:** testes no Workspace/adaptadores devem disparar o handshake do frontend antes de `open_tab` retornar, com término imediato no callback da escrita. Verificar observação já instalada, captura do resultado e exatamente uma escrita efetiva apesar das duas tentativas de despacho.
- **Inicialização e disputa:** produzir eventos de startup antes do armamento e uma submissão concorrente antes da escrita inicial. Verificar exclusão desses blocos ou falha explícita, jamais adoção silenciosa do primeiro evento disponível.
- **Falhas de acompanhamento:** instalação malsucedida, falha de escrita, shell sem OSC, invalidação de geração, PTY encerrado com código 0 sem fim de A e fechamento do consumidor. Não podem produzir sucesso nem deixar item ativo indefinidamente. Resultado de A já retido vence invalidação posterior do shell.
- **Rajada e limpeza:** inundar o barramento geral acima de sua capacidade 256 enquanto A termina; o acompanhamento direto deve preservar A. Verificar desregistro dos handles/taps após cada saída e que eventos de comandos posteriores não acumulam memória.
- **Integração do domínio:** conservar falha parcial na terceira aba, foco apenas na primeira, abertura ordenada, templates contínuos, múltiplos itens/sessões, cancelamento durante abertura e idempotência por `request_id`. Usar SQLite temporário real para conferir resultado do item e redução do grupo, além dos eventos emitidos.

Usar barreiras, canais e callbacks controlados, com timeout apenas como limite de falha. Não usar `sleep` para tentar provocar a corrida. Prefixar os novos testes nativos/adaptadores com `multi_tab_initial_` para permitir execução direcionada.

## Banco de dados

**Nenhuma migration.** `command_group_run_items` já possui `command_block_id`, `session_id`, `pty_id`, `exit_code`, `signal` e timestamps. Preservar WAL e sanitização de saída pelo repositório. O estado de observação é transitório; recuperação após reinício continua usando o mecanismo existente de runs interrompidos. Resultados históricos incorretos não podem ser reclassificados automaticamente sem evidência inequívoca do bloco original.

## Riscos de quebra e Não-Regressão

- **Contrato interno de abertura:** receptor não é clonável como os metadados atuais. Atualizar todos os implementadores/doubles; verificar compilação de todos os targets dos dois pacotes envolvidos.
- **Despacho comum do Workspace:** risco de execução duplicada ou perda do comando em abas normais. Testar handshake concorrente, geração obsoleta, ausência de comando inicial e claim idempotente; preservar a API pública.
- **Correlação no primeiro início:** startup e entrada concorrente podem confundir a identificação. A fronteira armada precisa ser comprovada pelos testes; se não houver exclusividade verificável, a implementação deve acrescentar reserva explícita de execução no produtor antes de prosseguir, em vez de reduzir o teste a uma hipótese de ordenação.
- **Finalização/cancelamento:** risco de duplo evento terminal, deadlock ou fechamento da aba reutilizada. Testar disputa entre término, cancelamento e `wait_exit`; não manter mutex bloqueante durante `await`.
- **Saída e performance:** não reter todo o fluxo de terminal nem bloquear a thread de leitura. Captura guarda somente metadados do bloco inicial e resultado; manter os limites e a sanitização dos sinks existentes.
- **Modos Sequencial/Paralelo:** continuam dependendo de outros contratos de eventos. Não alterar silenciosamente sua semântica; executar a suíte inteira de `mt-commands` após os testes direcionados.

## Validação

Executar na worktree de implementação, após concluir código e testes:

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p mt-commands --test groups_runner multi_tab
cargo test --manifest-path src-tauri/Cargo.toml -p maxterminal --lib multi_tab_initial_
cargo test --manifest-path src-tauri/Cargo.toml -p mt-commands
cargo check --manifest-path src-tauri/Cargo.toml -p maxterminal --all-targets
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
```

Critério conclusivo: os novos testes precisam ser efetivamente descobertos/executados (zero testes selecionados não vale). A conclusão A/1 emitida antes do retorno da abertura deve estar persistida como falha com bloco A; após B/0, os mesmos campos continuam intactos. O teste do adaptador/Workspace deve provar captura antes da escrita e idempotência diante do handshake, evitando um falso positivo produzido apenas por mocks. Registrar quantidade de testes e códigos de saída; dependências nativas indisponíveis constituem bloqueio de validação, não aprovação.

## Skills Aplicáveis

- `code-review`: revisão da causalidade, contratos internos, concorrência, limpeza e testes de regressão.
- `test-driven-development`: orientar a especificação dos testes determinísticos da regressão; nesta tarefa, respeitar a exigência de implementar o bloco completo antes da validação em lote.
- `security-and-hardening`: conferir que o novo acompanhamento mantém a sanitização e não introduz persistência adicional de comandos/segredos.
- `superpowers`: organizar a execução multi-arquivo e os critérios de conclusão, quando iniciada a implementação.

As skills citadas estão disponíveis no catálogo desta sessão. Skills de Vue não são necessárias para a solução proposta, centrada no backend Rust. Este documento contém somente planejamento; nenhum código-fonte foi alterado.
