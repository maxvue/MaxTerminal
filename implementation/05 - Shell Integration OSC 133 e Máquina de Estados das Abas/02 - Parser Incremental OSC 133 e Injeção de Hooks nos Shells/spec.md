# Especificação Técnica — Implementação 02: Crate `mt-shell` — Parser Incremental OSC 133/633, Scripts de Hook e Injeção sem Dotfiles

> **Fase:** 05 — Shell Integration OSC 133 e Máquina de Estados das Abas
> **Implementação:** `02 - Parser Incremental OSC 133 e Injeção de Hooks nos Shells`
> **Fontes da verdade:** [`../../AGENTS.md`](../../../AGENTS.md), [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md) (§§3, 4 e 5.1).

---

## 1. Contexto, Objetivo e Escopo

Esta implementação constrói a base do crate `mt-shell`: o codec de escaping byte-a-byte, o parser incremental O(n) de sequências OSC 133 e OSC 633 (variante autenticada por nonce/geração do MaxTerminal), os scripts de hook embarcados para `bash` (≥ 4.4), `zsh` (≥ 5.8) e `fish` (≥ 3.6), e o injetor de inicialização que prepara o spawn do shell sem jamais tocar nos dotfiles ou criar arquivos em `$HOME`.

### Escopo Incluído
1. **Estrutura do crate `mt-shell` e detecção de shell (`kind.rs`):**
   - Classificação do executável (`Bash`, `Zsh`, `Fish`, `Unsupported`), inspeção de argumentos que impedem injeção interativa (`-c`, scripts posicionais, `--norc`, `--no-rcs`, `--no-config`, etc.) e geração de `Nonce128` por PTY (128 bits aleatórios via CSPRNG, codificados em 32 caracteres hexadecimais minúsculos).
