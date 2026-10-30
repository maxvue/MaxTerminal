# Implementação 02 — Contexto Sanitizado, Classificador de Risco, Sugestão NL→Shell, Diagnóstico Inline, RAG e Geração Assistida (`spec.md`)

> **Fase:** 12 — Integração MaxAiManager (`@` / `?`)
> **Unidade:** `02 - Contexto Sanitizado, Classificador de Risco, Sugestão NL-Shell, Diagnóstico Inline, RAG e Geração Assistida`
> **Pré-requisitos:** Implementação 01 da Fase 12 (`mt-ai` base, `GatewayClient`, `CatalogService`, `AiRepo`, `AiRequestRegistry`)
> **Cobertura de Escopo da Fase 12:** `I-05`, `I-08` (backend), `I-09`, `I-10` (backend), `I-11` (backend), `I-12` (backend)
> **Cobertura de Critérios de Aceite:** `CA-01` (completo), `CA-15` (backend), `CA-16` (backend), `CA-17`, `CA-18` (backend), `CA-19`, `CA-20` (backend), `CA-21`, `CA-22` (backend), `CA-25` (backend), `CA-26`, `CA-27`, `CA-34` (backend), `CA-36` (validação JSON Schema local)

---

## 1. Contexto, Objetivo e Escopo

Esta segunda unidade completa todos os motores de domínio e serviços de negócio do crate `mt-ai`:
1. **Contexto Sanitizado (`context.rs`):** Define `AiContext`, `OsInfo`, `RecentCommand`, `BlockFailure` e a trait `AiContextSource`, aplicando sanitização dupla (`mt_db::sanitizer::sanitize()`), remoção de sequências ANSI (`strip-ansi-escapes`), abreviação de `$HOME` para `~` e truncamento seguro (comandos `<= 1 KiB`, cauda de output `<= output_tail_bytes` preservando o final).
2. **Classificador Determinístico de Risco (`risk.rs`):** Implementa `RiskClassifier` com segmentação de comandos shell (`&&`, `||`, `;`, `|`, `&`, `$(...)`, crases), tokenização com `shell-words`, remoção de wrappers (`sudo`, `doas`, `env`, `nice`, `time`, `xargs`), mais de 40 regras para comandos `destructive` e `caution`, extração do `confirm_token` e combinação monotônica `level = max(local, model_hint)` onde a dica do modelo nunca rebaixa o risco local (`CA-17`, `CA-19`).
3. **Templates de Prompt e Delimitação Anti-Injection (`prompts/`):** Isola os prompts de sistema e de usuário em português do Brasil (`pt-BR`) para sugestão NL→Shell, diagnóstico inline e geração estruturada, encapsulando saídas de terminal em blocos `<<<OUTPUT ... OUTPUT>>>` tratados estritamente como dados não confiáveis.
4. **Conversão NL→Shell (`suggest.rs`):** Orquestra a geração de comandos a partir dos gatilhos `@` e `?`, emitindo `AiStreamEvent` (`Started`, `Delta`, `Completed`, `Failed`, `Cancelled`), validando a resposta JSON estrita (recusando prosa, comandos vazios, multilinhas ou caracteres de controle `NUL`/`CR`/`ESC`) e combinando o risco local com a dica do modelo.
5. **Diagnóstico Rápido Inline (`diagnose.rs`):** Implementa `DiagnosisService` com normalização de output preservando caminhos/códigos de erro, `error_hash` SHA-256 canônico sobre `DiagnosisCacheKey`, cache SQLite com TTL de 30 dias, substituição imediata por painel (`Superseded`), fila limitada de capacidade 3 + semáforo de concorrência 1 e rate limiter de `max_per_minute` (padrão 10/min).
6. **Consulta RAG (`rag.rs`):** Implementa `RagService` para busca em `/v1/rags/{slug}/search`, com validação estrita de slug (`^[a-z0-9][a-z0-9_-]{0,63}$`), codificação via `Url::path_segments_mut().push()`, ordenação estável por score, truncamento de conteúdo em 2 KiB e filtragem de URLs `http`/`https`.
7. **Geração Assistida Estruturada (`structured.rs`) e `AiService` (`lib.rs`):** Implementa `StructuredGenerator` com validação JSON Schema (`jsonschema` draft 2020-12), bloqueio de `$ref` remotos, cache de até 64 schemas compilados, até 1 tentativa automática de reparo dentro do deadline total de 90 s (`CA-27`, `CA-36`), e unifica tudo na fachada `AiService` com health check periódico em background.

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

