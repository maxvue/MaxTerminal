//! Crate `mt-ai`: integração nativa com o gateway local `MaxAiManager` (`X-Agent-Name: MaxTerminal`),
//! catálogo em cascata (**Harness → Modelo → Nível de Raciocínio**), conversão NL→Shell (`@` e `?`),
//! classificador determinístico de risco, diagnóstico rápido inline, consulta RAG e geração estruturada.
//!
//! Este crate depende exclusivamente de `mt-core` e `mt-db` (sem dependência de `mt-pty` ou `tauri`),
//! garantindo por construção que a camada de IA jamais escreva diretamente em PTYs (`CA-15`).

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
use crate::client::dialect::{ChatRequest, GatewayDialect};
use crate::client::http::{GatewayClient, GatewayHealth, GatewayStatus, Op, now_iso8601};
use crate::config::{AiSelection, AiSettings};
use crate::context::{AiContext, AiContextSource};
use crate::diagnose::{DiagnoseRequest, DiagnosisOutcome, DiagnosisService};
use crate::error::{ai_error, codes};
use crate::ids::AiRequestId;
use crate::prompts::suggest::build_suggest_messages;
use crate::rag::{RagSearchRequest, RagSearchResponse, RagService, RagSlug};
use crate::registry::{AiRequestRegistry, RequestGuard, RequestMeta, TerminalState};
use crate::repo::{
    AiHistoryFilter, AiHistoryItem, AiHistoryKind, AiHistoryPage, AiRepo, CacheClearScope,
};
use crate::risk::{RiskAssessment, RiskClassifier};
use crate::structured::{StructuredGenerator, StructuredRequest, StructuredResponse};
use crate::suggest::{
    AiStreamEvent, CommandSuggestion, MAX_SUGGEST_PROMPT_CHARS, SuggestRequest, parse_suggestion,
};
use mt_core::MtResult;
use mt_core::ids::{CommandBlockId, PaneId, ProjectId};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::watch;

/// Token opaco devolvido por `AiService::prepare_suggest`; não é `Clone` nem serializável.
pub struct SuggestTicket {
    guard: RequestGuard,
    selection: AiSelection,
}

/// Fachada principal de Inteligência Artificial do `MaxTerminal`.
pub struct AiService {
    /// Configurações ativas em memória sincronizadas com `ai_settings`.
    pub settings: RwLock<AiSettings>,
    /// Cliente HTTP/SSE do gateway.
    pub client: Arc<GatewayClient>,
    /// Serviço de catálogo de harnesses/modelos/raciocínio.
    pub catalog: CatalogService,
    /// Classificador determinístico de risco de comandos shell.
    pub risk: RiskClassifier,
    /// Serviço de diagnóstico rápido inline.
    pub diagnosis: DiagnosisService,
    /// Serviço de busca RAG.
    pub rag: RagService,
    structured: StructuredGenerator,
    /// Registro de requisições ativas e tombstones de cancelamento.
    pub requests: AiRequestRegistry,
    /// Repositório SQLite (`ai_settings`, caches e histórico).
    pub repo: Arc<AiRepo>,
    /// Fonte desacoplada de contexto do terminal.
    pub context: Arc<dyn AiContextSource>,
    status_tx: watch::Sender<GatewayHealth>,
    status_rx: watch::Receiver<GatewayHealth>,
    active_suggest_by_pane: RwLock<HashMap<PaneId, AiRequestId>>,
}

impl AiService {
    /// Inicializa o `AiService` carregando as configurações persistidas no SQLite.
    pub async fn new(repo: Arc<AiRepo>, context: Arc<dyn AiContextSource>) -> MtResult<Arc<Self>> {
        let initial_settings = repo.get_settings().await.unwrap_or_default();
        let client = Arc::new(GatewayClient::new(&initial_settings)?);
        Ok(Self::with_client(repo, context, initial_settings, client))
    }

    /// Inicializa o `AiService` com um dialeto customizado (útil para testes).
    pub async fn with_dialect(
        repo: Arc<AiRepo>,
        context: Arc<dyn AiContextSource>,
        dialect: Arc<dyn GatewayDialect>,
    ) -> MtResult<Arc<Self>> {
        let initial_settings = repo.get_settings().await.unwrap_or_default();
        let client = Arc::new(GatewayClient::with_dialect(&initial_settings, dialect)?);
        Ok(Self::with_client(repo, context, initial_settings, client))
    }

