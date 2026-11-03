import type { CanonicalLabelGroup, CanonicalLabelSpec } from '@/types/bindings';

export const CANONICAL_GITHUB_LABELS: readonly CanonicalLabelSpec[] = Object.freeze([
    {
        name: 'Waiting Plan',
        group: 'state',
        order: 1,
        colorHex: 'D93F0B',
        description: 'Aguardando geração do plano de implementação pelo MaxCode.'
    },
    {
        name: 'Planning',
        group: 'state',
        order: 2,
        colorHex: 'FBCA04',
        description: 'Plano de implementação em elaboração.'
    },
    {
        name: 'Planned',
        group: 'state',
        order: 3,
        colorHex: '0E8A16',
        description: 'Plano de implementação concluído e pronto para execução.'
    },
    {
        name: 'Waiting Re-plan',
        group: 'state',
        order: 4,
        colorHex: 'E99695',
        description: 'Aguardando replanejamento após revisão.'
    },
    {
        name: 'Waiting Execute',
        group: 'state',
        order: 5,
        colorHex: '1D76DB',
        description: 'Na fila aguardando execução da implementação.'
    },
    {
        name: 'Executing',
        group: 'state',
        order: 6,
        colorHex: '0052CC',
        description: 'Implementação em andamento por um agente.'
    },
    {
        name: 'Executed',
        group: 'state',
        order: 7,
        colorHex: '5319E7',
        description: 'Implementação concluída e pronta para verificação.'
    },
    {
        name: 'Waiting Check',
        group: 'state',
        order: 8,
        colorHex: 'C5DEF5',
        description: 'Aguardando etapa de verificação automatizada/revisão.'
    },
    {
        name: 'Checking',
        group: 'state',
        order: 9,
        colorHex: 'BFD4F2',
        description: 'Em verificação pelo verificador do MaxCode.'
    },
    {
        name: 'Checked Approved',
        group: 'state',
        order: 10,
        colorHex: '2EA44F',
        description: 'Verificação aprovada com sucesso.'
    },
    {
        name: 'Checked Rejected',
        group: 'state',
        order: 11,
        colorHex: 'B60205',
        description: 'Verificação reprovada; requer correção adicional.'
    },
    {
        name: 'Waiting Merge',
        group: 'state',
        order: 12,
        colorHex: '6F42C1',
        description: 'Aguardando integração (merge) na branch principal.'
    },
    {
        name: 'Fix Merged',
        group: 'state',
        order: 13,
        colorHex: '28A745',
        description: 'Correção integrada na branch principal.'
    },
    {
        name: 'Waiting More Info',
        group: 'state',
        order: 14,
        colorHex: 'D876E3',
        description: 'Bloqueada aguardando mais informações do autor.'
    },
    {
        name: 'Blocked',
        group: 'state',
        order: 15,
        colorHex: '7057FF',
        description: 'Bloqueada por dependência externa.'
    },
    {
        name: 'Duplicated',
        group: 'closure',
        order: 16,
        colorHex: 'CFD3D7',
        description: 'Encerrada por ser duplicata de outra issue.'
    },
    {
        name: "Won't Fix",
        group: 'closure',
        order: 17,
        colorHex: 'FFFFFF',
        description: 'Encerrada sem implementação prevista.'
    },
    {
        name: 'Invalid',
        group: 'closure',
        order: 18,
        colorHex: 'E4E669',
        description: 'Encerrada por relato inválido ou irreproduzível.'
    },
    {
        name: 'Regra de Negocio',
        group: 'category',
        order: 19,
        colorHex: '006B75',
        description: 'Categoria: regra de negócio ou domínio.'
    },
    {
        name: 'Layout / UI',
        group: 'category',
        order: 20,
        colorHex: 'C2E0C6',
        description: 'Categoria: interface visual, layout ou estilos.'
    },
    {
        name: 'Performance',
        group: 'category',
        order: 21,
        colorHex: 'F9D0C4',
        description: 'Categoria: desempenho, latência ou uso de recursos.'
    },
    {
        name: 'Seguranca',
        group: 'category',
        order: 22,
        colorHex: 'EE0701',
        description: 'Categoria: segurança, permissões ou sanitização.'
    },
    {
        name: 'Banco de Dados',
        group: 'category',
        order: 23,
        colorHex: 'FEF2C0',
        description: 'Categoria: SQLite, migrations ou consultas.'
    },
    {
        name: 'Integracao',
        group: 'category',
        order: 24,
        colorHex: 'BFDADC',
        description: 'Categoria: integração com serviços externos, IPC ou CLI.'
    }
]);

export const STATE_LABELS: readonly CanonicalLabelSpec[] = Object.freeze(
    CANONICAL_GITHUB_LABELS.filter((l) => l.group === 'state')
);

export const CLOSURE_LABELS: readonly CanonicalLabelSpec[] = Object.freeze(
    CANONICAL_GITHUB_LABELS.filter((l) => l.group === 'closure')
);

export const CATEGORY_LABELS: readonly CanonicalLabelSpec[] = Object.freeze(
    CANONICAL_GITHUB_LABELS.filter((l) => l.group === 'category')
);

export const CANONICAL_LABELS = CANONICAL_GITHUB_LABELS;
export const CANONICAL_STATE_LABELS = STATE_LABELS;
export const CANONICAL_CLOSURE_LABELS = CLOSURE_LABELS;
export const CANONICAL_CATEGORY_LABELS = CATEGORY_LABELS;

const BY_LOWER = new Map<string, CanonicalLabelSpec>(
    CANONICAL_GITHUB_LABELS.map((spec) => [spec.name.toLowerCase(), spec])
);

export function findCanonicalLabel(name: string): CanonicalLabelSpec | null {
    return BY_LOWER.get(name.trim().toLowerCase()) ?? null;
}

export const findCanonicalSpec = findCanonicalLabel;

export function classifyCanonicalLabelGroup(name: string): CanonicalLabelGroup | null {
    return findCanonicalLabel(name)?.group ?? null;
}

export function sortRepoLabelsCanonically<T extends { name: string }>(labels: readonly T[]): T[] {
    return [...labels].sort((a, b) => {
        const specA = findCanonicalLabel(a.name);
        const specB = findCanonicalLabel(b.name);
        if (specA && specB) {
            return specA.order - specB.order;
        }
        if (specA) {
            return -1;
        }
        if (specB) {
            return 1;
        }
        return a.name.toLowerCase().localeCompare(b.name.toLowerCase());
    });
}

export const sortLabelsCanonically = sortRepoLabelsCanonically;

export function normalizeHexColor(colorHex: string): string {
    const clean = colorHex.replace(/^#/, '').trim();
    return /^[0-9a-fA-F]{6}$/.test(clean) ? clean.toLowerCase() : '64748b';
}

export function labelContrastTextColor(colorHex: string): string {
    const clean = colorHex.replace(/^#/, '').trim();
    if (clean.length !== 6) {
        return '#f8fafc';
    }
    const r = Number.parseInt(clean.slice(0, 2), 16);
    const g = Number.parseInt(clean.slice(2, 4), 16);
    const b = Number.parseInt(clean.slice(4, 6), 16);
    if (Number.isNaN(r) || Number.isNaN(g) || Number.isNaN(b)) {
        return '#f8fafc';
    }
    // Luminância relativa (YIQ) para garantir contraste legível em qualquer cor hexadecimal.
    const yiq = (r * 299 + g * 587 + b * 114) / 1000;
    return yiq >= 150 ? '#0f172a' : '#f8fafc';
}

export const computeContrastTextColor = labelContrastTextColor;