### 2.1. Arquivos a Modificar
- `src-tauri/crates/mt-ai/src/lib.rs` — implementação completa de `AiService` agregando todos os sub-serviços e loop de monitoramento de saúde (`GatewayStatus`).

### 2.2. Arquivos a Criar
- `src-tauri/crates/mt-ai/src/context.rs`
- `src-tauri/crates/mt-ai/src/risk.rs`
- `src-tauri/crates/mt-ai/src/prompts/mod.rs`
- `src-tauri/crates/mt-ai/src/prompts/suggest.rs`
- `src-tauri/crates/mt-ai/src/prompts/diagnose.rs`
- `src-tauri/crates/mt-ai/src/prompts/structured.rs`
- `src-tauri/crates/mt-ai/src/suggest.rs`
- `src-tauri/crates/mt-ai/src/diagnose.rs`
- `src-tauri/crates/mt-ai/src/rag.rs`
- `src-tauri/crates/mt-ai/src/structured.rs`
- `src-tauri/crates/mt-ai/tests/risk_rules.rs`
- `src-tauri/crates/mt-ai/tests/suggest_parse.rs`
- `src-tauri/crates/mt-ai/tests/diagnose_cache.rs`
- `src-tauri/crates/mt-ai/tests/structured.rs`

---

## 3. Especificação Técnica Exaustiva

### 3.1. Contexto da Requisição e Sanitização Dupla (`context.rs`)

```rust
use crate::config::ContextSettings;
use mt_core::{CommandBlockId, MtResult, PaneId, ProjectId, PtyId, SessionId, TabId};
use serde::{Deserialize, Serialize};

pub const MAX_COMMAND_BYTES: usize = 1024;
pub const SANITIZER_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OsInfo {
    pub family: String,
    pub distro: String,
    pub kernel: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RecentCommand {
    pub command: String,
    pub exit_code: Option<i32>,
    pub cwd: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiContext {
    pub shell: String,
    pub os: OsInfo,
    pub cwd: String,
    pub project_stack: Option<String>,
    pub project_name: Option<String>,
    pub recent_commands: Vec<RecentCommand>,
    pub output_tail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneBindingSnapshot {
    pub project_id: Option<ProjectId>,
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub session_id: SessionId,
    pub pty_id: PtyId,
}

#[derive(Clone, Debug)]
pub struct BlockFailure {
    pub pane_id: PaneId,
    pub session_id: SessionId,
    pub pty_id: PtyId,
    pub project_id: Option<ProjectId>,
    pub command: String,
    pub exit_code: Option<i32>,
    pub output_tail: String,
    pub is_interactive: bool,
}

#[async_trait::async_trait]
pub trait AiContextSource: Send + Sync {
    async fn for_pane(&self, pane: PaneId, opts: &ContextSettings) -> MtResult<AiContext>;
    async fn for_block(&self, block: CommandBlockId, tail_bytes: u32) -> MtResult<(AiContext, BlockFailure)>;
    fn pane_snapshot(&self, pane: PaneId) -> Option<PaneBindingSnapshot>;
    fn project_of_pane(&self, pane: PaneId) -> Option<ProjectId>;
}

impl AiContext {
    /// Remove códigos ANSI, abrevia o diretório `$HOME` para `~`, trunca comandos em 1 KiB e `output_tail`
    /// mantendo os últimos `tail_bytes` em fronteira UTF-8 válida, e reaplica `mt_db::sanitizer::sanitize()`
    /// em todos os campos textuais (defesa em profundidade — CA-18, CA-31).
    pub fn sanitize_and_bound(mut self, max_recent: u8, tail_bytes: u32) -> Self;
}

pub fn strip_ansi_and_sanitize(input: &str) -> String;
pub fn truncate_utf8_head(input: &str, max_bytes: usize) -> String;
pub fn truncate_utf8_tail(input: &str, max_bytes: usize) -> String;
pub fn abbreviate_home(path: &str, home_dir: Option<&str>) -> String;
pub fn detect_linux_os_info() -> OsInfo;
```

---

### 3.2. Classificador de Risco (`risk.rs`)

