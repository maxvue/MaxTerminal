# Plano de Execução — Implementação 01: Crate `mt-git`

> **Fase:** 08 — Indicadores Contextuais: Git, Worktree e Portas Ativas
> **Unidade:** `01 - Crate mt-git Descoberta Status Cache Watcher e Worktrees`
> **Especificação:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

### Passo 1 — Estrutura do Crate `mt-git`, Tipos e Erros
1. Registrar `"crates/mt-git"` em `src-tauri/Cargo.toml` (`[workspace].members` e `[workspace.dependencies]`).
2. Criar `src-tauri/crates/mt-git/Cargo.toml` com todas as dependências listadas em `spec.md` §3.1.
3. Implementar `src-tauri/crates/mt-git/src/types.rs` com `RepoKey`, `GitContext`, `GitBranch`, `GitChanges`, `GitUpstream`, `WorktreeKind`, `GitWorktree`, `DegradedReason`, `WorktreeEntry`, `WorktreeCreateRequest`, `WorktreeLocation`, `RefreshReason` e `GitStatusChanged`, todos com `#[derive(Serialize, Deserialize, specta::Type)]` e `#[serde(rename_all = "camelCase")]`.
4. Implementar `src-tauri/crates/mt-git/src/error.rs` com `GitError`, mapeamento dos 16 códigos `git.*` e conversão `From<GitError> for mt_core::MtError`.

### Passo 2 — Descoberta de Repositório, Worktree Vinculada e Leitura de `HEAD` (`discover.rs`)
1. Implementar `discover(cwd: &Path) -> Result<Option<RepoLocation>, GitError>`:
   - Canonicalização via `dunce::canonicalize`, suporte a `GIT_CEILING_DIRECTORIES` e limite de 128 ancestrais.
   - Leitura de `.git` diretório vs arquivo `gitdir: <path>`.
   - Resolução de `commondir` e comprovação estrita de worktree vinculada: `git_dir != common_dir`, entrada sob `<common_dir>/worktrees/<name>` e verificação do vínculo reverso `<git_dir>/gitdir` apontando de volta para `<work_tree>/.git`.
   - Garantir que submódulos (`.git/modules/...`), `--separate-git-dir` e repositórios comuns dentro de pastas chamadas `.worktrees` ou `.max-code-worktrees` tenham `worktree: None`.
2. Implementar `classify_worktree(work_tree: &Path) -> WorktreeKind` (`.max-code-worktrees` ⇒ `MaxCode`, `.worktrees` ⇒ `Project`, senão `Other`).
3. Implementar `read_head(location: &RepoLocation) -> Result<GitBranch, GitError>` resolvendo refs simbólicas, refs com `/` (ex.: `refs/heads/feature/foo`), `packed-refs`, estado `Detached` (short_oid de 7 caracteres), `Unborn` e detecção de `reftable`.
4. Implementar `confirm_with_git(runner, provisional)` usando `rev-parse --absolute-git-dir`, `rev-parse --path-format=absolute --git-common-dir`, `rev-parse --show-toplevel` e `worktree list --porcelain -z` para preencher `main_work_tree`.

### Passo 3 — Parser Porcelain v2 (`porcelain.rs`) e Fixtures Binárias
1. Implementar `parse(bytes: &[u8]) -> Result<PorcelainStatus, GitError>` processando registros separados por `\0`:
   - Cabeçalhos `# branch.oid`, `# branch.head`, `# branch.upstream`, `# branch.ab`.
   - Registros `1 XY sub ...` e `2 XY sub ...` (consumindo o próximo registro NUL com o caminho original em `2`).
   - Tratamento de submódulos `S<c><m><u>` contabilizando `unstaged` no máximo uma vez por registro.
   - Registros `u` (`conflicted`), `?` (`untracked`) e `!` (ignorado).
2. Gerar/gravar as 9 fixtures binárias em `src-tauri/crates/mt-git/tests/fixtures/porcelain/` (`clean.bin`, `mixed.bin`, `rename.bin`, `conflict.bin`, `detached.bin`, `initial.bin`, `no_upstream.bin`, `gone.bin`, `utf8_spaces.bin`) e escrever os testes unitários e `proptest` em `porcelain.rs`.

### Passo 4 — Executor Seguro `CliGitRunner` (`runner.rs`)
1. Implementar a trait `GitRunner` e a struct `CliGitRunner`.
2. Garantir limpeza de variáveis `GIT_*` herdadas (incluindo pares `GIT_CONFIG_COUNT`/`GIT_CONFIG_KEY_*`/`GIT_CONFIG_VALUE_*`), injeção de `GIT_OPTIONAL_LOCKS=0`, `GIT_TERMINAL_PROMPT=0`, `LC_ALL=C`, `-c core.fsmonitor=false`, `--no-optional-locks`.
3. Implementar drenagem limitada a 16 MiB (`GitError::OutputLimit`) e timeout de 5 s com `child.kill().await` + `child.wait().await` (`GitError::Timeout`), além de detecção de `dubious ownership` (`GitError::UnsafeRepository`).
4. Implementar um `FakeGitRunner` para testes determinísticos de concorrência, timeout e falhas.

