# Fase 08 — Indicadores Contextuais: Git, Worktree e Portas Ativas

> Fonte da verdade: [`../../AGENTS.md`](../../AGENTS.md) §4.2 ("Indicadores Contextuais na Barra da Aba").
> Contratos compartilhados: [`../README.md`](../README.md) §2. Especificação técnica: [`spec.md`](./spec.md).

---

## 1. Objetivo da Fase

Exibir, na barra de cada aba, o **contexto vivo** do diretório e dos processos daquela aba:

1. **Git & Worktree** — branch atual (ou `HEAD` destacado com SHA curto), estado `dirty`
   (contagens de *staged*, *unstaged*, *untracked* e conflitos), *ahead/behind* em relação ao upstream
   e indicação explícita quando o diretório é uma **worktree vinculada** (`.worktrees/`,
   `.max-code-worktrees/` ou outra), com o nome da worktree.
2. **Portas Ativas** — detectar quando qualquer processo da árvore de PIDs da aba passa a escutar uma
   porta TCP local (ex.: `:5177`, `:8000`) ou vincular um socket UDP e exibir um badge com protocolo,
   painel e dono. TCP acessível em loopback permite **abrir no navegador**; ambos permitem
   **encerrar exatamente o processo selecionado** (com confirmação e identidade revalidada).

Além disso, esta fase publica uma **API pública de worktrees** (listar/criar/remover), em Rust e via IPC,
que será consumida pelas Fases 13 (isolamento opcional de Agentes CLI) e 16 (abrir aba na worktree da issue).

---

## 2. Escopo

### 2.1. Incluído

**Crate `mt-git` (`src-tauri/crates/mt-git/`)**
- Descoberta de repositório a partir do `cwd` do painel ativo da aba (subindo diretórios até achar `.git`).
- Detecção de worktree vinculada: `.git` como **arquivo** `gitdir: <caminho>` + validação de `commondir`
  no gitdir; classificação `maxCode` (`/.max-code-worktrees/`), `project` (`/.worktrees/`) ou `other`;
  e vínculo reverso `gitdir`; confirmar via Git quando disponível. Nome de pasta só define a apresentação
  após comprovar o vínculo. Submódulos e gitdir separado **não** são classificados como worktree.
- Leitura rápida de `HEAD` por arquivo (sem processo) para pintura inicial e modo degradado.
- Status completo via `git --no-optional-locks status --porcelain=v2 --branch -z` (escolha justificada no
  `spec.md` §2.1): branch, oid, upstream, ahead/behind, contagens de alterações.
- Cache por repositório (chave = gitdir canônico da worktree), *single-flight*, contagem de referências
  por painel e limite global de concorrência.
- Watch de `HEAD`, `index`, `refs/`, `packed-refs` (e do `commondir` em worktrees) com `notify`
  (inotify) + debounce de 300 ms; gatilhos adicionais: fim de comando (OSC 133 `D`, Fase 05), foco da
  janela e reconciliação de 30 s visível/120 s oculta enquanto houver painel vivo.
- Evento `git://status-changed`.
- API de worktrees: `list`, `create`, `remove`, `prune` (trait `WorktreeManager` + comandos IPC).

**Crate `mt-ports` (`src-tauri/crates/mt-ports/`)**
- Parser de `/proc/net/{tcp,tcp6,udp,udp6}`: TCP `0A` = `LISTEN`; UDP vinculado, sem presumir HTTP
  ou estado `LISTEN`. Inclui IPv4, IPv6, IPv4-mapeado-em-IPv6 e namespace de rede.
- Mapeamento `inode → donos` lendo `/proc/<pid>/fd/*` **apenas** dos PIDs supervisionados da aba (Fase 02),
  conservando `(ptyId, pid, starttime)`; mesmo número de porta nunca colapsa donos/protocolos diferentes.
- *Poller* único com agendamento adaptativo por aba: 1 s quando `running`/`continuous`; 5 s quando
  ocioso com descendentes ou portas conhecidas; 30 s ocioso sem descendentes, como reconciliação;
  desligado apenas sem PTY vivo. Detecta também servidor que faz bind após o prompt retornar.
- Eventos `port://opened`, `port://updated`, `port://closed` e snapshots versionados para reconciliação.
- Comandos `port_open_browser` (adaptador `tauri-plugin-opener` registrado **nesta fase**) e `port_kill`
  (SIGTERM → espera 3 s → SIGKILL, revalidação de posse e `pidfd` para não sinalizar PID reutilizado).

