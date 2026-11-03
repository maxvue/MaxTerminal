import type { StackKind } from '@/types/bindings';

/** Identidade visual fixa associada a cada stack detectada (§2.5 da spec). */
export interface StackVisualIdentity {
    icon: string;
    color: string;
    label: string;
}

/** Mapa canônico de identidade visual por stack (espelho de `mt-projects/src/identity.rs`). */
export const STACK_IDENTITY_MAP: Readonly<Record<Exclude<StackKind, 'unknown'>, StackVisualIdentity>> = Object.freeze({
    tauri: {
        icon: 'logos:tauri',
        color: '#24C8DB',
        label: 'Tauri'
    },
    vscode_extension: {
        icon: 'vscode-icons:file-type-vscode',
        color: '#007ACC',
        label: 'Extensão VS Code'
    },
    laravel: {
        icon: 'logos:laravel',
        color: '#FF2D20',
        label: 'Laravel'
    },
    vue: {
        icon: 'logos:vue',
        color: '#42B883',
        label: 'Vue'
    },
    rust: {
        icon: 'logos:rust',
        color: '#DEA584',
        label: 'Rust'
    },
    python: {
        icon: 'logos:python',
        color: '#3776AB',
        label: 'Python'
    }
});

/** Rótulos amigáveis em pt-BR para todas as variantes de `StackKind`. */
export const STACK_LABELS_PT_BR: Readonly<Record<StackKind, string>> = Object.freeze({
    tauri: 'Tauri',
    vscode_extension: 'Extensão VS Code',
    laravel: 'Laravel',
    vue: 'Vue',
    rust: 'Rust',
    python: 'Python',
    unknown: 'Genérico'
});

/** Paleta determinística de ícones de fallback para `StackKind = 'unknown'`. */
export const FALLBACK_ICONS: readonly string[] = Object.freeze([
    'mdi:folder-code-outline',
    'mdi:terminal-box-outline',
    'mdi:cube-outline',
    'mdi:RocketLaunchOutline',
    'mdi:layers-triple-outline',
    'mdi:source-branch',
    'mdi:console-line',
    'mdi:hexagon-multiple-outline',
    'mdi:code-braces-box',
    'mdi:compass-outline',
    'mdi:puzzle-outline',
    'mdi:lightning-bolt-outline'
]);

/** Paleta determinística de cores de fallback para `StackKind = 'unknown'`. */
export const FALLBACK_COLORS: readonly string[] = Object.freeze([
    '#14B8A6',
    '#0EA5E9',
    '#6366F1',
    '#8B5CF6',
    '#EC4899',
    '#F43F5E',
    '#F59E0B',
    '#10B981',
    '#06B6D4',
    '#3B82F6',
    '#A855F7',
    '#84CC16'
]);

/** Lista curada de ícones sugeridos no modal de criação/edição de projeto. */
export const PROJECT_ICON_SUGGESTIONS: readonly string[] = Object.freeze([
    'logos:laravel',
    'logos:vue',
    'logos:tauri',
    'vscode-icons:file-type-vscode',
    'logos:rust',
    'logos:python',
    ...FALLBACK_ICONS
]);

/** Paleta de cores sugeridas no seletor de criação/edição de projeto. */
export const PROJECT_COLOR_SUGGESTIONS: readonly string[] = Object.freeze([
    '#24C8DB',
    '#007ACC',
    '#FF2D20',
    '#42B883',
    '#DEA584',
    '#3776AB',
    ...FALLBACK_COLORS
]);

const ICON_REGEX = /^[a-z0-9-]+:[a-zA-Z0-9-]+$/;
const COLOR_REGEX = /^#[0-9A-Fa-f]{6}$/;

/** Valida client-side se o identificador Iconify segue `^[a-z0-9-]+:[a-zA-Z0-9-]+$` (máx. 80 chars). */
export function isValidProjectIcon(icon: string): boolean {
    const trimmed = icon.trim();
    return trimmed.length > 0 && trimmed.length <= 80 && ICON_REGEX.test(trimmed);
}

/** Valida client-side se a cor segue `^#[0-9A-Fa-f]{6}$`. */
export function isValidProjectColor(color: string): boolean {
    return COLOR_REGEX.test(color.trim());
}

/** Normaliza uma cor hexadecimal válida para caixa alta (`#RRGGBB`). */
export function normalizeProjectColor(color: string): string {
    return color.trim().toUpperCase();
}

/** Valida client-side se o título pós-trim possui `1..=120` codepoints e nenhum caractere de controle. */
export function isValidProjectTitle(title: string): boolean {
    const trimmed = title.trim();
    const codepoints = Array.from(trimmed);
    if (codepoints.length === 0 || codepoints.length > 120) {
        return false;
    }
    for (const ch of codepoints) {
        const code = ch.codePointAt(0) ?? 0;
        if (code <= 0x1f || (code >= 0x7f && code <= 0x9f)) {
            return false;
        }
    }
    return true;
}