### Passo 5 — Watcher Inotify com Debounce e Fallback (`watcher.rs`)
1. Implementar `RepoWatcher` usando `notify::RecommendedWatcher` em modo `RecursiveMode::NonRecursive`.
2. Observar `git_dir`, `common_dir`, `refs/heads`, diretórios pais de refs com `/` (ex.: `refs/heads/feature/foo`) e `refs/remotes/<upstream>`.
3. Filtrar eventos `Access`, arquivos `*.lock` e `ORIG_HEAD`.
4. Implementar coordenador de debounce por repositório (300 ms *trailing*, teto máximo de 1 s) e fallback para polling de 5 s com `DegradedReason::WatchUnavailable` caso `notify` retorne erro (`ENOSPC`).

### Passo 6 — Cache, Single-Flight e `GitService` (`cache.rs`, `lib.rs`)
1. Implementar `RepoCache` / `RepoEntry` e `GitService` com `Semaphore::new(4)`, `DashMap<PaneId, PaneBinding>`, `epoch`, `revision` monotônica e `generation` por binding.
2. Implementar `track_pane`, `untrack_pane`, `untrack_pane_if_matches`, `set_visible`, `request_refresh`, `status_once`, `panes_inside`, `reserve_cwd` assíncrono e `has_active_or_reserved_inside`. Exportar `CwdReservationGuard: Send + 'static` (token proprietário, Drop síncrono). Usar `cwd_gate` conforme `spec.md` §3.8: leitura para reserva/instalação do binding, mutex de `common_dir` → escrita para remoção/prune, sem manter locks no token durante spawn.
3. Garantir single-flight com flag `dirty_again` (se um evento ocorrer enquanto `status` estiver rodando, reexecuta ao terminar).
4. Implementar expiração de cache em 60 s após `refcount == 0` e loop de reconciliação periódica (30 s para repos com aba visível, 120 s para repos ocultos com painel vivo, pausado sem bindings).

### Passo 7 — API de Worktrees (`worktree.rs`) e Testes de Integração
1. Implementar `WorktreeManager` e `CliWorktreeManager` (`list`, `create`, `remove`, `prune`).
2. Adicionar doc-test em `WorktreeManager` mostrando uso de `WorktreeCreateRequest` com `WorktreeLocation::MaxCode`.
3. Criar `tests/discover.rs`, `tests/watcher.rs` e `tests/worktree.rs` cobrindo todos os cenários obrigatórios.
4. Em `tests/worktree.rs`, usar barreiras/notificações (sem sleeps arbitrários) para testar reserva × remoção: reserva já inserida recusa `remove(force=true)`; remoção já iniciada bloqueia reserva até reap e depois falha na revalidação do cwd; registrar binding antes de Drop mantém proteção; Drop após falha sem PTY libera token. Cobrir subdiretório, alias e repositório aninhado, rollback condicional por `PtyId` e cancelamento de subprocesso sem liberar a escrita prematuramente.

---

## 2. Comandos de Verificação e Testes

Execute os comandos abaixo na worktree ativa:

```bash
cd "$(git rev-parse --show-toplevel)"
cd src-tauri

# 1. Verificar compilação e tipos do crate mt-git
cargo check -p mt-git --all-targets

# 2. Verificar lint estrito sem warnings
cargo clippy -p mt-git --all-targets -- -D warnings

# 3. Executar testes unitários (porcelain, runner, cache) e de integração (discover, watcher, worktree)
cargo test -p mt-git

# 4. Executar doc-tests da API pública de WorktreeManager (contrato para Fases 13 e 16)
cargo test --doc -p mt-git
```

---

## 3. Critérios de Aceite da Implementação 01 (Mapeados para a Fase 08)

- [ ] **CA-08-01 (`porcelain`):** Parser passa em todas as 9 fixtures (`clean`, `mixed` com `1 MM`, `rename` `2 R.` consumindo caminho original após NUL, `conflict` `u UU`, `untracked`, `detached`, `initial` sem commits, `no_upstream`, `gone`, `utf8_spaces`) e no property test sem *panic*.
- [ ] **CA-08-02 (`discover`):** Detecta diretório normal, subdiretório profundo, worktrees em `.worktrees/x`, `.max-code-worktrees/wt-1` e caminho arbitrário (`Other`); **não** classifica submódulo, `--separate-git-dir` nem repo comum dentro de pasta `.worktrees` como worktree vinculada; retorna `None` fora de repo e em repo bare.
- [ ] **CA-08-03 (`runner` & `cache`):** Single-flight coalesce 10 chamadas concorrentes de `request_refresh` em 1 execução (+ no máximo 1 reexecução se `dirty_again`); timeout de 5 s mata o filho e marca `DegradedReason::Timeout`; ausência de `git` produz `DegradedReason::GitNotFound` mantendo a branch lida de `HEAD`.
- [ ] **CA-08-04 (`watcher`):** Escrita em `HEAD` ou ref com `/` (`refs/heads/feature/foo`) dispara 1 evento após debounce (~300 ms); escrita em `index.lock` ou `ORIG_HEAD` é ignorada; 50 escritas em 200 ms respeitam o teto de 1 s.
- [ ] **CA-08-05 (`worktree`):** `create` com nome inválido (`../x`, vazio, com `/`) falha com `git.invalid_name`; `remove` da principal falha com `git.worktree_is_main`; `remove` com painel rastreado ou `cwd` reservado falha com `git.worktree_in_use` inclusive com `force = true`; `.gitignore` nunca é alterado na worktree principal; doc-test de `WorktreeManager` passa.
- [ ] **CA-08-06 (`clippy`):** `cargo clippy -p mt-git --all-targets -- -D warnings` termina com código 0.
