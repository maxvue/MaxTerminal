import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const currentDir = path.dirname(fileURLToPath(import.meta.url));
const workspaceRoot = path.resolve(currentDir, '../..');
const srcDir = path.resolve(workspaceRoot, 'src');

/**
 * Coleta recursivamente todos os arquivos com extensão fornecida.
 */
function walkFiles(dir: string, extension: string): string[] {
    const results: string[] = [];
    const entries = fs.readdirSync(dir, { withFileTypes: true });
    for (const entry of entries) {
        const fullPath = path.join(dir, entry.name);
        if (entry.isDirectory()) {
            results.push(...walkFiles(fullPath, extension));
        } else if (entry.isFile() && entry.name.endsWith(extension)) {
            results.push(fullPath);
        }
    }
    return results;
}

describe('Auditoria Arquitetural MaxComponentsUi (T6)', () => {
    const vueFiles = walkFiles(srcDir, '.vue');

    it('todos os modais padronizados com MaxModal utilizam o componente da biblioteca', () => {
        const expectedModals = [
            'AgentTabOpenDialog.vue',
            'AgentWorktreeCloseDialog.vue',
            'DestructiveConfirm.vue',
            'BugCycleConfirmModal.vue',
            'GithubAuthPanel.vue',
            'IssueCreateModal.vue',
            'IssueOpenTabModal.vue',
            'ProjectAddModal.vue',
            'ProjectRootModal.vue',
            'ArtisanCommandModal.vue',
            'TemplateEditorModal.vue'
        ];

        for (const modalFile of expectedModals) {
            const foundPath = vueFiles.find((f) => path.basename(f) === modalFile);
            expect(foundPath, `Arquivo ${modalFile} não encontrado em src/`).toBeDefined();
            if (foundPath) {
                const content = fs.readFileSync(foundPath, 'utf-8');
                expect(content, `${modalFile} deve usar o componente MaxModal`).toContain('<MaxModal');
                expect(content, `${modalFile} deve importar MaxModal`).toMatch(/MaxModal/);
            }
        }
    });

    it('overlays especializados CommandGroupModal e GlobalSearchPalette cumprem requisitos de acessibilidade', () => {
        const specializedOverlays = ['CommandGroupModal.vue', 'GlobalSearchPalette.vue'];
        for (const fileName of specializedOverlays) {
            const foundPath = vueFiles.find((f) => path.basename(f) === fileName);
            expect(foundPath).toBeDefined();
            if (foundPath) {
                const content = fs.readFileSync(foundPath, 'utf-8');
                expect(content).toContain('role="dialog"');
                expect(content).toContain('aria-modal="true"');
            }
        }
    });

    it('exceções de textarea correspondem estritamente aos casos documentados (Seção 4.2)', () => {
        const textareasFound: { file: string; line: number }[] = [];

        for (const filePath of vueFiles) {
            const content = fs.readFileSync(filePath, 'utf-8');
            const lines = content.split('\n');
            lines.forEach((line, idx) => {
                if (/<textarea\b/.test(line)) {
                    textareasFound.push({ file: path.relative(workspaceRoot, filePath), line: idx + 1 });
                }
            });
        }

        expect(textareasFound.length).toBeGreaterThan(0);
        const allowedFiles = new Set([
            'src/components/terminal/RichInput.vue',
            'src/components/commandGroups/CommandGroupSuggestPanel.vue'
        ]);

        for (const item of textareasFound) {
            expect(allowedFiles.has(item.file), `Textarea não permitida encontrada em ${item.file}:${item.line}`).toBe(true);
        }
    });

    it('SessionHistoryDrawer preserva aside especializado de navegação rápida (Seção 4.2)', () => {
        const drawerPath = vueFiles.find((f) => path.basename(f) === 'SessionHistoryDrawer.vue');
        expect(drawerPath).toBeDefined();
        if (drawerPath) {
            const content = fs.readFileSync(drawerPath, 'utf-8');
            expect(content).toContain('<aside');
            expect(content).not.toContain('<MaxDrawer');
        }
    });

    it('contratos compartilhados C1-C4 estão estritamente cumpridos', () => {
        const pluginsPath = path.resolve(srcDir, 'app/plugins.ts');
        const pluginsContent = fs.readFileSync(pluginsPath, 'utf-8');
        expect(pluginsContent).toMatch(/app\.use\(MaxComponentsUi/);

        const appPath = path.resolve(srcDir, 'app/App.vue');
        const appContent = fs.readFileSync(appPath, 'utf-8');
        expect(appContent).toContain('<MaxToast');

        const allSourceFiles = [...vueFiles, ...walkFiles(srcDir, '.ts')];
        for (const file of allSourceFiles) {
            const content = fs.readFileSync(file, 'utf-8');
            expect(content).not.toMatch(/severity:\s*['"]warn['"]/);
            expect(content).not.toMatch(/variant:\s*['"]warn['"]/);
            expect(content).not.toMatch(/Toast\.show\([^)]*actionLabel/);
            expect(content).not.toMatch(/Toast\.show\([^)]*onAction/);
        }
    });

    it('contratos de componentes de UI (MaxButton, MaxTag, MaxAlert) são respeitados', () => {
        const nonCredentialVueFiles = vueFiles.filter((f) => !f.includes('/credentials/'));
        for (const file of nonCredentialVueFiles) {
            const content = fs.readFileSync(file, 'utf-8');
            const relativePath = path.relative(workspaceRoot, file);

            // MaxButton e MaxIconButton não devem usar variant="ghost" ou variant="primary"
            expect(content, `${relativePath} contém variant="ghost"`).not.toMatch(/variant=["']ghost["']/);
            expect(content, `${relativePath} contém variant="primary"`).not.toMatch(/<MaxButton[^>]*variant=["']primary["']/);

            // MaxTag não deve usar variant nem severity inválida
            expect(content, `${relativePath} contém MaxTag com variant`).not.toMatch(/<MaxTag[^>]*variant=/);

            // MaxAlert não deve usar severity="error" nem atributo legado type=
            expect(content, `${relativePath} contém MaxAlert com severity="error"`).not.toMatch(/<MaxAlert[^>]*severity=["']error["']/);
            expect(content, `${relativePath} contém MaxAlert com type=`).not.toMatch(/<MaxAlert[^>]*\btype=["']/);
        }
    });

    it('nenhum componente desconhecido ou inválido é importado de @maxvue/max-components-ui', () => {
        const maxComponentsUiIndexPath = path.resolve(workspaceRoot, 'node_modules/@maxvue/max-components-ui/dist/index.d.ts');
        const indexDtsContent = fs.readFileSync(maxComponentsUiIndexPath, 'utf-8');

        const allSourceFiles = [...vueFiles, ...walkFiles(srcDir, '.ts')];
        const importRegex = /import\s+\{([^}]+)\}\s+from\s+['"]@maxvue\/max-components-ui['"]/g;

        const importedNames = new Set<string>();

        for (const file of allSourceFiles) {
            const content = fs.readFileSync(file, 'utf-8');
            let match: RegExpExecArray | null = importRegex.exec(content);
            while (match !== null) {
                const names = match[1]?.split(',').map((n) => n.trim().replace(/^type\s+/, '').split(/\s+as\s+/)[0]?.trim()) ?? [];
                for (const name of names) {
                    if (name) {
                        importedNames.add(name);
                    }
                }
                match = importRegex.exec(content);
            }
        }

        expect(importedNames.size).toBeGreaterThan(0);

        for (const name of importedNames) {
            const existsInDist = indexDtsContent.includes(name);
            expect(existsInDist, `Componente ${name} importado de @maxvue/max-components-ui não existe na biblioteca compilada`).toBe(true);
        }
    });
});
