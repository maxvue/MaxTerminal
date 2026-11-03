import type { UnlistenFn } from '@tauri-apps/api/event';
import {
    commands,
    events,
    type DbMaintenanceFinished,
    type DbRecovered,
    type DbRecoveryAction,
    type DbStatusDto,
    type DbWriterLag,
    type SettingsChanged,
    type SettingSnapshot,
    type Value as JsonValue
} from '@/types/bindings';
import { call, isMtIpcError, type IpcResult, type MtIpcError } from './core';

/** Resultado discriminado retornado pelos helpers IPC do domínio `db`. */
export type DbCallResult<T> = { ok: true; data: T } | { ok: false; error: MtIpcError };

async function callDb<T>(
    command: string,
    request: () => Promise<IpcResult<T>>
): Promise<DbCallResult<T>> {
    try {
        const data = await call(command, request);
        return { ok: true, data };
    } catch (err: unknown) {
        if (isMtIpcError(err)) {
            return { ok: false, error: err };
        }
        throw err;
    }
}

/**
 * Lê um mapa de configurações tipadas do banco (`null` retorna todas as chaves registradas).
 */
export function getSettings(
    keys?: string[]
): Promise<DbCallResult<Partial<Record<string, JsonValue>>>> {
    return callDb('db_settings_get', () => commands.dbSettingsGet(keys ?? null));
}

/**
 * Persiste uma chave de configuração validando tipo, bloqueando segredos e checando revisão otimista.
 */
export function setSetting(
    key: string,
    value: JsonValue,
    expectedRevision?: number | null
): Promise<DbCallResult<SettingSnapshot>> {
    return callDb('db_settings_set', () =>
        commands.dbSettingsSet(key, value, expectedRevision ?? null)
    );
}

/**
 * Lê a fotografia completa (`key`, `value`, `revision`, `updatedAt`) de uma única configuração.
 */
export function getSettingSnapshot(key: string): Promise<DbCallResult<SettingSnapshot>> {
    return callDb('db_setting_snapshot', () => commands.dbSettingSnapshot(key));
}

/**
 * Consulta o estado operacional (`ready` ou `recovery_required`) e métricas do `BatchWriter`.
 */
export function getStatus(): Promise<DbCallResult<DbStatusDto>> {
    return callDb('db_status', () => commands.dbStatus());
}

/**
 * Executa uma ação explícita de recuperação do banco de dados (`retry_open`, `restore_from_backup`, `reset_database`).
 */
export function recover(action: DbRecoveryAction): Promise<DbCallResult<DbStatusDto>> {
    return callDb('db_recover', () => commands.dbRecover(action));
}

/**
 * Registra listener para o evento `db://settings-changed`.
 */
export function onSettingsChanged(handler: (ev: SettingsChanged) => void): Promise<UnlistenFn> {
    return events.dbSettingsChanged.listen((e) => handler(e.payload));
}

/**
 * Registra listener para o evento `db://recovered`.
 */
export function onDbRecovered(handler: (ev: DbRecovered) => void): Promise<UnlistenFn> {
    return events.dbRecovered.listen((e) => handler(e.payload));
}

/**
 * Registra listener para o evento `db://maintenance-finished`.
 */
export function onMaintenanceFinished(handler: (ev: DbMaintenanceFinished) => void): Promise<UnlistenFn> {
    return events.dbMaintenanceFinished.listen((e) => handler(e.payload));
}

/**
 * Registra listener para o evento `db://writer-lag`.
 */
export function onWriterLag(handler: (ev: DbWriterLag) => void): Promise<UnlistenFn> {
    return events.dbWriterLag.listen((e) => handler(e.payload));
}
