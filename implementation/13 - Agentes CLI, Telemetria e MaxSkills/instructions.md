# Fase 13 — Agentes CLI, Telemetria e MaxSkills (`instructions.md`)

> Fonte da verdade: [`../../AGENTS.md`](../../AGENTS.md) §5.2 e contratos compartilhados de [`../README.md`](../README.md) (§2.1–§2.6).
> Especificação técnica detalhada: [`./spec.md`](./spec.md).
> Depende de: Fases 07, 08, 09 e 12 (e, transitivamente, 02, 03, 05 e 06).

---

## 1. Objetivo da Fase

Transformar o MaxTerminal em um **orquestrador nativo dos 4 Agentes CLI** — `claude` (Claude Code), `codex` (Codex),
`agy` (Antigravity) e `opencode` (OpenCode) — entregando integralmente o AGENTS.md §5.2:

1. **Registro de agentes** (`mt-agents`): descritores declarativos (binário, args, env, detecção via `PATH`, versão,
   capacidades verificadas por versão e padrões de possível espera) que preenchem o `environment_kind = 'agent'` dos Templates da Fase 07.
2. **Templates e atalhos dedicados**: abrir uma aba de cada agente com 1 clique ou 1 atalho de teclado.
3. **Isolamento opcional em Git Worktree**: ao abrir a aba de agente, criar/vincular uma worktree (`.worktrees/wt-<slug>`
   ou `.max-code-worktrees/wt-<slug>`) usando a API da Fase 08, vincular aba ↔ worktree e oferecer remoção ao fechar.
4. **Telemetria estilo `StatusLine`** na barra da aba: modelo ativo, % da janela de contexto, cotas de 5h e semanal.
5. **Alerta de Permissão/Input**: detectar quando o agente pausa aguardando aprovação/resposta → aba em `error`
   ("Agente aguarda intervenção", Fase 05), destaque do projeto (Fase 06) e notificação nativa (Fase 09).
6. **MaxSkills + Memória Contextual**: sincronizar as skills de `~/GitHub/MaxSkills` para os diretórios de cada agente
   (symlink/cópia, conflitos, watch) e manter memória de comandos recorrentes por projeto exposta como sugestões.

Ao final, o usuário consegue pressionar `Ctrl+Alt+Shift+C`, confirmar o isolamento de um projeto Git e abrir o
Claude Code em uma worktree; `Ctrl+Alt+C` respeita a preferência de isolamento, desativada por padrão.
Modelo/contexto/cotas aparecem quando a versão e a fonte os disponibilizam; ausência é `null`, nunca zero inventado.
Alertas confirmados e indícios heurísticos têm apresentação distinta, com fonte e data de observação.

---

## 2. Escopo

### 2.1. Incluído

