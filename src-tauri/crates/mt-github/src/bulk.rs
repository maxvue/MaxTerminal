//! Orquestração da ação global em lote **"Corrigir todos os Bugs"** (Padrão `MaxCode`).

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use mt_core::ids::ProjectId;
use uuid::Uuid;

use crate::cache::GithubCacheRepo;
use crate::error::{GithubError, GithubErrorKind, GithubResult};
use crate::events::GithubBugCycleProgressEventPayload;
use crate::model::{
    BugCycleCandidateItem, BugCyclePreview, BugCycleRunState, BugCycleSkippedItem,
    GithubRepoIdentity, IssueFilter, IssueSortOrder, IssueStateFilter, IssueSummary,
    StartBugCycleRequest,
};
use crate::ports::{GithubEventSink, MaxCodeBridgePort, ProjectLookupPort};

/// Coordenador do ciclo automatizado de resolução de bugs em fila.
pub struct BugCycleOrchestrator {
    cache: GithubCacheRepo,
    projects: Arc<dyn ProjectLookupPort>,
    bridge: Arc<dyn MaxCodeBridgePort>,
    events: Arc<dyn GithubEventSink>,
}

impl BugCycleOrchestrator {
    /// Cria um novo `BugCycleOrchestrator`.
    #[must_use]
    pub fn new(
        cache: GithubCacheRepo,
        projects: Arc<dyn ProjectLookupPort>,
        bridge: Arc<dyn MaxCodeBridgePort>,
        events: Arc<dyn GithubEventSink>,
    ) -> Self {
        Self {
            cache,
            projects,
            bridge,
            events,
        }
    }

    /// Calcula a prévia determinística de candidatos elegíveis e itens ignorados antes de iniciar o ciclo.
    pub async fn preview(
        &self,
        project_id: ProjectId,
        repo: &GithubRepoIdentity,
        include_replan_and_rejected: bool,
    ) -> GithubResult<BugCyclePreview> {
        let page = self
            .cache
            .list_issues(
                &repo.repo_key,
                &IssueFilter {
                    state: IssueStateFilter::Open,
                    sort: IssueSortOrder::CreatedAsc,
                    limit: Some(200),
                    ..Default::default()
                },
                false,
            )
            .await?;

        let mut candidates = Vec::new();
        let mut skipped = Vec::new();

        for issue in &page.items {
            match classify_issue_for_bug_cycle(issue, include_replan_and_rejected) {
                CycleEligibility::Eligible {
                    priority_rank,
                    planned_stage,
                } => {
                    candidates.push((
                        priority_rank,
                        issue.number,
                        BugCycleCandidateItem {
                            issue_number: issue.number,
                            title: issue.title.clone(),
                            active_state_label: issue.active_state_label.clone(),
                            has_plan: issue.has_plan,
                            planned_stage: planned_stage.to_string(),
                        },
                    ));
                }
                CycleEligibility::Skipped { reason } => {
                    skipped.push(BugCycleSkippedItem {
                        issue_number: issue.number,
                        title: issue.title.clone(),
                        reason: reason.to_string(),
                    });
                }
            }
        }

        // Ordena por prioridade do estágio no workflow e depois por número crescente da issue
        candidates.sort_by_key(|(rank, num, _)| (*rank, *num));
        let ordered_candidates: Vec<BugCycleCandidateItem> =
            candidates.into_iter().map(|(_, _, c)| c).collect();

        let active_cycle = self
            .cache
            .get_latest_bug_cycle(&repo.repo_key)
            .await?
            .filter(|c| matches!(c.status.as_str(), "queued" | "running" | "paused"));

        Ok(BugCyclePreview {
            project_id,
            repo_key: repo.repo_key.clone(),
            candidates: ordered_candidates,
            skipped,
            active_cycle,
        })
    }

    /// Inicia um novo ciclo automatizado em lote após confirmação do usuário, garantindo no máximo 1 ciclo ativo por repositório.
    pub async fn start(
        &self,
        repo: &GithubRepoIdentity,
        req: &StartBugCycleRequest,
    ) -> GithubResult<BugCycleRunState> {
        let preview = self
            .preview(req.project_id, repo, req.include_replan_and_rejected)
            .await?;

        if preview.active_cycle.is_some() {
            return Err(GithubError::new(
                GithubErrorKind::Conflict,
                "Já existe um ciclo automatizado de correção de bugs ativo para este repositório.",
            ));
        }

        let selected_numbers: Vec<u64> = if req.issue_numbers.is_empty() {
            preview.candidates.iter().map(|c| c.issue_number).collect()
        } else {
            preview
                .candidates
                .iter()
                .filter(|c| req.issue_numbers.contains(&c.issue_number))
                .map(|c| c.issue_number)
                .collect()
        };

        if selected_numbers.is_empty() {
            return Err(GithubError::new(
                GithubErrorKind::ValidationFailed,
                "Nenhuma issue elegível selecionada para iniciar o ciclo automatizado de correção.",
            ));
        }

        let now_ms = current_epoch_ms();
        let first_issue = selected_numbers.first().copied();
        let first_stage = preview
            .candidates
            .iter()
            .find(|c| Some(c.issue_number) == first_issue)
            .map_or_else(|| String::from("plan"), |c| c.planned_stage.clone());

        let mut run_state = BugCycleRunState {
            cycle_id: Uuid::now_v7().to_string(),
            project_id: req.project_id,
            repo_key: repo.repo_key.clone(),
            status: String::from("running"),
            total_issues: u32::try_from(selected_numbers.len()).unwrap_or(0),
            completed_issues: 0,
            failed_issues: 0,
            current_issue: first_issue,
            current_stage: Some(first_stage),
            issue_numbers: selected_numbers,
            skipped: preview.skipped,
            error_message: None,
            started_at_ms: now_ms,
            updated_at_ms: now_ms,
            finished_at_ms: None,
        };

        self.cache.insert_bug_cycle(&run_state).await?;

        let project_path = self.projects.project_root_path(req.project_id).await?;
        if let Err(err) = self
            .bridge
            .start_bugs_cycle(&run_state, &project_path.to_string_lossy())
            .await
        {
            run_state.status = String::from("failed");
            run_state.error_message = Some(err.message.clone());
            run_state.updated_at_ms = current_epoch_ms();
            run_state.finished_at_ms = Some(run_state.updated_at_ms);
            let _ = self.cache.update_bug_cycle(&run_state).await;
            return Err(err);
        }

        self.events
            .emit_bug_cycle_progress(GithubBugCycleProgressEventPayload {
                cycle: run_state.clone(),
            });

        Ok(run_state)
    }

