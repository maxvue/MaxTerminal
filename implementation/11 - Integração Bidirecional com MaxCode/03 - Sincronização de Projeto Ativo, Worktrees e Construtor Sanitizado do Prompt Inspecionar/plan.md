# Implementação 03 — Sincronização de Projeto Ativo, Worktrees e Construtor Sanitizado do Prompt Inspecionar (`plan.md`)

> Especificação técnica desta unidade: [`./spec.md`](./spec.md).
> Fase 11: [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md).

---

## 1. Checklist Sequencial de Execução

- [ ] **Passo 1: Implementar `paths.rs` com Validação Real de Worktrees Git e Testes Unitários**
  1. Criar `src-tauri/crates/mt-bridge/src/paths.rs` implementando `canonicalize_project_path`, `resolve_worktree_root`, `resolve_worktree_root_cached` (cache LRU de 128 entradas com TTL de 30 s + `spawn_blocking`), `match_project` e `validate_cwd_within_project`.
  2. Adicionar testes unitários em `src/paths.rs` cobrindo:
     - Repositório padrão com diretório `.git`.
     - Worktree em `.max-code-worktrees/wt-1` e `.worktrees/wt-2` com arquivo `.git` (`gitdir: ...`), `commondir` relativo (`../..`) e arquivo reverso `<gitdir>/gitdir`.
     - Worktree externa fora da árvore do repositório principal.
     - Rejeição de pasta chamada `.max-code-worktrees/falsa` cujo `.git` aponta para outro repositório sem vínculo reverso.
     - Proteção contra prefixo parcial de string (`/tmp/projeto-malicioso` não faz match com `/tmp/projeto`).
     - Rejeição de `cwd` que escapa do projeto via `..` ou symlink (`BridgeError::CwdOutsideProject`).

- [ ] **Passo 2: Implementar `prompt.rs` com Sanitização Dupla e Testes Unitários**
  1. Criar `src-tauri/crates/mt-bridge/src/prompt.rs` implementando `clean_terminal_text`, `truncate_utf8_suffix`, `truncate_utf8_prefix` e `build_inspect_prompt`.
  2. Adicionar testes unitários em `src/prompt.rs` cobrindo:
     - Caso exato do **CA-29**: `ls /naoexiste` (exit 2) produzindo `Ao executar o comando "ls /naoexiste" tive o seguinte erro: "ls: não foi possível acessar '/naoexiste': Arquivo ou diretório inexistente"`.
     - Remoção de sequências ANSI CSI/OSC e retorno de carro `\r` de barras de progresso (**CA-31**).
     - Limite das últimas 80 linhas e 6000 caracteres em `${erro}` (com prefixo `[…] `) e 2000 caracteres em `${comando}` (**CA-31**), incluindo caracteres multibyte UTF-8 (acentos e emojis) na fronteira exata de corte.
     - Saída vazia ou apenas espaços gerando `código de saída <N>`.
     - Segredos (`Bearer abc...`, `APP_KEY=base64:...`) posicionados na fronteira do corte sendo integralmente mascarados antes do truncamento (**CA-30**).
     - Preservação de aspas internas sem escape artificial no template.

- [ ] **Passo 3: Implementar `sync.rs` (`LoopGuard` + `ActiveProjectSync`) com Testes Unitários**
  1. Criar `src-tauri/crates/mt-bridge/src/sync.rs` implementando `LoopGuard` e `ActiveProjectSync`.
  2. Adicionar testes unitários em `src/sync.rs` cobrindo:
     - Foco remoto (`origin: PeerApp::Maxcode`) aplicado e confirmado sem ecoar `project.focus`, mesmo após espera de 1600 ms ou reconexão (**CA-25**).
     - Seleção manual local (`origin: PeerApp::Maxterminal`) do mesmo projeto logo após uma seleção remota enviando `project.focus` normalmente (**CA-25**).
     - Debounce de 250 ms colapsando 5 seleções manuais rápidas em exatamente 1 mensagem `project.focus` (**CA-25**).
     - Desativação a quente (`set_enabled(false)`) cancelando imediatamente o timer de debounce pendente e recusando `on_remote_opened` (**CA-27**).
     - Reconhecimento de `correlation_id` próprio sem reaplicar foco.

