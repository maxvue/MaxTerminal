# Plano de implementação — Issue #22

### Descrição e Causa Raiz

**Veredito: implementação necessária; defeito confirmado por inspeção do código.** Base analisada: `0a769a40c4bfe80abc2f4ffc68a44a49019d5a0b`. As linhas abaixo referem-se a essa base. Esta etapa somente documenta o plano; não executou reprodução em desktop, testes ou alterações de código-fonte.

O cancelamento de uma execução Multi-Abas pode encerrar trabalho posterior realizado na mesma aba. Sessão e PTY identificam o shell duradouro, não o comando individual. Quando o monitor do grupo ainda considera o item ativo, esses identificadores continuam iguais após o comando inicial terminar e outro começar.

**Causa raiz comprovada:** `src-tauri/src/command_groups/adapters.rs:L630-651` implementa `close_tab_if_running_initial` verificando somente existência de painel, sessão e PTY. Em `L647`, `let _ = disp == InitialDispatchState::Sent;` descarta a comparação; em `L648-650`, chama incondicionalmente `tab_close` depois das verificações de identidade. Isso viola o contrato de `src-tauri/crates/mt-commands/src/groups/ports.rs:L260-267` e `src-tauri/crates/mt-commands/src/groups/multi_tab.rs:L470-471`.

**Uma simples condição `disp == Sent` não corrige o defeito.** `src-tauri/crates/mt-core/src/workspace/tab.rs:L111-126` define `Sent` como “escrito com sucesso no PTY”, não “comando em execução”. O despacho marca esse valor em `src-tauri/src/workspace/manager.rs:L2641-2678`; o estado não contém término nem identidade do bloco. Reutilizá-lo como indicador de atividade também comprometeria a idempotência do envio, verificada em `L2626-2630`.

**Agravantes e janela causal demonstrável:**

- `src-tauri/src/command_groups/adapters.rs:L611-626` envia o comando antes de devolver a aba e devolve `initial_block_id: None`.
- O runner somente assina eventos depois de `open_tab` retornar: `src-tauri/crates/mt-commands/src/groups/multi_tab.rs:L185-230`. Um comando curto pode terminar antes da assinatura. A assinatura é de um broadcast sem reprodução de eventos passados (`adapters.rs:L456-488`) e descarta `Lagged` (`L483`). A perda do término é, portanto, uma sequência possível no código, não uma reprodução empírica alegada.
- O monitor atual aceita eventos de bloco da sessão sem vinculá-los de maneira imutável ao comando inicial (`multi_tab.rs:L306-328`). Em `L453-458`, a entrada ativa só é removida ao finalizar o monitor. O cancelamento percorre essas entradas em `L491-509`.
- `adapters.rs:L636-640` escolhe o primeiro painel de uma lista derivada das chaves de `BTreeMap` (`manager.rs:L343-347`), embora exista um `primary_pane_id` explícito (`manager.rs:L91-95`). A posição da lista não constitui identidade confiável do painel inicial.
- `manager.rs:L1457-1509` faz operações assíncronas antes de retirar a aba e iniciar a terminação. Uma checagem externa seguida de `tab_close().await` mantém uma janela entre verificar e agir.
- `manager.rs:L1538-1548` encerra todos os PTYs da aba, inclusive painéis adicionados posteriormente. Isso exige preservar splits com trabalho independente.

**Rastreamento UI ⇄ Store ⇄ IPC ⇄ Service ⇄ execução ⇄ DB:**