| # | Item | Detalhe |
|---|------|---------|
| I-01 | Crate `mt-agents` | Módulos `registry`, `detect`, `launch`, `isolation`, `relay`, `telemetry/*`, `attention/*`, `skills/*`, `memory`, `settings`, `error` (ver spec §2). |
| I-02 | `AgentDescriptor` dos 4 agentes | `AgentKind::{Claude, Codex, Agy, OpenCode}`; descritores embutidos (`descriptors/*.toml` via `include_str!`) com override do usuário em `$XDG_CONFIG_HOME/maxterminal/agents.toml`. |
| I-03 | Detecção de instalação | Resolução no `PATH` do *login shell* do usuário + diretórios extras (`~/.local/bin`, `~/.bun/bin`, `~/.npm-global/bin`, `~/.opencode/bin`), execução de `--version` com timeout de 3 s, cache em memória + evento `agent://installations-changed`. |
| I-04 | Integração com Templates (Fase 07) | Implementação do ponto de extensão `EnvironmentResolver` da Fase 07 para `environment_kind = 'agent'`: monta `program`, `args`, `env`, `cwd` finais do PTY; comando `agent_templates_ensure` cria os 4 templates dedicados (idempotente) por projeto. |
| I-05 | Atalhos e lançador de 1 clique | `AgentLauncherMenu.vue` na barra de abas + atalhos padrão `Ctrl+Alt+C/X/A/O` (com `Shift` = abrir isolado em worktree), configuráveis e com detecção de colisão. |
| I-06 | Isolamento em worktree | `agent_worktree_propose` (slug/branch/caminho), `agent_tab_open` com `WorktreeIsolation::{None, Create, Existing}`, tabela `agent_tab_links`, `agent_tab_close_check` + `AgentWorktreeCloseDialog.vue` (Manter / Remover / Remover e apagar branch). Remoção nunca automática quando há alterações. |
| I-07 | Relay de agentes | Binário auxiliar `mt-agent-relay` (sidecar) + servidor Unix socket `$XDG_RUNTIME_DIR/maxterminal/agent-relay.sock` (0600, token por aba) recebendo `statusline`, `hook <evento>` e `codex-notify`. |
| I-08 | Telemetria | Trait `TelemetrySource` + fontes `ClaudeStatusLineSource`, `ClaudeTranscriptSource`, `CodexSessionSource`, `OutputHeuristicSource`; fontes internas/heurísticas só habilitadas após validação da versão e correlação inequívoca da sessão. Agregador com prioridade por campo, throttle de 1 Hz, proveniência, validade, qualidade e snapshot persistido. |
| I-09 | `AgentStatusLine.vue` | Componente compacto na barra da aba: modelo, barra de contexto %, cotas 5h/semanal, estado de atenção, tooltip com fonte/idade dos dados; "—" quando indisponível. |
| I-10 | Detector de atenção | Máquina de estados por execução: `Notification` tipada do Claude pode confirmar espera; `Stop`/Codex `notify` indicam término de turno, não permissão. Regex/OSC/BEL/inatividade geram somente indício, nunca aprovação automática ou certeza. |
| I-11 | Integração com estados de aba | `TabStateInput::AttentionRequired` da Fase 05 → `TabStatus = 'error'`, `seen = false` em episódio confirmado; resolução correlacionada restaura o estado subjacente atual sem apagar falha real. Extensão requerida descrita em spec §1.2. |
| I-12 | Notificação nativa | Uso do `Notifier` da Fase 09 com ação "Abrir aba"; sem notificação se a janela está focada **e** a aba é a ativa; 1 notificação por episódio. |
| I-13 | Sincronização MaxSkills | Catálogo a partir de `index.json` (fallback: varredura de `**/SKILL.md`); alvos por agente/escopo (projeto ou global); modos `symlink`/`copy` com manifesto; detecção de conflito (`ForeignSymlink`, `Unmanaged`, `LocallyModified`); resolução `Keep`/`Overwrite` (com backup)/`Adopt`; *dry-run*; watch com debounce. |
| I-14 | Memória contextual | Tabela `command_memory` derivada **incrementalmente e com deduplicação** de `command_blocks` (Fase 05): frequência, sucesso/falha, último uso/cwd, fixar/ocultar/esquecer. Painel de sugestões por *frecency* + prefixo, inserção explícita no input da Fase 04; sem fabricar histórico de execução. |
| I-15 | Stores e IPC frontend | `src/stores/agents.ts` (`useAgentsStore`), `src/stores/skills.ts` (`useSkillsStore`, incluindo memória contextual), `src/ipc/{agents,skills,memory}.ts`, composables e componentes (spec §6). |
| I-16 | Migration | `mt-db/migrations/NNNN_13_agents_skills_memory.sql` (próximo número sequencial livre) com as tabelas da spec §5. |
| I-17 | Pontos de extensão | `agent_tab_open` aceita worktree existente (consumido pela Fase 16); `memory_suggestions` e `MemoryQuery` reutilizáveis (Fase 15); `AgentRegistry::descriptors()` público. |