    fn with_client(
        repo: Arc<AiRepo>,
        context: Arc<dyn AiContextSource>,
        initial_settings: AiSettings,
        client: Arc<GatewayClient>,
    ) -> Arc<Self> {
        let initial_health = GatewayHealth {
            status: if !initial_settings.enabled {
                GatewayStatus::Disabled
            } else if initial_settings.base_url.is_none() {
                GatewayStatus::Unconfigured
            } else {
                GatewayStatus::Offline
            },
            version: None,
            latency_ms: None,
            base_url: initial_settings.base_url.clone(),
            checked_at: now_iso8601(),
            config_revision: initial_settings.revision,
            capabilities: client.capabilities(),
        };
        let (status_tx, status_rx) = watch::channel(initial_health);
        let catalog = CatalogService::new(Arc::clone(&client), Arc::clone(&repo));

        Arc::new(Self {
            settings: RwLock::new(initial_settings),
            client,
            catalog,
            risk: RiskClassifier::default_rules(),
            diagnosis: DiagnosisService::new(),
            rag: RagService,
            structured: StructuredGenerator::new(),
            requests: AiRequestRegistry::new(),
            repo,
            context,
            status_tx,
            status_rx,
            active_suggest_by_pane: RwLock::new(HashMap::new()),
        })
    }

    /// Retorna um `watch::Receiver<GatewayHealth>` para acompanhar mudanças de status do gateway.
    #[must_use]
    pub fn subscribe_status(&self) -> watch::Receiver<GatewayHealth> {
        self.status_rx.clone()
    }

    /// Executa a verificação de saúde do gateway e notifica observadores se o status mudou.
    pub async fn health(&self) -> GatewayHealth {
        let h = self
            .client
            .health()
            .await
            .unwrap_or_else(|_| GatewayHealth {
                status: GatewayStatus::Offline,
                version: None,
                latency_ms: None,
                base_url: self.get_settings().base_url,
                checked_at: now_iso8601(),
                config_revision: self.client.config_revision(),
                capabilities: self.client.capabilities(),
            });
        let _ = self.status_tx.send(h.clone());
        h
    }

    /// Retorna um clone das configurações ativas em memória.
    #[must_use]
    pub fn get_settings(&self) -> AiSettings {
        self.settings.read().clone()
    }

    /// Valida, persiste e aplica novas configurações em quente (`CA-06`, `CA-24`).
    pub async fn update_settings(&self, next: AiSettings) -> MtResult<AiSettings> {
        let prev = self.get_settings();
        let saved = self.repo.save_settings(next).await?;

        let connection_changed = prev.base_url != saved.base_url
            || prev.allow_remote != saved.allow_remote
            || prev.enabled != saved.enabled;

        if connection_changed {
            let _ = self.requests.cancel_all();
            self.diagnosis.cancel_all_pending();
            self.catalog.invalidate_memory();
        } else if !saved.diagnosis.enabled && prev.diagnosis.enabled {
            self.diagnosis.cancel_all_pending();
        }

        self.client.reconfigure(&saved)?;
        *self.settings.write() = saved.clone();
        let _ = self.health().await;

        Ok(saved)
    }

    /// Obtém o catálogo de harnesses (memória -> gateway -> cache SQLite -> fallback de identidades).
    pub async fn catalog_get(&self) -> HarnessCatalog {
        self.catalog.get().await
    }

    /// Força a revalidação do catálogo junto ao gateway.
    pub async fn catalog_refresh(&self) -> MtResult<HarnessCatalog> {
        self.catalog.refresh().await
    }

    /// Monta o preview sanitizado do contexto que será enviado ao gateway para um painel (`CA-18`).
    pub async fn context_preview(&self, pane_id: PaneId) -> MtResult<AiContext> {
        let opts = self.get_settings().context;
        let raw_ctx = self.context.for_pane(pane_id, &opts).await?;
        Ok(raw_ctx.sanitize_and_bound(opts.recent_commands, opts.output_tail_bytes))
    }