1. UI: `src/components/workspace/TabWorkspaceView.vue:L284-292` encaminha cancelamento do grupo ou item.
2. Composable: `src/composables/commandGroups/useCommandGroupRun.ts:L174-189` chama a store.
3. Store: `src/stores/commandGroups.ts:L595-597` chama IPC e aplica o snapshot retornado.
4. IPC: `src/ipc/commandGroups.ts:L67-71` invoca `command_group_run_cancel`; `src-tauri/src/commands/command_groups.rs:L124-129` chama o serviço. Não há rota HTTP nesse fluxo.
5. Serviço: `src-tauri/crates/mt-commands/src/groups/service.rs:L570-590` seleciona `MultiTabRunner::cancel`.
6. Persistência do cancelamento: `src-tauri/crates/mt-commands/src/groups/repo.rs:L1422-1519` marca itens ativos como `cancelling` nas tabelas `command_group_run_items`/`command_group_runs`. Esse status não comprova que o comando inicial ainda está ativo no shell.
7. Efeito destrutivo: `multi_tab.rs:L502-509` → `adapters.rs:L630-651` → `manager.rs:L1488-1548`: remoção da aba e encerramento dos PTYs.
8. Persistência e retorno: `manager.rs:L1564-1598` fecha sessões, arquiva `tab_session`, fecha a aba no banco e emite eventos. `multi_tab.rs:L520` retorna o run, propagado até a store.

**Fonte de identidade de execução disponível:** `src-tauri/crates/mt-core/src/shell.rs:L483-503` oferece `pty_id`, `session_id`, `active_execution` e `last_completion`. O ShellService atualiza o registro de estado antes de persistir/publicar os eventos específicos do bloco (`src-tauri/crates/mt-shell/src/service.rs:L846-888` e `L891-914`). Esses dados permitem distinguir o bloco inicial de um bloco posterior; o status agregado da aba não permite.

**Cenário determinístico a automatizar:** abrir item Multi-Abas; registrar bloco inicial A; concluir A mantendo sessão/PTY; iniciar bloco B; manter artificialmente o monitor do grupo atrasado; solicitar cancelamento. Hoje o adaptador passa pelas verificações e fecha a aba. O resultado esperado é recusar o fechamento, preservar a aba e manter B vivo. Um segundo cenário conclui A antes da assinatura do monitor.

### Arquivos afetados

Implementação proposta:

- `src-tauri/src/workspace/manager.rs`: rastreamento efêmero do comando inicial por painel/geração, integração ao despacho e operação de fechamento condicional dentro do gerenciador.
- `src-tauri/src/command_groups/adapters.rs`: alimentar o rastreamento pelos callbacks síncronos já existentes; delegar o fechamento condicional ao workspace, eliminando a comparação descartada e a escolha por posição do painel.
- `src-tauri/crates/mt-commands/src/groups/ports.rs`: esclarecer as condições conservadoras de retorno `false`, mantendo a assinatura pública atual.
- `src-tauri/tests/group_initial_cancellation.rs` (novo): testes de integração do adaptador/workspace reais, despacho, identidade e preservação de PTYs.
- `src-tauri/crates/mt-commands/tests/groups_runner.rs`: testes de cancelamento do runner com monitor atrasado, cancelamento durante abertura e porta que recusa fechar.

Testes unitários do rastreador e da decisão atômica devem ficar em módulo `#[cfg(test)]` de `manager.rs`, para acessar o estado privado sem expor API de produção apenas para testes. UI, bindings IPC, modelo persistido de despacho e schemas SQLite não precisam de alteração.

### Execuções propostas

1. **Manter separados despacho e execução.** Introduzir estado interno, não serializado, por painel com identidade `(tab_id, pane_id, session_id, pty_id, generation)` e ciclo de vida do comando inicial: sem comprovação, aguardando identificação, bloco ativo identificado, concluído/inválido. Guardar `CommandBlockId` imutável após a identificação. Não mudar o significado de `InitialDispatchState::Sent` nem inventar persistência para esse novo estado.

