# Fase 11 — Integração Bidirecional com MaxCode (`instructions.md`)

> Fonte da verdade: [`../../AGENTS.md`](../../AGENTS.md) §6 e contratos compartilhados de [`../README.md`](../README.md) (§2.1–§2.6).
> Especificação técnica detalhada: [`./spec.md`](./spec.md).

**Status dos contratos externos:** o AGENTS.md fixa os schemes, o loopback, a sincronização e o Plan Mode,
mas não fornece a API da extensão MaxCode. Rotas, porta, descoberta, handshake e payloads abaixo são
**propostas locais v1**, implementadas atrás de `MaxCodeAdapter` e condicionadas à validação com a versão
real da extensão. Mock aprovado não comprova interoperabilidade. Registrar versão testada, capabilities e
fixtures de troca antes de declarar integração real concluída; sem suporte retornar `bridge.unsupported_capability`.

---

## 1. Objetivo da Fase

Estabelecer o canal de comunicação **bidirecional, autenticado e restrito ao loopback** entre o MaxTerminal e a
extensão VS Code **MaxCode**, cobrindo integralmente o AGENTS.md §6:

1. **Servidor loopback** (`127.0.0.1`) HTTP + WebSocket no crate `mt-bridge`, com porta fixa configurável e
   fallback, arquivo de descoberta com token compartilhado e protocolo de mensagens JSON versionado.
2. **URI schemes**: o MaxTerminal passa a responder a `maxterminal://...` (acordando o app se estiver fechado e
   encaminhando para a instância ativa) e passa a disparar `vscode://johnattas.maxcode/...` para acordar o MaxCode
   quando não houver conexão ativa.
3. **Sincronização de Projeto Ativo** em tempo real, nos dois sentidos, com match por caminho canônico
    (inclusive worktrees `.worktrees/` e `.max-code-worktrees/`) e prevenção de loop por `origin`/`correlationId` e deduplicação.
4. **Botão "Inspecionar"** em cada bloco de comando que terminou com `exit != 0`, abrindo uma nova conversa em
   **Plan Mode** no MaxCode com o prompt exato:
    `Ao executar o comando "${comando}" tive o seguinte erro: "${erro}"` (sanitização/truncamento apenas nos valores).
5. **API genérica reutilizável** `maxcode_open_plan(prompt, project_path, source)` consumida pela Fase 14.
6. **Protocolo proposto de issues e ciclo delegado**, entregue em [`11/06`](./06%20-%20Protocolo%20de%20Issues%20e%20Ciclo%20de%20Bugs%20MaxCode/spec.md): `issue.command`, `bugs.cycle.{preview,start,status,progress}`, capabilities `issues_protocol_v1`/`bugs_cycle_v1` e URI de issues condicionados à validação da extensão. A Fase 16 somente consome por `MaxCodePort`.

Ao final da fase, abrir uma pasta no VS Code com o MaxCode foca o projeto correspondente no MaxTerminal (e
vice-versa), e um comando com erro pode ser enviado ao MaxCode com um clique — com ou sem a conexão WebSocket
estabelecida: a URI somente acorda o destino; o conteúdo aguarda um canal autenticado. URI nunca contém
token, prompt, comando, erro ou segredo. Timeout de wake-up não é confirmação de entrega.

---

## 2. Escopo

### 2.1. Incluído

