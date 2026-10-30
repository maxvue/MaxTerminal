# Fase 02 — Core PTY e Supervisão de Processos

> Fonte da verdade: [`../../AGENTS.md`](../../AGENTS.md) (§2 Stack — `portable-pty` + `/proc`; Diretriz §4 — ciclo de vida
> seguro de PTYs, nenhum processo órfão, Channels sem bloqueio da UI). Contratos compartilhados:
> [`../README.md`](../README.md) §2. Especificação técnica detalhada: [`spec.md`](./spec.md).

---

## 1. Objetivo da Fase

Entregar o crate `mt-pty` e a camada IPC correspondente no crate binário `maxterminal`, fornecendo um **núcleo de PTY
confiável, rápido e supervisionado** sobre o qual todas as fases seguintes constroem terminais, abas, agentes e
detectores:

1. Criar PTYs reais (`portable-pty`) com shell, `cwd`, argumentos e variáveis de ambiente controlados.
2. Transportar o output do PTY ao frontend em **chunks binários** via `tauri::ipc::Channel`, com coalescing e
   **backpressure ponta a ponta** (rajadas intensas não podem travar a UI nem estourar memória).
3. Supervisionar a árvore de processos de cada PTY via `/proc` (filhos, foreground via `tcgetpgrp`, `cmdline`, `cwd`,
   estado) e expor essa informação por IPC e por API Rust para as Fases 05, 07, 08 e 13.
4. Encerrar todos os processos pertencentes ao PTY no fechamento explícito: `SIGHUP → SIGTERM → SIGKILL`,
   contenção Linux em cgroup v2 por PTY, confirmação de grupo vazio e reaping. `/proc`, process groups e subreaper
   são complementares, não prova de contenção de `setsid`/double-fork. Falha de cleanup permanece supervisionada
   e visível; nunca é apresentada como fechamento bem-sucedido.
5. Emitir o evento global `pty://exit` com código de saída/sinal.

Não há UI de terminal nesta fase: o resultado é verificável por testes Rust, testes Vitest dos wrappers IPC e um
exemplo de linha de comando (`cargo run --example pty_smoke`).

---

## 2. Escopo

### 2.1. Incluído

| Item | Detalhe |
|------|---------|
| Crate `mt-pty` (`src-tauri/crates/mt-pty`) | Independente de Tauri (testável isoladamente); integra-se ao binário por traits (`OutputSink`, `PtyEventListener`). |
| `PtyManager` | Registro concorrente de PTYs, limites, spawn, write, resize, kill, list, info, attach, ack, shutdown. |
| Spawn | Resolução de shell (`spec.shell` → `$SHELL` → `getpwuid` → `/bin/bash` → `/bin/sh`), login opcional, `args`, `cwd` e env validados + variáveis de identificação; sanitização para persistência pertence à Fase 03. |
| Leitura | Thread dedicada por PTY com `poll(2)`, buffer de coalescing (janela 4 ms / até 256 KiB), envio binário com cabeçalho de frame. |
| Backpressure | Controle de fluxo por bytes não confirmados (`pty_ack`), marcas d'água alta/baixa; pausa da leitura propaga a pressão ao processo filho pelo buffer do kernel. |
| Replay | Ring de 1 MiB; cursor por frame + offset de bytes, geração de conexão, `Gap` explícito e ACK cumulativo idempotente. |
| Escrita | Thread de escrita por PTY com fila limitada; fragmentação de colagens grandes em blocos de 4 KiB. |
| Resize | `rows/cols/pixel_width/pixel_height` com validação e deduplicação. |
| Kill gracioso | HUP aos membros dos grupos shell/foreground por pidfd → TERM aos membros atuais do cgroup → cgroup.kill; modo force, confirmação de vazio e reaping. |
| Supervisão `/proc` | `ProcSource` (trait) com implementação real via crate `procfs`; árvore de processos, foreground pgid, `cmdline`, `exe`, `cwd`, estado, `start_time`, RSS. |
| Detecção de mudança de foreground | Supervisor periódico (1 s) que emite `pty://foreground-changed` (baixo volume). |
| Garantia de não-órfãos | Cgroup v2 delegado por PTY, launcher bloqueado antes do `exec`, watchdog externo em EOF de canal privado, `cgroup.kill`, subreaper e reaping; shutdown coordenado em `ExitRequested`. |
| Eventos | `pty://exit`, `pty://foreground-changed`, `pty://spawned`, `pty://cleanup-failed` (baixo volume); frames de dados/fim pelo Channel. |
| Comandos IPC | `pty_spawn`, `pty_write`, `pty_resize`, `pty_kill`, `pty_list`, `pty_process_info`, `pty_ack`, `pty_attach`, `pty_detach` (nove comandos). |
| API Rust para fases futuras | `pids()`, `process_tree()`, `process_report()`, `foreground_process()`, `signal_process()` (restrito ao cgroup), `subscribe()` (fan-out), `add_output_tap()` (somente leitura). |
| Frontend (sem UI) | `src/ipc/pty.ts` (wrappers tipados), `src/ipc/ptyFrame.ts` (decodificador de frames), `src/composables/usePtyStream.ts`, `src/stores/pty.ts` (`usePtyStore`). |
| Testes e benchmark | Unitários, integração com PTY real, testes de não-órfãos (inclusive panic em subprocesso), benchmark `criterion` de vazão. |

