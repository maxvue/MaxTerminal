<template>
    <MaxModal v-model:visible="editorStore.isOpen" :title="dialogTitle" :no-button="true" width="54rem" :dismissable="!editorStore.saving" :close-on-escape="!editorStore.saving" :before-close="onBeforeClose">
        <div class="theme-editor">
            <MaxAlert v-if="editorStore.errorMessage" :message="editorStore.errorMessage" severity="danger" class="theme-editor__alert" />

            <div class="theme-editor__section">
                <h4 class="theme-editor__section-title">Identificação e Cursor</h4>
                <div class="theme-editor__meta-grid">
                    <div class="theme-editor__field">
                        <label class="theme-editor__label" for="theme-editor-name">Nome do tema</label>
                        <MaxInputText id="theme-editor-name" v-model="editorStore.data.draft.name" placeholder="Ex.: Oceano Noturno" aria-label="Nome do tema" />
                    </div>
                    <div class="theme-editor__field">
                        <label class="theme-editor__label" for="theme-editor-author">Autor (opcional)</label>
                        <MaxInputText id="theme-editor-author" :model-value="editorStore.data.draft.author ?? ''" placeholder="Ex.: Equipe Max" aria-label="Autor do tema" @update:model-value="onAuthorChange" />
                    </div>
                    <div class="theme-editor__field">
                        <label class="theme-editor__label" for="theme-editor-cursor-style">Estilo do cursor</label>
                        <MaxInputSelect id="theme-editor-cursor-style" :model-value="editorStore.data.draft.cursorStyle" :options="cursorStyleOptions" option-label="label" option-value="value" aria-label="Estilo do cursor do tema" @update:model-value="onCursorStyleChange" />
                    </div>
                </div>
            </div>

            <div class="theme-editor__section">
                <h4 class="theme-editor__section-title">Cores Principais do Terminal</h4>
                <div class="theme-editor__color-grid">
                    <div class="theme-editor__color-item">
                        <span class="theme-editor__label">Fundo principal</span>
                        <div class="theme-editor__color-control">
                            <MaxColorPicker :model-value="stripHash(editorStore.data.draft.background)" aria-label="Seletor de cor de fundo principal" @update:model-value="onCoreColorPick('background', $event)" />
                            <MaxInputText v-model="editorStore.data.draft.background" aria-label="Código hexadecimal de fundo principal" class="theme-editor__hex-input" />
                        </div>
                    </div>

                    <div class="theme-editor__color-item">
                        <span class="theme-editor__label">Texto principal</span>
                        <div class="theme-editor__color-control">
                            <MaxColorPicker :model-value="stripHash(editorStore.data.draft.foreground)" aria-label="Seletor de cor de texto principal" @update:model-value="onCoreColorPick('foreground', $event)" />
                            <MaxInputText v-model="editorStore.data.draft.foreground" aria-label="Código hexadecimal de texto principal" class="theme-editor__hex-input" />
                        </div>
                    </div>

                    <div class="theme-editor__color-item">
                        <span class="theme-editor__label">Cor do cursor</span>
                        <div class="theme-editor__color-control">
                            <MaxColorPicker :model-value="stripHash(editorStore.data.draft.cursor)" aria-label="Seletor de cor do cursor" @update:model-value="onCoreColorPick('cursor', $event)" />
                            <MaxInputText v-model="editorStore.data.draft.cursor" aria-label="Código hexadecimal da cor do cursor" class="theme-editor__hex-input" />
                        </div>
                    </div>

                    <div class="theme-editor__color-item">
                        <span class="theme-editor__label">Texto sob o cursor</span>
                        <div class="theme-editor__color-control">
                            <MaxColorPicker :model-value="stripHash(editorStore.data.draft.cursorAccent)" aria-label="Seletor de cor do texto sob o cursor" @update:model-value="onCoreColorPick('cursorAccent', $event)" />
                            <MaxInputText v-model="editorStore.data.draft.cursorAccent" aria-label="Código hexadecimal do texto sob o cursor" class="theme-editor__hex-input" />
                        </div>
                    </div>

                    <div class="theme-editor__color-item">
                        <span class="theme-editor__label">Fundo da seleção</span>
                        <div class="theme-editor__color-control">
                            <MaxColorPicker :model-value="stripHash(editorStore.selectionParts.hex)" aria-label="Seletor de cor do fundo da seleção" @update:model-value="onSelectionHexPick" />
                            <MaxInputText :model-value="editorStore.selectionParts.hex" aria-label="Código hexadecimal do fundo da seleção" class="theme-editor__hex-input" @update:model-value="onSelectionHexInput" />
                        </div>
                    </div>

                    <div class="theme-editor__color-item">
                        <span class="theme-editor__label">Opacidade da seleção (%)</span>
                        <div class="theme-editor__color-control">
                            <MaxInputNumber :model-value="editorStore.selectionParts.alphaPercent" :min="5" :max="100" :step="5" aria-label="Opacidade da seleção em percentual" @update:model-value="onSelectionAlphaChange" />
                            <MaxTag :value="editorStore.data.draft.selectionBackground" severity="secondary" :no-uppercase="true" />
                        </div>
                    </div>

                    <div class="theme-editor__color-item">
                        <span class="theme-editor__label">Texto selecionado (opcional)</span>
                        <div class="theme-editor__color-control">
                            <MaxColorPicker :model-value="stripHash(editorStore.data.draft.selectionForeground ?? editorStore.data.draft.foreground)" aria-label="Seletor de cor do texto selecionado" @update:model-value="onOptionalColorPick('selectionForeground', $event)" />
                            <MaxInputText :model-value="editorStore.data.draft.selectionForeground ?? ''" placeholder="Automático (glifo)" aria-label="Código hexadecimal do texto selecionado" class="theme-editor__hex-input" @update:model-value="onOptionalColorInput('selectionForeground', $event)" />
                        </div>
                    </div>

                    <div class="theme-editor__color-item">
                        <span class="theme-editor__label">Destaque da aba ativa (opcional)</span>
                        <div class="theme-editor__color-control">
                            <MaxColorPicker :model-value="stripHash(editorStore.data.draft.activeTabAccent ?? editorStore.data.draft.cursor)" aria-label="Seletor de cor de destaque da aba ativa" @update:model-value="onOptionalColorPick('activeTabAccent', $event)" />
                            <MaxInputText :model-value="editorStore.data.draft.activeTabAccent ?? ''" placeholder="Automático (primária)" aria-label="Código hexadecimal de destaque da aba ativa" class="theme-editor__hex-input" @update:model-value="onOptionalColorInput('activeTabAccent', $event)" />
                        </div>
                    </div>
                </div>
            </div>

            <div class="theme-editor__section">
                <div class="theme-editor__section-header">
                    <h4 class="theme-editor__section-title">Paleta ANSI de 16 Cores</h4>
                    <MaxTag v-if="indexedColorCount > 0" :value="`${indexedColorCount} cores indexadas (16–255) preservadas`" severity="info" :no-uppercase="true" />
                </div>
                <div class="theme-editor__ansi-grid">
                    <div v-for="entry in ansiFields" :key="entry.key" class="theme-editor__color-item">
                        <span class="theme-editor__label">{{ entry.label }}</span>
                        <div class="theme-editor__color-control">
                            <MaxColorPicker :model-value="stripHash(editorStore.data.draft.ansi[entry.key])" :aria-label="`Seletor de cor para ${entry.label}`" @update:model-value="onAnsiColorPick(entry.key, $event)" />
                            <MaxInputText v-model="editorStore.data.draft.ansi[entry.key]" :aria-label="`Código hexadecimal para ${entry.label}`" class="theme-editor__hex-input" />
                        </div>
                    </div>
                </div>
            </div>

            <footer class="theme-editor__footer">
                <MaxButton type="button" label="Cancelar" variant="outlined" :disabled="editorStore.saving" @click="editorStore.closeEditor()" />
                <MaxButton v-if="editorStore.isDirty" type="button" label="Restaurar valores" variant="text" :disabled="editorStore.saving" @click="editorStore.resetDraft()" />
                <MaxButton type="button" :label="submitLabel" :loading="editorStore.saving" :disabled="!canSubmit" @click="onSubmit" />
            </footer>
        </div>
    </MaxModal>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import {
        MaxAlert,
        MaxButton,
        MaxColorPicker,
        MaxInputNumber,
        MaxInputSelect,
        MaxInputText,
        MaxModal,
        MaxTag
    } from '@maxvue/max-components-ui';
    import { useThemeEditorStore } from '@/stores/themeEditor';
    import type {
        AnsiPaletteInput,
        CursorStyle,
        ResolvedTheme
    } from '@/types/theme';

    const emit = defineEmits<{
        saved: [resolved: ResolvedTheme];
    }>();

    const editorStore = useThemeEditorStore();

    const cursorStyleOptions: Array<{ label: string; value: CursorStyle }> = [
        { label: 'Bloco', value: 'block' },
        { label: 'Sublinhado', value: 'underline' },
        { label: 'Barra vertical', value: 'bar' }
    ];

    const ansiFields: Array<{ key: keyof AnsiPaletteInput; label: string }> = [
        { key: 'black', label: 'Preto (ANSI 0)' },
        { key: 'red', label: 'Vermelho (ANSI 1)' },
        { key: 'green', label: 'Verde (ANSI 2)' },
        { key: 'yellow', label: 'Amarelo (ANSI 3)' },
        { key: 'blue', label: 'Azul (ANSI 4)' },
        { key: 'magenta', label: 'Magenta (ANSI 5)' },
        { key: 'cyan', label: 'Ciano (ANSI 6)' },
        { key: 'white', label: 'Branco (ANSI 7)' },
        { key: 'brightBlack', label: 'Preto brilhante (ANSI 8)' },
        { key: 'brightRed', label: 'Vermelho brilhante (ANSI 9)' },
        { key: 'brightGreen', label: 'Verde brilhante (ANSI 10)' },
        { key: 'brightYellow', label: 'Amarelo brilhante (ANSI 11)' },
        { key: 'brightBlue', label: 'Azul brilhante (ANSI 12)' },
        { key: 'brightMagenta', label: 'Magenta brilhante (ANSI 13)' },
        { key: 'brightCyan', label: 'Ciano brilhante (ANSI 14)' },
        { key: 'brightWhite', label: 'Branco brilhante (ANSI 15)' }
    ];

    const dialogTitle = computed(() =>
        editorStore.data.mode === 'edit'
            ? `Editar Tema: ${editorStore.baselineDraft.name || editorStore.data.draft.name}`
            : 'Criar Novo Tema Customizado'
    );

    const submitLabel = computed(() =>
        editorStore.data.mode === 'edit' ? 'Salvar alterações' : 'Criar tema'
    );

    const indexedColorCount = computed(
        () => Object.keys(editorStore.data.draft.indexedColors ?? {}).length
    );

    const canSubmit = computed(
        () =>
            !editorStore.saving &&
            !editorStore.isReadOnly &&
            editorStore.data.draft.name.trim().length > 0 &&
            editorStore.isColorsValid
    );

    function stripHash(hex: string): string {
        return hex.trim().replace(/^#/, '');
    }

    function normalizeHexFromPicker(raw: unknown): string {
        if (typeof raw !== 'string') {
            return '#000000';
        }
        const trimmed = raw.trim();
        if (!trimmed) {
            return '#000000';
        }
        return trimmed.startsWith('#') ? trimmed.toLowerCase() : `#${trimmed.toLowerCase()}`;
    }

    function onAuthorChange(val: string | number | undefined): void {
        const text = String(val ?? '').trim();
        editorStore.data.draft.author = text.length > 0 ? text : null;
    }

    function onCursorStyleChange(val: unknown): void {
        if (val === 'block' || val === 'underline' || val === 'bar') {
            editorStore.setCursorStyle(val);
        }
    }

    function onCoreColorPick(
        field: 'background' | 'foreground' | 'cursor' | 'cursorAccent',
        val: unknown
    ): void {
        editorStore.data.draft[field] = normalizeHexFromPicker(val);
    }

    function onSelectionHexPick(val: unknown): void {
        editorStore.setSelectionColor(normalizeHexFromPicker(val));
    }

    function onSelectionHexInput(val: string | number | undefined): void {
        const text = String(val ?? '').trim();
        if (text.length > 0) {
            editorStore.setSelectionColor(text);
        }
    }

    function onSelectionAlphaChange(val: number | null | undefined): void {
        if (typeof val === 'number' && Number.isFinite(val)) {
            editorStore.setSelectionColor(editorStore.selectionParts.hex, val);
        }
    }

    function onOptionalColorPick(
        field: 'selectionForeground' | 'activeTabAccent',
        val: unknown
    ): void {
        editorStore.data.draft[field] = normalizeHexFromPicker(val);
    }

    function onOptionalColorInput(
        field: 'selectionForeground' | 'activeTabAccent',
        val: string | number | undefined
    ): void {
        const trimmed = String(val ?? '').trim();
        editorStore.data.draft[field] = trimmed.length > 0 ? trimmed : null;
    }

    function onAnsiColorPick(key: keyof AnsiPaletteInput, val: unknown): void {
        editorStore.data.draft.ansi[key] = normalizeHexFromPicker(val);
    }

    function onBeforeClose(event: { preventDefault: () => void }): void {
        if (editorStore.saving) {
            event.preventDefault();
        }
    }

    async function onSubmit(): Promise<void> {
        try {
            const saved = await editorStore.submitDraft();
            emit('saved', saved);
        } catch {
            // Erro já exposto em editorStore.errorMessage com modal aberto
        }
    }
</script>

<style lang="scss" scoped>
    .theme-editor {
        display: flex;
        flex-direction: column;
        gap: 1rem;
    }

    .theme-editor__alert {
        margin-bottom: 0.25rem;
    }

    .theme-editor__section {
        display: flex;
        flex-direction: column;
        gap: 0.625rem;
        padding: 0.75rem;
        border-radius: 0.5rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
    }

    .theme-editor__section-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
        flex-wrap: wrap;
    }

    .theme-editor__section-title {
        margin: 0;
        font-size: 0.8125rem;
        font-weight: 600;
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .theme-editor__meta-grid {
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(13rem, 1fr));
        gap: 0.75rem;
    }

    .theme-editor__field {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
    }

    .theme-editor__label {
        font-size: 0.75rem;
        font-weight: 500;
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .theme-editor__color-grid,
    .theme-editor__ansi-grid {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(14rem, 1fr));
        gap: 0.625rem;
    }

    .theme-editor__color-item {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
    }

    .theme-editor__color-control {
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .theme-editor__hex-input {
        flex: 1;
        font-family: var(--mt-font-mono);
    }

    .theme-editor__footer {
        display: flex;
        align-items: center;
        justify-content: flex-end;
        gap: 0.5rem;
    }
</style>
