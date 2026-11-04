import { describe, expect, it } from 'vitest';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { resolve } from 'node:path';
import { buildCssVarsRecord } from '@/theme/cssTokens';
import { FALLBACK_MAX_NAVY_TEAL_TOKENS } from '@/theme/bootstrap';

/**
 * Coleta recursivamente todos os arquivos com extensões especificadas em um diretório.
 */
function collectFiles(dir: string, extensions: string[]): string[] {
    const results: string[] = [];
    try {
        const entries = readdirSync(dir);
        for (const entry of entries) {
            const fullPath = resolve(dir, entry);
            const stat = statSync(fullPath);
            if (stat.isDirectory()) {
                results.push(...collectFiles(fullPath, extensions));
            } else if (extensions.some((ext) => fullPath.endsWith(ext))) {
                results.push(fullPath);
            }
        }
    } catch {
        // Ignora diretórios inacessíveis
    }
    return results;
}

describe('themeCoverage (auditoria estática de cores na UI)', () => {
    it('garante que buildCssVarsRecord exporta todos os contratos essenciais da biblioteca e de UI', () => {
        const vars = buildCssVarsRecord(FALLBACK_MAX_NAVY_TEAL_TOKENS);

        const requiredTokens = [
            '--max-inputtext-border-color',
            '--max-inputtext-disabled-background',
            '--max-inputtext-focus-border-color',
            '--max-form-field-disabled-background',
            '--max-floatlabel-on-active-background',
            '--max-surface-400',
            '--max-button-primary-border-color',
            '--max-button-secondary-border-color',
            '--max-button-info-border-color',
            '--max-button-success-border-color',
            '--max-button-warn-border-color',
            '--max-button-help-border-color',
            '--max-button-danger-border-color',
            '--max-button-contrast-border-color',
            '--max-focus-ring-color',
            '--max-focus-ring-offset-color',
            '--max-focus-ring',
            '--max-focus-outline',
            '--max-primary-content',
            '--max-secondary-content',
            '--max-success-content',
            '--max-info-content',
            '--max-warning-content',
            '--max-danger-content',
            '--max-help-content',
            '--max-contrast-content',
            '--max-content-secondary',
            '--max-content-placeholder',
            '--max-content-help',
            '--max-content-disabled',
            '--max-selection-background',
            '--max-selection-content',
            '--max-selection-hover-background',
            '--max-selection-hover-content',
            '--max-table-header-bg',
            '--max-table-header-text',
            '--max-table-row-even-bg',
            '--max-table-row-odd-bg',
            '--max-table-row-selected-bg',
            '--max-table-border-color',
            '--max-purple-500',
            '--max-purple-400',
            '--max-purple-300',
            '--max-purple-100',
            '--mt-ui-surface',
            '--mt-ui-surface-panel',
            '--mt-ui-surface-elevated',
            '--mt-ui-surface-hover',
            '--mt-ui-surface-active',
            '--mt-ui-border-subtle',
            '--mt-ui-border',
            '--mt-ui-border-strong',
            '--mt-ui-text-muted',
            '--mt-ui-text-secondary',
            '--mt-ui-text',
            '--mt-ui-text-strong',
            '--mt-ui-highlight-bg',
            '--mt-ui-highlight-text',
            '--mt-ui-primary-subtle',
            '--mt-ui-success-subtle',
            '--mt-ui-warning-subtle',
            '--mt-ui-danger-subtle',
            '--mt-ui-info-subtle',
            '--mt-ui-issue-category',
            '--mt-ui-issue-category-hover',
            '--mt-ui-issue-subtle',
            '--mt-ui-overlay',
            '--mt-ui-shadow',
            '--mt-ui-shadow-lg'
        ];

        for (const token of requiredTokens) {
            expect(vars[token], `Token ausente em buildCssVarsRecord: ${token}`).toBeDefined();
            expect(vars[token]?.length, `Token vazio: ${token}`).toBeGreaterThan(0);
        }
    });

    it('verifica ausência de cores RGB fixas de status semântico em estilos scoped dos componentes principais', () => {
        const rootDir = resolve(__dirname, '../../src/components');
        const vueFiles = collectFiles(rootDir, ['.vue']);

        // Expressões que representam cores RGB/HEX fixas arbitrárias em propriedades CSS de cor de fundo/borda/texto
        // excluindo sombras pretas (rgb(0 0 0 ...)) e variáveis semânticas
        const hardcodedFixedColorRegex = /background(-color)?:\s*rgb\((?!(0\s+0\s+0|255\s+255\s+255\s*\/\s*12%))\d+\s+\d+\s+\d+/i;

        const violations: string[] = [];

        for (const file of vueFiles) {
            // ThemeSwatch é catálogo de dados visuais dos esquemas disponíveis; deve manter amostras de dados
            if (file.endsWith('ThemeSwatch.vue')) {
                continue;
            }

            const content = readFileSync(file, 'utf-8');
            const styleMatch = content.match(/<style[^>]*>([\s\S]*?)<\/style>/);
            if (!styleMatch || !styleMatch[1]) {
                continue;
            }

            const styleContent = styleMatch[1];
            const lines = styleContent.split('\n');
            for (let i = 0; i < lines.length; i++) {
                const line = lines[i]!;
                if (hardcodedFixedColorRegex.test(line)) {
                    violations.push(`${file.replace(rootDir, 'src/components')}:${i + 1} -> ${line.trim()}`);
                }
            }
        }

        expect(violations, `Encontradas cores RGB fixas não autorizadas na UI:\n${violations.join('\n')}`).toEqual([]);
    });
});
