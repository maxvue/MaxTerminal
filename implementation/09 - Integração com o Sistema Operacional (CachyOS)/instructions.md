# Fase 09 — Integração com o Sistema Operacional (CachyOS) (`instructions.md`)

> Fonte da verdade: [`../../AGENTS.md`](../../AGENTS.md) §3 ("Experiência no Sistema Operacional") e contratos
> compartilhados de [`../README.md`](../README.md) (§2.1–§2.6).
> Especificação técnica detalhada: [`./spec.md`](./spec.md).

---

## 1. Objetivo da Fase

Fazer o MaxTerminal se comportar como um aplicativo nativo de primeira classe no CachyOS (KDE Plasma 6 / Hyprland,
Wayland ou X11), implementando no crate `mt-os` e na camada de frontend correspondente:

1. **System Tray:** fechar a janela com abas `running`/`continuous` oculta o app na bandeja mantendo PTYs e servidores
   vivos; sem abas ativas, o app sai limpando todos os PTYs. Menu da bandeja com Mostrar/Ocultar, contadores agregados e
   "Sair" com confirmação que encerra tudo **sem processos órfãos**. O ícone da bandeja reflete o estado agregado.
2. **Atalho Global (Quake Mode / Quick Toggle):** atalho configurável e persistido que mostra/oculta/foca a janela em
   qualquer workspace, com estratégia explícita para as limitações do Wayland (portal `GlobalShortcuts` e comando CLI
   `maxterminal --toggle` via single-instance) e modo drop-down opcional.
3. **Links Clicáveis para o Editor:** caminhos `caminho:linha[:coluna]` (relativos ao cwd do painel emissor ou absolutos) no
   terminal viram links validados no backend e abertos no VS Code (`code --goto`) ou MaxCode (`vscode://file/...`),
   com editor configurável.
4. **Infraestrutura de Notificações Nativas:** API única `notify_user(kind, title, body, tab_id)` (Rust e IPC), com clique
   na notificação focando a aba de origem. Consumidores: hook interno desta fase para "fim de comando longo com janela sem
   foco" (sobre eventos da Fase 05) e Fase 13 (agente aguardando input).
5. **Autostart opcional** (`~/.config/autostart`) com opção de iniciar minimizado na bandeja.
6. **Frontend:** `useWindowStore`, `useOsSettingsStore`, composable de link provider do xterm e a tela
    **Preferências > Sistema**.
7. **Ativação de instância única e deep links:** infraestrutura de recepção de `maxterminal://`, fila limitada e registro
   de handlers Rust. A Fase 11 fornece as rotas de negócio e autenticação da ponte; a Fase 09 não depende dela.

---

## 2. Escopo

### 2.1. Incluído

