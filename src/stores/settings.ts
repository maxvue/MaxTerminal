import type { UnlistenFn } from '@tauri-apps/api/event';
import { defineStore } from 'pinia';
import { computed, ref, shallowRef } from 'vue';
import type { MtIpcError } from '@/ipc';
import * as dbIpc from '@/ipc/db';
import type { DbRecoveryAction, DbStatusDto, Value as JsonValue } from '@/types/bindings';
import {
    SETTINGS_DEFAULTS,
    type SettingKey,
    type SettingsSchema,
    type SettingValue
} from '@/types/settings';

export const useSettingsStore = defineStore('settings', () => {
    const values = ref<SettingsSchema>({ ...SETTINGS_DEFAULTS });
    const revisions = ref<Partial<Record<SettingKey, number>>>({});
    const hydrated = ref<boolean>(false);
    const loading = ref<boolean>(false);
    const dbStatus = shallowRef<DbStatusDto | null>(null);
    const lastError = shallowRef<MtIpcError | null>(null);

    let unlistenChanged: UnlistenFn | null = null;
    let unlistenRecovered: UnlistenFn | null = null;

    const isRecoveryRequired = computed<boolean>(
        () => dbStatus.value?.health.state === 'recovery_required'
    );

    async function ensureListeners(): Promise<void> {
        if (!unlistenChanged) {
            try {
                unlistenChanged = await dbIpc.onSettingsChanged((ev) => {
                    const key = ev.key as SettingKey;
                    if (key in SETTINGS_DEFAULTS) {
                        (values.value as Record<string, unknown>)[key] = ev.value;
                        revisions.value[key] = ev.revision;
                    }
                });
            } catch {
                // Ambiente de teste sem mock de plugin:event
            }
        }
        if (!unlistenRecovered) {
            try {
                unlistenRecovered = await dbIpc.onDbRecovered(() => {
                    void hydrate();
                });
            } catch {
                // Ambiente de teste sem mock de plugin:event
            }
        }
    }

    async function hydrate(): Promise<void> {
        loading.value = true;
        lastError.value = null;
        try {
            if (!unlistenChanged || !unlistenRecovered) {
                await ensureListeners();
            }

            const statusRes = await dbIpc.getStatus();
            if (!statusRes.ok) {
                lastError.value = statusRes.error;
                return;
            }
            if (!statusRes.data?.health) {
                return;
            }
            dbStatus.value = statusRes.data;

            if (statusRes.data.health.state === 'recovery_required') {
                return;
            }

            const allRes = await dbIpc.getSettings();
            if (!allRes.ok) {
                lastError.value = allRes.error;
                return;
            }
            if (!allRes.data || typeof allRes.data !== 'object') {
                return;
            }

            const merged: Record<string, unknown> = { ...SETTINGS_DEFAULTS };
            for (const [k, v] of Object.entries(allRes.data)) {
                if (k in SETTINGS_DEFAULTS && v !== undefined) {
                    merged[k] = v;
                }
            }
            values.value = merged as unknown as SettingsSchema;
            hydrated.value = true;
        } finally {
            loading.value = false;
        }
    }

    function get<K extends SettingKey>(key: K): SettingValue<K> {
        return values.value[key];
    }

    async function set<K extends SettingKey>(
        key: K,
        value: SettingValue<K>
    ): Promise<boolean> {
        const expected = revisions.value[key] ?? null;
        const res = await dbIpc.setSetting(key, value as unknown as JsonValue, expected);

        if (res.ok) {
            (values.value as Record<string, unknown>)[key] = res.data.value;
            revisions.value[key] = res.data.revision;
            lastError.value = null;
            return true;
        }

        if (res.error.code === 'db.setting_conflict') {
            const snap = await dbIpc.getSettingSnapshot(key);
            if (snap.ok) {
                (values.value as Record<string, unknown>)[key] = snap.data.value;
                revisions.value[key] = snap.data.revision;
                const retry = await dbIpc.setSetting(
                    key,
                    value as unknown as JsonValue,
                    snap.data.revision
                );
                if (retry.ok) {
                    (values.value as Record<string, unknown>)[key] = retry.data.value;
                    revisions.value[key] = retry.data.revision;
                    lastError.value = null;
                    return true;
                }
                lastError.value = retry.error;
                return false;
            }
        }

        lastError.value = res.error;
        return false;
    }

    async function recover(action: DbRecoveryAction): Promise<boolean> {
        loading.value = true;
        lastError.value = null;
        try {
            const res = await dbIpc.recover(action);
            if (!res.ok) {
                lastError.value = res.error;
                return false;
            }
            dbStatus.value = res.data;
            if (res.data.health.state === 'ready') {
                await hydrate();
            }
            return res.data.health.state === 'ready';
        } finally {
            loading.value = false;
        }
    }

    function dispose(): void {
        unlistenChanged?.();
        unlistenChanged = null;
        unlistenRecovered?.();
        unlistenRecovered = null;
    }

    return {
        values,
        revisions,
        hydrated,
        loading,
        dbStatus,
        lastError,
        isRecoveryRequired,
        hydrate,
        get,
        set,
        recover,
        dispose
    };
});
