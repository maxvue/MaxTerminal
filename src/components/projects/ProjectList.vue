<template>
    <aside class="project-list" aria-label="Projetos" @keydown="handleListKeydown">
        <header class="project-list__header">
            <span class="project-list__heading">Projetos</span>
            <div class="project-list__toolbar">
                <MaxButton type="button" variant="secondary" size="small" class="project-list__tool-btn" title="Adicionar projeto" aria-label="Adicionar projeto" @click="showAddModal = true">
                    <MaxIcon icon="mdi:plus" />
                    <span>Adicionar projeto</span>
                </MaxButton>
                <MaxIconButton icon="mdi:folder-multiple-plus-outline" size="1.1" class="project-list__tool-btn project-list__tool-btn--icon" title="Adicionar pasta de projetos" aria-label="Adicionar pasta de projetos" @click="openNewRootModal" />
            </div>
        </header>

        <ProjectDiscoveryPanel v-if="projectsStore.discovered.length > 0" :items="projectsStore.discovered" @accept="handleAcceptDiscovered" @dismiss="handleDismissDiscovered" />

        <MaxAlert v-if="projectsStore.error && projectsStore.projects.length === 0" type="danger" role="alert" class="project-list__error">
            <span>{{ projectsStore.error }}</span>
            <MaxButton type="button" variant="primary" size="small" class="project-list__retry-btn" @click="handleRetry">
                Tentar novamente
            </MaxButton>
        </MaxAlert>

        <MaxAlert v-if="visibilityError" type="danger" role="alert" class="project-list__error">
            <span>{{ visibilityError }}</span>
            <MaxButton type="button" variant="primary" size="small" class="project-list__retry-btn" @click="visibilityError = null">
                Fechar
            </MaxButton>
        </MaxAlert>

        <div class="project-list__scroll">
            <MaxQueryState :state="listState" :retryable="true" :message="projectsStore.error ?? undefined" @retry="handleRetry">
                <template #empty>
                    <ProjectEmptyState @add-project="showAddModal = true" @add-root="openNewRootModal" />
                </template>
                <ul v-if="projectsStore.visibleProjects.length > 0" class="project-list__items" role="list">
                    <li v-for="proj in projectsStore.visibleProjects" :key="proj.id" class="project-list__li" :draggable="!projectsStore.reordering" @dragstart="onDragStart(proj.id)" @dragover.prevent @drop="onDrop(proj.id)">
                        <ProjectItem :project="proj" :counters="projectsStore.countersFor(proj.id)" :selected="projectsStore.selectedId === proj.id" :hidden="false" :disabled="isItemSaving(proj.id)" :pending="isItemSaving(proj.id)" @select="handleSelectProject" @edit="handleOpenEdit" @configure="handleConfigureProject" @redetect="handleRedetect" @remove="handleRemoveProject" @hide="handleHideProject" />
                    </li>
                </ul>
                <div v-else-if="projectsStore.projects.length > 0" class="project-list__all-hidden">
                    <MaxIcon icon="lucide:eye-off" class="project-list__all-hidden-icon" />
                    <span>Todos os projetos estão ocultos</span>
                </div>
            </MaxQueryState>
        </div>

        <div v-if="projectsStore.hiddenProjects.length > 0" class="project-list__accordion-wrapper">
            <MaxAccordion v-model:value="hiddenAccordionValue" class="project-list__accordion">
                <MaxAccordionItem value="hidden-projects" :title="`Ocultos (${projectsStore.hiddenProjects.length})`" :header-aria-level="3">
                    <ul class="project-list__items project-list__items--hidden" role="list">
                        <li v-for="proj in projectsStore.hiddenProjects" :key="proj.id" class="project-list__li">
                            <ProjectItem :project="proj" :counters="projectsStore.countersFor(proj.id)" :selected="projectsStore.selectedId === proj.id" :hidden="true" :disabled="isItemSaving(proj.id)" :pending="isItemSaving(proj.id)" @select="handleSelectProject" @edit="handleOpenEdit" @configure="handleConfigureProject" @redetect="handleRedetect" @remove="handleRemoveProject" @show="handleShowProject" />
                        </li>
                    </ul>
                </MaxAccordionItem>
            </MaxAccordion>
        </div>

        <footer class="project-list__footer">
            <MaxButton type="button" variant="secondary" class="project-list__settings-btn" title="Configurações gerais" aria-label="Abrir configurações gerais" @click="handleOpenGeneralSettings">
                <MaxIcon icon="lucide:settings" />
                <span>Configurações</span>
            </MaxButton>
        </footer>

        <ProjectAddModal v-model:visible="showAddModal" />
        <ProjectRootModal v-model:visible="showRootModal" :root="editingRoot" />
        <ProjectEditModal v-model:visible="showEditModal" :project="editingProject" />
    </aside>
