/** Resultado de correspondência fuzzy com pontuação e índices destacados. */
export interface FuzzyMatchResult<T> {
    item: T;
    score: number;
    matchedIndices: number[];
}

const WORD_BOUNDARY_CHARS = new Set(['-', ':', '/', '_', ' ', '.']);

function normalizeForMatch(str: string): string {
    return str
        .normalize('NFD')
        .replace(/[\u0300-\u036f]/g, '')
        .toLowerCase();
}

/**
 * Calcula a pontuação de subsequência de `query` dentro de `text` (case-insensitive, ignorando acentos).
 * Retorna `null` se todos os caracteres de `query` não aparecerem em ordem em `text`.
 */
export function fuzzyScore(
    text: string,
    query: string
): { score: number; matchedIndices: number[] } | null {
    const cleanQuery = query.trim();
    if (cleanQuery.length === 0) {
        return { score: 0, matchedIndices: [] };
    }

    const normText = normalizeForMatch(text);
    const normQuery = normalizeForMatch(cleanQuery);

    if (normQuery.length > normText.length) {
        return null;
    }

    const matchedIndices: number[] = [];
    let score = 0;
    let textIdx = 0;
    let prevMatchIdx = -2;

    if (normText.startsWith(normQuery)) {
        score += 100;
    }

    for (let qIdx = 0; qIdx < normQuery.length; qIdx++) {
        const qChar = normQuery[qIdx]!;
        const foundIdx = normText.indexOf(qChar, textIdx);
        if (foundIdx === -1) {
            return null;
        }

        matchedIndices.push(foundIdx);
        score += 10;

        if (foundIdx === 0) {
            score += 35;
        } else if (WORD_BOUNDARY_CHARS.has(normText[foundIdx - 1]!)) {
            score += 25;
        }

        if (foundIdx === prevMatchIdx + 1) {
            score += 20;
        } else {
            score -= Math.min(10, foundIdx - (prevMatchIdx >= 0 ? prevMatchIdx : 0));
        }

        prevMatchIdx = foundIdx;
        textIdx = foundIdx + 1;
    }

    // Penaliza levemente candidatos muito longos para desempatar a favor do mais conciso.
    score -= Math.min(15, normText.length - normQuery.length);

    return {
        score,
        matchedIndices
    };
}

/**
 * Filtra e ordena uma lista de itens pela pontuação `fuzzyScore` (preservando ordem relativa em empate).
 */
export function fuzzyFilter<T>(
    items: readonly T[],
    query: string,
    key?: keyof T | ((item: T) => string)
): T[] {
    if (query.trim().length === 0) {
        return [...items];
    }

    const extractText = (item: T): string => {
        if (typeof key === 'function') {
            return key(item);
        }
        if (key !== undefined) {
            const val = item[key];
            return typeof val === 'string' ? val : String(val ?? '');
        }
        return typeof item === 'string' ? item : String(item);
    };

    const scored: Array<{ item: T; score: number; originalIndex: number }> = [];
    for (let i = 0; i < items.length; i++) {
        const candidate = items[i]!;
        const res = fuzzyScore(extractText(candidate), query);
        if (res !== null) {
            scored.push({
                item: candidate,
                score: res.score,
                originalIndex: i
            });
        }
    }

    scored.sort((a, b) => {
        if (b.score !== a.score) {
            return b.score - a.score;
        }
        return a.originalIndex - b.originalIndex;
    });

    return scored.map((entry) => entry.item);
}
