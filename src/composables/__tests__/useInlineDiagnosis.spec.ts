import { ref } from 'vue';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as adapters from '@/composables/ai/adapters';
import { useInlineDiagnosis, type DiagnosableBlockLike } from '@/composables/ai/useInlineDiagnosis';
import { aiIpc } from '@/ipc/ai';
import { DEFAULT_AI_SETTINGS, useAiStore } from '@/stores/ai';
import { useTerminalStore } from '@/stores/terminal';
import type { InlineDiagnosis_Serialize } from '@/types/bindings';

function makeDiagnosis(
    level: 'safe' | 'caution' | 'destructive' = 'safe'
): InlineDiagnosis_Serialize {
    return {
        blockId: 'block-1',
        errorHash: 'sha256-abc',
        summary: 'A porta 5177 já está ocupada por outro processo Vite.',
        fixCommand: level === 'destructive' ? 'rm -rf node_modules' : 'npx kill-port 5177',
        fixRisk: {
            level,
            localLevel: level,
            modelHint: null,
            reasons:
                level === 'destructive'
                    ? [
                        {
                            ruleId: 'rm-recursive-or-force',
                            message: 'Remove diretório recursivamente',
                            segment: 'rm -rf node_modules'
                        }
                    ]
                    : [],
            requiresConfirmation: level === 'destructive',
            confirmToken: level === 'destructive' ? 'rm -rf node_modules' : null
        },
        fromCache: false,
        createdAt: '2026-11-01T12:00:00Z'
    };
}

describe('useInlineDiagnosis (Fase 12 — Unidade 05)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.restoreAllMocks();
    });

    it('avalia elegibilidade (ignora exit 0, 130 e alt-screen), obtém diagnóstico, insere/executa correção e dispensa', async () => {
        const aiStore = useAiStore();
        aiStore.settings = { ...DEFAULT_AI_SETTINGS };

        const blockRef = ref<DiagnosableBlockLike>({
            id: 'block-1',
            paneId: 'pane-1',
            status: 'success',
            exitCode: 0,
            command: 'npm run dev'
        });

        const diag = useInlineDiagnosis(blockRef, { autoTrigger: false });
        expect(diag.eligible.value).toBe(false);

        // Exit 130 (Ctrl+C) também não é elegível
        blockRef.value = { ...blockRef.value, status: 'error', exitCode: 130 };
        expect(diag.eligible.value).toBe(false);

        // Exit 1 com erro é elegível
        blockRef.value = { ...blockRef.value, status: 'error', exitCode: 1 };
        expect(diag.eligible.value).toBe(true);

        // Se o painel estiver em altScreen (TUI), não é elegível
        const terminalStore = useTerminalStore();
        terminalStore.register('pane-1', 'pty-1');
        terminalStore.patch('pane-1', {
            modes: {
                altScreen: true,
                mouseTracking: 'none',
                bracketedPaste: false,
                applicationCursorKeys: false
            }
        });
        expect(diag.eligible.value).toBe(false);
        terminalStore.patch('pane-1', {
            modes: {
                altScreen: false,
                mouseTracking: 'none',
                bracketedPaste: false,
                applicationCursorKeys: false
            }
        });
        expect(diag.eligible.value).toBe(true);

        // Solicita diagnóstico
        vi.spyOn(aiIpc, 'diagnoseBlock').mockResolvedValueOnce({
            status: 'ready',
            diagnosis: makeDiagnosis('safe')
        });

        const result = await diag.diagnoseNow();
        expect(result?.summary).toContain('porta 5177');
        expect(diag.visible.value).toBe(true);

        const insertSpy = vi.spyOn(adapters, 'insertCommandIntoRichInput').mockReturnValue(true);
        expect(diag.insertFix()).toBe(true);
        expect(insertSpy).toHaveBeenCalledWith('pane-1', 'npx kill-port 5177');

        vi.spyOn(aiIpc, 'classifyRisk').mockResolvedValueOnce({
            level: 'safe',
            localLevel: 'safe',
            modelHint: null,
            reasons: [],
            requiresConfirmation: false,
            confirmToken: null
        });
        const execSpy = vi.spyOn(adapters, 'executeCommandInPane').mockResolvedValue();
        await diag.executeSafeOrCautionFix();
        expect(execSpy).toHaveBeenCalledWith('pane-1', 'npx kill-port 5177');

        // Dispensar diagnóstico
        const dismissSpy = vi.spyOn(aiIpc, 'diagnosisDismiss').mockResolvedValue();
        await diag.dismiss();
        expect(dismissSpy).toHaveBeenCalledWith('block-1');
        expect(diag.visible.value).toBe(false);

        // Execução de correção destrutiva com token
        aiStore.setBlockDiagnosis('block-1', {
            blockId: 'block-1',
            status: 'ready',
            diagnosis: makeDiagnosis('destructive'),
            skipReason: null,
            error: null
        });
        await expect(diag.executeSafeOrCautionFix()).rejects.toThrow('destrutiva');
        await expect(diag.executeDestructiveFix('errado')).rejects.toThrow(
            'Token de confirmação inválido'
        );
        await diag.executeDestructiveFix('rm -rf node_modules');
        expect(execSpy).toHaveBeenCalledWith('pane-1', 'rm -rf node_modules');

        // Caso skipped pelo backend
        aiStore.setBlockDiagnosis('block-2', {
            blockId: 'block-2',
            status: 'pending',
            diagnosis: null,
            skipReason: null,
            error: null
        });
        blockRef.value = {
            id: 'block-2',
            paneId: 'pane-1',
            status: 'error',
            exitCode: 2,
            command: 'bad-cmd'
        };
        vi.spyOn(aiIpc, 'diagnoseBlock').mockResolvedValueOnce({
            status: 'skipped',
            reason: 'rateLimited'
        });
        expect(await diag.diagnoseNow()).toBeNull();
        expect(diag.state.value?.status).toBe('skipped');
    });
});
