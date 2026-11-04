import path from 'node:path';
import { fileURLToPath } from 'node:url';
import vue from '@vitejs/plugin-vue';
import { defineConfig } from 'vitest/config';

const workspaceRoot = fileURLToPath(new URL('.', import.meta.url));

export default defineConfig({
    cacheDir: path.resolve(workspaceRoot, '.tmp/vite'),
    plugins: [vue()],
    resolve: {
        preserveSymlinks: false,
        alias: {
            '@': path.resolve(workspaceRoot, 'src')
        },
        dedupe: [
            'vue',
            'pinia',
            'vue-router',
            'axios',
            '@maxvue/max-use',
            '@maxvue/max-pinia',
            '@maxvue/max-components-ui'
        ]
    },
    test: {
        globals: true,
        environment: 'happy-dom',
        setupFiles: ['./tests/setup.ts'],
        server: {
            deps: {
                inline: [/^@maxvue\//]
            }
        },
        include: ['tests/**/*.spec.ts', 'src/**/__tests__/**/*.spec.ts'],
        coverage: {
            provider: 'v8',
            reporter: ['text', 'json', 'html'],
            include: [
                'src/ipc/**/*.ts',
                'src/stores/**/*.ts',
                'src/app/logger.ts',
                'src/composables/**/*.ts',
                'src/components/shell/DbRecoveryPanel.vue',
                'src/terminal/**/*.ts',
                'src/components/terminal/*.vue'
            ],
            exclude: [
                'src/types/bindings.ts',
                'src/**/*.d.ts',
                'src/**/index.ts',
                'src/**/__tests__/**'
            ],
            thresholds: {
                lines: 80
            }
        }
    }
});
