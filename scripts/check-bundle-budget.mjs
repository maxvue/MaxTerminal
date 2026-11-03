#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import zlib from 'node:zlib';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const rootDir = path.resolve(__dirname, '..');
const distDir = path.join(rootDir, 'dist');
const baselinePath = path.join(rootDir, 'bench/baseline.json');
const envPath = path.join(rootDir, 'bench/environment.json');

function walkFiles(dir) {
    if (!fs.existsSync(dir)) {
        return [];
    }
    const entries = fs.readdirSync(dir, { withFileTypes: true });
    const files = [];
    for (const entry of entries) {
        const full = path.join(dir, entry.name);
        if (entry.isDirectory()) {
            files.push(...walkFiles(full));
        } else if (entry.isFile()) {
            files.push(full);
        }
    }
    return files;
}

function detectEntryJsFiles(indexHtmlPath, allFiles) {
    if (!fs.existsSync(indexHtmlPath)) {
        return new Set();
    }
    const html = fs.readFileSync(indexHtmlPath, 'utf8');
    const matches = [...html.matchAll(/src=["']\/?([^"']+\.js)["']/g)];
    const referenced = new Set(matches.map((m) => path.resolve(distDir, m[1])));
    if (referenced.size > 0) {
        return referenced;
    }
    const fallback = allFiles.filter((f) => path.basename(f).startsWith('index-') && f.endsWith('.js'));
    return new Set(fallback);
}

export function analyzeDistBundle(targetDistDir = distDir) {
    const allFiles = walkFiles(targetDistDir);
    if (allFiles.length === 0) {
        throw new Error(`Diretório de build vazio ou inexistente: ${targetDistDir}`);
    }

    const indexHtmlPath = path.join(targetDistDir, 'index.html');
    const entrySet = detectEntryJsFiles(indexHtmlPath, allFiles);

    const categories = {
        entryJs: { rawBytes: 0, gzipBytes: 0, files: 0 },
        asyncJs: { rawBytes: 0, gzipBytes: 0, files: 0 },
        css: { rawBytes: 0, gzipBytes: 0, files: 0 },
        assets: { rawBytes: 0, gzipBytes: 0, files: 0 },
        total: { rawBytes: 0, gzipBytes: 0, files: 0 }
    };

    for (const filePath of allFiles) {
        if (filePath.endsWith('.map')) {
            continue;
        }
        const buf = fs.readFileSync(filePath);
        const rawBytes = buf.byteLength;
        const gzipBytes = zlib.gzipSync(buf, { level: 9 }).byteLength;

        let bucket = 'assets';
        if (filePath.endsWith('.js')) {
            bucket = entrySet.has(filePath) ? 'entryJs' : 'asyncJs';
        } else if (filePath.endsWith('.css')) {
            bucket = 'css';
        }

        categories[bucket].rawBytes += rawBytes;
        categories[bucket].gzipBytes += gzipBytes;
        categories[bucket].files += 1;

        categories.total.rawBytes += rawBytes;
        categories.total.gzipBytes += gzipBytes;
        categories.total.files += 1;
    }

    return categories;
}

function main() {
    const args = new Set(process.argv.slice(2));
    const isSmoke = args.has('--smoke');
    const isStrictGate = args.has('--strict-gate');

    const categories = analyzeDistBundle(distDir);
    const baseline = JSON.parse(fs.readFileSync(baselinePath, 'utf8'));
    const env = fs.existsSync(envPath)
        ? JSON.parse(fs.readFileSync(envPath, 'utf8'))
        : null;

    const budget = baseline.bundleBudget ?? {};
    const violations = [];

    if (budget.entryJsRawMaxBytes && categories.entryJs.rawBytes > budget.entryJsRawMaxBytes) {
        violations.push(`entryJs.rawBytes (${categories.entryJs.rawBytes}) > ${budget.entryJsRawMaxBytes}`);
    }
    if (budget.entryJsGzipMaxBytes && categories.entryJs.gzipBytes > budget.entryJsGzipMaxBytes) {
        violations.push(`entryJs.gzipBytes (${categories.entryJs.gzipBytes}) > ${budget.entryJsGzipMaxBytes}`);
    }
    if (budget.asyncJsRawMaxBytes && categories.asyncJs.rawBytes > budget.asyncJsRawMaxBytes) {
        violations.push(`asyncJs.rawBytes (${categories.asyncJs.rawBytes}) > ${budget.asyncJsRawMaxBytes}`);
    }
    if (budget.asyncJsGzipMaxBytes && categories.asyncJs.gzipBytes > budget.asyncJsGzipMaxBytes) {
        violations.push(`asyncJs.gzipBytes (${categories.asyncJs.gzipBytes}) > ${budget.asyncJsGzipMaxBytes}`);
    }
    if (budget.cssRawMaxBytes && categories.css.rawBytes > budget.cssRawMaxBytes) {
        violations.push(`css.rawBytes (${categories.css.rawBytes}) > ${budget.cssRawMaxBytes}`);
    }
    if (budget.cssGzipMaxBytes && categories.css.gzipBytes > budget.cssGzipMaxBytes) {
        violations.push(`css.gzipBytes (${categories.css.gzipBytes}) > ${budget.cssGzipMaxBytes}`);
    }
    if (budget.totalRawMaxBytes && categories.total.rawBytes > budget.totalRawMaxBytes) {
        violations.push(`total.rawBytes (${categories.total.rawBytes}) > ${budget.totalRawMaxBytes}`);
    }
    if (budget.totalGzipMaxBytes && categories.total.gzipBytes > budget.totalGzipMaxBytes) {
        violations.push(`total.gzipBytes (${categories.total.gzipBytes}) > ${budget.totalGzipMaxBytes}`);
    }

    const isPendingBaseline =
        baseline.status === 'pending_measurement' ||
        !env ||
        baseline.environmentId !== env.environmentId;

    let comparisonStatus = 'passed';
    if (violations.length > 0) {
        comparisonStatus = 'exceeded';
    } else if (isPendingBaseline) {
        comparisonStatus = 'not_comparable';
    }

    const report = {
        status: comparisonStatus,
        baselineStatus: baseline.status,
        categories,
        violations
    };

    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);

    if (violations.length > 0) {
        process.exit(1);
    }
    if (isStrictGate && comparisonStatus === 'not_comparable' && !isSmoke) {
        process.exit(2);
    }
}

if (import.meta.url === `file://${process.argv[1]}`) {
    main();
}
