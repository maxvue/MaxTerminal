import path from 'node:path';

/**
 * Configuração WebdriverIO / Browser Runner para a suíte E2E de UI Frontend (Camada A — CA-27).
 * Roda em navegador headless sobre o servidor Vite com transporte IPC mockado.
 */
export const config = {
    runner: 'local',
    specs: [path.resolve(process.cwd(), 'e2e/frontend/specs/**/*.e2e.ts')],
    maxInstances: 1,
    capabilities: [
        {
            browserName: 'chrome',
            'goog:chromeOptions': {
                args: ['--headless=new', '--disable-gpu', '--no-sandbox', '--window-size=1440,900']
            }
        }
    ],
    logLevel: 'warn',
    baseUrl: process.env.VITE_E2E_BASE_URL ?? 'http://127.0.0.1:1420',
    waitforTimeout: 10_000,
    connectionRetryTimeout: 30_000,
    connectionRetryCount: 2,
    framework: 'mocha',
    reporters: ['spec'],
    mochaOpts: {
        ui: 'bdd',
        timeout: 30_000
    }
};
