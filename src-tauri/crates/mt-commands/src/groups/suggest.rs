//! Sugestão assistida de Grupos de Comandos via `MaxAiManager` (Fase 12) e consulta opcional
//! à memória contextual de comandos recorrentes (Fase 13).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use mt_core::{MtResult, PaneId, ProjectId};
use mt_db::sanitizer::sanitize;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::groups::model::{
    CommandGroupInput, CommandGroupItemInput, GroupErrorCode, GroupScope, ShellDialect,
};
use crate::groups::ports::{
    GroupAiPort, GroupShellPort, GroupTabPort, RecurringCommandEntry, RecurringCommandsSource,
    SuggestPromptContext,
};
use crate::groups::resolve::detect_shell_dialect;
use crate::groups::validate::validate_group_input;

/// Identificador canônico do JSON Schema fechado registrado na Fase 12 para sugestão de grupos.
pub const AI_SUGGEST_SCHEMA_ID: &str = "command-group.suggestion.v1";

/// Limite máximo de caracteres no campo `goal` de `GroupSuggestRequest`.
pub const MAX_SUGGEST_GOAL_CHARS: usize = 2000;

/// Requisição IPC para `command_group_suggest`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupSuggestRequest {
    /// Identificador `AiRequestId` (UUID v7) usado para correlacionar e cancelar via `ai_cancel`.
    pub request_id: String,
    /// Projeto alvo opcional (obrigatório quando `scope == Local`).
    pub project_id: Option<ProjectId>,
    /// Painel de contexto opcional.
    pub pane_id: Option<PaneId>,
    /// Escopo fixado pelo editor (`local` ou `global`).
    pub scope: GroupScope,
    /// Descrição em linguagem natural do objetivo do grupo (1–2000 caracteres).
    pub goal: String,
    /// Rascunho atual do modal (quando houver).
    pub draft: Option<CommandGroupInput>,
    /// Revisão local do rascunho no editor para proteção contra respostas obsoletas.
    pub draft_revision: u64,
}

/// Resposta IPC de `command_group_suggest`: rascunho validado e nunca persistido/executado automaticamente.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupSuggestResponse {
    /// Identificador da requisição IA original.
    pub request_id: String,
    /// Revisão do rascunho correspondente no editor.
    pub draft_revision: u64,
    /// Rascunho validado pronto para revisão humana no modal.
    pub suggestion: CommandGroupInput,
}

/// Resposta IPC de `command_group_recurring_commands`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecurringCommandsResponse {
    /// Indica se a fonte de memória contextual da Fase 13 está ativa.
    pub available: bool,
    /// Lista de comandos recorrentes sanitizados do projeto.
    pub items: Vec<RecurringCommandEntry>,
}

/// Orquestrador de sugestões assistidas por IA e memória contextual de comandos recorrentes.
pub struct GroupSuggester {
    ai_port: Arc<dyn GroupAiPort>,
    tab_port: Arc<dyn GroupTabPort>,
    shell_port: Arc<dyn GroupShellPort>,
    recurring_source: Option<Arc<dyn RecurringCommandsSource>>,
}

impl GroupSuggester {
    /// Cria um novo `GroupSuggester`.
    #[must_use]
    pub fn new(
        ai_port: Arc<dyn GroupAiPort>,
        tab_port: Arc<dyn GroupTabPort>,
        shell_port: Arc<dyn GroupShellPort>,
        recurring_source: Option<Arc<dyn RecurringCommandsSource>>,
    ) -> Self {
        Self {
            ai_port,
            tab_port,
            shell_port,
            recurring_source,
        }
    }