    /// Classifica deterministicamente o risco de um comando shell (`CA-16`, `CA-19`).
    #[must_use]
    pub fn classify_risk(&self, command: &str) -> RiskAssessment {
        self.risk.classify(command)
    }

    /// Fase síncrona do fluxo de sugestão (`ai_suggest_command`):
    /// Valida `settings.enabled`, resolve a seleção, cancela eventual sugestão anterior do mesmo `pane_id`
    /// e registra `req.request_id` no `AiRequestRegistry` antes do spawn assíncrono.
    pub fn prepare_suggest(&self, req: &SuggestRequest) -> MtResult<SuggestTicket> {
        let settings = self.get_settings();
        if !settings.enabled {
            return Err(ai_error(
                codes::AI_DISABLED,
                "A integração com o MaxAiManager está desativada nas preferências.",
            ));
        }

        if req.prompt.trim().is_empty() {
            return Err(ai_error(
                codes::AI_EMPTY_COMMAND,
                "Informe uma descrição do comando desejado.",
            ));
        }

        let raw_sel = match &req.selection {
            Some(sel) => sel.clone(),
            None => settings.default_selection.clone().ok_or_else(|| {
                ai_error(
                    codes::AI_NO_DEFAULT_SELECTION,
                    "Configure o padrão em Preferências > IA.",
                )
            })?,
        };

        // Se o catálogo já estiver carregado em memória com modelos, valida sincronamente a seleção
        let snap_cat = self.catalog.snapshot_or_fallback();
        let has_any_models = snap_cat.harnesses.iter().any(|h| !h.models.is_empty());
        let resolved_sel = if has_any_models {
            self.catalog.resolve(&snap_cat, &raw_sel)?.selection
        } else {
            // Verifica ao menos que o harness_id não é vazio
            if raw_sel.harness_id.trim().is_empty() || raw_sel.model_id.trim().is_empty() {
                return Err(ai_error(
                    codes::AI_UNKNOWN_SELECTION,
                    "Selecione um Harness e Modelo válidos.",
                ));
            }
            raw_sel
        };

        // Cancela eventual sugestão anterior em andamento no mesmo painel
        {
            let mut active_map = self.active_suggest_by_pane.write();
            if let Some(prev_id) = active_map.insert(req.pane_id, req.request_id)
                && prev_id != req.request_id
            {
                let _ = self.requests.cancel(prev_id);
            }
        }

        let pane_snap = self.context.pane_snapshot(req.pane_id);
        let guard = self.requests.begin(
            req.request_id,
            RequestMeta {
                pane_id: Some(req.pane_id),
                session_id: pane_snap.as_ref().map(|s| s.session_id),
                pty_id: pane_snap.as_ref().map(|s| s.pty_id),
                config_revision: settings.revision,
            },
        )?;

        Ok(SuggestTicket {
            guard,
            selection: resolved_sel,
        })
    }

