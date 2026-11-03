#!/usr/bin/env node

/**
 * Orquestrador canônico de validação de qualidade do MaxTerminal.
 * Executa sequencialmente os 10 passos de validação frontend e backend Rust,
 * interrompendo a execução no primeiro erro com código de saída != 0.
 */

import { spawnSync } from 'node:child_process';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const workspaceRoot = fileURLToPath(new URL('..', import.meta.url));
const srcTauriDir = path.join(workspaceRoot, 'src-tauri');

// Garante que os binários e toolchains do Cargo/Rustup estejam acessíveis mesmo com HOME isolado
const realHome = process.env.MAXCODE_REAL_HOME || os.homedir();
const cargoHome = process.env.CARGO_HOME || path.join(realHome, '.cargo');
const rustupHome = process.env.RUSTUP_HOME || path.join(realHome, '.rustup');
const env = {
    ...process.env,
    CARGO_HOME: cargoHome,
    RUSTUP_HOME: rustupHome,
    PATH: `${path.join(cargoHome, 'bin')}:${path.join(os.homedir(), '.cargo', 'bin')}:${process.env.PATH ?? ''}`
};

const checks = [
    {
        name: '1/10: Verificação de bibliotecas locais (--check)',
        cmd: 'node',
        args: ['scripts/sync-local-libs.mjs', '--check'],
        cwd: workspaceRoot
    },
    {
        name: '2/10: Verificação de estilo e linting JavaScript/TypeScript/Vue (ESLint)',
        cmd: 'npm',
        args: ['run', 'lint'],
        cwd: workspaceRoot
    },
    {
        name: '3/10: Verificação de estilo CSS/SCSS (Stylelint)',
        cmd: 'npm',
        args: ['run', 'lint:style'],
        cwd: workspaceRoot
    },
    {
        name: '4/10: Checagem estrita de tipos TypeScript (tsgo)',
        cmd: 'npm',
        args: ['run', 'typecheck'],
        cwd: workspaceRoot
    },
    {
        name: '5/10: Checagem estrita de tipos SFC Vue (vue-tsc)',
        cmd: 'npm',
        args: ['run', 'typecheck:vue'],
        cwd: workspaceRoot
    },
    {
        name: '6/10: Execução dos testes automatizados com cobertura (Vitest)',
        cmd: 'npm',
        args: ['run', 'test:coverage'],
        cwd: workspaceRoot
    },
    {
        name: '7/10: Compilação de produção da aplicação frontend (Vite build)',
        cmd: 'npm',
        args: ['run', 'build'],
        cwd: workspaceRoot
    },
    {
        name: '8/10: Verificação de formatação Rust (cargo fmt)',
        cmd: 'cargo',
        args: ['fmt', '--all', '--check'],
        cwd: srcTauriDir
    },
    {
        name: '9/10: Análise estática do código Rust (cargo clippy)',
        cmd: 'cargo',
        args: ['clippy', '--locked', '--workspace', '--all-targets', '--', '-D', 'warnings'],
        cwd: srcTauriDir
    },
    {
        name: '10/10: Testes unitários e de integração do backend Rust (cargo test)',
        cmd: 'cargo',
        args: ['test', '--workspace', '--locked'],
        cwd: srcTauriDir
    }
];

console.log('🚀 Iniciando bateria completa de validação de qualidade do MaxTerminal...\n');

for (const check of checks) {
    console.log(`▶ Executando: ${check.name}`);
    const result = spawnSync(check.cmd, check.args, {
        cwd: check.cwd,
        env,
        stdio: 'inherit'
    });

    if (result.status !== 0) {
        console.error(`\n✖ Falha na etapa: ${check.name} (código de saída: ${result.status})`);
        process.exit(result.status ?? 1);
    }
    console.log(`✔ Concluído com sucesso: ${check.name}\n`);
}

console.log('🎉 Todos os 10 passos de validação passaram com sucesso!');
