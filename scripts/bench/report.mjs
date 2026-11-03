#!/usr/bin/env node
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const rootDir = path.resolve(__dirname, '../..');
const envPath = path.join(rootDir, 'bench/environment.json');
const baselinePath = path.join(rootDir, 'bench/baseline.json');

function readCpuGovernor() {
    try {
        return fs
            .readFileSync('/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor', 'utf8')
            .trim();
    } catch {
        return 'unknown';
    }
}

export function collectEnvironmentProfile() {
    const cpus = os.cpus();
    return {
        schemaVersion: 1,
        capturedAtIso: new Date().toISOString(),
        platform: os.platform(),
        arch: os.arch(),
        kernelRelease: os.release(),
        cpuModel: cpus[0]?.model ?? 'unknown',
        cpuLogicalCores: cpus.length,
        cpuGovernor: readCpuGovernor(),
        totalMemoryBytes: os.totalmem(),
        displayServer: process.env.WAYLAND_DISPLAY
            ? 'wayland'
            : process.env.DISPLAY
                ? 'x11'
                : 'headless',
        compositor: process.env.XDG_CURRENT_DESKTOP ?? 'unknown'
    };
}

export function computeSummary(values) {
    if (!Array.isArray(values) || values.length === 0) {
        return { count: 0, p50: 0, p95: 0, p99: 0, mean: 0, stdev: 0 };
    }
    const sorted = [...values].sort((a, b) => a - b);
    const pick = (pct) => {
        const idx = Math.min(
            sorted.length - 1,
            Math.max(0, Math.round((pct / 100) * (sorted.length - 1)))
        );
        return sorted[idx] ?? 0;
    };
    const mean = sorted.reduce((acc, v) => acc + v, 0) / sorted.length;
    const variance =
        sorted.reduce((acc, v) => acc + (v - mean) ** 2, 0) / sorted.length;
    return {
        count: sorted.length,
        p50: Number(pick(50).toFixed(2)),
        p95: Number(pick(95).toFixed(2)),
        p99: Number(pick(99).toFixed(2)),
        mean: Number(mean.toFixed(2)),
        stdev: Number(Math.sqrt(variance).toFixed(2))
    };
}

function main() {
    const currentEnv = collectEnvironmentProfile();
    const baseline = JSON.parse(fs.readFileSync(baselinePath, 'utf8'));
    const recordedEnv = fs.existsSync(envPath)
        ? JSON.parse(fs.readFileSync(envPath, 'utf8'))
        : null;

    const status =
        baseline.status === 'pending_measurement' ||
        !recordedEnv ||
        recordedEnv.cpuModel !== currentEnv.cpuModel
            ? 'not_comparable'
            : 'comparable';

    const report = {
        status,
        baselineStatus: baseline.status,
        currentEnvironment: currentEnv,
        recordedEnvironmentId: recordedEnv?.environmentId ?? null
    };

    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
    main();
}
