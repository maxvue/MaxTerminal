/** Candidato de link de arquivo extraído de uma linha do terminal. */
export interface ParsedLink {
    /** Trecho bruto correspondente na linha (incluindo `:linha:coluna` ou `(linha)`). */
    raw: string;
    /** Caminho extraído (normalizado sem prefixo `a/` ou `b/` de git diff). */
    path: string;
    /** Linha opcional (`>= 1`). */
    line?: number | undefined;
    /** Coluna opcional (`>= 1`). */
    column?: number | undefined;
    /** Coluna inicial 1-based inclusiva na linha do terminal. */
    startCol: number;
    /** Coluna final 1-based inclusiva na linha do terminal. */
    endCol: number;
}

const URL_SCHEME_RE = /^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//;
const SPECIAL_URI_RE = /^(?:mailto|tel|javascript|data|vbscript|vscode|maxterminal|about|blob):/i;
const IPV4_OR_HOST_PORT_RE = /^(?:\d{1,3}(?:\.\d{1,3}){3}|localhost)(?::\d+)?$/i;
const PURE_HEX_OR_SEMVER_RE = /^(?:[0-9a-f]{7,64}|v?\d+\.\d+(?:\.\d+)*(?:-[0-9A-Za-z.-]+)?)$/i;
const TIME_LIKE_RE = /^\d{1,2}:\d{2}(?::\d{2})?$/;

const KNOWN_EXACT_FILENAMES = new Set([
    'Cargo.toml',
    'Cargo.lock',
    'package.json',
    'package-lock.json',
    'pnpm-lock.yaml',
    'yarn.lock',
    'tsconfig.json',
    'vite.config.ts',
    'vite.config.js',
    'vitest.config.ts',
    'uno.config.ts',
    'eslint.config.js',
    'Makefile',
    'Dockerfile',
    'AGENTS.md',
    'CLAUDE.md',
    'README.md',
    '.env',
    '.env.example',
    '.gitignore'
]);

const KNOWN_EXTENSIONS = new Set([
    'rs',
    'ts',
    'tsx',
    'js',
    'jsx',
    'mjs',
    'cjs',
    'vue',
    'py',
    'php',
    'rb',
    'go',
    'java',
    'kt',
    'c',
    'cc',
    'cpp',
    'h',
    'hpp',
    'cs',
    'swift',
    'scala',
    'lua',
    'sh',
    'bash',
    'zsh',
    'fish',
    'sql',
    'json',
    'jsonc',
    'yaml',
    'yml',
    'toml',
    'ini',
    'conf',
    'cfg',
    'xml',
    'html',
    'htm',
    'css',
    'scss',
    'sass',
    'less',
    'md',
    'mdx',
    'txt',
    'log',
    'lock',
    'env',
    ' blade.php'
]);

function hasLikelyFileShape(path: string): boolean {
    if (!path || path.length > 512) {
        return false;
    }
    if (URL_SCHEME_RE.test(path) || SPECIAL_URI_RE.test(path)) {
        return false;
    }
    if (IPV4_OR_HOST_PORT_RE.test(path) || PURE_HEX_OR_SEMVER_RE.test(path) || TIME_LIKE_RE.test(path)) {
        return false;
    }
    if (path.startsWith(':')) {
        return false;
    }

    const base = path.split('/').pop() ?? path;
    if (!base || base === '.' || base === '..') {
        return false;
    }
    if (KNOWN_EXACT_FILENAMES.has(base)) {
        return true;
    }

    const hasSlash = path.includes('/');
    const dotIdx = base.lastIndexOf('.');
    if (dotIdx > 0 && dotIdx < base.length - 1) {
        const ext = base.slice(dotIdx + 1).toLowerCase();
        if (KNOWN_EXTENSIONS.has(ext)) {
            return true;
        }
        if (hasSlash && /^[a-z0-9]{1,10}$/i.test(ext)) {
            return true;
        }
    }

    // Caminhos explícitos absolutos, home ou relativos com múltiplos segmentos (ex.: /etc/hosts)
    if ((path.startsWith('/') || path.startsWith('~/') || path.startsWith('./') || path.startsWith('../')) && hasSlash) {
        return base.length >= 2 && !base.endsWith(':');
    }

    return false;
}

function parsePositiveInt(raw: string | undefined): number | undefined {
    if (!raw) {
        return undefined;
    }
    const num = Number.parseInt(raw, 10);
    if (!Number.isSafeInteger(num) || num < 1 || num > 10_000_000) {
        return undefined;
    }
    return num;
}

function normalizeGitDiffPrefix(path: string): string {
    if ((path.startsWith('a/') || path.startsWith('b/')) && path.length > 2) {
        return path.slice(2);
    }
    return path;
}

function overlaps(a: { startCol: number; endCol: number }, b: { startCol: number; endCol: number }): boolean {
    return a.startCol <= b.endCol && b.startCol <= a.endCol;
}