    /// Retorna o estado atual ou último ciclo conhecido para o repositório.
    pub async fn status(&self, repo_key: &str) -> GithubResult<Option<BugCycleRunState>> {
        self.cache.get_latest_bug_cycle(repo_key).await
    }

    /// Aplica atualização de progresso ou cancelamento/pausa em um ciclo existente.
    pub async fn update_cycle_status(
        &self,
        repo_key: &str,
        new_status: &str,
        completed_issues: Option<u32>,
        failed_issues: Option<u32>,
        current_issue: Option<u64>,
        current_stage: Option<String>,
        error_message: Option<String>,
    ) -> GithubResult<BugCycleRunState> {
        let mut current = self
            .cache
            .get_latest_bug_cycle(repo_key)
            .await?
            .ok_or_else(|| {
                GithubError::new(
                    GithubErrorKind::NotFound,
                    "Nenhum ciclo de correção de bugs encontrado para este repositório.",
                )
            })?;

        let now_ms = current_epoch_ms();
        current.status = new_status.to_string();
        if let Some(c) = completed_issues {
            current.completed_issues = c;
        }
        if let Some(f) = failed_issues {
            current.failed_issues = f;
        }
        current.current_issue = current_issue;
        current.current_stage = current_stage;
        if let Some(msg) = error_message {
            current.error_message = Some(mt_db::sanitizer::sanitize(&msg).into_owned());
        }
        current.updated_at_ms = now_ms;
        if matches!(new_status, "completed" | "cancelled" | "failed") {
            current.finished_at_ms = Some(now_ms);
        }

        self.cache.update_bug_cycle(&current).await?;
        self.events
            .emit_bug_cycle_progress(GithubBugCycleProgressEventPayload {
                cycle: current.clone(),
            });
        Ok(current)
    }
}

enum CycleEligibility {
    Eligible {
        priority_rank: u8,
        planned_stage: &'static str,
    },
    Skipped {
        reason: &'static str,
    },
}

fn classify_issue_for_bug_cycle(
    issue: &IssueSummary,
    include_replan_and_rejected: bool,
) -> CycleEligibility {
    if let Some(ref closure) = issue.active_closure_label {
        return match closure.as_str() {
            "Duplicated" => CycleEligibility::Skipped {
                reason: "Marcada como duplicada (Duplicated).",
            },
            "Evidence not Found" => CycleEligibility::Skipped {
                reason: "Sem evidências para reprodução (Evidence not Found).",
            },
            "Dismissed" => CycleEligibility::Skipped {
                reason: "Descartada (Dismissed).",
            },
            _ => CycleEligibility::Skipped {
                reason: "Possui label de encerramento.",
            },
        };
    }

    match issue.active_state_label.as_deref() {
        Some("Waiting Execute") => CycleEligibility::Eligible {
            priority_rank: 1,
            planned_stage: "execute",
        },
        Some("Planned") => CycleEligibility::Eligible {
            priority_rank: 2,
            planned_stage: "execute",
        },
        Some("Executed") | Some("Waiting Check") => CycleEligibility::Eligible {
            priority_rank: 3,
            planned_stage: "check",
        },
        Some("Waiting Plan") | None => CycleEligibility::Eligible {
            priority_rank: 4,
            planned_stage: if issue.has_plan { "execute" } else { "plan" },
        },
        Some("Waiting Re-plan") | Some("Checked Rejected") | Some("User Rejected") => {
            if include_replan_and_rejected {
                CycleEligibility::Eligible {
                    priority_rank: 5,
                    planned_stage: "plan",
                }
            } else {
                CycleEligibility::Skipped {
                    reason: "Aguardando replanejamento/revisão (excluída pelo filtro atual).",
                }
            }
        }
        Some("Waiting More Info") => CycleEligibility::Skipped {
            reason: "Aguardando mais informações do autor (Waiting More Info).",
        },
        Some("Checked Approved")
        | Some("Fix Merged")
        | Some("Waiting Deploy")
        | Some("Deployed")
        | Some("Waiting user test")
        | Some("User Approved") => CycleEligibility::Skipped {
            reason: "Correção já aprovada, integrada ou em fase de homologação/deploy.",
        },
        Some(_) => CycleEligibility::Eligible {
            priority_rank: 6,
            planned_stage: "plan",
        },
    }
}

fn current_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0))
}
