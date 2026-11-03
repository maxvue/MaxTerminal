import { onUnmounted, ref } from 'vue';
import { useThemeStore } from '@/stores/theme';
import type { ThemeId } from '@/types/theme';

/**
 * Composable de pré-visualização efêmera (hover/foco) de esquemas de cores no `ThemePicker`,
 * revertendo automaticamente ao desmontar ou sair do card.
 */
export function useThemePreview() {
    const themeStore = useThemeStore();
    const previewingId = ref<ThemeId | null>(null);
    let hoverTimer: ReturnType<typeof setTimeout> | null = null;

    function clearTimer(): void {
        if (hoverTimer !== null) {
            clearTimeout(hoverTimer);
            hoverTimer = null;
        }
    }

    function previewOnHover(id: ThemeId, delayMs = 90): void {
        clearTimer();
        hoverTimer = setTimeout(() => {
            previewingId.value = id;
            void themeStore.startPreview(id);
        }, delayMs);
    }

    function stopPreview(): void {
        clearTimer();
        previewingId.value = null;
        themeStore.cancelPreview();
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
