import type { Component } from 'vue';

/** Definição de uma seção plugável do diálogo de preferências (`PreferencesDialog.vue`). */
export interface PreferencesSectionRegistration {
    /** Identificador único da seção (ex.: `'system'`). */
    id: string;
    /** Título visível na barra lateral em `pt-BR`. */
    title: string;
    /** Ícone Iconify (`ex.: 'lucide:monitor-cog'`). */
    icon: string;
    /** Ordem numérica crescente de exibição. */
    order: number;
    /** Componente Vue renderizado quando a seção está ativa. */
    component: Component;
}

const sectionsMap = new Map<string, PreferencesSectionRegistration>();

/** Registra ou atualiza uma seção no diálogo de preferências. */
export function registerPreferencesSection(section: PreferencesSectionRegistration): void {
    sectionsMap.set(section.id, section);
}

/** Lista todas as seções registradas ordenadas por `order` crescente. */
export function listPreferencesSections(): PreferencesSectionRegistration[] {
    return Array.from(sectionsMap.values()).sort((a, b) => a.order - b.order);
}

/** Limpa o registro de seções (uso exclusivo em testes). */
export function clearPreferencesSectionsForTests(): void {
    sectionsMap.clear();
}
