import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import DbRecoveryPanel from '@/components/shell/DbRecoveryPanel.vue';
import { MtIpcError } from '@/ipc/core';
import * as dbIpc from '@/ipc/db';
import { useSettingsStore } from '@/stores/settings';
import type { DbRecovered, DbStatusDto, SettingsChanged } from '@/types/bindings';
import { SETTINGS_DEFAULTS } from '@/types/settings';

vi.mock('@/ipc/db', () => ({
    getSettings: vi.fn(),
    setSetting: vi.fn(),
    getSettingSnapshot: vi.fn(),
    getStatus: vi.fn(),
    recover: vi.fn(),
    onSettingsChanged: vi.fn(),
    onDbRecovered: vi.fn(),
    onMaintenanceFinished: vi.fn(),
    onWriterLag: vi.fn()
}));

const readyStatus: DbStatusDto = {
    health: {
        state: 'ready',
        schema_version: 1,
        sqlite_version: '3.51.3'
    },
    pendingBytes: 0,
    pendingRecords: 0,
    writerLagEvents: 0
};

const recoveryStatus: DbStatusDto = {
    health: {
        state: 'recovery_required',
        code: 'db.corrupted',
        reason: 'O banco de dados local apresenta inconsistências estruturais.',
        db_path: '/tmp/maxterminal/maxterminal.db',
        available_backups: [
            {
                fileName: 'maxterminal-20260417T153045Z-pre_migration.db',
                path: '/tmp/maxterminal/backups/maxterminal-20260417T153045Z-pre_migration.db',
                kind: 'pre_migration',
                createdAt: 1776439845000,
                sizeBytes: 32768
            },
            {
                fileName: 'maxterminal-20260417T160000Z-manual.db',
                path: '/tmp/maxterminal/backups/maxterminal-20260417T160000Z-manual.db',
                kind: 'manual',
                createdAt: 1776441600000,
                sizeBytes: 512
            }
        ]
    },
    pendingBytes: 0,
    pendingRecords: 0,
    writerLagEvents: 0
};