### 2.2. Fora de escopo (e fase responsável)

| Item | Fase |
|------|------|
| Spawn de PTY, kill de árvore de processos, leitura de `/proc` (apenas consumidos aqui) | 02 (já entregue) |
| Sanitizador `sanitize()`, pool SQLite, runner de migrations | 03 (já entregue) |
| Infra de gatilhos/sugestões do input rico, intercepção de teclas no xterm | 04 (já entregue) |
| Máquina de estados `TabStatus`, `command_blocks`, OSC 133 | 05 (já entregue) |
| Badges e destaque visual do projeto (apenas alimentados aqui) | 06 (já entregue) |
| CRUD de templates, abas, split panes, sessões | 07 (já entregue) |
| Criação/remoção/status de worktree e branch (API consumida) | 08 (já entregue) |
| Infra de notificações nativas, tray | 09 (já entregue) |
| Catálogo de harness/modelos/raciocínio do MaxAiManager e conversão NL→shell | 12 (já entregue) |
| Comandos Artisan `>` e atalhos `/` | 14 |
| Grupos de comandos `>>` (podem consumir `memory_suggestions` como fonte de sugestão) | 15 |
| Abrir aba de agente posicionada na worktree de uma issue; "Corrigir todos os Bugs" | 16 (consome `agent_tab_open` com `WorktreeIsolation::Existing`) |
| Empacotamento do sidecar `mt-agent-relay` (PKGBUILD/AppImage), E2E com agentes reais, busca global FTS5 | 17 |
| Edição automática de `.gitignore` dos projetos para diretórios de skills | Não previsto no AGENTS.md — apenas reportado no plano de sincronização |
| Envio de prompts automáticos/iniciais ao agente, gerenciamento de contas/login dos agentes | Não previsto no AGENTS.md |

---

## 3. Pré-requisitos

### 3.1. Fases e contratos consumidos
| Fase | Contrato consumido (nomes esperados; ver spec §1.2 para adaptação) |
|------|------------------------------------------------------------------------|
| 01 | `mt-core` (`MtError`, `ErrorCode`, ids, `AppPaths`), `tauri-specta` |
| 02 | `PtyManager::{foreground_process, add_output_tap, subscribe}`, `OutputTap`; supervisão de PTYs consumida via Fase 07 |
| 03 | `mt_db::Db`, `sanitizer::sanitize()`, marcador de redação, runner de migrations |
| 04 | `registerTerminalExtension`, `useRichInput`; atalhos no xterm e sugestões explicitamente selecionadas |
| 05 | `TabStateRegistry::apply`, `TabStateInput::AttentionRequired`, tabela `command_blocks`, `ShellService::subscribe()` / `CommandEvent::Finished` |
| 06 | `project_get` (pasta raiz, título), store `useProjectsStore` (destaque) |
| 07 | `tab_templates.environment_kind`/`agent_id`, `EnvironmentResolver`, `WorkspaceManager::open_tab`, `WorkspaceObserver`, evento `tab://closed` |
| 08 | `WorktreeManager::{list, create, remove}`, `GitService::status_once`, `GitRunner::run` |
| 09 | `AttentionRegistry`, `Notifier::notify_user`, evento `os://focus-tab`, foco da janela |
| 12 | `ai_catalog_get` / `HarnessCatalog` como sugestões de seleção; mapeamento explícito para IDs/flags do CLI. Catálogo do gateway não comprova suporte local. |

### 3.2. Ferramentas
| Ferramenta | Uso | Verificação |
|------------|-----|-------------|
| Toolchain da Fase 01 | build/test | `npm run check` |
| `git` ≥ 2.40 | worktrees | `git --version` |
| Agentes CLI (opcionais p/ testes manuais) | validação das suposições | `claude --version`, `codex --version`, `agy --version`, `opencode --version` |
| `~/GitHub/MaxSkills` | fonte local de skills | diretório legível; `index.json` opcional, descoberta de `SKILL.md` suportada |
| Crates novos | `notify` (watch), `regex`, `strip-ansi-escapes`, `sha2`, `walkdir`, `which`, `tokio` (`net` Unix), `serde_json`, `toml` | `cargo tree -p mt-agents` |