### 2.2. Fora de escopo (e fase responsável)

| Item | Fase |
|------|------|
| Renderização do terminal (`@xterm/xterm`, WebGL, fit, search, web-links), consumo do `usePtyStream`, debounce de resize no `ResizeObserver` | **04** |
| Camada de blocos / input rico estilo Warp e registro de gatilhos | **04** |
| Parser OSC 133, hooks zsh/bash/fish, exit code **por comando**, máquina de estados `TabStatus` (`running`/`continuous`/...) | **05** (consome `add_output_tap` e `pty://exit`) |
| Persistência de scrollback/comandos no SQLite e sanitização de segredos | **03** (infra) / **05**–**07** (uso) — somente registro efêmero de identidade/cgroup em `$XDG_RUNTIME_DIR`; nenhum output persistido. |
| Abas, templates, "Comando Inicial", split panes, "Iniciar nova Sessão" (reinício do PTY) | **07** (consome `pty_spawn`/`pty_kill` e `owner`) |
| Detector de portas ativas e "encerrar processo da porta" | **08** (consome `pids()` e `signal_process()`) |
| Git/worktree na barra da aba | **08** (consome `cwd` do processo via `process_tree()`) |
| Manter PTYs vivos na bandeja ao fechar a janela, notificações nativas | **09**; `CloseRequested` não chama kill diretamente. Se resultar em saída do app, `ExitRequested` coordena cleanup. |
| Detecção de agentes CLI e telemetria | **13** (consome `foreground_process()` e `pty://foreground-changed`) |
| Variáveis de ambiente de agentes/templates | **07**/**13** (repassadas via `SpawnSpec.env`) |
| Remoção de variáveis específicas de empacotamento (AppImage), benchmarks E2E com UI | **17** (usa o ponto de extensão `EnvPolicy`) |

---

## 3. Pré-requisitos

### 3.1. Fases
- **Fase 01** concluída: workspace Cargo em `src-tauri/`, crate `mt-core` com `MtError` (`{ code, message, details? }`),
  newtypes `PtyId`/`TabId`/`PaneId` (Uuid v7), pipeline `tauri-specta` gerando `src/types/bindings.ts`, lint
  (`clippy -D warnings`, ESLint), `tsgo` e Vitest configurados, pastas `src/ipc`, `src/stores`, `src/composables`.

### 3.2. Ferramentas e ambiente
- Rust, edition e lints herdados da Fase 01 (edition 2024, MSRV ≥ 1.85); sem relaxar `unsafe_code = forbid`.
- Linux com `/proc`, pidfds e cgroup v2 com `cgroup.kill` (kernel ≥ 5.14), subtree delegado gravável pelo usuário.
  Provisionamento por serviço/scope de usuário systemd com delegação, ou subtree previamente delegado pelo ambiente.
  Falta de contenção retorna `pty.containment_unavailable` antes de executar o shell; não usar fallback silencioso.
- Helper launcher/watchdog entregue nesta fase, inclusive no exemplo headless; não depende da Fase 09/17.
- Shells para testes: `bash` e `sh` obrigatórios; `zsh` e `fish` opcionais (testes marcados `#[ignore]` se ausentes).
- Utilitários usados nos testes: `stty`, `sleep`, `head`, `base64`, `setsid`, `sha256sum`.
- Node/npm conforme Fase 01 (Vitest, `tsgo`).

### 3.3. Dependências Rust novas (crate `mt-pty`)
`portable-pty 0.9`, `nix`/`rustix` compatíveis com o MSRV (wrappers seguros para pidfd, poll, sinais, termios e subreaper), `procfs 0.17`,
`crossbeam-channel`, `parking_lot`, `arc-swap`, `bytes`, `thiserror`, `serde`, `specta`, `tracing`, `signal-hook`,
`serde_json`. Dev: `tempfile`, `criterion`, `proptest`, `assert_matches`.
Fixar versões compatíveis no lockfile durante a implementação; não copiar versões como promessa de APIs não verificadas.

---

## 4. Entregáveis

### 4.1. Backend — crate `src-tauri/crates/mt-pty/`
```
mt-pty/
├── Cargo.toml
├── src/
│   ├── lib.rs                 # reexports públicos
│   ├── config.rs              # PtyConfig (limites, janelas, marcas d'água, timeouts)
│   ├── error.rs               # PtyError + From<PtyError> for MtError
│   ├── types.rs               # SpawnSpec, PtySize, PtySummary, PtyStatus, KillMode, ExitInfo, PtyOwner
│   ├── manager.rs             # PtyManager (registro, API pública)
│   ├── instance.rs            # PtyInstance (estado por PTY, threads)
│   ├── spawn/{mod,shell,env}.rs   # resolução de shell, EnvPolicy, validação de cwd/env
│   ├── io/{mod,reader,writer,frame,flow,replay}.rs  # leitura+coalescing, escrita, frames, backpressure, ring buffer
│   ├── sink.rs                # trait OutputSink + NullSink/VecSink (testes)
│   ├── events.rs              # PtyEvent, PtyEventListener, broadcast
│   ├── tap.rs                 # trait OutputTap (ponto de extensão Fase 05)
│   ├── kill.rs                # KillPlan, escalonamento de sinais, trait Signaler
│   ├── proc/{mod,source,tree,info,foreground}.rs  # ProcSource, ProcessInfo, ProcessTree
│   ├── supervisor.rs          # supervisor periódico (foreground, reaper de órfãos adotados)
│   ├── containment/{mod,cgroup,launcher,watchdog}.rs # posse por cgroup e contenção antes do exec
│   ├── guard/{mod,registry_file,panic,signals}.rs # registro por instância, notificação de emergência
│   └── bin/mt-pty-helper.rs    # modos launcher e watchdog, sem inicializar Tauri
├── examples/pty_smoke.rs
├── benches/throughput.rs
└── tests/{spawn,io,backpressure,kill,orphans,proc,panic_cleanup}.rs
```

### 4.2. Backend — crate binário `src-tauri/src/`
- `commands/pty.rs` — os nove comandos `#[tauri::command]` + `#[specta::specta]`.
- `events/pty.rs` — `TauriEventListener`, `ChannelSink` e `BinaryPtyFrame`: `Channel<BinaryPtyFrame>` com
  `IpcResponse` retornando `InvokeResponseBody::Raw`, marcador Specta opaco (sem DTO JSON); o TS recebe `ArrayBuffer` por tipo declarado manualmente (`PtyChannelMessage`), exceção única à regra de bindings (spec §9.3 e `05/spec.md` §3.1).
- `lib.rs` — registro do `Arc<PtyManager>`, comandos no builder, `ExitRequested` → shutdown assíncrono coordenado;
   `Exit` verifica conclusão. Resolver binário helper sem shell e instalar recovery por instância.
- `capabilities/default.json` — permissões dos comandos `pty_*` restritas à janela `main`.

### 4.3. Frontend
- `src/ipc/pty.ts`, `src/ipc/ptyFrame.ts`, `src/composables/usePtyStream.ts`, `src/stores/pty.ts`.
- Testes: `src/ipc/__tests__/ptyFrame.spec.ts`, `src/stores/__tests__/pty.spec.ts`,
  `src/composables/__tests__/usePtyStream.spec.ts`.
- `src/types/bindings.ts` regenerado (sem edição manual).

---

## 5. Critérios de Aceite

Testes Rust são headless com cgroup delegado. Teste de transporte real Tauri usa job WebView próprio com display
virtual; mocks não substituem a prova binária do Channel. Metas temporais usam runner de referência descrito.

### 5.1. Qualidade estática
1. `cargo fmt --check` e `cargo clippy --workspace --all-targets -- -D warnings` passam sem avisos.
2. `cargo test -p mt-pty` passa (testes com `zsh`/`fish` podem estar `#[ignore]` quando o shell não existe).
3. `npm run typecheck` (`tsgo`) e `npm run test` (Vitest) passam; `src/types/bindings.ts` contém `PtySummary`, `SpawnSpec`,
   `PtyExitEvent`, `ProcessInfo` e os nove comandos, e `git diff --exit-code src/types/bindings.ts` é limpo após
   `cargo run --bin export_bindings` (ou mecanismo equivalente da Fase 01).

### 5.2. Spawn, I/O e resize (testes `tests/spawn.rs`, `tests/io.rs`)
4. `spawn` de `/bin/sh -c 'printf hello; exit 3'` produz exatamente os bytes `hello` no sink e `ExitInfo { code: Some(3), signal: None, reason: Exited }`.
5. `cwd` inexistente → `MtError.code == "pty.cwd_invalid"`; shell inexistente → `"pty.shell_not_found"`;
   chave de env inválida (`"A=B"`, vazia ou com `\0`) → `"pty.env_invalid"`.
6. O processo filho enxerga `TERM=xterm-256color`, `COLORTERM=truecolor`, `TERM_PROGRAM=MaxTerminal`,
   `MAXTERMINAL_PTY_ID=<id>` e o `cwd` solicitado (`pwd`).
7. Após `resize(40, 120)`, `stty size` executado no PTY imprime `40 120`; resize com `rows == 0` ou `cols > 1000`
   retorna `"pty.resize_invalid"`; resize idêntico ao atual não chama `ioctl` (contador interno).
8. `write` de 1 MiB em consumidor com slave raw e echo desativado resulta em SHA-256 idêntico.
   Testar separadamente entrada textual com disciplina de linha normal e todos os bytes 0–255 sem UTF-8 acidental.
9. UTF-8 multibyte/ANSI cortados entre chunks chegam byte a byte íntegros; `decodeFrame` não decodifica texto.
   Consumidor de teste usa `TextDecoder` com `stream: true`; renderer posterior recebe `Uint8Array` diretamente.

### 5.3. Rajadas e backpressure (`tests/backpressure.rs`, `benches/throughput.rs`)
10. Fixture determinística de 200 MiB em slave raw: hash idêntico à origem, ordem preservada; medir vazão core e
    WebView separadamente. Meta core ≥ 150 MiB/s na máquina de referência identificada, não gate de CI compartilhada.
11. Com sink que **não confirma** (`ack` ausente) estando anexado: bytes em trânsito nunca excedem
    `high_watermark` (1 MiB); RSS do processo de teste cresce < 16 MiB durante 10 s de rajada; o processo
    produtor para de avançar até ACK (contador da fixture); estado `S` sozinho não comprova bloqueio por backpressure.
    Neste teste, desativar expiração da lease; outro teste cobre timeout e invalidação da conexão.
12. Ao retomar acks, o fluxo continua sem perda nem duplicação (números de sequência contínuos).
13. Com Channel **desanexado** (componente desmontado), a leitura não bloqueia; o replay retém apenas os últimos
    1 MiB; `pty_attach` entrega primeiro o replay e depois os dados ao vivo, sem lacunas de sequência após o replay.
14. Latência de eco de tecla (write `a` → frame contendo `a`) p99 < 10 ms com o PTY ocioso (teste com 1 000 amostras).
15. Flush por 4 ms **ou** 256 KiB; em alta vazão pode haver mais de 250 frames/s. Verificar teto de bytes/frame,
    janela de crédito e memória, não limite de frames/s incompatível com 150 MiB/s.

### 5.4. Kill e não-órfãos (`tests/kill.rs`, `tests/orphans.rs`, `tests/panic_cleanup.rs`)
16. `pty_kill(graceful)` em shell ocioso termina com `reason: KilledByUser` em < 500 ms (sai já no `SIGHUP`).
17. Processo que ignora `SIGHUP` (`trap '' HUP; sleep 1000`) é encerrado por `SIGTERM` após `hup_grace` (padrão 1 s);
    processo que ignora `HUP` e `TERM` é encerrado por `SIGKILL` após `term_grace` (padrão 2 s). Tempo total ≤ 3,5 s.
18. Background, `nohup`, `setsid`, double-fork imediato e daemon com ambiente limpo: após conclusão de `pty_kill`,
    cgroup `populated=0`, filhos diretos/adotados reapados e identidades ausentes em `/proc`. Rodar com dois PTYs;
    fechar um não sinaliza o outro. Testar forks durante a escalada e falha antes de liberar o launcher.
19. Quando o shell sai sozinho (`exit`), membros remanescentes do cgroup são encerrados por TERM/KILL e o
    `pty://exit` é publicado após enfileirar `End`, carregando `finalSeq`; eventos e Channel não têm ordem relativa
    de recepção garantida. O consumidor aguarda seu cursor alcançar `finalSeq` ou uma lacuna declarada.
20. `PtyManager::shutdown()` com 20 PTYs ativos encerra todos em ≤ 3,5 s no cenário saudável, sem cgroups populados.
    Espera ininterruptível simulada retorna falha e continua supervisionada, sem evento falso de sucesso.
21. Teste em subprocesso que provoca `panic!` no app de teste: após a morte do subprocesso, nenhum descendente
    permanece vivo após o watchdog confirmar cleanup, inclusive em build `panic=abort` da Fase 01.
22. Matar o app com `SIGKILL`: watchdog externo limpa o cgroup sem reiniciar o app. Recovery posterior cobre morte
    simultânea do app/watchdog, valida boot/instância/cgroup e não toca outra instância viva nem PID reutilizado.
23. Processos órfãos adotados pelo subreaper não viram zumbis (`State: Z`) por mais de 2 ciclos do supervisor.
24. `CloseRequested` não chama kill; saída efetiva passa por `ExitRequested`, bloqueia novos spawns, termina cleanup
    fora do event loop e só então libera `Exit`. Fechar/minimizar sem sair preserva os PTYs.

### 5.5. Supervisão `/proc` (`tests/proc.rs`)
25. `process_info` de um PTY rodando `sleep 30` retorna `foreground.cmdline == ["sleep", "30"]`,
    `foreground.state == Sleeping`, `shell.pid == pids.shell_pid` e `cwd` igual ao diretório após `cd /tmp`.
26. `process_tree` lista corretamente 3 níveis (`sh → sh → sleep`) com `ppid` consistentes; processo que sai entre a
    listagem e a leitura não gera erro (é omitido).
27. `signal_process(pty_id, ProcessIdentity)` fora do cgroup retorna `"pty.pid_not_owned"`; PID reutilizado entre
    amostragem/sinalização não recebe sinal (pidfd validado). Snapshot de `/proc` não autoriza kill.
28. `pty://foreground-changed` é emitido no máximo uma vez por mudança, com latência ≤ 1,5 s, ao executar `vim`/`sleep`.

### 5.6. IPC e frontend
29. Teste de integração Tauri (`tauri::test::mock_builder`) cobre os nove comandos e valida a serialização de erros
    `{ code, message }` com mensagens em pt-BR.
30. Vitest: validar frames, ACK cumulativo após consumo, ACK duplicado/antigo/futuro, replay sujeito a crédito,
    reconnect simultâneo ao output, detach explícito, consumidor lento/falho, troca de `ptyId` e exit antecipado.
    Store mantém tombstone, reconcilia listagem com eventos e libera listeners ao desmontar.
31. `cargo run -p mt-pty --example pty_smoke -- --cmd 'ls -la'` imprime o output e o código de saída.
32. Sem delegação/pidfd, spawn falha antes do código do usuário; launcher cancelado/timeout não executa shell.
33. Taps recebem cada frame bruto original uma vez, inclusive desanexado; replay não duplica observação. Registrar
    tap antes de spawn observa primeiro byte; remover handle impede callbacks posteriores à conclusão da remoção.
34. Repetir 100 ciclos spawn/attach/resize/kill: sem crescimento de FDs/threads/cgroups; cancelar write bloqueado,
    resize concorrente com kill, spawn concorrente com shutdown e falha de envio do Channel.
35. `pty_attach` recebe Channel como argumento direto, nunca dentro de DTO; geração com pins da Fase 01
    (Specta RC27 e conjunto compatível) preserva `Result<T, MtError>` explícito. Teste demonstra que `send(Ok)`
    sem callback do consumidor concluído não libera crédito: ACK representa consumo, não aceitação no transporte.
    O gate T-06 (`05/spec.md` §3.1) permanece pendente até comprovar compilação/exportação e transporte Raw em
    WebView real. A estratégia de marcador Specta opaco não é implementação validada; o alias manual
    `PtyChannelMessage = ArrayBuffer` vive em `src/ipc/ptyFrame.ts`, e cast/grep/mocks não provam transporte.

---

## 6. Riscos e Mitigação

| Risco | Impacto | Mitigação |
|-------|---------|-----------|
| Rajadas (ex.: `cat` de log gigante) saturam o IPC/WebView e travam a UI | Alto | Coalescing por janela de tempo/tamanho, frames binários (sem JSON/base64), controle de fluxo por ack com marcas d'água; benchmark e teste de RSS (critérios 10–15). |
| Frontend não confirma | Médio | Lease expirada invalida geração, desanexa e só retém replay; ACK antigo não reativa conexão. Política e lacunas explícitas. |
| `setsid`, double fork e ambiente limpo | Alto | Cgroup antes do exec, `cgroup.kill` cobre forks concorrentes; subreaper faz reaping. Sem subtree delegado, spawn falha. |
| Reutilização de PID | Alto | `(boot_id, pid, start_time)` + pidfd validado; não usar PGID antigo como identidade estável. |
| Panic com locks travados | Médio | Watchdog fora do processo; hook apenas notifica sem locks/allocações complexas. Não depender de `catch_unwind` em `panic=abort`. |
| `SIGKILL` do app | Médio | Watchdog observa EOF de canal não herdável; recovery cobre falha também do watchdog. HUP do master não é garantia. |
| Corrida entre `child.wait()` e reaper de órfãos (roubo de `waitpid`) | Médio | Reaper nunca usa `waitpid(-1)`; só `waitpid(pid, WNOHANG)` em PIDs adotados que não são filhos diretos registrados. |
| `read` no master retorna `EIO` quando o slave fecha (Linux) | Baixo | Tratado como EOF normal; testes cobrem. |
| Escrita quando filho não lê stdin | Médio | Fila limitada por bytes/pedidos e writer com poll + cancelamento; shutdown não depende de write_all bloqueante. |
| Muitas threads com muitos PTYs | Baixo | 3 workers/PTY, supervisor compartilhado, watchdog por instância e pool global de buffers; validar stack de 256 KiB por teste. |
| `/proc` com `hidepid` ou permissões restritas | Baixo | Campos inacessíveis viram `None`; erro `pty.proc_unavailable` só quando o próprio PTY não é legível. |
| Exposição de segredos em `cmdline`/env via IPC ou logs | Médio | Env nunca é retornado por IPC nem logado (só chaves); `cmdline` só para processos do próprio PTY; persistência fica a cargo das fases que usam o sanitizer da Fase 03. |
| Formato de mensagens binárias do `Channel` divergir entre versões do Tauri | Médio | Spike/teste de integração confirmando entrega como `ArrayBuffer`; cabeçalho de frame versionado (`version = 1`). |
