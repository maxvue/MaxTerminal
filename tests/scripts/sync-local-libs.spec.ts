import fs from 'node:fs';
import path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { syncLocalLibs } from '../../scripts/sync-local-libs.mjs';

const workspaceTmpBase = path.resolve(process.cwd(), '.tmp');

describe('syncLocalLibs (sincronização de bibliotecas locais)', () => {
    let tempRoot: string;
    let appDir: string;
    let libsDir: string;

    beforeEach(() => {
        fs.mkdirSync(workspaceTmpBase, { recursive: true });
        tempRoot = fs.mkdtempSync(path.join(workspaceTmpBase, 'maxterminal-sync-spec-'));
        appDir = path.join(tempRoot, 'app');
        libsDir = path.join(tempRoot, 'libs');

        fs.mkdirSync(appDir, { recursive: true });
        fs.mkdirSync(libsDir, { recursive: true });

        // Cria package.json da aplicação de teste
        fs.writeFileSync(
            path.join(appDir, 'package.json'),
            JSON.stringify(
                {
                    name: 'test-app',
                    maxLocalLibs: {
                        '@maxvue/max-use': 'MaxUse'
                    }
                },
                null,
                4
            )
        );

        // Cria estrutura válida da biblioteca local MaxUse
        const maxUseDir = path.join(libsDir, 'MaxUse');
        fs.mkdirSync(path.join(maxUseDir, 'dist'), { recursive: true });

        fs.writeFileSync(
            path.join(maxUseDir, 'package.json'),
            JSON.stringify(
                {
                    name: '@maxvue/max-use',
                    version: '2.0.0',
                    exports: {
                        '.': './dist/index.es.js'
                    }
                },
                null,
                4
            )
        );

        fs.writeFileSync(path.join(maxUseDir, 'dist', 'index.es.js'), 'export const useTest = () => true;\n');
    });

    afterEach(() => {
        try {
            fs.rmSync(tempRoot, { recursive: true, force: true });
        } catch {
            // Ignora falha de limpeza em teardown
        }
    });

    it('deve criar symlink absoluto e manter idempotência em execuções sucessivas', () => {
        const firstResult = syncLocalLibs({
            workspaceRoot: appDir,
            libsRoot: libsDir,
            checkOnly: false,
            logger: () => {}
        });

        expect(firstResult.ok).toBe(true);

        const destLink = path.join(appDir, 'node_modules', '@maxvue', 'max-use');
        const stat = fs.lstatSync(destLink);
        expect(stat.isSymbolicLink()).toBe(true);

        const target = fs.readlinkSync(destLink);
        const expectedTarget = fs.realpathSync(path.join(libsDir, 'MaxUse'));
        expect(target).toBe(expectedTarget);

        // Segunda execução deve ser idempotente
        const secondResult = syncLocalLibs({
            workspaceRoot: appDir,
            libsRoot: libsDir,
            checkOnly: false,
            logger: () => {}
        });

        expect(secondResult.ok).toBe(true);
        expect(fs.readlinkSync(destLink)).toBe(expectedTarget);
    });

    it('em modo checkOnly deve falhar se o symlink não existir e não deve alterar o sistema de arquivos', () => {
        const result = syncLocalLibs({
            workspaceRoot: appDir,
            libsRoot: libsDir,
            checkOnly: true,
            logger: () => {}
        });

        expect(result.ok).toBe(false);
        const destLink = path.join(appDir, 'node_modules', '@maxvue', 'max-use');
        expect(fs.existsSync(destLink)).toBe(false);
    });

    it('deve falhar e não criar links se um artefato em dist/ não existir', () => {
        const brokenLibDir = path.join(libsDir, 'MaxUse');
        // Sobrescreve package.json apontando para artefato inexistente
        fs.writeFileSync(
            path.join(brokenLibDir, 'package.json'),
            JSON.stringify(
                {
                    name: '@maxvue/max-use',
                    version: '2.0.0',
                    exports: {
                        '.': './dist/nao-existe.js'
                    }
                },
                null,
                4
            )
        );

        const result = syncLocalLibs({
            workspaceRoot: appDir,
            libsRoot: libsDir,
            checkOnly: false,
            logger: () => {}
        });

        expect(result.ok).toBe(false);
        const destLink = path.join(appDir, 'node_modules', '@maxvue', 'max-use');
        expect(fs.existsSync(destLink)).toBe(false);
    });

    it('deve recusar sobrescrever quando o destino já for um diretório real', () => {
        const destDir = path.join(appDir, 'node_modules', '@maxvue', 'max-use');
        fs.mkdirSync(destDir, { recursive: true });
        fs.writeFileSync(path.join(destDir, 'arquivo-real.txt'), 'conteudo real');

        const result = syncLocalLibs({
            workspaceRoot: appDir,
            libsRoot: libsDir,
            checkOnly: false,
            logger: () => {}
        });

        expect(result.ok).toBe(false);
        const failure = result.results.find((r: { pkg: string; error?: string }) => r.pkg === '@maxvue/max-use');
        expect(failure?.error).toMatch(/já existe como diretório\/arquivo real/);

        // Confirma que o diretório real foi preservado intacto
        expect(fs.lstatSync(destDir).isSymbolicLink()).toBe(false);
        expect(fs.existsSync(path.join(destDir, 'arquivo-real.txt'))).toBe(true);
    });

    it('deve falhar com libsRoot relativo ou inválido sem alterar o sistema de arquivos (CA-03)', () => {
        const result = syncLocalLibs({
            workspaceRoot: appDir,
            libsRoot: 'caminho/relativo/invalido',
            checkOnly: true,
            logger: () => {}
        });

        expect(result.ok).toBe(false);
        expect(result.results[0]?.error).toMatch(/deve ser absoluto/);
        const destLink = path.join(appDir, 'node_modules', '@maxvue', 'max-use');
        expect(fs.existsSync(destLink)).toBe(false);
    });
});