**Frontend**
- Wrappers IPC `src/ipc/git.ts` e `src/ipc/ports.ts` (usando tipos gerados em `src/types/bindings.ts`).
- Stores `useGitStore` (`src/stores/git.ts`) e `usePortsStore` (`src/stores/ports.ts`).
- Composable `useTabIndicatorsSync` (sincroniza painéis/abas da Fase 07 com o backend).
- Componentes em `src/components/indicators/`: `GitIndicator.vue`, `WorktreeBadge.vue`, `PortBadge.vue`
  e o contêiner `TabIndicators.vue`, inserido no **slot de indicadores da TabBar** (ponto de extensão da Fase 07).
- Confirmação de encerramento via `MaxPopoverConfirm`; tooltips via `v-tooltip`; ícones via `MaxIcon`.

### 2.2. Fora de escopo (e fase responsável)

| Item | Fase |
|------|------|
| Criação automática de worktree ao abrir aba de Agente CLI (UI/fluxo) | 13 |
| Abrir aba na worktree/branch de uma issue; badge 🟣 de Issues; `gh` CLI | 16 |
| Contador de Issues e qualquer chamada à API do GitHub | 16 |
| Notificações nativas do SO (ex.: "porta aberta") e system tray | 09 |
| Links clicáveis de arquivo → editor | 09 |
| Telemetria de agentes (StatusLine) que eventualmente exiba branch | 13 |
| Busca global/FTS de histórico e benchmarks gerais / empacotamento | 17 |
| Operações Git de escrita além de worktree (commit, checkout, push, stash) | — (não previstas no AGENTS.md) |
| Sockets UNIX, descoberta dentro de outros *network namespaces* (containers Docker) | — (limitação explícita; UDP no namespace do app está incluído) |
| Persistência em SQLite de status Git ou portas | — (estado efêmero; nenhuma migration nesta fase) |

---

## 3. Pré-requisitos

### 3.1. Fases
- **Fase 01** — workspace Cargo, `mt-core` (`MtError`, `TabId`, `PaneId`, `PtyId`), `tauri-specta`,
  lint/typecheck (`tsgo`), Vitest, `@maxvue/max-components-ui`.
- **Fase 02** — `PtyManager::{summary, process_report, descendant_pids, owner_of_pid, add_listener}`;
  `ProcessInfo.start_time`, `PtyOwner`, árvore supervisionada e `cwd` do shell. Consumido por adaptadores
  (`ProcessTreeSource`, `CwdFallback`) implementados no crate binário (ver `spec.md` §1.3).
- **Fase 05** (transitiva via 07) — `TabStatus`, `cwd` por painel (OSC 7) e marcador de fim de comando (OSC 133 `D`).
- **Fase 07** — `WorkspaceObserver`, snapshot de workspace, `useTabsStore.tabsById`, `TabView.panes`,
  painel ativo e slot `TabBar #indicators="{ tab }"`. Status vem da Fase 05, não de um campo inexistente de `TabView`.
- **Extensão implementada pela 08 sobre a base 02:** `ExactProcessSignaler` com `pidfd` e validação do
  registro do PTY; `signal_process(id, pid, sig)` sozinho não elimina a corrida entre checagem e sinal.
- **Fase 03** (transitiva via 07) — sanitização de erros de Git no adaptador; nenhuma persistência nova.

### 3.2. Ferramentas
- `git` ≥ 2.38 no `PATH` (porcelain v2 com `-z`, `worktree list --porcelain -z`). Ausência ⇒ modo degradado.
- `tauri-plugin-opener` v2, no crate binário, inicializado pela 08; navegador padrão configurado no SO.
- Kernel Linux com `/proc/net/{tcp,tcp6,udp,udp6}`, inotify e `pidfd_open`/`pidfd_send_signal` (CachyOS).
- Crates: `notify` (versão compatível com o workspace), `nix`/`rustix` (sinais e pidfd), `tokio` (time, process, sync), `thiserror`,
  `serde`, `specta`, `tracing`, `dunce` (canonicalização), `tempfile` (dev).
- `python3` (apenas testes de integração de `mt-ports`; testes marcados `#[ignore]` se ausente).

---

## 4. Entregáveis