| # | Item | Detalhe |
|---|------|---------|
| I-01 | Crate `mt-bridge` | Módulos `config`, `discovery`, `auth`, `protocol`, `server` (axum + tokio), `session`, `client`, `dedup`, `uri`, `paths`, `sync`, `prompt`, `error`. Sem dependência de Tauri (testável isolado). |
| I-02 | Servidor HTTP + WS | Bind **estrito** em `127.0.0.1` (nunca `0.0.0.0`/`::`); porta padrão `47615`, configurável; fallback sequencial `47616..=47624` e, por fim, porta efêmera (`:0`). Rotas `GET /v1/health`, `POST /v1/messages`, `GET /v1/ws`. |
| I-03 | Arquivo de descoberta | `$XDG_RUNTIME_DIR/maxterminal/bridge.json` (fallback `$XDG_STATE_HOME/maxterminal/run/bridge.json`), diretório `0700`, arquivo `0600`, escrita atômica; validar UID, identidade da instância/processo e ausência de symlinks, não apenas PID. |
| I-04 | Autenticação | Token aleatório de 256 bits (base64url) regenerado a cada inicialização; `Authorization: Bearer <token>` comparado em tempo constante; validação de `Origin` (allowlist) e de `Host` (anti DNS-rebinding). |
| I-05 | Protocolo v1 proposto | Envelope JSON versionado (`v`, `id`, `type`, `ts`, `reply_to`, `origin`, `correlationId`, `payload`), tipos e capabilities em spec §5; deduplicação de operações entre transportes. |
| I-06 | Cliente MaxCode | `MaxCodeAdapter`: WS → HTTP autenticado opcional (descoberta proposta, só se validada) → URI de wake-up + fila efêmera até reconexão autenticada. Nunca encaminhar payload pela URI nem repetir operação ambígua sem dedup negociada. |
| I-07 | Deep link `maxterminal://` | Scheme `maxterminal`, `.desktop` com MIME; rotas `maxterminal://project/open?path=` e `maxterminal://tab/open?project=&cwd=&requestId=`; tab é Shell neutro, sem execução inicial; single-instance (09), fila com readiness e dedup. |
| I-08 | URI `vscode://johnattas.maxcode/` | Proposta `/bridge/wake?requestId=<uuid>`; identificador público não autoriza ler payload. Rota deve ser confirmada pelo adaptador. Encoding RFC 3986, sem conteúdo sensível (spec §6). |
| I-09 | Sync de projeto ativo | `ActiveProjectSync`, caminhos/worktrees verificados, `origin` + `correlationId` propagados até a seleção; dedup idempotente e debounce de 250 ms. Janela temporal nunca substitui causalidade. |
| I-10 | Botão "Inspecionar" | `InspectButton.vue` no slot de ações da Fase 04; `CommandBlockDto.exitCode` conhecido e diferente de zero (05). Prompt Rust a partir de `ErrorContext`, com sanitização dos valores, remoção de ANSI e truncamento antes da interpolação. |
| I-11 | API genérica | IPC `maxcode_open_plan(prompt, project_path, source)` e Rust `MaxCodeClient::open_plan(prompt, project_path, source, pre_truncated)`; genérico passa `false`, Inspecionar propaga truncamento. |
| I-12 | Comandos IPC | `bridge_status`, `bridge_get_config`, `bridge_set_config`, `bridge_restart`, `bridge_regenerate_token`, `maxcode_open_plan`, `maxcode_open_project`, `maxcode_inspect_block`, `bridge_notify_active_project`. |
| I-13 | Eventos | `bridge://status-changed`, `bridge://peer-connected`, `bridge://peer-disconnected`, `bridge://project-focus-requested`, `bridge://tab-opened`, `bridge://delivery-failed`; snapshots revisionados e origem/correlação (spec §12). |
| I-14 | Frontend | `src/ipc/bridge.ts`, `src/stores/maxCodeBridge.ts` (`useMaxCodeBridgeStore`), `src/composables/useActiveProjectSync.ts`, `src/composables/useMaxCodeInspect.ts`, `src/components/bridge/{InspectButton.vue,BridgeStatusIndicator.vue,BridgeSettingsSection.vue}`. |
| I-15 | Configuração | `$XDG_CONFIG_HOME/maxterminal/bridge.toml` (porta, habilitado, sync ativo, allowlist extra de Origin). |
| I-16 | Protocolo de issues (11/06) | `MaxCodeClient::{send_issue_command,bugs_cycle_preview,bugs_cycle_start,bugs_cycle_status}`, assinatura Rust de progresso, capabilities e URI gated; sem dependência da 16, banco ou novos comandos Tauri. |
| I-17 | Adaptadores SO (11/04) | `ActivationHandler` no roteador único da 09 e `EditorOpener` via `EditorLauncher::set_bridge_opener`; abertura de arquivo pela ponte somente com suporte validado, fallback explícito da 09. |

### 2.2. Fora de escopo (e fase responsável)