    /// Gera um rascunho validado via Fase 12:
    /// 1. Valida `goal` (1..=2000 caracteres após trim) e sanitiza `goal` e `draft` com `mt_db::sanitizer::sanitize`.
    /// 2. Força `scope` e `project_id` a partir da requisição do editor.
    /// 3. Submete ao `GroupAiPort::generate_group_suggestion`.
    /// 4. Passa o `CommandGroupInput` retornado por `validate_group_input(&suggestion, project_root.as_deref())`,
    ///    garantindo que limites de 1–50 itens, `cwd` relativo confinado, chaves POSIX de `env` e ausência de segredos sejam respeitados.
    pub async fn suggest(&self, req: GroupSuggestRequest) -> MtResult<GroupSuggestResponse> {
        if req.request_id.trim().is_empty() {
            return Err(GroupErrorCode::Validation.into_error_with_fields(
                "O identificador request_id da sugestão não pode ser vazio.",
                vec!["requestId".to_string()],
            ));
        }

        match req.scope {
            GroupScope::Local => {
                if req.project_id.is_none() {
                    return Err(GroupErrorCode::ProjectRequired.into_error_with_fields(
                        "Sugestões para grupos locais exigem um projeto válido.",
                        vec!["projectId".to_string()],
                    ));
                }
            }
            GroupScope::Global => {
                if req.project_id.is_some() {
                    return Err(GroupErrorCode::ScopeMismatch.into_error_with_fields(
                        "Sugestões para grupos globais não podem fixar project_id no rascunho.",
                        vec!["projectId".to_string()],
                    ));
                }
            }
        }

        let trimmed_goal = req.goal.trim();
        let goal_chars = trimmed_goal.chars().count();
        if goal_chars == 0 || goal_chars > MAX_SUGGEST_GOAL_CHARS {
            return Err(GroupErrorCode::Validation.into_error_with_fields(
                format!(
                    "O objetivo da sugestão deve ter entre 1 e {MAX_SUGGEST_GOAL_CHARS} caracteres."
                ),
                vec!["goal".to_string()],
            ));
        }

        let sanitized_goal = sanitize(trimmed_goal).into_owned();
        let sanitized_draft = req.draft.as_ref().map(sanitize_draft_input);

        let project_root: Option<PathBuf> = if let Some(ref pid) = req.project_id {
            Some(self.tab_port.resolve_project_root(pid).await?)
        } else {
            None
        };

        let mut shell_dialect = ShellDialect::Bash;
        if let Some(ref pane_id) = req.pane_id {
            if let Ok(state) = self.shell_port.inspect_pane(pane_id).await {
                if let Ok(d) = detect_shell_dialect(&state.shell_path) {
                    shell_dialect = d;
                }
            }
        } else if let Some(ref pid) = req.project_id {
            if let Ok(templates) = self.tab_port.list_project_templates(pid).await {
                if let Some(tpl) = templates
                    .iter()
                    .find(|t| t.is_default && t.is_shell_environment)
                    .or_else(|| templates.iter().find(|t| t.is_shell_environment))
                {
                    if let Ok(d) = detect_shell_dialect(&tpl.shell_path) {
                        shell_dialect = d;
                    }
                }
            }
        }

        let recurring_commands = if let Some(ref pid) = req.project_id {
            match self.list_recurring(pid, 15).await {
                Ok(resp) if resp.available => resp
                    .items
                    .into_iter()
                    .map(|e| sanitize(&e.command).into_owned())
                    .collect(),
                _ => Vec::new(),
            }
        } else {
            Vec::new()
        };

        let project_name = project_root
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map(|s| sanitize(s).into_owned());

        let context = SuggestPromptContext {
            project_id: req.project_id.clone(),
            pane_id: req.pane_id.clone(),
            scope: req.scope,
            project_name,
            project_stacks: Vec::new(),
            shell_dialect,
            goal: sanitized_goal,
            existing_draft: sanitized_draft,
            recurring_commands,
        };

        let mut raw_suggestion = self
            .ai_port
            .generate_group_suggestion(&req.request_id, context)
            .await?;

        // Força o escopo e project_id originais do editor
        raw_suggestion.scope = req.scope;
        raw_suggestion.project_id = req.project_id.clone();

        let validated = validate_group_input(&raw_suggestion, project_root.as_deref())?;
        let normalized_suggestion = CommandGroupInput {
            scope: validated.scope,
            project_id: validated.project_id,
            name: validated.name,
            description: validated.description,
            labels: validated.labels.into_iter().map(|(_, lbl)| lbl).collect(),
            mode: validated.mode,
            separator: validated.separator,
            cwd: validated.cwd,
            env: validated.env,
            items: validated
                .items
                .into_iter()
                .map(|i| CommandGroupItemInput {
                    id: i.id,
                    title: i.title,
                    command: i.command,
                    cwd: i.cwd,
                    env: i.env,
                    continuous: i.continuous,
                })
                .collect(),
        };

        Ok(GroupSuggestResponse {
            request_id: req.request_id,
            draft_revision: req.draft_revision,
            suggestion: normalized_suggestion,
        })
    }

