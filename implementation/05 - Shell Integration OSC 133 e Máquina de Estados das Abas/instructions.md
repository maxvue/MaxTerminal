# Fase 05 — Shell Integration OSC 133 e Máquina de Estados das Abas

> Fonte da verdade: [`../../AGENTS.md`](../../AGENTS.md), §§4.1, 4.2 e 6.
> Contratos compartilhados: [`../README.md`](../README.md). Detalhamento normativo: [`spec.md`](./spec.md).

## 1. Objetivo

Reconhecer fronteiras de comandos no PTY com hooks bash/zsh/fish, produzir blocos sanitizados e manter os estados `running`, `continuous`, `finished`, `error` e `idle`. O resultado de cada execução e a confirmação de visualização têm identidade própria: focar uma execução anterior não confirma uma conclusão futura.

OSC 133 A/B/C/D delimita prompt/entrada/execução/fim; **não transmite sozinho comando ou cwd**. Esses metadados vêm do protocolo auxiliar versionado da §4 da spec. A origem do dado, a confiança e a possibilidade de resultado desconhecido fazem parte do contrato público.

## 2. Escopo

### Incluído

- Crate `mt-shell`: injetor, scripts embarcados, scanner incremental de bytes, tracker, captura limitada, fallback `/proc`, redutor de estados, registry runtime e persistência.
- Hooks interativos sem modificar dotfiles; coexistência com configuração e prompts do usuário, login/non-login, opt-out e degradação explícita.
- OSC terminado em BEL ou ST, dividido em quaisquer chunks, com limites de memória, recuperação de sequências inválidas e testes de não interferência no stream bruto.
- `sessions` mínima e `command_blocks` com FTS5, pertencentes a esta fase; nenhum requisito de tabelas `projects`, `tabs`, templates ou workspace da 07.
- Estado por vínculo runtime `(tabId, paneId, ptyId, sessionId)` usando os ids da 01, geração por execução e confirmação com identidade exata.
- Agregador puro para futuros panes; playground da 04 demonstra duas abas lógicas de um painel, sem construir splits.
- APIs de consulta/visualização/override contínuo; eventos revisionados; `ErrorContext` sanitizado para consumidores posteriores.
- Store `useTabStatusStore`, store de histórico `useShellHistoryStore`, adaptador para o **`useCommandBlocksStore().feed` já existente na 04**, composables e indicadores.

### Limites de responsabilidade

| Consumidor posterior | Entrega desta fase |
|---|---|
| 06 — projetos e badges | `countsFor(tabIds)`; associação projeto/aba é externa |
| 07 — abas, templates, splits e sessões | vínculo runtime, sessão mínima, política contínua, detach/reset interno; não é dependência |
| 08 — portas | extensão opcional de classificação; não presumir servidor porque levou muito tempo |
| 09 — tray/notificações | snapshots e `hasActiveProcesses` calculado por todos os panes |
| 11/12 — Inspecionar/diagnóstico | `command_block_error_context`; ações reais ficam nas fases consumidoras |
| 13 — intervenção de agente | entrada interna de atenção versionada; detecção de texto/modelo não entra aqui |
| 17 — busca global | consulta FTS paginada e limites de captura |

SSH e shells aninhados são uma execução do shell externo. Não instalar hooks remotamente. O PTY combina stdout/stderr: a cauda de erro é **saída combinada da execução que falhou**, não stderr isolado.

## 3. Pré-requisitos e reconciliação

| Base | Contrato efetivamente documentado | Uso/adição nesta fase |
|---|---|---|
| 01 | ids Uuid v7, `MtError`, bindings e `unwrap` | Reutilizar, sem redeclarar ids TS ou importar modelos da 07 |
| 02 | `SpawnSpec`, `PtyOwner`, `OutputTap::on_output(pty_id, seq, bytes)`, `ExitInfo`, `PtyManager::{add_output_tap,process_report,foreground_process}` | Tap **somente leitura**. Adicionar lifecycle genérico de preparação antes de iniciar reader e argumento IPC opcional de integração; detalhe na spec §2 |
| 03 | `Db::{read,write}`, `DbSetup`, migrations, `SanitizedText`, `StreamSanitizer`, `FtsSpec`, `build_match_query` | `DbWriter` é privado: não importar nem inventar `submit`. Registrar migrations/FTS e usar a fachada |
| 04 | `TerminalExtension`, `TerminalInstanceHandle`, `CommandBlockFeed`, `CommandBlockModel`, `registerContextProvider`, `registerInputModeHint` | Adaptar feed/status e preservar store; posições visuais aproximadas não viram offsets persistentes |

