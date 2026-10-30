# Plano de Execução — Implementação 02: Crate `mt-shell` — Parser Incremental OSC 133/633, Scripts de Hook e Injeção sem Dotfiles

> **Fase:** 05 — Shell Integration OSC 133 e Máquina de Estados das Abas
> **Implementação:** `02 - Parser Incremental OSC 133 e Injeção de Hooks nos Shells`
> **Especificação de referência:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

- [ ] **Passo 1: Criar o crate `mt-shell` e implementar `kind.rs` (`ShellKind` e `Nonce128`)**
  1. Criar `src-tauri/crates/mt-shell/Cargo.toml` e adicionar `"crates/mt-shell"` ao `[workspace].members` em `src-tauri/Cargo.toml`.
  2. Implementar `src-tauri/crates/mt-shell/src/kind.rs` com `ShellKind::detect`, `Nonce128::generate` (usando `rand::rngs::OsRng` ou `rand::thread_rng()`, comparação em tempo constante e `Debug` redigido) e `check_args_compatibility`.
  3. Escrever testes unitários em `kind.rs` cobrindo detecção de caminhos (`/bin/bash`, `/usr/bin/zsh`, `/usr/bin/fish`, `/bin/sh`, `nu`), rejeição de `-c`, `--norc`, `--no-rcs`, `--no-config` e scripts posicionais, e aceitação de `-i`, `-l`, `--login`.

- [ ] **Passo 2: Implementar o Codec de Escaping (`osc/escape.rs`) e o Parser Incremental (`osc/scanner.rs`)**
  1. Implementar `src-tauri/crates/mt-shell/src/osc/escape.rs` com `unescape_osc_field`, `escape_osc_field` e `validate_cwd_path`, respeitando os tetos de 32 KiB (OSC), 16 KiB (comando) e 8 KiB (`cwd`).
  2. Implementar `src-tauri/crates/mt-shell/src/osc/scanner.rs` e `src-tauri/crates/mt-shell/src/osc/mod.rs` com `OscScanner`, `ScanState` (`Ground`, `Esc`, `OscBody`, `OscEsc`, `DiscardOsc`, `DiscardOscEsc`, `IgnoredString`, `IgnoredStringEsc`, `Csi`), `ShellMark`, `ScannedMark` e `ScanMetrics`.
  3. Garantir fast-path `memchr` em `Ground`, tratamento de `BEL` e `ESC \` (inclusive quando `ESC` é o último byte do chunk `N` e `\` é o primeiro byte do chunk `N+1`), cancelamento por `CAN`/`SUB`, ressincronização quando `ESC` dentro do OSC inicia outra sequência, e cálculo preciso de `StreamPosition { frame_seq, byte_offset }`.
  4. Garantir validação estrita da gramática fechada (`633;P;MT=1`, `633;E`, `633;P;Cwd`, `133;A/B/C/D`), rejeitando chaves duplicadas ou extras e preservando `exit_code: None` quando `D` não traz código.

- [ ] **Passo 3: Criar a Suíte de Testes do Scanner (`tests/scanner_split.rs`) e Benchmark (`benches/osc_scan.rs`)**
  1. Criar `src-tauri/crates/mt-shell/tests/scanner_split.rs` testando:
     - Todas as partições possíveis em 2 e 3 chunks de um corpus cobrindo `BEL`, `ST`, `ESC` partido no final do chunk, UTF-8 multibyte partido dentro do comando/cwd, múltiplos comandos no mesmo chunk, OSC desconhecido (`OSC 0`, `OSC 8`, `OSC 52`), `ESC ]` falso dentro de `DCS`/`APC`, `CAN`/`SUB`, overflow > 32 KiB e `finish()` no EOF.
     - Verificação de que todas as partições produzem exatamente o mesmo vetor de `ScannedMark` e os mesmos `StreamPosition`.
     - Property tests com `proptest` (`ProptestConfig::with_cases(2500)`) gerando sequências arbitrárias de bytes e partições aleatórias, garantindo zero panics e `scanner.buffer.capacity() <= 32768`.
  2. Criar `src-tauri/crates/mt-shell/benches/osc_scan.rs` com `criterion` medindo:
     - Stream puro ASCII/ANSI sem OSC (meta ≥ 1 GiB/s).
     - Stream misto de prompts e comandos com OSC 133/633 e SGR (meta ≥ 300 MiB/s).

- [ ] **Passo 4: Escrever os Scripts de Hook para `bash`, `zsh` e `fish` e o Injetor (`inject.rs`)**
  1. Criar `src-tauri/crates/mt-shell/scripts/bash/mt-integration.bash` seguindo todas as regras da §3.5.A da `spec.md`.
  2. Criar `src-tauri/crates/mt-shell/scripts/zsh/{.zshenv,.zprofile,.zshrc,.zlogin,mt-integration.zsh}` seguindo todas as regras da §3.5.B da `spec.md`.
  3. Criar `src-tauri/crates/mt-shell/scripts/fish/mt-integration.fish` seguindo todas as regras da §3.5.C da `spec.md`.
  4. Implementar `src-tauri/crates/mt-shell/src/inject.rs` com `ShellInjector::ensure_scripts` (materialização atômica em `$XDG_RUNTIME_DIR/maxterminal/shell-integration/<sha256>/`, permissões `0700`/`0600`, verificação de UID e symlinks) e `ShellInjector::inject`.

- [ ] **Passo 5: Criar os Testes de Injeção e de Shells Reais (`bash`, `zsh`, `fish`)**
  1. Criar `src-tauri/crates/mt-shell/tests/injection_no_dotfile_changes.rs`:
     - Criar diretório `$HOME` temporário com `.bashrc`, `.bash_profile`, `.zshenv`, `.zshrc`, `.config/fish/config.fish` preexistentes, calcular o SHA-256 de toda a árvore antes e depois do ciclo de vida do shell e comprovar que nenhum arquivo foi alterado ou criado em `$HOME`.
     - Testar opt-out (`enabled = false` e `MAXTERMINAL_DISABLE_SHELL_INTEGRATION=1`), flags incompatíveis e falha de diretório runtime (`scripts_unavailable`).
  2. Criar `src-tauri/crates/mt-shell/tests/shells_bash.rs`, `shells_zsh.rs` e `shells_fish.rs` executando processos reais em PTY (com skip explícito e logado caso o binário do shell não exista no ambiente local, exigindo os 3 no CI):
     - Handshake `633;P;MT=1`, `true` (`D;0`), `false` (`D;1`), `(exit 42)` / `fish -c "exit 42"` (`D;42`), `cd /tmp` (atualização de `Cwd`), comando multiline, pipeline (`false | true` e `true | false`), função shell, builtin, `Ctrl+C`, Enter vazio (não gera C/D), comando com `;`, `\`, `BEL`, `ESC` e caracteres Unicode/emoji.
     - Verificar que `env | grep MT_SHELL_NONCE` dentro do shell retorna vazio (nonce não herdado por processos filhos).
     - No `shells_bash.rs`: testar `HISTCONTROL=ignorespace` (comando iniciado com espaço) e `set +o history`, comprovando que `q=unavailable` é emitido e que o comando anterior do histórico **jamais** é reutilizado.
     - No `shells_zsh.rs`: testar coexistência com função `precmd()` preexistente, `ZDOTDIR` customizado pelo usuário e tema que redefine `PROMPT`.

---

## 2. Comandos de Verificação e Validação

```bash
# 1. Compilar mt-shell e seus alvos de teste/benchmark
cargo check -p mt-shell --all-targets