### 4.1. Rust — `src-tauri/crates/mt-git/`
- `Cargo.toml`
- `src/lib.rs` — reexports e `GitService`
- `src/types.rs` — `GitContext`, `GitBranch`, `GitChanges`, `GitWorktree`, `WorktreeKind`, `WorktreeEntry`, `RepoKey`
- `src/discover.rs` — descoberta de repo, parsing de `.git` arquivo/diretório, `commondir`, `HEAD`
- `src/porcelain.rs` — parser de `status --porcelain=v2 --branch -z`
- `src/runner.rs` — `GitRunner` (trait) + `CliGitRunner` (processo `git` com timeout e env seguro)
- `src/cache.rs` — `RepoCache`, single-flight, refcount, expiração
- `src/watcher.rs` — `RepoWatcher` (`notify` + debounce)
- `src/worktree.rs` — trait `WorktreeManager` + `CliWorktreeManager`, validações de nome/branch
- `src/error.rs` — `GitError` + `From<GitError> for MtError`
- `tests/fixtures/porcelain/*.bin`, `tests/discover.rs`, `tests/worktree.rs`, `tests/watcher.rs`

### 4.2. Rust — `src-tauri/crates/mt-ports/`
- `Cargo.toml`
- `src/lib.rs` — `PortsService`
- `src/types.rs` — `ListeningPort`, `PortId`, `PortProtocol`, `PortSnapshot`, `PortAddress`, `IpFamily`, `KillOutcome`, `PollMode`
- `src/procnet.rs` — parser de `/proc/net/{tcp,tcp6,udp,udp6}`
- `src/fdmap.rs` — mapa `inode → Vec<(pid, starttime, comm)>` restrito à árvore
- `src/tree.rs` — trait `ProcessTreeSource` (+ `ProcfsTreeSource` para testes)
- `src/poller.rs` — agendador adaptativo e diff de snapshots
- `src/killer.rs` — SIGTERM → SIGKILL com revalidação e trait `ExactProcessSignaler`
- `src/opener.rs` — seleção de destino loopback e trait `PortUrlOpener` (sem dependência de Tauri)
- `src/error.rs` — `PortsError` + `From<PortsError> for MtError`
- `tests/fixtures/proc_net_tcp*.txt`, `tests/procnet.rs`, `tests/live_listen.rs`, `tests/poller.rs`

### 4.3. Rust — crate binário `src-tauri/src/`
- `commands/git.rs` — comandos `git_*`
- `commands/ports.rs` — comandos `port_*`
- `events/git.rs`, `events/ports.rs` — emissores tipados
- `adapters/process_tree.rs` — implementa `ProcessTreeSource` e `CwdFallback` sobre `mt-pty` (Fase 02)
- `adapters/port_opener.rs`, `adapters/exact_process_signaler.rs`, `adapters/indicators.rs` — plugin opener,
  pidfd e observador da Fase 07; registro do plugin e capabilities dos comandos da janela `main`
- Registro dos comandos/eventos no `tauri_specta::Builder` e dos serviços no `State` do Tauri

### 4.4. Frontend
- `src/ipc/git.ts`, `src/ipc/ports.ts`
- `src/stores/git.ts`, `src/stores/ports.ts`
- `src/composables/useTabIndicatorsSync.ts`
- `src/components/indicators/TabIndicators.vue`, `GitIndicator.vue`, `WorktreeBadge.vue`, `PortBadge.vue`
- Testes: `src/stores/__tests__/git.spec.ts`, `src/stores/__tests__/ports.spec.ts`,
  `src/components/indicators/__tests__/*.spec.ts`

### 4.5. Banco de dados
- **Nenhuma** migration (estado puramente em memória).

---

## 5. Critérios de Aceite

### 5.1. Qualidade e build
- [ ] `cargo build --workspace` sem warnings.
- [ ] `cargo clippy -p mt-git -p mt-ports -p maxterminal -- -D warnings` passa.
- [ ] `cargo test -p mt-git -p mt-ports` passa (testes `#[ignore]` só os que dependem de `python3`).
- [ ] `cargo test -p mt-ports -- --ignored` passa em máquina com `python3`.
- [ ] `src/types/bindings.ts` regenerado contém `GitContext`, `WorktreeEntry`, `ListeningPort`, `KillOutcome`
      e nenhum desses tipos está duplicado à mão em `src/`.
- [ ] `pnpm typecheck` (tsgo) e `pnpm lint` passam; `pnpm vitest run src/stores src/components/indicators` passa.

### 5.2. Testes unitários obrigatórios (Rust)
- [ ] Parser porcelain: repo limpo; staged+unstaged no mesmo arquivo (`1 MM`); renomeado (`2 R.` com caminho
      original após NUL); conflito (`u UU`); untracked; `(detached)`; `(initial)` sem commits;
      sem upstream (sem `branch.ab`); upstream ausente/gone; caminhos com espaços e UTF-8.