| # | Item | Detalhe |
|---|------|---------|
| I-01 | Crate `mt-os` | Módulos `settings`, `environment`, `tray`, `lifecycle`, `quake`, `shortcut` (backends plugin/portal/CLI), `cli`, `activation`, `links`, `editor`, `notify`, `attention`, `autostart`, `error` (ver spec §2). |
| I-02 | Persistência de settings de SO | Migration própria `0005_09_os_settings.sql` (tabela `os_settings`, linha única JSON versionada) + repositório `OsSettingsRepo` no `mt-db` (somente arquivo desta fase). |
| I-03 | Ciclo de vida da janela | Interceptação de `WindowEvent::CloseRequested` e `RunEvent::ExitRequested`; política `CloseBehavior` (`auto` / `always_tray` / `always_quit`); decisão pura `decide_close()` testável. |
| I-04 | Encerramento sem órfãos | `ShutdownCoordinator` serializa saída, bloqueia novos spawns, chama `WorkspaceManager::shutdown` (07), reconcilia PTYs restantes (02), drena gravações/sessões (03/05/07) e só então autoriza `app.exit(0)`; sobreviventes impedem saída interativa bem-sucedida. |
| I-05 | Tray | `TrayIconBuilder` (Tauri v2 `tray-icon`), menu dinâmico com contadores (Executando, Execução Contínua, Finalizado, Com Erro, Aguardando intervenção), Mostrar/Ocultar, Preferências, Sair; 3 variantes de ícone (`idle`, `active`, `attention`); atualização com debounce. |
| I-06 | Detecção de tray disponível | Verificação do `org.kde.StatusNotifierWatcher` no D-Bus de sessão; se ausente, "fechar para bandeja" degrada para minimizar (nunca deixa o app invisível sem forma de retorno). |
| I-07 | Confirmação de saída | Diálogo nativo (`tauri-plugin-dialog`) quando há abas `running`/`continuous`, funcional mesmo com a janela oculta. |
| I-08 | Atalho global | `ShortcutManager` com cadeia de backends: `tauri-plugin-global-shortcut` (X11), portal `org.freedesktop.portal.GlobalShortcuts` via `ashpd` (Wayland), e `CliOnly` (instruções para o compositor). Status exposto à UI. |
| I-09 | Single-instance + CLI + deep links | `tauri-plugin-single-instance` com feature `deep-link`, **primeiro** plugin; `tauri-plugin-deep-link` registra `maxterminal`; flags `--toggle`, `--show`, `--hide`, `--start-hidden`; recepção única e fila até os handlers e workspace estarem prontos. |
| I-10 | Quake Mode | Decisão pura `decide_toggle(visible, focused, minimized)`; modo `normal` e `drop_down` (topo do monitor sob o cursor, largura total, altura %, always-on-top, ocultar ao perder foco opcional) com degradação documentada no Wayland. |
| I-11 | Links para editor | Parser TS de candidatos (`caminho:linha[:coluna]`), link provider xterm da Fase 04, validação em lote via `os_link_resolve` com `LinkContext` (aba/painel/PTY/geração de cwd); abertura via `os_link_open`, recusando contexto obsoleto. |
| I-12 | Editores | `EditorKind`: `vs_code`, `vs_code_insiders`, `max_code` (URI `vscode://file/`), `custom` (argv com placeholders `{file}`, `{line}`, `{column}`); detecção de binário/handler MIME; trait `EditorOpener` como ponto de extensão (Fase 11). |
| I-13 | Notificações | `Notifier` sobre `notify-rust` (D-Bus `org.freedesktop.Notifications`, ação `default` = clique) com fallback `tauri-plugin-notification` (sem clique); dedupe, rate limit, sanitização do corpo; evento `os://focus-tab` no clique. |
| I-14 | Hook de comando longo | Listener sobre o evento de fim de comando da Fase 05: se duração ≥ limiar configurado e janela sem foco → `notify_user(CommandFinished|CommandFailed, …)`. Opcional, desligável. |
| I-15 | Registro de atenção | `AttentionRegistry` (`set/clear` por `TabId` com `AttentionReason::AwaitingInput`) que alimenta ícone/contador da bandeja — ponto de extensão para a Fase 13. |
| I-16 | Autostart | `tauri-plugin-autostart` (arquivo `.desktop` em `~/.config/autostart/` com `--start-hidden`), sincronizado com `OsSettings.autostart`. |
| I-17 | Frontend | `src/ipc/os.ts`, `src/stores/window.ts` (`useWindowStore`), `src/stores/osSettings.ts` (`useOsSettingsStore`), composables `useEditorLinks`, `useOsFocusTab`, `useShortcutRecorder`; componentes em `src/components/preferences/` e `src/components/os/`. |
| I-18 | Preferências > Sistema | Seção com: comportamento ao fechar, atalho global (gravador + status do backend + ajuda Wayland), modo drop-down, editor, notificações, autostart. Cria o contêiner de Preferências com registro de seções se ainda não existir. |
| I-19 | Capabilities | `src-tauri/capabilities/os.json` e permissões dos comandos próprios; somente webview local `main`. Uso Rust de plugins não exige liberar suas APIs JS. |

### 2.2. Fora de escopo (e fase responsável)

