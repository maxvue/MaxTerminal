<template>
    <div class="php-preview" data-testid="artisan-php-preview">
        <div class="php-preview__tabs" role="tablist">
            <button type="button" role="tab" class="php-preview__tab" :class="{ 'php-preview__tab--active': activeTab === 'php' }" :aria-selected="activeTab === 'php'" data-testid="preview-tab-php" @click="activeTab = 'php'">
                <span>Comando PHP</span>
            </button>
            <button type="button" role="tab" class="php-preview__tab" :class="{ 'php-preview__tab--active': activeTab === 'routes' }" :aria-selected="activeTab === 'routes'" data-testid="preview-tab-routes" @click="activeTab = 'routes'">
                <span>routes/console.php</span>
                <span v-if="preview?.consolePatch" class="php-preview__pill" :class="preview.consolePatch.manualOnly ? 'php-preview__pill--warn' : 'php-preview__pill--info'">
                    {{ preview.consolePatch.manualOnly ? 'Manual' : preview.consolePatch.willCreate ? 'Novo' : 'Patch' }}
                </span>
            </button>
            <button type="button" role="tab" class="php-preview__tab" :class="{ 'php-preview__tab--active': activeTab === 'manual' }" :aria-selected="activeTab === 'manual'" data-testid="preview-tab-manual" @click="activeTab = 'manual'">
                <span>Passos manuais</span>
                <span v-if="manualCount > 0" class="php-preview__pill php-preview__pill--warn">{{ manualCount }}</span>
            </button>
        </div>

        <div v-if="loading" class="php-preview__loading" data-testid="preview-loading">
            Atualizando prévia…
        </div>

        <div v-if="activeTab === 'php'" class="php-preview__panel">
            <div v-if="preview" class="php-preview__meta">
                <span class="php-preview__path" data-testid="preview-file-path">{{ preview.path }}</span>
                <code class="php-preview__sig" data-testid="preview-signature">{{ preview.signature }}</code>
            </div>
            <pre v-if="preview?.php" class="php-preview__code" data-testid="preview-php-code"><code>{{ preview.php }}</code></pre>
            <div v-else class="php-preview__placeholder" data-testid="preview-php-empty">
                Preencha os campos obrigatórios sem erros para visualizar o código PHP gerado.
            </div>
        </div>

        <div v-else-if="activeTab === 'routes'" class="php-preview__panel">
            <template v-if="preview?.consolePatch">
                <div class="php-preview__meta">
                    <span class="php-preview__path">{{ preview.consolePatch.path }}</span>
                    <span class="php-preview__hint">
                        {{ preview.consolePatch.manualOnly ? 'Aplicação manual necessária' : preview.consolePatch.willCreate ? 'O arquivo será criado automaticamente' : 'Bloco delimitado será atualizado' }}
                    </span>
                </div>
                <pre class="php-preview__code" data-testid="preview-routes-diff"><code>{{ preview.consolePatch.unifiedDiff }}</code></pre>
            </template>
            <div v-else class="php-preview__placeholder">
                Ative a opção de Agendamento (Schedule) para visualizar as alterações em routes/console.php.
            </div>
        </div>

        <div v-else class="php-preview__panel">
            <template v-if="manualSteps.length > 0">
                <ol class="php-preview__steps" data-testid="preview-manual-steps">
                    <li v-for="(step, idx) in manualSteps" :key="idx" class="php-preview__step">
                        <pre class="php-preview__step-text"><code>{{ step }}</code></pre>
                        <button type="button" class="php-preview__copy-btn" data-testid="copy-manual-step-btn" @click="copyText(step)">
                            <MaxIcon icon="mdi:content-copy" />
                            <span>Copiar trecho</span>
                        </button>
                    </li>
                </ol>
            </template>
            <div v-else class="php-preview__placeholder">
                Nenhum passo manual adicional é necessário.
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed, ref, watch } from 'vue';
    import type { ArtisanPreview } from '@/types/bindings';

    export interface ArtisanPhpPreviewProps {
        preview: ArtisanPreview | null;
        loading: boolean;
    }

    const props = defineProps<ArtisanPhpPreviewProps>();

    const activeTab = ref<'php' | 'routes' | 'manual'>('php');

    const manualSteps = computed<string[]>(() => props.preview?.manualSteps ?? []);
    const manualCount = computed<number>(() => manualSteps.value.length);

    watch(
        () => props.preview?.manualSteps?.length ?? 0,
        (count, prevCount) => {
            if (count > 0 && prevCount === 0 && !props.preview?.php) {
                activeTab.value = 'manual';
            }
        }
    );

    async function copyText(text: string): Promise<void> {
        if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
            await navigator.clipboard.writeText(text).catch(() => {});
        }
    }