    /// Consulta os comandos recorrentes da Fase 13 se a fonte opcional estiver registrada e disponível;
    /// caso contrário retorna `{ available: false, items: vec![] }` sem erro.
    pub async fn list_recurring(
        &self,
        project_id: &ProjectId,
        limit: u32,
    ) -> MtResult<RecurringCommandsResponse> {
        if limit == 0 || limit > 50 {
            return Err(GroupErrorCode::Validation.into_error_with_fields(
                "O limite de comandos recorrentes deve estar entre 1 e 50.",
                vec!["limit".to_string()],
            ));
        }

        let Some(ref source) = self.recurring_source else {
            return Ok(RecurringCommandsResponse {
                available: false,
                items: Vec::new(),
            });
        };
        if !source.is_available() {
            return Ok(RecurringCommandsResponse {
                available: false,
                items: Vec::new(),
            });
        }

        let raw_items = source.list_recurring(project_id, limit).await?;
        let sanitized_items = raw_items
            .into_iter()
            .filter_map(|entry| {
                let clean_cmd = sanitize(&entry.command).into_owned();
                if clean_cmd.trim().is_empty() {
                    return None;
                }
                let clean_cwd = entry.cwd.map(|c| sanitize(&c).into_owned());
                Some(RecurringCommandEntry {
                    command: clean_cmd,
                    cwd: clean_cwd,
                    frequency: entry.frequency,
                    last_used_at: entry.last_used_at,
                })
            })
            .collect();

        Ok(RecurringCommandsResponse {
            available: true,
            items: sanitized_items,
        })
    }
}

fn sanitize_draft_input(draft: &CommandGroupInput) -> CommandGroupInput {
    let clean_env: BTreeMap<String, String> = draft
        .env
        .iter()
        .map(|(k, v)| (sanitize(k).into_owned(), sanitize(v).into_owned()))
        .collect();
    let clean_items = draft
        .items
        .iter()
        .map(|item| CommandGroupItemInput {
            id: item.id.clone(),
            title: sanitize(&item.title).into_owned(),
            command: sanitize(&item.command).into_owned(),
            cwd: item.cwd.as_ref().map(|c| sanitize(c).into_owned()),
            env: item
                .env
                .iter()
                .map(|(k, v)| (sanitize(k).into_owned(), sanitize(v).into_owned()))
                .collect(),
            continuous: item.continuous,
        })
        .collect();

    CommandGroupInput {
        scope: draft.scope,
        project_id: draft.project_id.clone(),
        name: sanitize(&draft.name).into_owned(),
        description: sanitize(&draft.description).into_owned(),
        labels: draft
            .labels
            .iter()
            .map(|l| sanitize(l).into_owned())
            .collect(),
        mode: draft.mode,
        separator: draft.separator,
        cwd: draft.cwd.as_ref().map(|c| sanitize(c).into_owned()),
        env: clean_env,
        items: clean_items,
    }
}