```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum RiskLevel {
    Safe,
    Caution,
    Destructive,
}

impl RiskLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Caution => "caution",
            Self::Destructive => "destructive",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RiskReason {
    pub rule_id: String,
    pub message: String,
    pub segment: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RiskAssessment {
    pub level: RiskLevel,
    pub local_level: RiskLevel,
    pub model_hint: Option<RiskLevel>,
    pub reasons: Vec<RiskReason>,
    pub requires_confirmation: bool,
    pub confirm_token: Option<String>,
}

pub struct RiskClassifier {
    // Regras pré-compiladas com once_cell::sync::Lazy
}

impl RiskClassifier {
    pub fn default_rules() -> Self;
    pub fn classify(&self, command: &str) -> RiskAssessment;
    pub fn combine(local: RiskAssessment, hint: Option<RiskLevel>) -> RiskAssessment;
}
```

- **Algoritmo detalhado de `RiskClassifier::classify` (`CA-16`, `CA-17`, `CA-19`):**
  1. **Extração de segmentos e subshells:**
     - Varre o comando respeitando aspas simples `'...'` e duplas `"..."`, extraindo subshells `$(...)` e `` `...` `` como segmentos extras a inspecionar e dividindo o comando principal nos operadores `&&`, `||`, `;`, `|` e `&` (mas preservando pipes como `curl ... | sh` também para inspeção de pipeline completo).
  2. **Tokenização com `shell-words`:**
     - Se `shell_words::split(segment)` falhar (aspas desbalanceadas ou sintaxe ambígua), classifica o segmento no mínimo como `RiskLevel::Caution` com `rule_id = "unparseable-shell"`.
  3. **Desempacotamento de wrappers:**
     - Remove prefixos `sudo`, `doas`, `pkexec`, `env` (e pares `VAR=val`), `nice`, `nohup`, `time`, `xargs`, `command`, `builtin`.
     - Se `sudo`, `doas` ou `pkexec` estiver presente, adiciona motivo `caution` (`rule_id = "privilege-escalation"`) e preserva o primeiro comando real após o wrapper (ou `"sudo"` se o usuário preferir confirmar pelo executável principal; para `sudo rm -r x`, `confirm_token` = primeiro token do segmento ou comando principal de acordo com a regra: **1º token do segmento mais perigoso** ou executável destrutivo; importante: para `rm -rf ./build`, o 1º token é `"rm"` — `CA-16`).
  4. **Catálogo obrigatório de regras `Destructive` (cobertura de todos os casos de `CA-19`):**
     - `rm-recursive-or-force`: `rm` com `-r`, `-R`, `--recursive`, `-rf`, `-fr` ou `-f` em `/`, `~`, `*`, `..`, ou qualquer `-rf`/`-fr`/`-r`.
     - `dd-disk-write`: `dd` com argumento `of=*`.
     - `filesystem-format`: `mkfs`, `mkfs.*`, `wipefs`, `shred`, `fdisk`, `parted`, `sgdisk`, `cfdisk`.
     - `block-device-redirect`: redirecionamento `>` ou `>>` para `/dev/sd*`, `/dev/nvme*`, `/dev/vd*`, `/dev/hd*`, `/dev/loop*`.
     - `git-force-push`: `git push` com `--force`, `-f`, `--force-with-lease` ou refspec iniciando com `+`.
     - `git-hard-reset`: `git reset --hard`.
     - `git-clean-force`: `git clean` com `-f`, `-fd`, `-fx`, `-fdx`.
     - `git-branch-force-delete`: `git branch -D`.
     - `git-discard-worktree`: `git checkout -- .` ou `git restore .`.
     - `chmod-chown-recursive-root`: `chmod` ou `chown` com `-R` sobre `/`, `/*`, `~`, ou `chmod -R 777 /`.
     - `fork-bomb`: padrão `:(){ :|:& };:` ou equivalente.
     - `pipe-to-shell`: pipeline `curl|wget|fetch ... | sh|bash|zsh|dash|python|perl|sudo`.
     - `sql-destructive`: `DROP DATABASE`, `DROP TABLE`, `TRUNCATE` (case-insensitive) fora de `echo`/`grep` puramente literais sem pipe para banco, ou em chamadas `mysql`, `psql`, `sqlite3`, `mariadb`.
     - `artisan-destructive`: `php artisan` com `migrate:fresh`, `migrate:reset`, `db:wipe`, `migrate:refresh`.
     - `docker-prune-volume`: `docker system prune -a`, `docker volume rm`, `docker volume prune`.
     - `kill-all-processes`: `kill -9 -1` ou `killall -9`.
     - `systemd-stop-disable`: `systemctl stop` ou `systemctl disable` (com `sudo` ou direto).
     - `find-delete`: `find` contendo `-delete` ou `-exec rm`.
  5. **Prevenção de Falsos Positivos (`CA-19`):**
     - Se o executável do segmento for `echo` ou `printf` sem pipe (`|`) para um interpretador/shell e sem redirecionamento para dispositivo, mas os argumentos literais contiverem texto semelhante a comando destrutivo (ex.: `echo "rm -rf"`), classifica como `RiskLevel::Caution` (`rule_id = "literal-dangerous-pattern"`), nunca `Destructive`.
     - Se o executável for puramente de leitura/busca (`grep`, `rg`, `awk`, `cat`, `less`, `head`, `tail`, `ls`, `stat`, `file`, `wc`, `which`, `type`) sem redirecionamento `>` e sem `-exec`, argumentos como `grep rm file` são classificados como `RiskLevel::Safe`.
  6. **Combinação com dica do modelo (`RiskClassifier::combine`):**
     - `let final_level = std::cmp::max(local.level, hint.unwrap_or(RiskLevel::Safe));`
     - `requires_confirmation = final_level == RiskLevel::Destructive`.
     - Se `final_level == RiskLevel::Destructive` e `confirm_token.is_none()`, extrai o 1º token do comando como `confirm_token`.