| Item | Fase |
|------|------|
| Implementação do lado da extensão MaxCode (handler `vscode://johnattas.maxcode/...`, cliente WS, arquivo de descoberta próprio) | Repositório **MaxCode** (contrato externo documentado em spec §5 e §6) |
| Single-instance, tray, foco/exibição da janela, notificações nativas (apenas **consumidos** aqui) | 09 |
| Seleção de projeto, store de projetos, resolução de `ProjectId` por caminho (apenas consumidos) | 06 |
| Criação de abas a partir de template, `TabTemplate`, abas por `cwd` (apenas consumidos) | 07 |
| Detecção de exit code, `CommandBlock` com comando/output, OSC 133 (apenas consumidos) | 05 |
| Status de Git/worktree na barra da aba (UI) | 08 |
| Diagnóstico inline por IA ao lado do botão "Inspecionar" (o slot é compartilhado; o conteúdo é da 12) | 12 |
| Abertura de abas de Agente CLI vinculadas a worktree | 13 |
| Botão "Criar com MaxCode" do modal Artisan (consome `maxcode_open_plan`) | 14 |
| UI, domínio GitHub, persistência e acompanhamento das ações "Abrir / Analisar (`BUG PLAN`) / Corrigir (`BUG FIX`)" e "Corrigir todos os Bugs" | 16; consome protocolo de 11/06 via `MaxCodePort`, sem editar a 11; execução é do MaxCode, nunca mapear Fix para Plan |
| Registro do scheme no pacote final (PKGBUILD/AppImage, `update-desktop-database`) | 17 |
| Testes E2E com VS Code real | 17 |

---

## 3. Pré-requisitos

### 3.1. Fases

| Fase | Contrato consumido |
|------|--------------------|
| 01 | `mt-core` (`MtError`, `ErrorCode`, `MtResult`, ids, `AppPaths` XDG), `tauri-specta`, logging, `AppShell`/`AppTitleBar`. |
| 03 | `mt_db::sanitizer::sanitize()` (obrigatório no prompt do "Inspecionar" e em todo log de mensagem). |
| 04 | Slot/registro de ações do `CommandBlock` (ponto de extensão da camada de blocos). |
| 05 | `ShellService::error_context(block_id, 80) -> ErrorContext`, `command_block_get`, `CommandBlockDto`; `exit_code` Rust / `exitCode` TS e tail sanitizada do bloco correto. |
| 06 | `useProjectsStore` (projeto ativo, lista com `path`), comando de seleção de projeto e repositório `ProjectRepo::find_by_path`. |
| 07 | `WorkspaceManager::open_tab(TabOpenRequest { project_id, template_id, overrides, focus }) -> TabView`, resolução de template e vínculo sessão/aba/projeto. |
| 09 | Único plugin single-instance e `cli::handle_second_instance`, `QuakeController::show_and_focus`, `ShutdownHooks`; composição do encaminhamento no binário, sem presumir API `on_args`. |

> Os nomes exatos dos símbolos consumidos estão na spec §2.3; se divergirem da spec da fase dona, **vale a fase dona** e
> apenas o adaptador `src-tauri/src/bridge/adapters.rs` é ajustado.

### 3.2. Ferramentas

| Ferramenta | Uso | Verificação |
|------------|-----|-------------|
| `xdg-utils` | `xdg-open` (disparo de `vscode://`) e `xdg-mime` (registro do scheme) | `xdg-mime query default x-scheme-handler/maxterminal` |
| `desktop-file-utils` | `update-desktop-database`, `desktop-file-validate` | `desktop-file-validate maxterminal.desktop` |
| VS Code + extensão MaxCode | testes manuais de ponta a ponta | `code --list-extensions \| grep -i maxcode` |
| `websocat` e `curl` | verificação manual do servidor | `websocat --version` |
| Crates | `axum` 0.8 (feature `ws`), `tokio`, `tower-http` (limit/timeout), `tokio-tungstenite` (dev), `subtle`, `rand`, `base64`, `url`, `percent-encoding`, `strip-ansi-escapes`, `toml`, `tauri-plugin-deep-link` 2.x | `cargo tree -p mt-bridge` |

---

## 4. Entregáveis

