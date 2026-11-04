import type { WorkspaceSelection } from './workspaceSelection';

/** Esquema tipado das configurações introduzidas na Fase 06 e estendidas com persistência de workspace. */
export interface ProjectSettingsSchema {
    /** Identificador do projeto atualmente selecionado na coluna esquerda (`null` quando nenhum). */
    'projects.selected_id': string | null;
    /** Largura percentual da coluna esquerda de projetos (`15..=45`, padrão `22`). */
    'layout.columns.left_size': number;
    /** Lista de identificadores de projetos ocultos da lista principal na coluna esquerda. */
    'ui.projects.hidden_ids': string[];
    /** Seleção completa do workspace persistida duravelmente (projeto ativo e abas visíveis). */
    'ui.workspace.selection': WorkspaceSelection | null;
}

/** Valores padrão imutáveis das configurações da Fase 06 e workspace. */
export const PROJECT_SETTINGS_DEFAULTS: Readonly<ProjectSettingsSchema> = Object.freeze({
    'projects.selected_id': null,
    'layout.columns.left_size': 22,
    'ui.projects.hidden_ids': [],
    'ui.workspace.selection': null
});

/** Limite mínimo permitido para a coluna esquerda (%). */
export const LAYOUT_LEFT_MIN_SIZE = 15;

/** Limite máximo permitido para a coluna esquerda (%). */
export const LAYOUT_LEFT_MAX_SIZE = 45;

/** Largura padrão da coluna esquerda (%). */
export const LAYOUT_LEFT_DEFAULT_SIZE = 22;
