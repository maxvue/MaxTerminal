import { clearMocks } from '@tauri-apps/api/mocks';
import { afterEach } from 'vitest';

afterEach(() => {
    clearMocks();
});
