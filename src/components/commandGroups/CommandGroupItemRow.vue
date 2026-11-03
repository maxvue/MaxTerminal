<template>
    <div ref="rowRootRef" class="group-item-row" :data-testid="`group-item-row-${index}`" tabindex="-1" @keydown="onRowKeydown">
        <div class="group-item-row__header">
            <span class="group-item-row__badge" aria-hidden="true">#{{ index + 1 }}</span>
            <div class="group-item-row__main-fields">
                <label class="group-item-row__field">
                    <span class="group-item-row__label">Título do Item</span>
                    <input type="text" class="group-item-row__input" :data-testid="`item-title-${index}`" placeholder="Ex.: Subir servidor Vite" :value="item.title" @input="onTitleInput">
                    <span v-if="titleError" class="group-item-row__error" :data-testid="`item-title-error-${index}`">{{ titleError }}</span>
                </label>

                <label class="group-item-row__field group-item-row__field--cmd">
                    <span class="group-item-row__label">Comando Shell</span>
                    <input type="text" class="group-item-row__input group-item-row__input--mono" :data-testid="`item-command-${index}`" placeholder="npm run dev" :value="item.command" @input="onCommandInput">
                    <span v-if="commandError" class="group-item-row__error" :data-testid="`item-command-error-${index}`">{{ commandError }}</span>
                </label>
            </div>

            <div class="group-item-row__actions">
                <button type="button" class="group-item-row__icon-btn" :data-testid="`item-move-up-${index}`" :disabled="index === 0" title="Mover para cima (Alt+↑)" aria-label="Mover item para cima" @click="emit('move-up', index)">
                    <MaxIcon icon="lucide:arrow-up" />
                </button>
                <button type="button" class="group-item-row__icon-btn" :data-testid="`item-move-down-${index}`" :disabled="index >= total - 1" title="Mover para baixo (Alt+↓)" aria-label="Mover item para baixo" @click="emit('move-down', index)">
                    <MaxIcon icon="lucide:arrow-down" />
                </button>
                <button type="button" class="group-item-row__icon-btn" :data-testid="`item-duplicate-${index}`" title="Duplicar item" aria-label="Duplicar item" @click="emit('duplicate', index)">
                    <MaxIcon icon="lucide:copy" />
                </button>
                <button type="button" class="group-item-row__icon-btn group-item-row__icon-btn--danger" :data-testid="`item-remove-${index}`" title="Remover item" aria-label="Remover item" @click="emit('remove', index)">
                    <MaxIcon icon="lucide:trash-2" />
                </button>
            </div>
        </div>

        <div class="group-item-row__sub">
            <label class="group-item-row__field group-item-row__field--cwd">
                <span class="group-item-row__label">Diretório relativo (opcional)</span>
                <input type="text" class="group-item-row__input group-item-row__input--mono" :data-testid="`item-cwd-${index}`" placeholder="packages/web" :value="item.cwd ?? ''" @input="onCwdInput">
                <span v-if="cwdError" class="group-item-row__error" :data-testid="`item-cwd-error-${index}`">{{ cwdError }}</span>
            </label>

            <label class="group-item-row__checkbox">
                <input type="checkbox" :data-testid="`item-continuous-${index}`" :checked="item.continuous" @change="onContinuousChange">
                <span>Execução Contínua (daemon/watcher)</span>
            </label>

            <button type="button" class="group-item-row__env-toggle" :data-testid="`item-add-env-${index}`" @click="onAddEnvPair">
                <MaxIcon icon="lucide:plus" />
                <span>Variável de ambiente ({{ item.envPairs.length }})</span>
            </button>
        </div>

        <div v-if="item.envPairs.length > 0" class="group-item-row__env-list">
            <p class="group-item-row__env-hint">Valores de ambiente são passados literalmente ao processo. Para segredos, prefira referenciar <code>$VAR</code> no comando.</p>
            <div v-for="(pair, envIdx) in item.envPairs" :key="envIdx" class="group-item-row__env-row">
                <input type="text" class="group-item-row__input group-item-row__input--mono" :data-testid="`item-${index}-env-key-${envIdx}`" placeholder="NOME_VAR" :value="pair.key" @input="onEnvKeyInput(envIdx, $event)">
                <input :type="shouldMaskEnv(pair.key, envIdx) ? 'password' : 'text'" class="group-item-row__input group-item-row__input--mono" :data-testid="`item-${index}-env-val-${envIdx}`" placeholder="valor" :value="pair.value" @input="onEnvValueInput(envIdx, $event)">
                <button v-if="isSensitiveEnvKey(pair.key)" type="button" class="group-item-row__icon-btn" :aria-label="revealedEnvIndices.has(envIdx) ? 'Ocultar valor' : 'Mostrar valor'" @click="toggleRevealEnv(envIdx)">
                    <MaxIcon :icon="revealedEnvIndices.has(envIdx) ? 'lucide:eye-off' : 'lucide:eye'" />
                </button>
                <button type="button" class="group-item-row__icon-btn group-item-row__icon-btn--danger" aria-label="Remover variável" @click="onRemoveEnvPair(envIdx)">
                    <MaxIcon icon="lucide:x" />
                </button>
            </div>
            <span v-if="envError" class="group-item-row__error" :data-testid="`item-env-error-${index}`">{{ envError }}</span>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed, ref } from 'vue';
    import { isSensitiveEnvKey } from '@/composables/commandGroups/useCommandGroupEditor';
    import type { EditableGroupItem } from '@/types/commandGroups';

    export interface CommandGroupItemRowProps {
        item: EditableGroupItem;
        index: number;
        total: number;
        fieldErrors: Record<string, string>;
    }

    export interface CommandGroupItemRowEmits {
        (e: 'update:item', item: EditableGroupItem): void;
        (e: 'move-up', index: number): void;
        (e: 'move-down', index: number): void;
        (e: 'duplicate', index: number): void;
        (e: 'remove', index: number): void;
    }

    const props = defineProps<CommandGroupItemRowProps>();
    const emit = defineEmits<CommandGroupItemRowEmits>();

    const rowRootRef = ref<HTMLElement | null>(null);
    const revealedEnvIndices = ref<Set<number>>(new Set());

    const titleError = computed<string | undefined>(
        () => props.fieldErrors[`items.${props.index}.title`] ?? props.fieldErrors[`items[${props.index}].title`]
    );
    const commandError = computed<string | undefined>(
        () => props.fieldErrors[`items.${props.index}.command`] ?? props.fieldErrors[`items[${props.index}].command`]
    );
    const cwdError = computed<string | undefined>(
        () => props.fieldErrors[`items.${props.index}.cwd`] ?? props.fieldErrors[`items[${props.index}].cwd`]
    );
    const envError = computed<string | undefined>(
        () => props.fieldErrors[`items.${props.index}.env`] ?? props.fieldErrors[`items[${props.index}].env`]
    );

    function shouldMaskEnv(key: string, envIdx: number): boolean {
        return isSensitiveEnvKey(key) && !revealedEnvIndices.value.has(envIdx);
    }

    function toggleRevealEnv(envIdx: number): void {
        const next = new Set(revealedEnvIndices.value);
        if (next.has(envIdx)) {
            next.delete(envIdx);
        } else {
            next.add(envIdx);
        }
        revealedEnvIndices.value = next;
    }

    function emitPatch(patch: Partial<EditableGroupItem>): void {
        emit('update:item', {
            ...props.item,
            ...patch
        });
    }

    function onTitleInput(event: Event): void {
        emitPatch({ title: (event.target as HTMLInputElement).value });
    }

    function onCommandInput(event: Event): void {
        emitPatch({ command: (event.target as HTMLInputElement).value });
    }

    function onCwdInput(event: Event): void {
        emitPatch({ cwd: (event.target as HTMLInputElement).value });
    }

    function onContinuousChange(event: Event): void {
        emitPatch({ continuous: (event.target as HTMLInputElement).checked });
    }

    function onAddEnvPair(): void {
        const nextPairs = [...props.item.envPairs, { key: '', value: '' }];
        emitPatch({ envPairs: nextPairs });
    }

    function onEnvKeyInput(envIdx: number, event: Event): void {
        const nextPairs = props.item.envPairs.map((pair, idx) =>
            idx === envIdx ? { ...pair, key: (event.target as HTMLInputElement).value } : pair
        );
        emitPatch({ envPairs: nextPairs });
    }

    function onEnvValueInput(envIdx: number, event: Event): void {
        const nextPairs = props.item.envPairs.map((pair, idx) =>
            idx === envIdx ? { ...pair, value: (event.target as HTMLInputElement).value } : pair
        );
        emitPatch({ envPairs: nextPairs });
    }

    function onRemoveEnvPair(envIdx: number): void {
        const nextPairs = props.item.envPairs.filter((_, idx) => idx !== envIdx);
        emitPatch({ envPairs: nextPairs });
    }

    function onRowKeydown(event: KeyboardEvent): void {
        if (!event.altKey) {
            return;
        }
        if (event.key === 'ArrowUp' && props.index > 0) {
            event.preventDefault();
            emit('move-up', props.index);
        } else if (event.key === 'ArrowDown' && props.index < props.total - 1) {
            event.preventDefault();
            emit('move-down', props.index);
        }
    }