---

### 3.3. Sugestão NL→Shell (`suggest.rs` e `prompts/suggest.rs`)

```rust
use crate::config::AiSelection;
use crate::context::AiContext;
use crate::ids::AiRequestId;
use crate::risk::{RiskAssessment, RiskLevel};
use mt_core::{MtError, MtResult, PaneId};
use serde::{Deserialize, Serialize};

pub const PROMPT_VERSION_SUGGEST: u32 = 1;
pub const MAX_SUGGEST_PROMPT_CHARS: usize = 2000;
pub const MAX_EXPLANATION_CHARS: usize = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AiTrigger {
    At,
    Question,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SuggestRequest {
    pub request_id: AiRequestId,
    pub pane_id: PaneId,
    pub trigger: AiTrigger,
    pub selection: Option<AiSelection>,
    pub prompt: String,
    pub regenerate_from: Option<AiRequestId>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandSuggestion {
    pub request_id: AiRequestId,
    pub command: String,
    pub explanation: String,
    pub risk: RiskAssessment,
    pub selection: AiSelection,
    pub context_summary: AiContext,
    pub latency_ms: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AiStreamEvent {
    Started {
        request_id: AiRequestId,
        selection: AiSelection,
        context: AiContext,
    },
    Delta {
        request_id: AiRequestId,
        text: String,
    },
    Completed {
        request_id: AiRequestId,
        suggestion: CommandSuggestion,
    },
    Failed {
        request_id: AiRequestId,
        error: MtError,
    },
    Cancelled {
        request_id: AiRequestId,
    },
}

#[derive(Clone, Debug, Deserialize)]
pub struct RawModelSuggestion {
    pub command: String,
    pub explanation: String,
    pub risk: Option<RiskLevel>,
}

/// Faz o parse estrito da sugestão retornada pelo modelo:
/// - Aceita JSON direto ou um único bloco cercado ```json ... ```.
/// - Rejeita prosa fora do JSON, múltiplos blocos, JSON malformado (`ai.bad_response`).
/// - Rejeita `command` vazio após trim (`ai.empty_command`).
/// - Rejeita `command` contendo `\n`, `\r`, `\0`, `\x1b` (ESC) ou outros caracteres de controle (`ai.bad_response`).
/// - Nunca remove `$ ` nem junta múltiplas linhas com `&&`.
pub fn parse_suggestion(raw: &str) -> MtResult<RawModelSuggestion>;
```

---

### 3.4. Diagnóstico Rápido Inline (`diagnose.rs` e `prompts/diagnose.rs`)