| Item | Fase |
|------|------|
| Encerramento de processos por grupo, SIGTERM/SIGKILL, leitura de `/proc` (apenas **consumido** aqui) | 02 |
| Sanitizador de segredos e conexão SQLite (apenas consumidos) | 03 |
| Instância xterm, addons e registro de link providers (apenas consumido) | 04 |
| Máquina de estados `TabStatus`, cwd por painel, evento de fim de comando (apenas consumidos) | 05 |
| Seleção de projeto/aba e marcação `seen` ao visualizar (apenas consumidas no foco) | 06 / 07 |
| Temas da bandeja/ícones por tema e importação Tabby | 10 |
| Rotas de negócio de `maxterminal://`, autenticação/loopback com MaxCode e abertura **pela ponte** (registra `ActivationHandler`/`EditorOpener` na infraestrutura desta fase) | 11 |
| Detecção de "agente aguardando input/aprovação" e chamada de `notify_user(AgentAwaitingInput, …)` | 13 |
| Notificações relacionadas a Issues/"Corrigir todos os Bugs" | 16 |
| Empacotamento (PKGBUILD/AppImage), instalação do binário em `/usr/bin/maxterminal`, `.desktop` do sistema, testes E2E | 17 |

---

## 3. Pré-requisitos

### 3.1. Fases
| Fase | Contrato consumido |
|------|--------------------|
| 01 | `mt-core` (`MtError`, `ErrorCode` por constantes de domínio, ids, `AppPaths`), `tauri-specta`, `state.rs`, `capabilities/` |
| 02 | API global de encerramento de PTYs (`shutdown_all` com prazo), PID do shell por `PtyId` |
| 03 | Pool/conexão SQLite, runner de migrations, `mt_db::sanitizer::sanitize()`, checkpoint WAL |
| 04 | Ponto de extensão de link providers do xterm (ou acesso à instância `Terminal`) |
| 05 | `TabStateRegistry::list`, `TabStateSnapshot.panes` e `badge_bucket`, `ShellService::subscribe`, `CommandEvent::{Finished,CwdChanged}`, `shell://cwd-changed`, `CommandFinished.exit_code: Option<i32>` |
| 07 | `WorkspaceManager::{shutdown,register_observer}`, `WorkspaceObserver`, vínculo aba/painel/PTY, `useTabsStore.focus(projectId, tabId, paneId?)` |

> Nota: a Fase 08 não é dependência desta fase.

Dependências diretas do DAG: **05 e 07**. As Fases 01–04 e 06 são bases transitivas. Contadores visuais usam o bucket
agregado da Fase 05; a decisão de manter processos vivos inspeciona **todos os painéis**, pois um erro não visto
pode ocultar `running`/`continuous` de outro split no status agregado. Adaptadores desta fase ficam no binário;
nenhum contrato de Fase 11/13 é importado por `mt-os` ou `mt-db`.

### 3.2. Ferramentas e pacotes de sistema (CachyOS)
| Item | Verificação |
|------|-------------|
| `libayatana-appindicator` (tray) | `pacman -Q libayatana-appindicator` |
| Servidor de notificações (Plasma, `mako`, `dunst`, `swaync`) | `busctl --user list \| grep org.freedesktop.Notifications` |
| `xdg-desktop-portal` + backend (`-kde` ou `-hyprland`) | `busctl --user introspect org.freedesktop.portal.Desktop /org/freedesktop/portal/desktop \| grep GlobalShortcuts` |
| VS Code (`code`) e/ou extensão MaxCode | `command -v code` |
| `xdg-utils` (`xdg-open` para URIs `vscode://`) | `command -v xdg-open` |
| Crates: plugins Tauri citados na spec, incluindo `tauri-plugin-deep-link`, `ashpd`, `notify-rust`, `zbus` | `cargo tree` e compatibilidade com o toolchain fixado na Fase 01 |
| Frontend: `@tauri-apps/api` e bindings gerados; pacotes JS dos plugins são dispensáveis quando não usados | `npm ls @tauri-apps/api` |