As extensões aditivas descritas aqui são **entregáveis da implementação da 05**, não APIs já existentes das bases. Esta revisão modifica apenas os dois documentos da 05.

Ferramentas da implementação: Rust/workspace da 01, Node/gerenciador e scripts do projeto, bash ≥4.4, zsh ≥5.8 e fish ≥3.6. CI deve instalar os três shells; ausência local é skip explícito, nunca teste contado como aprovado. Conferir compatibilidade das APIs com o lockfile; a consulta Context7 nesta revisão ficou indisponível por cota mensal.

## 4. Entregáveis

```text
src-tauri/crates/mt-shell/
  scripts/{bash/mt-integration.bash,zsh/*,fish/mt-integration.fish}
  src/{lib,kind,inject,tracker,capture,fallback,persist,service,runtime}.rs
  src/osc/{mod,scanner,escape}.rs
  src/state/{machine,registry,daemon}.rs
  tests/{scanner_split,tracker,state_table,shells_bash,shells_zsh,shells_fish,injection_no_dotfile_changes}.rs
  benches/osc_scan.rs
src-tauri/crates/mt-db/migrations/NNNN_05_shell_sessions_command_blocks.sql
src-tauri/crates/mt-db/src/repos/{sessions,command_blocks}.rs
src-tauri/crates/mt-pty/src/spawn/             # lifecycle genérico aditivo
src-tauri/src/{commands,events}/shell.rs
src-tauri/src/commands/tab_state.rs
src/ipc/{shell,tabState}.ts
src/stores/{tabStatus,shellHistory}.ts
src/composables/{useShellMarkers,useShellCommandBlocks,useTabSeenTracker}.ts
src/terminal/extensions/shellIntegration.ts
src/components/shell/{TabStatusIndicator,CommandBlockStatus}.vue
```

Atualizar composição do binário, registro Specta/capabilities e playground da 04 durante a implementação. Não criar um segundo hub/ack do PTY nem uma segunda store `commandBlocks`.

## 5. Critérios de aceite verificáveis

### Parser, protocolo e hooks

- [ ] Corpus BEL/ST, ESC/ST partidos, UTF-8 partido, múltiplos comandos no mesmo chunk, OSC desconhecido, CSI/DCS, CAN/SUB, overflow e EOF: todas as divisões em 2/3 chunks produzem as mesmas marcas e offsets; stream enviado ao Channel/replay permanece byte a byte idêntico.
- [ ] `proptest` (≥2.000 casos) sem panic/alocação ilimitada; OSC >32 KiB não gera marca; recupera no terminador ou cancelamento definido.
- [ ] Marcas sem nonce, nonce errado, `g` antigo, metadado duplicado ou forjado não mudam estado autoritativo. Documentar que nonce evita confusão/replay acidental, **não isola processos do mesmo usuário**.
- [ ] Cada shell executa `true`, `false`, `(exit 42)` (equivalente fish), `cd`, multiline, pipeline, função, builtin, `exec`, `Ctrl+C`, prompt vazio e entrada com `;`, barra, BEL, ESC e Unicode.
- [ ] Comando completo só é anunciado quando disponível; bash com histórico desabilitado/filtrado não atribui o comando anterior nem usa `$BASH_COMMAND` como linha completa.
- [ ] `$?`/`$status` capturado antes de helpers; status visto por hooks do usuário preservado; DEBUG/PROMPT_COMMAND e temas não causam recursão, duplicação ou execução espúria.
- [ ] Login/non-login e flags incompatíveis têm testes; hashes de dotfiles permanecem iguais; nenhum arquivo novo em HOME; nonce não herdado por filhos lançados após bootstrap.
- [ ] Opt-out/shell incompatível → `proc` imediatamente; sem handshake → após 5 s; chegada tardia de handshake reconcilia uma execução, sem dupla conclusão.

