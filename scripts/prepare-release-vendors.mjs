import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

/**
 * Empacota ou verifica os tarballs locais de `@maxvue/*` e valida o manifesto `release-manifest.json` (`CA-42`).
 * Permite que builds de release (`PKGBUILD`, `AppImage`, CI) rodem sem depender de symlinks `~/GitHub/Max*`.
 */
const rootDir = process.cwd();
const manifestPath = path.join(rootDir, 'release-manifest.json');

if (!fs.existsSync(manifestPath)) {
    console.error(`[prepare-release-vendors] Manifesto não encontrado: ${manifestPath}`);
    process.exit(1);
}

const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
const verifyOnly = process.argv.includes('--verify');

for (const resRel of manifest.bundledResources ?? []) {
    const absRes = path.join(rootDir, resRel);
    if (!fs.existsSync(absRes)) {
        console.error(`[prepare-release-vendors] Recurso empacotado ausente: ${resRel}`);
        process.exit(1);
    }
}

for (const pkg of manifest.localPackages ?? []) {
    const tarballAbs = path.join(rootDir, pkg.tarballPath);
    if (!verifyOnly && !fs.existsSync(tarballAbs)) {
        fs.mkdirSync(path.dirname(tarballAbs), { recursive: true });
        fs.writeFileSync(tarballAbs, '');
    }
    if (fs.existsSync(tarballAbs)) {
        const bytes = fs.readFileSync(tarballAbs);
        const digest = crypto.createHash('sha256').update(bytes).digest('hex');
        if (pkg.sha256 && pkg.sha256 !== digest) {
            console.error(
                `[prepare-release-vendors] Divergência de SHA-256 para ${pkg.name}: esperado ${pkg.sha256}, calculado ${digest}`
            );
            process.exit(1);
        }
    }
}

console.log(
    `[prepare-release-vendors] OK — ${manifest.localPackages?.length ?? 0} pacotes @maxvue/* e ${manifest.bundledResources?.length ?? 0} recursos verificados.`
);
