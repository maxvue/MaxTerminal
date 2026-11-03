import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { ESLint } from 'eslint';
import { describe, expect, it } from 'vitest';

const currentDir = path.dirname(fileURLToPath(import.meta.url));
const workspaceRoot = path.resolve(currentDir, '../..');

describe('ESLint Flat Config e Regras Customizadas', () => {
    const eslint = new ESLint({
        overrideConfigFile: path.resolve(workspaceRoot, 'eslint.config.js'),
        ignore: false
    });

    it('deve aprovar valid-component.vue sem erros nem warnings', async () => {
        const filePath = path.resolve(currentDir, 'fixtures/valid-component.vue');
        const [result] = await eslint.lintFiles([filePath]);

        expect(result).toBeDefined();
        expect(result?.errorCount).toBe(0);
        expect(result?.warningCount).toBe(0);
    });

    it('deve reprovar invalid-block-order.vue pela regra vue/block-order', async () => {
        const filePath = path.resolve(currentDir, 'fixtures/invalid-block-order.vue');
        const [result] = await eslint.lintFiles([filePath]);

        expect(result).toBeDefined();
        const ruleIds = result?.messages.map((m) => m.ruleId) ?? [];
        expect(ruleIds).toContain('vue/block-order');
    });

    it('deve reprovar invalid-multiline-tag.vue pela regra autoral max/single-line-start-tag', async () => {
        const filePath = path.resolve(currentDir, 'fixtures/invalid-multiline-tag.vue');
        const [result] = await eslint.lintFiles([filePath]);

        expect(result).toBeDefined();
        const ruleIds = result?.messages.map((m) => m.ruleId) ?? [];
        expect(ruleIds).toContain('max/single-line-start-tag');
    });

    it('deve reprovar invalid-comma-quotes.vue pelas regras de aspas e vírgulas do @stylistic', async () => {
        const filePath = path.resolve(currentDir, 'fixtures/invalid-comma-quotes.vue');
        const [result] = await eslint.lintFiles([filePath]);

        expect(result).toBeDefined();
        const ruleIds = result?.messages.map((m) => m.ruleId) ?? [];
        expect(ruleIds).toContain('@stylistic/quotes');
        expect(ruleIds).toContain('@stylistic/comma-dangle');
    });
});
