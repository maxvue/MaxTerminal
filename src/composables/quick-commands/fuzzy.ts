import type { ArtisanCommand, ArtisanOption } from '@/types/bindings';

export interface FuzzyMatchResult {
    score: number;
    indices: number[];
}

const BOUNDARY_CHARS = new Set([':', '-', '_', '.', ' ', '/']);

function isBoundary(target: string, index: number): boolean {
    if (index === 0) {
        return true;
    }
    const prev = target[index - 1];
    return prev !== undefined && BOUNDARY_CHARS.has(prev);
}

/**
 * Executa o casamento fuzzy de `query` contra `target` retornando score e índices destacados.
 * Complexidade O(n * m) com preferência por prefixo e fronteiras de namespace (`:`, `-`, `_`).
 */
export function fuzzyMatch(query: string, target: string): FuzzyMatchResult | null {
    const qRaw = query.trim().toLowerCase();
    if (qRaw.length === 0) {
        return { score: 0, indices: [] };
    }

    const tRaw = target.toLowerCase();
    if (qRaw.length > tRaw.length) {
        return null;
    }

    if (tRaw === qRaw) {
        const indices = Array.from({ length: qRaw.length }, (_, i) => i);
        return { score: 10_000, indices };
    }

    if (tRaw.startsWith(qRaw)) {
        const indices = Array.from({ length: qRaw.length }, (_, i) => i);
        return {
            score: 5_000 - (tRaw.length - qRaw.length),
            indices
        };
    }

    // Procura o melhor caminho favorecendo fronteiras de segmento (`:`, `-`, `_`)
    const indices: number[] = [];
    let score = 0;
    let ti = 0;
    let lastMatch = -1;

    for (let qi = 0; qi < qRaw.length; qi += 1) {
        const qc = qRaw[qi];
        let chosen = -1;

        // Tenta primeiro casar consecutivamente se o caractere anterior acabou de casar em fronteira de namespace (`:`)
        if (
            lastMatch !== -1 &&
            ti === lastMatch + 1 &&
            tRaw[ti] === qc &&
            canMatchRemainder(qRaw, qi + 1, tRaw, ti + 1)
        ) {
            chosen = ti;
        }

        // Senão tenta casar em uma fronteira de palavra/namespace (`:`, `-`, `_`)
        if (chosen === -1) {
            for (let k = ti; k < tRaw.length; k += 1) {
                if (tRaw[k] === qc && isBoundary(tRaw, k)) {
                    chosen = k;
                    break;
                }
            }
        }

        // Caso não haja fronteira disponível que permita completar o restante da query, usa o primeiro match
        const remainingQuery = qRaw.length - qi - 1;
        if (chosen !== -1 && !canMatchRemainder(qRaw, qi + 1, tRaw, chosen + 1)) {
            chosen = -1;
        }

        if (chosen === -1) {
            for (let k = ti; k <= tRaw.length - 1 - remainingQuery; k += 1) {
                if (tRaw[k] === qc) {
                    chosen = k;
                    break;
                }
            }
        }

        if (chosen === -1) {
            return null;
        }

        score += 10;
        if (chosen === 0 || tRaw[chosen - 1] === ':') {
            score += 20;
        } else if (isBoundary(tRaw, chosen)) {
            score += 12;
        }
        if (lastMatch !== -1 && chosen === lastMatch + 1) {
            score += 18;
        }
        const gap = chosen - ti;
        if (gap > 0) {
            score -= Math.min(gap, 5);
        }

        indices.push(chosen);
        lastMatch = chosen;
        ti = chosen + 1;
    }

    return { score, indices };
}

function canMatchRemainder(q: string, qStart: number, t: string, tStart: number): boolean {
    let ti = tStart;
    for (let qi = qStart; qi < q.length; qi += 1) {
        const idx = t.indexOf(q[qi] ?? '', ti);
        if (idx === -1) {
            return false;
        }
        ti = idx + 1;
    }
    return true;
}

/** Retorna apenas a pontuação numérica do match fuzzy ou `null` quando não há correspondência. */
export function fuzzyScore(query: string, target: string): number | null {
    const res = fuzzyMatch(query, target);
    return res ? res.score : null;
}

/**
 * Ordena comandos Artisan segundo a consulta `query` (CA-12).
 * Quando `query` é vazia, preserva a ordem alfabética por `name`.
 */
export function rankCommands(
    query: string,
    commands: readonly ArtisanCommand[]
): ArtisanCommand[] {
    const q = query.trim();
    if (q.length === 0) {
        return [...commands].sort((a, b) => a.name.localeCompare(b.name));
    }

    const scored: { cmd: ArtisanCommand; score: number }[] = [];
    for (const cmd of commands) {
        let best = fuzzyScore(q, cmd.name);
        if (cmd.aliases && cmd.aliases.length > 0) {
            for (const alias of cmd.aliases) {
                const aliasScore = fuzzyScore(q, alias);
                if (aliasScore !== null) {
                    const penalized = aliasScore - 100;
                    if (best === null || penalized > best) {
                        best = penalized;
                    }
                }
            }
        }
        if (best !== null) {
            scored.push({ cmd, score: best });
        }
    }

    scored.sort((a, b) => {
        if (b.score !== a.score) {
            return b.score - a.score;
        }
        if (a.cmd.name.length !== b.cmd.name.length) {
            return a.cmd.name.length - b.cmd.name.length;
        }
        return a.cmd.name.localeCompare(b.cmd.name);
    });

    return scored.map((entry) => entry.cmd);
}

/**
 * Ordena opções (`--flags`) segundo a consulta `query`.
 * Remove prefixos `-` / `--` da consulta para comparar tanto o nome completo quanto sem `--`.
 */
export function rankOptions(
    query: string,
    options: readonly ArtisanOption[]
): ArtisanOption[] {
    const clean = query.trim().replace(/^-+/, '');
    if (clean.length === 0) {
        return [...options].sort((a, b) => a.name.localeCompare(b.name));
    }

    const scored: { opt: ArtisanOption; score: number }[] = [];
    for (const opt of options) {
        const bareName = opt.name.replace(/^--/, '');
        let best = fuzzyScore(clean, bareName);
        const fullScore = fuzzyScore(query.trim(), opt.name);
        if (fullScore !== null && (best === null || fullScore > best)) {
            best = fullScore;
        }
        if (opt.shortcut) {
            const scScore = fuzzyScore(clean, opt.shortcut.replace(/^-+/, ''));
            if (scScore !== null) {
                const penalized = scScore - 50;
                if (best === null || penalized > best) {
                    best = penalized;
                }
            }
        }
        if (best !== null) {
            scored.push({ opt, score: best });
        }
    }

    scored.sort((a, b) => {
        if (b.score !== a.score) {
            return b.score - a.score;
        }
        return a.opt.name.localeCompare(b.opt.name);
    });

    return scored.map((s) => s.opt);
}