/**
 * Extrai candidatos de caminhos de arquivo (com linha e coluna opcionais) de uma linha de texto do terminal.
 * Suporta:
 * 1. `path/to/file.ext:42:10` e `--> src/lib.rs:42:10`
 * 2. Python `File "/path/mod.py", line 87`
 * 3. PHP/Laravel/Vitest `/path/User.php(42)` ou `/path/User.php(42,10)`
 * 4. Headers de `git diff` (`a/src/foo.ts`, `b/src/foo.ts`)
 * 5. Caminhos simples (`./vite.config.ts`, `~/proj/README.md`, `src/main.ts`)
 */
export function parseLinkCandidates(lineText: string): ParsedLink[] {
    if (!lineText || lineText.length === 0) {
        return [];
    }
    // Limita varredura em linhas gigantescas para proteger a UI thread
    const text = lineText.length > 4096 ? lineText.slice(0, 4096) : lineText;
    const results: ParsedLink[] = [];

    // 1. Python traceback: File "/path/to/file.py", line 87
    const pythonRe = /File\s+"([^"\s]+)"\s*,\s*line\s+(\d+)/g;
    for (const match of text.matchAll(pythonRe)) {
        const fullMatch = match[0];
        const rawPath = match[1];
        const rawLine = match[2];
        const matchIndex = match.index ?? 0;
        if (!rawPath) {
            continue;
        }
        const cleanPath = normalizeGitDiffPrefix(rawPath);
        if (!hasLikelyFileShape(cleanPath)) {
            continue;
        }
        const startCol = matchIndex + 1;
        const endCol = matchIndex + fullMatch.length;
        results.push({
            raw: fullMatch,
            path: cleanPath,
            line: parsePositiveInt(rawLine),
            startCol,
            endCol
        });
    }

    // 2. Token scanner para caminhos `path:line:col`, `path(line)` e caminhos simples
    const tokenRe = /[^\s"'<>`]+/g;
    for (const match of text.matchAll(tokenRe)) {
        let token = match[0];
        let offset = match.index ?? 0;

        // Ignora se for uma URL completa com esquema
        if (URL_SCHEME_RE.test(token) || SPECIAL_URI_RE.test(token)) {
            continue;
        }

        // Remove delimitadores iniciais comuns: `(`, `[`, `{`
        while (token.length > 0 && /^[([{]/.test(token)) {
            token = token.slice(1);
            offset += 1;
        }

        // Remove pontuação final comum: `,`, `;`, `.`, `]`, `}` (preservando `)` apenas se for `(line)`)
        while (token.length > 0 && /[,;.\]}]$/.test(token)) {
            token = token.slice(0, -1);
        }

        // Se termina com `)` mas não tem `(\d+)` no final, remove `)` final
        while (token.endsWith(')') && !/\(\d+(?:,\d+)?\)$/.test(token)) {
            token = token.slice(0, -1);
        }

        // Remove `:` final solto (ex.: `src/lib.rs:`)
        while (token.endsWith(':')) {
            token = token.slice(0, -1);
        }

        if (token.length < 2) {
            continue;
        }

        let pathPart = token;
        let lineNum: number | undefined;
        let colNum: number | undefined;

        // Caso A: `caminho(42)` ou `caminho(42,10)`
        const parenMatch = /^(.+?)\((\d+)(?:,(\d+))?\)$/.exec(token);
        if (parenMatch && parenMatch[1]) {
            pathPart = parenMatch[1];
            lineNum = parsePositiveInt(parenMatch[2]);
            colNum = parsePositiveInt(parenMatch[3]);
        } else {
            // Caso B: `caminho:42:10` ou `caminho:42`
            const colonMatch = /^(.+?):(\d+)(?::(\d+))?$/.exec(token);
            if (colonMatch && colonMatch[1]) {
                pathPart = colonMatch[1];
                lineNum = parsePositiveInt(colonMatch[2]);
                colNum = parsePositiveInt(colonMatch[3]);
            }
        }

        if (IPV4_OR_HOST_PORT_RE.test(pathPart) || URL_SCHEME_RE.test(pathPart)) {
            continue;
        }

        const normalizedPath = normalizeGitDiffPrefix(pathPart);
        if (!hasLikelyFileShape(normalizedPath)) {
            continue;
        }

        const startCol = offset + 1;
        const endCol = offset + token.length;
        const candidate: ParsedLink = {
            raw: token,
            path: normalizedPath,
            ...(lineNum !== undefined ? { line: lineNum } : {}),
            ...(colNum !== undefined ? { column: colNum } : {}),
            startCol,
            endCol
        };

        if (!results.some((existing) => overlaps(existing, candidate))) {
            results.push(candidate);
        }
    }

    results.sort((a, b) => a.startCol - b.startCol);
    return results.slice(0, 64);
}