2. **Armar o rastreamento antes da escrita inicial.** Em `workspace_initial_command_run`, dentro da exclusão por aba já existente, preparar a identidade antes de `pty_manager.write`. Usar a fronteira de execução do shell e o estado de prompt para impedir que um bloco anterior seja capturado. Só associar o próximo bloco quando houver evidência confiável de que corresponde ao despacho. Se o shell estiver ocupado, a integração não for confiável, a escrita falhar ou a correlação for ambígua, manter o fechamento automático desautorizado. Uma nova chamada idempotente não pode rearmar um comando já concluído. Comandos iguais em texto não são identidade suficiente.

3. **Atualizar o ciclo independentemente do monitor assíncrono do grupo.** Aproveitar `TauriGroupShellAdapter`, registrado como listener em `src-tauri/src/command_groups/mod.rs:L70-77`. Encaminhar snapshots/callbacks de início, término e perda de integração ao workspace antes de encaminhar o evento pelo broadcast. Identificar sempre painel, sessão, PTY e geração; usar revisões do snapshot para rejeitar observações antigas. Vincular uma única vez o bloco inicial; término correspondente é terminal para aquela geração e um bloco posterior jamais rearma o cancelamento. Não depender de o receiver do runner ter recebido `BlockFinished`. Consultar também o registro atual do ShellService no fechamento, pois a publicação do callback específico pode estar atrasada em relação ao registro. Limpar/inutilizar o rastreamento em fechamento, substituição do PTY, nova sessão, saída e restauração.

4. **Adicionar fechamento condicional no WorkspaceManager.** Localizar o painel pela identidade esperada e conferir o painel primário, em vez de usar `tab_pane_ids().first()`. Exigir simultaneamente: aba aberta, sessão/PTY/geração originais, bloco inicial conhecido e ainda ativo, integração confiável e `active_execution.block_id` atual igual ao bloco inicial. Comando concluído, prompt ocioso, bloco diferente, painel ausente ou estado desconhecido retornam `Ok(false)` sem sinal, arquivamento ou evento de aba fechada. Preservar conservadoramente abas com painéis adicionais, pois `tab_close` termina todos eles.

5. **Eliminar a janela de verificação externa.** Refatorar apenas o trecho necessário de `tab_close` para compartilhar sua infraestrutura com a nova operação condicional. Adquirir `tab_lock`, efetuar preparações assíncronas e revalidar imediatamente no ponto em que a aba passa a `Closing`/é retirada do mapa. A validação e a reserva do fechamento precisam ser indivisíveis em relação às atualizações do rastreador; uma leitura antiga seguida por `tab_close` público não serve. Definir ordem de locks consistente, sem chamar novamente um método que obtenha o mesmo `tab_lock`, sem manter `parking_lot` guard através de `.await` e sem invocar callbacks do shell com locks reentrantes retidos. Se a condição perder validade durante uma espera no banco, recusar a ação e concluir/descartar corretamente a preparação operacional. O caminho de fechamento explícito pelo usuário conserva sua semântica.

6. **Usar a guarda no adaptador.** Substituir o corpo inseguro de `close_tab_if_running_initial` pela delegação à operação condicional. Documentar no trait que `true` significa fechamento efetivo e `false` inclui ausência de prova de atividade. Os dois chamadores existentes — cancelamento normal (`multi_tab.rs:L502-509`) e durante abertura (`L193-202`) — passam a usar a mesma proteção sem alterar IPC.

7. **Implementar o bloco completo e os testes antes da validação.** Preparar em conjunto as mudanças de produção e todos os testes abaixo. Só depois executar testes, tipos/check e build aplicáveis; consolidar as falhas observadas, corrigi-las em lote e repetir as validações afetadas. Não encerrar a implementação com apenas mocks da própria guarda: o teste principal deve exercer o adaptador real e seu efeito sobre o PTY.

**Limite do escopo:** a correção garante que a autorização para fechar não depende da defasagem do monitor. A entrega tardia/perdida de eventos e a associação de blocos no monitor são agravantes comprovados, mas uma reformulação geral de seu protocolo não integra esta correção. Não usar a conclusão de um comando posterior para “provar” que o inicial estava ativo. A eventual permanência de um item em `cancelling` quando o canal perde eventos deve ser registrada como limitação preexistente, sem voltar a matar a aba para forçar a conclusão do grupo.