    /// Fase assíncrona do fluxo de sugestão (`ai_suggest_command`):
    /// Monta o `AiContext`, transmite `Started`/`Delta`/`Completed|Failed|Cancelled` via `on_event`
    /// (garantindo exatamente um evento terminal) e grava o histórico sanitizado no SQLite.
    #[allow(clippy::too_many_lines)]
    pub async fn run_suggest_stream(
        &self,
        req: SuggestRequest,
        ticket: SuggestTicket,
        on_event: impl Fn(AiStreamEvent) -> bool + Send + Sync + 'static,
    ) -> MtResult<CommandSuggestion> {
        let SuggestTicket {
            guard,
            mut selection,
        } = ticket;
        let req_id = req.request_id;
        let start = Instant::now();
        let settings = self.get_settings();
        let history_gen = self.repo.current_history_generation();
        let initial_snap = self.context.pane_snapshot(req.pane_id);

        // Valida a seleção contra o catálogo atualizado (caso não estivesse em memória no prepare_suggest)
        let catalog = self.catalog_get().await;
        if catalog.harnesses.iter().any(|h| !h.models.is_empty()) {
            match self.catalog.resolve(&catalog, &selection) {
                Ok(resolved) => selection = resolved.selection,
                Err(err) => {
                    self.clear_pane_suggest(req.pane_id, req_id);
                    if guard.try_finish(TerminalState::Failed) {
                        let _ = on_event(AiStreamEvent::Failed {
                            request_id: req_id,
                            error: err.clone(),
                        });
                    } else {
                        let _ = on_event(AiStreamEvent::Cancelled { request_id: req_id });
                    }
                    return Err(err);
                }
            }
        }

        let raw_ctx = match self.context.for_pane(req.pane_id, &settings.context).await {
            Ok(c) => c.sanitize_and_bound(
                settings.context.recent_commands,
                settings.context.output_tail_bytes,
            ),
            Err(err) => {
                self.clear_pane_suggest(req.pane_id, req_id);
                if guard.try_finish(TerminalState::Failed) {
                    let _ = on_event(AiStreamEvent::Failed {
                        request_id: req_id,
                        error: err.clone(),
                    });
                } else {
                    let _ = on_event(AiStreamEvent::Cancelled { request_id: req_id });
                }
                return Err(err);
            }
        };

        if guard.is_cancelled() {
            self.clear_pane_suggest(req.pane_id, req_id);
            let _ = guard.try_finish(TerminalState::Cancelled);
            let _ = on_event(AiStreamEvent::Cancelled { request_id: req_id });
            return Err(ai_error(codes::AI_CANCELLED, "Sugestão cancelada."));
        }

        if !on_event(AiStreamEvent::Started {
            request_id: req_id,
            selection: selection.clone(),
            context: raw_ctx.clone(),
        }) {
            let _ = self.requests.cancel(req_id);
            self.clear_pane_suggest(req.pane_id, req_id);
            return Err(ai_error(
                codes::AI_CANCELLED,
                "Canal de streaming fechado pelo receptor.",
            ));
        }

        let sanitized_prompt: String = mt_db::sanitizer::sanitize(req.prompt.trim())
            .chars()
            .take(MAX_SUGGEST_PROMPT_CHARS)
            .collect();
        let messages = build_suggest_messages(&raw_ctx, &sanitized_prompt);
        let chat_req = ChatRequest {
            selection: selection.clone(),
            messages,
            json_mode: true,
            stream: true,
            max_tokens: Some(600),
        };

        let on_event_ref = &on_event;
        let mut channel_broken = false;
        let mut delta_cb = |delta_str: &str| {
            if !channel_broken && !guard.is_cancelled() {
                let ok = on_event_ref(AiStreamEvent::Delta {
                    request_id: req_id,
                    text: delta_str.to_string(),
                });
                if !ok {
                    channel_broken = true;
                    guard.token().cancel();
                }
            }
        };

        let chat_outcome = self
            .client
            .chat(&chat_req, Op::Suggest, guard.token(), &mut delta_cb)
            .await;

        self.clear_pane_suggest(req.pane_id, req_id);
        let latency_ms = u32::try_from(start.elapsed().as_millis()).unwrap_or(u32::MAX);

        // Verifica se a sessão ou PTY do painel mudaram durante a geração (CA-33)
        if let (Some(before), Some(after)) = (
            initial_snap.as_ref(),
            self.context.pane_snapshot(req.pane_id),
        ) && (before.session_id != after.session_id || before.pty_id != after.pty_id)
        {
            let _ = guard.try_finish(TerminalState::Cancelled);
            let _ = on_event(AiStreamEvent::Cancelled { request_id: req_id });
            return Err(ai_error(
                codes::AI_CONTEXT_STALE,
                "O painel ou a sessão mudou durante a geração da sugestão.",
            ));
        }

        if guard.is_cancelled() || channel_broken {
            let _ = guard.try_finish(TerminalState::Cancelled);
            let _ = on_event(AiStreamEvent::Cancelled { request_id: req_id });
            let _ = self
                .repo
                .insert_history(
                    history_gen,
                    settings.save_history,
                    AiHistoryItem {
                        id: req_id,
                        kind: AiHistoryKind::Suggest,
                        trigger: Some(req.trigger.as_str().to_string()),
                        project_id: initial_snap.as_ref().and_then(|s| s.project_id),
                        tab_id: initial_snap.as_ref().map(|s| s.tab_id),
                        pane_id: Some(req.pane_id),
                        session_id: initial_snap.as_ref().map(|s| s.session_id),
                        block_id: None,
                        harness_id: Some(selection.harness_id),
                        model_id: Some(selection.model_id),
                        reasoning_id: selection.reasoning_id,
                        rag_slug: None,
                        schema_id: None,
                        prompt_sanitized: sanitized_prompt,
                        response_sanitized: None,
                        risk_level: None,
                        status: "cancelled".to_string(),
                        error_code: Some(codes::AI_CANCELLED.0.to_string()),
                        latency_ms: Some(latency_ms),
                        created_at: now_iso8601(),
                    },
                )
                .await;
            return Err(ai_error(codes::AI_CANCELLED, "Sugestão cancelada."));
        }

        let chat_resp = match chat_outcome {
            Ok(r) => r,
            Err(err) => {
                if err.code == codes::AI_CANCELLED || guard.is_cancelled() {
                    let _ = guard.try_finish(TerminalState::Cancelled);
                    let _ = on_event(AiStreamEvent::Cancelled { request_id: req_id });
                } else if guard.try_finish(TerminalState::Failed) {
                    let _ = on_event(AiStreamEvent::Failed {
                        request_id: req_id,
                        error: err.clone(),
                    });
                } else {
                    let _ = on_event(AiStreamEvent::Cancelled { request_id: req_id });
                }
                let _ = self
                    .repo
                    .insert_history(
                        history_gen,
                        settings.save_history,
                        AiHistoryItem {
                            id: req_id,
                            kind: AiHistoryKind::Suggest,
                            trigger: Some(req.trigger.as_str().to_string()),
                            project_id: initial_snap.as_ref().and_then(|s| s.project_id),
                            tab_id: initial_snap.as_ref().map(|s| s.tab_id),
                            pane_id: Some(req.pane_id),
                            session_id: initial_snap.as_ref().map(|s| s.session_id),
                            block_id: None,
                            harness_id: Some(selection.harness_id),
                            model_id: Some(selection.model_id),
                            reasoning_id: selection.reasoning_id,
                            rag_slug: None,
                            schema_id: None,
                            prompt_sanitized: sanitized_prompt,
                            response_sanitized: None,
                            risk_level: None,
                            status: if err.code == codes::AI_CANCELLED {
                                "cancelled".to_string()
                            } else {
                                "failed".to_string()
                            },
                            error_code: Some(err.code.to_string()),
                            latency_ms: Some(latency_ms),
                            created_at: now_iso8601(),
                        },
                    )
                    .await;
                return Err(err);
            }
        };

        let raw_sug = match parse_suggestion(&chat_resp.content) {
            Ok(s) => s,
            Err(err) => {
                if guard.try_finish(TerminalState::Failed) {
                    let _ = on_event(AiStreamEvent::Failed {
                        request_id: req_id,
                        error: err.clone(),
                    });
                } else {
                    let _ = on_event(AiStreamEvent::Cancelled { request_id: req_id });
                }
                let _ = self
                    .repo
                    .insert_history(
                        history_gen,
                        settings.save_history,
                        AiHistoryItem {
                            id: req_id,
                            kind: AiHistoryKind::Suggest,
                            trigger: Some(req.trigger.as_str().to_string()),
                            project_id: initial_snap.as_ref().and_then(|s| s.project_id),
                            tab_id: initial_snap.as_ref().map(|s| s.tab_id),
                            pane_id: Some(req.pane_id),
                            session_id: initial_snap.as_ref().map(|s| s.session_id),
                            block_id: None,
                            harness_id: Some(selection.harness_id),
                            model_id: Some(selection.model_id),
                            reasoning_id: selection.reasoning_id,
                            rag_slug: None,
                            schema_id: None,
                            prompt_sanitized: sanitized_prompt,
                            response_sanitized: None,
                            risk_level: None,
                            status: "failed".to_string(),
                            error_code: Some(err.code.to_string()),
                            latency_ms: Some(latency_ms),
                            created_at: now_iso8601(),
                        },
                    )
                    .await;
                return Err(err);
            }
        };

        let local_risk = self.risk.classify(&raw_sug.command);
        let combined_risk = RiskClassifier::combine(local_risk, raw_sug.risk);

        let suggestion = CommandSuggestion {
            request_id: req_id,
            command: raw_sug.command.clone(),
            explanation: raw_sug.explanation,
            risk: combined_risk.clone(),
            selection: selection.clone(),
            context_summary: raw_ctx,
            latency_ms,
        };

        if guard.try_finish(TerminalState::Completed) {
            let _ = on_event(AiStreamEvent::Completed {
                request_id: req_id,
                suggestion: suggestion.clone(),
            });
            let _ = self
                .repo
                .insert_history(
                    history_gen,
                    settings.save_history,
                    AiHistoryItem {
                        id: req_id,
                        kind: AiHistoryKind::Suggest,
                        trigger: Some(req.trigger.as_str().to_string()),
                        project_id: initial_snap.as_ref().and_then(|s| s.project_id),
                        tab_id: initial_snap.as_ref().map(|s| s.tab_id),
                        pane_id: Some(req.pane_id),
                        session_id: initial_snap.as_ref().map(|s| s.session_id),
                        block_id: None,
                        harness_id: Some(selection.harness_id),
                        model_id: Some(selection.model_id),
                        reasoning_id: selection.reasoning_id,
                        rag_slug: None,
                        schema_id: None,
                        prompt_sanitized: sanitized_prompt,
                        response_sanitized: Some(raw_sug.command),
                        risk_level: Some(combined_risk.level.as_str().to_string()),
                        status: "completed".to_string(),
                        error_code: None,
                        latency_ms: Some(latency_ms),
                        created_at: now_iso8601(),
                    },
                )
                .await;
            Ok(suggestion)
        } else {
            let _ = on_event(AiStreamEvent::Cancelled { request_id: req_id });
            Err(ai_error(codes::AI_CANCELLED, "Sugestão cancelada."))
        }
    }

