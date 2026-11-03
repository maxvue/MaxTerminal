import { describe, expect, it } from 'vitest';
import { parseLinkCandidates } from '@/composables/editorLinks/parseLinkCandidates';

describe('parseLinkCandidates (Fase 09)', () => {
    it('extrai caminhos no formato Rust / Cargo / TypeScript / ESLint (arquivo:linha:coluna)', () => {
        const line = '  --> src-tauri/crates/mt-os/src/lib.rs:42:10';
        const candidates = parseLinkCandidates(line);
        expect(candidates).toHaveLength(1);
        expect(candidates[0]).toMatchObject({
            raw: 'src-tauri/crates/mt-os/src/lib.rs:42:10',
            path: 'src-tauri/crates/mt-os/src/lib.rs',
            line: 42,
            column: 10
        });
    });

    it('extrai tracebacks de Python (File "...", line N)', () => {
        const line = '  File "/home/user/app/main.py", line 87, in <module>';
        const candidates = parseLinkCandidates(line);
        expect(candidates).toHaveLength(1);
        expect(candidates[0]).toMatchObject({
            path: '/home/user/app/main.py',
            line: 87
        });
    });

    it('extrai caminhos PHP / Laravel / Vitest com linha entre parênteses', () => {
        const line = '#0 /var/www/app/Models/User.php(42): Illuminate\\Foundation->run()';
        const candidates = parseLinkCandidates(line);
        expect(candidates).toHaveLength(1);
        expect(candidates[0]).toMatchObject({
            raw: '/var/www/app/Models/User.php(42)',
            path: '/var/www/app/Models/User.php',
            line: 42
        });
    });

    it('normaliza prefixos a/ e b/ de git diff', () => {
        const line = 'diff --git a/src/stores/window.ts b/src/stores/window.ts';
        const candidates = parseLinkCandidates(line);
        expect(candidates).toHaveLength(2);
        expect(candidates[0]?.path).toBe('src/stores/window.ts');
        expect(candidates[1]?.path).toBe('src/stores/window.ts');
    });

    it('extrai caminhos simples com extensão ou nomes de arquivo conhecidos', () => {
        const line = 'Editando ./vite.config.ts e Cargo.toml no projeto';
        const candidates = parseLinkCandidates(line);
        expect(candidates.map((c) => c.path)).toEqual(['./vite.config.ts', 'Cargo.toml']);
    });

    it('ignora URLs HTTP/HTTPS, IPs com porta, portas isoladas, hashes hexadecimais e semver', () => {
        const line =
            'Servidor em http://127.0.0.1:8000 (127.0.0.1:8000 :5177) commit deadbeef12345678 v1.2.3';
        const candidates = parseLinkCandidates(line);
        expect(candidates).toEqual([]);
    });
});
