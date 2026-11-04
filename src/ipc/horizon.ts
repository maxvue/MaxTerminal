import { unwrap } from '@/ipc/core';
import {
    commands,
    type HorizonClearPreview,
    type HorizonClearResult_Serialize,
    type HorizonSnapshot_Serialize,
    type ProjectId
} from '@/types/bindings';

/** Obtém o snapshot das métricas e status do Laravel Horizon para o projeto indicado (`horizon_snapshot`). */
export function getHorizonSnapshot(projectId: ProjectId): Promise<HorizonSnapshot_Serialize> {
    return unwrap(commands.horizonSnapshot(projectId), 'horizon_snapshot');
}

/** Inicia o daemon do Laravel Horizon em um PTY supervisionado e exclusivo (`horizon_start`). */
export function startHorizon(
    projectId: ProjectId,
    expectedContextRevision: string,
    requestId: string
): Promise<HorizonSnapshot_Serialize> {
    return unwrap(
        commands.horizonStart(projectId, expectedContextRevision, requestId),
        'horizon_start'
    );
}

/** Encerra graciosamente o processo master do Horizon supervisionado pelo MaxTerminal (`horizon_stop`). */
export function stopHorizon(
    projectId: ProjectId,
    expectedGeneration: number,
    requestId: string
): Promise<HorizonSnapshot_Serialize> {
    return unwrap(
        commands.horizonStop(projectId, expectedGeneration, requestId),
        'horizon_stop'
    );
}

/** Solicita a prévia e o token temporário de confirmação para limpeza de tarefas pendentes (`horizon_clear_pending_preview`). */
export function previewClearPending(projectId: ProjectId): Promise<HorizonClearPreview> {
    return unwrap(commands.horizonClearPendingPreview(projectId), 'horizon_clear_pending_preview');
}

/** Executa a limpeza confirmada de tarefas pendentes (ready + delayed), preservando tarefas reservadas (`horizon_clear_pending`). */
export function clearPending(
    projectId: ProjectId,
    token: string,
    requestId: string
): Promise<HorizonClearResult_Serialize> {
    return unwrap(
        commands.horizonClearPending(projectId, token, requestId),
        'horizon_clear_pending'
    );
}