- [ ] `discover`: diretório normal; subdiretório profundo; `.git` arquivo de worktree em `.worktrees/x`,
      `.max-code-worktrees/wt-1` e caminho arbitrário; submódulo (não é worktree); `gitdir` relativo; cwd fora de repo ⇒ `None`.
- [ ] `procnet`: IPv4 `0100007F:1F90` ⇒ `127.0.0.1:8080`; IPv6 `::1`, `::`, IPv4-mapeado; ignora estados ≠ `0A`;
      ignora linhas malformadas sem *panic*; benchmark de 10 000 linhas com meta < 5 ms em máquina registrada.
- [ ] Poller (com `tokio::time::pause`): aba `running` varre a cada 1 s; `idle` sem descendentes varre a cada 30 s;
      `idle` com descendente/porta conhecida varre a cada 5 s; transição `running → idle` dispara uma varredura final;
      diff não emite `opened` repetido para a mesma porta.
- [ ] Killer: recusa PID fora da árvore (`ports.not_tracked`), recusa PID raiz do shell
       (`ports.kill_root_refused`), detecta reuso de PID por `starttime` divergente (`ports.owner_gone`).
- [ ] UDP/UDP6 vinculado aparece com rótulo UDP e sem ação de navegador; mesma porta TCP/UDP ou em dois
      painéis mantém identidades distintas; socket herdado por dois donos exige selecionar o dono exato.
- [ ] TOCTOU: mudança de dono, inode, UID, namespace, geração de PTY ou `starttime` entre confirmação e
      sinal recusa a ação; PID reutilizado após a última leitura não é sinalizado graças ao pidfd.
- [ ] Encerramento não usa `killpg`, `fuser -k`, `pkill` ou busca por número de porta; shell, irmãos e
      processo externo na mesma porta permanecem vivos. Se o socket fechar durante a espera, não escalar.
- [ ] Watchers cobrem refs com `/`, rename atômico de index/HEAD e eventos durante status; resposta antiga
      após `cd`, fechamento ou troca de PTY não repõe badges; falha em `/proc` não emite fechamentos falsos.
- [ ] Abrir badge funciona com apenas fases 01–08, via plugin opener; IPv6 usa URL com colchetes quando
      necessário; bind exclusivo em IP de LAN não abre um serviço diferente em localhost.
- [ ] Worktree: `create` com nome inválido (`../x`, vazio, com `/`) ⇒ `git.invalid_name`; `remove` da worktree
      principal ⇒ `git.worktree_is_main`; `remove` com painel rastreado dentro ⇒ `git.worktree_in_use`, inclusive com `force`.

### 5.3. Comportamento observável (teste manual roteirizado, registrado no PR)
1. Abrir aba em `~/GitHub/MaxTerminal` ⇒ `GitIndicator` mostra `main` em ≤ 500 ms após o `cwd` ser conhecido.
2. `touch tmp.txt` na aba ⇒ após o fim do comando, contador de *untracked* = 1 em ≤ 1 s; `rm tmp.txt` ⇒ some.
3. Em **outro** terminal, `git checkout -b teste` no mesmo repo ⇒ a aba atualiza para `teste` em ≤ 1 s
      (watch de `HEAD`, sem interação na aba).
4. `cd .max-code-worktrees/wt-xxxx` ⇒ aparece `WorktreeBadge` com ícone/cor de MaxCode e nome `wt-xxxx`;
      `cd /tmp` ⇒ indicadores Git desaparecem.
5. `git checkout --detach` ⇒ indicador mostra SHA curto (7 caracteres) com estilo de HEAD destacado.
6. `npm run dev` (Vite) ⇒ `PortBadge` `:5173` (ou a porta usada) aparece em ≤ 2 s; clique abre o navegador
      padrão no endereço HTTP local compatível com o bind (`localhost`, `127.0.0.1` ou `[::1]`).
7. Clique em "Encerrar" no badge ⇒ `MaxPopoverConfirm` pede confirmação; ao confirmar o processo termina,
      após confirmar a saída, o badge some em ≤ 2 s; a Fase 05 determina o status. Socket compartilhado pode
      continuar no outro dono: o resultado reporta esse fato, sem matar outros processos.