```rust
use crate::config::AiSelection;
use crate::ids::AiRequestId;
use crate::risk::RiskAssessment;
use crate::AiService;
use dashmap::DashMap;
use mt_core::{CommandBlockId, MtResult, PaneId, ProjectId};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

pub const PROMPT_VERSION_DIAGNOSE: u32 = 1;
pub const MAX_DIAGNOSIS_SUMMARY_CHARS: usize = 160;
pub const MAX_DIAGNOSIS_QUEUE_WAITING: usize = 3;
pub const DIAGNOSIS_CACHE_TTL_DAYS: i64 = 30;

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InlineDiagnosis {
    pub block_id: CommandBlockId,
    pub error_hash: String,
    pub summary: String,
    pub fix_command: Option<String>,
    pub fix_risk: Option<RiskAssessment>,
    pub from_cache: bool,
    pub created_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosisSkip {
    Disabled,
    ProjectOptOut,
    IgnoredExitCode,
    Interactive,
    RateLimited,
    Superseded,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum DiagnosisOutcome {
    Ready { diagnosis: InlineDiagnosis },
    Skipped { reason: DiagnosisSkip },
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DiagnoseRequest {
    pub request_id: AiRequestId,
    pub block_id: CommandBlockId,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosisCacheKey {
    pub gateway_key: String,
    pub project_id: Option<ProjectId>,
    pub cwd: String,
    pub shell: String,
    pub command: String,
    pub exit_code: i32,
    pub sanitized_tail: String,
    pub selection: AiSelection,
    pub prompt_version: u32,
    pub sanitizer_version: u32,
}

pub struct DiagnosisService {
    semaphore: Semaphore,
    waiting_queue: Mutex<VecDeque<(AiRequestId, CancellationToken)>>,
    pending_by_pane: DashMap<PaneId, (AiRequestId, CancellationToken)>,
    dismissed_blocks: DashMap<CommandBlockId, ()>,
    limiter: Mutex<RateWindow>,
}

impl DiagnosisService {
    pub fn new() -> Self;
    pub fn error_hash(key: &DiagnosisCacheKey) -> String;
    pub fn normalize(tail: &str) -> String;
    pub fn dismiss_block(&self, block_id: CommandBlockId);
    pub fn is_dismissed(&self, block_id: CommandBlockId) -> bool;
    pub fn cancel_all_pending(&self);
    pub async fn diagnose(&self, svc: &AiService, req: DiagnoseRequest) -> MtResult<DiagnosisOutcome>;
}
```
- **Regras obrigatórias do `DiagnosisService` (`CA-20`, `CA-21`, `CA-22`, `CA-24`, `CA-34`):**
  1. `normalize(tail)` remove sequências ANSI (`strip-ansi-escapes`), converte `\r\n` e `\r` em `\n`, e remove linhas em branco finais, mas **preserva** argumentos, caminhos de arquivos, números de linha e códigos de erro para nunca colidir erros distintos no mesmo hash (`R-08`).
  2. Antes de consultar cache ou rede, verifica:
     - `settings.enabled && settings.diagnosis.enabled` → senão `Skipped { reason: Disabled }`.
     - `block_failure.is_interactive` → se `true`, `Skipped { reason: Interactive }`.
     - `block_failure.exit_code`: se `None` ou `Some(0)` ou contido em `settings.diagnosis.ignored_exit_codes` (ex.: `130`), retorna `Skipped { reason: IgnoredExitCode }`.
     - `block_failure.project_id` contido em `settings.diagnosis.disabled_projects` → `Skipped { reason: ProjectOptOut }`.
  3. **Substituição por painel (`Superseded`):** Se já houver um diagnóstico pendente no mesmo `pane_id`, cancela o token anterior (que terminará como `Skipped { reason: Superseded }`) e registra o novo. Dois painéis diferentes na mesma aba executam sem cancelar um ao outro (`CA-34`).
  4. **Cache Hit (`CA-21`):** Se `repo.get_diagnosis_cache(&error_hash, &now)` retornar entrada válida, reclassifica `fix_command` com `svc.risk.classify()` atual, atribui `block_id: req.block_id` e `from_cache: true`, e retorna `DiagnosisOutcome::Ready` **sem consumir rate limit nem fazer chamada HTTP**.
  5. **Fila limitada (3) + Concorrência (1) + Rate Limit (`max_per_minute`):** Em caso de cache miss, verifica `RateWindow` (`Skipped { reason: RateLimited }` se excedido). Se a fila de espera (`waiting_queue`) já tiver 3 itens aguardando o `Semaphore(1)`, remove o mais antigo da fila e aciona seu token de descarte (`Skipped { reason: Superseded }`). Após adquirir o semáforo, revalida se o painel/sessão/configuração não mudaram e se o bloco não foi dispensado (`dismiss_block`).
  6. Trunca `summary` em no máximo 160 caracteres (adicionando `"…"` se truncado — `CA-23`).

---

### 3.5. Consulta RAG (`rag.rs`)

