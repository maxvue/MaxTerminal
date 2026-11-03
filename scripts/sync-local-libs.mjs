#!/usr/bin/env node

/**
 * Script de sincronização de bibliotecas locais (@maxvue/*) para o MaxTerminal.
 * Valida integridade antes da aplicação, cria symlinks absolutos e oferece modo --check.
 */

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ALLOWED_PACKAGES = new Set([
    '@maxvue/max-components-ui',
    '@maxvue/max-use',
    '@maxvue/max-pinia'
]);

/**
 * Coleta recursivamente os caminhos de arquivos declarados em exports/main/module/types.
 *
 * @param {unknown} value Estrutura de exportação do package.json.
 * @param {Set<string>} paths Acumulador de caminhos encontrados.
 * @returns {Set<string>}
 */
function collectExportPaths(value, paths = new Set()) {
    if (typeof value === 'string') {
        paths.add(value);
    } else if (value && typeof value === 'object') {
        for (const item of Object.values(value)) {
            collectExportPaths(item, paths);
        }
    }
    return paths;
}

/**
 * Sincroniza bibliotecas locais criando symlinks absolutos em node_modules/@maxvue/*.
 *
 * @param {object} [options] Opções de execução.
 * @param {string} [options.workspaceRoot] Raiz da worktree da aplicação.
 * @param {string} [options.libsRoot] Diretório base onde residem os repositórios locais.
 * @param {boolean} [options.checkOnly] Se verdadeiro, apenas valida sem alterar o sistema de arquivos.
 * @param {(line: string) => void} [options.logger] Função para saída de logs.
 * @returns {{ ok: boolean, results: Array<{ pkg: string, target?: string, error?: string }> }}
 */
