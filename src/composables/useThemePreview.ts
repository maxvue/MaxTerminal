import { onUnmounted, ref, toValue, watch, type MaybeRefOrGetter } from 'vue';
import { useThemeStore } from '@/stores/theme';
import type { ThemeId } from '@/types/theme';

/**
 * Composable de pré-visualização efêmera (hover/foco) de esquemas de cores no `ThemePicker`,
 * revertendo automaticamente ao desmontar, sair do card ou suspender durante modais de edição/exclusão.
 */
export function useThemePreview(suspended?: MaybeRefOrGetter<boolean>) {
    const themeStore = useThemeStore();
    const previewingId = ref<ThemeId | null>(null);
    let hoverTimer: ReturnType<typeof setTimeout> | null = null;

    function isSuspended(): boolean {
        return Boolean(suspended !== undefined && toValue(suspended));
    }

    function clearTimer(): void {
        if (hoverTimer !== null) {
            clearTimeout(hoverTimer);
            hoverTimer = null;
        }
    }

    function previewOnHover(id: ThemeId, delayMs = 90): void {
        if (isSuspended()) {
            return;
        }
        clearTimer();
        hoverTimer = setTimeout(() => {
            if (isSuspended()) {
                return;
            }
            previewingId.value = id;
            void themeStore.startPreview(id);
        }, delayMs);
    }

    function stopPreview(): void {
        clearTimer();
        previewingId.value = null;
        themeStore.cancelPreview();
    }

    if (suspended !== undefined) {
        watch(
            () => toValue(suspended),
            (nowSuspended) => {
                if (nowSuspended) {
                    stopPreview();
                }
            }
        );
    }

    onUnmounted(() => {
        stopPreview();
    });

    return {
        previewingId,
        previewOnHover,
        stopPreview
    };
}
