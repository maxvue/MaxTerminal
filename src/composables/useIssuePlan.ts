import { computed, type Ref } from 'vue';
import type { ExtractedPlan, IssueDetail } from '@/types/bindings';
import { renderSafeMarkdown } from './useMarkdownRenderer';

const PROVISIONAL_STATES = new Set(['waiting plan', 'waiting re-plan', 'checked rejected']);

/**
 * Composable de leitura e formatação da seção `## PLANO DE IMPLEMENTAÇÃO` de uma issue.
 */
export function useIssuePlan(detail: Ref<IssueDetail | null>) {
    const plan = computed<ExtractedPlan | null>(() => {
        const d = detail.value as (IssueDetail & { extractedPlan?: ExtractedPlan | null }) | null;
        if (!d) {
            return null;
        }
        return d.plan ?? d.extractedPlan ?? null;
    });

    const hasPlan = computed<boolean>(() => Boolean(plan.value && plan.value.markdown.trim()));

    const isProvisional = computed<boolean>(() => {
        const labels = detail.value?.summary.labels ?? [];
        return labels.some((l) => PROVISIONAL_STATES.has(l.name.trim().toLowerCase()));
    });

    const authorLogin = computed<string>(() => plan.value?.authorLogin ?? 'autor');

    const headingTitle = computed<string>(() => {
        const p = plan.value as (ExtractedPlan & { headingRaw?: string | null }) | null;
        const raw = p?.headingRaw ?? 'PLANO DE IMPLEMENTAÇÃO';
        return raw.replace(/^#+\s*/, '').trim() || 'PLANO DE IMPLEMENTAÇÃO';
    });

    const sourceLabel = computed<string>(() => {
        if (!plan.value) {
            return '';
        }
        return plan.value.source === 'comment' ? 'Comentário' : 'Descrição';
    });

    const renderedPlanHtml = computed<string>(() => {
        if (!plan.value) {
            return '';
        }
        return renderSafeMarkdown(plan.value.markdown);
    });

    return {
        plan,
        hasPlan,
        isProvisional,
        authorLogin,
        headingTitle,
        sourceLabel,
        planHtml: renderedPlanHtml,
        renderedPlanHtml
    };
}