```rust
use crate::client::http::{GatewayClient, Op};
use crate::config::RagSettings;
use crate::ids::AiRequestId;
use mt_core::{MtResult, PaneId};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

pub const MAX_RAG_QUERY_CHARS: usize = 2000;
pub const MAX_RAG_CONTENT_BYTES: usize = 2048;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RagSlug {
    pub slug: String,
    pub title: Option<String>,
    pub documents: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RagSearchRequest {
    pub request_id: AiRequestId,
    pub slug: String,
    pub query: String,
    pub top_k: Option<u8>,
    pub pane_id: Option<PaneId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RagResult {
    pub id: Option<String>,
    pub title: Option<String>,
    pub content: String,
    pub score: Option<f32>,
    pub source: Option<String>,
    pub url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RagSearchResponse {
    pub slug: String,
    pub query: String,
    pub results: Vec<RagResult>,
    pub latency_ms: u32,
}

pub struct RagService;

impl RagService {
    pub fn validate_slug(slug: &str) -> MtResult<()>;
    pub fn default_slug(settings: &RagSettings, stack: Option<&str>) -> Option<String>;
    pub async fn list(&self, client: &GatewayClient, settings: &RagSettings) -> MtResult<Vec<RagSlug>>;
    pub async fn search(
        &self,
        client: &GatewayClient,
        settings: &RagSettings,
        req: &RagSearchRequest,
        cancel: &CancellationToken,
    ) -> MtResult<RagSearchResponse>;
}
```
- **Regras obrigatórias do `RagService` (`CA-25`, `CA-26`):**
  1. `validate_slug(slug)` valida regex `^[a-z0-9][a-z0-9_-]{0,63}$`; se inválido, retorna imediatamente `Err(ai_error(codes::AI_INVALID_SLUG, ...))` **antes** de qualquer requisição HTTP (`CA-26`).
  2. O caminho `/v1/rags/{slug}/search` é montado usando segmentos codificados (`Url::path_segments_mut().push("v1").push("rags").push(slug).push("search")`).
  3. Quando o gateway retornar `404` na busca de um slug, retorna `Err(ai_error_with_details(codes::AI_RAG_NOT_FOUND, format!("Base de conhecimento '{}' não encontrada.", req.slug), json!({ "slug": req.slug })))` (`CA-26`).
  4. Sanitiza `title`, `content` e `source` com `mt_db::sanitizer::sanitize()`, trunca `content` em `2 KiB` (`2048 bytes`) e aceita `url` somente se iniciar com `http://` ou `https://` válidos (descartando `javascript:`, `file:`, `data:`).
  5. Se todos os itens possuírem `score: Some(s)` finito (`s.is_finite()`), ordena em ordem decrescente estável; caso contrário, preserva a ordem original retornada pelo gateway.

---

### 3.6. Geração Assistida Estruturada (`structured.rs` e `prompts/structured.rs`)