</template>

<script setup lang="ts">
    import { computed, nextTick, ref } from 'vue';
    import { MaxAccordion, MaxAccordionItem, MaxAlert, MaxButton, MaxIconButton, MaxQueryState } from '@maxvue/max-components-ui';
    import ProjectAddModal from '@/components/projects/ProjectAddModal.vue';
    import ProjectDiscoveryPanel from '@/components/projects/ProjectDiscoveryPanel.vue';
    import ProjectEditModal from '@/components/projects/ProjectEditModal.vue';
    import ProjectEmptyState from '@/components/projects/ProjectEmptyState.vue';
    import ProjectItem from '@/components/projects/ProjectItem.vue';
    import { computeReorderedProjectIds } from '@/components/projects/projectOrder';
    import ProjectRootModal from '@/components/projects/ProjectRootModal.vue';
    import { useProjectsStore } from '@/stores/projects';
    import { useWindowStore } from '@/stores/window';
    import type { Project_Serialize, ProjectId, ProjectRoot } from '@/types/bindings';

    const projectsStore = useProjectsStore();
    const windowStore = useWindowStore();

    const showAddModal = ref<boolean>(false);
    const showRootModal = ref<boolean>(false);
    const showEditModal = ref<boolean>(false);
    const editingRoot = ref<ProjectRoot | null>(null);
    const editingProjectId = ref<ProjectId | null>(null);
    const draggedVisibleId = ref<ProjectId | null>(null);
    const hiddenAccordionValue = ref<string | undefined>(undefined);
    const savingProjectId = ref<ProjectId | null>(null);
    const visibilityError = ref<string | null>(null);

    const editingProject = computed<Project_Serialize | null>(() => {
        if (!editingProjectId.value) {
            return null;
        }
        return projectsStore.projectById(editingProjectId.value) ?? null;
    });

    const listState = computed<'loading' | 'error' | 'ready-empty' | 'ready-with-data'>(() => {
        if (projectsStore.loading && projectsStore.projects.length === 0) return 'loading';
        if (projectsStore.error && projectsStore.projects.length === 0) return 'error';
        if (projectsStore.projects.length === 0) return 'ready-empty';
        return 'ready-with-data';
    });

    function isItemSaving(id: ProjectId): boolean {
        return savingProjectId.value === id;
    }

    function openNewRootModal(): void {
        editingRoot.value = null;
        showRootModal.value = true;
    }

    function handleSelectProject(id: ProjectId): void {
        projectsStore.select(id);
    }

    function handleOpenEdit(id: ProjectId): void {
        editingProjectId.value = id;
        showEditModal.value = true;
    }

    function handleConfigureProject(id: ProjectId): void {
        windowStore.openProjectSettings(id);
    }

    function handleOpenGeneralSettings(): void {
        windowStore.openPreferences('system');
    }

    async function handleRedetect(id: ProjectId): Promise<void> {
        try {
            await projectsStore.redetect(id);
        } catch {
            // Erro já registrado em projectsStore.error.
        }
    }

    async function handleRemoveProject(id: ProjectId): Promise<void> {
        try {
            await projectsStore.removeProject(id);
        } catch {
            // Erro já registrado em projectsStore.error.
        }
    }

    async function handleHideProject(id: ProjectId): Promise<void> {
        if (savingProjectId.value !== null) {
            return;
        }
        const visibleList = projectsStore.visibleProjects;
        const currentIdx = visibleList.findIndex((p) => p.id === id);
        let neighborId: ProjectId | null = null;
        if (currentIdx !== -1 && visibleList.length > 1) {
            const candidate = visibleList[currentIdx + 1] ?? visibleList[currentIdx - 1];
            neighborId = candidate?.id ?? null;
        }

        savingProjectId.value = id;
        visibilityError.value = null;
        try {
            const ok = await projectsStore.setProjectHidden(id, true);
            if (!ok) {
                visibilityError.value = 'Falha ao ocultar o projeto. Tente novamente.';
                return;
            }
            await nextTick();
            if (neighborId) {
                const el = document.querySelector<HTMLElement>(`[data-project-id="${neighborId}"] .project-item__main`);
                el?.focus();
            } else {
                const headerEl = document.querySelector<HTMLElement>('.project-list__accordion-wrapper [role="button"]');
                headerEl?.focus();
            }
        } catch {
            visibilityError.value = 'Erro ao ocultar o projeto. Tente novamente.';
        } finally {
            savingProjectId.value = null;
        }
    }

    async function handleShowProject(id: ProjectId): Promise<void> {
        if (savingProjectId.value !== null) {
            return;
        }
        const hiddenList = projectsStore.hiddenProjects;
        const currentIdx = hiddenList.findIndex((p) => p.id === id);
        let neighborId: ProjectId | null = null;
        if (currentIdx !== -1 && hiddenList.length > 1) {
            const candidate = hiddenList[currentIdx + 1] ?? hiddenList[currentIdx - 1];
            neighborId = candidate?.id ?? null;
        }

        savingProjectId.value = id;
        visibilityError.value = null;
        try {
            const ok = await projectsStore.setProjectHidden(id, false);
            if (!ok) {
                visibilityError.value = 'Falha ao restaurar o projeto. Tente novamente.';
                return;
            }
            await nextTick();
            if (neighborId && projectsStore.hiddenProjects.length > 0) {
                const el = document.querySelector<HTMLElement>(`[data-project-id="${neighborId}"] .project-item__main`);
                el?.focus();
            } else {
                const settingsBtn = document.querySelector<HTMLElement>('.project-list__settings-btn');
                settingsBtn?.focus();
            }
        } catch {
            visibilityError.value = 'Erro ao restaurar o projeto. Tente novamente.';
        } finally {
            savingProjectId.value = null;
        }
    }

    async function handleAcceptDiscovered(paths: string[]): Promise<void> {
        try {
            await projectsStore.acceptDiscovered(paths);
        } catch {
            // Erro já registrado em projectsStore.error.
        }
    }

    async function handleDismissDiscovered(paths: string[]): Promise<void> {
        try {
            await projectsStore.dismissDiscovered(paths);
        } catch {
            // Erro já registrado em projectsStore.error.
        }
    }

    async function handleRetry(): Promise<void> {
        try {
            await projectsStore.resync();
        } catch {
            // Mantém mensagem de erro.
        }
    }

    function onDragStart(id: ProjectId): void {
        if (projectsStore.reordering) {
            return;
        }
        draggedVisibleId.value = id;
    }

    async function onDrop(targetId: ProjectId): Promise<void> {
        const fromId = draggedVisibleId.value;
        draggedVisibleId.value = null;
        if (!fromId || fromId === targetId || projectsStore.reordering) {
            return;
        }
        const hiddenSet = new Set(projectsStore.hiddenProjectIds);
        const reorderedIds = computeReorderedProjectIds(
            projectsStore.projects,
            hiddenSet,
            fromId,
            targetId
        );
        if (!reorderedIds) {
            return;
        }
        try {
            await projectsStore.reorder(reorderedIds);
        } catch {
            // Recarregado pela store em caso de mismatch.
        }
    }

    function isEditableElement(target: EventTarget | null): boolean {
        if (!target || !(target instanceof HTMLElement)) {
            return false;
        }
        if (target.closest('.project-list__footer')) {
            return true;
        }
        if (target.closest('.project-item__actions') || target.closest('.project-list__toolbar')) {
            return true;
        }
        if (target.closest('.max-accordion-item-header') || target.closest('[role="heading"]')) {
            return true;
        }
        const tag = target.tagName.toLowerCase();
        return tag === 'input' || tag === 'textarea' || tag === 'select' || target.isContentEditable;
    }

    async function handleListKeydown(e: KeyboardEvent): Promise<void> {
        if (isEditableElement(e.target)) {
            return;
        }

        const targetEl = e.target instanceof HTMLElement ? e.target : null;
        const isFromHidden = Boolean(targetEl?.closest('.project-list__items--hidden'));
        const list = isFromHidden ? projectsStore.hiddenProjects : projectsStore.visibleProjects;

        if (list.length === 0) {
            return;
        }

        const currentIdx = projectsStore.selectedId
            ? list.findIndex((p) => p.id === projectsStore.selectedId)
            : -1;

        if (e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
            if (isFromHidden || currentIdx === -1 || projectsStore.reordering) {
                return;
            }
            const targetIdx = e.key === 'ArrowUp' ? currentIdx - 1 : currentIdx + 1;
            if (targetIdx < 0 || targetIdx >= list.length) {
                return;
            }
            e.preventDefault();

            const hiddenSet = new Set(projectsStore.hiddenProjectIds);
            const fromProj = list[currentIdx];
            const targetProj = list[targetIdx];
            if (!fromProj || !targetProj) {
                return;
            }
            const reorderedIds = computeReorderedProjectIds(
                projectsStore.projects,
                hiddenSet,
                fromProj.id,
                targetProj.id
            );
            if (!reorderedIds) {
                return;
            }
            try {
                await projectsStore.reorder(reorderedIds);
            } catch {
                // Ignora.
            }
            return;
        }

        if (e.key === 'ArrowDown') {
            e.preventDefault();
            const nextIdx = currentIdx < list.length - 1 ? currentIdx + 1 : 0;
            const nextProj = list[nextIdx];
            if (nextProj) {
                projectsStore.select(nextProj.id);
            }
        } else if (e.key === 'ArrowUp') {
            e.preventDefault();
            const prevIdx = currentIdx > 0 ? currentIdx - 1 : list.length - 1;
            const prevProj = list[prevIdx];
            if (prevProj) {
                projectsStore.select(prevProj.id);
            }
        }
    }
