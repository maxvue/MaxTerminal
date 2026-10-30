# Implementação 04 — Sincronizador MaxSkills e Memória Contextual de Comandos (`plan.md`)

> Especificação técnica: [`./spec.md`](./spec.md).

---

## 1. Checklist Sequencial de Execução

- [ ] **Passo 1 — Implementar Catálogo e Hash de Árvore de Skills (`crates/mt-agents/src/skills/catalog.rs`)**
  1. Implementar `compute_skill_tree_hash` com ordenação determinística de caminhos relativos, verificação de limites (`MAX_SKILL_DEPTH = 12`, `MAX_SKILL_FILES = 10_000`, `MAX_SKILL_BYTES = 64 MiB`) e bloqueio de symlinks externos ou cíclicos.
  2. Implementar leitura de `index.json` (com validação de schema e diagnóstico quando desconhecido) e fallback por varredura de `**/SKILL.md` (ignorando `.git`, `node_modules` e worktrees, extraindo frontmatter YAML de `name`, `description`, `tags`, `stacks` e detectando nomes duplicados).
  3. Implementar cálculo de `SkillRelevance` (`Recommended` quando `stacks` intersecta o `StackKind` do projeto, `Neutral` caso contrário).

- [ ] **Passo 2 — Implementar Resolução de Alvos e Classificação de Conflitos (`targets.rs` e `conflict.rs`)**
  1. Implementar `agent_skill_dir` para Claude (`.claude/skills`), Codex (`.agents/skills`), OpenCode V2 (`.opencode/skills` e `$XDG_CONFIG_HOME/opencode/skills`) e `agy` (exigindo `dest_override` explícito).
  2. Implementar validação de `project_root` (incluindo worktrees do mesmo repositório), detecção de symlinks ancestrais via `symlink_metadata`, deduplicação por `dest_canonical` e recusa de escape para fora do escopo.
  3. Implementar `conflict.rs` classificando cada entrada como `Create`, `Update`, `Unchanged`, `Conflict` (`ForeignSymlink`, `Unmanaged`, `LocallyModified`) ou `Prune` (garantindo que itens modificados localmente virem `Conflict` em vez de `Prune`).

- [ ] **Passo 3 — Implementar `skills_plan`, `skills_apply` e Watcher (`sync.rs` e `watch.rs`)**
  1. Implementar `SkillsService::plan` (100% *dry-run*, sem criar pastas ou backups, armazenando o plano em memória por 5 min com `fingerprint` do estado da fonte, destino e banco).
  2. Implementar `SkillsService::apply` validando o `fingerprint` sob lock do destino (`skills.plan_stale`), executando resoluções `Keep`, `Adopt` (somente quando o hash/alvo coincide com a fonte) e `Overwrite` (criando backup completo em `$XDG_STATE_HOME/maxterminal/skills-backup/`), operando via staging irmão + rename atômico e gravando `.maxterminal-skill.json` apenas no modo `Copy`.
  3. Implementar `watch.rs` com `notify-debouncer-full` (debounce 500 ms) emitindo `skills://changed` em ≤ 2 s e acionando `auto_sync` somente para ações sem conflito.