---

## 4. Entregáveis

### 4.1. Backend (`src-tauri/`)
- `crates/mt-agents/Cargo.toml`
- `crates/mt-agents/src/lib.rs`, `error.rs`, `settings.rs`
- `crates/mt-agents/src/registry.rs`, `descriptors/{claude,codex,agy,opencode}.toml`
- `crates/mt-agents/src/detect.rs`, `launch.rs`, `isolation.rs`
- `src/agents/{adapters.rs,resolver.rs}`: integração com `EnvironmentResolver` do binário da Fase 07, sem dependência circular
- `crates/mt-agents/src/relay/{mod.rs,server.rs,protocol.rs}`, `src/bin/mt-agent-relay.rs`
- `crates/mt-agents/src/telemetry/{mod.rs,aggregator.rs,claude_statusline.rs,claude_transcript.rs,codex_session.rs,output_heuristic.rs,tail.rs}`
- `crates/mt-agents/src/attention/{mod.rs,detector.rs,patterns.rs}`, `patterns/default.toml`
- `crates/mt-agents/src/skills/{mod.rs,catalog.rs,targets.rs,sync.rs,conflict.rs,watch.rs}`
- `crates/mt-agents/src/memory.rs`
- `crates/mt-agents/tests/{registry.rs,detect.rs,launch.rs,isolation.rs,relay.rs,telemetry.rs,attention.rs,skills_sync.rs,memory.rs}` + `tests/fixtures/**`
- `crates/mt-db/migrations/NNNN_13_agents_skills_memory.sql`
- `src/commands/{agents.rs,skills.rs,memory.rs}`, registro em `src/specta.rs` e `src/lib.rs`
- `tauri.conf.json`: `bundle.externalBin` com `binaries/mt-agent-relay` (empacotamento final na Fase 17)

### 4.2. Frontend (`src/`)
- `ipc/agents.ts`, `ipc/skills.ts`, `ipc/memory.ts`
- `stores/agents.ts`, `stores/skills.ts`
- `composables/useAgentShortcuts.ts`, `useAgentTelemetry.ts`, `useAgentAttention.ts`, `useCommandMemorySuggestions.ts`
- `components/agents/{AgentStatusLine.vue,AgentLauncherMenu.vue,AgentTabOpenDialog.vue,AgentWorktreeCloseDialog.vue,AgentAttentionIndicator.vue,AgentSettingsPanel.vue}`
- `components/skills/{SkillsSyncPanel.vue,SkillConflictList.vue,CommandMemoryList.vue}`
- `types/bindings.ts` regenerado (sem tipos escritos à mão)
- `tests/` → `stores/agents.spec.ts`, `stores/skills.spec.ts`, `components/agents/AgentStatusLine.spec.ts`, `components/agents/AgentWorktreeCloseDialog.spec.ts`, `composables/useAgentShortcuts.spec.ts`

---

## 5. Critérios de Aceite

Comandos executados na raiz da worktree devem terminar com **exit code 0**, salvo indicação.

### 5.1. Qualidade
- [ ] CA-01 `cargo clippy --workspace --all-targets -- -D warnings` e `cargo fmt --all --check` passam.
- [ ] CA-02 `cargo test -p mt-agents` passa com todos os testes listados em spec §11; cobertura de linhas ≥ 80% em `telemetry/`, `attention/`, `skills/` e `memory.rs` (`cargo llvm-cov -p mt-agents`).
- [ ] CA-03 `npm run lint`, `npm run typecheck`, `npm run typecheck:vue` e `npm run test` passam; `git diff --exit-code src/types/bindings.ts` após `npm run bindings` retorna 0.
- [ ] CA-04 Nenhum `unsafe`; nenhum `std::thread::sleep`/IO bloqueante em tarefa `tokio` (revisão + `clippy::await_holding_lock` ativo).