2. **Codec de Escaping e Parser Incremental OSC (`osc/escape.rs`, `osc/scanner.rs`):**
   - Codec de desescape em passe único sobre bytes (`\\` → `\`, `\x3b` → `;`, `\xHH` para controles C0/DEL/C1), validação UTF-8 estrita, rejeição de `\0` e limites rígidos (OSC bruto ≤ 32 KiB, comando decodificado ≤ 16 KiB, `cwd` ≤ 8 KiB e caminho absoluto começando com `/`).
   - `OscScanner` incremental com fast-path `memchr2(0x1b, 0x18)` no estado `Ground`, rastreamento de strings de controle `DCS`/`SOS`/`PM`/`APC` e `CSI` para impedir falsos positivos de `ESC ]` internos, suporte a terminadores `BEL` (`0x07`) e `ST` (`ESC \`) mesmo quando divididos entre chunks, cancelamento por `CAN`/`SUB` (`0x18`/`0x1A`), ressincronização em `ESC` inesperado e cálculo exato de `StreamPosition { frame_seq, byte_offset }`.
3. **Scripts de Hook Embarcados (`scripts/bash/`, `scripts/zsh/`, `scripts/fish/`):**
   - Protocolo no fio:
     - Handshake: `ESC ] 633 ; P ; MT=1 ; n=<nonce> BEL`
     - Diretório atual: `ESC ] 633 ; P ; Cwd=<escaped-path> ; n=<nonce> ; g=<generation> BEL`
     - Linha de comando: `ESC ] 633 ; E ; <escaped-command> ; q=<complete|partial|unavailable> ; n=<nonce> ; g=<generation> BEL`
     - Marcadores OSC 133: `ESC ] 133 ; A ; n=<nonce> ; g=<generation> BEL`, `B`, `C`, e `D [ ; <exit-code> ] ; n=<nonce> ; g=<generation> BEL`.
   - Captura de `$?`/`$status` na primeira instrução antes de qualquer helper, preservação do código para hooks do usuário, remoção da variável de ambiente de transporte do nonce antes de executar processos filhos e ausência total de modificação em `$HOME`.
4. **Injetor (`inject.rs`):**
   - Materialização segura e atômica dos scripts em `$XDG_RUNTIME_DIR/maxterminal/shell-integration/<sha256>/` (diretório `0700`, arquivos `0600`, validação de dono UID e `O_NOFOLLOW`), com fallback seguro ou degradação explícita para `IntegrationMode::Proc` (`scripts_unavailable`).

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

| Ação | Caminho do Arquivo | Responsabilidade |
|------|--------------------|------------------|
| Modificar | `src-tauri/Cargo.toml` | Inclusão de `crates/mt-shell` nos membros do workspace e dependências compartilhadas |
| Criar | `src-tauri/crates/mt-shell/Cargo.toml` | Manifesto do crate `mt-shell` (deps: `mt-core`, `mt-pty`, `mt-db`, `memchr`, `vte`, `rand`, `sha2`, `parking_lot`, `crossbeam-channel`, `tokio`, `tracing`, `thiserror`, `serde`, `percent-encoding`; dev-deps: `proptest`, `criterion`, `tempfile`) |
| Criar | `src-tauri/crates/mt-shell/src/lib.rs` | Export dos módulos públicos do crate `mt-shell` |
| Criar | `src-tauri/crates/mt-shell/src/kind.rs` | `ShellKind`, `Nonce128`, análise de flags de linha de comando e elegibilidade de injeção |
| Criar | `src-tauri/crates/mt-shell/src/osc/mod.rs` | Tipos `ShellMark`, `ScannedMark`, `ScanMetrics`, `DiagnosticOsc` e re-exports |
| Criar | `src-tauri/crates/mt-shell/src/osc/escape.rs` | Funções `unescape_osc_field`, `escape_osc_field`, `validate_cwd_path` e limites de tamanho |
| Criar | `src-tauri/crates/mt-shell/src/osc/scanner.rs` | Máquina de estados incremental `OscScanner` e parser da gramática fechada 133/633 |
| Criar | `src-tauri/crates/mt-shell/src/inject.rs` | `ScriptMaterializer`, `ShellInjector`, `InjectionOutcome` e preparação de `ResolvedSpawn` |
| Criar | `src-tauri/crates/mt-shell/scripts/bash/mt-integration.bash` | Script de bootstrap e hooks (`PROMPT_COMMAND` + `DEBUG` trap) para GNU Bash ≥ 4.4 |
| Criar | `src-tauri/crates/mt-shell/scripts/zsh/.zshenv` | Wrapper `.zshenv` do `ZDOTDIR` preservando `ZDOTDIR` do usuário e `RCS`/`GLOBAL_RCS` |
| Criar | `src-tauri/crates/mt-shell/scripts/zsh/.zprofile` | Wrapper `.zprofile` do `ZDOTDIR` para shells login |
| Criar | `src-tauri/crates/mt-shell/scripts/zsh/.zshrc` | Wrapper `.zshrc` do `ZDOTDIR` carregando `.zshrc` do usuário e instalando hooks |
| Criar | `src-tauri/crates/mt-shell/scripts/zsh/.zlogin` | Wrapper `.zlogin` do `ZDOTDIR` carregando `.zlogin` do usuário e garantindo hooks em login |
| Criar | `src-tauri/crates/mt-shell/scripts/zsh/mt-integration.zsh` | Implementação dos hooks `precmd`/`preexec` e marcas de prompt `%{...%}` para Zsh ≥ 5.8 |
| Criar | `src-tauri/crates/mt-shell/scripts/fish/mt-integration.fish` | Implementação dos hooks `fish_preexec`, `fish_postexec` e wrapper `fish_prompt` para Fish ≥ 3.6 |
| Criar | `src-tauri/crates/mt-shell/tests/scanner_split.rs` | Testes exaustivos de divisão em 2/3 chunks e property tests (`proptest` ≥ 2.000 casos) |
| Criar | `src-tauri/crates/mt-shell/tests/injection_no_dotfile_changes.rs` | Testes verificando inalterabilidade de `$HOME`, permissões `0700`/`0600` e flags incompatíveis |
| Criar | `src-tauri/crates/mt-shell/tests/shells_bash.rs` | Testes de integração real com PTY para `bash` (login, non-login, códigos de saída, multiline, histórico desabilitado, Ctrl+C) |
| Criar | `src-tauri/crates/mt-shell/tests/shells_zsh.rs` | Testes de integração real com PTY para `zsh` (login, non-login, `ZDOTDIR` customizado, `precmd` preexistente, temas) |
| Criar | `src-tauri/crates/mt-shell/tests/shells_fish.rs` | Testes de integração real com PTY para `fish` (`true`, `false`, `exit 42`, prompt customizado, variáveis) |
| Criar | `src-tauri/crates/mt-shell/benches/osc_scan.rs` | Benchmark `criterion` medindo throughput sem ESC (≥ 1 GiB/s) e corpus misto ANSI/OSC (≥ 300 MiB/s) |

---

## 3. Assinaturas Exatas e Algoritmos (Rust e Shell)

### 3.1. Identificação do Shell e Nonce (`src-tauri/crates/mt-shell/src/kind.rs`)

```rust
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellKind {
    Bash,
    Zsh,
    Fish,
    Unsupported(String),
}

