import axios from 'axios';
import { computed, getCurrentInstance, ref } from 'vue';
import { defineStore } from 'pinia';
import { _ } from '@maxvue/max-use';
import {
    THEME_SCHEME_VIRTUAL_ROUTE,
    clearLastThemeSchemeSaveState,
    getLastThemeSchemeSaveError,
    getLastThemeSchemeSaveResult,
    installThemeMaxPiniaAdapter,
    type ThemeSchemeSaveRequest
} from '@/ipc/themeMaxPiniaAdapter';
import type {
    AnsiPaletteInput,
    ColorScheme,
    CursorStyle,
    ResolvedTheme,
    ThemeDraftInput,
    ThemeId,
    ThemeOrigin
} from '@/types/theme';

export type ThemeEditorMode = 'create' | 'edit';

export interface ThemeEditorState {
    mode: ThemeEditorMode;
    editingId: ThemeId | null;
    origin: ThemeOrigin;
    draft: ThemeDraftInput;
}

interface MaxPiniaExposed {
    pauseSave?: () => void;
    resumeSave?: () => void;
    reload?: () => Promise<void>;
    saveInServer?: () => Promise<void>;
}

const DEFAULT_ANSI_PALETTE_INPUT: AnsiPaletteInput = {
    black: '#0b1420',
    red: '#f43f5e',
    green: '#10b981',
    yellow: '#f59e0b',
    blue: '#3b82f6',
    magenta: '#a855f7',
    cyan: '#06b6d4',
    white: '#e2e8f0',
    brightBlack: '#475569',
    brightRed: '#fb7185',
    brightGreen: '#34d399',
    brightYellow: '#fbbf24',
    brightBlue: '#60a5fa',
    brightMagenta: '#c084fc',
    brightCyan: '#22d3ee',
    brightWhite: '#f8fafc'
};

export function createDefaultThemeDraftInput(
    baseScheme?: ColorScheme | null
): ThemeDraftInput {
    if (baseScheme) {
        const cloned = schemeToDraftInput(baseScheme);
        return {
            ...cloned,
            name: '',
            author: baseScheme.metadata?.author ?? null
        };
    }
    return {
        name: '',
        author: null,
        foreground: '#e2e8f0',
        background: '#0b1420',
        cursor: '#14b8a6',
        cursorAccent: '#0b1420',
        cursorStyle: 'block',
        selectionBackground: '#14b8a652',
        selectionForeground: null,
        activeTabAccent: '#14b8a6',
        ansi: _.cloneDeep(DEFAULT_ANSI_PALETTE_INPUT),
        indexedColors: {}
    };
}

export function schemeToDraftInput(scheme: ColorScheme): ThemeDraftInput {
    return {
        name: scheme.name,
        author: scheme.metadata?.author ?? null,
        foreground: scheme.foreground,
        background: scheme.background,
        cursor: scheme.cursor,
        cursorAccent: scheme.cursorAccent,
        cursorStyle: scheme.cursorStyle,
        selectionBackground: scheme.selectionBackground,
        selectionForeground: scheme.selectionForeground ?? null,
        activeTabAccent: scheme.activeTabAccent ?? null,
        ansi: {
            black: scheme.ansi.black,
            red: scheme.ansi.red,
            green: scheme.ansi.green,
            yellow: scheme.ansi.yellow,
            blue: scheme.ansi.blue,
            magenta: scheme.ansi.magenta,
            cyan: scheme.ansi.cyan,
            white: scheme.ansi.white,
            brightBlack: scheme.ansi.brightBlack,
            brightRed: scheme.ansi.brightRed,
            brightGreen: scheme.ansi.brightGreen,
            brightYellow: scheme.ansi.brightYellow,
            brightBlue: scheme.ansi.brightBlue,
            brightMagenta: scheme.ansi.brightMagenta,
            brightCyan: scheme.ansi.brightCyan,
            brightWhite: scheme.ansi.brightWhite
        },
        indexedColors: _.cloneDeep(scheme.indexedColors ?? {})
    };
}

