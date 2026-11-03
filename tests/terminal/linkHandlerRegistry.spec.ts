import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
    clearLinkHandlersForTests,
    createOsc8LinkHandler,
    dispatchTerminalLink,
    isSafeHttpUrl,
    registerLinkHandler
} from '@/terminal/links/linkHandlerRegistry';

const mockCtx = {
    paneId: 'pane-1',
    ptyId: 'pty-1',
    projectId: null,
    cwd: '/tmp',
    extra: Object.freeze({})
};

describe('linkHandlerRegistry', () => {
    beforeEach(() => {
        clearLinkHandlersForTests();
        vi.restoreAllMocks();
    });

    it('rejeita esquemas perigosos (javascript:, file://, data:) e exige Ctrl+Clique', async () => {
        expect(isSafeHttpUrl('https://engeapp.com.br/docs')).toBe(true);
        expect(isSafeHttpUrl('http://127.0.0.1:5177')).toBe(true);
        expect(isSafeHttpUrl('javascript:alert(1)')).toBe(false);
        expect(isSafeHttpUrl('file:///etc/passwd')).toBe(false);
        expect(isSafeHttpUrl('data:text/html,<script>alert(1)</script>')).toBe(false);

        const openSpy = vi.spyOn(window, 'open').mockImplementation(() => null);

        // Clique sem Ctrl/Meta não ativa.
        const clickWithoutModifier = new MouseEvent('click', { ctrlKey: false, metaKey: false });
        const resultNoMod = await dispatchTerminalLink(clickWithoutModifier, 'https://engeapp.com.br', mockCtx);
        expect(resultNoMod).toBe(false);
        expect(openSpy).not.toHaveBeenCalled();

        // Clique com Ctrl em javascript: é bloqueado.
        const clickWithCtrl = new MouseEvent('click', { ctrlKey: true });
        const resultJs = await dispatchTerminalLink(clickWithCtrl, 'javascript:alert(1)', mockCtx);
        expect(resultJs).toBe(false);
        expect(openSpy).not.toHaveBeenCalled();

        // Clique com Ctrl em https:// abre com segurança.
        const resultHttps = await dispatchTerminalLink(clickWithCtrl, 'https://engeapp.com.br', mockCtx);
        expect(resultHttps).toBe(true);
    });

    it('respeita precedência por priority em handlers customizados e aplica política a OSC 8', async () => {
        const calls: string[] = [];

        registerLinkHandler({
            id: 'low-prio',
            priority: 10,
            match: /app\/.*/,
            accepts: (uri) => uri.startsWith('app/'),
            activate: (_e, uri) => {
                calls.push(`low:${uri}`);
            }
        });

        const subHigh = registerLinkHandler({
            id: 'high-prio',
            priority: 50,
            match: /app\/.*/,
            accepts: (uri) => uri.startsWith('app/'),
            activate: (_e, uri) => {
                calls.push(`high:${uri}`);
            }
        });

        const ctrlClick = new MouseEvent('click', { ctrlKey: true });
        await dispatchTerminalLink(ctrlClick, 'app/Models/User.php:42', mockCtx);
        expect(calls).toEqual(['high:app/Models/User.php:42']);

        subHigh.dispose();
        await dispatchTerminalLink(ctrlClick, 'app/Models/User.php:42', mockCtx);
        expect(calls).toEqual(['high:app/Models/User.php:42', 'low:app/Models/User.php:42']);

        // Testa OSC 8 linkHandler.
        let hovered: string | null = null;
        const osc8 = createOsc8LinkHandler(() => mockCtx, (uri) => {
            hovered = uri;
        });
        osc8.hover?.(ctrlClick, 'https://maxterminal.dev', {
            start: { x: 0, y: 0 },
            end: { x: 10, y: 0 }
        });
        expect(hovered).toBe('https://maxterminal.dev');
        osc8.leave?.(ctrlClick, 'https://maxterminal.dev', {
            start: { x: 0, y: 0 },
            end: { x: 10, y: 0 }
        });
        expect(hovered).toBeNull();
    });
});