---

## 4. Entregáveis

### 4.1. Backend (`src-tauri/`)
- `crates/mt-os/Cargo.toml`
- `crates/mt-os/src/lib.rs` — `OsIntegration` (fachada), `OsDeps`, re-exports
- `crates/mt-os/src/error.rs` — constantes `os.*` de `ErrorCode`
- `crates/mt-os/src/settings.rs` — `OsSettings`, `OsSettingsPatch`, validação, defaults
- `crates/mt-os/src/environment.rs` — `OsEnvironment`, `DisplayServer`, `Compositor`, detecção por env + D-Bus
- `crates/mt-os/src/status.rs` — `TabStatusProvider` (trait), `StatusCounters`, agregação
- `crates/mt-os/src/attention.rs` — `AttentionRegistry`, `AttentionReason`
- `crates/mt-os/src/tray.rs` — `TrayController`, `TrayVisualState`, montagem do menu
- `crates/mt-os/src/lifecycle.rs` — `CloseBehavior`, `CloseDecision`, `decide_close()`, `ShutdownCoordinator`, trait `ShutdownHooks`
- `crates/mt-os/src/quake.rs` — `QuakeController`, `ToggleDecision`, `decide_toggle()`, geometria drop-down
- `crates/mt-os/src/shortcut/{mod.rs,plugin_backend.rs,portal_backend.rs,cli_backend.rs,accelerator.rs}`
- `crates/mt-os/src/cli.rs` — `CliAction`, `parse_cli_args()`, handler do single-instance
- `crates/mt-os/src/activation.rs` — `ActivationRouter`, `ActivationEnvelope`, `ActivationHandler`, fila/dedupe/readiness
- `crates/mt-os/src/links.rs` — `LinkCandidate`, `ResolvedLink`, `LinkResolver`, trait `TabCwdProvider`
- `crates/mt-os/src/editor.rs` — `EditorKind`, `EditorLauncher`, trait `EditorOpener`, `build_editor_invocation()`
- `crates/mt-os/src/notify.rs` — `Notifier`, `NotificationKind`, `NotifyRequest`, `RateLimiter`
- `crates/mt-os/src/long_command.rs` — hook de fim de comando longo
- `crates/mt-os/src/autostart.rs` — `AutostartSync`
- `crates/mt-os/icons/tray/{idle,active,attention}.png` (22/32 px, embutidos com `include_bytes!`)
- `crates/mt-os/tests/{close_decision.rs,toggle.rs,links.rs,editor_argv.rs,rate_limit.rs,cli.rs,environment.rs,settings.rs}`
- `crates/mt-db/migrations/0005_09_os_settings.sql` + `crates/mt-db/src/repos/os_settings.rs`
- `src/commands/os.rs` (binário) — 17 comandos IPC (spec §3) registrados no builder `tauri-specta`
- `src/adapters/os.rs` — integração com Fases 02/03/05/07, sem dependência reversa
- `src/lib.rs` — registro dos plugins (single-instance primeiro), `setup` de `OsIntegration`, handlers de janela/run-event
- `capabilities/os.json`, `permissions/os.toml`, manifesto de comandos próprios em `build.rs`, configuração estática do scheme

### 4.2. Frontend (`src/`)
- `ipc/os.ts`
- `stores/window.ts` (`useWindowStore`), `stores/osSettings.ts` (`useOsSettingsStore`)
- `composables/useEditorLinks.ts`, `composables/editorLinks/parseLinkCandidates.ts`, `composables/editorLinks/linkCache.ts`
- `composables/useOsFocusTab.ts`, `composables/useShortcutRecorder.ts`
- `components/preferences/PreferencesDialog.vue`, `components/preferences/registry.ts` (se não existir)
- `components/preferences/system/PreferencesSystemSection.vue`
- `components/preferences/system/ShortcutRecorder.vue`, `WaylandShortcutHelp.vue`, `EditorSelector.vue`, `CloseBehaviorSelector.vue`
- `components/os/ShortcutStatusBadge.vue`
- Testes: `tests/composables/editorLinks/parseLinkCandidates.spec.ts`, `tests/composables/useEditorLinks.spec.ts`,
  `tests/stores/window.spec.ts`, `tests/stores/osSettings.spec.ts`, `tests/composables/useShortcutRecorder.spec.ts`,
  `tests/components/preferences/PreferencesSystemSection.spec.ts`

