import type { UnlistenFn } from '@tauri-apps/api/event';
import { defineStore } from 'pinia';
import { computed, ref } from 'vue';
import { isMtIpcError, osIpc } from '@/ipc';
import type {
    AutostartStatus_Serialize,
    CloseBehavior,
    EditorKind,
    NotificationSettings_Serialize,
    OsSettingsPatch_Deserialize,
    OsSettings_Serialize,
    QuakeSettings_Serialize,
    ShortcutStatus_Serialize
} from '@/types/bindings';

function cloneSettings(src: OsSettings_Serialize): OsSettings_Serialize {
    return {
        schemaVersion: src.schemaVersion,
        closeBehavior: src.closeBehavior,
        quake: { ...src.quake },
        editor: {
            kind: src.editor.kind,
            customArgv: [...src.editor.customArgv]
        },
        notifications: { ...src.notifications },
        autostart: src.autostart,
        startMinimized: src.startMinimized
    };
}

function mergePatchOptimistically(
    base: OsSettings_Serialize,
    patch: OsSettingsPatch_Deserialize
): OsSettings_Serialize {
    const next = cloneSettings(base);
    if (patch.closeBehavior !== undefined && patch.closeBehavior !== null) {
        next.closeBehavior = patch.closeBehavior;
    }
    if (patch.quake) {
        const q = patch.quake;
        if (q.enabled !== undefined && q.enabled !== null) {
            next.quake.enabled = q.enabled;
        }
        if (q.accelerator !== undefined && q.accelerator !== null) {
            next.quake.accelerator = q.accelerator;
        }
        if (q.mode !== undefined && q.mode !== null) {
            next.quake.mode = q.mode;
        }
        if (q.dropDownHeightPct !== undefined && q.dropDownHeightPct !== null) {
            next.quake.dropDownHeightPct = q.dropDownHeightPct;
        }
        if (q.alwaysOnTop !== undefined && q.alwaysOnTop !== null) {
            next.quake.alwaysOnTop = q.alwaysOnTop;
        }
        if (q.hideOnBlur !== undefined && q.hideOnBlur !== null) {
            next.quake.hideOnBlur = q.hideOnBlur;
        }
        if (q.preferredBackend !== undefined && q.preferredBackend !== null) {
            next.quake.preferredBackend = q.preferredBackend;
        }
    }
    if (patch.editor) {
        if (patch.editor.kind !== undefined) {
            next.editor.kind = patch.editor.kind;
        }
        if (patch.editor.customArgv !== undefined) {
            next.editor.customArgv = [...patch.editor.customArgv];
        }
    }
    if (patch.notifications) {
        const n = patch.notifications;
        if (n.enabled !== undefined && n.enabled !== null) {
            next.notifications.enabled = n.enabled;
        }
        if (n.longCommandEnabled !== undefined && n.longCommandEnabled !== null) {
            next.notifications.longCommandEnabled = n.longCommandEnabled;
        }
        if (n.longCommandThresholdSecs !== undefined && n.longCommandThresholdSecs !== null) {
            next.notifications.longCommandThresholdSecs = n.longCommandThresholdSecs;
        }
        if (n.onlyWhenUnfocused !== undefined && n.onlyWhenUnfocused !== null) {
            next.notifications.onlyWhenUnfocused = n.onlyWhenUnfocused;
        }
    }
    if (patch.autostart !== undefined && patch.autostart !== null) {
        next.autostart = patch.autostart;
    }
    if (patch.startMinimized !== undefined && patch.startMinimized !== null) {
        next.startMinimized = patch.startMinimized;
    }
    return next;
}

function extractErrorMessage(err: unknown): string {
    if (isMtIpcError(err)) {
        return err.message;
    }
    if (err instanceof Error) {
        return err.message;
    }
    return 'Não foi possível salvar as preferências do sistema.';
}

/**
 * Store reativa das preferências de integração com o sistema operacional (`os_settings`).
 * Realiza atualização otimista com rollback automático em caso de rejeição do backend.
 */