### Testes de regressão e validação em lote

Criar fixtures com banco temporário e recursos inteiramente contidos na worktree de implementação. Nos testes concorrentes, controlar a ordem por canais/barreiras; evitar depender de sleeps e velocidade do CI. Todo PTY/processo de teste deve ser encerrado pela própria fixture ao fim.

**Testes do rastreamento e da decisão:**

- Bloco A ativo, identidade original e integração confiável: permite fechar uma única vez.
- A concluído com sucesso, falha ou interrupção: recusa fechamento, mesmo com `initial_dispatch == Sent`.
- A concluído e B ativo no mesmo PTY/sessão: recusa; incluir B com texto idêntico ao de A.
- Estados `None`, `Pending`, `Waiting`, `Claimed` e `Sent` sem bloco correlacionado: nenhum autoriza sozinho o fechamento.
- Eventos atrasados/duplicados, outra sessão, outro PTY, geração substituída ou revisão antiga não reativam nem concluem o bloco errado.
- Término entre preparar a operação no banco e reservar o fechamento: revalidação recusa a ação.
- Cancelamento e término concorrentes: testar as duas ordens; se o término foi observado antes da reserva de fechamento, nenhum kill é autorizado.
- Aba removida, nova sessão, PTY encerrado, integração degradada e split posterior: preservação/retorno `false` conforme o contrato.
- Repetição do handshake não reenviará nem rearmará o comando inicial; o comando inicial muito curto pode terminar antes de o despacho terminar suas operações de banco.

**Testes de integração obrigatórios (`group_initial_cancellation`):**

1. `preserves_followup_command_in_same_session`: usar o adaptador/workspace real, concluir A e iniciar B duradouro; pedir fechamento condicional e afirmar `false`, aba presente, mesmo PTY vivo, resposta posterior do shell e ausência de arquivamento/fechamento no banco.
2. `closes_only_running_initial_command`: com A comprovadamente ativo, afirmar `true`, fechamento da aba e limpeza da árvore de processos; segunda tentativa não repete os efeitos.
3. `preserves_tab_when_initial_finishes_before_subscription`: concluir A antes de assinar o monitor e demonstrar que a proteção do workspace já conhece o término.
4. `rechecks_initial_after_async_close_preparation`: bloquear a preparação, concluir A/iniciar B e liberar a espera; nenhum processo de B pode ser encerrado.
5. `preserves_additional_split_panes`: adicionar painel com trabalho independente; cancelamento do grupo não encerra esse painel nem a aba inteira.

**Testes do runner (`groups_runner`):** simular retenção de `BlockFinished` enquanto o estado real já concluiu A; confirmar encaminhamento das identidades corretas, respeito ao retorno `false` e ausência de fallback de kill. Cobrir cancelamento global, item individual, cancelamento enquanto `open_tab` está em voo e timeout existente sem destruir a aba preservada. Manter o teste existente de falha parcial na abertura e os testes sequencial/paralelo.

