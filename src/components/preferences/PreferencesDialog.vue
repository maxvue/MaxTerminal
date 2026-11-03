<template>
    <MaxModal :visible="windowStore.preferencesOpen" title="Configurações gerais" :no-button="true" width="100vw" max-width="100vw" max-height="100vh" height="100vh" class="prefs-dialog-modal" :dismissable="true" :close-on-escape="true" @update:visible="onVisibleUpdate">
        <div class="prefs-dialog__body">
            <nav class="prefs-dialog__nav" aria-label="Seções de preferências">
                <MaxButton v-for="sec in sections" :key="sec.id" type="button" :class="activeSectionId === sec.id ? 'prefs-dialog__nav-btn is-active' : 'prefs-dialog__nav-btn'" variant="text" @click="selectSection(sec.id)">
                    <MaxIcon :icon="sec.icon" />
                    <span>{{ sec.title }}</span>
                </MaxButton>
            </nav>
            <div class="prefs-dialog__content">
                <component :is="activeSection.component" v-if="activeSection" />
            </div>
        </div>
    </MaxModal>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import MaxButton from '@maxvue/max-components-ui/components/MaxButton';
    import MaxModal from '@maxvue/max-components-ui/components/MaxModal';
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

    function onVisibleUpdate(val: boolean): void {
        if (!val) {
            windowStore.closePreferences();
        }
    }
</script>

<style lang="scss" scoped>
    :deep(.max-modal.prefs-dialog-modal),
    :deep(.prefs-dialog-modal.max-modal),
    :deep(.prefs-dialog-modal) {
        background-color: var(--mt-ui-surface, var(--background-950, var(--background-0)));
        border-color: var(--mt-ui-border, var(--background-700, var(--background-100)));
        color: var(--mt-ui-text, var(--background-100, var(--background-800)));
        max-height: 85vh;
        display: flex;
        flex-direction: column;

        .max-modal-content {
            padding: 0;
            flex: 1;
            min-height: 0;
            overflow: hidden;
            display: flex;
            flex-direction: column;
        }
    }

    .prefs-dialog__body {
        display: grid;
        grid-template-columns: 13rem 1fr;
        flex: 1;
        min-height: 28rem;
        height: 100%;
        overflow: hidden;
    }

    .prefs-dialog__nav {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
        padding: 0.75rem;
        background: var(--mt-ui-surface-panel, var(--background-25));
        border-right: 1px solid var(--mt-ui-border, var(--background-100));
        overflow-y: auto;
    }

    .prefs-dialog__nav-btn {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        width: 100%;
        justify-content: flex-start;
        padding: 0.5rem 0.625rem;
        border: none;
        border-radius: 0.375rem;
        background: transparent;
        color: var(--mt-ui-text-muted, var(--background-400));
        font-size: 0.8125rem;
        font-weight: 500;
        text-align: left;
        cursor: pointer;

        :deep(.max-button-label) {
            display: inline-flex;
            align-items: center;
            justify-content: flex-start;
            gap: 0.5rem;
            width: 100%;
            text-align: left;
        }

        &:hover {
            background: var(--mt-ui-surface-hover, var(--background-75));
            color: var(--mt-ui-text-strong, var(--background-950));
        }

        &.is-active {
            background: color-mix(in srgb, var(--max-primary-500) 18%, transparent) !important;
            color: var(--max-primary-400) !important;
            font-weight: 600;
            border-color: transparent !important;
        }
    }

    .prefs-dialog__content {
        padding: 1rem 1.25rem;
        overflow-y: auto;
        min-width: 0;
    }
</style>