---

## 5. Critérios de Aceite

Comandos executados na raiz da worktree; todos devem retornar **exit 0**, salvo indicação.

### 5.1. Qualidade e testes automatizados
- [ ] CA-01 `cd src-tauri && cargo clippy -p mt-os --all-targets -- -D warnings` passa.
- [ ] CA-02 `cd src-tauri && cargo test -p mt-os` passa, cobrindo no mínimo: tabela-verdade completa de `decide_close()`
  (3 `CloseBehavior` × com/sem abas ativas × tray disponível/indisponível × `force_quit`); tabela de `decide_toggle()`;
  ≥ 25 casos de resolução de links (relativo, absoluto, `~`, `./`, `../`, inexistente, diretório, symlink para fora do
  cwd, linha/coluna 0, overflow numérico); `build_editor_invocation()` para os 4 editores (inclusive caminho com espaços, `#`,
  `%` e acentos); `RateLimiter` (dedupe 10 s, máximo 5/30 s); `parse_cli_args()` (aceita só a whitelist); detecção de
  ambiente com mapas de env simulados (KDE Wayland, Hyprland, GNOME, X11, XWayland).
- [ ] CA-03 `cargo test -p mt-db os_settings` passa: migration aplica, linha default é criada, JSON inválido é rejeitado
  pelo `CHECK (json_valid(...))`, roundtrip de `OsSettings` com campos ausentes usa defaults (compatibilidade futura).
- [ ] CA-04 `npm run typecheck && npm run typecheck:vue && npm run lint` passam.
- [ ] CA-05 `npx --no-install vitest run tests/composables/editorLinks tests/stores/window.spec.ts tests/stores/osSettings.spec.ts tests/composables/useShortcutRecorder.spec.ts tests/components/preferences` passa, com cobertura ≥ 90% de linhas em `parseLinkCandidates.ts`.
- [ ] CA-06 `npm run bindings && git diff --exit-code src/types/bindings.ts` retorna 0 e `bindings.ts` exporta os 17
  comandos `os*` e os tipos `OsSettings`, `OsEnvironment`, `ShortcutStatus`, `ResolvedLink`, `NotificationKind`.

### 5.2. System Tray e ciclo de vida (manual, KDE Plasma 6 Wayland e Hyprland)
- [ ] CA-07 Com uma aba executando `sleep 600` (estado `running`), fechar a janela (botão X ou `Ctrl+Q` do compositor)
  oculta a janela; o ícone aparece na bandeja; `pgrep -x sleep` continua retornando o PID.
- [ ] CA-08 Com uma aba `npm run dev` (estado `continuous`), após ocultar para a bandeja, `curl -sf http://127.0.0.1:<porta>` continua respondendo.
- [ ] CA-09 Sem painéis ativos nem atividade supervisionada pendente, fechar encerra shells ociosos, arquiva sessões e
  sai após o shutdown. Verificar PIDs/start-times previamente capturados, inclusive descendentes reparentados, em até
  5 s no cenário controlado; `pgrep -P` isolado não comprova ausência de órfãos.
- [ ] CA-10 Menu da bandeja exibe "Mostrar" quando oculto e "Ocultar" quando visível; os contadores "Executando: N",
  "Execução contínua: N", "Finalizado: N", "Com erro: N", "Aguardando intervenção: N" batem com os badges da Fase 06 em
  até 500 ms após cada mudança de estado.