### 4.1. Backend — crate `src-tauri/crates/mt-bridge/`
- `Cargo.toml`
- `src/lib.rs` — reexports e `BridgeHandle`
- `src/error.rs` — códigos `bridge.*`
- `src/config.rs` — `BridgeConfig` + leitura/escrita TOML
- `src/discovery.rs` — `DiscoveryFile`, escrita atômica 0600, leitura da descoberta do MaxCode
- `src/auth.rs` — `Token`, `verify_bearer`, `OriginPolicy`, `HostPolicy`
- `src/protocol.rs` — `Envelope`, `MessageType`, payloads, `PROTOCOL_VERSION`
- `src/server.rs` — `BridgeServer` (axum Router, bind com fallback, graceful shutdown)
- `src/session.rs` — `PeerRegistry`, `PeerSession`, heartbeat
- `src/client.rs` — `MaxCodeClient`, `MaxCodeAdapter`, fila de wake-up e confirmação autenticada
- `src/dedup.rs` — reserva atômica, cache de resultados e correlação entre transportes
- `src/uri.rs` — `MaxTerminalUri` (parse), `MaxCodeUri` (build), limites
- `src/paths.rs` — `canonicalize_project_path`, `resolve_worktree_root`
- `src/sync.rs` — `ActiveProjectSync`, `LoopGuard`
- `src/prompt.rs` — `build_inspect_prompt`, `truncate_utf8_prefix`, `truncate_utf8_suffix`
- `src/handler.rs` — trait `BridgeHost` (inversão de dependência para o app Tauri)
- `tests/server_it.rs`, `tests/uri_it.rs`, `tests/discovery_it.rs`

### 4.2. Backend — crate binário `src-tauri/`
- `src/bridge/mod.rs` — bootstrap no `setup()`, shutdown coordenado pelo hook da Fase 09 antes do exit final
- `src/bridge/host.rs` — implementação de `BridgeHost` (emite eventos, consulta projetos/blocos)
- `src/bridge/adapters.rs` — adaptadores para contratos das Fases 05/06/07/09
- `src/bridge/deep_link.rs` — `BridgeActivationHandler` registrado no `ActivationRouter` da 09, sem outro listener/plugin/fila de transporte
- `src/bridge/editor.rs` — `BridgeEditorOpener`, registro idempotente via `EditorLauncher::set_bridge_opener`
- `src/commands/bridge.rs` — 9 comandos IPC
- `src/events/bridge.rs` — payloads dos eventos `bridge://*`
- `tauri.conf.json` — bloco `plugins.deep-link.desktop.schemes = ["maxterminal"]`
- `capabilities/default.json` — revisar menor privilégio; deep links tratados no Rust não precisam expor gestão de registro ao webview
- `packaging/linux/maxterminal.desktop` (com `MimeType=x-scheme-handler/maxterminal;` e `Exec=maxterminal %u`)

### 4.3. Frontend
- `src/ipc/bridge.ts`
- `src/stores/maxCodeBridge.ts`
- `src/composables/useActiveProjectSync.ts`, `src/composables/useMaxCodeInspect.ts`
- `src/components/bridge/InspectButton.vue`, `BridgeStatusIndicator.vue`, `BridgeSettingsSection.vue`
- `src/types/bindings.ts` (regenerado)
- Testes: `tests/stores/maxCodeBridge.spec.ts`, `tests/composables/useActiveProjectSync.spec.ts`,
  `tests/components/bridge/InspectButton.spec.ts`, `tests/components/bridge/BridgeStatusIndicator.spec.ts`

### 4.4. Banco de dados
- **Nenhuma migration** nesta fase (ver spec §7).

### 4.5. Sub-implementação 06 — protocolo de issues
- `crates/mt-bridge/src/issues.rs`, extensão aditiva de `protocol.rs`, `client.rs`, `uri.rs`, `session.rs` e `lib.rs`.
- `crates/mt-bridge/fixtures/maxcode/issues-protocol.v1.json`, `tests/issues_protocol_it.rs` e `tests/bugs_cycle_it.rs`.
- Ordem: 11/01 → 02 → 03 → 04 → 05 → **06**; consumidor 16 após contrato da 06. Protocolo proposto não é marcado como validado por passar fixtures.

---

## 5. Critérios de Aceite

Comandos executados na raiz da worktree; todos com exit 0 salvo indicação.

