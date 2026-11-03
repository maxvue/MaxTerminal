import type { IDisposable, Terminal } from '@xterm/xterm';
import type { Disposable, MouseTrackingMode, TerminalModes } from '@/types/terminal';

/** Opções de inicialização do `ModeTracker`. */
export interface ModeTrackerOptions {
    xterm: Terminal;
    onModesChange: (modes: TerminalModes, prevModes: TerminalModes) => void;
    onTitleChange: (title: string) => void;
}

const VALID_MOUSE_MODES = new Set<MouseTrackingMode>(['none', 'x10', 'vt200', 'drag', 'any']);

function sanitizeTitle(raw: string, maxLength: number): string {
    // Remove controles C0 (0x00..0x1F), DEL (0x7F) e controles C1 (0x80..0x9F).
    let out = '';
    for (let i = 0; i < raw.length; i++) {
        const code = raw.charCodeAt(i);
        if ((code >= 0x00 && code <= 0x1f) || code === 0x7f || (code >= 0x80 && code <= 0x9f)) {
            continue;
        }
        out += raw[i];
        if (out.length >= maxLength) {
            break;
        }
    }
    return out;
}

/**
 * Monitor de modos VT (`altScreen`, `mouseTracking`, `bracketedPaste`, `applicationCursorKeys`)
 * e título OSC 0/2 do `@xterm/xterm`.
 */
export class ModeTracker implements Disposable {
    public static readonly MAX_TITLE_LENGTH = 256;

    private readonly xterm: Terminal;
    private readonly onModesChange: (modes: TerminalModes, prevModes: TerminalModes) => void;
    private readonly onTitleChange: (title: string) => void;

    private currentModes: TerminalModes = {
        altScreen: false,
        mouseTracking: 'none',
        bracketedPaste: false,
        applicationCursorKeys: false
    };
    private currentTitle = '';
    private readonly subs: IDisposable[] = [];
    private disposed = false;

    public constructor(opts: ModeTrackerOptions) {
        this.xterm = opts.xterm;
        this.onModesChange = opts.onModesChange;
        this.onTitleChange = opts.onTitleChange;

        if (this.xterm.buffer?.onBufferChange) {
            this.subs.push(
                this.xterm.buffer.onBufferChange(() => {
                    this.sampleModes();
                })
            );
        }

        if (this.xterm.onTitleChange) {
            this.subs.push(
                this.xterm.onTitleChange((rawTitle) => {
                    if (this.disposed) {
                        return;
                    }
                    const clean = sanitizeTitle(rawTitle, ModeTracker.MAX_TITLE_LENGTH);
                    if (clean !== this.currentTitle) {
                        this.currentTitle = clean;
                        this.onTitleChange(clean);
                    }
                })
            );
        }
    }

    /** Amostra os modos atuais do xterm e emite `onModesChange` apenas se houver diferença. */
    public sampleModes(): TerminalModes {
        if (this.disposed) {
            return { ...this.currentModes };
        }

        const altScreen = this.xterm.buffer?.active?.type === 'alternate';
        const rawMouse = this.xterm.modes?.mouseTrackingMode ?? 'none';
        const mouseTracking: MouseTrackingMode = VALID_MOUSE_MODES.has(rawMouse as MouseTrackingMode)
            ? (rawMouse as MouseTrackingMode)
            : 'none';
        const bracketedPaste = Boolean(this.xterm.modes?.bracketedPasteMode);
        const applicationCursorKeys = Boolean(this.xterm.modes?.applicationCursorKeysMode);

        const next: TerminalModes = {
            altScreen,
            mouseTracking,
            bracketedPaste,
            applicationCursorKeys
        };

        const prev = this.currentModes;
        if (
            prev.altScreen !== next.altScreen ||
            prev.mouseTracking !== next.mouseTracking ||
            prev.bracketedPaste !== next.bracketedPaste ||
            prev.applicationCursorKeys !== next.applicationCursorKeys
        ) {
            this.currentModes = next;
            this.onModesChange({ ...next }, { ...prev });
        }

        return { ...this.currentModes };
    }

    public getModes(): TerminalModes {
        return { ...this.currentModes };
    }

    public getTitle(): string {
        return this.currentTitle;
    }

    public dispose(): void {
        if (this.disposed) {
            return;
        }
        this.disposed = true;
        for (const sub of this.subs) {
            sub.dispose();
        }
        this.subs.length = 0;
    }
}