</script>

<style lang="scss" scoped>
    @use '@/styles/params' as *;

    .project-list {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
        background: var(--mt-ui-surface-panel, var(--background-25));
        border-right: 1px solid var(--mt-ui-border, var(--background-100));
        user-select: none;
    }

    .project-list__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
        padding: 0.625rem 0.75rem;
        border-bottom: 1px solid var(--mt-ui-border, var(--background-100));
    }

    .project-list__heading {
        font-size: 0.75rem;
        font-weight: 700;
        text-transform: uppercase;
        letter-spacing: 0.04em;
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .project-list__toolbar {
        display: flex;
        align-items: center;
        gap: 0.25rem;
    }

    .project-list__tool-btn {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        padding: 0.25rem 0.5rem;
        border-radius: 0.375rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text, var(--background-800));
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &:hover {
            background: var(--mt-ui-surface-hover, var(--background-75));
        }

        &:focus-visible {
            @include mt-focus-ring;
        }
    }

    .project-list__tool-btn--icon {
        padding: 0.25rem 0.375rem;
    }

    .project-list__error {
        margin: 0.5rem;
        padding: 0.5rem;
        border-radius: 0.375rem;
        border: 1px solid var(--max-danger-500);
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--max-danger-400);
        font-size: 0.75rem;
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .project-list__retry-btn {
        align-self: flex-start;
        padding: 0.25rem 0.5rem;
        border-radius: 0.25rem;
        border: 1px solid var(--max-primary-500);
        background: var(--max-primary-600);
        color: var(--max-primary-content);
        font-size: 0.6875rem;
        cursor: pointer;
    }

    .project-list__scroll {
        flex: 1;
        min-height: 0;
        overflow-y: auto;
        padding: 0.375rem;
    }

    .project-list__items {
        list-style: none;
        margin: 0;
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: 0.1875rem;
    }

    .project-list__li {
        list-style: none;
    }

    .project-list__all-hidden {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 0.625rem;
        padding: 2.5rem 1rem;
        color: var(--background-400);
        font-size: 0.8125rem;
        text-align: center;
    }

    .project-list__all-hidden-icon {
        font-size: 1.5rem;
        color: var(--background-500);
    }

    .project-list__accordion-wrapper {
        border-top: 1px solid var(--background-700);
        background: var(--background-800);
        max-height: 14rem;
        overflow-y: auto;
    }

    :deep(.project-list__accordion) {
        border: none !important;
        border-radius: 0 !important;
        background: transparent !important;
    }

    :deep(.project-list__accordion .max-accordion-item) {
        border-bottom: none !important;
    }

    :deep(.project-list__accordion .max-accordion-item-header) {
        padding: 0.4375rem 0.75rem !important;
        color: var(--background-300) !important;
        font-size: 0.75rem !important;
        font-weight: 600 !important;
        border-radius: 0 !important;

        &:hover {
            background: var(--background-700) !important;
            color: var(--background-100) !important;
        }

        &:focus-visible {
            @include mt-focus-ring;
        }
    }

    :deep(.project-list__accordion .max-accordion-item-content-inner) {
        padding: 0.25rem 0.375rem 0.375rem !important;
    }

    .project-list__footer {
        display: flex;
        align-items: center;
        padding: 0.5rem 0.75rem;
        border-top: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
    }

    .project-list__settings-btn {
        display: inline-flex;
        align-items: center;
        gap: 0.5rem;
        width: 100%;
        padding: 0.375rem 0.5rem;
        border-radius: 0.375rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text, var(--background-800));
        font-size: 0.8125rem;
        font-weight: 500;
        cursor: pointer;

        &:hover {
            background: var(--mt-ui-surface-hover, var(--background-75));
            color: var(--mt-ui-text-strong, var(--background-950));
        }

        &:focus-visible {
            @include mt-focus-ring;
        }
    }
</style>