</script>

<style lang="scss" scoped>
    .group-item-row {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
        padding: 0.75rem;
        border: 1px solid var(--background-700);
        border-radius: 0.375rem;
        background: var(--background-950);
    }

    .group-item-row__header {
        display: flex;
        align-items: flex-start;
        gap: 0.625rem;
    }

    .group-item-row__badge {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        min-width: 1.75rem;
        height: 1.75rem;
        margin-top: 1.125rem;
        border-radius: 0.25rem;
        background: var(--background-800);
        color: var(--max-primary-300);
        font-family: var(--mt-font-mono);
        font-size: 0.75rem;
        font-weight: 600;
    }

    .group-item-row__main-fields {
        flex: 1;
        display: grid;
        grid-template-columns: 1fr 1.6fr;
        gap: 0.625rem;
        min-width: 0;
    }

    .group-item-row__field {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
        min-width: 0;
    }

    .group-item-row__label {
        font-size: 0.7188rem;
        font-weight: 600;
        color: var(--background-300);
    }

    .group-item-row__input {
        width: 100%;
        padding: 0.375rem 0.5rem;
        border: 1px solid var(--background-700);
        border-radius: 0.25rem;
        background: var(--background-900);
        color: var(--background-50);
        font-size: 0.8125rem;
        outline: none;

        &--mono {
            font-family: var(--mt-font-mono);
        }

        &:focus {
            border-color: var(--max-primary-500);
        }
    }

    .group-item-row__error {
        font-size: 0.7188rem;
        color: var(--max-danger-400);
    }

    .group-item-row__actions {
        display: flex;
        align-items: center;
        gap: 0.25rem;
        margin-top: 1.125rem;
    }

    .group-item-row__icon-btn {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 1.75rem;
        height: 1.75rem;
        border: 1px solid var(--background-700);
        border-radius: 0.25rem;
        background: var(--background-800);
        color: var(--background-200);
        cursor: pointer;

        &:disabled {
            opacity: 0.4;
            cursor: not-allowed;
        }

        &--danger {
            color: var(--max-danger-400);
        }
    }

    .group-item-row__sub {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 0.75rem;
        padding-left: 2.375rem;
    }

    .group-item-row__field--cwd {
        width: 15rem;
    }

    .group-item-row__checkbox {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        font-size: 0.7813rem;
        color: var(--background-200);
        cursor: pointer;
    }

    .group-item-row__env-toggle {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        padding: 0.25rem 0.5rem;
        border: 1px solid var(--background-700);
        border-radius: 0.25rem;
        background: var(--background-800);
        color: var(--background-200);
        font-size: 0.75rem;
        cursor: pointer;
    }

    .group-item-row__env-list {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
        padding-left: 2.375rem;
    }

    .group-item-row__env-hint {
        margin: 0;
        font-size: 0.7188rem;
        color: var(--background-400);
    }

    .group-item-row__env-row {
        display: grid;
        grid-template-columns: 1fr 1.4fr auto auto;
        gap: 0.375rem;
        align-items: center;
    }
</style>
