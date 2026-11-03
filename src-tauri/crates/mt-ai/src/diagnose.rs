//! Motor de Diagnóstico Rápido Inline (`DiagnosisService`) com cache SHA-256 por 30 dias,
//! substituição por painel (`Superseded`), fila limitada (`3`) + semáforo (`1`) e rate limiter.

use crate::AiService;
use crate::client::dialect::ChatRequest;
use crate::client::http::{Op, format_unix_ms_iso8601, now_iso8601};
use crate::config::AiSelection;
use crate::context::{SANITIZER_VERSION, strip_ansi_and_sanitize};
use crate::error::{ai_error, codes};
use crate::ids::AiRequestId;
use crate::prompts::diagnose::build_diagnose_messages;
use crate::registry::{RequestMeta, TerminalState};
use crate::repo::{AiHistoryItem, AiHistoryKind, CachedDiagnosisRow};
use crate::risk::{RiskAssessment, RiskClassifier, RiskLevel};
use crate::suggest::{extract_strict_json_object, truncate_chars};
use mt_core::MtResult;
use mt_core::ids::{CommandBlockId, PaneId, ProjectId};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

/// Versão do template de prompt de diagnóstico.
pub const PROMPT_VERSION_DIAGNOSE: u32 = 1;
/// Limite máximo de caracteres no resumo do diagnóstico (`<= 160` — `CA-23`).
pub const MAX_DIAGNOSIS_SUMMARY_CHARS: usize = 160;
/// Quantidade máxima de diagnósticos aguardando na fila do semáforo.
pub const MAX_DIAGNOSIS_QUEUE_WAITING: usize = 3;
/// Tempo de vida em dias de entradas em `ai_diagnosis_cache`.
pub const DIAGNOSIS_CACHE_TTL_DAYS: i64 = 30;

/// Diagnóstico rápido inline pronto para exibição ao lado do botão "Inspecionar".
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InlineDiagnosis {
    /// Bloco de comando diagnosticado.
    pub block_id: CommandBlockId,
    /// Hash SHA-256 canônico do erro.
    pub error_hash: String,
    /// Resumo curto da causa em português do Brasil (`<= 160` caracteres).
    pub summary: String,
    /// Comando corretivo opcional (em linha única).
    pub fix_command: Option<String>,
    /// Avaliação de risco do `fix_command` quando presente.
    pub fix_risk: Option<RiskAssessment>,
    /// Indica se o resultado veio do cache SQLite (`0` chamadas HTTP).
    pub from_cache: bool,
    /// Timestamp ISO-8601 UTC de criação.
    pub created_at: String,
}

/// Razão pela qual o diagnóstico automático foi ignorado sem erro.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosisSkip {
    /// Diagnóstico desativado nas configurações.
    Disabled,
    /// Projeto configurado com opt-out de diagnóstico automático.
    ProjectOptOut,
    /// Código de saída ignorado (ex.: `0`, `null` ou `130`).
    IgnoredExitCode,
    /// Bloco executado em TUI interativo ou aba de Agente CLI.
    Interactive,
    /// Limite de diagnósticos por minuto atingido.
    RateLimited,
    /// Substituído por um erro mais recente no mesmo painel ou na fila.
    Superseded,
}

/// Resultado de `ai_diagnose_block`.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum DiagnosisOutcome {
    /// Diagnóstico concluído (via gateway ou cache).
    Ready {
        /// Dados do diagnóstico.
        diagnosis: InlineDiagnosis,
    },
    /// Diagnóstico pulado por regra de filtragem/concorrência.
    Skipped {
        /// Motivo do descarte.
        reason: DiagnosisSkip,
    },
}

/// Requisição IPC para diagnosticar um bloco que terminou com erro.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DiagnoseRequest {
    /// Identificador único da requisição.
    pub request_id: AiRequestId,
    /// Identificador do bloco de comando com falha.
    pub block_id: CommandBlockId,
}

/// Chave canônica serializada para cálculo do `error_hash` SHA-256 (`CA-21`, `R-08`).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosisCacheKey {
    /// Chave do gateway ativo.
    pub gateway_key: String,
    /// Projeto associado.
    pub project_id: Option<ProjectId>,
    /// Diretório de trabalho normalizado.
    pub cwd: String,
    /// Nome do shell.
    pub shell: String,
    /// Comando executado.
    pub command: String,
    /// Código de saída não-zero.
    pub exit_code: i32,
    /// Cauda de saída normalizada e sanitizada.
    pub sanitized_tail: String,
    /// Seleção utilizada.
    pub selection: AiSelection,
    /// Versão do prompt.
    pub prompt_version: u32,
    /// Versão do sanitizador.
    pub sanitizer_version: u32,
}