# 2. Executar testes unitários, split de chunks e proptest (≥ 2.000 casos)
cargo test -p mt-shell --test scanner_split -- --nocapture

# 3. Executar testes de injeção sem alteração de dotfiles e shells reais (bash, zsh, fish)
cargo test -p mt-shell --test injection_no_dotfile_changes -- --nocapture
cargo test -p mt-shell --test shells_bash -- --nocapture
cargo test -p mt-shell --test shells_zsh -- --nocapture
cargo test -p mt-shell --test shells_fish -- --nocapture

# 4. Executar benchmark do scanner OSC (validar ≥ 1 GiB/s sem ESC e ≥ 300 MiB/s com OSC/ANSI)
cargo bench -p mt-shell --bench osc_scan -- --warm-up-time 1 --measurement-time 2

# 5. Validar clippy sem warnings
cargo clippy -p mt-shell --all-targets -- -D warnings
```

---

## 3. Critérios de Aceite desta Implementação (Mapeamento Fase 05)

- [x] **CA-PAR-01 (instructions.md §5):** Corpus BEL/ST, ESC/ST partidos, UTF-8 partido, múltiplos comandos no mesmo chunk, OSC desconhecido, CSI/DCS, CAN/SUB, overflow e EOF produzem as mesmas marcas e offsets em todas as divisões de 2 e 3 chunks; stream bruto permanece byte a byte idêntico.
- [x] **CA-PAR-02 (instructions.md §5):** `proptest` (≥ 2.000 casos) sem panic ou alocação ilimitada; OSC > 32 KiB descartado e ressincronizado no terminador ou `CAN`/`SUB`.
- [x] **CA-SHL-01 (instructions.md §5):** `bash`, `zsh` e `fish` executam `true`, `false`, `(exit 42)`, `cd`, multiline, pipeline, função, builtin, `exec`, `Ctrl+C`, prompt vazio e entrada com `;`, `\`, BEL, ESC e Unicode.
- [x] **CA-SHL-02 (instructions.md §5):** Comando completo só é anunciado quando disponível; bash com histórico desabilitado/filtrado (`ignorespace`/`set +o history`) não atribui o comando anterior nem usa `$BASH_COMMAND` como linha completa.
- [x] **CA-SHL-03 (instructions.md §5):** `$?`/`$status` capturado na primeira linha antes de helpers; status visto por hooks do usuário preservado; DEBUG/PROMPT_COMMAND e temas não causam recursão ou duplicação.
- [x] **CA-SHL-04 (instructions.md §5):** Login/non-login e flags incompatíveis testados; hashes de dotfiles em `$HOME` inalterados; nenhum arquivo criado em `$HOME`; nonce não herdado por filhos lançados após o bootstrap.
- [x] **CA-PERF-01 (instructions.md §5):** Benchmark `osc_scan` comprova throughput ≥ 1 GiB/s sem ESC e ≥ 300 MiB/s em corpus OSC/ANSI.