    /// Conveniência para consumidores em processo e testes: `prepare_suggest` + `run_suggest_stream`.
    pub async fn suggest_command(
        &self,
        req: SuggestRequest,
        on_event: impl Fn(AiStreamEvent) + Send + Sync + 'static,
    ) -> MtResult<CommandSuggestion> {
        let ticket = self.prepare_suggest(&req)?;
        self.run_suggest_stream(req, ticket, move |evt| {
            on_event(evt);
            true
        })
        .await
    }

    /// Cancela uma requisição ativa (ou registra tombstone se ainda não iniciou).
    pub fn cancel(&self, request_id: AiRequestId) -> bool {
        self.requests.cancel(request_id)
    }

    /// Executa o diagnóstico rápido inline para um bloco de comando com falha.
    pub async fn diagnose_block(&self, req: DiagnoseRequest) -> MtResult<DiagnosisOutcome> {
        self.diagnosis.diagnose(self, req).await
    }

    /// Dispensa o diagnóstico de um bloco específico.
    pub fn dismiss_diagnosis(&self, block_id: CommandBlockId) {
        self.diagnosis.dismiss_block(block_id);
    }

    /// Lista as bases de conhecimento RAG disponíveis.
    pub async fn rag_list_slugs(&self) -> MtResult<Vec<RagSlug>> {
        let settings = self.get_settings();
        self.rag.list(&self.client, &settings.rag).await
    }