impl ShellKind {
    pub fn detect(program: &Path) -> Self;
    pub fn as_str(&self) -> &str;
}

/// Nonce de 128 bits por PTY para correlação de marcas OSC.
/// Debug e Display NUNCA imprimem o valor real (`Nonce128([REDACTED])`).
#[derive(Clone, PartialEq, Eq)]
pub struct Nonce128 {
    hex: [u8; 32],
}

impl Nonce128 {
    pub fn generate() -> Self;
    pub fn as_str(&self) -> &str;
    pub fn constant_time_eq(&self, candidate: &[u8]) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncompatibilityReason {
    OptedOut,
    EnvDisabled,
    UnsupportedShell(String),
    NonInteractiveFlag(String),
    RcSuppressedFlag(String),
    ScriptsUnavailable(String),
}

impl IncompatibilityReason {
    pub fn code(&self) -> &'static str; // "opted_out", "env_disabled", "unsupported_shell", "incompatible_flags", "scripts_unavailable"
}

/// Verifica se os argumentos do spawn permitem injeção de hooks interativos.
/// Recusa `-c`, `-s`, `--norc`, `--rcfile`, `--init-file`, `--posix` (bash),
/// `-f`, `--no-rcs`, `--no-globalrcs` (zsh), `-N`, `--no-config`, `-C`, `--init-command` conflitante (fish),
/// ou presença de argumento posicional de script (ex.: `bash script.sh`).
pub fn check_args_compatibility(kind: &ShellKind, args: &[String]) -> Result<(), IncompatibilityReason>;
```

### 3.2. Escaping e Limites (`src-tauri/crates/mt-shell/src/osc/escape.rs`)

```rust
pub const MAX_OSC_RAW_BYTES: usize = 32 * 1024;       // 32 KiB
pub const MAX_COMMAND_DECODED_BYTES: usize = 16 * 1024; // 16 KiB
pub const MAX_CWD_DECODED_BYTES: usize = 8 * 1024;     // 8 KiB

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnescapeError {
    TooLong,
    InvalidEscape,
    ContainsNul,
    InvalidUtf8,
    ControlCharUnescaped,
}

/// Decodifica em um único passe sobre bytes:
/// - `\\` -> `\`
/// - `\xHH` (2 dígitos hexadecimais ASCII) -> byte correspondente (rejeitando `\x00`)
/// - Qualquer outro `\` -> `Err(UnescapeError::InvalidEscape)`
/// - Bytes de controle crus (`< 0x20`, `0x7f`) ou `;` não escapado -> `Err(UnescapeError::ControlCharUnescaped)`
/// - Se o resultado exceder `max_decoded_len` -> `Err(UnescapeError::TooLong)` sem alocar além do teto.
/// - Ao final, converte via `String::from_utf8`.
pub fn unescape_osc_field(raw: &[u8], max_decoded_len: usize) -> Result<String, UnescapeError>;

/// Codifica texto para uso em testes/fixtures:
/// `\` -> `\\`, `;` -> `\x3b`, controles C0 (`0x00..=0x1f`), DEL (`0x7f`) e C1 (`0x80..=0x9f`) -> `\xHH` minúsculo.
pub fn escape_osc_field(input: &str) -> String;

/// Valida um `cwd` já desescapado:
/// - Deve começar com `/` e ter comprimento `1..=MAX_CWD_DECODED_BYTES`.
/// - Não pode conter bytes de controle (`< 0x20`, `0x7f`).
/// - Preserva o caminho lógico (symlinks intactos), sem chamar `std::fs::canonicalize`.
pub fn validate_cwd_path(decoded: &str) -> bool;
```

### 3.3. Parser Incremental OSC (`src-tauri/crates/mt-shell/src/osc/scanner.rs`)

```rust
use mt_core::shell::{CommandTextQuality, StreamPosition};
use crate::kind::Nonce128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanState {
    Ground,
    Esc,
    OscBody,
    OscEsc,
    DiscardOsc,
    DiscardOscEsc,
    /// Ignora payload de DCS (`ESC P`), SOS (`ESC X`), PM (`ESC ^`) e APC (`ESC _`) até ST (`ESC \`),
    /// impedindo que bytes `ESC ]` dentro dessas strings sejam interpretados como OSC.
    IgnoredString,
    IgnoredStringEsc,
    /// Consome sequência CSI (`ESC [`) até o byte final `0x40..=0x7E`.
    Csi,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellMark {
    Handshake,
    PromptStart,
    PromptEnd,
    CommandLine {
        text: Option<String>,
        quality: CommandTextQuality,
    },
    Cwd {
        path: String,
    },
    CommandExecuted,
    CommandFinished {
        exit_code: Option<i32>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticOsc {
    Osc7Cwd { host: String, path: String },
    ForeignOsc633,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedMark {
    pub mark: ShellMark,
    pub nonce_valid: bool,
    pub generation: u64,
    /// Posição do primeiro byte (`ESC`) da sequência OSC no stream bruto.
    pub start: StreamPosition,
    /// Posição do primeiro byte imediatamente após o terminador (`BEL` ou `ST`) no stream bruto.
    /// Quando a sequência termina exatamente no último byte de um frame, `end` aponta para
    /// `StreamPosition { frame_seq: current_seq, byte_offset: chunk_len as u32 }`.
    pub end: StreamPosition,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanMetrics {
    pub valid_marks: u64,
    pub invalid_nonce_marks: u64,
    pub malformed_marks: u64,
    pub oversized_osc_dropped: u64,
    pub diagnostic_osc_count: u64,
}

pub struct OscScanner {
    state: ScanState,
    expected_nonce: Nonce128,
    buffer: Vec<u8>, // capacidade limitada a MAX_OSC_RAW_BYTES (32 KiB)
    mark_start: StreamPosition,
    metrics: ScanMetrics,
}

impl OscScanner {
    pub fn new(expected_nonce: Nonce128) -> Self;
    pub fn state(&self) -> ScanState;
    pub fn metrics(&self) -> &ScanMetrics;

    /// Processa um frame do PTY de forma incremental e O(n), emitindo marcas via callback
    /// sem alocar vetores proporcionais ao número de marcas e SEM alterar o slice `bytes`.
    pub fn feed<F>(&mut self, seq: u64, bytes: &[u8], emit: F)
    where
        F: FnMut(ScannedMark);

    /// Invocado no EOF do PTY: descarta qualquer sequência incompleta sem emitir marca D sintética.
    pub fn finish(&mut self);
}
```

#### Regras Normativas da Gramática no `OscScanner`
1. **Gramática Fechada (chaves exatas, sem duplicatas, sem chaves desconhecidas):**
   - `633;P;MT=1;n=<nonce>` → `ShellMark::Handshake` (`generation = 0`).
   - `633;E;<escaped-cmd>;q=<complete|partial|unavailable>;n=<nonce>;g=<u64>` → `ShellMark::CommandLine`. Se `<escaped-cmd>` exceder `16 KiB` decodificados, mas o OSC total tiver `<= 32 KiB` e `n`/`g` forem válidos, emitir `ShellMark::CommandLine { text: None, quality: CommandTextQuality::Unavailable }` (nunca truncar o comando). Se `q=unavailable` ou `<escaped-cmd>` for vazio com `q != complete`, `text = None`.
   - `633;P;Cwd=<escaped-path>;n=<nonce>;g=<u64>` → `ShellMark::Cwd { path }` somente se o caminho decodificado passar em `validate_cwd_path`. Se exceder `8 KiB` ou for inválido, incrementar `malformed_marks` e ignorar.
   - `133;A;n=<nonce>;g=<u64>` → `ShellMark::PromptStart`.
   - `133;B;n=<nonce>;g=<u64>` → `ShellMark::PromptEnd`.
   - `133;C;n=<nonce>;g=<u64>` → `ShellMark::CommandExecuted`.
   - `133;D;n=<nonce>;g=<u64>` → `ShellMark::CommandFinished { exit_code: None }`.
   - `133;D;<i32>;n=<nonce>;g=<u64>` → `ShellMark::CommandFinished { exit_code: Some(code) }`. Se o campo de código estiver vazio (`133;D;;n=...`), resulta em `exit_code: None`; jamais converter vazio em `0`.
2. **Cancelamento e Ressincronização:**
   - Bytes `0x18` (`CAN`) ou `0x1A` (`SUB`) em qualquer estado fora de `Ground` limpam o buffer e retornam imediatamente para `Ground`.
   - Em `OscEsc`: se o byte seguinte for `\`, completa o terminador `ST` (`ESC \`), processa o buffer e volta a `Ground`. Se o byte seguinte for `]` (`ESC ]`), cancela o OSC anterior incompleto, inicia um novo `OscBody` com `mark_start` atualizado para o `ESC` recém-visto e limpa o buffer. Se for `[` (`CSI`), `P`/`X`/`^`/`_` (strings ignoradas) ou outro byte de escape, cancela o OSC incompleto e transita para o estado correspondente sem perder o byte atual.
   - Em `OscBody`: se `buffer.len() == MAX_OSC_RAW_BYTES` (32 KiB) e chegar mais um byte que não seja `BEL` nem `ESC`, limpar `buffer`, incrementar `metrics.oversized_osc_dropped` e transitar para `DiscardOsc` até encontrar `BEL`, `ESC \` ou `CAN`/`SUB`.

### 3.4. Injetor e Materialização (`src-tauri/crates/mt-shell/src/inject.rs`)

```rust
use std::path::{Path, PathBuf};
use mt_core::shell::IntegrationMode;
use mt_pty::ResolvedSpawn;
use crate::kind::{IncompatibilityReason, Nonce128, ShellKind};

#[derive(Debug, Clone)]
pub struct MaterializedScripts {
    pub version_hash: String, // SHA-256 hex truncado a 16 chars dos scripts embarcados
    pub root_dir: PathBuf,    // $XDG_RUNTIME_DIR/maxterminal/shell-integration/<sha256>
    pub bash_script: PathBuf,
    pub zsh_dir: PathBuf,
    pub fish_script: PathBuf,
}

#[derive(Debug, Clone)]
pub struct InjectionOutcome {
    pub shell_kind: ShellKind,
    pub mode: IntegrationMode, // Pending (quando hooks injetados; vira Hooks no handshake) ou Proc / Disabled
    pub reason: Option<String>,
    pub nonce: Nonce128,
    pub scripts_version: String,
}

pub struct ShellInjector {
    runtime_base_dir: PathBuf,
}

impl ShellInjector {
    pub fn new(runtime_base_dir: PathBuf) -> Self;

    /// Materializa de forma idempotente os scripts embarcados via `include_str!`:
    /// - Valida que `runtime_base_dir` pertence ao UID atual, não é symlink e tem modo 0700.
    /// - Cria subpastas com 0700 e escreve cada arquivo via temporário + rename atômico com modo 0600.
    pub fn ensure_scripts(&self) -> Result<MaterializedScripts, IncompatibilityReason>;

    /// Modifica `spawn` in-place (em `SpawnLifecycle::prepare`) injetando variáveis e flags:
    /// - Se `enabled == false`: verifica se é desativação total ou opt-out de hooks -> retorna `IntegrationMode::Proc` com `reason = Some("opted_out")`.
    /// - Se `spawn.env.get("MAXTERMINAL_DISABLE_SHELL_INTEGRATION") == Some("1")`: retorna `IntegrationMode::Proc` com `reason = Some("env_disabled")`.
    /// - Sempre define `MT_SHELL_NONCE=<nonce>` no `spawn.env` apenas quando `mode == IntegrationMode::Pending` (os scripts copiam para variável local não exportada e fazem `unset MT_SHELL_NONCE` imediatamente).
    pub fn inject(&self, spawn: &mut ResolvedSpawn, enabled: bool) -> InjectionOutcome;
}
```

#### Regras de Injeção por Shell
1. **GNU Bash (`ShellKind::Bash`):**
   - **Non-login (`spawn.login == false`):**
     - Preserva flags compatíveis existentes e injeta `--rcfile <bash_script> -i`.
     - Se o usuário tinha `--rcfile` ou `--init-file` explícito, `check_args_compatibility` recusa a injeção e usa `IntegrationMode::Proc` (`incompatible_flags`).
   - **Login (`spawn.login == true`):**
     - Mantém `--login` / `-l` nativo (para carregar `/etc/profile`, `~/.bash_profile` / `~/.bash_login` / `~/.profile` e `~/.bash_logout`).
     - Se já houver `PROMPT_COMMAND` no `spawn.env`, move seu valor para `MT_ORIG_PROMPT_COMMAND` e define `PROMPT_COMMAND="source '<bash_script>'"`.
     - O script `mt-integration.bash`, ao ser carregado na primeira execução do `PROMPT_COMMAND`, restaura `PROMPT_COMMAND` para `MT_ORIG_PROMPT_COMMAND` e anexa o hook do MaxTerminal sem reexecutar o bootstrap. Se o `.bash_profile` do usuário sobrescrever totalmente `PROMPT_COMMAND` sem preservá-lo, nenhum handshake será emitido e o fallback de 5 s ativará `IntegrationMode::Proc` com motivo `handshake_timeout`.
2. **Zsh (`ShellKind::Zsh`):**
   - Se `spawn.env` (ou ambiente herdado) possuir `ZDOTDIR`, salva em `MT_ORIG_ZDOTDIR`.
   - Define `ZDOTDIR=<zsh_dir>` e `MT_ZSH_BOOTSTRAP_DIR=<zsh_dir>`.
   - Os arquivos `.zshenv`, `.zprofile`, `.zshrc` e `.zlogin` dentro de `<zsh_dir>`:
     - Capturam `MT_SHELL_NONCE` em variável escalar interna (`typeset -g __mt_nonce="$MT_SHELL_NONCE"`) e executam `unset MT_SHELL_NONCE` já em `.zshenv`.
     - Respeitam `[[ -o rcs ]]` antes de carregar os dotfiles do usuário em `${MT_ORIG_ZDOTDIR:-$HOME}`.
     - Se o dotfile do usuário alterar `ZDOTDIR` para outro caminho diferente de `<zsh_dir>`, atualizam `MT_ORIG_ZDOTDIR="$ZDOTDIR"` e restauram temporariamente `ZDOTDIR` durante o startup restante; ao final de `.zshrc` / `.zlogin`, restauram `ZDOTDIR` para `MT_ORIG_ZDOTDIR` (ou fazem `unset ZDOTDIR` se não existia), garantindo que processos filhos não herdem o diretório de bootstrap.
3. **Fish (`ShellKind::Fish`):**
   - Anexa `--init-command "source '<fish_script>'"` (com path escapado para sintaxe Fish) aos argumentos de `spawn.args`, preservando qualquer `--init-command` anterior do usuário.

### 3.5. Semântica Detalhada dos Scripts de Hook

#### A. `scripts/bash/mt-integration.bash`
- Guarda de idempotência: `[[ -n "${__mt_bash_loaded:-}" ]] && return 0; __mt_bash_loaded=1`.
- Verifica shell interativo (`[[ $- == *i* ]]`) e `BASH_VERSINFO[0] -gt 4 || (BASH_VERSINFO[0] -eq 4 && BASH_VERSINFO[1] -ge 4)`.
- Copia nonce para variável não exportada `__mt_nonce="${MT_SHELL_NONCE:-}"` e executa `unset MT_SHELL_NONCE`. Se `__mt_nonce` estiver vazio, aborta silenciosamente.
- No modo non-login (`shopt -q login_shell` falso): carrega `/etc/bash.bashrc` (se existir e ainda não carregado) e `~/.bashrc` (se existir) antes de instalar os hooks.
- Função de escape puro em bash (`__mt_escape`): usa substituição de parâmetros e iteração byte/char sem invocar `sed`/`awk`/`python` nem usar `printf` com o texto como formato (`printf '%s'`).
- Emissão do handshake inicial: `printf '\e]633;P;MT=1;n=%s\a' "$__mt_nonce"`.
- Hook de prompt (`__mt_prompt_cmd`):
  - **Primeira linha obrigatória:** `local __mt_status=$? __mt_pipestatus=("${PIPESTATUS[@]}")`.
  - Desarma flag `__mt_in_prompt=1` para que comandos executados dentro do `PROMPT_COMMAND` do usuário não disparem `__mt_debug_trap`.
  - Se `__mt_executing=1`: emite `ESC ] 133 ; D ; $__mt_status ; n=$__mt_nonce ; g=$__mt_gen BEL` e limpa `__mt_executing=0`.
  - Emite `ESC ] 633 ; P ; Cwd=<escaped $PWD> ; n=$__mt_nonce ; g=$__mt_gen BEL`.
  - Executa o `PROMPT_COMMAND` original do usuário restaurando `(exit $__mt_status)` antes de chamá-lo para que temas/prompts vejam o `$?` real.
  - Decora `PS1` de forma idempotente envolvendo `\[\e]133;A;n=...;g=...\a\]` no início e `\[\e]133;B;n=...;g=...\a\]` no fim (atualizando o número da geração atual via variável avaliada no prompt).
  - Ao final do prompt, captura o último identificador do histórico (`HISTTIMEFORMAT='' builtin history 1`) em `__mt_last_hist_id` e define `__mt_in_prompt=0` e `__mt_armed=1`.
- Hook `DEBUG` (`__mt_debug_trap`):
  - **Primeira linha obrigatória:** `local __mt_dbg_status=$?`.
  - Ignora se `__mt_in_prompt=1`, se `__mt_armed=0`, se `[[ -n "${COMP_LINE:-}" ]]` (autocompletar programmable completion) ou se o comando for o próprio `__mt_prompt_cmd`.
  - Desarma `__mt_armed=0`, define `__mt_executing=1` e incrementa `__mt_gen=$((__mt_gen + 1))`.
  - Obtém a linha de comando:
    - Verifica se `set -o | grep -q '^history[[:space:]]*on'` está ativo e lê `HISTTIMEFORMAT='' builtin history 1`.
    - Se o ID do histórico mudou em relação a `__mt_last_hist_id`, extrai a linha completa (incluindo multiline) com `q=complete`.
    - Se o ID do histórico **não** mudou (ex.: comando iniciado com espaço sob `HISTCONTROL=ignorespace`, duplicata sob `ignoredups`, `HISTIGNORE` ou `set +o history`), **nunca** usa a entrada anterior do histórico nem assume que `$BASH_COMMAND` é a linha completa: emite comando vazio com `q=unavailable` (preservando a privacidade de `ignorespace` e evitando atribuir comando antigo).
  - Emite `633;E`, `633;P;Cwd` e `133;C` com `g=$__mt_gen`.
  - Restaura `(exit $__mt_dbg_status)` antes de retornar.

#### B. `scripts/zsh/mt-integration.zsh`
- Usa `zmodload zsh/parameter` / `add-zsh-hook`.
- Se existir função `precmd` definida pelo usuário antes da instalação, preserva-a ou garante que `__mt_precmd` capture `local __mt_status=$?` no início absoluto do ciclo de prompt e restaure `return $__mt_status` para os hooks seguintes.
- Em `__mt_preexec`: recebe `$1` (linha digitada) ou `$3` (linha completa), incrementa `__mt_gen`, escapa comando e `$PWD` usando expansão nativa Zsh, e emite `633;E` (`q=complete`), `633;P;Cwd` e `133;C`.
- Em `__mt_precmd`: se `__mt_executing == 1`, emite `133;D;$__mt_status` com `g=$__mt_gen` e zera `__mt_executing=0`. Emite `633;P;Cwd` e envolve `PROMPT`/`PS1` com `%{\e]133;A;n=...;g=...\a%}` e `%{\e]133;B;n=...;g=...\a%}` sem duplicar caso o tema recalcule `PROMPT`.

#### C. `scripts/fish/mt-integration.fish`
- Copia `MT_SHELL_NONCE` para `set -g __mt_nonce $MT_SHELL_NONCE` (global, **não** exportada `-x`) e executa `set -e MT_SHELL_NONCE`.
- Emite handshake inicial `633;P;MT=1;n=$__mt_nonce` e `633;P;Cwd` inicial (`g=0`).
- `function __mt_preexec --on-event fish_preexec`: incrementa `__mt_gen`, define `__mt_executing 1`, escapa `$argv[1]` e `$PWD`, emite `633;E` (`q=complete`), `633;P;Cwd` e `133;C`.
- `function __mt_postexec --on-event fish_postexec`: captura `set -l last_status $status` na primeira linha! Se `__mt_executing = 1`, emite `133;D;$last_status` com `g=$__mt_gen` e define `__mt_executing 0`.
- Wrapper de `fish_prompt`: preserva a função `fish_prompt` original copiando-a para `__mt_orig_fish_prompt` (se ainda não copiada), emite `133;A` antes do output do prompt original (preservando `$status` para `__mt_orig_fish_prompt`) e `133;B` ao final. Se `fish_postexec` não rodou (ex.: `Ctrl+C` ou comando cancelado), fecha eventual execução aberta sem fabricar status 0.

---

## 4. Casos de Borda e Garantias de Segurança

1. **Inalterabilidade do Stream Bruto:**
   - `OscScanner::feed` recebe `&[u8]` imutável. Os bytes originais (incluindo as sequências OSC e nonces) seguem inalterados para o `Channel` e `ReplayRing` da Fase 02/04.
2. **Isolamento de Subshells e Filhos (`unset MT_SHELL_NONCE`):**
   - Nenhum processo filho iniciado pelo usuário no shell (`env`, `python`, subshell `bash`/`zsh`/`fish`) enxerga `MT_SHELL_NONCE` no `environ`, impedindo que subshells aninhados reinicializem o handshake ou colidam com as gerações do shell pai.
3. **Comandos Ignorados por Espaço Inicial (`HISTCONTROL=ignorespace`):**
   - Quando o usuário digita um comando precedido por espaço no bash com `ignorespace`/`ignoreboth`, o histórico não avança: o hook declara `q=unavailable` com comando vazio, sem jamais atribuir o comando anterior do histórico nem expor o comando com espaço via `$BASH_COMMAND`.
4. **Limites de Memória no Scanner:**
   - `OscScanner.buffer` é pré-alocado/limitado a `32 KiB`. Qualquer sequência OSC maior entra em `DiscardOsc`, mantendo memória constante O(1) por PTY mesmo sob ataque de `cat /dev/urandom` ou sequência infinita sem `BEL`/`ST`.