8. Processo que ignora SIGTERM (`python3 -c "import signal,socket;signal.signal(15,signal.SIG_IGN);s=socket.socket();s.bind(('127.0.0.1',9123));s.listen();input()"`)
      ⇒ encerrado por SIGKILL após ~3 s; resultado `KillOutcome.killed` e toast "Processo encerrado à força".
9. `python3 -m http.server 9000 &` seguido de prompt ocioso ⇒ badge `:9000` permanece (polling de 5 s);
      `kill %1` ⇒ badge some em ≤ 6 s.
10. Com 20 abas ociosas sem portas/descendentes, observar no máximo uma reconciliação por aba a cada 30 s;
      sem PTYs vivos, nenhuma varredura. Medir CPU média < 0,5 % por 60 s na máquina de referência.
11. Repositório sem `git` instalado (simular com `PATH` sem git) ⇒ indicador mostra apenas a branch lida de
      `HEAD` com tooltip "Git não encontrado — status reduzido"; nenhum erro não tratado.
12. Repositório grande (ex.: clone do kernel Linux) ⇒ UI não trava; se `status` exceder 5 s, indicador
       mostra branch + ícone de relógio com tooltip "Status do Git excedeu o tempo limite".
13. Servidor iniciado com `&`, que só faz bind 10 s após voltar ao prompt, aparece em até 6 s após o bind
      se o descendente já foi detectado; reconciliação cobre perda de gatilho em até 31 s.
14. Criar worktree em caminho arbitrário e repo comum numa pasta chamada `.worktrees`: apenas a primeira
      recebe badge. Validar gitdir separado, principal bare, submódulo e `HEAD` no commondir.

### 5.4. Contratos para fases posteriores
- [ ] `mt_git::WorktreeManager` e os comandos `git_worktree_list/create/remove/prune` documentados com
      exemplos em doc-tests (`cargo test --doc -p mt-git`).
- [ ] `useGitStore().worktreeCreate()` / `worktreeList()` disponíveis e cobertos por Vitest (mock de IPC).
- [ ] `usePortsStore().portsForProject(projectId)` disponível (getter agregado para uso futuro).

---

## 6. Riscos e Mitigação

| Risco | Impacto | Mitigação |
|-------|---------|-----------|
| `git status` reescreve o `index` e dispara o watcher | CPU alta | `--no-optional-locks` + `GIT_OPTIONAL_LOCKS=0`; ignorar acesso/locks, coalescer por assinatura; não suprimir mudanças reais durante 500 ms |
| Repositório malicioso com `core.fsmonitor` executando comando arbitrário ao rodar `status` | Execução de código | Forçar `-c core.fsmonitor=false`; `GIT_TERMINAL_PROMPT=0`; nunca usar shell para montar comandos |
| `safe.directory` / "dubious ownership" | Status falha | Erro `git.unsafe_repository` com mensagem pt-BR orientando o usuário; nunca alterar config global |
| Repositórios enormes (status lento) | Indicador atrasado / CPU | Timeout de 5 s com kill do processo; semáforo global (4); single-flight por repo; resultado parcial com branch por arquivo |
| Alterações no working tree não geram eventos em `.git/` | Dirty desatualizado | Refresh no fim de cada comando (OSC 133 `D`), no foco e reconciliação de 30 s visível/120 s oculta |
| Limite de inotify (`max_user_watches`) | Watch falha | Watch não recursivo apenas de arquivos-chave e `refs/heads`; em erro, cair para polling de 5 s do repo afetado e logar `warn` |
| Reuso de PID entre varredura e `port_kill` | Matar processo errado | Revalidar PTY/UID/namespace/inode/starttime e sinalizar por pidfd estável; indisponibilidade ⇒ ação desabilitada |
| Processo em outro *network namespace* (Docker) | Porta não detectada | Limitação documentada no tooltip de ajuda; fora do escopo do AGENTS.md |
| Varredura de `/proc` cara com muitas abas | CPU | *Poller* único lê as quatro tabelas uma vez por tick para todas as abas devidas; só lê `fd/` das árvores; sem trabalho sem PTY vivo |
| Navegador padrão/opener indisponível | Botão não funciona | Erro `ports.opener_failed` com toast pt-BR; adaptador/plugin pertence à 08, sem dependência da 09 |
| Matar o próprio shell da aba ao encerrar porta | Aba morre | Recusar PID raiz do PTY (`ports.kill_root_refused`) |
| Contratos da Fase 02/07 com nomes divergentes | Retrabalho | Consumo isolado em adaptadores (`adapters/process_tree.rs`, `useTabIndicatorsSync`) — única superfície a ajustar |
