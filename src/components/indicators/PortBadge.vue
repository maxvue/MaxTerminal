<template>
    <div v-if="props.ports.length > 0" class="port-badge" @click.stop>
        <div v-for="item in visiblePorts" :key="item.id" class="port-badge__item">
            <MaxButton type="button" class="port-badge__open-btn" :title="`Abrir no navegador (${item.browserUrl}) — ${item.processName} (PID ${item.pid})`" @click.stop="emit('open-browser', item)">
                <span class="port-badge__dot" />
                <span class="port-badge__number">:{{ item.port }}</span>
            </MaxButton>
            <MaxIconButton icon="lucide:square" :size="0.625" :disabled="Boolean(props.killingByPortId?.[item.id])" :aria-label="`Encerrar processo da porta :${item.port}`" :title="`Encerrar processo ${item.processName} (PID ${item.pid})`" class="port-badge__kill-btn" @click.stop="requestKill(item)" />
        </div>
        <MaxButton v-if="overflowCount > 0" type="button" class="port-badge__overflow" :title="`Mais ${overflowCount} porta(s) ativa(s)`" @click.stop="popoverOpen = !popoverOpen">
            <span>+{{ overflowCount }}</span>
        </MaxButton>
        <div v-if="confirmingPort" class="port-badge__confirm" role="alertdialog" aria-label="Confirmar encerramento da porta">
            <span class="port-badge__confirm-text">Encerrar {{ confirmingPort.processName }} (PID {{ confirmingPort.pid }}) na porta :{{ confirmingPort.port }}?</span>
            <div class="port-badge__confirm-actions">
                <MaxButton type="button" size="sm" severity="danger" class="port-badge__confirm-yes" @click.stop="confirmKill">Encerrar processo da porta</MaxButton>
                <MaxButton type="button" size="sm" severity="secondary" class="port-badge__confirm-no" @click.stop="confirmingPort = null">Cancelar</MaxButton>
            </div>
        </div>
        <div v-if="popoverOpen && overflowCount > 0" class="port-badge__list-popover" role="dialog" aria-label="Todas as portas ativas">
            <div v-for="item in props.ports" :key="item.id" class="port-badge__popover-row">
                <MaxButton type="button" class="port-badge__popover-open" @click.stop="emit('open-browser', item)">
                    <strong>:{{ item.port }}</strong>
                    <span>{{ item.processName }} (PID {{ item.pid }})</span>
                </MaxButton>
                <MaxButton type="button" size="sm" severity="danger" @click.stop="requestKill(item)">Encerrar</MaxButton>
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed, ref } from 'vue';
    import { MaxButton, MaxIconButton } from '@maxvue/max-components-ui';
    import type { ListeningPort_Serialize, PortId } from '@/types/bindings';

    export interface PortBadgeProps {
        ports: ListeningPort_Serialize[];
        killingByPortId?: Record<PortId, boolean>;
        maxInline?: number;
    }

    const props = withDefaults(defineProps<PortBadgeProps>(), {
        killingByPortId: () => ({}),
        maxInline: 2
    });

    const emit = defineEmits<{
        'open-browser': [port: ListeningPort_Serialize];
        'kill-port': [port: ListeningPort_Serialize];
    }>();

    const confirmingPort = ref<ListeningPort_Serialize | null>(null);
    const popoverOpen = ref<boolean>(false);

    const visiblePorts = computed<ListeningPort_Serialize[]>(() =>
        props.ports.slice(0, props.maxInline)
    );

    const overflowCount = computed<number>(() =>
        Math.max(0, props.ports.length - props.maxInline)
    );

    function requestKill(port: ListeningPort_Serialize): void {
        confirmingPort.value = port;
    }

    function confirmKill(): void {
        if (!confirmingPort.value) {
            return;
        }
        const target = confirmingPort.value;
        confirmingPort.value = null;
        emit('kill-port', target);
    }
</script>

<style lang="scss" scoped>
    .port-badge {
        position: relative;
        display: inline-flex;
        align-items: center;
        gap: 4px;

        &__item {
            display: inline-flex;
            align-items: center;
            background: color-mix(in srgb, var(--max-info-500) 12%, transparent);
            border: 1px solid color-mix(in srgb, var(--max-info-500) 40%, transparent);
            border-radius: 4px;
        }

        &__open-btn {
            display: inline-flex;
            align-items: center;
            gap: 4px;
            padding: 1px 5px;
            font-family: monospace;
            font-size: 0.6875rem;
            color: var(--max-info-500);
            background: transparent;
            border: none;
            cursor: pointer;
        }

        &__dot {
            width: 5px;
            height: 5px;
            background: var(--max-success-500);
            border-radius: 999px;
        }

        &__kill-btn {
            display: inline-flex;
            align-items: center;
            justify-content: center;
            padding: 1px 4px;
            font-size: 0.625rem;
            color: var(--background-400);
            background: transparent;
            border: none;
            border-left: 1px solid color-mix(in srgb, var(--max-info-500) 25%, transparent);
            cursor: pointer;

            &:disabled {
                opacity: 0.4;
                cursor: not-allowed;
            }

            &:hover:not(:disabled) {
                color: var(--max-danger-500);
            }
        }

        &__overflow {
            padding: 1px 5px;
            font-family: monospace;
            font-size: 0.6875rem;
            color: var(--max-info-400);
            background: var(--mt-ui-surface-elevated, var(--background-50));
            border: 1px solid var(--mt-ui-border, var(--background-100));
            border-radius: 4px;
            cursor: pointer;
        }

        &__confirm,
        &__list-popover {
            position: absolute;
            top: calc(100% + 4px);
            right: 0;
            z-index: 45;
            min-width: 240px;
            padding: 8px 10px;
            background: var(--mt-ui-surface-panel, var(--background-25));
            border: 1px solid var(--mt-ui-border, var(--background-100));
            border-radius: 6px;
        }

        &__confirm-text {
            display: block;
            margin-bottom: 6px;
            font-size: 0.6875rem;
            color: var(--mt-ui-text-strong, var(--background-950));
        }

        &__confirm-actions {
            display: flex;
            justify-content: flex-end;
            gap: 6px;
        }

        &__confirm-yes {
            padding: 2px 8px;
            font-size: 0.6875rem;
            color: var(--max-danger-content, #fff);
            background: var(--max-danger-500);
            border: none;
            border-radius: 4px;
            cursor: pointer;
        }

        &__confirm-no {
            padding: 2px 8px;
            font-size: 0.6875rem;
            color: var(--mt-ui-text-secondary, var(--background-600));
            background: var(--mt-ui-surface-elevated, var(--background-50));
            border: 1px solid var(--mt-ui-border, var(--background-100));
            border-radius: 4px;
            cursor: pointer;
        }

        &__popover-row {
            display: flex;
            align-items: center;
            justify-content: space-between;
            gap: 8px;
            padding: 3px 0;
            font-size: 0.6875rem;
        }

        &__popover-open {
            display: inline-flex;
            align-items: center;
            gap: 6px;
            color: var(--mt-ui-text, var(--background-800));
            background: transparent;
            border: none;
            cursor: pointer;
        }

        &__popover-kill {
            padding: 1px 6px;
            font-size: 0.625rem;
            color: var(--max-danger-500);
            background: transparent;
            border: 1px solid color-mix(in srgb, var(--max-danger-500) 40%, transparent);
            border-radius: 4px;
            cursor: pointer;
        }
    }
</style>
