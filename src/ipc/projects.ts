import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from '@/ipc/core';
import {
    commands,
    type DiscoveredFolder,
    type NewProject_Deserialize,
    type NewProjectRoot,
    type Project_Serialize,
    type ProjectChangedPayload_Serialize,
    type ProjectDiscoveredPayload_Serialize,
    type ProjectEvent,
    type ProjectId,
    type ProjectIdentity,
    type ProjectPatch_Deserialize,
    type ProjectRemovedPayload_Serialize,
    type ProjectRoot,
    type ProjectRootChangedPayload,
    type ProjectRootId,
    type ProjectRootPatch_Deserialize,
    type ProjectSnapshot_Serialize,
    type RootImportReport_Serialize
} from '@/types/bindings';

/** Retorna uma visão consistente e versionada de todo o catálogo (`project_snapshot`). */
export function projectSnapshot(): Promise<ProjectSnapshot_Serialize> {
    return unwrap(commands.projectSnapshot(), 'project_snapshot');
}

/** Lista todos os projetos cadastrados ordenados por `sort_order` (`project_list`). */
export function projectList(): Promise<Project_Serialize[]> {
    return unwrap(commands.projectList(), 'project_list');
}

/** Cadastra um novo projeto individual (`project_add`). */
export function projectAdd(input: NewProject_Deserialize): Promise<Project_Serialize> {
    return unwrap(commands.projectAdd(input), 'project_add');
}

/** Atualiza metadados de um projeto existente (`project_update`). */
export function projectUpdate(
    id: ProjectId,
    patch: ProjectPatch_Deserialize
): Promise<Project_Serialize> {
    return unwrap(commands.projectUpdate(id, patch), 'project_update');
}

/** Remove um projeto do cadastro após executar os hooks de remoção (`project_remove`). */
export function projectRemove(id: ProjectId): Promise<null> {
    return unwrap(commands.projectRemove(id), 'project_remove');
}

/** Reordena a lista completa de projetos (`project_reorder`). */
export function projectReorder(ids: ProjectId[]): Promise<null> {
    return unwrap(commands.projectReorder(ids), 'project_reorder');
}

/** Re-detecta a stack tecnológica de um projeto cadastrado (`project_redetect`). */
export function projectRedetect(id: ProjectId): Promise<Project_Serialize> {
    return unwrap(commands.projectRedetect(id), 'project_redetect');
}

/** Calcula a identidade visual e stack de um caminho sem persistir no banco (`project_preview_identity`). */
export function projectPreviewIdentity(path: string): Promise<ProjectIdentity> {
    return unwrap(commands.projectPreviewIdentity(path), 'project_preview_identity');
}

/** Verifica a presença das pastas de todos os projetos no disco e retorna apenas os alterados (`project_refresh_missing`). */
export function projectRefreshMissing(): Promise<Project_Serialize[]> {
    return unwrap(commands.projectRefreshMissing(), 'project_refresh_missing');
}

/** Lista todas as Pastas de Projetos cadastradas (`project_root_list`). */
export function projectRootList(): Promise<ProjectRoot[]> {
    return unwrap(commands.projectRootList(), 'project_root_list');
}

/** Cadastra uma nova Pasta de Projetos e executa sua varredura inicial (`project_root_add`). */
export function projectRootAdd(input: NewProjectRoot): Promise<RootImportReport_Serialize> {
    return unwrap(commands.projectRootAdd(input), 'project_root_add');
}

/** Atualiza configurações de uma Pasta de Projetos (`project_root_update`). */
export function projectRootUpdate(
    id: ProjectRootId,
    patch: ProjectRootPatch_Deserialize
): Promise<ProjectRoot> {
    return unwrap(commands.projectRootUpdate(id, patch), 'project_root_update');
}

/** Remove uma Pasta de Projetos preservando os projetos já importados (`project_root_remove`). */
export function projectRootRemove(id: ProjectRootId): Promise<null> {
    return unwrap(commands.projectRootRemove(id), 'project_root_remove');
}

/** Executa uma nova varredura manual em uma Pasta de Projetos (`project_root_rescan`). */
export function projectRootRescan(id: ProjectRootId): Promise<RootImportReport_Serialize> {
    return unwrap(commands.projectRootRescan(id), 'project_root_rescan');
}

/** Lista todas as subpastas descobertas aguardando aceite ou dispensa (`project_discovery_list`). */
export function projectDiscoveryList(): Promise<DiscoveredFolder[]> {
    return unwrap(commands.projectDiscoveryList(), 'project_discovery_list');
}

/** Aceita um lote de subpastas descobertas convertendo-as em projetos (`project_discovery_accept`). */
export function projectDiscoveryAccept(paths: string[]): Promise<Project_Serialize[]> {
    return unwrap(commands.projectDiscoveryAccept(paths), 'project_discovery_accept');
}

/** Dispensa um lote de subpastas descobertas (`project_discovery_dismiss`). */
export function projectDiscoveryDismiss(paths: string[]): Promise<null> {
    return unwrap(commands.projectDiscoveryDismiss(paths), 'project_discovery_dismiss');
}

/** Escuta o evento global `project://discovered`. */
export function onProjectDiscovered(
    cb: (p: ProjectEvent<ProjectDiscoveredPayload_Serialize>) => void
): Promise<UnlistenFn> {
    return listen<ProjectEvent<ProjectDiscoveredPayload_Serialize>>('project://discovered', (ev) => {
        cb(ev.payload);
    });
}

/** Escuta o evento global `project://removed`. */
export function onProjectRemoved(
    cb: (p: ProjectEvent<ProjectRemovedPayload_Serialize>) => void
): Promise<UnlistenFn> {
    return listen<ProjectEvent<ProjectRemovedPayload_Serialize>>('project://removed', (ev) => {
        cb(ev.payload);
    });
}

/** Escuta o evento global `project://changed`. */
export function onProjectChanged(
    cb: (p: ProjectEvent<ProjectChangedPayload_Serialize>) => void
): Promise<UnlistenFn> {
    return listen<ProjectEvent<ProjectChangedPayload_Serialize>>('project://changed', (ev) => {
        cb(ev.payload);
    });
}

/** Escuta o evento global `project://root-changed`. */
export function onProjectRootChanged(
    cb: (p: ProjectEvent<ProjectRootChangedPayload>) => void
): Promise<UnlistenFn> {
    return listen<ProjectEvent<ProjectRootChangedPayload>>('project://root-changed', (ev) => {
        cb(ev.payload);
    });
}

/**
 * Abre o seletor nativo de diretório do sistema operacional.
 * Retorna `null` se o usuário cancelar ou se o plugin de diálogo não estiver disponível.
 */
export async function pickFolder(title: string): Promise<string | null> {
    try {
        const result = await invoke<string | string[] | null>('plugin:dialog|open', {
            options: {
                title,
                directory: true,
                multiple: false
            }
        });
        if (typeof result === 'string' && result.trim().length > 0) {
            return result;
        }
        if (Array.isArray(result) && typeof result[0] === 'string' && result[0].trim().length > 0) {
            return result[0];
        }
        return null;
    } catch {
        return null;
    }
}