```rust
use crate::config::AiSelection;
use crate::ids::AiRequestId;
use crate::AiService;
use dashmap::DashMap;
use mt_core::{MtResult, PaneId, ProjectId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub const MAX_SCHEMA_BYTES: usize = 32 * 1024;
pub const MAX_STRUCTURED_PROMPT_CHARS: usize = 8000;
pub const MAX_CACHED_VALIDATORS: usize = 64;
/// Ids builtin (versionados, sem alias): D-05 / C-05 / C-06.
pub const BUILTIN_SCHEMA_IDS: [&str; 2] = ["artisan.command.v1", "command-group.suggestion.v1"];

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StructuredRequest {
    pub request_id: AiRequestId,
    /// Versionado, casa `^[a-z][a-z0-9]*([.-][a-z0-9]+)*\.v[0-9]+$`: `artisan.command.v1` (Fase 14) ou
    /// `command-group.suggestion.v1` (Fase 15). Sem alias sem `.vN`.
    pub schema_id: String,
    pub schema: Value,
    pub prompt: String,
    pub system: Option<String>,
    pub selection: Option<AiSelection>,
    pub pane_id: Option<PaneId>,
    pub project_id: Option<ProjectId>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StructuredResponse {
    pub request_id: AiRequestId,
    pub value: Value,
    pub repaired: bool,
    pub latency_ms: u32,
}

pub struct StructuredGenerator {
    compiled: DashMap<(String, [u8; 32]), Arc<jsonschema::Validator>>,
}

impl StructuredGenerator {
    pub fn new() -> Self;
    pub fn validate_schema_security(schema: &Value) -> MtResult<()>;
    pub async fn generate(
        &self,
        svc: &AiService,
        req: StructuredRequest,
        cancel: &CancellationToken,
    ) -> MtResult<StructuredResponse>;
}
```
- **Regras obrigatórias do `StructuredGenerator` (`CA-27`, `CA-36`):**
  1. `validate_schema_security(&req.schema)` verifica que o JSON serializado do schema tem `<= 32 KiB`, profundidade `<= 16` e percorre recursivamente todos os nós garantindo que qualquer chave `"$ref"` comece estritamente com `"#"` (referência local no mesmo documento). Qualquer `$ref` externo (`http://`, `https://`, `file://`, caminho relativo) é rejeitado imediatamente com `ai.schema_validation` (`CA-36`).
  1b. Valida `schema_id` contra o regex acima (`core.invalid_argument`, campo `schemaId`, se violar); ids de `BUILTIN_SCHEMA_IDS` são os contratos das Fases 14 e 15 e `artisan.command` / `command-group.suggestion` (sem `.vN`) são rejeitados. O schema em si vem do consumidor.
  2. Compila o `jsonschema::Validator` (draft 2020-12) e armazena no cache `compiled` indexado por `(schema_id, sha256(schema))` até o limite de 64 validadores.
  3. Envia a 1ª requisição ao gateway com `json_mode = true`. Se o JSON retornado não passar no validador (ou não for JSON sintaticamente válido), coleta até 10 mensagens de erro do validador (sanitizadas), monta o prompt de reparo (`prompts/structured.rs`) e faz **exatamente 1 tentativa adicional** dentro do mesmo `CancellationToken` e do mesmo deadline total (`structured_ms` = 90 s).
  4. Se a 2ª resposta passar na validação, retorna `StructuredResponse { repaired: true, .. }`. Se continuar inválida, retorna `Err(ai_error_with_details(codes::AI_SCHEMA_VALIDATION, "A resposta da IA não segue o formato esperado.", json!({ "errors": sanitized_errors })))` (`CA-27`).
  5. Sanitiza todas as strings dentro do `Value` final antes de entregá-lo ao chamador (`CA-36`).
  6. **Entrada pública única:** consumidores (Fases 14 e 15) usam somente `AiService::generate_structured(&self, req: StructuredRequest) -> MtResult<StructuredResponse>` (IPC `ai_generate_structured`); `StructuredGenerator::generate` é interno ao crate.

---

### 3.7. Serviço Agregador (`src-tauri/crates/mt-ai/src/lib.rs`)