- [ ] CA-11 "Sair" com abas ativas abre diálogo nativo "Sair e encerrar os processos de N abas?" mesmo com a janela oculta;
  "Cancelar" mantém tudo vivo; "Sair e encerrar tudo" encerra o app e, após 5 s, `pgrep -x sleep` e `ss -ltnp | grep <porta>` retornam vazio (nenhum órfão, inclusive netos como `node` filho de `npm`).
- [ ] CA-12 Ícone da bandeja: `idle` sem atividade; `active` com ≥ 1 `running`/`continuous`; `attention` com ≥ 1 `error`
  não visto ou ≥ 1 aba com atenção registrada; erro visto perde destaque, intervenção permanece até ser resolvida.
- [ ] CA-13 Sem `StatusNotifierWatcher` no D-Bus (ex.: Hyprland sem waybar tray), fechar a janela com abas ativas
  **minimiza** em vez de ocultar e a tela de Preferências mostra o aviso "Bandeja do sistema indisponível".
- [ ] CA-14 `SIGTERM` enviado ao processo do app (`kill <pid>`) dispara o mesmo `ShutdownCoordinator` (sem órfãos).

### 5.3. Atalho global / Quake Mode
- [ ] CA-15 Em sessão X11, o atalho padrão `Ctrl+Alt+Backquote` (configurável) alterna: oculto → visível e focado; visível
  sem foco → focado; visível e focado → oculto. Validar a política do gerenciador de janelas para workspace/foco; não
  pressupor que `set_focus` move janelas entre desktops.
- [ ] CA-16 Quando o portal efetivamente disponibilizar e conceder `GlobalShortcuts`, backend = `portal`, sessão
  permanece viva e o trigger efetivo exibido é o retornado pelo compositor. Registrar versão e comportamento observado
  no KDE/Hyprland; presença do pacote do portal, isoladamente, não comprova suporte nem concessão.
- [ ] CA-17 Sem portal disponível, o status é `cli_only` e Preferências exibe o snippet pronto para copiar
  (`bind = SUPER, grave, exec, maxterminal --toggle` / atalho personalizado no KDE).
- [ ] CA-18 `maxterminal --toggle` executado com o app aberto **não** cria segunda instância (`pgrep -c -x maxterminal` = 1)
  e alterna a janela em < 150 ms; com o app fechado, inicia o app visível.
- [ ] CA-19 Alterar o atalho em Preferências persiste no SQLite (`SELECT value_json FROM os_settings` reflete) e é
  re-registrado sem reiniciar; acelerador inválido ou já em uso retorna erro `os.shortcut_invalid`/`os.shortcut_conflict`
  exibido em pt-BR, mantendo o atalho anterior ativo.
- [ ] CA-20 Modo drop-down em X11: janela aparece no topo do monitor sob o cursor, largura total, altura igual ao % configurado,
  always-on-top; com "ocultar ao perder foco" ligado, clicar fora oculta. Em Wayland, Preferências informa que posição e
  always-on-top dependem do compositor (sem erro em runtime).

### 5.4. Links clicáveis
- [ ] CA-21 Em uma aba com cwd = projeto Laravel, `echo app/Models/User.php:42` renderiza sublinhado ao passar o mouse
  (com `Ctrl` pressionado, conforme política da Fase 04) e o clique executa `code --goto <abs>/app/Models/User.php:42:1`
  (verificável com `editor.kind = custom` apontando para um script que registra o argv em arquivo).
- [ ] CA-22 `echo /etc/hostname:1:3`, `echo ./src/main.ts:10:5` e `echo ~/.zshrc:3` funcionam; `echo nao/existe.php:10`
  **não** vira link; `echo src/` (diretório) não vira link.
- [ ] CA-23 Após `cd subdir` na aba, caminhos relativos passam a ser resolvidos contra o novo cwd (cwd vindo da Fase 05).
- [ ] CA-24 Com `editor.kind = max_code`, o clique abre URI `vscode://file/...:42:1` corretamente codificada pelo opener do sistema.
- [ ] CA-25 Uma linha com 200 candidatos dispara no máximo 4 chamadas `os_link_resolve` (lote ≤ 64)
  e hover repetido na mesma linha em 5 s usa o cache (0 chamadas — verificado em teste com `mockIPC`).

