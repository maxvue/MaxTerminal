import { describe, expect, it } from 'vitest';
import type { Terminal } from '@xterm/xterm';
import type { TerminalModes } from '@/types/terminal';
import { ModeTracker } from '@/terminal/engine/modeTracker';
import { MockXtermTerminal } from './mocks/xtermMock';

describe('ModeTracker', () => {
    it('detecta transições de altScreen (?1049h/?1049l), mouseTracking, bracketedPaste e trunca título a 256 caracteres', () => {
        const mockTerm = new MockXtermTerminal();
        const modeChanges: Array<{ next: TerminalModes; prev: TerminalModes }> = [];
        const titles: string[] = [];

        const tracker = new ModeTracker({
            xterm: mockTerm as unknown as Terminal,
            onModesChange: (next, prev) => {
                modeChanges.push({ next, prev });
            },
            onTitleChange: (title) => {
                titles.push(title);
            }
        });

        // Entra em alternate screen (?1049h).
        mockTerm.switchBuffer('alternate');
        expect(modeChanges).toHaveLength(1);
        expect(modeChanges[0]?.next.altScreen).toBe(true);

        // Ativa mouse tracking e bracketed paste.
        mockTerm.modes.mouseTrackingMode = 'any';
        mockTerm.modes.bracketedPasteMode = true;
        tracker.sampleModes();
        expect(modeChanges).toHaveLength(2);
        expect(modeChanges[1]?.next).toEqual({
            altScreen: true,
            mouseTracking: 'any',
            bracketedPaste: true,
            applicationCursorKeys: false
        });

        // Sai de alternate screen (?1049l).
        mockTerm.switchBuffer('normal');
        expect(tracker.getModes().altScreen).toBe(false);

        // Título com controle C0 e mais de 256 caracteres.
        const longRawTitle = `\x07\x1b${'A'.repeat(300)}`;
        mockTerm.emitTitleChange(longRawTitle);
        expect(titles).toHaveLength(1);
        expect(titles[0]?.length).toBe(256);
        expect(titles[0]?.includes('\x07')).toBe(false);

        tracker.dispose();
    });
});
