import type { AppInfo, FrontendLogEvent } from '@/types/bindings';
import { commands } from '@/types/bindings';
import { call } from './core';

/** Obtém os metadados estáticos e diretórios XDG do MaxTerminal. */
export async function getAppInfo(): Promise<AppInfo> {
    return call('app_info', () => commands.appInfo());
}

/** Envia um evento estático enumerado de erro do frontend para o log estruturado nativo. */
export async function logFrontend(event: FrontendLogEvent): Promise<void> {
    await call('app_log_frontend', () => commands.appLogFrontend(event));
}