    /// Executa busca RAG em `/v1/rags/{slug}/search` registrando no `AiRequestRegistry` e no histórico.
    pub async fn rag_search(&self, req: RagSearchRequest) -> MtResult<RagSearchResponse> {
        let settings = self.get_settings();
        let pane_snap = req.pane_id.and_then(|p| self.context.pane_snapshot(p));
        let guard = self.requests.begin(
            req.request_id,
            RequestMeta {
                pane_id: req.pane_id,
                session_id: pane_snap.as_ref().map(|s| s.session_id),
                pty_id: pane_snap.as_ref().map(|s| s.pty_id),
                config_revision: settings.revision,
            },
        )?;
        let history_gen = self.repo.current_history_generation();

        let res = self
            .rag
            .search(&self.client, &settings.rag, &req, guard.token())
            .await;

        match &res {
            Ok(resp) => {
                let _ = guard.try_finish(TerminalState::Completed);
                let _ = self
                    .repo
                    .insert_history(
                        history_gen,
                        settings.save_history,
                        AiHistoryItem {
                            id: req.request_id,
                            kind: AiHistoryKind::Rag,
                            trigger: None,
                            project_id: pane_snap.as_ref().and_then(|s| s.project_id),
                            tab_id: pane_snap.as_ref().map(|s| s.tab_id),
                            pane_id: req.pane_id,
                            session_id: pane_snap.as_ref().map(|s| s.session_id),
                            block_id: None,
                            harness_id: None,
                            model_id: None,
                            reasoning_id: None,
                            rag_slug: Some(resp.slug.clone()),
                            schema_id: None,
                            prompt_sanitized: resp.query.clone(),
                            response_sanitized: Some(format!(
                                "{} resultado(s)",
                                resp.results.len()
                            )),
                            risk_level: None,
                            status: "completed".to_string(),
                            error_code: None,
                            latency_ms: Some(resp.latency_ms),
                            created_at: now_iso8601(),
                        },
                    )
                    .await;
            }
            Err(err) => {
                let _ = guard.try_finish(TerminalState::Failed);
                let _ = self
                    .repo
                    .insert_history(
                        history_gen,
                        settings.save_history,
                        AiHistoryItem {
                            id: req.request_id,
                            kind: AiHistoryKind::Rag,
                            trigger: None,
                            project_id: pane_snap.as_ref().and_then(|s| s.project_id),
                            tab_id: pane_snap.as_ref().map(|s| s.tab_id),
                            pane_id: req.pane_id,
                            session_id: pane_snap.as_ref().map(|s| s.session_id),
                            block_id: None,
                            harness_id: None,
                            model_id: None,
                            reasoning_id: None,
                            rag_slug: Some(req.slug.clone()),
                            schema_id: None,
                            prompt_sanitized: req.query.clone(),
                            response_sanitized: None,
                            risk_level: None,
                            status: if err.code == codes::AI_CANCELLED {
                                "cancelled".to_string()
                            } else {
                                "failed".to_string()
                            },
                            error_code: Some(err.code.to_string()),
                            latency_ms: None,
                            created_at: now_iso8601(),
                        },
                    )
                    .await;
            }
        }

        res
    }

