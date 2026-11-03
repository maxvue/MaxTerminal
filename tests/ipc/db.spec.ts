import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';
import * as dbIpc from '@/ipc/db';
import { errorMessages } from '@/types/errors';

describe('ipc/db', () => {
    it('encaminha getSettings com keys nulas ou lista explícita', async () => {
        const seenArgs: Array<Record<string, unknown>> = [];
        mockIPC((cmd, args) => {
            if (cmd === 'db_settings_get') {
                seenArgs.push((args ?? {}) as Record<string, unknown>);
                return {
                    'ui.theme': 'max-navy-teal',
                    'terminal.fontSize': 14
                };
            }
            return null;
        });

        const allRes = await dbIpc.getSettings();
        expect(allRes.ok).toBe(true);
        if (allRes.ok) {
            expect(allRes.data['ui.theme']).toBe('max-navy-teal');
        }

        const subsetRes = await dbIpc.getSettings(['ui.theme']);
        expect(subsetRes.ok).toBe(true);
        expect(seenArgs[0]?.keys).toBeNull();
        expect(seenArgs[1]?.keys).toEqual(['ui.theme']);
    });

    it('encaminha setSetting, getSettingSnapshot, getStatus e recover com sucesso', async () => {
        mockIPC((cmd, args) => {
            const payload = (args ?? {}) as Record<string, unknown>;
            if (cmd === 'db_settings_set') {
                return {
                    key: payload.key,
                    value: payload.value,
                    revision: (typeof payload.expectedRevision === 'number' ? payload.expectedRevision : 0) + 1,
                    updatedAt: 1700000000000
                };
            }
            if (cmd === 'db_setting_snapshot') {
                return {
                    key: payload.key,
                    value: 'max-dark',
                    revision: 3,
                    updatedAt: 1700000000100
                };
            }
            if (cmd === 'db_status') {
                return {
                    health: {
                        state: 'ready',
                        schema_version: 1,
                        sqlite_version: '3.51.3'
                    },
                    pendingBytes: 0,
                    pendingRecords: 0,
                    writerLagEvents: 0
                };
            }
            if (cmd === 'db_recover') {
                return {
                    health: {
                        state: 'ready',
                        schema_version: 1,
                        sqlite_version: '3.51.3'
                    },
                    pendingBytes: 0,
                    pendingRecords: 0,
                    writerLagEvents: 0
                };
            }
            return null;
        });

        const setRes = await dbIpc.setSetting('ui.theme', 'max-dark', 2);
        expect(setRes.ok).toBe(true);
        if (setRes.ok) {
            expect(setRes.data.revision).toBe(3);
            expect(setRes.data.value).toBe('max-dark');
        }

        const snapRes = await dbIpc.getSettingSnapshot('ui.theme');
        expect(snapRes.ok).toBe(true);
        if (snapRes.ok) {
            expect(snapRes.data.revision).toBe(3);
        }

        const statusRes = await dbIpc.getStatus();
        expect(statusRes.ok).toBe(true);
        if (statusRes.ok) {
            expect(statusRes.data.health.state).toBe('ready');
        }

        const recoverRes = await dbIpc.recover({ action: 'retry_open' });
        expect(recoverRes.ok).toBe(true);
    });

    it('propaga MtError estruturado em falhas de comandos db_*', async () => {
        mockIPC((cmd) => {
            if (cmd === 'db_settings_set') {
                throw {
                    code: 'db.setting_secret_rejected',
                    message: 'Credenciais e segredos não podem ser salvos.',
                    details: { key: 'privacy.redactCustomPatterns' }
                };
            }
            return null;
        });

        const res = await dbIpc.setSetting('privacy.redactCustomPatterns', ['ghp_secret']);
        expect(res.ok).toBe(false);
        if (!res.ok) {
            expect(res.error.code).toBe('db.setting_secret_rejected');
            expect(res.error.details).toEqual({ key: 'privacy.redactCustomPatterns' });
        }
    });

    it('registra listeners tipados para os 4 eventos db://*', async () => {
        mockIPC((cmd) => {
            if (cmd === 'plugin:event|listen') {
                return 1;
            }
            if (cmd === 'plugin:event|unlisten') {
                return null;
            }
            return null;
        });

        const unlisten1 = await dbIpc.onSettingsChanged(vi.fn());
        const unlisten2 = await dbIpc.onDbRecovered(vi.fn());
        const unlisten3 = await dbIpc.onMaintenanceFinished(vi.fn());
        const unlisten4 = await dbIpc.onWriterLag(vi.fn());

        expect(typeof unlisten1).toBe('function');
        expect(typeof unlisten2).toBe('function');
        expect(typeof unlisten3).toBe('function');
        expect(typeof unlisten4).toBe('function');
    });

    it('mapeia todos os 17 códigos de erro db.* em errorMessages (pt-BR)', () => {
        const expectedCodes = [
            'db.open_failed',
            'db.locked_by_other_instance',
            'db.corrupted',
            'db.migration_failed',
            'db.migration_checksum_mismatch',
            'db.schema_newer_than_app',
            'db.busy',
            'db.constraint_violation',
            'db.query_invalid',
            'db.cursor_invalid',
            'db.setting_unknown_key',
            'db.setting_invalid_value',
            'db.setting_secret_rejected',
            'db.setting_conflict',
            'db.backup_failed',
            'db.writer_closed',
            'db.recovery_required'
        ];

        for (const code of expectedCodes) {
            expect(errorMessages[code]).toBeDefined();
            expect(errorMessages[code]?.length).toBeGreaterThan(10);
        }
    });
});
