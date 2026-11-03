<template>
    <div v-if="props.conflicts.length > 0" class="skill-conflict-list" data-testid="skill-conflict-list">
        <h4 class="skill-conflict-list__title">Conflitos Detectados ({{ props.conflicts.length }})</h4>
        <div class="skill-conflict-list__items">
            <div v-for="item in props.conflicts" :key="item.skill" class="skill-conflict-list__item" :data-testid="`skill-conflict-${item.skill}`">
                <div class="skill-conflict-list__info">
                    <div class="skill-conflict-list__name-row">
                        <MaxIcon icon="lucide:alert-triangle" />
                        <strong>{{ item.skill }}</strong>
                        <span class="skill-conflict-list__dest">{{ item.dest }}</span>
                    </div>
                    <p class="skill-conflict-list__reason">{{ formatConflictReason(item.conflict) }}</p>
                </div>
                <div class="skill-conflict-list__actions" role="radiogroup" :aria-label="`Resolução para ${item.skill}`">
                    <button type="button" class="skill-conflict-list__btn" :class="{ 'skill-conflict-list__btn--active': resolutionFor(item.skill) === 'keep' }" :data-testid="`resolve-keep-${item.skill}`" @click="setResolution(item.skill, 'keep')">Manter atual</button>
                    <button type="button" class="skill-conflict-list__btn" :class="{ 'skill-conflict-list__btn--active': resolutionFor(item.skill) === 'overwrite' }" :data-testid="`resolve-overwrite-${item.skill}`" @click="setResolution(item.skill, 'overwrite')">Sobrescrever com backup</button>
                    <button type="button" class="skill-conflict-list__btn" :class="{ 'skill-conflict-list__btn--active': resolutionFor(item.skill) === 'adopt' }" :data-testid="`resolve-adopt-${item.skill}`" @click="setResolution(item.skill, 'adopt')">Adotar</button>
                </div>
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
    import type { ConflictKind, ConflictResolution, SyncAction } from '@/types/bindings';

    export type ConflictSyncAction = Extract<SyncAction, { action: 'conflict' }>;

    export interface SkillConflictListProps {
        conflicts: ConflictSyncAction[];
        modelValue: Record<string, ConflictResolution>;
    }

    const props = defineProps<SkillConflictListProps>();

    const emit = defineEmits<{
        'update:modelValue': [value: Record<string, ConflictResolution>];
    }>();

    function resolutionFor(skill: string): ConflictResolution {
        return props.modelValue[skill] ?? 'keep';
    }

    function setResolution(skill: string, resolution: ConflictResolution): void {
        emit('update:modelValue', {
            ...props.modelValue,
            [skill]: resolution
        });
    }

    function formatConflictReason(conflict: ConflictKind): string {
        switch (conflict.kind) {
            case 'foreignSymlink':
                return `Symlink externo apontando para "${conflict.target}".`;
            case 'unmanaged':
                return 'Diretório existente não gerenciado pelo MaxTerminal.';
            case 'locallyModified':
                return 'Cópia modificada localmente após a última sincronização.';
        }
    }
</script>

<style lang="scss" scoped>
    .skill-conflict-list {
        display: flex;
        flex-direction: column;
        gap: 0.625rem;
        padding: 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--max-warning-500);
        background: rgb(245 158 11 / 10%);
    }

    .skill-conflict-list__title {
        margin: 0;
        font-size: 0.8125rem;
        font-weight: 600;
        color: var(--max-warning-300);
    }

    .skill-conflict-list__items {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
    }

    .skill-conflict-list__item {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.75rem;
        flex-wrap: wrap;
        padding: 0.5rem 0.625rem;
        border-radius: 0.25rem;
        background: var(--background-900);
        border: 1px solid var(--background-700);
    }

    .skill-conflict-list__info {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
    }

    .skill-conflict-list__name-row {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        font-size: 0.8125rem;
        color: var(--background-100);
    }

    .skill-conflict-list__dest {
        font-size: 0.7188rem;
        color: var(--background-400);
        font-family: monospace;
    }

    .skill-conflict-list__reason {
        margin: 0;
        font-size: 0.75rem;
        color: var(--max-warning-300);
    }

    .skill-conflict-list__actions {
        display: inline-flex;
        gap: 0.375rem;
    }

    .skill-conflict-list__btn {
        padding: 0.25rem 0.5rem;
        border-radius: 0.25rem;
        border: 1px solid var(--background-600);
        background: var(--background-800);
        color: var(--background-200);
        font-size: 0.7188rem;
        font-weight: 600;
        cursor: pointer;
    }

    .skill-conflict-list__btn--active {
        border-color: var(--max-primary-500);
        background: rgb(20 184 166 / 20%);
        color: var(--max-primary-300);
    }
</style>