```rust
pub mod catalog;
pub mod client;
pub mod config;
pub mod context;
pub mod diagnose;
pub mod error;
pub mod ids;
pub mod prompts;
pub mod rag;
pub mod registry;
pub mod repo;
pub mod risk;
pub mod structured;
pub mod suggest;

use crate::catalog::{CatalogService, HarnessCatalog};
use crate::client::http::{GatewayClient, GatewayHealth, GatewayStatus};
use crate::config::{AiSelection, AiSettings};
use crate::context::{AiContext, AiContextSource};
use crate::diagnose::{DiagnoseRequest, DiagnosisOutcome, DiagnosisService};
use crate::ids::AiRequestId;
use crate::rag::{RagSearchRequest, RagSearchResponse, RagService, RagSlug};
use crate::registry::AiRequestRegistry;
use crate::repo::{AiHistoryFilter, AiHistoryPage, AiRepo, CacheClearScope};
use crate::risk::{RiskAssessment, RiskClassifier};
use crate::structured::{StructuredGenerator, StructuredRequest, StructuredResponse};
use crate::suggest::{AiStreamEvent, CommandSuggestion, SuggestRequest};
use mt_core::{CommandBlockId, MtResult, PaneId, ProjectId};
use parking_lot::RwLock;
use std::sync::Arc;
use tokio::sync::watch;

/// Token opaco devolvido por `prepare_suggest`; não é `Clone` nem serializável.
pub struct SuggestTicket { /* RequestGuard, AiSelection resolvida, CancellationToken */ }

pub struct AiService {
    pub settings: RwLock<AiSettings>,
    pub client: Arc<GatewayClient>,
    pub catalog: CatalogService,
    pub risk: RiskClassifier,
    pub diagnosis: DiagnosisService,
    pub rag: RagService,
    structured: StructuredGenerator,
    pub requests: AiRequestRegistry,
    pub repo: Arc<AiRepo>,
    pub context: Arc<dyn AiContextSource>,
    status_tx: watch::Sender<GatewayHealth>,
    status_rx: watch::Receiver<GatewayHealth>,
}

impl AiService {
    pub async fn new(repo: Arc<AiRepo>, context: Arc<dyn AiContextSource>) -> MtResult<Arc<Self>>;
    pub fn subscribe_status(&self) -> watch::Receiver<GatewayHealth>;
    pub async fn health(&self) -> GatewayHealth;
    pub fn get_settings(&self) -> AiSettings;
    pub async fn update_settings(&self, next: AiSettings) -> MtResult<AiSettings>;
    pub async fn catalog_get(&self) -> HarnessCatalog;
    pub async fn catalog_refresh(&self) -> MtResult<HarnessCatalog>;
    pub async fn context_preview(&self, pane_id: PaneId) -> MtResult<AiContext>;
    pub fn classify_risk(&self, command: &str) -> RiskAssessment;
    /// Fase síncrona do fluxo de sugestão, usada pelo comando IPC `ai_suggest_command` (12/03 §3.2).
    /// Valida `settings.enabled` (`ai.disabled`), resolve a seleção (`req.selection` ou `default_selection`:
    /// `ai.no_default_selection` / `ai.unknown_selection` via `CatalogService::resolve`), cancela a sugestão
    /// anterior do mesmo `pane_id` (emitindo `Cancelled` no canal antigo) e registra `req.request_id` no
    /// `AiRequestRegistry::begin` (`ai.request_conflict` se duplicado; consome tombstone → `ai.cancelled`).
    /// Não faz I/O de rede nem `await`; o `SuggestTicket` devolvido carrega o `RequestGuard`, a `AiSelection`
    /// resolvida e o `CancellationToken`.
    pub fn prepare_suggest(&self, req: &SuggestRequest) -> MtResult<SuggestTicket>;
    /// Fase assíncrona: monta `AiContext` (`AiContextSource::for_pane`), chama o gateway com stream, emite
    /// `Started`/`Delta`/`Completed|Failed|Cancelled` por `on_event` (um único produtor; nenhum evento após o
    /// terminal) e grava histórico sanitizado. `on_event` retorna `false` quando o Channel falhou; nesse caso a
    /// requisição é cancelada e o guard liberado. Erros de rede chegam como `Failed` e também no `Err` de retorno.
    pub async fn run_suggest_stream(
        &self,
        req: SuggestRequest,
        ticket: SuggestTicket,
        on_event: impl Fn(AiStreamEvent) -> bool + Send + Sync + 'static,
    ) -> MtResult<CommandSuggestion>;
    /// Conveniência para consumidores em processo e testes: `prepare_suggest` + `run_suggest_stream`
    /// (callback sem retorno de saúde do canal). O IPC usa a forma em duas fases porque o aceite do invoke
    /// precisa ser síncrono.
    pub async fn suggest_command(
        &self,
        req: SuggestRequest,
        on_event: impl Fn(AiStreamEvent) + Send + Sync + 'static,
    ) -> MtResult<CommandSuggestion>;
    pub fn cancel(&self, request_id: AiRequestId) -> bool;
    pub async fn diagnose_block(&self, req: DiagnoseRequest) -> MtResult<DiagnosisOutcome>;
    pub fn dismiss_diagnosis(&self, block_id: CommandBlockId);
    pub async fn rag_list_slugs(&self) -> MtResult<Vec<RagSlug>>;
    pub async fn rag_search(&self, req: RagSearchRequest) -> MtResult<RagSearchResponse>;
    /// ÚNICO ponto de entrada público para geração estruturada por consumidores (Fases 14 e 15, via IPC
    /// `ai_generate_structured` e adaptadores TS). Nenhum consumidor chama `StructuredGenerator` diretamente
    /// nem cria rota/cliente próprio; validação de schema, reparo único, sanitização, histórico e registro
    /// de requisição acontecem aqui.
    pub async fn generate_structured(&self, req: StructuredRequest) -> MtResult<StructuredResponse>;
    pub async fn history_list(&self, filter: AiHistoryFilter) -> MtResult<AiHistoryPage>;
    pub async fn history_clear(&self, project_id: Option<ProjectId>) -> MtResult<u32>;
    pub async fn cache_clear(&self, scope: CacheClearScope) -> MtResult<u32>;
}
```
- Quando `ai_suggest_command` é iniciado para um `pane_id` que já possui outra sugestão em andamento, `AiService` cancela primeiro a sugestão anterior daquele `pane_id` (emitindo `AiStreamEvent::Cancelled` na canaleta antiga) antes de iniciar a nova (`spec.md` §3).
- Ao executar `update_settings`: se `base_url`, `allow_remote` ou `enabled` mudarem, `AiService` chama `requests.cancel_all()`, `diagnosis.cancel_all_pending()`, `client.reconfigure(&saved)` e `catalog.invalidate_memory()`.