### Estados e identidade

- [ ] Tabela da spec §6 coberta para foco presente/ausente, PTY substituído, conclusão atrasada e confirmação de geração anterior.
- [ ] `sleep 2` → verde; sucesso não visto → âmbar; falha não vista → vermelho; ao observar a conclusão correspondente, bucket → neutro preservando resultado.
- [ ] Manter foco durante execução não confirma automaticamente D; frontend confirma somente a geração concluída efetivamente apresentada.
- [ ] `exitCode=null` → bloco `unknown`, estado `idle` com conclusão desconhecida; nunca sucesso fabricado. Saída do shell não substitui resultado conhecido de comando já concluído.
- [ ] `npm run dev` → contínuo por política/heurística; `vite build`, `docker compose up -d` e tarefa apenas demorada não promovem; override `off` vence heurística.
- [ ] Agregação determinística, uma aba em um bucket, sem contar ids duplicados; erro não visto em um pane não esconde processo ativo em outro para o tray.

### Persistência, captura e frontend

- [ ] Migration aplicada sobre base03/vazio, reaplicação pelo runner, rollback, FK/cascade, FTS insert/update/delete/rebuild; ids textuais e rowid estável.
- [ ] Segredos partidos entre chunks, linhas, limites cabeça/cauda e lacunas não chegam a DB, FTS, eventos, logs ou `ErrorContext`; usar `SanitizedText` nos repositórios.
- [ ] Teste `echo 'Authorization: Bearer abcdef123456'; false` retorna contexto mascarado, sem ANSI, até 50 linhas por padrão/200 no máximo.
- [ ] Fila limitada em bytes **e** mensagens; saturação marca lacuna/degradação, não cresce sem limite, não congela PTY e não mantém `running` fictício após saída.
- [ ] DB indisponível: estado avança, evento declara `persistence=failed`, contexto vem do cache sanitizado limitado ou erro tipado; não promete commit inexistente.
- [ ] Hidratação assina antes de consultar, descarta revisões antigas, reconcilia perdas/reload e limpa listeners; replay visual não executa novamente transições/INSERTs.
- [ ] Adaptador usa `begin/markOutputStart/end/patchMeta`, mapeando `succeeded→success`, `failed→error`, `unknown→aborted`; sem store duplicada nem `v-html`.
- [ ] `cargo test -p mt-shell`, testes DB/migrations, testes frontend afetados, typecheck/lint e `cargo clippy --workspace --all-targets -- -D warnings` passam na implementação.
- [ ] Medir benchmark do scanner e pipeline: metas ≥1 GiB/s sem ESC, ≥300 MiB/s corpus OSC/ANSI, overhead ≤5%; p95 estado após D ≤50 ms sob carga nominal, sem aguardar SQLite. Registrar hardware/build; metas são critérios, não resultados desta revisão.
- [ ] Rajada 50 MB com captura: RSS incremental por PTY ≤16 MiB e limites mantidos com consumidor lento; testes isolam o custo do scrollback/replay preexistente.

## 6. Riscos e mitigação

| Risco | Resposta obrigatória |
|---|---|
| Temas/traps substituem hooks | Instalação idempotente, testes de coexistência, capacidade degradada explícita |
| `/proc` perde comandos rápidos/builtins | `source=proc`, confiança inferida, código desconhecido; não afirmar prompt/sucesso pela ausência de filho |
| OSC de programa imita integração | Nonce + geração + ordem, sem autorização baseada em output; metadados nunca avaliados |
| Consumidor atrasado/DB cheio | Fila limitada, lacunas explícitas, cache sanitizado, reidratação autoritativa |
| Acoplamento à 07 | Runtime e sessão mínima próprios, sem FK para tabelas futuras |
| Falso visto por foco | ACK com `ptyId`, `blockId`, geração e revisão observada |
| Segredo recortado antes de sanitizar | Sanitização streaming antes de retenção; lacuna invalida contexto parcial |