### 5.5. Notificações
- [ ] CA-26 `notify_user(CommandFailed, 'Comando falhou', 'npm test (exit 1)', Some(tab))` exibe notificação nativa com
  ícone do app; clicar mostra a janela, seleciona o projeto e ativa a aba (`os://focus-tab` recebido pelo frontend).
- [ ] CA-27 Com a janela sem foco e limiar = 10 s, `sleep 12; false` gera notificação "Comando falhou"; `sleep 2; false`
  não gera; com a janela focada não gera (`only_when_unfocused = true`).
- [ ] CA-28 Corpo contendo `Authorization: Bearer abc123` chega ao servidor de notificações mascarado (verificado com
  `dbus-monitor --session "interface='org.freedesktop.Notifications'"`).
- [ ] CA-29 20 chamadas idênticas em 2 s produzem 1 notificação (dedupe/`replaces_id`); 20 chamadas distintas produzem ≤ 5.

### 5.6. Autostart
- [ ] CA-30 Ligar "Iniciar com o sistema" cria `~/.config/autostart/maxterminal.desktop` contendo `Exec=… --start-hidden`;
  desligar remove o arquivo. Após login, o app inicia oculto na bandeja e restaura as abas "Adicionar ao Workspace" (Fase 07).

### 5.7. Convenções
- [ ] CA-31 Todos os textos de menu da bandeja, diálogos, notificações e Preferências em pt-BR; identificadores em inglês.
- [ ] CA-32 Nenhum `std::process::Command` usa shell (`sh -c`) para abrir editor ou URI; argv sempre em vetor.

### 5.8. Integração, concorrência e recuperação
- [ ] CA-33 Split com erro não visto e servidor em outro painel: fechar mantém todos os PTYs, leitura de output,
  captura sanitizada e supervisão; reabrir não cria nova sessão nem reexecuta Comando Inicial.
- [ ] CA-34 Duas solicitações simultâneas de saída produzem uma confirmação e um shutdown. `ExitRequested` com código
  não pode pular a limpeza. Falha com sobreviventes mantém supervisão e sinaliza erro; não reporta sucesso.
- [ ] CA-35 Remover o serviço de tray enquanto oculto restaura uma janela acessível; minimizar não suportado mantém
  a janela visível. `hide_on_blur` não reage a diálogos, seletor de portal nem Preferências pertencentes ao app.
- [ ] CA-36 URI `maxterminal://` com instância fechada, aberta e inicializando chega uma vez ao handler fake da Fase 09.
  Sem handler correspondente, retorna indisponibilidade; não executa comandos nem abre projetos por conta própria.
- [ ] CA-37 Permissões JS diretas de opener/atalho/autostart são negadas; comandos `os_*` funcionam apenas em `main`
  local. Teste de capability inclui outra webview, origem remota e manifesto de comandos próprios.
- [ ] CA-38 Dois painéis em pastas distintas resolvem o mesmo caminho relativo corretamente. Mudança de cwd, troca de
  PTY ou fechamento durante hover/clique invalida resultado antigo; Unicode, espaços e wrap têm range xterm correto.
- [ ] CA-39 Testar notificação sem servidor, sem suporte a ações, aba removida e tempestade de eventos de agente;
  fila de ações é limitada/cancelável e nenhuma thread cresce indefinidamente.
- [ ] CA-40 Falha de persistência após alterar atalho/autostart restaura estado anterior ou informa estado degradado
  reconciliado. Inicialização minimizada não é anulada pelo primeiro render do frontend.

---

## 6. Riscos e Mitigação

