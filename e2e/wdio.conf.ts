import path from 'node:path';

/**
 * Configuração WebdriverIO + tauri-driver (WebKitWebDriver + Xvfb) para a
 * suíte E2E Desktop Tauri real (Camada B — CA-28).
 */
export const config = {
    runner: 'local',
    host: '127.0.0.1',
    port: 4444,
    specs: [path.resolve(process.cwd(), 'e2e/specs/**/*.e2e.ts')],
    maxInstances: 1,
    capabilities: [
        {
            maxInstances: 1,
            'tauri:options': {
                application: path.resolve(
                    process.cwd(),
                    'src-tauri/target/release/maxterminal'
                )
            }
        }
    ],
    logLevel: 'warn',
    waitforTimeout: 15_000,
    connectionRetryTimeout: 60_000,
    connectionRetryCount: 2,
    framework: 'mocha',
    reporters: ['spec'],
    mochaOpts: {
        ui: 'bdd',
        timeout: 60_000
    }
};
