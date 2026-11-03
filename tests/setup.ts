import { clearMocks } from '@tauri-apps/api/mocks';
import { config } from '@vue/test-utils';
import { afterEach } from 'vitest';

config.global.stubs = {
    ...config.global.stubs,
    teleport: true
};

config.global.directives = {
    ...config.global.directives,
    tooltip: () => {}
};

afterEach(() => {
    clearMocks();
});