- [ ] **Passo 4 — Implementar `CommandMemory` (`crates/mt-agents/src/memory.rs`)**
  1. Implementar `CommandMemory::normalize`, `command_hash` e `score` conforme as regras de privacidade, sanitização (`[REDACTED:` rejeitado), preservação de espaços internos/aspas e fórmula de *frecency*.
  2. Implementar `process_block`, `on_command_finished` e `reconcile_unprocessed_on_startup` usando transação atômica com `command_memory_processed` e `command_memory_preferences`, além de poda automática para 2 000 entradas por projeto (preservando `pinned`).
  3. Implementar `suggestions` (com escape de `\`, `%`, `_` para `LIKE ... ESCAPE '\'`), `list`, `set_pinned` (limite de 100 → `memory.pin_limit`), `set_hidden`, `forget` (gravando tombstone `forgotten = 1`) e `rebuild`.

- [ ] **Passo 5 — Integrar `AgentService` e Expor Comandos IPC (`src-tauri/src/commands/{skills,memory}.rs`)**
  1. Criar `src-tauri/src/commands/skills.rs` (`skills_scan`, `skills_targets_list`, `skills_target_upsert`, `skills_target_remove`, `skills_plan`, `skills_apply`) e `src-tauri/src/commands/memory.rs` (`memory_suggestions`, `memory_list`, `memory_pin`, `memory_hide`, `memory_forget`, `memory_rebuild`).
  2. Registrar os comandos e eventos (`skills://changed`, `skills://sync-completed`, `memory://updated`) em `src-tauri/src/specta.rs` e `src-tauri/src/lib.rs`, conectando o listener de `ShellService::subscribe()` ao `CommandMemory::on_command_finished`.

- [ ] **Passo 6 — Escrever Testes de Integração e Cobertura (`tests/skills_sync.rs` e `tests/memory.rs`)**
  1. Em `tests/skills_sync.rs`: testar equivalência entre `index.json` e varredura de `SKILL.md` (CA-26), *dry-run* sem alterar hash do disco (CA-27), sync em `symlink` e `copy` idempotente (CA-28), conflitos `ForeignSymlink`, `Unmanaged` e `LocallyModified` + backup em `Overwrite` (CA-29), evento `skills://changed` + `auto_sync` em ≤ 2 s (CA-30) e sync em worktree / bloqueio de symlink ancestral / rejeição de plano vencido (CA-35).
  2. Em `tests/memory.rs`: testar `npm run build` 3x vs `ls` 1x com prefixo `'np'`, rejeição de `[REDACTED:...]` e `memory_hide` (CA-31), equivalência exata entre incremental e `memory_rebuild` (CA-32), replay duplicado de `CommandFinished`, persistência de `forget` após `rebuild` e distinção de espaços entre aspas (CA-33).
  3. Verificar cobertura de linhas ≥ 80% em `telemetry/`, `attention/`, `skills/` e `memory.rs` com `cargo llvm-cov -p mt-agents` (CA-02).

---

## 2. Comandos de Verificação e Testes

Executar na raiz da worktree (`/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-6f18b3d0`):

```bash
# 1. Formatação e lint de todo o workspace Rust
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings

# 2. Executar todas as suítes de testes do crate mt-agents
cargo test --manifest-path src-tauri/Cargo.toml -p mt-agents

# 3. Verificar cobertura >= 80% nos módulos críticos (CA-02)
cargo llvm-cov --manifest-path src-tauri/Cargo.toml -p mt-agents --summary-only
```

---

## 3. Critérios de Aceite Atendidos nesta Implementação

- **CA-02**: `cargo test -p mt-agents` passa com todas as suítes e cobertura de linhas ≥ 80% em `telemetry/`, `attention/`, `skills/` e `memory.rs`.
- **CA-26**: `skills_scan` lista as skills de `index.json` com nome, descrição pt-BR e relevância por projeto; sem `index.json`, a varredura de `SKILL.md` produz o mesmo conjunto.
- **CA-27**: `skills_plan` (*dry-run*) não altera o disco e lista ações `create`/`update`/`unchanged`/`conflict`/`prune`.
- **CA-28**: Sync em modo `symlink` cria links absolutos por skill; em modo `copy` grava `.maxterminal-skill.json` com hash; re-executar é idempotente (0 ações).
- **CA-29**: Destino symlink externo gera `ForeignSymlink`, diretório sem manifesto gera `Unmanaged` e cópia editada localmente gera `LocallyModified`; nenhum é sobrescrito sem `Overwrite`, e `Overwrite` cria backup em `$XDG_STATE_HOME/maxterminal/skills-backup/`.
- **CA-30**: Criar/editar um `SKILL.md` na fonte dispara `skills://changed` em ≤ 2 s e atualiza alvos com `auto_sync` sem conflito.
- **CA-31**: Após executar `npm run build` 3x e `ls` 1x, `memory_suggestions(project, 'np')` retorna `npm run build` em 1º lugar com `use_count = 3`; comandos com `[REDACTED:...]` nunca aparecem; `memory_hide` remove da sugestão e persiste.
- **CA-32**: `memory_rebuild` reconstrói a tabela a partir de `command_blocks` com resultado idêntico ao incremental.
- **CA-33**: Replay duplicado de `CommandFinished` não aumenta contadores; `memory_forget` continua respeitado após `rebuild`/reinício; comandos com aspas/espaços internos distintos não são fundidos.
- **CA-35**: Sync em worktree usa o destino dessa worktree; symlink ancestral externo não é seguido sem autorização; plano vencido retorna `skills.plan_stale`; `prune` preserva arquivos modificados localmente; fonte ausente mantém cópias existentes.