struct RateWindow {
    timestamps: VecDeque<Instant>,
}

impl RateWindow {
    fn new() -> Self {
        Self {
            timestamps: VecDeque::new(),
        }
    }

    fn try_acquire(&mut self, max_per_minute: u32) -> bool {
        if max_per_minute == 0 {
            return false;
        }
        let now = Instant::now();
        let window = Duration::from_secs(60);
        self.timestamps
            .retain(|ts| now.duration_since(*ts) < window);
        if self.timestamps.len() >= max_per_minute as usize {
            return false;
        }
        self.timestamps.push_back(now);
        true
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDiagnosisModelResponse {
    summary: String,
    #[serde(default)]
    fix_command: Option<String>,
    #[serde(default)]
    risk: Option<RiskLevel>,
}

/// Serviço coordenador de Diagnóstico Rápido Inline (`CA-20`..`CA-24`, `CA-34`).
pub struct DiagnosisService {
    semaphore: Semaphore,
    waiting_queue: Mutex<VecDeque<(AiRequestId, CancellationToken)>>,
    pending_by_pane: RwLock<HashMap<PaneId, (AiRequestId, CancellationToken)>>,
    dismissed_blocks: RwLock<HashSet<CommandBlockId>>,
    limiter: Mutex<RateWindow>,
}

impl Default for DiagnosisService {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagnosisService {
    /// Cria uma nova instância do `DiagnosisService`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            semaphore: Semaphore::new(1),
            waiting_queue: Mutex::new(VecDeque::new()),
            pending_by_pane: RwLock::new(HashMap::new()),
            dismissed_blocks: RwLock::new(HashSet::new()),
            limiter: Mutex::new(RateWindow::new()),
        }
    }

    /// Calcula o `error_hash` SHA-256 hexadecimal de 64 caracteres sobre a `DiagnosisCacheKey`.
    #[must_use]
    pub fn error_hash(key: &DiagnosisCacheKey) -> String {
        let serialized = serde_json::to_vec(key).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(&serialized);
        hex::encode(hasher.finalize())
    }

    /// Normaliza a cauda de saída removendo escapes ANSI e normalizando quebras de linha,
    /// mas preservando caminhos de arquivos, números de linha, argumentos e códigos de erro (`R-08`).
    #[must_use]
    pub fn normalize(tail: &str) -> String {
        let clean = strip_ansi_and_sanitize(tail);
        clean
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .trim_end()
            .to_string()
    }

    /// Marca um `CommandBlockId` como dispensado pelo usuário e cancela eventual diagnóstico pendente (`CA-34`).
    pub fn dismiss_block(&self, block_id: CommandBlockId) {
        self.dismissed_blocks.write().insert(block_id);
    }

    /// Verifica se o bloco foi dispensado pelo usuário.
    #[must_use]
    pub fn is_dismissed(&self, block_id: CommandBlockId) -> bool {
        self.dismissed_blocks.read().contains(&block_id)
    }

    /// Cancela todos os diagnósticos pendentes na fila e por painel (chamado ao desativar diagnóstico — `CA-24`).
    pub fn cancel_all_pending(&self) {
        for (_, (_, tok)) in self.pending_by_pane.write().drain() {
            tok.cancel();
        }
        for (_, tok) in self.waiting_queue.lock().drain(..) {
            tok.cancel();
        }
    }

    /// Executa o fluxo completo de diagnóstico de um bloco com falha.
    #[allow(clippy::too_many_lines)]
    pub async fn diagnose(
        &self,
        svc: &AiService,
        req: DiagnoseRequest,
    ) -> MtResult<DiagnosisOutcome> {
        let settings = svc.get_settings();
        if !settings.enabled || !settings.diagnosis.enabled {
            return Ok(DiagnosisOutcome::Skipped {
                reason: DiagnosisSkip::Disabled,
            });
        }

        if self.is_dismissed(req.block_id) {
            return Ok(DiagnosisOutcome::Skipped {
                reason: DiagnosisSkip::Disabled,
            });
        }

        let (ctx, failure) = svc
            .context
            .for_block(req.block_id, settings.context.output_tail_bytes)
            .await?;

        if failure.is_interactive {
            return Ok(DiagnosisOutcome::Skipped {
                reason: DiagnosisSkip::Interactive,
            });
        }

        let Some(exit_code) = failure.exit_code else {
            return Ok(DiagnosisOutcome::Skipped {
                reason: DiagnosisSkip::IgnoredExitCode,
            });
        };

        if exit_code == 0 || settings.diagnosis.ignored_exit_codes.contains(&exit_code) {
            return Ok(DiagnosisOutcome::Skipped {
                reason: DiagnosisSkip::IgnoredExitCode,
            });
        }

        if let Some(proj_id) = failure.project_id
            && settings.diagnosis.disabled_projects.contains(&proj_id)
        {
            return Ok(DiagnosisOutcome::Skipped {
                reason: DiagnosisSkip::ProjectOptOut,
            });
        }

        // Registra requisição no AiRequestRegistry para suportar cancelamento por painel/global
        let guard = svc.requests.begin(
            req.request_id,
            RequestMeta {
                pane_id: Some(failure.pane_id),
                session_id: Some(failure.session_id),
                pty_id: Some(failure.pty_id),
                config_revision: settings.revision,
            },
        )?;

        // Substituição por painel: cancela eventual diagnóstico pendente do mesmo pane_id
        let supersede_token = CancellationToken::new();
        {
            let mut by_pane = self.pending_by_pane.write();
            if let Some((_, prev_tok)) =
                by_pane.insert(failure.pane_id, (req.request_id, supersede_token.clone()))
            {
                prev_tok.cancel();
            }
        }

        let catalog = svc.catalog_get().await;
        let selection = resolve_diagnosis_selection(&settings, &svc.catalog, &catalog)?;
        let normalized_tail = Self::normalize(&failure.output_tail);
        let gw_key = svc.client.gateway_key();

        let cache_key = DiagnosisCacheKey {
            gateway_key: gw_key.clone(),
            project_id: failure.project_id,
            cwd: ctx.cwd.clone(),
            shell: ctx.shell.clone(),
            command: failure.command.clone(),
            exit_code,
            sanitized_tail: normalized_tail.clone(),
            selection: selection.clone(),
            prompt_version: PROMPT_VERSION_DIAGNOSE,
            sanitizer_version: SANITIZER_VERSION,
        };
        let error_hash = Self::error_hash(&cache_key);
        let now_str = now_iso8601();

        // 1. Verifica cache SQLite antes do rate limit e sem chamada HTTP (CA-21)
        if let Some(cached) = svc.repo.get_diagnosis_cache(&error_hash, &now_str).await? {
            self.cleanup_pane_pending(failure.pane_id, req.request_id);
            let fix_risk = cached.fix_command.as_deref().map(|cmd| {
                let local = svc.risk.classify(cmd);
                let hint = cached.fix_risk.as_deref().and_then(RiskLevel::from_str_opt);
                RiskClassifier::combine(local, hint)
            });
            let _ = guard.try_finish(TerminalState::Completed);
            return Ok(DiagnosisOutcome::Ready {
                diagnosis: InlineDiagnosis {
                    block_id: req.block_id,
                    error_hash,
                    summary: truncate_chars(&cached.summary, MAX_DIAGNOSIS_SUMMARY_CHARS),
                    fix_command: cached.fix_command,
                    fix_risk,
                    from_cache: true,
                    created_at: cached.created_at,
                },
            });
        }

        // 2. Verifica fila de espera limitada (capacidade 3)
        {
            let mut q = self.waiting_queue.lock();
            if q.len() >= MAX_DIAGNOSIS_QUEUE_WAITING
                && let Some((_, oldest_tok)) = q.pop_front()
            {
                oldest_tok.cancel();
            }
            q.push_back((req.request_id, supersede_token.clone()));
        }

        // 3. Aguarda o semáforo (concorrência = 1)
        let permit = tokio::select! {
            biased;
            () = guard.token().cancelled() => {
                self.remove_from_waiting(req.request_id);
                self.cleanup_pane_pending(failure.pane_id, req.request_id);
                return Err(ai_error(codes::AI_CANCELLED, "Diagnóstico cancelado."));
            }
            () = supersede_token.cancelled() => {
                self.remove_from_waiting(req.request_id);
                self.cleanup_pane_pending(failure.pane_id, req.request_id);
                return Ok(DiagnosisOutcome::Skipped {
                    reason: DiagnosisSkip::Superseded,
                });
            }
            res = self.semaphore.acquire() => {
                self.remove_from_waiting(req.request_id);
                res.map_err(|_| ai_error(codes::AI_CANCELLED, "Semáforo de diagnóstico encerrado."))?
            }
        };

        // 4. Revalida estado após obter o semáforo
        if guard.is_cancelled() {
            drop(permit);
            self.cleanup_pane_pending(failure.pane_id, req.request_id);
            return Err(ai_error(codes::AI_CANCELLED, "Diagnóstico cancelado."));
        }
        if supersede_token.is_cancelled() {
            drop(permit);
            self.cleanup_pane_pending(failure.pane_id, req.request_id);
            return Ok(DiagnosisOutcome::Skipped {
                reason: DiagnosisSkip::Superseded,
            });
        }
        let latest_settings = svc.get_settings();
        if !latest_settings.enabled
            || !latest_settings.diagnosis.enabled
            || self.is_dismissed(req.block_id)
        {
            drop(permit);
            self.cleanup_pane_pending(failure.pane_id, req.request_id);
            return Ok(DiagnosisOutcome::Skipped {
                reason: DiagnosisSkip::Disabled,
            });
        }
        if let Some(snap) = svc.context.pane_snapshot(failure.pane_id)
            && (snap.session_id != failure.session_id || snap.pty_id != failure.pty_id)
        {
            drop(permit);
            self.cleanup_pane_pending(failure.pane_id, req.request_id);
            return Err(ai_error(
                codes::AI_CONTEXT_STALE,
                "A sessão ou o PTY do painel mudou antes da conclusão do diagnóstico.",
            ));
        }

        // 5. Aplica rate limiter apenas para chamadas reais de rede (cache miss)
        if !self
            .limiter
            .lock()
            .try_acquire(latest_settings.diagnosis.max_per_minute)
        {
            drop(permit);
            self.cleanup_pane_pending(failure.pane_id, req.request_id);
            return Ok(DiagnosisOutcome::Skipped {
                reason: DiagnosisSkip::RateLimited,
            });
        }

        let cache_gen = svc.repo.current_cache_generation();
        let history_gen = svc.repo.current_history_generation();
        let start = Instant::now();
        let messages = build_diagnose_messages(&ctx, &failure.command, exit_code, &normalized_tail);
        let chat_req = ChatRequest {
            selection: selection.clone(),
            messages,
            json_mode: true,
            stream: false,
            max_tokens: Some(400),
        };

        let combined_cancel = CancellationToken::new();
        let c_guard = guard.token().clone();
        let c_sup = supersede_token.clone();
        let c_inner = combined_cancel.clone();
        let watcher = tokio::spawn(async move {
            tokio::select! {
                () = c_guard.cancelled() => c_inner.cancel(),
                () = c_sup.cancelled() => c_inner.cancel(),
            }
        });

        let mut noop_delta = |_: &str| {};
        let chat_outcome = svc
            .client
            .chat(&chat_req, Op::Diagnosis, &combined_cancel, &mut noop_delta)
            .await;
        watcher.abort();
        drop(permit);
        self.cleanup_pane_pending(failure.pane_id, req.request_id);

        if supersede_token.is_cancelled() && !guard.is_cancelled() {
            return Ok(DiagnosisOutcome::Skipped {
                reason: DiagnosisSkip::Superseded,
            });
        }

        let chat_resp = match chat_outcome {
            Ok(r) => r,
            Err(err) => {
                let _ = guard.try_finish(TerminalState::Failed);
                return Err(err);
            }
        };

        let json_slice = extract_strict_json_object(&chat_resp.content)?;
        let raw_diag: RawDiagnosisModelResponse =
            serde_json::from_str(json_slice).map_err(|_| {
                ai_error(
                    codes::AI_BAD_RESPONSE,
                    "Resposta JSON de diagnóstico malformada.",
                )
            })?;

        let summary = truncate_chars(
            &strip_ansi_and_sanitize(raw_diag.summary.trim()),
            MAX_DIAGNOSIS_SUMMARY_CHARS,
        );
        if summary.is_empty() {
            let _ = guard.try_finish(TerminalState::Failed);
            return Err(ai_error(
                codes::AI_BAD_RESPONSE,
                "Resumo de diagnóstico vazio retornado pela IA.",
            ));
        }

        let fix_command = raw_diag
            .fix_command
            .as_deref()
            .map(str::trim)
            .filter(|s| {
                !s.is_empty()
                    && !s
                        .chars()
                        .any(|c| c == '\n' || c == '\r' || c == '\0' || c == '\x1b')
            })
            .map(|s| mt_db::sanitizer::sanitize(s).into_owned());

        let fix_risk = fix_command.as_deref().map(|cmd| {
            let local = svc.risk.classify(cmd);
            RiskClassifier::combine(local, raw_diag.risk)
        });

        let created_at = now_iso8601();
        let expires_ms = mt_db::time::now_unix_ms() + DIAGNOSIS_CACHE_TTL_DAYS * 24 * 3600 * 1000;
        let expires_at = format_unix_ms_iso8601(expires_ms);

        let _ = svc
            .repo
            .put_diagnosis_cache(
                cache_gen,
                &CachedDiagnosisRow {
                    error_hash: error_hash.clone(),
                    gateway_key: gw_key,
                    project_id: failure.project_id,
                    summary: summary.clone(),
                    fix_command: fix_command.clone(),
                    fix_risk: fix_risk.as_ref().map(|r| r.level.as_str().to_string()),
                    harness_id: selection.harness_id.clone(),
                    model_id: selection.model_id.clone(),
                    reasoning_id: selection.reasoning_id.clone(),
                    prompt_version: PROMPT_VERSION_DIAGNOSE,
                    sanitizer_version: SANITIZER_VERSION,
                    hits: 0,
                    created_at: created_at.clone(),
                    expires_at,
                },
            )
            .await;

        let latency_ms = u32::try_from(start.elapsed().as_millis()).unwrap_or(u32::MAX);
        let _ = svc
            .repo
            .insert_history(
                history_gen,
                latest_settings.save_history,
                AiHistoryItem {
                    id: req.request_id,
                    kind: AiHistoryKind::Diagnosis,
                    trigger: None,
                    project_id: failure.project_id,
                    tab_id: svc.context.pane_snapshot(failure.pane_id).map(|s| s.tab_id),
                    pane_id: Some(failure.pane_id),
                    session_id: Some(failure.session_id),
                    block_id: Some(req.block_id),
                    harness_id: Some(selection.harness_id),
                    model_id: Some(selection.model_id),
                    reasoning_id: selection.reasoning_id,
                    rag_slug: None,
                    schema_id: None,
                    prompt_sanitized: failure.command,
                    response_sanitized: Some(summary.clone()),
                    risk_level: fix_risk.as_ref().map(|r| r.level.as_str().to_string()),
                    status: "completed".to_string(),
                    error_code: None,
                    latency_ms: Some(latency_ms),
                    created_at: created_at.clone(),
                },
            )
            .await;

        let _ = guard.try_finish(TerminalState::Completed);
        Ok(DiagnosisOutcome::Ready {
            diagnosis: InlineDiagnosis {
                block_id: req.block_id,
                error_hash,
                summary,
                fix_command,
                fix_risk,
                from_cache: false,
                created_at,
            },
        })
    }

    fn remove_from_waiting(&self, request_id: AiRequestId) {
        let mut q = self.waiting_queue.lock();
        q.retain(|(id, _)| *id != request_id);
    }

    fn cleanup_pane_pending(&self, pane_id: PaneId, request_id: AiRequestId) {
        let mut by_pane = self.pending_by_pane.write();
        if by_pane
            .get(&pane_id)
            .is_some_and(|(active_id, _)| *active_id == request_id)
        {
            by_pane.remove(&pane_id);
        }
    }
}

fn resolve_diagnosis_selection(
    settings: &crate::config::AiSettings,
    catalog_svc: &crate::catalog::CatalogService,
    catalog: &crate::catalog::HarnessCatalog,
) -> MtResult<AiSelection> {
    if let Some(explicit) = &settings.diagnosis.selection {
        return Ok(catalog_svc.resolve(catalog, explicit)?.selection);
    }
    if let Some(def) = &settings.default_selection {
        let mut adjusted = def.clone();
        if let Some(lowest) = catalog_svc.lowest_reasoning(catalog, &def.harness_id, &def.model_id)
        {
            adjusted.reasoning_id = Some(lowest);
        }
        return Ok(catalog_svc.resolve(catalog, &adjusted)?.selection);
    }
    for h in &catalog.harnesses {
        if let Some(m) = h.models.first() {
            let lowest = m
                .reasoning_levels
                .iter()
                .min_by_key(|r| r.rank)
                .map(|r| r.id.clone());
            return Ok(AiSelection {
                harness_id: h.id.clone(),
                model_id: m.id.clone(),
                reasoning_id: lowest,
            });
        }
    }
    Err(ai_error(
        codes::AI_NO_DEFAULT_SELECTION,
        "Configure a seleção padrão em Preferências > IA para utilizar o diagnóstico.",
    ))
}