export function syncLocalLibs(options = {}) {
    const {
        workspaceRoot = process.cwd(),
        libsRoot: customLibsRoot,
        checkOnly = false,
        logger = console.log
    } = options;

    const results = [];

    // 1. Resolução do diretório base das bibliotecas
    const realHome = process.env.MAXCODE_REAL_HOME || os.homedir();
    const defaultFallback = fs.existsSync('/home/johnattas/GitHub')
        ? '/home/johnattas/GitHub'
        : path.join(realHome, 'GitHub');
    let rawLibsRoot = customLibsRoot ?? process.env.MAX_LIBS_ROOT ?? defaultFallback;
    if (typeof rawLibsRoot === 'string') {
        rawLibsRoot = rawLibsRoot.trim();
        if (rawLibsRoot === '~') {
            rawLibsRoot = realHome;
        } else if (rawLibsRoot.startsWith('~/')) {
            rawLibsRoot = path.join(realHome, rawLibsRoot.slice(2));
        }
    }

    if (typeof rawLibsRoot !== 'string' || rawLibsRoot.length === 0 || !path.isAbsolute(rawLibsRoot)) {
        const errorMsg = `O caminho de MAX_LIBS_ROOT deve ser absoluto: "${String(rawLibsRoot)}"`;
        logger(`✖ ${errorMsg}`);
        return {
            ok: false,
            results: [{ pkg: 'all', error: errorMsg }]
        };
    }

    const libsRoot = path.normalize(rawLibsRoot);

    if (!fs.existsSync(libsRoot)) {
        const errorMsg = `Diretório base de bibliotecas locais não existe: ${libsRoot}`;
        logger(`✖ ${errorMsg}`);
        return {
            ok: false,
            results: [{ pkg: 'all', error: errorMsg }]
        };
    }

    // 2. Leitura do manifesto package.json da aplicação
    const appPkgPath = path.join(workspaceRoot, 'package.json');
    if (!fs.existsSync(appPkgPath)) {
        const errorMsg = `Manifesto package.json da aplicação não encontrado em: ${appPkgPath}`;
        logger(`✖ ${errorMsg}`);
        return {
            ok: false,
            results: [{ pkg: 'all', error: errorMsg }]
        };
    }

    let appPkg;
    try {
        appPkg = JSON.parse(fs.readFileSync(appPkgPath, 'utf8'));
    } catch (err) {
        const errorMsg = `Erro ao interpretar package.json da aplicação: ${err.message}`;
        logger(`✖ ${errorMsg}`);
        return {
            ok: false,
            results: [{ pkg: 'all', error: errorMsg }]
        };
    }

    const maxLocalLibs = appPkg.maxLocalLibs;
    if (!maxLocalLibs || typeof maxLocalLibs !== 'object') {
        const errorMsg = 'Campo "maxLocalLibs" ausente ou inválido no package.json da aplicação.';
        logger(`✖ ${errorMsg}`);
        return {
            ok: false,
            results: [{ pkg: 'all', error: errorMsg }]
        };
    }

    // 3. Fase de validação estrita (100% somente-leitura)
    const plannedActions = [];
    let hasValidationError = false;

    for (const [pkg, folder] of Object.entries(maxLocalLibs)) {
        if (!ALLOWED_PACKAGES.has(pkg)) {
            const error = `Pacote local não permitido no manifesto: ${pkg}`;
            results.push({ pkg, error });
            logger(`✖ ${pkg}: ${error}`);
            hasValidationError = true;
            continue;
        }

        const candidateSrcDir = path.resolve(libsRoot, folder);
        if (!fs.existsSync(candidateSrcDir)) {
            const error = `Diretório da biblioteca local não encontrado: ${candidateSrcDir}`;
            results.push({ pkg, error });
            logger(`✖ ${pkg}: ${error}`);
            hasValidationError = true;
            continue;
        }

        let realSrcDir;
        try {
            realSrcDir = fs.realpathSync(candidateSrcDir);
        } catch (err) {
            const error = `Falha ao resolver caminho real de ${candidateSrcDir}: ${err.message}`;
            results.push({ pkg, error });
            logger(`✖ ${pkg}: ${error}`);
            hasValidationError = true;
            continue;
        }

        const libPkgPath = path.join(realSrcDir, 'package.json');
        if (!fs.existsSync(libPkgPath)) {
            const error = `Manifesto package.json ausente na biblioteca: ${libPkgPath}`;
            results.push({ pkg, error });
            logger(`✖ ${pkg}: ${error}`);
            hasValidationError = true;
            continue;
        }

        let libPkg;
        try {
            libPkg = JSON.parse(fs.readFileSync(libPkgPath, 'utf8'));
        } catch (err) {
            const error = `Falha ao interpretar ${libPkgPath}: ${err.message}`;
            results.push({ pkg, error });
            logger(`✖ ${pkg}: ${error}`);
            hasValidationError = true;
            continue;
        }

        if (libPkg.name !== pkg) {
            const error = `Nome do pacote incompatível em ${libPkgPath}. Esperado: "${pkg}", obtido: "${libPkg.name}".`;
            results.push({ pkg, error });
            logger(`✖ ${pkg}: ${error}`);
            hasValidationError = true;
            continue;
        }

        if (!libPkg.version || typeof libPkg.version !== 'string') {
            const error = `Versão ausente ou inválida no package.json de ${pkg}.`;
            results.push({ pkg, error });
            logger(`✖ ${pkg}: ${error}`);
            hasValidationError = true;
            continue;
        }

        // Validação dos artefatos apontados em exports / main / module / types
        const declaredFiles = new Set();
        if (libPkg.main) collectExportPaths(libPkg.main, declaredFiles);
        if (libPkg.module) collectExportPaths(libPkg.module, declaredFiles);
        if (libPkg.types) collectExportPaths(libPkg.types, declaredFiles);
        if (libPkg.exports) collectExportPaths(libPkg.exports, declaredFiles);

        let missingArtifact = null;
        for (const relPath of declaredFiles) {
            if (typeof relPath !== 'string') continue;
            // Ignora padrões curinga como "./dist/themes/*"
            if (relPath.includes('*')) {
                const baseDir = path.dirname(relPath);
                const absBaseDir = path.resolve(realSrcDir, baseDir);
                if (!fs.existsSync(absBaseDir)) {
                    missingArtifact = `Diretório curinga não encontrado: ${relPath} (${absBaseDir})`;
                    break;
                }
                continue;
            }

            const absFilePath = path.resolve(realSrcDir, relPath);
            if (!fs.existsSync(absFilePath)) {
                missingArtifact = `Artefato declarado não existe: ${relPath} (${absFilePath})`;
                break;
            }

            const stat = fs.statSync(absFilePath);
            if (!stat.isFile() || stat.size === 0) {
                missingArtifact = `Artefato não é arquivo válido ou está vazio: ${relPath}`;
                break;
            }
        }

        if (missingArtifact) {
            results.push({ pkg, error: missingArtifact });
            logger(`✖ ${pkg}: ${missingArtifact}`);
            hasValidationError = true;
            continue;
        }

        // Validação de peerDependencies obrigatórias
        const peerDeps = libPkg.peerDependencies ?? {};
        const peerMeta = libPkg.peerDependenciesMeta ?? {};
        let missingPeer = null;

        for (const peerName of Object.keys(peerDeps)) {
            if (peerMeta[peerName]?.optional === true) {
                continue;
            }
            if (peerName in maxLocalLibs) {
                continue;
            }
            const peerPkgJson = path.join(workspaceRoot, 'node_modules', peerName, 'package.json');
            if (!fs.existsSync(peerPkgJson)) {
                missingPeer = `Peer dependency obrigatória "${peerName}" não encontrada em node_modules da aplicação.`;
                break;
            }
        }

        if (missingPeer) {
            results.push({ pkg, error: missingPeer });
            logger(`✖ ${pkg}: ${missingPeer}`);
            hasValidationError = true;
            continue;
        }

        // Validação de destino em node_modules/<pkg>
        const destPath = path.join(workspaceRoot, 'node_modules', pkg);
        const destStat = fs.lstatSync(destPath, { throwIfNoEntry: false });

        if (destStat) {
            if (!destStat.isSymbolicLink()) {
                const error = `Destino já existe como diretório/arquivo real; remova manualmente ou execute npm ci (${destPath})`;
                results.push({ pkg, error });
                logger(`✖ ${pkg}: ${error}`);
                hasValidationError = true;
                continue;
            }

            if (checkOnly) {
                let currentLinkTarget;
                try {
                    currentLinkTarget = fs.readlinkSync(destPath);
                } catch (err) {
                    const error = `Falha ao ler symlink existente: ${err.message}`;
                    results.push({ pkg, error });
                    logger(`✖ ${pkg}: ${error}`);
                    hasValidationError = true;
                    continue;
                }

                const resolvedCurrent = path.isAbsolute(currentLinkTarget)
                    ? currentLinkTarget
                    : path.resolve(path.dirname(destPath), currentLinkTarget);

                if (resolvedCurrent !== realSrcDir) {
                    const error = `Symlink aponta para alvo incorreto: ${resolvedCurrent} (esperado: ${realSrcDir})`;
                    results.push({ pkg, error });
                    logger(`✖ ${pkg}: ${error}`);
                    hasValidationError = true;
                    continue;
                }
            }
        } else if (checkOnly) {
            const error = `Symlink não encontrado para ${pkg} em ${destPath}`;
            results.push({ pkg, error });
            logger(`✖ ${pkg}: ${error}`);
            hasValidationError = true;
            continue;
        }

        plannedActions.push({
            pkg,
            folder,
            realSrcDir,
            destPath,
            destStat
        });
    }

    if (hasValidationError) {
        return { ok: false, results };
    }

    if (checkOnly) {
        for (const action of plannedActions) {
            logger(`✔ ${action.pkg} → ${action.realSrcDir} (validado com sucesso)`);
            results.push({ pkg: action.pkg, target: action.realSrcDir });
        }
        return { ok: true, results };
    }

    // 4. Fase de aplicação atômica e idempotente
    const maxvueDir = path.join(workspaceRoot, 'node_modules', '@maxvue');
    fs.mkdirSync(maxvueDir, { recursive: true });

    for (const action of plannedActions) {
        const { pkg, folder, realSrcDir, destPath, destStat } = action;

        if (destStat && destStat.isSymbolicLink()) {
            const currentTarget = fs.readlinkSync(destPath);
            const resolvedCurrent = path.isAbsolute(currentTarget)
                ? currentTarget
                : path.resolve(path.dirname(destPath), currentTarget);

            if (resolvedCurrent === realSrcDir) {
                logger(`✔ ${pkg} → ${realSrcDir} (preservado/idempotente)`);
                results.push({ pkg, target: realSrcDir });
                continue;
            }

            // Criação atômica via symlink temporário e renameSync
            const tmpName = path.join(maxvueDir, `.tmp-${folder}-${process.pid}-${Date.now()}`);
            fs.symlinkSync(realSrcDir, tmpName, 'dir');
            fs.renameSync(tmpName, destPath);
            logger(`✔ ${pkg} → ${realSrcDir} (atualizado atomicamente)`);
            results.push({ pkg, target: realSrcDir });
        } else {
            // Criação direta de symlink absoluto
            fs.symlinkSync(realSrcDir, destPath, 'dir');
            logger(`✔ ${pkg} → ${realSrcDir}`);
            results.push({ pkg, target: realSrcDir });
        }
    }

    return { ok: true, results };
}

// Execução CLI direta
if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
    const isCheck = process.argv.includes('--check');
    const result = syncLocalLibs({ checkOnly: isCheck });
    if (!result.ok) {
        process.exit(1);
    }
}
