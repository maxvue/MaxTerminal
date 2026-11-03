<template>
    <div v-if="report" class="import-report" role="status" aria-live="polite">
        <div class="import-report__header">
            <div class="import-report__summary">
                <MaxTag :value="`${report.imported.length} importado(s)`" severity="success" :no-uppercase="true" class="import-report__badge import-report__badge--success" />
                <MaxTag v-if="report.warnings.length > 0" :value="`${report.warnings.length} aviso(s)`" severity="warning" :no-uppercase="true" class="import-report__badge import-report__badge--warning" />
                <MaxTag v-if="report.errors.length > 0" :value="`${report.errors.length} falha(s)`" severity="danger" :no-uppercase="true" class="import-report__badge import-report__badge--danger" />
                <MaxTag v-if="report.skipped.length > 0" :value="`${report.skipped.length} ignorado(s)`" severity="secondary" :no-uppercase="true" class="import-report__badge" />
            </div>
            <MaxIconButton icon="lucide:x" size="small" variant="text" class="import-report__close" aria-label="Fechar relatório de importação" @click="emit('close')" />
        </div>

        <ul v-if="report.warnings.length > 0" class="import-report__list import-report__list--warning">
            <li v-for="(warn, idx) in report.warnings" :key="`w-${idx}`">
                <strong>{{ warn.schemeName }}:</strong> {{ warn.message }}
            </li>
        </ul>

        <ul v-if="report.errors.length > 0" class="import-report__list import-report__list--danger">
            <li v-for="(fail, idx) in report.errors" :key="`f-${idx}`">
                <strong>{{ fail.source }}:</strong> {{ fail.error.kind }}
            </li>
        </ul>
    </div>
</template>

<script setup lang="ts">
    import { MaxIconButton, MaxTag } from '@maxvue/max-components-ui';
    import type { ImportReport } from '@/types/theme';

    export interface ThemeImportReportProps {
        report: ImportReport | null;
    }

    defineProps<ThemeImportReportProps>();

    const emit = defineEmits<{
        close: [];
    }>();
</script>

<style lang="scss" scoped>
    .import-report {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
        padding: 0.75rem;
        border-radius: 0.5rem;
        border: 1px solid var(--background-700);
        background: var(--background-950);
        font-size: 0.75rem;
    }

    .import-report__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
    }

    .import-report__summary {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        flex-wrap: wrap;
    }

    .import-report__badge {
        padding: 0.125rem 0.5rem;
        border-radius: 0.25rem;
        background: var(--background-800);
        color: var(--background-200);
        font-weight: 600;

        &--success {
            background: color-mix(in srgb, var(--max-success-500) 20%, transparent);
            color: var(--max-success-400);
        }

        &--warning {
            background: color-mix(in srgb, var(--max-warning-500) 20%, transparent);
            color: var(--max-warning-400);
        }

        &--danger {
            background: color-mix(in srgb, var(--max-danger-500) 20%, transparent);
            color: var(--max-danger-400);
        }
    }

    .import-report__close {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 1.5rem;
        height: 1.5rem;
        border: none;
        border-radius: 0.25rem;
        background: transparent;
        color: var(--background-300);
        cursor: pointer;

        &:hover {
            background: var(--background-800);
            color: var(--background-50);
        }
    }

    .import-report__list {
        margin: 0;
        padding-left: 1.125rem;
        display: flex;
        flex-direction: column;
        gap: 0.25rem;

        &--warning {
            color: var(--max-warning-400);
        }

        &--danger {
            color: var(--max-danger-400);
        }
    }
</style>
