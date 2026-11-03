<template>
    <MaxModal :visible="props.visible" title="Editar projeto" :no-button="true" max-width="32rem" class="project-edit-modal" :dismissable="true" :close-on-escape="true" @update:visible="handleClose">
        <ProjectIdentityForm v-if="props.project" :project="props.project" :active="props.visible" @saved="handleSaved" @cancel="handleClose" />
    </MaxModal>
</template>

<script setup lang="ts">
    import MaxModal from '@maxvue/max-components-ui/components/MaxModal';
    import ProjectIdentityForm from '@/components/projects/ProjectIdentityForm.vue';
    import type { Project_Serialize } from '@/types/bindings';

    export interface ProjectEditModalProps {
        visible: boolean;
        project: Project_Serialize | null;
    }

    const props = defineProps<ProjectEditModalProps>();

    const emit = defineEmits<{
        'update:visible': [v: boolean];
        saved: [p: Project_Serialize];
    }>();

    function handleClose(): void {
        emit('update:visible', false);
    }

    function handleSaved(p: Project_Serialize): void {
        emit('saved', p);
        emit('update:visible', false);
    }
</script>

<style lang="scss" scoped>
    :deep(.project-edit-modal.max-modal) {
        background-color: var(--background-800);
        border-color: var(--background-600);
        color: var(--background-100);

        .max-modal-content {
            padding: 1.125rem;
        }
    }
</style>