    /// Ponto de entrada público único para geração estruturada (consumido pelas Fases 14 e 15 via `ai_generate_structured`).
    pub async fn generate_structured(
        &self,
        req: StructuredRequest,
    ) -> MtResult<StructuredResponse> {
        let settings = self.get_settings();
        if !settings.enabled {
            return Err(ai_error(
                codes::AI_DISABLED,
                "A integração com o MaxAiManager está desativada nas preferências.",
            ));
        }

        let pane_snap = req.pane_id.and_then(|p| self.context.pane_snapshot(p));
        let guard = self.requests.begin(
            req.request_id,
            RequestMeta {
                pane_id: req.pane_id,
                session_id: pane_snap.as_ref().map(|s| s.session_id),
                pty_id: pane_snap.as_ref().map(|s| s.pty_id),
                config_revision: settings.revision,
            },
        )?;

        let res = self.structured.generate(self, req, guard.token()).await;
        match &res {
            Ok(_) => {
                let _ = guard.try_finish(TerminalState::Completed);
            }
            Err(err) => {
                if err.code == codes::AI_CANCELLED {
                    let _ = guard.try_finish(TerminalState::Cancelled);
                } else {
                    let _ = guard.try_finish(TerminalState::Failed);
                }
            }
        }
        res
    }

    /// Lista páginas do histórico de prompts sanitizados.
    pub async fn history_list(&self, filter: AiHistoryFilter) -> MtResult<AiHistoryPage> {
        self.repo.list_history(&filter).await
    }

    /// Limpa o histórico de prompts globalmente ou por projeto.
    pub async fn history_clear(&self, project_id: Option<ProjectId>) -> MtResult<u32> {
        self.repo.clear_history(project_id).await
    }

    /// Limpa caches de IA conforme o escopo informado.
    pub async fn cache_clear(&self, scope: CacheClearScope) -> MtResult<u32> {
        if matches!(scope, CacheClearScope::Catalog | CacheClearScope::All) {
            self.catalog.invalidate_memory();
        }
        self.repo.clear_cache(scope).await
    }

    fn clear_pane_suggest(&self, pane_id: PaneId, req_id: AiRequestId) {
        let mut map = self.active_suggest_by_pane.write();
        if map.get(&pane_id) == Some(&req_id) {
            map.remove(&pane_id);
        }
    }
}
