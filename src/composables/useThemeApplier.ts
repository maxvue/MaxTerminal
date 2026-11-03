import { onMounted, onUnmounted, watch } from 'vue';
import { useProjectsStore } from '@/stores/projects';
import { useThemeStore } from '@/stores/theme';

/**
 * Composable raiz montado em `App.vue` que:
 * 1. Hidrata `useThemeStore` no boot;
 * 2. Observa `prefers-color-scheme: dark` quando `followSystem === true`;
 * 3. Sincroniza o tema efetivo quando o projeto selecionado em `useProjectsStore` muda.
 */
export function useThemeApplier(): { themeStore: ReturnType<typeof useThemeStore> } {
    const themeStore = useThemeStore();
    const projectsStore = useProjectsStore();

    let mediaQuery: MediaQueryList | null = null;

    const handleSystemColorSchemeChange = (ev: MediaQueryListEvent | MediaQueryList): void => {
        if (!themeStore.appearance.followSystem) {
            return;
        }
        const targetId = ev.matches
            ? themeStore.appearance.preferredDarkThemeId
            : themeStore.appearance.preferredLightThemeId;
        if (targetId && targetId !== themeStore.appearance.activeThemeId) {
            void themeStore.setActiveTheme(targetId, { kind: 'global' });
        }
    };

    watch(
        () => projectsStore.selectedId,
        (projectId) => {
            void themeStore.setCurrentProjectContext(projectId ?? null);
        },
        { immediate: true }
    );

    watch(
        () => themeStore.appearance?.followSystem ?? false,
        (enabled) => {
            if (enabled && mediaQuery) {
                handleSystemColorSchemeChange(mediaQuery);
            }
        }
    );

    onMounted(() => {
        void themeStore.init().catch(() => {});
        if (typeof window !== 'undefined' && typeof window.matchMedia === 'function') {
            mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
            mediaQuery.addEventListener('change', handleSystemColorSchemeChange);
        }
    });

    onUnmounted(() => {
        if (mediaQuery) {
            mediaQuery.removeEventListener('change', handleSystemColorSchemeChange);
            mediaQuery = null;
        }
    });

    return {
        themeStore
    };
}