export const useOsSettingsStore = defineStore('osSettings', () => {
    const settings = ref<OsSettings_Serialize | null>(null);
    const saving = ref<boolean>(false);
    const error = ref<string | null>(null);
    const autostart = ref<AutostartStatus_Serialize | null>(null);
    const lastRevision = ref<number>(0);

    let unlistenSettings: UnlistenFn | null = null;
    let loadPromise: Promise<void> | null = null;

    const editorKind = computed<EditorKind>(() => settings.value?.editor.kind ?? 'vs_code');
    const quake = computed<QuakeSettings_Serialize | null>(() => settings.value?.quake ?? null);
    const closeBehavior = computed<CloseBehavior>(
        () => settings.value?.closeBehavior ?? 'auto'
    );
    const notifications = computed<NotificationSettings_Serialize | null>(
        () => settings.value?.notifications ?? null
    );

    async function ensureSubscribed(): Promise<void> {
        if (unlistenSettings) {
            return;
        }
        unlistenSettings = await osIpc.onSettingsChanged((ev) => {
            if (ev.revision <= lastRevision.value) {
                return;
            }
            lastRevision.value = ev.revision;
            settings.value = ev.payload;
        });
    }

    async function load(): Promise<void> {
        if (loadPromise) {
            return loadPromise;
        }
        loadPromise = (async () => {
            try {
                await ensureSubscribed();
                const [loadedSettings, loadedAutostart] = await Promise.all([
                    osIpc.getSettings(),
                    osIpc.getAutostart()
                ]);
                settings.value = loadedSettings;
                autostart.value = loadedAutostart;
                error.value = null;
            } catch (err) {
                error.value = extractErrorMessage(err);
                throw err;
            } finally {
                loadPromise = null;
            }
        })();
        return loadPromise;
    }

    async function update(patch: OsSettingsPatch_Deserialize): Promise<OsSettings_Serialize> {
        const previous = settings.value ? cloneSettings(settings.value) : null;
        if (settings.value) {
            settings.value = mergePatchOptimistically(settings.value, patch);
        }
        saving.value = true;
        error.value = null;
        try {
            const updated = await osIpc.updateSettings(patch);
            settings.value = updated;
            if (patch.autostart !== undefined && patch.autostart !== null && autostart.value) {
                autostart.value = {
                    ...autostart.value,
                    enabled: updated.autostart
                };
            }
            return updated;
        } catch (err) {
            if (previous) {
                settings.value = previous;
            }
            error.value = extractErrorMessage(err);
            throw err;
        } finally {
            saving.value = false;
        }
    }

    async function setAccelerator(accelerator: string): Promise<OsSettings_Serialize> {
        return update({
            quake: { accelerator }
        });
    }

    async function setAutostart(enabled: boolean): Promise<AutostartStatus_Serialize> {
        const prevAutostart = autostart.value ? { ...autostart.value } : null;
        const prevSettings = settings.value ? cloneSettings(settings.value) : null;
        if (autostart.value) {
            autostart.value = { ...autostart.value, enabled };
        }
        if (settings.value) {
            settings.value = { ...settings.value, autostart: enabled };
        }
        saving.value = true;
        error.value = null;
        try {
            const status = await osIpc.setAutostart(enabled);
            autostart.value = status;
            if (settings.value) {
                settings.value = { ...settings.value, autostart: status.enabled };
            }
            return status;
        } catch (err) {
            if (prevAutostart) {
                autostart.value = prevAutostart;
            }
            if (prevSettings) {
                settings.value = prevSettings;
            }
            error.value = extractErrorMessage(err);
            throw err;
        } finally {
            saving.value = false;
        }
    }

    async function configurePortal(): Promise<ShortcutStatus_Serialize> {
        saving.value = true;
        error.value = null;
        try {
            return await osIpc.configurePortal();
        } catch (err) {
            error.value = extractErrorMessage(err);
            throw err;
        } finally {
            saving.value = false;
        }
    }

    function dispose(): void {
        if (unlistenSettings) {
            unlistenSettings();
            unlistenSettings = null;
        }
        loadPromise = null;
    }

    function $reset(): void {
        dispose();
        settings.value = null;
        saving.value = false;
        error.value = null;
        autostart.value = null;
        lastRevision.value = 0;
    }

    return {
        settings,
        saving,
        error,
        autostart,
        lastRevision,
        editorKind,
        quake,
        closeBehavior,
        notifications,
        load,
        update,
        setAccelerator,
        setAutostart,
        configurePortal,
        dispose,
        $reset
    };
});