### 5.1. Qualidade
- [ ] CA-01 `cargo clippy -p mt-bridge --all-targets -- -D warnings` e `cargo test -p mt-bridge` passam.
- [ ] CA-02 `cargo test --manifest-path src-tauri/Cargo.toml --workspace` e scripts de lint/typecheck/test definidos pela Fase 01 passam; comandos Cargo de CA-01/04/05 usam o mesmo `--manifest-path` quando executados da raiz.
- [ ] CA-03 `npm run bindings && git diff --exit-code src/types/bindings.ts` retorna 0 e o arquivo exporta os 9 comandos do I-12 e os tipos `BridgeStatus`, `BridgeConfig`, `BridgeState`, `PeerInfo`, `DeliveryChannel`, `DeliveryReport`.
- [ ] CA-04 `mt-bridge` não depende de `tauri` (`cargo tree -p mt-bridge -e normal | grep -c '^.*tauri '` = 0).
- [ ] CA-05 Cobertura de testes Rust ≥ 85% de linhas em `protocol.rs`, `auth.rs`, `uri.rs`, `prompt.rs`, `sync.rs`, `paths.rs` (`cargo llvm-cov -p mt-bridge`).

### 5.2. Servidor e segurança
- [ ] CA-06 Com o app aberto, `ss -ltnp | grep maxterminal` mostra **apenas** `127.0.0.1:<porta>`; nenhum socket em `0.0.0.0` ou `[::]`.
- [ ] CA-07 Com a porta `47615` ocupada (`nc -l 127.0.0.1 47615`), o app sobe em `47616` e `bridge.json` reflete a porta real; com `47615..=47624` ocupadas, usa porta efêmera.
- [ ] CA-08 `stat -c '%a' $XDG_RUNTIME_DIR/maxterminal/bridge.json` = `600` e o diretório = `700`; o arquivo contém `version`, `port`, `token`, `pid`, `started_at`, `protocol`.
- [ ] CA-09 `curl -s 127.0.0.1:$PORT/v1/health` retorna `200 {"ok":true,"protocol":1}` sem expor token nem versão detalhada.
- [ ] CA-10 `POST /v1/messages` sem token → `401 bridge.unauthorized`; token inválido → `401`; com `Origin: https://evil.example` → `403 bridge.origin_forbidden`; com `Host: evil.example:$PORT` → `403 bridge.host_forbidden`; corpo > 256 KiB → `413`.
- [ ] CA-11 `websocat -H "Authorization: Bearer $TOKEN" ws://127.0.0.1:$PORT/v1/ws` recebe `hello.ack` após enviar `hello`; sem `hello` em 5 s a conexão é fechada com código `4001`.
- [ ] CA-12 Mensagem com `v: 2` recebe `error` com `code = bridge.unsupported_version`; `type` desconhecido recebe `error bridge.unknown_type` sem derrubar a sessão.
- [ ] CA-13 Sem `pong` por 45 s, a sessão é encerrada e `bridge://peer-disconnected` é emitido.
- [ ] CA-14 Ao fechar o app via "Sair" (tray) o `bridge.json` é removido; após `kill -9`, a próxima inicialização sobrescreve o arquivo órfão (pid morto).
- [ ] CA-15 `bridge_regenerate_token` invalida o token anterior: sessões abertas são encerradas (`4003`) e o novo token é gravado no arquivo.

### 5.3. Deep links `maxterminal://`
- [ ] CA-16 `desktop-file-validate packaging/linux/maxterminal.desktop` passa e o arquivo contém `MimeType=x-scheme-handler/maxterminal;`.
- [ ] CA-17 Com o app **fechado**, `xdg-open 'maxterminal://project/open?path=%2Fhome%2Fjohnattas%2FGitHub%2FMaxTerminal'` inicia o app e foca o projeto.
- [ ] CA-18 Com o app **aberto** (inclusive minimizado na bandeja), o mesmo comando não cria segunda instância (`pgrep -c maxterminal` = 1), exibe a janela e foca o projeto.
- [ ] CA-19 `maxterminal://tab/open` abre aba Shell neutra, sem comando inicial, após validar projeto/`cwd`; escolher template com execução exige pedido autenticado. `cwd` fora do projeto/worktree verificada é rejeitado com "Pasta fora do projeto"; URI nunca permite disparar template arbitrário.
- [ ] CA-20 URIs malformadas (rota desconhecida, `path` relativo, `path` inexistente, parâmetro > 4096 bytes) geram toast de erro pt-BR e log `WARN`, sem crash.

