import { execFileSync, spawn, type ChildProcess } from 'node:child_process';
import { once } from 'node:events';
import fs from 'node:fs';
import path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

const source = fs.readFileSync(path.resolve('.vscode/run-dev.sh'), 'utf8');
// Carrega somente as funções: nunca executa a task real nem sua limpeza global.
const functions = source.match(/^\w+\(\) \{[\s\S]*?^\}/gm)?.join('\n') ?? '';

describe('limpeza da task DEV', () => {
    let root: string;
    let app: string;
    let backend: string;
    const children: ChildProcess[] = [];

    beforeEach(() => {
        fs.mkdirSync('.tmp', { recursive: true });
        root = fs.mkdtempSync(path.resolve('.tmp/run-dev-'));
        app = path.join(root, 'app com espaços');
        backend = path.join(app, 'src-tauri/target/debug/maxterminal');
        fs.mkdirSync(path.dirname(backend), { recursive: true });
        fs.copyFileSync('/usr/bin/sleep', backend);
    });

    afterEach(async () => {
        for (const child of children.splice(0)) {
            if (child.exitCode === null && child.signalCode === null) {
                child.kill('SIGKILL');
                await once(child, 'exit');
            }
        }
        const pidFile = path.join(root, 'backend.pid');
        if (fs.existsSync(pidFile)) {
            const pid = fs.readFileSync(pidFile, 'utf8').trim();
            try {
                const exe = fs.readlinkSync(`/proc/${pid}/exe`);
                if (exe === backend || exe === path.join(root, 'bin/slow-task')) {
                    process.kill(Number(pid), 'SIGKILL');
                }
            } catch {
                // O processo sintético já encerrou.
            }
        }
        fs.rmSync(root, { recursive: true, force: true });
    });

    async function startBackend(cwd: string): Promise<ChildProcess> {
        const child = spawn(backend, ['60'], { cwd, stdio: 'ignore' });
        children.push(child);
        await once(child, 'spawn');
        return child;
    }

    function runFunction(command: string): void {
        execFileSync('bash', ['-c', `${functions}\n${command}`], {
            cwd: app,
            env: { ...process.env, LOCKFILE: path.join(root, 'task.pid') },
            stdio: 'pipe',
            timeout: 10_000
        });
    }

    async function startTask(npmScript: string): Promise<ChildProcess> {
        fs.mkdirSync(path.join(app, '.vscode'), { recursive: true });
        fs.mkdirSync(path.join(root, 'bin'), { recursive: true });
        fs.mkdirSync(path.join(root, 'home'), { recursive: true });
        fs.writeFileSync(path.join(app, '.vscode/run-dev.sh'), source);
        fs.writeFileSync(path.join(root, 'bin/npm'), `#!/bin/bash\n${npmScript}\n`, { mode: 0o755 });
        fs.copyFileSync('/usr/bin/sleep', path.join(root, 'bin/slow-task'));
        const child = spawn('bash', ['.vscode/run-dev.sh'], {
            cwd: app,
            env: {
                ...process.env,
                HOME: path.join(root, 'home'),
                TMPDIR: root,
                PATH: `${path.join(root, 'bin')}:${process.env.PATH}`,
                MAXTERMINAL_RUN_DEV_ACTIVE: '0',
                FIXTURE_BACKEND: backend,
                FIXTURE_PID_FILE: path.join(root, 'backend.pid')
            },
            stdio: 'ignore'
        });
        children.push(child);
        await once(child, 'spawn');
        return child;
    }

    function backendIsRunning(): boolean {
        const pid = fs.readFileSync(path.join(root, 'backend.pid'), 'utf8').trim();
        try {
            const stat = fs.readFileSync(`/proc/${pid}/stat`, 'utf8');
            return stat.slice(stat.lastIndexOf(')') + 2).split(' ')[0] !== 'Z';
        } catch {
            return false;
        }
    }

    it.each(['.', 'src-tauri'])('encerra o backend com cwd em %s', async (cwd) => {
        const child = await startBackend(path.join(app, cwd));
        runFunction("kill_in_project 'target/debug/maxterminal'");
        await expect.poll(() => child.signalCode).toBe('SIGTERM');
    });

    it('preserva o backend de outro projeto ou worktree', async () => {
        const other = path.join(root, 'other/src-tauri');
        fs.mkdirSync(other, { recursive: true });
        const child = await startBackend(other);
        runFunction("kill_in_project 'target/debug/maxterminal'");
        expect(child.signalCode).toBeNull();
        expect(() => process.kill(child.pid!, 0)).not.toThrow();
    });

    it.each(['.bin/vite', '.bin/tauri', 'vite/bin/vite.js', '@tauri-apps/cli/tauri.js'])('reconhece a entrada Node %s', async (entry) => {
        const script = path.join(app, 'node_modules', entry);
        fs.mkdirSync(path.dirname(script), { recursive: true });
        fs.writeFileSync(script, 'setInterval(() => {}, 1000);');
        const child = spawn(process.execPath, [script], { cwd: app, stdio: 'ignore' });
        children.push(child);
        await once(child, 'spawn');
        runFunction(`kill_in_project 'node_modules/.bin/${entry.includes('vite') ? 'vite' : 'tauri'}'`);
        await expect.poll(() => child.signalCode).toBe('SIGTERM');
    });

    it('encerra também um executável substituído enquanto estava em execução', async () => {
        const child = await startBackend(path.join(app, 'src-tauri'));
        fs.unlinkSync(backend);
        runFunction("kill_in_project 'target/debug/maxterminal'");
        await expect.poll(() => child.signalCode).toBe('SIGTERM');
    });

    it('resolve CARGO_TARGET_DIR relativo a src-tauri', async () => {
        const custom = path.join(app, 'src-tauri/custom-target/debug/maxterminal');
        fs.mkdirSync(path.dirname(custom), { recursive: true });
        fs.copyFileSync(backend, custom);
        const child = spawn(custom, ['60'], { cwd: path.join(app, 'src-tauri'), stdio: 'ignore' });
        children.push(child);
        await once(child, 'spawn');
        runFunction("CARGO_TARGET_DIR=custom-target; kill_in_project 'target/debug/maxterminal'");
        await expect.poll(() => child.signalCode).toBe('SIGTERM');
    });

    it('aguarda o backend concluir o encerramento antes de retornar', async () => {
        fs.copyFileSync(process.execPath, backend);
        const child = spawn(backend, ['-e', 'process.on("SIGTERM", () => setTimeout(() => process.exit(0), 250)); console.log("pronto"); setInterval(() => {}, 1000);'], {
            cwd: path.join(app, 'src-tauri'),
            stdio: ['ignore', 'pipe', 'ignore']
        });
        children.push(child);
        await once(child.stdout!, 'data');
        const started = performance.now();
        runFunction("kill_in_project 'target/debug/maxterminal'");
        expect(performance.now() - started).toBeGreaterThanOrEqual(200);
        expect(fs.existsSync(`/proc/${child.pid}/exe`)).toBe(false);
    });

    it('cancela a reinicialização sem SIGKILL se o backend não encerrar', async () => {
        fs.copyFileSync(process.execPath, backend);
        const child = spawn(backend, ['-e', 'process.on("SIGTERM", () => {}); console.log("pronto"); setInterval(() => {}, 1000);'], {
            cwd: path.join(app, 'src-tauri'),
            stdio: ['ignore', 'pipe', 'ignore']
        });
        children.push(child);
        await once(child.stdout!, 'data');
        expect(() => runFunction("kill_in_project 'target/debug/maxterminal'")).toThrow();
        expect(() => process.kill(child.pid!, 0)).not.toThrow();
    }, 15_000);

    it('não trata uma menção ao binário como identidade do processo', async () => {
        const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)', 'target/debug/maxterminal'], {
            cwd: app,
            stdio: 'ignore'
        });
        children.push(child);
        await once(child, 'spawn');
        runFunction("kill_in_project 'target/debug/maxterminal'");
        await new Promise(setImmediate);
        expect(() => process.kill(child.pid!, 0)).not.toThrow();
    });

    it('não encerra um PID alheio encontrado no arquivo de controle', async () => {
        const child = await startBackend(path.join(app, 'src-tauri'));
        fs.writeFileSync(path.join(root, 'task.pid'), String(child.pid));
        runFunction('take_over');
        await new Promise(setImmediate);
        expect(() => process.kill(child.pid!, 0)).not.toThrow();
    });

    it('preserva o código da falha e limpa o backend iniciado pela task', async () => {
        const task = await startTask('cd src-tauri\n"$FIXTURE_BACKEND" 60 </dev/null >/dev/null 2>&1 &\necho $! > "$FIXTURE_PID_FILE"\nexit 23');
        const [code] = await once(task, 'exit');
        expect(code).toBe(23);
        expect(backendIsRunning()).toBe(false);
        expect(fs.readdirSync(root).filter((name) => name.endsWith('.pid') && name !== 'backend.pid')).toEqual([]);
    }, 20_000);

    it('limpa o backend ao receber SIGTERM enquanto aguarda a task', async () => {
        const task = await startTask('echo $$ > "$FIXTURE_PID_FILE"\ncd src-tauri\nexec "$FIXTURE_BACKEND" 60');
        await expect.poll(() => fs.existsSync(path.join(root, 'backend.pid')), { timeout: 10_000 }).toBe(true);
        const exited = once(task, 'exit');
        task.kill('SIGTERM');
        const [code] = await exited;
        expect(code).toBe(143);
        expect(backendIsRunning()).toBe(false);
    }, 20_000);

    it('encerra a task mesmo quando o Tauri ainda não foi iniciado', async () => {
        const task = await startTask('echo $$ > "$FIXTURE_PID_FILE"\nexec "$(dirname "$0")/slow-task" 60');
        await expect.poll(() => fs.existsSync(path.join(root, 'backend.pid')), { timeout: 10_000 }).toBe(true);
        const exited = once(task, 'exit');
        task.kill('SIGTERM');
        expect((await exited)[0]).toBe(143);
        expect(backendIsRunning()).toBe(false);
    }, 20_000);

    it('aguarda a task anterior liberar seus processos antes de reiniciar', async () => {
        const first = await startTask('echo $$ > "$FIXTURE_PID_FILE"\ncd src-tauri\nexec "$FIXTURE_BACKEND" 60');
        await expect.poll(() => fs.existsSync(path.join(root, 'backend.pid')), { timeout: 10_000 }).toBe(true);
        const firstExited = once(first, 'exit');
        const second = await startTask('exit 0');
        const secondExited = once(second, 'exit');
        expect((await firstExited)[0]).toBe(143);
        expect((await secondExited)[0]).toBe(0);
        expect(backendIsRunning()).toBe(false);
    }, 20_000);
});
