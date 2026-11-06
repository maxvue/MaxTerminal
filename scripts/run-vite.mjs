#!/usr/bin/env node

/** Supervisiona o Vite sem ocultar sua saída nem perder o código/sinal de término. */
import { spawn } from 'node:child_process';
import { constants } from 'node:os';
import path from 'node:path';

const child = spawn(process.execPath, [
    path.resolve('node_modules/vite/bin/vite.js'),
    '--host', '127.0.0.1',
    '--port', '5177',
    '--strictPort',
    // Evita gravar o bundle da configuração em node_modules compartilhado com a raiz.
    '--configLoader', 'runner',
    ...process.argv.slice(2)
], { stdio: 'inherit' });

let requestedSignal = null;

function report(event, details = {}) {
    console.error(JSON.stringify({
        component: 'frontend_dev',
        timestamp: new Date().toISOString(),
        event,
        pid: child.pid ?? null,
        ...details
    }));
}

function forwardSignal(signal) {
    requestedSignal = signal;
    report('signal', { signal });
    // Mantém a supervisão até o filho efetivamente encerrar, sem SIGKILL automático.
    child.kill(signal);
}

function detachSignals() {
    process.off('SIGINT', forwardSignal);
    process.off('SIGTERM', forwardSignal);
}

process.on('SIGINT', forwardSignal);
process.on('SIGTERM', forwardSignal);

child.once('spawn', () => report('start'));
child.once('error', (error) => {
    report('error', { message: error.message });
    detachSignals();
    process.exitCode = 1;
});
child.once('exit', (code, signal) => {
    report('exit', { code, signal, requestedSignal });
    detachSignals();
    const exitSignal = signal ?? requestedSignal;
    process.exitCode = exitSignal ? 128 + constants.signals[exitSignal] : (code ?? 1);
});
