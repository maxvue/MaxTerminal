<template>
    <div v-if="windowStore.preferencesOpen" class="prefs-dialog-backdrop" role="dialog" aria-modal="true" aria-label="Preferências do MaxTerminal" @keydown.esc="onClose" @mousedown.self="onClose">
        <div class="prefs-dialog">
            <header class="prefs-dialog__header">
                <h2 class="prefs-dialog__title">Preferências do MaxTerminal</h2>
                <button type="button" class="prefs-dialog__close" aria-label="Fechar preferências" @click="onClose">
                    <MaxIcon icon="lucide:x" />
                </button>
            </header>
            <div class="prefs-dialog__body">
                <nav class="prefs-dialog__nav" aria-label="Seções de preferências">
                    <button v-for="sec in sections" :key="sec.id" type="button" class="prefs-dialog__nav-item" :class="{ 'is-active': activeSectionId === sec.id }" @click="selectSection(sec.id)">
                        <MaxIcon :icon="sec.icon" />
                        <span>{{ sec.title }}</span>
                    </button>
                </nav>
                <div class="prefs-dialog__content">
                    <component :is="activeSection.component" v-if="activeSection" />
                </div>
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import {
        listPreferencesSections,
        registerPreferencesSection
    } from '@/components/preferences/registry';
    import AgentSettingsPanel from '@/components/agents/AgentSettingsPanel.vue';
    import BridgeSettingsSection from '@/components/bridge/BridgeSettingsSection.vue';
    import AiPreferences from '@/components/preferences/AiPreferences.vue';
    import AppearancePreferences from '@/components/preferences/appearance/AppearancePreferences.vue';
    import SkillsAndMemoryPreferences from '@/components/preferences/SkillsAndMemoryPreferences.vue';
    import PreferencesSystemSection from '@/components/preferences/system/PreferencesSystemSection.vue';
    import { useWindowStore } from '@/stores/window';

    registerPreferencesSection({
        id: 'system',
        title: 'Sistema & SO',
        icon: 'lucide:monitor-cog',
        order: 10,
        component: PreferencesSystemSection
    });

    registerPreferencesSection({
        id: 'appearance',
        title: 'Aparência & Temas',
        icon: 'lucide:palette',
        order: 20,
        component: AppearancePreferences
    });

    registerPreferencesSection({
        id: 'ai',
        title: 'Inteligência Artificial',
        icon: 'lucide:sparkles',
        order: 25,
        component: AiPreferences
    });

    registerPreferencesSection({
        id: 'agents',
        title: 'Agentes CLI',
        icon: 'lucide:bot',
        order: 26,
        component: AgentSettingsPanel
    });

    registerPreferencesSection({
        id: 'skills',
        title: 'MaxSkills & Memória',
        icon: 'lucide:book-marked',
        order: 28,
        component: SkillsAndMemoryPreferences
    });

    registerPreferencesSection({
        id: 'bridge',
        title: 'Integração MaxCode',
        icon: 'lucide:cable',
        order: 30,
        component: BridgeSettingsSection
    });

    const windowStore = useWindowStore();

    const sections = computed(() => listPreferencesSections());
    const activeSectionId = computed<string>(() => windowStore.preferencesSection || 'system');
    const activeSection = computed(
        () =>
            sections.value.find((s) => s.id === activeSectionId.value) ??
            sections.value[0] ??
            null
    );

    function selectSection(sectionId: string): void {
        windowStore.openPreferences(sectionId);
    }

    function onClose(): void {
        windowStore.closePreferences();
    }
</script>

<style lang="scss" scoped>
    .prefs-dialog-backdrop {
        position: fixed;
        inset: 0;
        z-index: 60;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 1.5rem;
        background: rgb(0 0 0 / 60%);
    }

    .prefs-dialog {
        display: flex;
        flex-direction: column;
        width: min(56rem, 100%);
        max-height: 85vh;
        border-radius: 0.625rem;
        border: 1px solid var(--background-700);
        background: var(--background-950);
        color: var(--background-100);
        overflow: hidden;
    }

    .prefs-dialog__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 0.75rem 1rem;
        background: var(--background-900);
        border-bottom: 1px solid var(--background-800);
    }

    .prefs-dialog__title {
        margin: 0;
        font-size: 0.9375rem;
        font-weight: 600;
        color: var(--background-50);
    }

    .prefs-dialog__close {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 1.75rem;
        height: 1.75rem;
        border: none;
        border-radius: 0.375rem;
        background: transparent;
        color: var(--background-300);
        cursor: pointer;

        &:hover {
            background: var(--background-800);
            color: var(--background-50);
        }
    }

    .prefs-dialog__body {
        display: grid;
        grid-template-columns: 13rem 1fr;
        flex: 1;
        min-height: 0;
        overflow: hidden;
    }

    .prefs-dialog__nav {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
        padding: 0.75rem;
        background: var(--background-900);
        border-right: 1px solid var(--background-800);
        overflow-y: auto;
    }

    .prefs-dialog__nav-item {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        padding: 0.5rem 0.625rem;
        border: none;
        border-radius: 0.375rem;
        background: transparent;
        color: var(--background-300);
        font-size: 0.8125rem;
        font-weight: 500;
        text-align: left;
        cursor: pointer;

        &:hover {
            background: var(--background-800);
            color: var(--background-100);
        }

        &.is-active {
            background: color-mix(in srgb, var(--max-primary-500) 18%, transparent);
            color: var(--max-primary-400);
            font-weight: 600;
        }
    }

    .prefs-dialog__content {
        padding: 1rem 1.25rem;
        overflow-y: auto;
    }
</style>