**Execução em lote, a partir da raiz da worktree, após completar implementação e testes:**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p maxterminal --lib
cargo test --manifest-path src-tauri/Cargo.toml -p maxterminal --test group_initial_cancellation
cargo test --manifest-path src-tauri/Cargo.toml -p mt-commands --test groups_runner --test groups_service
cargo test --manifest-path src-tauri/Cargo.toml -p mt-shell -p mt-db
cargo check --manifest-path src-tauri/Cargo.toml -p maxterminal -p mt-commands --all-targets
git diff --check
```

Registrar comandos, quantidade de testes efetivamente executados e resultados; zero testes selecionados não constitui aprovação. Requisitos nativos de Tauri/GTK/WebKit devem estar disponíveis para o alvo de integração; indisponibilidade deve constar como bloqueio de validação, nunca como teste aprovado. Como os contratos frontend não mudam, não é necessário build de UI para provar esta correção.

### Banco de dados

**Nenhuma migration.** O rastreamento é efêmero, vinculado à geração do painel. Preservar os valores persistidos de `initial_dispatch`, WAL, sanitização e contratos das tabelas existentes. Os testes devem verificar que recusar fechamento não marca a aba como fechada, não encerra sessões e não arquiva a sessão ativa. Caso haja preparação em `workspace_operations`, ela não pode ficar pendente nem ser interpretada como fechamento realizado após a recusa.

### Riscos de quebra e Não-Regressão

| Risco | Proteção e validação |
| --- | --- |
| Confundir despacho com atividade e permitir novo envio | Manter `Sent` intacto; teste de handshake repetido após término. |
| Identificar como inicial um comando posterior | Registro antes da escrita, identidade de bloco imutável, fronteira confiável e recusa em ambiguidade; teste com comandos de texto idêntico. |
| Fechar depois de validar um snapshot antigo | Revalidação dentro da operação de fechamento e barreiras de concorrência em torno da preparação assíncrona. |
| Deadlock entre callbacks, mapa de abas, shell e `tab_lock` | Ordem explícita de locks, nenhuma espera assíncrona sob lock síncrono e testes concorrentes com timeout. |
| Encerrar split ou sessão substituída | Localização por identidade completa; política conservadora para múltiplos painéis; testes de split, restart e nova sessão. |
| Perda de integração impedir cancelamento automático | Recusar fechamento quando não há prova de atividade; fechamento manual continua funcional e é testado separadamente. |
| Vazamento de rastreamento entre gerações | Limpeza nos caminhos de saída/reinício/restauração e testes de reutilização do painel. |
| Alterar outros modos de execução ou fechamento manual | Preservar assinaturas IPC/trait; rodar `groups_runner`, `groups_service` e testes do caminho explícito de fechamento. |
| Monitor continuar atrasado após recusa | Verificar que timeout/retorno `false` nunca causa kill alternativo; documentar a limitação de estado do grupo sem ampliar o escopo. |
| Arquivamento incorreto ou processo órfão | Verificações de DB e da árvore de processos tanto no fechamento aceito quanto no recusado. |

### Validação

Prova principal: `cargo test --manifest-path src-tauri/Cargo.toml -p maxterminal --test group_initial_cancellation`. A suíte nova deve executar os cenários acima sobre o adaptador real. O cenário decisivo mantém sessão e PTY constantes, troca o bloco A por B e prova que B continua executando após cancelar, enquanto o controle positivo prova que A ainda ativo pode ser encerrado. Apenas verificar retorno booleano, comparar código textual ou passar uma suíte existente com porta fake que retorna `Ok(true)` não comprova a correção (`src-tauri/crates/mt-commands/tests/groups_runner.rs:L241-248`).

Critérios de aceite: todos os testes novos executados e aprovados; nenhum fechamento após término do comando inicial observado antes da reserva de fechamento; nenhuma regressão de despacho idempotente/fechamento explícito; nenhum arquivamento indevido; cancelamento do inicial efetivamente ativo continua funcional; validações em lote aprovadas e diff limitado aos arquivos planejados.

### Skills Aplicáveis

- `test-driven-development`: desenho dos testes de regressão comportamentais e da correção mínima; nesta execução futura, respeitar a ordem de implementação completa e validação em lote solicitada, documentando essa restrição ao ciclo usual da skill.
- `code-review`: revisar identidade do comando, fronteira de autorização do fechamento, locks, limpeza de processos e aderência ao contrato antes de integração.
- `git-worktree`: executar a implementação isolada, manter alterações e testes na worktree e integrar/limpar somente conforme autorização.

As skills de Vue não são necessárias para a correção backend delimitada neste plano. Não presumir skills citadas como exemplo que não estejam disponíveis no ambiente.
