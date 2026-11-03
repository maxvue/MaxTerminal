import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { call } from '@/ipc/core';
import {
    commands,
    type ConflictResolution,
    type ProjectId,
    type SkillCatalog,
    type SkillsChangedPayload,
    type SkillSyncTarget_Deserialize,
    type SkillSyncTarget_Serialize,
    type SyncPlan,
    type SyncResult
} from '@/types/bindings';

export type { SkillsChangedPayload };

export const skillsIpc = {
    /** Escaneia o repositório local `MaxSkills` e calcula relevância para o projeto opcional (`skills_scan`). */
    scan: (projectId: ProjectId | null = null): Promise<SkillCatalog> =>
        call('skills_scan', () => commands.skillsScan(projectId)),

    /** Lista os alvos de sincronização de skills cadastrados (`skills_targets_list`). */
    targetsList: (projectId: ProjectId | null = null): Promise<SkillSyncTarget_Serialize[]> =>
        call('skills_targets_list', () => commands.skillsTargetsList(projectId)),

    /** Cria ou atualiza um alvo de sincronização de skills validando o diretório de destino (`skills_target_upsert`). */
    targetUpsert: (target: SkillSyncTarget_Deserialize): Promise<SkillSyncTarget_Serialize> =>
        call('skills_target_upsert', () => commands.skillsTargetUpsert(target)),

    /** Remove um alvo de sincronização e opcionalmente limpa apenas as entradas gerenciadas intactas (`skills_target_remove`). */
    targetRemove: (targetId: number, removeManaged: boolean): Promise<number> =>
        call('skills_target_remove', () => commands.skillsTargetRemove(targetId, removeManaged)),

    /** Calcula um plano dry-run de sincronização sem modificar o disco (`skills_plan`). */
    plan: (targetId: number): Promise<SyncPlan> =>
        call('skills_plan', () => commands.skillsPlan(targetId)),

    /** Aplica um plano de sincronização revalidando o fingerprint do disco (`skills_apply`). */
    apply: (
        planId: string,
        resolutions: Partial<Record<string, ConflictResolution>>
    ): Promise<SyncResult> => {
        const cleanResolutions: Record<string, ConflictResolution> = {};
        for (const [k, v] of Object.entries(resolutions)) {
            if (v !== undefined) {
                cleanResolutions[k] = v;
            }
        }
        return call('skills_apply', () => commands.skillsApply(planId, cleanResolutions));
    },

    /** Escuta o evento global `skills://changed`. */
    onChanged: (cb: (payload: SkillsChangedPayload) => void): Promise<UnlistenFn> =>
        listen<SkillsChangedPayload>('skills://changed', (ev) => cb(ev.payload)),

    /** Escuta o evento global `skills://sync-completed`. */
    onSyncCompleted: (cb: (result: SyncResult) => void): Promise<UnlistenFn> =>
        listen<SyncResult>('skills://sync-completed', (ev) => cb(ev.payload))
};
