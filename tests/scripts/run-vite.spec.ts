import { spawn, type ChildProcess } from 'node:child_process';
import { once } from 'node:events';
import fs from 'node:fs';
import path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

const runner = path.resolve('scripts/run-vite.mjs');

describe('supervisão do frontend DEV', () => {
    let root: string;
    const children: ChildProcess[] = [];

    beforeEach(() => {
        fs.mkdirSync('.tmp', { recursive: true });
        root = fs.mkdtempSync(path.resolve('.tmp/run-vite-'));
        fs.mkdirSync(path.join(root, 'node_modules/vite/bin'), { recursive: true });
    });

    afterEach(async () => {
        for (const child of children.splice(0)) {
            if (child.exitCode === null && child.signalCode === null) {
                child.kill('SIGTERM');
                await once(child, 'exit');
            }
        }
        fs.rmSync(root, { recursive: true, force: true });
    });

    function start(source: string) {
        fs.writeFileSync(path.join(root, 'node_modules/vite/bin/vite.js'), source);
        const child = spawn(process.execPath, [runner], { cwd: root, stdio: ['ignore', 'pipe', 'pipe'] });
        children.push(child);
        const output = { stdout: '', stderr: '' };
        child.stdout!.on('data', (chunk) => { output.stdout += String(chunk); });
        child.stderr!.on('data', (chunk) => { output.stderr += String(chunk); });
        return { child, output, exited: once(child, 'exit') };
    }

    it('preserva stdout, stderr e o código original da falha do Vite', async () => {
        const { child, output, exited } = start('console.log("frontend stdout"); console.error("frontend stderr"); process.exitCode = 23;');
        expect((await exited)[0]).toBe(23);
        expect(output.stdout).toContain('frontend stdout');
        expect(output.stderr).toContain('frontend stderr');
        expect(output.stderr).toMatch(/frontend_dev.*"pid":\d+.*"code":23.*"signal":null/);
        expect(output.stderr).toContain('"event":"exit"');
        expect(output.stderr).toMatch(/"timestamp":"\d{4}-/);
        expect(child.signalCode).toBeNull();
    });

    it('identifica o sinal que encerrou o Vite sem convertê-lo silenciosamente em código 1', async () => {
        const { output, exited } = start('process.kill(process.pid, "SIGTERM");');
        expect((await exited)[0]).toBe(143);
        expect(output.stderr).toContain('"signal":"SIGTERM"');
    });

    it('repassa SIGTERM e aguarda o frontend encerrar sem deixá-lo órfão', async () => {
        const { child, output, exited } = start('console.log(`pronto:${process.pid}`); setInterval(() => {}, 1000);');
        await expect.poll(() => output.stdout, { timeout: 1500 }).toMatch(/pronto:\d+/);
        const pid = Number(output.stdout.match(/pronto:(\d+)/)![1]);
        child.kill('SIGTERM');
        expect((await exited)[0]).toBe(143);
        expect(output.stderr).toContain('"event":"signal"');
        expect(() => process.kill(pid, 0)).toThrow();
    });

    it('mantém host e porta estritos e evita cache de configuração em node_modules compartilhado', async () => {
        const { output, exited } = start('console.log(JSON.stringify(process.argv.slice(2)));');
        expect((await exited)[0]).toBe(0);
        expect(JSON.parse(output.stdout.trim())).toEqual(['--host', '127.0.0.1', '--port', '5177', '--strictPort', '--configLoader', 'runner']);
    });
});
