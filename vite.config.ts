import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import vue from '@vitejs/plugin-vue';
import UnoCSS from 'unocss/vite';
import { defineConfig } from 'vite';

const workspaceRoot = fileURLToPath(new URL('.', import.meta.url));
const srcDir = path.resolve(workspaceRoot, 'src');

function resolveAllowedFsRoots(): string[] {
    const defaultFallback = fs.existsSync('/home/johnattas/GitHub')
        ? '/home/johnattas/GitHub'
        : path.join(os.homedir(), 'GitHub');
    const rawLibsRoot = process.env.MAX_LIBS_ROOT ?? defaultFallback;
    const libsRoot = rawLibsRoot.startsWith('~/')
        ? path.join(os.homedir(), rawLibsRoot.slice(2))
        : rawLibsRoot;

    const folders = ['MaxComponentsUi', 'MaxUse', 'MaxPinia'];
    const roots = [workspaceRoot];

    for (const folder of folders) {
        const candidate = path.resolve(libsRoot, folder);
        try {
            roots.push(fs.realpathSync(candidate));
        } catch {
            roots.push(candidate);
        }
    }

    return roots;
}

export default defineConfig({
    clearScreen: false,
    cacheDir: path.resolve(workspaceRoot, '.tmp/vite'),
    envPrefix: ['VITE_'],
    plugins: [
        vue(),
        UnoCSS()
    ],
    resolve: {
        preserveSymlinks: false,
        alias: {
            '@': srcDir
        },
        dedupe: ['vue', 'pinia']
    },
    optimizeDeps: {
        exclude: [
            '@maxvue/max-components-ui',
            '@maxvue/max-use',
            '@maxvue/max-pinia'
        ]
    },
    css: {
        preprocessorOptions: {
            scss: {
                additionalData: "@use '@/styles/params' as *;\n"
            }
        }
    },
    server: {
        host: '127.0.0.1',
        port: 5177,
        strictPort: true,
        fs: {
            strict: true,
            allow: resolveAllowedFsRoots()
        }
    },
    build: {
        target: 'es2022',
        sourcemap: true
    }
});
