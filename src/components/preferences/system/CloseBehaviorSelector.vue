<template>
    <div class="close-behavior-selector" role="radiogroup" aria-label="Comportamento ao fechar a janela">
        <div v-if="!props.trayAvailable" class="close-behavior-selector__warning" role="status">
            Bandeja do sistema (StatusNotifierWatcher) indisponível na sessão atual. Enquanto indisponível, fechar a janela encerrará ou minimizará sem ocultar na bandeja.
        </div>
        <label v-for="option in options" :key="option.value" class="close-behavior-selector__option" :class="{ 'is-selected': props.modelValue === option.value }">
            <input type="radio" name="closeBehavior" class="close-behavior-selector__radio" :value="option.value" :checked="props.modelValue === option.value" :disabled="props.disabled" @change="emit('update:modelValue', option.value)">
            <div class="close-behavior-selector__text">
                <span class="close-behavior-selector__title">{{ option.title }}</span>
                <span class="close-behavior-selector__desc">{{ option.description }}</span>
            </div>
        </label>
    </div>
</template>

<script setup lang="ts">
    import type { CloseBehavior } from '@/types/bindings';

    export interface CloseBehaviorSelectorProps {
        modelValue: CloseBehavior;
        trayAvailable?: boolean;
        disabled?: boolean;
    }

    const props = withDefaults(defineProps<CloseBehaviorSelectorProps>(), {
        trayAvailable: true,
        disabled: false
    });

    const emit = defineEmits<{
        'update:modelValue': [value: CloseBehavior];
    }>();

    interface CloseOption {
        value: CloseBehavior;
        title: string;
        description: string;
    }

    const options: CloseOption[] = [
        {
            value: 'auto',
            title: 'Minimizar para a bandeja quando houver abas em execução (Recomendado)',
            description: 'Mantém PTYs e servidores vivos na bandeja se houver abas em estado Executando ou Execução Contínua; caso contrário, encerra.'
        },
        {
            value: 'always_tray',
            title: 'Sempre minimizar para a bandeja ao fechar',
            description: 'O botão fechar apenas oculta a janela na bandeja do sistema, preservando todas as abas abertas.'
        },
        {
            value: 'always_quit',
            title: 'Sempre encerrar o MaxTerminal',
            description: 'Encerra todos os PTYs supervisionados e fecha completamente a aplicação.'
        }
    ];
</script>

<style lang="scss" scoped>
    .close-behavior-selector {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
    }

    .close-behavior-selector__warning {
        padding: 0.5rem 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--max-warning-500);
        background: color-mix(in srgb, var(--max-warning-500) 12%, transparent);
        color: var(--max-warning-400);
        font-size: 0.75rem;
    }

    .close-behavior-selector__option {
        display: flex;
        align-items: flex-start;
        gap: 0.625rem;
        padding: 0.625rem 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-700);
        background: var(--background-800);
        cursor: pointer;

        &.is-selected {
            border-color: var(--max-primary-500);
            background: color-mix(in srgb, var(--max-primary-500) 10%, var(--background-800));
        }
    }

    .close-behavior-selector__radio {
        margin-top: 0.1875rem;
        accent-color: var(--max-primary-500);
    }

    .close-behavior-selector__text {
        display: flex;
        flex-direction: column;
        gap: 0.125rem;
    }

    .close-behavior-selector__title {
        font-size: 0.8125rem;
        font-weight: 600;
        color: var(--background-100);
    }

    .close-behavior-selector__desc {
        font-size: 0.75rem;
        color: var(--background-400);
    }
</style>