- [ ] **Passo 4: Integrar `paths`, `sync` e `TerminalOpenTab` em `server.rs`, `lib.rs` e `tests/server_it.rs`**
  1. Atualizar `src-tauri/crates/mt-bridge/src/server.rs` para processar `Message::ProjectOpened`, `Message::ProjectClosed` e `Message::TerminalOpenTab` (validando projeto e `cwd` via `paths.rs` e invocando `BridgeHost::open_tab` sob reserva do `Deduplicator`).
  2. Atualizar `src-tauri/crates/mt-bridge/src/lib.rs` exportando `paths`, `prompt` e `sync`, e adicionando `BridgeHandle::sync(&self)` e `BridgeHandle::confirm_active_project`.
  3. Expandir `src-tauri/crates/mt-bridge/tests/server_it.rs` para validar ponta a ponta:
     - Peer envia `project.opened { path: "<repo>/.max-code-worktrees/wt-x" }` → `MockHost::focus_project` recebe `project.path == <repo>` e `worktree_path == Some("<repo>/.max-code-worktrees/wt-x")` (**CA-24**).
     - Reenvio da mesma mensagem `project.opened` (mesmo `id`) retorna o mesmo `ack` sem chamar `focus_project` uma segunda vez (**CA-25**).
     - `project.opened` com caminho não cadastrado retorna `ack { handled: false, reason: Some("project_not_found") }` (**CA-26**).

---

## 2. Comandos Exatos de Verificação

```bash
# 1. Lint estrito do crate mt-bridge completo (CA-01)
cargo clippy --manifest-path src-tauri/Cargo.toml -p mt-bridge --all-targets -- -D warnings

# 2. Execução de todos os testes unitários e de integração do mt-bridge (CA-01)
cargo test --manifest-path src-tauri/Cargo.toml -p mt-bridge

# 3. Verificação de cobertura >= 85% nos módulos exigidos pelo CA-05 (quando cargo-llvm-cov disponível)
cargo llvm-cov --manifest-path src-tauri/Cargo.toml -p mt-bridge --summary-only
```

---

## 3. Critérios de Aceite Mapeados (`CA-*`)

| Critério | Descrição | Como é verificado nesta Implementação |
|----------|-----------|---------------------------------------|
| **CA-05** | Cobertura ≥ 85% em `protocol.rs`, `auth.rs`, `uri.rs`, `prompt.rs`, `sync.rs`, `paths.rs`. | Validado pelos testes unitários de `prompt.rs`, `sync.rs`, `paths.rs` e suíte `mt-bridge`. |
| **CA-24** | `project.opened` em worktree seleciona projeto raiz e preenche `worktree_path`. | Validado em `src/paths.rs`, `src/sync.rs` e `tests/server_it.rs`. |
| **CA-25** | Debounce 250 ms na seleção manual; seleção remota não ecoa mesmo após 1500 ms; dedup idempotente. | Validado em `src/sync.rs` e `tests/server_it.rs`. |
| **CA-26** | Caminho sem projeto correspondente retorna `ack { handled: false, reason: 'project_not_found' }`. | Validado em `src/sync.rs` e `tests/server_it.rs`. |
| **CA-27** | Desligar `sync_active_project` interrompe envio e recebimento imediatamente. | Validado nos testes de `ActiveProjectSync::set_enabled(false)`. |
| **CA-29** | Formato exato do prompt para `ls /naoexiste` (exit 2). | Validado no teste unitário dedicado em `src/prompt.rs`. |
| **CA-30** | Segredos no comando/saída aparecem mascarados no prompt. | Validado nos testes de `src/prompt.rs`. |
| **CA-31** | Remoção de ANSI/CR e truncamento (80 linhas / 6000 chars em `${erro}`, 2000 chars em `${comando}`). | Validado nos testes de `src/prompt.rs`. |