### 5.2. Registro e detecção
- [ ] CA-05 `agent_list` retorna exatamente 4 descritores (`claude-code`, `codex`, `agy`, `opencode`) com `displayName`, ícone Iconify, cor semântica e capacidades efetivas por versão/fonte; `claude` é nome do executável, não ID do harness.
- [ ] CA-06 Teste com `PATH` temporário contendo scripts falsos (`claude` imprime `2.1.0 (Claude Code)`) → `installed = true`, `version = "2.1.0"`; binário ausente → `installed = false` e `reason` pt-BR; binário que trava → timeout em ≤ 3,5 s sem bloquear os demais (detecção paralela).
- [ ] CA-07 Override em `agents.toml` (`[claude] binary = "/opt/claude/bin/claude"`, `extra_args = ["--verbose"]`) é refletido em `agent_list` e no `LaunchSpec` gerado; TOML inválido gera `MtError` `agents.settings_invalid` sem derrubar o app (descritores embutidos permanecem).

### 5.3. Templates, atalhos e abertura
- [ ] CA-08 `agent_templates_ensure(project_id)` cria 4 templates `environment_kind='agent'` na primeira chamada e 0 na segunda (idempotente, verificado via contagem em `tab_templates`).
- [ ] CA-09 Clicar em "Claude Code" no `AgentLauncherMenu` abre uma aba cujo processo em foreground (via `/proc`) é o binário `claude`, no `cwd` da pasta do template/projeto.
- [ ] CA-10 `Ctrl+Alt+C/X/A/O` abrem a aba do agente correspondente mesmo com o foco dentro do xterm; com `Shift` abre o `AgentTabOpenDialog` já com isolamento marcado; agente não instalado mostra toast pt-BR "Agente não encontrado no PATH" e não cria aba.
- [ ] CA-11 Atalho configurado igual a outro atalho existente é rejeitado com `agents.shortcut_conflict`.

### 5.4. Isolamento em worktree
- [ ] CA-12 Abrir aba isolada em um repositório de teste cria `<repo>/.worktrees/wt-<slug>` na branch `agent/<agente>/<slug>` (ou `.max-code-worktrees/` conforme configuração), e o `cwd` do PTY é a worktree; `agent_tab_links` contém o vínculo e o indicador Git da Fase 08 mostra "worktree".
- [ ] CA-13 Se a raiz de worktrees não estiver ignorada (`git check-ignore`), o diálogo exibe aviso pt-BR antes de criar.
- [ ] CA-14 Após fechar a aba vinculada: worktree própria limpa → oferece "Manter", "Remover worktree" e, se branch mesclada, "Remover worktree e apagar branch"; alterações rastreadas, não rastreadas, ignoradas, submódulos, lock, uso compartilhado ou inspeção incompleta bloqueiam remoção nesta fase. Commits exclusivos bloqueiam apagar a branch. "Manter" preserva tudo; resolução permanece recuperável após reinício.
- [ ] CA-15 Worktree `Existing` é validada contra o mesmo repositório e vinculada sem criação; somente "Manter" disponível (`created_by_app = 0`), inclusive para o checkout principal.
- [ ] CA-16 O processo do agente é encerrado pela API da Fase 02 antes da remoção da worktree; nenhum processo órfão em `ps --ppid` após o fechamento.

