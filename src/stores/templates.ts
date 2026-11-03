import { defineStore } from 'pinia';
import { ref } from 'vue';
import { useIpcError } from '@/composables/useIpcError';
import * as templatesIpc from '@/ipc/templates';
import type {
    ProjectId,
    TabTemplate_Serialize,
    TabTemplateInput_Deserialize,
    TemplateChangedPayload,
    TemplateId
} from '@/types/bindings';

export const useTemplatesStore = defineStore('templates', () => {
    const ipcError = useIpcError();

    const byProject = ref<Record<ProjectId, TabTemplate_Serialize[]>>({});
    const loadingByProject = ref<Record<ProjectId, boolean>>({});
    const error = ref<string | null>(null);

    const inflightLoads = new Map<ProjectId, Promise<TabTemplate_Serialize[]>>();

    function templatesFor(projectId: ProjectId): TabTemplate_Serialize[] {
        return byProject.value[projectId] ?? [];
    }

    function defaultTemplateFor(projectId: ProjectId): TabTemplate_Serialize | null {
        const list = byProject.value[projectId] ?? [];
        return list.find((t) => t.isDefault) ?? list[0] ?? null;
    }

    function templateById(templateId: TemplateId): TabTemplate_Serialize | null {
        for (const list of Object.values(byProject.value)) {
            const found = list.find((t) => t.id === templateId);
            if (found) {
                return found;
            }
        }
        return null;
    }

    function setProjectTemplates(projectId: ProjectId, list: TabTemplate_Serialize[]): void {
        const sorted = [...list].sort((a, b) => a.position - b.position);
        byProject.value = {
            ...byProject.value,
            [projectId]: sorted
        };
    }

    async function loadForProject(
        projectId: ProjectId,
        force = false
    ): Promise<TabTemplate_Serialize[]> {
        if (!force && byProject.value[projectId] !== undefined) {
            return byProject.value[projectId] ?? [];
        }
        const existing = inflightLoads.get(projectId);
        if (existing) {
            return existing;
        }

        loadingByProject.value = {
            ...loadingByProject.value,
            [projectId]: true
        };
        error.value = null;

        const promise = templatesIpc
            .templateList(projectId)
            .then((list) => {
                setProjectTemplates(projectId, list);
                return byProject.value[projectId] ?? [];
            })
            .catch((err: unknown) => {
                error.value = ipcError.message(err);
                throw err;
            })
            .finally(() => {
                inflightLoads.delete(projectId);
                loadingByProject.value = {
                    ...loadingByProject.value,
                    [projectId]: false
                };
            });

        inflightLoads.set(projectId, promise);
        return promise;
    }

    async function createTemplate(
        projectId: ProjectId,
        input: TabTemplateInput_Deserialize
    ): Promise<TabTemplate_Serialize> {
        error.value = null;
        try {
            const created = await templatesIpc.templateCreate(projectId, input);
            await loadForProject(projectId, true);
            return created;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function updateTemplate(
        templateId: TemplateId,
        expectedRevision: number,
        input: TabTemplateInput_Deserialize
    ): Promise<TabTemplate_Serialize> {
        error.value = null;
        try {
            const updated = await templatesIpc.templateUpdate(
                templateId,
                expectedRevision,
                input
            );
            await loadForProject(updated.projectId, true);
            return updated;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function deleteTemplate(
        projectId: ProjectId,
        templateId: TemplateId,
        expectedRevision: number
    ): Promise<void> {
        error.value = null;
        try {
            await templatesIpc.templateDelete(templateId, expectedRevision);
            await loadForProject(projectId, true);
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function setDefaultTemplate(
        projectId: ProjectId,
        templateId: TemplateId
    ): Promise<TabTemplate_Serialize[]> {
        error.value = null;
        try {
            const updatedList = await templatesIpc.templateSetDefault(templateId);
            setProjectTemplates(projectId, updatedList);
            return updatedList;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        }
    }

    function handleTemplateChangedEvent(payload: TemplateChangedPayload): void {
        void loadForProject(payload.project_id, true).catch(() => {
            // Ignora falha silenciosa de refresh em background.
        });
    }

    function clearProject(projectId: ProjectId): void {
        if (byProject.value[projectId] === undefined) {
            return;
        }
        const next = { ...byProject.value };
        delete next[projectId];
        byProject.value = next;
    }

    function resetForTests(): void {
        inflightLoads.clear();
        byProject.value = {};
        loadingByProject.value = {};
        error.value = null;
    }

    return {
        byProject,
        loadingByProject,
        error,
        templatesFor,
        defaultTemplateFor,
        templateById,
        loadForProject,
        createTemplate,
        updateTemplate,
        deleteTemplate,
        setDefaultTemplate,
        handleTemplateChangedEvent,
        clearProject,
        resetForTests
    };
});
