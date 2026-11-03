//! Reconciliação de boot e restauração de abas do workspace (`workspace/restore.rs`).
//!
//! Implementa o contrato §3.8 da Fase 07:
//! 1. Geração de `boot_id` `UUIDv4` único;
//! 2. Reconciliação atômica de sessões órfãs (`status = 'active'` -> `'archived'`, `end_reason = 'crash_recovered'`)
//!    e operações interrompidas em `workspace_operations` de boots anteriores;
//! 3. Abertura automática das abas marcadas com `add_to_workspace = true` (reabrindo abas existentes
//!    preservando `TabId` e histórico de `tab_sessions` ou criando novas abas iniciais).

use mt_core::workspace::{TabOpenRequest, WorkspaceRestoreFailure, WorkspaceRestoreReport};
use mt_db::Db;
use mt_db::repos::{
    enforce_tab_sessions_retention_tx, mark_previous_boots_interrupted_tx,
    recover_crash_sessions_tx,
};

use crate::workspace::errors::map_db_error;
use crate::workspace::manager::WorkspaceManager;

/// Executa a reconciliação transacional de sessões e operações órfãs deixadas por encerramento abrupto,
/// seguida da restauração das abas de templates com `add_to_workspace = true`.
pub async fn run_boot_reconciliation_and_restore(
    db: &Db,
    manager: &WorkspaceManager,
    boot_id: &str,
) -> Result<WorkspaceRestoreReport, mt_core::error::MtError> {
    let now_ms = mt_db::time::now_unix_ms();
    let boot_id_owned = boot_id.to_string();

    let (keep_per_tab, max_age_days) = read_retention_settings(db);

    let (recovered_sessions, interrupted_operations, unlinked_legacy_sessions) = db
        .write_sized("workspace_boot_reconcile", 2048, move |tx| {
            let interrupted = mark_previous_boots_interrupted_tx(tx, &boot_id_owned, now_ms)?;
            let recovered = recover_crash_sessions_tx(tx, now_ms)?;

            let unlinked: i64 = tx
                .query_row(
                    "SELECT COUNT(1) FROM sessions WHERE tab_session_id IS NULL;",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0);

            let _ = enforce_tab_sessions_retention_tx(tx, keep_per_tab, max_age_days, now_ms)?;

            Ok((
                usize::try_from(recovered).unwrap_or(usize::MAX),
                usize::try_from(interrupted).unwrap_or(usize::MAX),
                usize::try_from(unlinked.max(0)).unwrap_or(0),
            ))
        })
        .await
        .map_err(map_db_error)?;

    // Garante que todos os projetos existentes possuam ao menos 1 template padrão
    let _ = db
        .templates()
        .ensure_all_projects_have_default(now_ms)
        .await;

    // Lista todos os templates marcados com add_to_workspace = true em projetos ativos
    let workspace_templates = db
        .templates()
        .list_workspace_templates()
        .await
        .map_err(map_db_error)?;

    let mut opened_tabs = 0usize;
    let mut failures = Vec::new();

    for template in workspace_templates {
        let req = TabOpenRequest {
            request_id: format!("boot-{}-{}", boot_id, template.id),
            project_id: template.project_id,
            template_id: Some(template.id),
            overrides: None,
            focus: false,
        };

        match manager.open_workspace_restore_tab(req).await {
            Ok(_tab_view) => {
                opened_tabs = opened_tabs.saturating_add(1);
            }
            Err(err) => {
                failures.push(WorkspaceRestoreFailure {
                    project_id: template.project_id,
                    template_id: Some(template.id),
                    tab_id: None,
                    code: err.code.0.to_string(),
                    message: err.message.into_owned(),
                });
            }
        }
    }

    Ok(WorkspaceRestoreReport {
        boot_id: boot_id.to_string(),
        opened_tabs,
        recovered_sessions,
        interrupted_operations,
        unlinked_legacy_sessions,
        failures,
    })
}

/// Lê as políticas de retenção de sessões configuradas em `settings` (com fallback para 50 sessões / 90 dias).
#[must_use]
pub fn read_retention_settings(db: &Db) -> (u32, u32) {
    let mut keep_per_tab = 50u32;
    let mut max_age_days = 90u32;
    if let Ok(settings) = db.settings() {
        if let Ok(snap) = settings.get_snapshot("workspace.sessions.keep_per_tab")
            && let Some(n) = snap.value.as_u64()
            && (1..=1000).contains(&n)
        {
            keep_per_tab = u32::try_from(n).unwrap_or(50);
        }
        if let Ok(snap) = settings.get_snapshot("workspace.sessions.max_age_days")
            && let Some(n) = snap.value.as_u64()
            && (1..=3650).contains(&n)
        {
            max_age_days = u32::try_from(n).unwrap_or(90);
        }
    }
    (keep_per_tab, max_age_days)
}
