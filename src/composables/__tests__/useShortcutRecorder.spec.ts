import { describe, expect, it } from 'vitest';
import {
    formatShortcutFromKeyboardEvent,
    useShortcutRecorder
} from '@/composables/useShortcutRecorder';

describe('useShortcutRecorder (Fase 09)', () => {
    it('formata Ctrl+Alt+Backquote canonicamente', () => {
        const ev = new KeyboardEvent('keydown', {
            code: 'Backquote',
            key: '`',
            ctrlKey: true,
            altKey: true
        });
        const res = formatShortcutFromKeyboardEvent(ev);
        expect(res).toEqual({
            accelerator: 'Ctrl+Alt+Backquote',
            cancelled: false,
            error: null
        });
    });

    it('aceita tecla de função F12 sem modificador', () => {
        const ev = new KeyboardEvent('keydown', {
            code: 'F12',
            key: 'F12'
        });
        const res = formatShortcutFromKeyboardEvent(ev);
        expect(res.accelerator).toBe('F12');
        expect(res.error).toBeNull();
    });

    it('rejeita letra comum sem modificador', () => {
        const ev = new KeyboardEvent('keydown', {
            code: 'KeyA',
            key: 'a'
        });
        const res = formatShortcutFromKeyboardEvent(ev);
        expect(res.accelerator).toBeNull();
        expect(res.error).toContain('modificador');
    });

    it('cancela gravação ao pressionar Escape', () => {
        const recorder = useShortcutRecorder();
        recorder.startRecording();
        expect(recorder.recording.value).toBe(true);

        const esc = new KeyboardEvent('keydown', {
            code: 'Escape',
            key: 'Escape'
        });
        const res = recorder.handleKeyDown(esc);
        expect(res?.cancelled).toBe(true);
        expect(recorder.recording.value).toBe(false);
    });
});
