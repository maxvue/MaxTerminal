# Plano de execução — abas persistentes, templates SSH e gatilhos no viewport

## 1. Identificação e regras de execução

- Projeto: MaxTerminal. Data: 03/11/2026. Worktree e branch: `wt-292f24a6`.
- Diretório exclusivo: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-292f24a6`.
- Especificação normativa desta proposta: `docs/plans/wt-292f24a6/spec.md`. Ler na primeira admissão junto deste plano; nas retomadas, consultar somente trechos pertinentes e alterações factuais.
- Não criar outra worktree, trocar branch, editar bibliotecas compartilhadas fora desta worktree, commitar, fazer merge/push ou remover a worktree. Integração é responsabilidade do MaxCode quando solicitada pelo usuário.
- Este documento planeja implementação futura; nesta sessão só foram escritos spec e plano. Não executar automaticamente o plano por sua existência.
- Comunicação, comentários e textos de UI em pt-BR; identificadores/arquivos em inglês. SFC na ordem template/script setup TypeScript/style scoped SCSS, quatro espaços, tags de abertura em uma linha, aspas simples, ponto e vírgula, sem vírgula final; tokens semânticos Max.

## 2. Pedido, decisões e preservações

### Requisitos

R1. As abas abertas devem continuar existindo ao fechar/reabrir o MaxTerminal.

R2. No modal de template de um projeto, adicionar `MaxInputSelect` com label `Origem`, opções `Local` e `Remoto`. Remoto permite IP, usuário, senha e `Utilizar Chave SSH`; o template deve efetivamente abrir uma conexão remota, não só salvar campos sem uso.

R3. Digitar `>`, `>>`, `@` e demais gatilhos existentes no `hybrid-terminal-viewport` abre os respectivos seletores no contexto em que eles são válidos.

R4. Complemento confirmado pelo usuário: senhas SSH **criptografadas no SQLite**, protegidas por **senha mestra ao abrir**. Não trocar por senha somente em memória nem chave no cofre do Linux.

R5. Frontend deve usar MaxComponentsUi, MaxPinia e MaxUse; integrar ao transporte Tauri existente, preservando segurança, cache de metadados e revisões.

### Decisões propostas para revisão

- Persistir intenção de aba em `tabs.closed_at`, não estado vivo do processo; restaurar sessões novas pelos mesmos IDs de aba/painel e snapshots.
- Fechar app arquiva sessões sem fechar intenção; fechar aba remove intenção. Crash segue reconciliação existente. Não ressuscitar indiscriminadamente abas históricas já fechadas por versões antigas.
- Abas manuais restauradas não reexecutam automaticamente comando inicial; expor execução explícita se houver comando. Templates de autoinício mantêm sua política atual. Histórico nunca é reproduzido.
- Remoto usa OpenSSH local, porta 22, IP v4/v6 e usuário validados. Chave pode usar caminho opcional ou identidades/agente padrão; chave privada não vai ao banco. Sem outro gerenciador de conexões ou servidor HTTP novo.
- Cofre configurado pede senha mestra uma vez por processo do app; usuário pode adiar, usando abas locais enquanto remotas por senha aguardam. Sem recuperação de senha mestra esquecida ou reset silencioso.
- No viewport elegível, transferir o draft para o RichInput existente, sem implementar emulador de edição de shell. Modo direto explícito/TUI/comando em execução continuam literais. Popups não são universais em prompts de senha ou shells remotos sem integração confiável.

### Preservar fora do pedido

Revisões otimistas, idempotência por requestId, sequência de eventos, histórico sanitizado, WAL, FTS, controle de geração, layout splitpanes, bandeja, supervisão de processos, seleção/cópia/colagem, terminal binário, inputModeResolver, ações dos provedores e disponibilidade contextual de Artisan/atalhos/IA. `>` continua restrito a projeto Laravel reconhecido; `/` não deve deixar de aceitar caminhos absolutos; `>>` vence `>` sem atraso artificial de execução.

Não refatorar todos os inputs antigos, stores ou bibliotecas; não adicionar descoberta remota de stack, deploy, portas ou filesystem; não encaminhar credenciais à IA/MaxCode. Não afirmar que restauração de aba é restauração do processo ou terminal anterior.

## 3. Evidências e mapa de arquivos

Working tree inicialmente limpa, branch confirmada `wt-292f24a6`; `.gitignore` já ignora `.worktrees/` e `.max-code-worktrees/`. O diretório deste plano não existia. Não foram examinadas outras worktrees.

### Arquivos existentes inspecionados

| Área | Arquivos e evidência |
|---|---|
| Restauração/shutdown | `src-tauri/src/workspace/restore.rs`, `manager.rs`, `environment.rs`, `mod.rs`: restauração seleciona templates automáticos; shutdown usa encerramento que fecha tabs; spawn sempre local. |
| Banco | `src-tauri/crates/mt-db/src/repos/tabs.rs`, `templates.rs`, `src/migrations/registry.rs`, `migrations/0004_07_workspace_templates_tabs_sessions.sql`: estado durável completo já existe, ID e layout são reaproveitáveis; snapshot sanitizado; migrations sequenciais/checksummed. |
| Contratos | `src-tauri/crates/mt-core/src/workspace/template.rs`, `tab.rs`; `src-tauri/src/commands/workspace.rs`: origem da aba manual/workspace não equivale a conexão local/SSH; ambiente shell/agente também não equivale a transporte. |
| PTY | `src-tauri/crates/mt-pty/src/spawn/{mod,shell}.rs`: executável explícito e args suportados, `cwd` validado localmente, lifecycle preparado antes do processo. |
| Templates | `src/components/workspace/TemplateEditorModal.vue`, `src/stores/templates.ts`: formulário local, emit de input tipado, leitura/cache manual e mutações IPC com expectedRevision. |
| Workspace UI | `src/components/workspace/PaneView.vue`, `src/stores/tabs.ts`: painel sem PTY já tem estado recuperável; hidratação consome snapshot/eventos; não usar IDs de PTY antigos. |
| Entrada | `src/components/terminal/{HybridTerminal,RichInput}.vue`, `src/composables/terminal/{useRichInput,useCompletion}.ts`, `src/terminal/engine/{terminalInstance,inputPipeline,modeTracker}.ts`: viewport e draft rico são caminhos separados; confirmação já tem outcomes e envio atômico; passthrough cancela completion. |
| Contexto | `src/terminal/extensions/{shellIntegration,terminalContext}.ts`, `src/terminal/input/inputModeResolver.ts`, `src/stores/terminal.ts`: OSC 133 correlacionado, fallback /proc, prioridade de TUI/overrides e hints existentes. |
| Provedores | `src/terminal/triggers/{triggerRegistry,providers/index}.ts`, `src/composables/quick-commands/{useArtisanTrigger,useShortcutTrigger}.ts`: parser de prefixos, guards e ações já implementadas; não duplicar catálogos. |
| Bootstrap/segurança | `src/app/{plugins,main}.ts`, `src/app/App.vue`, `src-tauri/src/{lib,main}.rs`, `src-tauri/src/commands/backup.rs`, `src-tauri/tauri.conf.json`: plugin instalado, restauração nativa independente de formulário; helper precisa sair antes do bootstrap; sem endpoint novo. |
| Validação | `package.json`, `src-tauri/Cargo.toml`, `scripts/{check,sync-local-libs}.mjs`, `vitest.config.ts`, `tests/setup.ts`: comandos e gates reais. |

Bibliotecas consultadas em leitura: `MaxComponentsUi/src/components/MaxInputSelect.vue`, `MaxInputPassword.vue`, `MaxInputCheckbox.vue`; `MaxPinia/src/plugin.ts`; `MaxUse/src/index.ts`. Os symlinks efetivos apontam para `/home/johnattas/GitHub/{MaxComponentsUi,MaxPinia,MaxUse}`. Não modificar esses repositórios nesta tarefa. Fonte do componente não comprova integridade do build/export instalado.

Versões declaradas: Vue `3.6.0-rc.13`, Pinia `3.0.1`, Axios `1.7.9`, xterm `^6.0.0`, Tauri API/CLI `2.5.0`, Vite `8.0.0`, Vitest `5.0.5`; Rust usa workspace, `portable-pty 0.9`, `rusqlite 0.40` e `rand 0.9`. Dependências criptográficas novas precisam de seleção de versões/MSRV, lock e revisão, não estão comprovadas como instaladas.

Fontes oficiais/documentação consultada e suas limitações estão no fim da spec. Não foram rodados preflight operacional, suíte, tipos, build, aplicativo, SSH ou migrações. Não há IP/credenciais externos autorizados para teste. Nada nesta investigação comprova funcionamento nativo ou segurança da implementação ainda inexistente.

## 4. Contratos compartilhados a estabilizar antes da implementação dependente

### A. Intenção de aba e restauração

1. Saída real: matar/drenar PTYs, finalizar sessões com `AppExit`, flush e retenção; manter abas/painéis logicamente abertos. Fechamento explícito/última folha/projeto removido conserva fechamento real. Não ignorar sobreviventes/falhas de persistência e reportar sucesso.
2. Restaurar por `TabId`/`launch_snapshot`, independente de template atual ou excluído. Não usar `find_workspace_tab_for_template` para todas as abas: isso colapsaria várias manuais no mesmo template. Reutilizar recuperação de layout sem substituir metadados pelo template editado.
3. Compensar falhas sem perder intenção durável. Materializar aba recuperável, sem PTY, com erro classificado; nova tentativa não duplica processo, sessão ou operação. `WorkspaceRestoreFailure` já comporta tabId; preencher quando conhecido.
4. Reconciliar autoinício depois da restauração por ID, respeitando índice único existente da aba `workspace` por template. Restaurar múltiplas manuais e não multiplicar automáticas a cada boot.
5. Preservar foco/ordem/layout e incrementar gerações/sessões. Remover eventos de boot/PTY antigos. Não derivar novo estado de runtime de cache do frontend.

### B. Conexão e credenciais

Contrato público proposto: união `connection.local | connection.ssh` em config/snapshot, independente de ambiente shell/agente e de origem manual/workspace. SSH contém IP, usuário e auth discriminada por password (credentialId opaco) ou key (identityFile opcional). Não há senha retornável em DTO público.

Input de create/update tem operação write-only de credencial: manter/substituir/remover; revisão otimista/ownership validados nativamente. Não tratar campo vazio da edição como apagar senha. Encriptação antes da escrita; commit de metadados/ciphertext atômico; falha ou cancelamento não deixa credential órfã. Snapshot pode referenciar credencial antiga enquanto uma aba aberta precisar dela; editar/excluir template não destrói essa referência.

Tabelas novas propostas: metadados de cofre singleton (versão, KDF, salt, DEK encapsulada e nonce), credenciais (ID, projeto, nonce/ciphertext, versão), conexão JSON em templates. Não expor ciphertext em stores/cache. Nomes finais/migration ID devem ser revalidados antes de escrever. A última SQL observada é `0011_17_search.sql`; não assumir que `0012` continuará livre em outra revisão.

Cofre: Argon2id → KEK; DEK aleatória encapsulada; XChaCha20-Poly1305 por credencial, nonces novos e AAD vinculada a cofre/projeto/credencial/versão/propósito. Parâmetros e UX completos estão na spec §3.3. Estado público volátil; desbloqueio nativo, revogação na saída, troca autenticada da senha mestra e backup consistente fazem parte do escopo. Sem segredo em settings genéricas, logs, FTS, snapshots ou cache.

IPC novo proposto: consultar estado, configurar, desbloquear, bloquear e trocar senha mestra; eventos sem payload secreto para estado/retentativa. Não criar comando de "ler senha" para o frontend. Permissões mínimas, erros pt-BR sem senha e sem `Debug` de requests. Requests sensíveis não passam por hashing/serialização de operação que persista o conteúdo em claro.

### C. SSH e origem dos recursos

OpenSSH direto no PTY, args estruturados, login falso, sem shell `-c` local para montar ssh. Serviço remoto aplica política de autenticação e askpass privado, vinculado à geração. Helper no mesmo executável é ramo anterior a bootstrap/logs/DB. IPC local autenticado e descartável; prompt de host/passphrase/desafio nunca recebe senha salva indiscriminadamente. Gate nativo de AppImage obrigatório.

Não aplicar hooks de agente/shell locais ao cliente SSH nem marcar o prompt local do projeto como remoto pronto. Fornecer conexão/origem no contexto dos painéis, e bloquear ações de filesystem/projeto locais quando remotos. Reinício, split, nova sessão e restauração propagam a mesma conexão. Sem cofre aberto, aba por senha fica visível e recuperável, não desaparece nem cai para shell local. Retentativas usam locks/idempotência existentes.

No remoto, usuário pode configurar pasta/comando, mas os caminhos não são validados como locais. Sem integração de shell remota verificável, comando inicial fica aguardando ação explícita; não enviar durante autenticação ou por timeout. A UI deve explicar a limitação. Não encaminhar envVars herdadas do host ao servidor. Não instalar scripts remotos nesta entrega.

### D. Frontend e ponte de entrada

- `Origem`: `MaxInputSelect` com opções `{value:'local', label:'Local', name:'Local'}` / remoto equivalente, v-model tipado; `optionValue/optionLabel/optionName` disponíveis na fonte local. Usar `MaxInputText`, `MaxInputPassword`, `MaxInputCheckbox`, botões/modal Max para novos campos e telas de cofre.
- Cache/get de metadados públicos dos templates com MaxPinia, partição por projeto e `isList`/`block_save`; fachada existente preservada. Propor adapter Axios exclusivo do plugin com rotas internas allowlisted → IPC existente. Não modificar Axios global nem criar HTTP fictício. Respeitar AbortSignal/revisões e testar caches entre projetos. Confirmações e credenciais nunca usam autosave; formulário permanece explícito.
- Estado de PTYs/input/cofre é efêmero em stores integradas ao Pinia da aplicação, com plugin MaxPinia, sem persistir runtime sensível. `isCached` não deve ser ativado indiscriminadamente em stores de terminal/cofre só para satisfazer convenção.
- Helpers genéricos, listeners e descarte com MaxUse onde houver utilitário equivalente. Algoritmo de domínio e gate de teclado permanecem locais; não importar VueUse/lodash diretamente para novos helpers.
- Ponte de keydown síncrona, descartável, isolada por instância/paneId/ptyId/geração, composta no handler já existente do xterm. Consome apenas primeiro gatilho elegível; transfere texto para RichInput e foco. Bufferizar eventual segunda tecla durante a transferência para não perder `>>`; não manter esse buffer fora da transição ou em cache.
- Elegibilidade requer prompt confiável ocioso e linha vazia conhecida, além de modo rico permitido e ausência de TUI/hints de aplicação/seleção/composição/busca. Usar/estender minimamente shell integration se o sinal necessário não existir; nunca assumir que todo buffer normal é shell. Descartar inscrição quando instância/PTY muda.
- Reusar parser, CompletionSession e outcomes existentes. Não executar ao abrir popup. Escape apenas fecha seleção, draft permanece; confirmação/envio continua pelo InputPipeline. Respostas de protocolo, colagem e onBinary permanecem fora do roteador de gatilhos.

## 5. Preflight futuro e gates de viabilidade (T0)

Antes de implementar, **somente por leitura**:

1. Conferir branch, status/diff, documentos e trabalho já realizado. Revalidar arquivos afetados após mudança de revisão; não sobrescrever trabalho preexistente.
2. Ler scripts e hooks atuais, exports/dist e symlinks efetivos das libs Max. Validar uma vez integridade antes de trabalho dependente. Na execução, o gate existente é `node scripts/sync-local-libs.mjs --check`; falha bloqueia harness dependente, sem rodar sync mutante ou editar biblioteca externa sem autorização.
3. Conferir que Vue/Pinia/Axios da app e do harness resolvem a mesma identidade esperada; plugin MaxPinia/LocalForage e adapter IPC precisam de bootstrap real nos testes relevantes. Dublê de MaxInputSelect sozinho não prova seleção/label/acessibilidade.
4. Identificar em código os diretórios efetivos de DB, WAL, logs, backups, cache, HOME/SSH/known_hosts e runtime Unix. Worktree isolada não isola esses recursos. Testes só com bancos/dirs temporários e credenciais sintéticas; nada de copiar .env, migrar/resetar DB real ou usar perfil compartilhado.
5. Conferir transporte OpenSSH/versão disponível, askpass force, executável empacotado e allowlist de env do PTY, integração disabled e identidade de processo. Não acessar chaves pessoais nem conectar a servidor sem autorização. Comprovar caminho seguro para iniciar app com dados descartáveis antes do aceite nativo.
6. Revalidar mantenabilidade/licenças/MSRV/features das crates propostas e API de zeroização/RNG; versões não foram fixadas em Plan. Resolver dependências apenas durante Execute, sem atualizar indiscriminadamente o restante do lock.

Gate falhado: tarefa fica `blocked` com próxima ação concreta. Sem recurso de teste nativo/servidor autorizado: `manualPending`; mocks podem avançar testes internos, mas não satisfazem aceite externo. Não substituir o transporte, gravar chave junto do banco ou desligar host verification para contornar dificuldades.

## 6. Etapas executáveis

Cada tarefa inclui sua implementação e testes. Concluir todo o escopo implementável antes de rodar suíte completa/tipos/build; não testar/buildar entre microalterações. Revisões de contrato por leitura são gates, não execução prematura da suíte. Registrar marcos e revisão de conteúdo.

### T1 — Domínio, schema e contratos públicos

Dependência: T0.
- Alterar `mt-core/src/workspace/{template,tab}.rs` para conexão discriminada, defaults legados e novos erros/estados recuperáveis mínimos; não reutilizar TabOrigin para transporte.
- Adicionar migration aditiva em `src-tauri/crates/mt-db/migrations/`, ampliar repositórios de templates/abas e criar repositório de credenciais/cofre. Validar snapshot legacy sem campo de conexão e atualizações concorrentes.
- Preservar integridade do FTS e backups: projeções públicas não podem indexar segredo; ajustar suposições fixas de schema, inclusive `commands/backup.rs` se necessário.
- Registrar DTOs novos no pipeline Specta/comandos/permissões existentes. Não editar `src/types/bindings.ts` manualmente; gerar quando conjunto de contratos estiver estabilizado.
- Testes novos propostos: modelos/validações de conexão e `mt-db/tests/remote_credentials.rs`; estender `migrations.rs`, `upgrade_from_fixtures.rs`, `backup.rs` quando pertinente. Usar fixtures sintéticas, nunca banco pessoal.
Conclusão: schema e payloads públicos/write-only definidos, isolamento por projeto e referências de snapshot cobertos por testes escritos.

### T2 — Cofre e fronteira IPC sensível

Dependência: T1.
- Criar módulos propostos `src-tauri/src/credentials/{mod,crypto,service}.rs` e `src-tauri/src/commands/credentials.rs`; integrar estado e shutdown em `lib.rs`, comandos/Specta e permissões estritamente necessárias.
- Implementar setup, unlock/lock, troca de senha mestra, encryption envelope, validação de KDF/nonce/AAD, serialização versionada e zeroização nativa. Não usar sanitizador como substituto da criptografia.
- Restrições: KDF fora da UI/transação longa, sem logs de parâmetros secretos, unlock simultâneo single-flight, tentativa errada/cancelamento sem alterações; perda de senha não apaga dados. Capturar concorrência lock durante operação.
- Testes de roundtrip, senha errada, parâmetros maliciosos, adulteração, troca de ciphertext entre projetos/IDs, nonces, rollback, troca de senha/backup antigo e ausência de claro em todos os sinks alcançáveis.
Gate: revisão independente focada em segredo/criptografia/IPC antes de disponibilizar ao transporte; nenhuma leitura pública de senha permitida.

### T3 — Persistência e restauração por ID

Dependência: T1; contrato de bloqueio T2 acordado.
- Refatorar `workspace/manager.rs` para separar encerrar runtime de fechar intenção, e `restore.rs` para enumerar abas abertas e restaurá-las pelo snapshot antes do autoinício complementar.
- Reutilizar/extrair primitiva de reopen em `mt-db/src/repos/tabs.rs` sem substituir metadados/snapshot por template atual. Garantir geração, ordem, foco, layouts e referências de sessão.
- Diferenciar rollback de nova abertura versus falha de restauração. Não fechar candidato restaurável; disponibilizar estado com erro/retry. Preservar report e ledger/revisões.
- Cobrir manual duplicada por template, exclusão/edição de template, fechamento explícito/último painel, remoção de projeto, crash, shutdown, falha parcial, dois ensure_restored concorrentes e reexecução de boot.
- Testes novos propostos: `src-tauri/tests/workspace_restore.rs` com harness DB/PTy isolado ou módulo de teste local se visibilidade exigir; não ampliar API pública apenas para testes.
Conclusão: intenção preservada e restauração por identidade sem depender de template nem replay de histórico; regressões de autoinício cobertas.

### T4 — OpenSSH supervisionado e askpass seguro

Dependências: T1, T2 e contrato do restaurador T3.
- Criar módulos propostos `src-tauri/src/remote/{mod,ssh,askpass}.rs`; integrar `main.rs`/entrypoint antes de bootstrap, `workspace/{manager,environment}.rs` e conexões por painel.
- Construir SpawnSpec para SSH com args separados e políticas explícitas; revisar `mt-pty/src/spawn/env.rs` e lifecycle de `mt-shell` antes de ampliar allowlist. Preservar processo/grupo supervisionado; sem fallback local.
- Implementar askpass por socket Unix privado, identidade do helper/ancestral SSH, geração, prazos e revogação. Não enviar senha a prompt desconhecido, host confirm ou passphrase. Expor confirmações efêmeras sem segredos em logs/eventos.
- Propagar origem em split/restart/session_new/restore; não aplicar hooks locais ao SSH. Separar cwd local/remote e comando aguardando confirmação; não prometer prompt remoto pelo readiness local.
- Testes com executável SSH sintético para args/env, identidade/tentativa cruzada/timeout/cancelamento e cleanup; testes nativos reais reservados a T8. Sem segredo em linha de comando ou artefatos.
Gate: revisão independente de auth/host key/IPC/helper e no-orphans. Cancelar conexão limpa recursos e conserva intenção quando a aba permanece aberta.

### T5 — Modal, cofre e stores Max

Dependências: T1/T2; T4 para integração remota.
- Alterar `TemplateEditorModal.vue` para origem/local/remoto e autenticação, com componentes Max, validação condicional, erros claros, estado de edição sem retornar senha e limpeza de refs locais.
- Ampliar `src/ipc/templates.ts`, criar wrapper proposto `src/ipc/credentials.ts`, integrar `src/stores/templates.ts` e store pública de cofre. Propor `src/app/maxPiniaIpcAdapter.ts` para metadata GET via MaxPinia, usando instância Axios exclusiva do plugin em `plugins.ts`.
- Manter fachada por projeto e operações explícitas de save; cache apenas DTO allowlisted, refresh após evento/revisão e invalidação correta. Testar com plugin real para impedir cache/autosave acidental de credenciais.
- Criar componentes propostos `src/components/credentials/VaultUnlockModal.vue` e `VaultSetupModal.vue` com fluxo de troca autenticada (componente separado se necessário); montar em `App.vue`. Expor ação de desbloquear/tentar novamente em `PaneView.vue` sem apagar abas.
- Status de cofre sempre consultado do Rust no bootstrap; cancelamento não bloqueia abas locais nem encerra SSH já autenticado. Não iniciar várias janelas para a mesma solicitação; correlacionar prompts por geração.
- Testes novos propostos `tests/components/workspace/TemplateEditorModal.spec.ts`, `tests/components/credentials/VaultFlow.spec.ts`, testes de stores/IPC/adapter no padrão existente. Verificar foco, teclado, labels, campos e limites por projeto.
Conclusão: R2/R4/R5 disponíveis ponta a ponta, sem senha em cache/store público e com retentativa nativa observável.

### T6 — Contexto remoto e guardas de ações locais

Dependências: T1/T4, junto da integração T5.
- Propagar origem em `src/types/terminal.ts`, contexto e DTO público apropriado. Ampliar providers/adapters locais somente para bloquear execução local indevida em remoto.
- Revisar os pontos de execução de `src/composables/quick-commands/`, `src/terminal/triggers/providers/commandGroupsTrigger.ts`, links e ações que inferem cwd local. Guards devem atuar também no backend quando um comando IPC possa ser chamado fora da UI.
- Em remoto sem integração confiável, mostrar limitação e preservar passthrough; não fingir descoberta remota. Não quebrar provedores locais nem mudar seleção de modelos IA.
- Testar comando/ação originados de pane remoto sem chamadas ao filesystem/projeto local; troca de aba não contamina contexto/cache.
Conclusão: nenhuma ação faz trabalho no projeto local supondo representar destino remoto.

### T7 — Ponte do viewport para gatilhos existentes

Dependências: contratos de contexto T6 estabilizados; pode ser implementada antes da integração de UI T5 se arquivos de escrita não coincidirem.
- Alterar `terminalInstance.ts` e interface pública do handle para registro descartável de intenção síncrona, compondo copiar/colar existente. Não usar API privada do xterm ou bloquear respostas VT no `onData` genérico.
- Criar composable proposto `src/composables/terminal/useViewportTriggers.ts` e integrar `HybridTerminal.vue`/`RichInput.vue` para draft/foco e buffer da transição. Expose mínimo de inserção de draft; reusar outcome/CompletionSession.
- Estabelecer elegibilidade conservadora com estado shell/linha vazia/geração; usar MaxUse para helpers de ciclo/listeners. Se não houver sinal suficiente, estender integração de shell mínima antes de habilitar consumo, não adivinhar pelo buffer.
- Impedir perda da segunda tecla de `>>`, vazamento de prefixo ao PTY, dupla confirmação e handler preso após remontagem. Preservar override passthrough, busca e IME.
- Estender `tests/terminal/{HybridTerminal,RichInput,inputPipeline,triggerRegistry,inputModeResolver}.spec.ts` e criar teste do novo composable; mocks capturam bytes enviados para comprovar exatamente zero antes de confirmar e um envio quando confirmado.
Conclusão: R3 no fluxo de shell local real, sem regressão de entrada direta ou rerroteamento global do terminal.

### T8 — Integração final, bindings, validação agrupada e aceite nativo

Dependências: T1–T7 implementadas e testes escritos, contratos revisados.
- Gerar bindings pelo script existente; revisar diff de bindings/permissões e rever mocks/fixtures/consumidores afetados. Revalidar superfície pública sem credenciais.
- Após gates de isolamento e libs, rodar validações agrupadas da seção 8; diagnosticar harness uma vez, corrigir falhas em lote e repetir somente os checks afetados pela nova revisão, mais suíte integrada quando necessária.
- Executar matriz manual com perfil descartável e servidor SSH autorizado. Aplicativo nativo/empacotado e processo real são gates separados de mocks do Vitest.
- Revisão independente final: domínio/migração, segurança cofre/askpass, regressões terminal/TUI, scope completo. Se delegação não estiver autorizada na sessão Execute, não iniciar agentes por iniciativa própria; solicitar a revisão independente ao responsável ou deixar gate pendente explícito.
- Registrar marcos, falhas e handoff sem declarar conclusão integral com `manualPending`, `blocked` ou falha vigente. Sem commit/merge/push/limpeza desta worktree.

### Concorrência e responsabilidade de escrita futura

Não há delegação autorizada implicitamente por este plano. Em eventual execução paralela autorizada: um responsável único por `mt-core`/schema/bindings/permissões; outro por cofre/remote; outro por UI; outro por ponte de viewport. `manager.rs`, `lib.rs`, `plugins.ts`, `types/terminal.ts` e bindings têm dono único e integração serial. Não dividir tarefas dependentes antes dos contratos T1/T2. Compartilhar disponibilidade de bibliotecas/servidor e documentação uma vez, sem múltiplos executores redescobrindo indisponibilidade. Não rodar suíte completa enquanto houver implementação concorrente.

## 7. Matriz de aceite por tarefa

| Tarefa/requisito | Evidência exigida | Testes pertinentes | Revisão pelo risco | Verificação manual |
|---|---|---|---|---|
| T0 / ambiente | Paths e recursos efetivos isolados; libs/harness íntegros | Gate local-libs após leitura | Operação/isolamento | Perfil nativo e SSH descartáveis autorizados |
| T1 / contratos e migração | Dados antigos locais continuam legíveis; conexões tipadas e ownership | Migração/upgrade, input inválido, FK/snapshot, conflito de revisão | Schema e bindings | Abrir projeto legado no perfil sintético migrado |
| T2 / R4 | Ciphertext autenticado; chave só na memória desbloqueada; nenhuma resposta com senha | Roundtrip, wrong master, nonce/AAD, tamper, KDF limites, rollback/troca/backup | Criptografia e fronteiras de segredo | Abrir app, unlock errado/cancelar/correto, trocar senha, reiniciar |
| T3 / R1 | Mesmo TabId, layout/ordem/foco/títulos, novos PTYs/sessões, sem duplicação | Shutdown/close/crash, múltiplas manuais, template removido, erro parcial | Lifecycle/compensações | Reiniciar duas vezes, fechar aba, bandeja versus saída real |
| T4 / R2 | SSH executado diretamente, auth segura e geração isolada | Args/env, helper cruzado, timeout/cancel, cleanup/processos | Askpass, host key e supervisão | Password, chave/agente, chave protegida, host novo/alterado, AppImage, resize e Ctrl+C |
| T5 / R2/R4/R5 | Componentes Max reais, store/cache público e fluxo de unlock | Modal, edição, validação, plugin/adapter, concorrência e limpeza | Cache/autosave e exposição frontend | Tab/Enter/Escape, senha não repopulada, reconectar pendentes uma vez |
| T6 / isolamento remoto | Ações locais não executadas a partir de contexto remoto | Guards frontend/backend e troca de pane | Execução no destino correto | Remoto não consulta Artisan/Git/filesystem local como servidor |
| T7 / R3 | Popup correto ao digitar no viewport; bytes não duplicados | Cada prefixo, `>>` rápido, escape, Enter/Tab/setas/clique, async/stale/dispose | Regressão terminal e composição de handlers | Laravel `>`, grupos `>>`, `@` estágios, `?`, `/`; Vim/htop/CLI, senha, linha já digitada, redirect, paste/IME |
| T8 / integral | Checks da revisão vigente + evidência nativa | `npm run check`, bindings e testes adicionados | Revisão independente final | Todos os cenários críticos aprovados ou entrega declaradamente pendente |

Testes de segredos usam canários sintéticos para procurar vazamento em DB/WAL/backup/FTS/logs/argv/env/cache/scrollback e requests de inspeção/IA. Não registrar o canário como credencial real. Aceite externo exige servidor/credenciais válidos autorizados; mocks não provam autenticação, host trust ou empacotamento.

## 8. Comandos confirmados e procedimento de validação futura

Executar na raiz autorizada, somente após preflight e implementação completa:

1. `node scripts/sync-local-libs.mjs --check` — gate de integridade sem sincronização mutante; se falhar, bloquear validação dependente. Pode ser reaproveitado na mesma revisão se libs/manifestos/links não mudarem.
2. `npm run bindings` — script comprovado: exporta os contratos via teste Rust ignorado específico. Executar quando contratos finais estiverem estáveis, com ambiente isolado; é ação de Execute, não verificação realizada em Plan.
3. `npm run check` — orquestrador existente: libs → ESLint → Stylelint → tsgo → vue-tsc → Vitest com cobertura → Vite build → cargo fmt --check → clippy → testes Rust workspace. O script para na primeira falha: etapas posteriores não são automaticamente aprovadas.

Comandos individuais comprovados para revalidação dirigida, sem contornar gate de biblioteca: `npm run lint`, `npm run lint:style`, `npm run typecheck`, `npm run typecheck:vue`, `npm run test:coverage`, `npm run build`. Backend, a partir de `src-tauri`: `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --workspace --locked`. `npm test` também existe, mas não substitui cobertura/gates finais.

Esses comandos não foram executados nesta sessão. Não prescrever comando inexistente de E2E/empacotamento: `tauri.conf.json` observado tem `bundle.active=false`. Descobrir procedimento nativo/empacotado seguro e suas pré-condições no Execute antes de rodá-lo; não ativar release/deploy nem alterar configuração global para simular validação.

Não contornar hooks, usar ignoreConfig, remover verificação ou chamar comando direto só para ignorar integridade falhada. Não copiar .env nem rodar migrations/resets contra dados reais. Diagnosticar bootstrap/identidades/cache em conjunto antes de repetir testes equivalentes; guardar todos os exit codes e falhas, inclusive da suíte completa. Resultados só valem para o conteúdo/revisão que verificaram. Mudança sem impacto demonstrável pode reaproveitar check; mudança de contrato, dependência ou artefato invalida os checks relevantes.

Para operações assíncronas futuras, aguardar evento/conclusão quando disponível; espera limitada e recuo quando necessário, sem polling repetitivo sem novidade. Cancelamento mantém resultado parcial e próxima ação explícita. Falha repetida sem progresso exige diagnóstico de causa, não apenas novo comando igual.

## 9. Segurança, operação e reversão

- O cofre protege cópia de dados em repouso; não protege host comprometido, dumps de memória ou senha mestra fraca. Não anunciar "banco inteiro criptografado".
- Não armazenar senha/chave mestra junto do ciphertext; não enviar senha SSH para challenge diferente de password, fingerprint ou prompt remoto. Mudança de host key bloqueia e exige resolução consciente.
- Preservar autorização em cada chamada IPC, ownership de projetos/credenciais e identidade de conexão. Eventos stale nunca desbloqueiam ou enviam senha para outra geração/painel.
- Backups antigos conservam versão/senha mestra antiga; informar isso na troca. Não apagar backups automaticamente nem prometer eliminação segura de ciphertext antigo em WAL/FS. Banco migrado não deve ser aberto por binário antigo sem análise de compatibilidade.
- Antes de migração operacional, somente com autorização e isolamento comprovados, usar o mecanismo de backup consistente já existente; a sessão Plan não autorizou criar backup do banco real. Rollback preferido é reverter código da implementação e restaurar backup de teste compatível, nunca migration reversa destrutiva improvisada.
- Não matar/alterar serviços externos. Processo remoto iniciado pelo usuário pode sobreviver no servidor pelas regras dele; supervisão local deve encerrar cliente e descendentes locais, sem prometer matar todo daemon remoto.
- Identificar restauradores bloqueados sem segredo em mensagem/log. Sem cofre/SSH/rede disponíveis, manter abas locais e candidatas remotas, com erro e ação concreta, sem repetir conexão infinitamente.
- Desempenho: KDF bounded/off-main-thread, unlock single-flight, restauração sem tempestade de conexões, locks por aba e escritor SQLite existente, sem transação presa aguardando rede/UI. Timeout/cancelamento limpam helper e conexão parcial.

## 10. Retomada, marcos e execution-state.json

Ao iniciar Execute: ler estes documentos, verificar alterações já existentes e registrar somente lacunas reais. Não reabrir decisões do usuário (SQLite cifrado + senha mestra) nem mudar transporte/política de segurança sem evidência nova e revisão. Nas retomadas, revalidar diff/estado local e conteúdo das tarefas relevantes, sem releitura integral compulsória de tudo. Registrar progresso após T1, T2, T3/T4, integração frontend e validação/handoff.

Somente durante Execute, escrever `docs/plans/wt-292f24a6/execution-state.json`; **não criar nesta sessão Plan**. Vincular ao SHA256 real de `plan.md` e às revisões reais de conteúdo das tarefas e da entrega. Contrato version 2 estrito, sem campos adicionais:

- Raiz: `{version: 2, planRevision, tasks, verification, updatedAt?, nextAction?}`. `planRevision` é hash SHA256 hexadecimal real do plano. `updatedAt`, se presente, é inteiro positivo em milissegundos, nunca ISO. Não usar `schemaVersion`, `worktree`, `branch`, `head`, `planPath`, `status` ou `checks` solto na raiz.
- Tarefa: `{id, status, files, validations, revision?, nextAction?}`. IDs T0–T8; `files` lista os caminhos relativos reais; `validations: []` é sempre obrigatório. Estados: `pending`, `running`, `completed`, `blocked`, `manualPending`, `validationFailed`. Não adicionar `title`/`evidence`. `revision`, quando presente, é SHA256 real do conteúdo factual definido para a tarefa, não título, timestamp, nome de branch ou hash inventado.
- `verification`: `{revision, checks}`. `revision` é SHA256 real da revisão factual validada, com critério determinístico de composição do conteúdo mantido pelo executor.
- Cada check, **somente** em `verification.checks`: `{command, revision, result, exitCode?}`. `command` tem até 4096 caracteres e corresponde a comando realmente executado; `revision` é SHA256 real correspondente ao conteúdo verificado. `result` estritamente `passed|failed|pending`; passed exige `exitCode: 0`, failed exige inteiro diferente de zero, pending não aceita exitCode. Nunca `exitCode: null`.
- `nextAction`, na raiz/tarefa, até 512 caracteres, deve explicar impedimento e próxima ação concreta. Campos `blocker`, `diagnostic`, `branchSync`, `durableSync` ou `followUp` são proibidos em qualquer nível.
- `validations` não recebe objetos de check duplicados. Manter o array obrigatório vazio até confirmar a representação de validações de tarefa aceita pelo consumidor MaxCode; não inventar outro schema. Se esse consumidor exigir validações por tarefa adicionais, resolver seu contrato antes de emitir registros dessa forma. Qualquer resultado de validação, se presente no contrato confirmado, só aceita `passed|failed|pending`, nunca `blocked`.

Registrar todas as falhas realmente observadas, inclusive do `npm run check`, sem inferir exitCode/aprovação de `tool:end.ok`. Checks vazios, resultado ausente/desatualizado ou falha vigente impedem validação integral. Version 1 pode permanecer legível por compatibilidade, sem provar conclusão. Registro autodeclarado não comprova execução nativa nem detecta check omitido: evidência de aceite manual precisa existir no handoff, sem acrescentar campos proibidos ao JSON.

Em handoff, informar tarefas restantes, arquivos alterados, gates aprovados na revisão vigente e próxima ação. `manualPending` exige ambiente/procedimento seguro específico, não pode ser reportado como conclusão total. Preservar worktree e alterações para o MaxCode; não commitar, integrar ou limpar automaticamente.

## 11. Revisão desta proposta

- Escopo cobre as três mudanças e as duas decisões adicionais do usuário; o cofre é consequência necessária da persistência cifrada com senha mestra, não substituição de requisito.
- Restauração reutiliza o banco e snapshots existentes, sem segunda persistência frontend; distingue shutdown, close e falha de restauração.
- SSH inclui transporte, autenticação, ciclo de vida e segredo, não só campos de formulário; bloqueia ações locais indevidas e não presume recursos remotos inexistentes.
- Gatilhos reutilizam provedores/CompletionSession e entrada rica, preservando TUI/passthrough, operadores shell, colagem e bytes binários.
- UI usa as bibliotecas obrigatórias com fronteira explícita entre cache público e segredo/runtime; não exige mudança nas bibliotecas compartilhadas.
- Contratos, tarefas, dependências, testes e matriz de aceite estão vinculados; gates externos e versões não comprovadas estão declarados, não tratados como aprovados.
- Limitações importantes para aprovação: abas históricas já fechadas não são recuperadas automaticamente; comandos iniciais manuais restaurados aguardam ação; recursos de projeto remoto não são simulados; aceite OpenSSH/AppImage e revisão independente permanecem obrigatórios na execução.
