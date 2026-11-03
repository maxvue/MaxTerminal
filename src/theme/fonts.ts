/** Presets recomendados de fontes monoespaçadas para desenvolvimento no Linux / CachyOS. */
export interface MonospaceFontPreset {
    id: string;
    label: string;
    family: string;
    supportsLigatures: boolean;
}

export const MONOSPACE_FONT_PRESETS: readonly MonospaceFontPreset[] = [
    {
        id: 'jetbrains-mono',
        label: 'JetBrains Mono (Padrão)',
        family: 'JetBrains Mono, Fira Code, Cascadia Code, monospace',
        supportsLigatures: true
    },
    {
        id: 'fira-code',
        label: 'Fira Code',
        family: 'Fira Code, JetBrains Mono, monospace',
        supportsLigatures: true
    },
    {
        id: 'cascadia-code',
        label: 'Cascadia Code',
        family: 'Cascadia Code, JetBrains Mono, monospace',
        supportsLigatures: true
    },
    {
        id: 'iosevka',
        label: 'Iosevka',
        family: 'Iosevka, JetBrains Mono, monospace',
        supportsLigatures: true
    },
    {
        id: 'hack',
        label: 'Hack',
        family: 'Hack, DejaVu Sans Mono, monospace',
        supportsLigatures: false
    },
    {
        id: 'system-mono',
        label: 'Monospace do Sistema',
        family: 'monospace',
        supportsLigatures: false
    }
] as const;

const SAFE_FONT_FAMILY_RE = /^[A-Za-z0-9 ,\-_'"]{1,200}$/;

/**
 * Valida se uma pilha CSS `font-family` contém apenas caracteres seguros.
 */
export function isValidFontFamilyStack(raw: string): boolean {
    const trimmed = raw.trim();
    return SAFE_FONT_FAMILY_RE.test(trimmed);
}