### 5.4. URI `vscode://johnattas.maxcode/` e cliente
- [ ] CA-21 Sem peer, conversa genérica usa `/bridge/wake?requestId=<uuid>` validado; mantém payload em memória por até 30 s e confirma entrega somente após ACK autenticado. Issues usam a exceção `/issue` de 11/06 somente em perfil validado wake-only, esperando handshake por 10 s. Sem extensão compatível retorna erro explícito, nunca sucesso baseado apenas em `xdg-open`.
- [ ] CA-22 Com peer WS, `maxcode.open_conversation` recebe ACK terminal em até 3 s. Timeout após envio tem resultado `bridge.delivery_unknown`; retry só com mesma chave e capability de dedup. ACK perdido e troca WS/HTTP não criam duas conversas/abas.
- [ ] CA-23 Nenhuma URI/argv/log contém prompt, saída, comando, Bearer ou segredo. Prompt genérico > 64 KiB é rejeitado; no Inspecionar truncar os valores antes da interpolação, preservando exatamente o template.

### 5.5. Sincronização de projeto ativo
- [ ] CA-24 Peer envia `project.opened { path: '<repo>/.max-code-worktrees/wt-x' }` → MaxTerminal seleciona o projeto `<repo>` (match canônico) e emite `bridge://project-focus-requested` com `worktreePath` preenchido (DTO camelCase).
- [ ] CA-25 Seleção manual envia uma `project.focus` (debounce 250 ms); seleção remota mantém `origin`/`correlationId` no backend e UI e não ecoa mesmo após 1500 ms ou reconexão. Repetição da mesma mensagem reapresenta ACK sem repetir efeitos; seleção manual posterior do mesmo projeto continua válida.
- [ ] CA-26 Caminho sem projeto correspondente não altera a seleção e emite `ack { handled: false, reason: 'project_not_found' }`.
- [ ] CA-27 Desligar "Sincronizar projeto ativo" em `BridgeSettingsSection` interrompe envio e recebimento de foco imediatamente.

### 5.6. Botão "Inspecionar"
- [ ] CA-28 O botão aparece **somente** em blocos com `exit_code != 0` e `null`/`0` o ocultam (teste de componente com os três casos).
- [ ] CA-29 Para `ls /naoexiste` (exit 2), o prompt enviado é exatamente
      `Ao executar o comando "ls /naoexiste" tive o seguinte erro: "ls: não foi possível acessar '/naoexiste': Arquivo ou diretório inexistente"`.
- [ ] CA-30 Segredos no comando/saída (`Bearer abc...`, `APP_KEY=base64:...`) aparecem mascarados no prompt (teste com o sanitizador real da Fase 03).
- [ ] CA-31 Sequências ANSI são removidas; o `${erro}` é limitado às últimas 80 linhas e 6000 caracteres; o `${comando}` a 2000 caracteres.
- [ ] CA-32 Durante o envio o botão mostra estado de carregamento e fica desabilitado; falha total exibe toast pt-BR "Não foi possível abrir o MaxCode" com o `code` do erro.