describe('stores/settings + DbRecoveryPanel', () => {
    let changedCallback: ((ev: SettingsChanged) => void) | null = null;
    let recoveredCallback: ((ev: DbRecovered) => void) | null = null;

    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
        changedCallback = null;
        recoveredCallback = null;

        vi.mocked(dbIpc.onSettingsChanged).mockImplementation(async (cb) => {
            changedCallback = cb;
            return () => {
                changedCallback = null;
            };
        });
        vi.mocked(dbIpc.onDbRecovered).mockImplementation(async (cb) => {
            recoveredCallback = cb;
            return () => {
                recoveredCallback = null;
            };
        });
    });

    it('hidrata configurações mesclando com SETTINGS_DEFAULTS quando o banco está ready', async () => {
        vi.mocked(dbIpc.getStatus).mockResolvedValue({ ok: true, data: readyStatus });
        vi.mocked(dbIpc.getSettings).mockResolvedValue({
            ok: true,
            data: {
                'ui.theme': 'max-dark',
                'terminal.fontSize': 16
            }
        });

        const store = useSettingsStore();
        await store.hydrate();

        expect(store.hydrated).toBe(true);
        expect(store.isRecoveryRequired).toBe(false);
        expect(store.get('ui.theme')).toBe('max-dark');
        expect(store.get('terminal.fontSize')).toBe(16);
        expect(store.get('terminal.scrollbackLines')).toBe(SETTINGS_DEFAULTS['terminal.scrollbackLines']);
    });

    it('entra em modo recovery_required sem chamar getSettings quando o banco está degradado', async () => {
        vi.mocked(dbIpc.getStatus).mockResolvedValue({ ok: true, data: recoveryStatus });

        const store = useSettingsStore();
        await store.hydrate();

        expect(store.hydrated).toBe(false);
        expect(store.isRecoveryRequired).toBe(true);
        expect(dbIpc.getSettings).not.toHaveBeenCalled();
    });

    it('atualiza valor e revisão em set() e reconcilia automaticamente em db.setting_conflict', async () => {
        vi.mocked(dbIpc.getStatus).mockResolvedValue({ ok: true, data: readyStatus });
        vi.mocked(dbIpc.getSettings).mockResolvedValue({ ok: true, data: {} });

        const store = useSettingsStore();
        await store.hydrate();

        // 1. Sucesso direto
        vi.mocked(dbIpc.setSetting).mockResolvedValueOnce({
            ok: true,
            data: {
                key: 'terminal.fontSize',
                value: 15,
                revision: 1,
                updatedAt: 1000
            }
        });

        const ok1 = await store.set('terminal.fontSize', 15);
        expect(ok1).toBe(true);
        expect(store.get('terminal.fontSize')).toBe(15);
        expect(store.revisions['terminal.fontSize']).toBe(1);

        // 2. Conflito de revisão na primeira tentativa -> busca snapshot (rev 4) -> reenvia -> sucesso (rev 5)
        vi.mocked(dbIpc.setSetting)
            .mockResolvedValueOnce({
                ok: false,
                error: new MtIpcError(
                    {
                        code: 'db.setting_conflict',
                        message: 'Conflito de revisão'
                    },
                    'db_settings_set'
                )
            })
            .mockResolvedValueOnce({
                ok: true,
                data: {
                    key: 'terminal.fontSize',
                    value: 18,
                    revision: 5,
                    updatedAt: 2000
                }
            });

        vi.mocked(dbIpc.getSettingSnapshot).mockResolvedValueOnce({
            ok: true,
            data: {
                key: 'terminal.fontSize',
                value: 16,
                revision: 4,
                updatedAt: 1500
            }
        });

        const ok2 = await store.set('terminal.fontSize', 18);
        expect(ok2).toBe(true);
        expect(dbIpc.getSettingSnapshot).toHaveBeenCalledWith('terminal.fontSize');
        expect(dbIpc.setSetting).toHaveBeenLastCalledWith('terminal.fontSize', 18, 4);
        expect(store.get('terminal.fontSize')).toBe(18);
        expect(store.revisions['terminal.fontSize']).toBe(5);
    });

    it('preserva lastError e retorna false quando set() recebe db.setting_secret_rejected', async () => {
        const store = useSettingsStore();
        const secretError = new MtIpcError(
            {
                code: 'db.setting_secret_rejected',
                message: 'Segredo bloqueado'
            },
            'db_settings_set'
        );

        vi.mocked(dbIpc.setSetting).mockResolvedValueOnce({
            ok: false,
            error: secretError
        });

        const ok = await store.set('privacy.redactCustomPatterns', ['ghp_123456']);
        expect(ok).toBe(false);
        expect(store.lastError?.code).toBe('db.setting_secret_rejected');
    });

    it('reage a eventos db://settings-changed e db://recovered', async () => {
        vi.mocked(dbIpc.getStatus).mockResolvedValue({ ok: true, data: readyStatus });
        vi.mocked(dbIpc.getSettings).mockResolvedValue({ ok: true, data: {} });

        const store = useSettingsStore();
        await store.hydrate();

        expect(changedCallback).toBeTypeOf('function');
        changedCallback?.({
            key: 'ui.theme',
            value: 'max-light',
            revision: 7,
            origin: 'sync'
        });

        expect(store.get('ui.theme')).toBe('max-light');
        expect(store.revisions['ui.theme']).toBe(7);

        expect(recoveredCallback).toBeTypeOf('function');
        recoveredCallback?.({
            strategy: 'reset_database',
            quarantinedPath: '/tmp/corrupted.db',
            recoveredAt: 3000
        });
        expect(dbIpc.getStatus).toHaveBeenCalledTimes(2);

        store.dispose();
        expect(changedCallback).toBeNull();
        expect(recoveredCallback).toBeNull();
    });

    it('renderiza DbRecoveryPanel quando recovery_required e dispara as 3 ações de recuperação', async () => {
        vi.mocked(dbIpc.getStatus).mockResolvedValue({ ok: true, data: recoveryStatus });
        vi.mocked(dbIpc.getSettings).mockResolvedValue({ ok: true, data: {} });

        const store = useSettingsStore();
        await store.hydrate();

        const wrapper = mount(DbRecoveryPanel);
        expect(wrapper.find('.db-recovery-panel').exists()).toBe(true);
        expect(wrapper.text()).toContain('db.corrupted');
        expect(wrapper.text()).toContain('32.0 KiB');
        expect(wrapper.text()).toContain('512 B');

        // 1. Falha ao tentar reabrir exibe alerta de erro
        vi.mocked(dbIpc.recover).mockResolvedValueOnce({
            ok: false,
            error: new MtIpcError(
                {
                    code: 'db.corrupted',
                    message: 'Banco continua corrompido.'
                },
                'db_recover'
            )
        });

        const buttons = wrapper.findAll('button');
        await buttons[0]!.trigger('click');
        await flushPromises();
        expect(dbIpc.recover).toHaveBeenCalledWith({ action: 'retry_open' });
        expect(wrapper.find('[role="alert"]').text()).toContain('inconsistências');

        // 2. Restaurar backup selecionado
        vi.mocked(dbIpc.recover).mockResolvedValueOnce({
            ok: true,
            data: recoveryStatus
        });
        await buttons[1]!.trigger('click');
        await flushPromises();
        expect(dbIpc.recover).toHaveBeenCalledWith({
            action: 'restore_from_backup',
            backup_file_name: 'maxterminal-20260417T153045Z-pre_migration.db'
        });

        // 3. Recriar banco limpo transiciona para ready e oculta painel
        vi.mocked(dbIpc.recover).mockResolvedValueOnce({
            ok: true,
            data: readyStatus
        });
        vi.mocked(dbIpc.getStatus).mockResolvedValueOnce({
            ok: true,
            data: readyStatus
        });
        await buttons[2]!.trigger('click');
        await flushPromises();
        expect(dbIpc.recover).toHaveBeenCalledWith({ action: 'reset_database' });
        expect(store.isRecoveryRequired).toBe(false);
    });
});
