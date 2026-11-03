import { computed, toValue, type MaybeRefOrGetter } from 'vue';
import type { HighlightSegment } from '@/types/bindings';

/**
 * Normaliza e valida um array de `HighlightSegment`:
 * - descarta segmentos vazios ou malformados;
 * - mescla segmentos adjacentes com o mesmo valor booleano `matched`;
 * - garante que nunca retorne `undefined` ou HTML interpretável.
 */
export function normalizeHighlightSegments(
    segments: readonly HighlightSegment[] | null | undefined
): HighlightSegment[] {
    if (!Array.isArray(segments) || segments.length === 0) {
        return [];
    }

    const merged: HighlightSegment[] = [];
    for (const seg of segments) {
        if (!seg || typeof seg.text !== 'string' || seg.text.length === 0) {
            continue;
        }
        const matched = Boolean(seg.matched);
        const prev = merged[merged.length - 1];
        if (prev && prev.matched === matched) {
            prev.text += seg.text;
        } else {
            merged.push({
                text: seg.text,
                matched
            });
        }
    }

    return merged;
}

/** Composable reativo que devolve `safeSegments` normalizados a partir de `HighlightSegment[]`. */
export function useHighlightSegments(
    source: MaybeRefOrGetter<readonly HighlightSegment[] | null | undefined>
) {
    const safeSegments = computed<HighlightSegment[]>(() =>
        normalizeHighlightSegments(toValue(source))
    );

    return {
        safeSegments
    };
}
