<template>
    <div v-if="hasAnyVisibleBadge" class="project-status-badges" :class="{ 'project-status-badges--compact': props.compact }" role="group" aria-label="Status das abas do projeto">
        <span v-for="badge in visibleNativeBadges" :key="badge.key" class="project-status-badges__pill" :data-badge-key="badge.key" :style="{ '--mt-badge-color': `var(${badge.colorVar})` }" :title="badge.tooltip" :aria-label="badge.ariaLabel">
            <span class="project-status-badges__dot" aria-hidden="true" />
            <span class="project-status-badges__count">{{ badge.count }}</span>
        </span>
        <span v-for="ext in visibleProviderBadges" :key="ext.id" class="project-status-badges__pill project-status-badges__pill--provider" :data-badge-key="ext.id" :style="{ '--mt-badge-color': `var(${ext.colorVar})` }" :title="ext.tooltip" :aria-label="ext.tooltip">
            <MaxIcon :icon="ext.icon" size="0.75" class="project-status-badges__icon" />
            <span class="project-status-badges__count">{{ ext.count }}</span>
        </span>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { evaluateBadgeProviderCount, useProjectBadgeRegistry } from '@/composables/useProjectBadgeRegistry';
    import type { ProjectStatusCounters } from '@/stores/projects';
    import type { ProjectId } from '@/types/bindings';

    export interface ProjectStatusBadgesProps {
        projectId: ProjectId;
        counters: ProjectStatusCounters;
        compact?: boolean;
    }

    interface NativeBadgeDescriptor {
        key: keyof ProjectStatusCounters;
        order: number;
        label: string;
        colorVar: string;
        count: number;
        tooltip: string;
        ariaLabel: string;
    }

    interface ProviderBadgeDescriptor {
        id: string;
        order: number;
        icon: string;
        colorVar: string;
        count: number;
        tooltip: string;
    }

    const props = withDefaults(defineProps<ProjectStatusBadgesProps>(), {
        compact: false
    });

    const registry = useProjectBadgeRegistry();

    function formatAriaLabel(key: keyof ProjectStatusCounters, count: number): string {
        const plural = count === 1 ? 'aba' : 'abas';
        switch (key) {
            case 'running':
                return `${count} ${plural} executando`;
            case 'continuous':
                return `${count} ${plural} em execução contínua`;
            case 'finished':
                return `${count} ${plural} ${count === 1 ? 'finalizada' : 'finalizadas'}`;
            case 'error':
                return `${count} ${plural} com erro`;
            case 'idle':
                return `${count} ${plural} ${count === 1 ? 'parada' : 'paradas'}`;
        }
    }

    const visibleNativeBadges = computed<NativeBadgeDescriptor[]>(() => {
        const defs: Array<{ key: keyof ProjectStatusCounters; order: number; label: string; colorVar: string }> = [
            { key: 'running', order: 10, label: 'Executando', colorVar: '--max-success-500' },
            { key: 'continuous', order: 20, label: 'Execução contínua', colorVar: '--max-info-500' },
            { key: 'finished', order: 30, label: 'Finalizado', colorVar: '--max-warning-500' },
            { key: 'error', order: 40, label: 'Com erro', colorVar: '--max-danger-500' },
            { key: 'idle', order: 50, label: 'Parado', colorVar: '--background-500' }
        ];

        const out: NativeBadgeDescriptor[] = [];
        for (const def of defs) {
            const raw = props.counters[def.key];
            if (typeof raw === 'number' && Number.isFinite(raw) && raw > 0) {
                const count = Math.floor(raw);
                if (count > 0) {
                    const ariaLabel = formatAriaLabel(def.key, count);
                    out.push({
                        key: def.key,
                        order: def.order,
                        label: def.label,
                        colorVar: def.colorVar,
                        count,
                        tooltip: `${def.label}: ${ariaLabel}`,
                        ariaLabel
                    });
                }
            }
        }
        return out;
    });

    const visibleProviderBadges = computed<ProviderBadgeDescriptor[]>(() => {
        const out: ProviderBadgeDescriptor[] = [];
        for (const provider of registry.providers.value) {
            const count = evaluateBadgeProviderCount(provider, props.projectId);
            if (count !== null && count > 0) {
                out.push({
                    id: provider.id,
                    order: provider.order,
                    icon: provider.icon,
                    colorVar: provider.colorVar,
                    count,
                    tooltip: provider.tooltip(count)
                });
            }
        }
        return out;
    });

    const hasAnyVisibleBadge = computed<boolean>(
        () => visibleNativeBadges.value.length > 0 || visibleProviderBadges.value.length > 0
    );
</script>

<style lang="scss" scoped>
    .project-status-badges {
        display: inline-flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.25rem;
    }

    .project-status-badges__pill {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        padding: 0.0625rem 0.4375rem;
        border-radius: 9999px;
        font-size: 0.6875rem;
        line-height: 1.25;
        font-weight: 600;
        font-variant-numeric: tabular-nums;
        background: var(--background-800);
        color: var(--background-100);
        border: 1px solid var(--mt-badge-color, var(--background-600));
    }

    .project-status-badges--compact .project-status-badges__pill {
        padding: 0.0625rem 0.35rem;
        font-size: 0.625rem;
    }

    .project-status-badges__dot {
        width: 0.4375rem;
        height: 0.4375rem;
        border-radius: 9999px;
        background: var(--mt-badge-color, var(--background-400));
        flex-shrink: 0;
    }

    .project-status-badges__icon {
        color: var(--mt-badge-color, var(--max-primary-400));
        flex-shrink: 0;
    }

    .project-status-badges__count {
        font-variant-numeric: tabular-nums;
    }
</style>
