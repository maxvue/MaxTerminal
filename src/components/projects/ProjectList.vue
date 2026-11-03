<template>
    <aside class="project-list" aria-label="Projetos" @keydown="handleListKeydown">
        <header class="project-list__header">
            <span class="project-list__heading">Projetos</span>
            <div class="project-list__toolbar">
                <button type="button" class="project-list__tool-btn" title="Adicionar projeto" aria-label="Adicionar projeto" @click="showAddModal = true">
                    <MaxIcon icon="mdi:plus" />
                    <span>Adicionar projeto</span>
                </button>
                <button type="button" class="project-list__tool-btn project-list__tool-btn--icon" title="Adicionar pasta de projetos" aria-label="Adicionar pasta de projetos" @click="openNewRootModal">
                    <MaxIcon icon="mdi:folder-multiple-plus-outline" />
                </button>
            </div>
        </header>

        <ProjectDiscoveryPanel v-if="projectsStore.discovered.length > 0" :items="projectsStore.discovered" @accept="handleAcceptDiscovered" @dismiss="handleDismissDiscovered" />

        <div v-if="projectsStore.error && projectsStore.projects.length === 0" class="project-list__error" role="alert">
            <span>{{ projectsStore.error }}</span>
            <button type="button" class="project-list__retry-btn" @click="handleRetry">
                Tentar novamente
            </button>
        </div>

        <div class="project-list__scroll">
            <ul v-if="projectsStore.projects.length > 0" class="project-list__items" role="list">
                <li v-for="(proj, idx) in projectsStore.projects" :key="proj.id" class="project-list__li" :draggable="!projectsStore.reordering" @dragstart="onDragStart(idx)" @dragover.prevent @drop="onDrop(idx)">
                    <ProjectItem :project="proj" :counters="projectsStore.countersFor(proj.id)" :selected="projectsStore.selectedId === proj.id" @select="handleSelectProject" @edit="handleOpenEdit" @redetect="handleRedetect" @remove="handleRemoveProject" />
                </li>
            </ul>
            <ProjectEmptyState v-else-if="!projectsStore.loading" @add-project="showAddModal = true" @add-root="openNewRootModal" />
        </div>

        <ProjectAddModal v-model:visible="showAddModal" />
        <ProjectRootModal v-model:visible="showRootModal" :root="editingRoot" />
        <ProjectEditModal v-model:visible="showEditModal" :project="editingProject" />
    </aside>
</template>

<script setup lang="ts">
    import { computed, ref } from 'vue';
    import ProjectAddModal from '@/components/projects/ProjectAddModal.vue';
    import ProjectDiscoveryPanel from '@/components/projects/ProjectDiscoveryPanel.vue';
    import ProjectEditModal from '@/components/projects/ProjectEditModal.vue';
    import ProjectEmptyState from '@/components/projects/ProjectEmptyState.vue';
    import ProjectItem from '@/components/projects/ProjectItem.vue';
    import ProjectRootModal from '@/components/projects/ProjectRootModal.vue';
    import { useProjectsStore } from '@/stores/projects';
    import type { Project_Serialize, ProjectId, ProjectRoot } from '@/types/bindings';

    const projectsStore = useProjectsStore();

    const showAddModal = ref<boolean>(false);
    const showRootModal = ref<boolean>(false);
    const showEditModal = ref<boolean>(false);
    const editingRoot = ref<ProjectRoot | null>(null);
    const editingProjectId = ref<ProjectId | null>(null);
    const draggedIndex = ref<number | null>(null);

    const editingProject = computed<Project_Serialize | null>(() => {
        if (!editingProjectId.value) {
            return null;
        }
        return projectsStore.projectById(editingProjectId.value) ?? null;
    });

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

    function onDragStart(idx: number): void {
        if (projectsStore.reordering) {
            return;
        }
        draggedIndex.value = idx;
    }

    async function onDrop(targetIdx: number): Promise<void> {
        const fromIdx = draggedIndex.value;
        draggedIndex.value = null;
        if (fromIdx === null || fromIdx === targetIdx || projectsStore.reordering) {
            return;
        }
        const ids = projectsStore.projects.map((p) => p.id);
        const [moved] = ids.splice(fromIdx, 1);
        if (!moved) {
            return;
        }
        ids.splice(targetIdx, 0, moved);
        try {
            await projectsStore.reorder(ids);
        } catch {
            // Recarregado pela store em caso de mismatch.
        }
    }

    function isEditableElement(target: EventTarget | null): boolean {
        if (!target || !(target instanceof HTMLElement)) {
            return false;
        }
        const tag = target.tagName.toLowerCase();
        return tag === 'input' || tag === 'textarea' || tag === 'select' || target.isContentEditable;
    }

    async function handleListKeydown(e: KeyboardEvent): Promise<void> {
        if (isEditableElement(e.target)) {
            return;
        }
        const list = projectsStore.projects;
        if (list.length === 0) {
            return;
        }

        const currentIdx = projectsStore.selectedId
            ? list.findIndex((p) => p.id === projectsStore.selectedId)
            : -1;

        if (e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
            if (currentIdx === -1 || projectsStore.reordering) {
                return;
            }
            const targetIdx = e.key === 'ArrowUp' ? currentIdx - 1 : currentIdx + 1;
            if (targetIdx < 0 || targetIdx >= list.length) {
                return;
            }
            e.preventDefault();
            const ids = list.map((p) => p.id);
            const [moved] = ids.splice(currentIdx, 1);
            if (!moved) {
                return;
            }
            ids.splice(targetIdx, 0, moved);
            try {
                await projectsStore.reorder(ids);
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
        background: var(--background-800);
        border-right: 1px solid var(--background-700);
        user-select: none;
    }

    .project-list__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
        padding: 0.625rem 0.75rem;
        border-bottom: 1px solid var(--background-700);
    }

    .project-list__heading {
        font-size: 0.75rem;
        font-weight: 700;
        text-transform: uppercase;
        letter-spacing: 0.04em;
        color: var(--background-300);
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
        border: 1px solid var(--background-600);
        background: var(--background-700);
        color: var(--background-100);
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &:hover {
            background: var(--background-600);
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
        background: var(--background-900);
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
        color: var(--background-50);
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
</style>
