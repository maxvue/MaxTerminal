import fs from 'node:fs';
import path from 'node:path';
import { setTimeout } from 'node:timers/promises';
import { createServer, normalizePath, type ViteDevServer } from 'vite';
import { afterEach, describe, expect, it, vi } from 'vitest';

const configFile = path.resolve('vite.config.ts');
const excludedFolders = ['.max-code-worktrees', '.worktrees', '.tmp'];

describe('isolamento do watcher do Vite', () => {
    let fixture: string;
    let server: ViteDevServer | undefined;

    afterEach(async () => {
        await server?.close();
        server = undefined;
        vi.restoreAllMocks();
        if (fixture) fs.rmSync(fixture, { recursive: true, force: true });
    });

    it.each(['project', '.max-code-worktrees/wt-active'])('ignora artefatos sem perder HMR na raiz %s', async (location) => {
        fs.mkdirSync('.tmp', { recursive: true });
        fixture = fs.mkdtempSync(path.resolve('.tmp/vite-watch-'));
        const root = path.join(fixture, location);
        const library = path.join(fixture, 'MaxUse');
        const libraryFile = path.join(library, 'index.js');
        fs.mkdirSync(path.join(root, 'src'), { recursive: true });
        fs.mkdirSync(path.join(root, 'node_modules/@maxvue'), { recursive: true });
        fs.mkdirSync(library, { recursive: true });
        fs.copyFileSync(configFile, path.join(root, 'vite.config.ts'));
        fs.writeFileSync(path.join(root, 'index.html'), '<script type="module" src="/src/main.ts"></script>');
        fs.writeFileSync(path.join(root, 'package.json'), '{"type":"module"}');
        fs.writeFileSync(path.join(library, 'package.json'), '{"name":"@maxvue/max-use","type":"module","exports":"./index.js"}');
        fs.writeFileSync(libraryFile, 'export const value = 1; if (import.meta.hot) import.meta.hot.accept();');
        fs.symlinkSync(library, path.join(root, 'node_modules/@maxvue/max-use'), 'dir');
        const sourceFile = path.join(root, 'src/main.ts');
        fs.writeFileSync(sourceFile, 'import { value } from "@maxvue/max-use"; console.log(value); if (import.meta.hot) import.meta.hot.accept();');

        for (const folder of excludedFolders) {
            const directory = path.join(root, folder, 'fixture');
            fs.mkdirSync(directory, { recursive: true });
            fs.writeFileSync(path.join(directory, 'index.html'), '<p>Fixture</p>');
            fs.writeFileSync(path.join(directory, 'tsconfig.json'), '{}');
        }

        server = await createServer({
            root,
            configFile: path.join(root, 'vite.config.ts'),
            configLoader: 'runner',
            logLevel: 'silent',
            server: {
                port: 0,
                fs: { allow: [root, library] }
            },
            optimizeDeps: { noDiscovery: true, include: [] }
        });
        const errors: Error[] = [];
        server.watcher.on('error', (error) => errors.push(error));
        await server.listen();
        const activeServer = server;
        const sent = vi.spyOn(server.ws, 'send');
        const events: string[] = [];
        server.watcher.on('all', (_event, file) => events.push(normalizePath(file)));

        const client = server.environments.client!;
        const main = await client.transformRequest('/src/main.ts');
        expect(main?.code).toContain('/@fs/');
        await client.transformRequest(`/@fs/${normalizePath(libraryFile)}`);
        await expect.poll(() => Object.keys(activeServer.watcher.getWatched())).toContain(normalizePath(path.dirname(sourceFile)));
        await expect.poll(() => Object.keys(activeServer.watcher.getWatched())).toContain(normalizePath(library));

        // A asserção antecede o ciclo para que uma regressão falhe sem recursão descontrolada.
        const watched = Object.keys(server.watcher.getWatched());
        for (const folder of excludedFolders) {
            const directory = normalizePath(path.join(root, folder));
            expect(watched.filter((entry) => entry === directory || entry.startsWith(`${directory}/`))).toEqual([]);
        }

        for (const folder of excludedFolders) {
            const directory = path.join(root, folder, 'fixture');
            const externalRepo = path.join(directory, 'external-huge-repo');
            fs.mkdirSync(externalRepo);
            fs.symlinkSync(externalRepo, path.join(externalRepo, 'self_cycle'), 'dir');
            fs.writeFileSync(path.join(directory, 'index.html'), '<p>Atualizado</p>');
            fs.writeFileSync(path.join(directory, 'tsconfig.json'), '{"compilerOptions":{}}');
        }
        await setTimeout(250);
        expect(errors).toEqual([]);
        expect(events).toEqual([]);
        // O UnoCSS pode emitir uma atualização vazia durante a extração inicial.
        expect(sent).not.toHaveBeenCalledWith(expect.objectContaining({ type: 'full-reload' }));
        expect(sent).not.toHaveBeenCalledWith(expect.objectContaining({ updates: expect.arrayContaining([expect.anything()]) }));
        sent.mockClear();

        fs.appendFileSync(sourceFile, '\nconsole.log("atualizado");');
        await expect.poll(() => sent.mock.calls).toContainEqual([expect.objectContaining({
            type: 'update',
            updates: expect.arrayContaining([expect.objectContaining({ path: '/src/main.ts' })])
        })]);
        expect(events).toContain(normalizePath(sourceFile));
        sent.mockClear();

        fs.appendFileSync(libraryFile, '\nconsole.log("biblioteca atualizada");');
        await expect.poll(() => sent.mock.calls).toContainEqual([expect.objectContaining({
            type: 'update',
            updates: expect.arrayContaining([expect.objectContaining({ path: expect.stringContaining(normalizePath(libraryFile)) })])
        })]);
        expect(events).toContain(normalizePath(libraryFile));
        expect(errors).toEqual([]);
        expect(server.httpServer?.listening).toBe(true);
    }, 20000);
});