### 5.5. Telemetria
- [ ] CA-17 Teste de integração: enviar via `mt-agent-relay statusline` o fixture `fixtures/claude/statusline.json` com o token da aba → em ≤ 1,5 s o evento `agent://telemetry` traz `model.display_name`, `context.percent` e (se presentes no fixture) `quota_5h`/`quota_weekly`.
- [ ] CA-18 Parsers de arquivos internos só produzem campos demonstrados por fixtures versionados e sessão correlacionada; formato desconhecido, associação ambígua e ausência de denominador resultam em `null`. Linhas corrompidas não são erro fatal; duas abas no mesmo cwd não compartilham medições por engano.
- [ ] CA-19 `AgentStatusLine.vue` renderiza "—" com tooltip "Indisponível para este agente" para campos `null` e marca dados com mais de 60 s como "desatualizado".
- [ ] CA-20 Emissão de `agent://telemetry` limitada a ≤ 1 evento/s por aba sob 50 atualizações/s (teste de throttle).
- [ ] CA-21 O statusLine original do usuário do Claude Code continua exibido dentro do TUI (encadeamento) — validação manual documentada.

### 5.6. Alerta de Permissão/Input
- [ ] CA-22 Fixtures ANSI versionados produzem `Suspected` com motivo/fonte; texto citado, BEL, silêncio e saída normal não produzem espera confirmada. Padrões sem fixture ficam desabilitados.
- [ ] CA-23 `Notification` Claude com `notification_type='permission_prompt'` da execução correta → `error`, `seen=false`, badge vermelho e notificação "Claude Code aguarda aprovação" conforme visibilidade. `auth_success`, `Stop` e Codex `agent-turn-complete` não geram esse alerta.
- [ ] CA-24 Digitar tecla, focar ou dispensar só reconhece visualização. Resolução nativa correlacionada limpa a atenção em ≤ 500 ms; `PreToolUse` sozinho não prova resolução. Saída do PTY e término real têm precedência; nenhuma duplicação de notificação por episódio.
- [ ] CA-25 Clique na ação da notificação foca a janela, seleciona o projeto e ativa a aba.

### 5.7. MaxSkills e memória contextual
- [ ] CA-26 `skills_scan` sobre um `MaxSkills` de teste lista as skills de `index.json` com nome, descrição pt-BR e relevância por projeto; sem `index.json`, a varredura de `SKILL.md` produz o mesmo conjunto.
- [ ] CA-27 `skills_plan` (dry-run) não altera o disco (hash de árvore idêntico antes/depois) e lista ações `create`/`update`/`unchanged`/`conflict`/`prune`.
- [ ] CA-28 Sync em modo `symlink` cria links absolutos por skill; em modo `copy` grava `.maxterminal-skill.json` com hash; re-executar é idempotente (0 ações).
- [ ] CA-29 Destino que é symlink para outro lugar (ex.: `~/.max-code/skills/x`) → `ForeignSymlink`; diretório sem manifesto → `Unmanaged`; cópia editada localmente → `LocallyModified`; nenhum destes é sobrescrito sem `Overwrite`, e `Overwrite` gera backup em `$XDG_STATE_HOME/maxterminal/skills-backup/`.
- [ ] CA-30 Criar/editar um `SKILL.md` na fonte dispara `skills://changed` em ≤ 2 s e, para alvos com `auto_sync`, a cópia é atualizada.
- [ ] CA-31 Após executar `npm run build` 3 vezes e `ls` 1 vez em um projeto, `memory_suggestions(project, 'np')` retorna `npm run build` em 1º lugar com `use_count = 3`; comandos com marcador de redação do sanitizador **não** aparecem; `memory_hide` remove da sugestão e persiste.
- [ ] CA-32 `memory_rebuild` reconstrói a tabela a partir de `command_blocks` com resultado idêntico ao incremental (teste de equivalência).
- [ ] CA-33 Replay duplicado de `CommandFinished` não aumenta contadores; esquecer continua respeitado após rebuild/reinício; comandos com aspas/espaços internos distintos não são fundidos.
- [ ] CA-34 Rotação/truncamento de JSONL, compactação, mudança de modelo, reconexão e nova sessão invalidam dados antigos; `null` explícito não é ignorado pelo merge. Tooltip distingue fonte estruturada, configuração e estimativa.
- [ ] CA-35 Sync em worktree usa destino dessa worktree; symlink ancestral externo não é seguido sem alvo explicitamente autorizado; dry-run vencido é rejeitado e prune preserva arquivos modificados. Fonte ausente mantém cópias existentes.

