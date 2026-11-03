import { onMounted, onUnmounted, watch } from 'vue';
import { useProjectsStore } from '@/stores/projects';
import { useThemeStore } from '@/stores/theme';
import {
    applyNativeWindowTheme,
    resolveNativeWindowThemeTarget,
    type NativeWindowThemeTarget
} from '@/theme/nativeWindow';

export interface UseThemeApplierOptions {
    /** Permite forçar o estado de disponibilidade da integração nativa (útil para testes de fallback). */
    nativeIntegrationAvailable?: boolean | undefined;
}

/**
 * Composable raiz montado em `App.vue` que:
 * 1. Hidrata `useThemeStore` no boot;
 * 2. Observa `prefers-color-scheme: dark` quando `followSystem === true`;
 * 3. Sincroniza o tema efetivo quando o projeto selecionado em `useProjectsStore` muda;
 * 4. Sincroniza a janela nativa do Tauri respeitando a prioridade de cores nativas do SO (null) e fallback escuro quando aplicável.
 */
export function useThemeApplier(options?: UseThemeApplierOptions): {
    themeStore: ReturnType<typeof useThemeStore>;
    syncNativeWindowTheme: () => Promise<boolean>;
} {
    const themeStore = useThemeStore();
    const projectsStore = useProjectsStore();

    let mediaQuery: MediaQueryList | null = null;

    // Sincronização inicial com a polaridade de bootstrap já aplicada ao DOM
    const initialPolarity: 'dark' | 'light' =
        typeof document !== 'undefined' &&
        document.documentElement.dataset.themePolarity === 'light'
            ? 'light'
            : 'dark';

    const syncNativeWindowTheme = (): Promise<boolean> => {
        const polarity = themeStore.effectiveResolved?.uiTokens.polarity ?? initialPolarity;
        const target: NativeWindowThemeTarget = resolveNativeWindowThemeTarget({
            contentPolarity: polarity,
            nativeIntegrationAvailable: options?.nativeIntegrationAvailable
        });
        return applyNativeWindowTheme(target);
    };

    void syncNativeWindowTheme();

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
        () => themeStore.effectiveResolved?.uiTokens.polarity,
        () => {
            void syncNativeWindowTheme();
        },
        { immediate: true }
    );

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
        themeStore,
        syncNativeWindowTheme
    };
}