### 5.7. UI de estado
- [ ] CA-33 `BridgeStatusIndicator` na `AppTitleBar` mostra: verde `--max-success-500` "MaxCode conectado", teal `--max-info-500` "Aguardando MaxCode", vermelho `--max-danger-500` "Ponte indisponível"; tooltip com porta e número de peers.
- [ ] CA-34 Registrar matriz da extensão real: versão, transporte, wake-up, foco, criação de conversa em Plan Mode e dedup. APIs propostas não validadas ficam indisponíveis com motivo; testes com peer fake são evidência do contrato local, não da extensão.
- [ ] CA-35 Uma URI recebida por startup, plugin e single-instance é roteada uma vez; fila aguarda stores/projetos prontos. Reconectar webview recupera snapshot e não cria segunda aba.
- [ ] CA-36 HTTP e WS têm limites de taxa, mensagens agregadas/fragmentadas, filas, conexões e operações em andamento; destino HTTP é literalmente `127.0.0.1`, sem proxy/redirect. Descoberta rejeita symlinks, dono/modo incorretos e instância obsoleta.
- [ ] CA-37 Nenhuma dependência de implementação das Fases 12–17 é exigida. `maxcode_open_plan` permanece genérico; BUG FIX nunca é anunciado como executado por abrir Plan Mode.
- [ ] CA-38 `11/06` cobre `issue.command`, `bugs.cycle.preview/start/status/progress`, capabilities `issues_protocol_v1`/`bugs_cycle_v1`, quatro métodos públicos e URI `/issue` gated; ausência de suporte desabilita ação com motivo, sem simular entrega.
- [ ] CA-39 Prévia não inicia tarefa, start único, ACK perdido vira `Unknown`/consulta de status; progresso de peer errado/revisão antiga é recusado. Início de lote exige contrato durável validado, não só dedup efêmera.
- [ ] CA-40 `11/04` implementa/registra `EditorOpener` via `set_bridge_opener`: linha/coluna preservadas, capability de arquivo validada, fallback explícito da 09 e registro idempotente. `ActivationHandler` usa listener/fila únicos da 09, com params sem segundo decoding.
- [ ] CA-41 A Fase 16 consome DTOs/client de 11/06 via `MaxCodePort`; mapeamento de RequestId, repo, worktree, hash e progresso documentado, sem dependência reversa. Validação externa pendente mantém recursos indisponíveis.

---

## 6. Riscos e Mitigação

| # | Risco | Impacto | Mitigação |
|---|-------|---------|-----------|
| R-01 | Página web maliciosa no navegador local tentando falar com o servidor (CSRF/DNS rebinding). | Alto | Token obrigatório (não exposto a navegadores), allowlist de `Origin` (requisições com `Origin` web são rejeitadas), validação de `Host` ∈ {`127.0.0.1:<porta>`, `localhost:<porta>`}, sem CORS (`Access-Control-Allow-Origin` nunca emitido). |
| R-02 | Outro usuário local lendo o token. | Médio | `$XDG_RUNTIME_DIR` (0700 por usuário) + arquivo 0600; token rotacionado a cada inicialização; nunca logado. |
| R-03 | Porta fixa ocupada por outro software. | Baixo | Fallback sequencial + efêmera; porta real sempre no `bridge.json`. |
| R-04 | Loop infinito de foco MaxTerminal ⇄ MaxCode. | Alto | `origin` + `correlationId` propagados, dedup atômica por operação, seleção remota explícita e debounce (CA-25). |
| R-05 | URI/argv vazando dados ou confirmação falsa. | Alto | URI só de wake-up; conteúdo em canal autenticado, TTL em memória e ACK terminal. |
| R-06 | Vazamento de segredos no prompt enviado ao MaxCode. | Alto | Normalizar e sanitizar valores completos antes de recortar; verificação final redige valores e reconstrói a frase fixa. Testes com padrões da Fase 03 e cortes na fronteira. |
| R-07 | `tauri-plugin-deep-link` no Linux exigir `.desktop` instalado para o scheme funcionar em `dev`. | Médio | Em debug, `app.deep_link().register_all()` no setup; em release, registro pelo pacote (Fase 17). Documentado. |
| R-08 | Divergência de nomes com contratos das Fases 05/06/07/09. | Médio | Adaptadores isolados em `src-tauri/src/bridge/adapters.rs`; `mt-bridge` depende só da trait `BridgeHost`. |
| R-09 | Extensão MaxCode sem protocolo compatível. | Alto | `MaxCodeAdapter`, capacidades verificadas e teste real; URI também é proposta e não garante fallback funcional. |
| R-10 | Worktree com metadados Git inválidos ou caminho forjado. | Médio | Validar vínculo com repositório registrado; nome de pasta nunca autoriza associação. Retornar erro explícito quando não houver prova. |
| R-11 | Bloqueio da thread principal durante `xdg-open` ou envio WS. | Médio | Todo I/O em tasks tokio; `xdg-open` com `tokio::process::Command` e timeout de 5 s; comandos IPC `async`. |
