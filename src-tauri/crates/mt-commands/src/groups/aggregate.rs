//! Máquina de estados de itens (`ItemStatus`), redutor determinístico de `RunStatus`
//! e cálculo de contadores (`GroupRunCounts`).

use crate::groups::model::{GroupRunCounts, ItemStatus, RunStatus};

/// Calcula os contadores de estados a partir da lista ordenada de estados dos itens.
#[must_use]
pub fn compute_counts(statuses: &[ItemStatus]) -> GroupRunCounts {
    let mut counts = GroupRunCounts {
        total: u32::try_from(statuses.len()).unwrap_or(u32::MAX),
        ..GroupRunCounts::default()
    };

    for status in statuses {
        match status {
            ItemStatus::Pending => counts.pending += 1,
            ItemStatus::Starting => counts.starting += 1,
            ItemStatus::Running => counts.running += 1,
            ItemStatus::Cancelling => counts.cancelling += 1,
            ItemStatus::Succeeded => counts.succeeded += 1,
            ItemStatus::Failed => counts.failed += 1,
            ItemStatus::Skipped => counts.skipped += 1,
            ItemStatus::Cancelled => counts.cancelled += 1,
            ItemStatus::Interrupted => counts.interrupted += 1,
        }
    }
    counts
}

/// Calcula o `RunStatus` agregado determinístico de acordo com a tabela da spec §4.4:
/// 1. Se `cancel_requested` e houver recurso não terminal (`pending | starting | running | cancelling`) -> `Cancelling`
/// 2. Se houver algum item `starting | running | cancelling` ou (`has_started` e algum `pending`) -> `Running`
/// 3. Se todos estiverem `pending` e `!has_started` -> `Queued`
/// 4. Se todos forem terminais:
///    - Algum `Interrupted` -> `Interrupted`
///    - Algum `Cancelled` -> `Cancelled`
///    - Houver `Succeeded` (> 0) e `Failed` (> 0) -> `Partial`
///    - Houver `Failed` (> 0) e nenhum `Succeeded` -> `Failed`
///    - Todos `Succeeded` -> `Succeeded`
#[must_use]
pub fn reduce_run_status(
    statuses: &[ItemStatus],
    has_started: bool,
    cancel_requested: bool,
) -> RunStatus {
    let counts = compute_counts(statuses);
    let non_terminal = counts.pending + counts.starting + counts.running + counts.cancelling;

    if non_terminal > 0 {
        if cancel_requested || counts.cancelling > 0 {
            return RunStatus::Cancelling;
        }
        if counts.starting > 0
            || counts.running > 0
            || counts.succeeded > 0
            || counts.failed > 0
            || counts.skipped > 0
            || counts.cancelled > 0
            || counts.interrupted > 0
            || has_started
        {
            return RunStatus::Running;
        }
        return RunStatus::Queued;
    }

    if counts.interrupted > 0 {
        return RunStatus::Interrupted;
    }
    if counts.cancelled > 0 {
        return RunStatus::Cancelled;
    }
    if counts.succeeded > 0 && counts.failed > 0 {
        return RunStatus::Partial;
    }
    if counts.failed > 0 {
        return RunStatus::Failed;
    }
    RunStatus::Succeeded
}

/// Verifica se a transição de estado de um item é válida, garantindo que estados terminais
/// (`Succeeded`, `Failed`, `Skipped`, `Cancelled`, `Interrupted`) sejam imutáveis.
#[must_use]
pub const fn can_transition_item(from: ItemStatus, to: ItemStatus) -> bool {
    if from.is_terminal() {
        return false;
    }
    match (from, to) {
        (
            ItemStatus::Pending,
            ItemStatus::Starting
            | ItemStatus::Running
            | ItemStatus::Skipped
            | ItemStatus::Cancelled
            | ItemStatus::Interrupted
            | ItemStatus::Failed,
        )
        | (
            ItemStatus::Starting | ItemStatus::Running,
            ItemStatus::Running
            | ItemStatus::Cancelling
            | ItemStatus::Succeeded
            | ItemStatus::Failed
            | ItemStatus::Cancelled
            | ItemStatus::Interrupted,
        )
        | (
            ItemStatus::Cancelling,
            ItemStatus::Cancelled
            | ItemStatus::Succeeded
            | ItemStatus::Failed
            | ItemStatus::Interrupted,
        ) => true,
        _ => false,
    }
}