function normalizeHex6(raw: string, fallback = '#14b8a6'): string {
    const cleaned = raw.trim().replace(/^#/, '');
    if (/^[0-9a-fA-F]{3}$/.test(cleaned)) {
        const r = cleaned[0]!;
        const g = cleaned[1]!;
        const b = cleaned[2]!;
        return `#${r}${r}${g}${g}${b}${b}`.toLowerCase();
    }
    if (/^[0-9a-fA-F]{6,8}$/.test(cleaned)) {
        return `#${cleaned.slice(0, 6)}`.toLowerCase();
    }
    return fallback;
}

/**
 * Separa uma cor `#RRGGBBAA` ou `#RRGGBB` em `{ hex: '#rrggbb', alphaPercent: 0..100 }`.
 */
export function parseSelectionBackground(raw: string): {
    hex: string;
    alphaPercent: number;
} {
    const cleaned = raw.trim().replace(/^#/, '');
    if (/^[0-9a-fA-F]{8}$/.test(cleaned)) {
        const hex = `#${cleaned.slice(0, 6)}`.toLowerCase();
        const alphaByte = Number.parseInt(cleaned.slice(6, 8), 16);
        const alphaPercent = Math.round((alphaByte / 255) * 100);
        return { hex, alphaPercent };
    }
    if (/^[0-9a-fA-F]{4}$/.test(cleaned)) {
        const r = cleaned[0]!;
        const g = cleaned[1]!;
        const b = cleaned[2]!;
        const a = cleaned[3]!;
        const hex = `#${r}${r}${g}${g}${b}${b}`.toLowerCase();
        const alphaByte = Number.parseInt(`${a}${a}`, 16);
        const alphaPercent = Math.round((alphaByte / 255) * 100);
        return { hex, alphaPercent };
    }
    return {
        hex: normalizeHex6(raw, '#14b8a6'),
        alphaPercent: 32
    };
}

/**
 * Formata `#RRGGBB` + `alphaPercent` (`0..100`) em `#rrggbbaa` canônico.
 */
export function formatSelectionBackground(
    hex: string,
    alphaPercent: number
): string {
    const baseHex = normalizeHex6(hex, '#14b8a6');
    const clampedPercent = Math.max(5, Math.min(100, Math.round(alphaPercent)));
    const alphaByte = Math.round((clampedPercent / 100) * 255);
    const alphaHex = alphaByte.toString(16).padStart(2, '0').toLowerCase();
    return `${baseHex}${alphaHex}`;
}

export function isValidHexColor(value: string | null | undefined): boolean {
    if (!value || typeof value !== 'string') {
        return false;
    }
    const trimmed = value.trim();
    return /^#([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/.test(trimmed);
}

export function isThemeDraftValid(draft: ThemeDraftInput): { valid: boolean; error?: string } {
    if (!draft.name || draft.name.trim().length === 0) {
        return { valid: false, error: 'O nome do tema é obrigatório.' };
    }
    if (!isValidHexColor(draft.background)) {
        return { valid: false, error: 'Cor de fundo principal inválida (esperado #RRGGBB).' };
    }
    if (!isValidHexColor(draft.foreground)) {
        return { valid: false, error: 'Cor de texto principal inválida (esperado #RRGGBB).' };
    }
    if (!isValidHexColor(draft.cursor)) {
        return { valid: false, error: 'Cor do cursor inválida (esperado #RRGGBB).' };
    }
    if (!isValidHexColor(draft.cursorAccent)) {
        return { valid: false, error: 'Cor de texto sob o cursor inválida (esperado #RRGGBB).' };
    }
    if (draft.selectionForeground && !isValidHexColor(draft.selectionForeground)) {
        return { valid: false, error: 'Cor de texto selecionado inválida (esperado #RRGGBB).' };
    }
    if (draft.activeTabAccent && !isValidHexColor(draft.activeTabAccent)) {
        return { valid: false, error: 'Cor de destaque da aba ativa inválida (esperado #RRGGBB).' };
    }
    for (const [key, color] of Object.entries(draft.ansi)) {
        if (!isValidHexColor(color)) {
            return { valid: false, error: `Cor ANSI "${key}" inválida (esperado #RRGGBB).` };
        }
    }
    return { valid: true };
}

const DEFAULT_EDITOR_STATE: ThemeEditorState = {
    mode: 'create',
    editingId: null,
    origin: 'custom',
    draft: createDefaultThemeDraftInput(null)
};

export const useThemeEditorStore = defineStore('theme-editor', () => {
    installThemeMaxPiniaAdapter();

    const isCached = ref(true);
    const id = ref('theme-editor');
    const data = ref<ThemeEditorState>(_.cloneDeep(DEFAULT_EDITOR_STATE));
    const default_value = ref<ThemeEditorState>(_.cloneDeep(DEFAULT_EDITOR_STATE));
    const block_save = ref(true);

    const isOpen = ref(false);
    const saving = ref(false);
    const errorMessage = ref<string | null>(null);
    const baselineDraft = ref<ThemeDraftInput>(
        _.cloneDeep(DEFAULT_EDITOR_STATE.draft)
    );
    const lastSavedResolved = ref<ResolvedTheme | null>(null);

    const options = {
        get: {
            route: THEME_SCHEME_VIRTUAL_ROUTE,
            data: computed(() => ({ id: data.value.editingId ?? '' }))
        },
        save: THEME_SCHEME_VIRTUAL_ROUTE
    };

    const isDirty = computed<boolean>(
        () => !_.isEqual(data.value.draft, baselineDraft.value)
    );

    const isReadOnly = computed<boolean>(
        () => data.value.mode === 'edit' && data.value.origin === 'builtin'
    );

    const validationResult = computed(() => isThemeDraftValid(data.value.draft));
    const isColorsValid = computed(() => validationResult.value.valid);

    const selectionParts = computed(() =>
        parseSelectionBackground(data.value.draft.selectionBackground)
    );

    function ensureAutoSavePaused(): void {
        const self = useThemeEditorStore() as unknown as Partial<MaxPiniaExposed>;
        if (typeof self.pauseSave === 'function') {
            self.pauseSave();
        }
    }

    function getSaveData(
        current: ThemeEditorState = data.value
    ): ThemeSchemeSaveRequest {
        const state = current ?? data.value;
        const cleanDraft: ThemeDraftInput = {
            ..._.cloneDeep(state.draft),
            name: state.draft.name.trim(),
            author:
                state.draft.author && state.draft.author.trim().length > 0
                    ? state.draft.author.trim()
                    : null,
            selectionForeground:
                state.draft.selectionForeground &&
                state.draft.selectionForeground.trim().length > 0
                    ? state.draft.selectionForeground.trim()
                    : null,
            activeTabAccent:
                state.draft.activeTabAccent &&
                state.draft.activeTabAccent.trim().length > 0
                    ? state.draft.activeTabAccent.trim()
                    : null
        };
        return {
            kind: state.mode === 'edit' ? 'update' : 'create',
            id: state.mode === 'edit' ? state.editingId : null,
            input: cleanDraft
        };
    }

    function applyResolvedIntoState(
        resolved: ResolvedTheme,
        mode: ThemeEditorMode
    ): void {
        const draft = schemeToDraftInput(resolved.scheme);
        baselineDraft.value = _.cloneDeep(draft);
        data.value = {
            mode,
            editingId: mode === 'edit' ? resolved.scheme.id : null,
            origin: resolved.scheme.origin,
            draft: _.cloneDeep(draft)
        };
    }

    function onServerData(raw: unknown): ThemeEditorState {
        if (raw && typeof raw === 'object' && 'scheme' in raw) {
            const resolved = raw as ResolvedTheme;
            lastSavedResolved.value = resolved;
            const draft = schemeToDraftInput(resolved.scheme);
            baselineDraft.value = _.cloneDeep(draft);
            const nextState: ThemeEditorState = {
                mode: 'edit',
                editingId: resolved.scheme.id,
                origin: resolved.scheme.origin,
                draft
            };
            data.value = nextState;
            return nextState;
        }
        return data.value;
    }

    function openCreate(baseScheme?: ColorScheme | null): void {
        ensureAutoSavePaused();
        errorMessage.value = null;
        lastSavedResolved.value = null;
        const draft = createDefaultThemeDraftInput(baseScheme ?? null);
        baselineDraft.value = _.cloneDeep(draft);
        data.value = {
            mode: 'create',
            editingId: null,
            origin: 'custom',
            draft: _.cloneDeep(draft)
        };
        isOpen.value = true;
    }

    function openEdit(source: ColorScheme | ResolvedTheme): void {
        ensureAutoSavePaused();
        errorMessage.value = null;
        lastSavedResolved.value = null;
        const scheme = 'scheme' in source ? source.scheme : source;
        const draft = schemeToDraftInput(scheme);
        baselineDraft.value = _.cloneDeep(draft);
        data.value = {
            mode: 'edit',
            editingId: scheme.id,
            origin: scheme.origin,
            draft: _.cloneDeep(draft)
        };
        isOpen.value = true;
    }

    async function loadAndOpenEdit(themeId: ThemeId): Promise<void> {
        ensureAutoSavePaused();
        errorMessage.value = null;
        lastSavedResolved.value = null;
        data.value.editingId = themeId;
        const self = useThemeEditorStore() as unknown as Partial<MaxPiniaExposed>;
        if (typeof self.reload === 'function') {
            await self.reload();
        }
        if (!lastSavedResolved.value) {
            const response = await axios.get<ResolvedTheme | null>(
                THEME_SCHEME_VIRTUAL_ROUTE,
                { params: { id: themeId } }
            );
            if (response.data) {
                onServerData(response.data);
            }
        }
        isOpen.value = true;
    }

    function closeEditor(): void {
        ensureAutoSavePaused();
        isOpen.value = false;
        errorMessage.value = null;
    }

    function resetDraft(): void {
        ensureAutoSavePaused();
        errorMessage.value = null;
        data.value = {
            ...data.value,
            draft: _.cloneDeep(baselineDraft.value)
        };
    }

    function setSelectionColor(hex: string, alphaPercent?: number): void {
        ensureAutoSavePaused();
        const current = parseSelectionBackground(
            data.value.draft.selectionBackground
        );
        const nextHex = hex || current.hex;
        const nextAlpha =
            typeof alphaPercent === 'number' ? alphaPercent : current.alphaPercent;
        data.value.draft.selectionBackground = formatSelectionBackground(
            nextHex,
            nextAlpha
        );
    }

    function setCursorStyle(style: CursorStyle): void {
        ensureAutoSavePaused();
        data.value.draft.cursorStyle = style;
    }

    async function submitDraft(): Promise<ResolvedTheme> {
        ensureAutoSavePaused();
        if (saving.value) {
            throw new Error('Salvamento já em andamento.');
        }
        if (isReadOnly.value) {
            const msg = 'Temas nativos são somente leitura. Duplique o tema para editá-lo.';
            errorMessage.value = msg;
            throw new Error(msg);
        }
        const validation = isThemeDraftValid(data.value.draft);
        if (!validation.valid) {
            const msg = validation.error ?? 'Dados do tema inválidos.';
            errorMessage.value = msg;
            throw new Error(msg);
        }

        saving.value = true;
        errorMessage.value = null;
        clearLastThemeSchemeSaveState();

        try {
            const self = useThemeEditorStore() as unknown as Partial<MaxPiniaExposed>;
            if (typeof self.saveInServer === 'function') {
                await self.saveInServer();
            }

            let adapterErr = getLastThemeSchemeSaveError();
            if (adapterErr) {
                throw adapterErr;
            }

            let saved =
                getLastThemeSchemeSaveResult() ?? lastSavedResolved.value;
            if (!saved) {
                const response = await axios.post<ResolvedTheme>(
                    THEME_SCHEME_VIRTUAL_ROUTE,
                    getSaveData(data.value)
                );
                adapterErr = getLastThemeSchemeSaveError();
                if (adapterErr) {
                    throw adapterErr;
                }
                saved =
                    getLastThemeSchemeSaveResult() ??
                    response?.data ??
                    lastSavedResolved.value;
            }
            if (!saved) {
                throw new Error('Não foi possível confirmar a gravação do tema.');
            }

            applyResolvedIntoState(saved, 'edit');
            lastSavedResolved.value = saved;
            isOpen.value = false;
            return saved;
        } catch (err) {
            const message =
                err && typeof err === 'object' && 'message' in err && typeof (err as { message: unknown }).message === 'string'
                    ? (err as { message: string }).message
                    : err instanceof Error
                        ? err.message
                        : 'Falha ao salvar o tema.';
            errorMessage.value = message;
            throw err;
        } finally {
            saving.value = false;
        }
    }

    // Garante que caso chamado dentro de um componente, o auto-save por debounce permaneça pausado
    if (getCurrentInstance()) {
        queueMicrotask(() => {
            ensureAutoSavePaused();
        });
    }

    return {
        isCached,
        id,
        data,
        default_value,
        block_save,
        options,
        isOpen,
        saving,
        errorMessage,
        baselineDraft,
        lastSavedResolved,
        isDirty,
        isReadOnly,
        validationResult,
        isColorsValid,
        selectionParts,
        getSaveData,
        onServerData,
        openCreate,
        openEdit,
        loadAndOpenEdit,
        closeEditor,
        resetDraft,
        setSelectionColor,
        setCursorStyle,
        submitDraft
    };
});
