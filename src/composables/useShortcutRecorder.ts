import { ref, type Ref } from 'vue';

export interface ShortcutCaptureResult {
    /** Acelerador canônico (`ex.: "Ctrl+Alt+Backquote"`, `"F12"`). */
    accelerator: string | null;
    /** Indica se o usuário pressionou `Escape` para cancelar a gravação. */
    cancelled: boolean;
    /** Mensagem de erro de validação em `pt-BR` quando a combinação é inválida. */
    error: string | null;
}

export interface UseShortcutRecorderReturn {
    recording: Readonly<Ref<boolean>>;
    pendingAccelerator: Readonly<Ref<string | null>>;
    validationError: Readonly<Ref<string | null>>;
    startRecording: () => void;
    cancelRecording: () => void;
    handleKeyDown: (event: KeyboardEvent) => ShortcutCaptureResult | null;
}

const MODIFIER_ONLY_CODES = new Set([
    'ControlLeft',
    'ControlRight',
    'ShiftLeft',
    'ShiftRight',
    'AltLeft',
    'AltRight',
    'MetaLeft',
    'MetaRight',
    'OSLeft',
    'OSRight'
]);

const MODIFIER_ONLY_KEYS = new Set([
    'Control',
    'Shift',
    'Alt',
    'AltGraph',
    'Meta',
    'OS',
    'Super',
    'Hyper'
]);

const CODE_TO_CANONICAL_KEY: Record<string, string> = {
    Backquote: 'Backquote',
    Space: 'Space',
    Tab: 'Tab',
    Enter: 'Enter',
    Backspace: 'Backspace',
    Delete: 'Delete',
    Insert: 'Insert',
    Home: 'Home',
    End: 'End',
    PageUp: 'PageUp',
    PageDown: 'PageDown',
    ArrowUp: 'Up',
    ArrowDown: 'Down',
    ArrowLeft: 'Left',
    ArrowRight: 'Right',
    Minus: 'Minus',
    Equal: 'Equal',
    BracketLeft: 'BracketLeft',
    BracketRight: 'BracketRight',
    Backslash: 'Backslash',
    Semicolon: 'Semicolon',
    Quote: 'Quote',
    Comma: 'Comma',
    Period: 'Period',
    Slash: 'Slash'
};

function resolveMainKeyToken(event: KeyboardEvent): string | null {
    const { code, key } = event;
    if (MODIFIER_ONLY_CODES.has(code) || MODIFIER_ONLY_KEYS.has(key)) {
        return null;
    }

    if (/^F([1-9]|1\d|2[0-4])$/.test(code)) {
        return code;
    }
    if (/^F([1-9]|1\d|2[0-4])$/.test(key)) {
        return key.toUpperCase();
    }
    if (/^Key([A-Z])$/.test(code)) {
        return code.slice(3);
    }
    if (/^Digit([0-9])$/.test(code)) {
        return code.slice(5);
    }
    if (CODE_TO_CANONICAL_KEY[code]) {
        return CODE_TO_CANONICAL_KEY[code] ?? null;
    }
    if (key === '`' || key === '~') {
        return 'Backquote';
    }
    if (key === ' ') {
        return 'Space';
    }
    if (/^[a-zA-Z0-9]$/.test(key)) {
        return key.toUpperCase();
    }
    return null;
}

/**
 * Converte um `KeyboardEvent` em um acelerador canônico (`Ctrl+Alt+Backquote`, `F12`, etc.)
 * compatível com a validação de `mt_os::settings::validate_accelerator`.
 */
export function formatShortcutFromKeyboardEvent(event: KeyboardEvent): ShortcutCaptureResult {
    if (event.key === 'Escape' && !event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey) {
        return {
            accelerator: null,
            cancelled: true,
            error: null
        };
    }

    const mainKey = resolveMainKeyToken(event);
    if (!mainKey) {
        return {
            accelerator: null,
            cancelled: false,
            error: 'Pressione uma tecla principal além dos modificadores (Ctrl, Alt, Shift ou Super).'
        };
    }

    const modifiers: string[] = [];
    if (event.ctrlKey) {
        modifiers.push('Ctrl');
    }
    if (event.altKey) {
        modifiers.push('Alt');
    }
    if (event.shiftKey) {
        modifiers.push('Shift');
    }
    if (event.metaKey) {
        modifiers.push('Super');
    }

    const isFunctionKey = /^F([1-9]|1\d|2[0-4])$/.test(mainKey);
    if (modifiers.length === 0 && !isFunctionKey) {
        return {
            accelerator: null,
            cancelled: false,
            error: 'Inclua pelo menos um modificador (Ctrl, Alt, Shift ou Super) ou use uma tecla F1–F24.'
        };
    }

    const accelerator = [...modifiers, mainKey].join('+');
    return {
        accelerator,
        cancelled: false,
        error: null
    };
}

/**
 * Composable para gravação interativa de atalho global (`ShortcutRecorder.vue`).
 */
export function useShortcutRecorder(): UseShortcutRecorderReturn {
    const recording = ref<boolean>(false);
    const pendingAccelerator = ref<string | null>(null);
    const validationError = ref<string | null>(null);

    function startRecording(): void {
        recording.value = true;
        pendingAccelerator.value = null;
        validationError.value = null;
    }

    function cancelRecording(): void {
        recording.value = false;
        pendingAccelerator.value = null;
        validationError.value = null;
    }

    function handleKeyDown(event: KeyboardEvent): ShortcutCaptureResult | null {
        if (!recording.value) {
            return null;
        }
        event.preventDefault();
        event.stopPropagation();

        // Ignora enquanto apenas teclas modificadoras estão sendo seguradas
        if (MODIFIER_ONLY_CODES.has(event.code) || MODIFIER_ONLY_KEYS.has(event.key)) {
            return null;
        }

        const result = formatShortcutFromKeyboardEvent(event);
        if (result.cancelled) {
            cancelRecording();
            return result;
        }

        if (result.error) {
            validationError.value = result.error;
            return result;
        }

        pendingAccelerator.value = result.accelerator;
        validationError.value = null;
        recording.value = false;
        return result;
    }

    return {
        recording,
        pendingAccelerator,
        validationError,
        startRecording,
        cancelRecording,
        handleKeyDown
    };
}
