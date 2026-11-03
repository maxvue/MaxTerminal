<template>
    <div class="wayland-shortcut-help">
        <div class="wayland-shortcut-help__header">
            <span class="wayland-shortcut-help__badge">Wayland / Compositor</span>
            <span class="wayland-shortcut-help__status">{{ backendStatusText }}</span>
        </div>
        <p class="wayland-shortcut-help__desc">
            Em sessões Wayland (KDE Plasma, Hyprland, Sway, GNOME), atalhos globais são gerenciados pelo Portal XDG ou diretamente pelas regras de keybind do compositor.
        </p>
        <div v-if="props.portalAvailable" class="wayland-shortcut-help__portal-row">
            <button type="button" class="wayland-shortcut-help__btn" :disabled="props.busy" @click="emit('configure-portal')">
                Configurar no Portal do Sistema
            </button>
            <span v-if="props.shortcutStatus?.effectiveTrigger" class="wayland-shortcut-help__trigger">
                Gatilho ativo no portal: <code>{{ props.shortcutStatus.effectiveTrigger }}</code>
            </span>
        </div>
        <div class="wayland-shortcut-help__cli">
            <div class="wayland-shortcut-help__cli-top">
                <span class="wayland-shortcut-help__cli-label">Comando CLI universal (fallback garantido):</span>
                <button type="button" class="wayland-shortcut-help__copy-btn" @click="copyCommand">
                    {{ copied ? 'Copiado!' : 'Copiar comando' }}
                </button>
            </div>
            <pre class="wayland-shortcut-help__code"><code>{{ snippetText }}</code></pre>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed, ref } from 'vue';
    import type { ShortcutStatus_Serialize } from '@/types/bindings';

    export interface WaylandShortcutHelpProps {
        shortcutStatus: ShortcutStatus_Serialize | null;
        portalAvailable?: boolean;
        busy?: boolean;
    }

    const props = withDefaults(defineProps<WaylandShortcutHelpProps>(), {
        portalAvailable: false,
        busy: false
    });

    const emit = defineEmits<{
        'configure-portal': [];
    }>();

    const copied = ref<boolean>(false);

    const backendStatusText = computed<string>(() => {
        const st = props.shortcutStatus;
        if (!st) {
            return 'Verificando suporte do compositor…';
        }
        if (st.backend === 'portal' && st.registered) {
            return 'Portal XDG GlobalShortcuts ativo';
        }
        if (st.backend === 'plugin' && st.registered) {
            return 'Captura direta X11 ativa';
        }
        return 'Modo CLI para keybind do compositor';
    });

    const snippetText = computed<string>(() => {
        const st = props.shortcutStatus;
        if (st?.compositorHint) {
            return st.compositorHint;
        }
        return st?.cliCommand ?? 'maxterminal --toggle';
    });

    async function copyCommand(): Promise<void> {
        const text = snippetText.value;
        try {
            if (typeof navigator !== 'undefined' && navigator.clipboard) {
                await navigator.clipboard.writeText(text);
                copied.value = true;
                setTimeout(() => {
                    copied.value = false;
                }, 2000);
            }
        } catch {
            // Ignora se clipboard não estiver disponível no ambiente de teste.
        }
    }
</script>

<style lang="scss" scoped>
    .wayland-shortcut-help {
        display: flex;
        flex-direction: column;
        gap: 0.625rem;
        padding: 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-700);
        background: var(--background-900);
    }

    .wayland-shortcut-help__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
    }

    .wayland-shortcut-help__badge {
        padding: 0.125rem 0.5rem;
        border-radius: 0.25rem;
        background: color-mix(in srgb, var(--max-info-500) 18%, transparent);
        color: var(--max-info-400);
        font-size: 0.6875rem;
        font-weight: 600;
    }

    .wayland-shortcut-help__status {
        font-size: 0.75rem;
        color: var(--background-300);
    }

    .wayland-shortcut-help__desc {
        margin: 0;
        font-size: 0.75rem;
        color: var(--background-400);
    }

    .wayland-shortcut-help__portal-row {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 0.625rem;
    }

    .wayland-shortcut-help__btn {
        height: 2rem;
        padding: 0 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--max-primary-500);
        background: color-mix(in srgb, var(--max-primary-500) 16%, transparent);
        color: var(--max-primary-400);
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &:disabled {
            opacity: 0.45;
            cursor: not-allowed;
        }

        &:hover:not(:disabled) {
            background: color-mix(in srgb, var(--max-primary-500) 26%, transparent);
        }
    }

    .wayland-shortcut-help__trigger {
        font-size: 0.75rem;
        color: var(--background-300);
    }

    .wayland-shortcut-help__cli {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .wayland-shortcut-help__cli-top {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
    }

    .wayland-shortcut-help__cli-label {
        font-size: 0.75rem;
        color: var(--background-300);
    }

    .wayland-shortcut-help__copy-btn {
        padding: 0.1875rem 0.5rem;
        border-radius: 0.25rem;
        border: 1px solid var(--background-600);
        background: var(--background-800);
        color: var(--background-200);
        font-size: 0.6875rem;
        cursor: pointer;

        &:hover {
            border-color: var(--max-primary-500);
            color: var(--background-50);
        }
    }

    .wayland-shortcut-help__code {
        margin: 0;
        padding: 0.5rem 0.625rem;
        border-radius: 0.25rem;
        background: var(--background-950);
        border: 1px solid var(--background-800);
        font-family: var(--mt-font-mono);
        font-size: 0.75rem;
        color: var(--max-primary-300);
        overflow-x: auto;
    }
</style>