</script>

<style lang="scss" scoped>
    .php-preview {
        display: flex;
        flex-direction: column;
        height: 100%;
        border: 1px solid var(--background-700);
        border-radius: 0.375rem;
        background: var(--background-950);
        overflow: hidden;

        &__tabs {
            display: flex;
            align-items: center;
            gap: 0.25rem;
            padding: 0.375rem 0.5rem;
            background: var(--background-900);
            border-bottom: 1px solid var(--background-800);
        }

        &__tab {
            display: inline-flex;
            align-items: center;
            gap: 0.375rem;
            padding: 0.25rem 0.625rem;
            border: 1px solid transparent;
            border-radius: 0.25rem;
            background: transparent;
            color: var(--background-300);
            font-size: 0.75rem;
            font-weight: 500;
            cursor: pointer;

            &--active {
                background: var(--background-800);
                border-color: var(--max-primary-500);
                color: var(--background-50);
            }
        }

        &__pill {
            padding: 0.0625rem 0.375rem;
            border-radius: 999px;
            font-size: 0.6875rem;
            font-weight: 600;

            &--info {
                background: var(--background-800);
                color: var(--max-info-400);
            }

            &--warn {
                background: var(--background-800);
                color: var(--max-warning-400);
            }
        }

        &__loading {
            padding: 0.25rem 0.625rem;
            background: var(--background-900);
            color: var(--max-info-400);
            font-size: 0.6875rem;
        }

        &__panel {
            display: flex;
            flex-direction: column;
            flex: 1;
            min-height: 0;
            padding: 0.625rem;
            gap: 0.5rem;
            overflow: auto;
        }

        &__meta {
            display: flex;
            flex-direction: column;
            gap: 0.25rem;
            padding-bottom: 0.375rem;
            border-bottom: 1px solid var(--background-800);
        }

        &__path {
            font-family: var(--mt-font-mono);
            font-size: 0.75rem;
            font-weight: 600;
            color: var(--max-primary-400);
        }

        &__sig {
            font-family: var(--mt-font-mono);
            font-size: 0.6875rem;
            color: var(--background-300);
            white-space: pre-wrap;
        }

        &__hint {
            font-size: 0.6875rem;
            color: var(--background-300);
        }

        &__code {
            margin: 0;
            padding: 0.625rem;
            border-radius: 0.25rem;
            background: var(--background-900);
            color: var(--background-100);
            font-family: var(--mt-font-mono);
            font-size: 0.75rem;
            line-height: 1.45;
            overflow: auto;
            white-space: pre;
        }

        &__placeholder {
            padding: 1.25rem;
            text-align: center;
            color: var(--background-400);
            font-size: 0.75rem;
        }

        &__steps {
            margin: 0;
            padding-left: 1.25rem;
            display: flex;
            flex-direction: column;
            gap: 0.625rem;
        }

        &__step {
            display: flex;
            flex-direction: column;
            gap: 0.375rem;
            color: var(--background-100);
            font-size: 0.75rem;
        }

        &__step-text {
            margin: 0;
            padding: 0.5rem;
            border-radius: 0.25rem;
            background: var(--background-900);
            color: var(--background-100);
            font-family: var(--mt-font-mono);
            font-size: 0.75rem;
            white-space: pre-wrap;
        }

        &__copy-btn {
            align-self: flex-start;
            display: inline-flex;
            align-items: center;
            gap: 0.25rem;
            padding: 0.25rem 0.5rem;
            border: 1px solid var(--background-700);
            border-radius: 0.25rem;
            background: var(--background-800);
            color: var(--background-100);
            font-size: 0.6875rem;
            cursor: pointer;

            &:hover {
                border-color: var(--max-primary-500);
                color: var(--background-50);
            }
        }
    }
</style>
