import { defineStore } from 'pinia';
import { computed, ref } from 'vue';
import { logger } from '@/app/logger';
import { getAppInfo } from '@/ipc/app';
import { isMtIpcError, MtIpcError } from '@/ipc/core';
import type { AppInfo } from '@/types/bindings';
import { IPC_TRANSPORT_ERROR, IPC_TRANSPORT_MESSAGE } from '@/types/errors';

export type AppBootStatus = 'idle' | 'loading' | 'ready' | 'error';

export interface BootstrapStep {
    name: string;
    run: () => Promise<void>;
}

export const useAppStore = defineStore('app', () => {
    const info = ref<AppInfo | null>(null);
    const status = ref<AppBootStatus>('idle');
    const error = ref<MtIpcError | null>(null);

    const steps: BootstrapStep[] = [];
    let stepsFrozen = false;
    let inFlightPromise: Promise<void> | null = null;

    const isReady = computed<boolean>(() => status.value === 'ready');
    const versionLabel = computed<string>(() => (info.value ? `v${info.value.version}` : ''));
    const isDebug = computed<boolean>(() => info.value?.buildProfile === 'debug');

    function registerBootstrapStep(step: BootstrapStep): void {
        if (stepsFrozen) {
            throw new Error(
                `Não é permitido registrar o passo de bootstrap '${step.name}' após o início do bootstrap.`
            );
        }
        const trimmedName = step.name.trim();
        if (trimmedName.length === 0) {
            throw new Error('O nome do passo de bootstrap não pode ser vazio.');
        }
        if (steps.some((existing) => existing.name === trimmedName)) {
            throw new Error(`Passo de bootstrap duplicado: '${trimmedName}'.`);
        }
        steps.push({
            name: trimmedName,
            run: step.run
        });
    }

    async function bootstrap(): Promise<void> {
        if (status.value === 'ready') {
            return;
        }
        if (inFlightPromise !== null) {
            return inFlightPromise;
        }

        stepsFrozen = true;
        status.value = 'loading';
        error.value = null;

        inFlightPromise = (async () => {
            try {
                const fetchedInfo = await getAppInfo();
                info.value = fetchedInfo;

                for (const step of steps) {
                    await step.run();
                }

                status.value = 'ready';
            } catch (err: unknown) {
                const normalized = isMtIpcError(err)
                    ? err
                    : new MtIpcError(
                        {
                            code: IPC_TRANSPORT_ERROR,
                            message: IPC_TRANSPORT_MESSAGE
                        },
                        'bootstrap'
                    );
                error.value = normalized;
                status.value = 'error';
                logger.report('bootstrap_failed');
            } finally {
                inFlightPromise = null;
            }
        })();

        return inFlightPromise;
    }

    async function retry(): Promise<void> {
        if (inFlightPromise !== null) {
            return inFlightPromise;
        }
        error.value = null;
        if (status.value === 'ready') {
            status.value = 'idle';
        }
        return bootstrap();
    }

    return {
        info,
        status,
        error,
        isReady,
        versionLabel,
        isDebug,
        registerBootstrapStep,
        bootstrap,
        retry
    };
});