| # | Risco | Impacto | Mitigação |
|---|-------|---------|-----------|
| R-01 | Atalho registrado via X11/XWayland não cobre toda sessão Wayland. | Alto | Detectar suporte e concessão reais do portal; fallback `cli_only` com `--toggle`, sem prometer suporte por versão do desktop. |
| R-02 | Compositor limita posicionamento, always-on-top e foco. | Médio | Drop-down em melhor esforço, recuperar janela normal; token de ativação só quando disponível e suportado pela API usada; regras de compositor validadas na versão instalada. |
| R-03 | Ocultar para bandeja quando não há tray (Hyprland sem waybar, GNOME sem extensão AppIndicator) deixa o app inacessível. | Alto | Detecção de `StatusNotifierWatcher`; degradação para minimizar; `maxterminal --show` sempre disponível. |
| R-04 | Clique em ícone de tray não gera evento no Linux (libappindicator). | Baixo | Todas as ações via menu; "Mostrar/Ocultar" como primeiro item. |
| R-05 | `tauri-plugin-notification` não entrega clique no desktop Linux. | Médio | D-Bus com ação `default` e stream compartilhado cancelável; fallback sem clique registrado no status, sem thread ilimitada por mensagem. |
| R-06 | Processos órfãos ao sair (netos, daemons que mudam de grupo). | Alto | Encerramento delegado à Fase 02 (process group + varredura `/proc` de descendentes); `ShutdownCoordinator` aguarda `ShutdownReport` e loga sobreviventes; handler de `SIGTERM`/`SIGINT`; CA-11/CA-14. |
| R-07 | Links: falsos positivos (versões `1.2.3:4`, horários `10:30`, URLs) e resolução ambígua. | Médio | Scanner de candidatos com `/` ou extensão, exclusão de URLs, canonicalização e limite de tamanho; arquivos fora do cwd são permitidos mediante clique. |
| R-08 | Injeção de argumentos via editor customizado ou nome de arquivo iniciado com `-`. | Alto | argv em vetor, caminho absoluto; placeholders somente dentro de um elemento, sem tokenização posterior; binário validado no `PATH`. |
| R-09 | Vazamento de segredos em notificações (corpo contém comando). | Médio | `sanitize()` da Fase 03 + truncamento em 200 caracteres antes de enviar ao D-Bus. |
| R-10 | Tempestade de notificações/atualizações de tray em rajadas de mudança de estado. | Médio | Debounce de 250 ms no tray, rate limiter de notificações, coalescência por `(tab_id, kind)`. |
| R-11 | Single-instance via D-Bus falhar em sandbox/AppImage. | Baixo | Plugin usa D-Bus de sessão no Linux; teste CA-18 incluído na matriz da Fase 17 para AppImage. |
| R-12 | Contratos das Fases 04/05 com nomes diferentes dos assumidos. | Médio | `mt-os` depende apenas de traits próprias (`TabStatusProvider`, `TabCwdProvider`, `ShutdownHooks`) implementadas por adaptadores finos no crate binário. |

## 7. Sequência de Implementação e Contratos de Saída

1. Fixar os adaptadores 05/07, DTOs e settings; gerar bindings e testar decisões puras com splits e estados desconhecidos.
2. Integrar lifecycle e barreira de saída antes de ligar tray/atalhos; esconder janela nunca destrói workspace/PTY.
3. Registrar single-instance/deep-link e handshake de prontidão; integrar Quake/portal com degradação explícita.
4. Implementar links por painel, editor e preferências; notificações/atenção e autostart usam a mesma fachada Rust.
5. Executar verificações automatizadas da implementação e matriz manual com versões/pacotes/resultados registrados.

Entregar para a **Fase 11**: `ActivationRouter::register_handler`, envelope validado de transporte, `EditorOpener` e
`show_and_focus`; registrar handlers no binário e manter autenticação, dedupe de negócio e loopback na fase consumidora.
Entregar para a **Fase 13**: `Notifier::notify_user`, `AttentionRegistry::{set,clear}` e foco por aba/painel; detecção
de agentes e seu estado funcional continuam na 13. Para a **Fase 17**: scheme estático, identidade desktop/ícone,
requisitos de D-Bus/libayatana, registro MIME e roteiro de smoke tests de pacote/AppImage.
