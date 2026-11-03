import {
    PROJECT_SETTINGS_DEFAULTS,
    type ProjectSettingsSchema
} from './projectSettings';

/** Esquema tipado das configurações de fundação registradas na Fase 03 e estendidas na Fase 06. */
export interface SettingsSchema extends ProjectSettingsSchema {
    'ui.theme': 'max-navy-teal' | 'max-dark' | 'max-light';
    'ui.locale': 'pt-BR';
    'terminal.scrollbackLines': number;
    'terminal.fontFamily': string;
    'terminal.fontSize': number;
    'privacy.redactCustomPatterns': string[];
    'workspace.sessions.keep_per_tab': number;
    'workspace.sessions.max_age_days': number;
    'workspace.scrollback.limit_mib': number;
}

/** União de chaves válidas do registro de configurações. */
export type SettingKey = keyof SettingsSchema;

/** Tipo do valor associado a uma chave `K` de `SettingsSchema`. */
export type SettingValue<K extends SettingKey> = SettingsSchema[K];

/** Valores padrão imutáveis espelhados do `SettingsRegistry` em Rust. */
export const SETTINGS_DEFAULTS: Readonly<SettingsSchema> = Object.freeze({
    'ui.theme': 'max-navy-teal',
    'ui.locale': 'pt-BR',
    'terminal.scrollbackLines': 10_000,
    'terminal.fontFamily': 'JetBrains Mono',
    'terminal.fontSize': 13,
    'privacy.redactCustomPatterns': [],
    'workspace.sessions.keep_per_tab': 50,
    'workspace.sessions.max_age_days': 90,
    'workspace.scrollback.limit_mib': 8,
    ...PROJECT_SETTINGS_DEFAULTS
});
