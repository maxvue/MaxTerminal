import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from '@/ipc/core';
import {
    commands,
    type ProjectId,
    type TabTemplate_Serialize,
    type TabTemplateInput_Deserialize,
    type TemplateChangedPayload,
    type TemplateId,
    type WorkspaceEvent
} from '@/types/bindings';

/** Lista todos os templates configurados em um projeto (`template_list`). */
export function templateList(projectId: ProjectId): Promise<TabTemplate_Serialize[]> {
    return unwrap(commands.templateList(projectId), 'template_list');
}

/** Obtém um template específico pelo identificador (`template_get`). */
export function templateGet(templateId: TemplateId): Promise<TabTemplate_Serialize> {
    return unwrap(commands.templateGet(templateId), 'template_get');
}

/** Cria um novo template de aba no projeto (`template_create`). */
export function templateCreate(
    projectId: ProjectId,
    input: TabTemplateInput_Deserialize
): Promise<TabTemplate_Serialize> {
    return unwrap(commands.templateCreate(projectId, input), 'template_create');
}

/** Atualiza um template existente validando `expected_revision` (`template_update`). */
export function templateUpdate(
    templateId: TemplateId,
    expectedRevision: number,
    input: TabTemplateInput_Deserialize
): Promise<TabTemplate_Serialize> {
    return unwrap(
        commands.templateUpdate(templateId, expectedRevision, input),
        'template_update'
    );
}

/** Remove um template não-padrão de um projeto (`template_delete`). */
export function templateDelete(
    templateId: TemplateId,
    expectedRevision: number
): Promise<null> {
    return unwrap(commands.templateDelete(templateId, expectedRevision), 'template_delete');
}

/** Define um template como padrão do projeto (`template_set_default`). */
export function templateSetDefault(templateId: TemplateId): Promise<TabTemplate_Serialize[]> {
    return unwrap(commands.templateSetDefault(templateId), 'template_set_default');
}

/** Escuta o evento global `template://changed`. */
export function onTemplateChanged(
    cb: (ev: WorkspaceEvent<TemplateChangedPayload>) => void
): Promise<UnlistenFn> {
    return listen<WorkspaceEvent<TemplateChangedPayload>>('template://changed', (ev) => {
        cb(ev.payload);
    });
}