---

## 6. Riscos e Mitigação

| # | Risco | Impacto | Mitigação |
|---|-------|---------|-----------|
| R-01 | Formatos de statusLine/transcripts/sessões dos agentes mudam entre versões (APIs não estáveis). | Alto | Parsers tolerantes (`serde(default)`, campos opcionais), proveniência por campo, fixtures versionados por versão do agente, todas as suposições marcadas em spec §7.4 com teste de validação manual; falha de parser degrada para "—", nunca erro fatal. |
| R-02 | Adaptadores estruturados não implementados/validados para versões locais de `agy`/OpenCode. | Médio | Campos `null` e capacidades indisponíveis; OpenCode possui API própria, mas esta fase não presume endpoints/eventos de outra versão. Heurística opt-in explicitamente estimada. |
| R-03 | Falsos positivos do detector de atenção (palavras "Allow"/"Yes" em output comum). | Alto | Padrões versionados, silêncio/foreground como filtros, conjunto negativo de fixtures; heurística sempre `Suspected`, somente fonte nativa correlacionada confirma espera. |
| R-04 | Injeção de `--settings` no Claude sobrescreve statusLine do usuário. | Médio | Resolver configuração efetiva antes de encadear e preservar stdout; precedência/política desconhecida desabilita apenas a injeção conflitante. |
| R-05 | Socket do relay usado por processos locais maliciosos para forjar alertas. | Médio | Socket 0600 em `$XDG_RUNTIME_DIR`, token aleatório de 256 bits por aba via env, limite de 64 KiB por mensagem, rejeição de tokens revogados ao fechar a aba. |
| R-06 | Remoção acidental de worktree com trabalho do agente. | Alto | Inspeção completa e revalidada, nunca remover `Existing`, alterações/arquivos ignorados bloqueiam remoção; sem `--force`/`branch -D` nesta fase. |
| R-07 | Sync de skills sobrescreve skills gerenciadas pelo MaxCode (`~/.max-code/skills`) ou editadas manualmente. | Alto | Classificação de conflitos, política "nunca sobrescrever sem ação explícita", backup antes de `Overwrite`, *dry-run* padrão na UI. |
| R-08 | Diretórios-alvo que são symlinks compartilhados (ex.: `.claude/skills` da worktree apontando para o repositório principal). | Médio | Resolver `canonicalize` do diretório-alvo e avisar quando aponta para fora da pasta do projeto; deduplicar alvos pelo caminho canônico. |
| R-09 | Tailing de transcripts grandes degrada performance. | Médio | Leitura inicial apenas dos últimos 256 KiB, depois incremental por offset; watch por `notify` em vez de polling; uma tarefa por aba ativa. |
| R-10 | `PATH` do app GUI difere do shell do usuário (agentes em `~/.local/bin` não encontrados). | Médio | Captura do `PATH` via `$SHELL -lic` com timeout (reaproveitando a Fase 02 se exposto), diretórios extras e override por agente. |
| R-11 | Memória contextual expor segredos. | Alto | Fonte sanitizada e revalidada; excluir marcadores de redação; `memory_forget` remove a sugestão e mantém tombstone para impedir reconstrução. Histórico original é gerido pela Fase 03. |
| R-12 | Nomes dos contratos das Fases 02–12 divergirem dos esperados. | Médio | Spec §1.2 isola cada dependência atrás de um adaptador fino em `src/commands/` / `mt-agents::ports`; divergência exige ajuste só no adaptador. |
