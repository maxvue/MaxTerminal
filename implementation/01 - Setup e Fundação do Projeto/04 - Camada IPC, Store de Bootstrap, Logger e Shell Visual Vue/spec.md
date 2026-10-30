# Implementação 04 — Camada IPC, Store de Bootstrap, Logger e Shell Visual Vue (`spec.md`)

> Fase: [`../instructions.md`](../instructions.md) | Especificação da Fase: [`../spec.md`](../spec.md)
> Depende de: [`../02 - Runtime Tauri, Logging, ACL e Bindings Specta/spec.md`](../02%20-%20Runtime%20Tauri,%20Logging,%20ACL%20e%20Bindings%20Specta/spec.md) e [`../03 - Tooling Frontend, Sync de Bibliotecas Locais e Qualidade/spec.md`](../03%20-%20Tooling%20Frontend,%20Sync%20de%20Bibliotecas%20Locais%20e%20Qualidade/spec.md)
> Escopo desta sub-implementação: `index.html`, `src/types/{bindings.ts,errors.ts,index.ts}`, `src/ipc/{core.ts,app.ts,index.ts}`, `src/composables/useIpcError.ts`, `src/app/{logger.ts,plugins.ts,main.ts,App.vue}`, `src/stores/app.ts`, `src/styles/{params.scss,theme.scss,main.scss}`, `src/components/shell/{AppShell.vue,AppTitleBar.vue,AppEmptyState.vue}` e testes unitários Vitest (`tests/ipc/core.spec.ts`, `tests/stores/app.spec.ts`, `tests/app/logger.spec.ts`, `tests/components/shell/AppShell.spec.ts`).

---

## 1. Objetivo e Fronteira Arquitetural

Implementar a camada de aplicação Vue 3.6 + Pinia + UnoCSS + SCSS sobre os bindings TypeScript gerados pelo `tauri-specta`:

1. **Fonte Única de Tipos (`src/types/`):**
   - `src/types/bindings.ts` é exclusivamente o arquivo gerado por `npm run bindings` (nenhum DTO Rust é redeclarado à mão).
   - `src/types/errors.ts` define `IPC_TRANSPORT_ERROR = 'ipc.transport'` e o dicionário `errorMessages` em `pt-BR`.
2. **Camada IPC Tipada (`src/ipc/core.ts`, `src/ipc/app.ts`, `src/ipc/index.ts`):**
   - `call(command, request)` e `unwrap(request, command)` desembrulham a união discriminada `Result<T, MtError>` (`{ status: 'ok', data: T } | { status: 'error', error: MtError }`), lançando `MtIpcError` em falhas de domínio e normalizando falhas de transporte/exceções síncronas/payloads malformados como `ipc.transport` com mensagem estática em `pt-BR`.
3. **Logger do Frontend (`src/app/logger.ts`):**
   - Envia apenas variantes de `FrontendLogEvent` (`'vue_error' | 'window_error' | 'unhandled_rejection' | 'bootstrap_failed'`), com fila limitada a 64 itens, deduplicação na mesma microtask, máximo de 1 requisição IPC em voo e desmontagem idempotente dos listeners globais.
4. **Store de Bootstrap (`src/stores/app.ts`):**
   - Gerencia `info`, `status` (`'idle' | 'loading' | 'ready' | 'error'`), `error`, `isReady`, `versionLabel`, `isDebug`, `bootstrap()`, `retry()` e `registerBootstrapStep(step)` isolados por instância Pinia.
5. **Shell Visual no Tema Max Navy/Teal (`src/styles/`, `src/app/App.vue`, `src/components/shell/`):**
   - `AppShell.vue`, `AppTitleBar.vue` e `AppEmptyState.vue` seguindo estritamente as regras de SFC Vue (`<template>`, `<script setup lang="ts">`, `<style lang="scss" scoped>`, tags de abertura em linha única, 4 espaços e tokens `--max-*`, `--background-*`, `--mt-*`).

---

## 2. Entrada HTML e Tipos (`index.html`, `src/types/`)

### 2.1. `index.html`

```html
<!doctype html>
<html lang="pt-BR" class="dark">
    <head>
        <meta charset="UTF-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1.0" />
        <title>MaxTerminal</title>
    </head>
    <body>
        <div id="app"></div>
        <script type="module" src="/src/app/main.ts"></script>
    </body>
</html>
```

### 2.2. `src/types/errors.ts` e `src/types/index.ts`

`src/types/errors.ts`:
```ts
/** Código de erro sintético emitido quando a ponte IPC Tauri falha fora do domínio Rust. */
export const IPC_TRANSPORT_ERROR = 'ipc.transport' as const;

/** Mensagem padrão em pt-BR quando ocorre erro de transporte IPC. */
export const IPC_TRANSPORT_MESSAGE = 'Não foi possível comunicar com o processo nativo do MaxTerminal.';

/** Mapa de mensagens amigáveis em pt-BR indexadas por `ErrorCode`. */
export const errorMessages: Partial<Record<string, string>> = {
    'core.internal': 'Ocorreu um erro interno inesperado no MaxTerminal.',
    'core.io': 'Falha de leitura ou gravação no sistema de arquivos.',
    'core.not_found': 'O recurso solicitado não foi encontrado.',
    'core.invalid_argument': 'Os parâmetros informados são inválidos.',
    'core.conflict': 'Operação em conflito com o estado atual.',
    'core.permission_denied': 'Permissão negada pelo sistema operacional.',
    'core.unsupported': 'Esta operação não é suportada neste ambiente.',
    'core.serialization': 'Falha ao processar os dados da operação.',
    'core.paths_unresolved': 'Não foi possível localizar os diretórios de configuração do usuário.',
    [IPC_TRANSPORT_ERROR]: IPC_TRANSPORT_MESSAGE
};
```

`src/types/index.ts`:
```ts
export * from './bindings';
export * from './errors';
```

---

## 3. Camada IPC e Tratamento de Erros (`src/ipc/`, `src/composables/useIpcError.ts`)

### 3.1. `src/ipc/core.ts`

```ts
import type { MtError, Result } from '@/types/bindings';
import { IPC_TRANSPORT_ERROR, IPC_TRANSPORT_MESSAGE } from '@/types/errors';

/** Alias para o envelope Result gerado pelo tauri-specta com ErrorHandlingMode::Result. */
export type IpcResult<T> = Result<T, MtError>;

/** Exceção tipada lançada na camada de fronteira IPC do frontend. */
export class MtIpcError extends Error {
    public readonly code: string;
    public readonly details: unknown;
    public readonly command: string;

    public constructor(payload: MtError, command: string) {
        super(payload.message);
        this.name = 'MtIpcError';
        this.code = payload.code;
        this.details = payload.details;
        this.command = command;
    }
}

/** Verifica se um valor desconhecido é uma instância de `MtIpcError`. */
export function isMtIpcError(value: unknown): value is MtIpcError {
    return value instanceof MtIpcError;
}

function isMtErrorPayload(value: unknown): value is MtError {
    if (typeof value !== 'object' || value === null) {
        return false;
    }
    const candidate = value as Record<string, unknown>;
    return (
        typeof candidate.code === 'string' &&
        candidate.code.length > 0 &&
        typeof candidate.message === 'string' &&
        candidate.message.length > 0
    );
}

function createTransportError(command: string): MtIpcError {
    return new MtIpcError(
        {
            code: IPC_TRANSPORT_ERROR,
            message: IPC_TRANSPORT_MESSAGE
        },
        command
    );
}

function decodeResult<T>(raw: unknown, command: string): T {
    if (typeof raw !== 'object' || raw === null) {
        throw createTransportError(command);
    }

    const envelope = raw as Record<string, unknown>;
    if (envelope.status === 'ok' && 'data' in envelope) {
        return envelope.data as T;
    }

    if (envelope.status === 'error' && isMtErrorPayload(envelope.error)) {
        throw new MtIpcError(envelope.error, command);
    }

    throw createTransportError(command);
}

/**
 * Executa uma chamada IPC gerada pelo tauri-specta capturando tanto rejeições assíncronas
 * quanto exceções síncronas e validando o envelope `IpcResult<T>`.
 */
export async function call<T>(command: string, request: () => Promise<IpcResult<T>>): Promise<T> {
    let raw: unknown;
    try {
        raw = await request();
    } catch (err: unknown) {
        if (isMtIpcError(err)) {
            throw err;
        }
        if (isMtErrorPayload(err)) {
            throw new MtIpcError(err, command);
        }
        throw createTransportError(command);
    }

    return decodeResult<T>(raw, command);
}

/**
 * Desembrulha uma Promise `IpcResult<T>` já iniciada usando a mesma validação de `call()`.
 */
export async function unwrap<T>(request: Promise<IpcResult<T>>, command = 'ipc'): Promise<T> {
    return call(command, () => request);
}
```

### 3.2. `src/ipc/app.ts` e `src/ipc/index.ts`

`src/ipc/app.ts`:
```ts
import { commands, type AppInfo, type FrontendLogEvent } from '@/types/bindings';
import { call } from './core';

/** Obtém os metadados estáticos e diretórios XDG do MaxTerminal. */
export async function getAppInfo(): Promise<AppInfo> {
    return call('app_info', () => commands.appInfo());
}

/** Envia um evento estático enumerado de erro do frontend para o log estruturado nativo. */
export async function logFrontend(event: FrontendLogEvent): Promise<void> {
    await call('app_log_frontend', () => commands.appLogFrontend(event));
}
```

`src/ipc/index.ts`:
```ts
export * as appIpc from './app';
export { call, isMtIpcError, MtIpcError, unwrap, type IpcResult } from './core';
```

### 3.3. `src/composables/useIpcError.ts`

```ts
import { isMtIpcError } from '@/ipc/core';
import { errorMessages } from '@/types/errors';

export interface UseIpcErrorReturn {
    message: (err: unknown) => string;
    code: (err: unknown) => string | null;
}

/** Traduz erros IPC (`MtIpcError`) em mensagens seguras em pt-BR para exibição na interface. */
export function useIpcError(): UseIpcErrorReturn {
    function code(err: unknown): string | null {
        if (isMtIpcError(err)) {
            return err.code;
        }
        return null;
    }

    function message(err: unknown): string {
        if (isMtIpcError(err)) {
            const mapped = errorMessages[err.code];
            if (mapped) {
                return mapped;
            }
            if (err.message.trim().length > 0) {
                return err.message;
            }
        }
        return 'Erro inesperado ao processar a operação.';
    }

    return {
        message,
        code
    };
}
```

---

## 4. Logger do Frontend e Store de Bootstrap (`src/app/logger.ts`, `src/stores/app.ts`)

### 4.1. `src/app/logger.ts`

```ts
import type { App } from 'vue';
import { logFrontend } from '@/ipc/app';
import type { FrontendLogEvent } from '@/types/bindings';

/** Limite máximo de eventos pendentes na fila em memória do logger do frontend. */
export const MAX_FRONTEND_LOG_QUEUE = 64;

const VALID_EVENTS = new Set<FrontendLogEvent>([
    'vue_error',
    'window_error',
    'unhandled_rejection',
    'bootstrap_failed'
]);

export interface FrontendLogger {
    report: (event: FrontendLogEvent) => void;
}

/** Cria uma instância isolada de `FrontendLogger` (útil para testes determinísticos e para o singleton). */
export function createFrontendLogger(
    sender: (event: FrontendLogEvent) => Promise<void> = logFrontend
): FrontendLogger & { flushForTests: () => Promise<void> } {
    const queue: FrontendLogEvent[] = [];
    const microtaskSeen = new Set<FrontendLogEvent>();
    let microtaskScheduled = false;
    let inFlight: Promise<void> | null = null;

    function scheduleDrain(): void {
        if (inFlight !== null || queue.length === 0) {
            return;
        }
        const nextEvent = queue.shift();
        if (!nextEvent) {
            return;
        }

        inFlight = sender(nextEvent)
            .catch(() => {
                // Silencia falha de envio de log para impedir recursão infinita de IPC.
            })
            .finally(() => {
                inFlight = null;
                if (queue.length > 0) {
                    scheduleDrain();
                }
            });
    }

    function report(event: FrontendLogEvent): void {
        if (!VALID_EVENTS.has(event)) {
            return;
        }
        if (microtaskSeen.has(event)) {
            return;
        }
        if (queue.length >= MAX_FRONTEND_LOG_QUEUE) {
            return;
        }

        microtaskSeen.add(event);
        if (!microtaskScheduled) {
            microtaskScheduled = true;
            queueMicrotask(() => {
                microtaskSeen.clear();
                microtaskScheduled = false;
            });
        }

        queue.push(event);
        scheduleDrain();
    }

    async function flushForTests(): Promise<void> {
        while (inFlight !== null || queue.length > 0) {
            if (inFlight !== null) {
                await inFlight;
            } else {
                scheduleDrain();
            }
        }
    }

    return {
        report,
        flushForTests
    };
}

/** Instância padrão do logger do frontend. */
export const logger: FrontendLogger = createFrontendLogger();

/**
 * Instala os handlers globais (`app.config.errorHandler`, `window.error`, `unhandledrejection`)
 * descartando mensagens livres e stacks, e retorna uma função de limpeza idempotente.
 */
export function installGlobalErrorHandlers(
    app: App,
    targetLogger: FrontendLogger = logger
): () => void {
    const previousVueHandler = app.config.errorHandler;

    const vueErrorHandler: NonNullable<typeof app.config.errorHandler> = (
        _err,
        _instance,
        _info
    ) => {
        targetLogger.report('vue_error');
    };

    const onWindowError = (_event: Event): void => {
        targetLogger.report('window_error');
    };

    const onUnhandledRejection = (_event: PromiseRejectionEvent): void => {
        targetLogger.report('unhandled_rejection');
    };

    app.config.errorHandler = vueErrorHandler;
    window.addEventListener('error', onWindowError);
    window.addEventListener('unhandledrejection', onUnhandledRejection);

    let disposed = false;
    return () => {
        if (disposed) {
            return;
        }
        disposed = true;
        if (app.config.errorHandler === vueErrorHandler) {
            app.config.errorHandler = previousVueHandler;
        }
        window.removeEventListener('error', onWindowError);
        window.removeEventListener('unhandledrejection', onUnhandledRejection);
    };
}
```

### 4.2. `src/stores/app.ts`

```ts
import { computed, ref } from 'vue';
import { defineStore } from 'pinia';
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
```

---

## 5. Estilos, Tema Max Navy/Teal e Componentes Vue (`src/styles/`, `src/components/shell/`, `src/app/`)

### 5.1. `src/styles/params.scss`, `src/styles/theme.scss`, `src/styles/main.scss`

`src/styles/params.scss`:
```scss
// Mixins e parâmetros SCSS compartilhados (sem emitir CSS direto no escopo global).
@mixin mt-focus-ring {
    outline: 2px solid var(--max-primary-400);
    outline-offset: 2px;
}
```

`src/styles/theme.scss`:
```scss
:root {
    color-scheme: dark;
    --mt-titlebar-height: 40px;
    --mt-statusbar-height: 28px;
    --mt-font-sans: 'Inter', system-ui, -apple-system, sans-serif;
    --mt-font-mono: 'JetBrains Mono', 'Fira Code', monospace;
}

html,
body,
#app {
    width: 100%;
    height: 100vh;
    margin: 0;
    padding: 0;
    overflow: hidden;
    background-color: var(--background-900);
    color: var(--background-100);
    font-family: var(--mt-font-sans);
    -webkit-font-smoothing: antialiased;
}

* {
    box-sizing: border-box;
}

::-webkit-scrollbar {
    width: 6px;
    height: 6px;
}

::-webkit-scrollbar-track {
    background: var(--background-900);
}

::-webkit-scrollbar-thumb {
    background: var(--background-700);
    border-radius: 3px;
}
```

`src/styles/main.scss`:
```scss
@use './theme';
```

### 5.2. `src/components/shell/AppTitleBar.vue`

```vue
<template>
    <header class="app-title-bar" flex items-center gap-2 px-3 h-10>
        <MaxIcon icon="mdi:console" class="app-title-bar__icon" />
        <span class="app-title-bar__title" font-semibold>{{ props.title }}</span>
        <span v-if="props.version" class="app-title-bar__version" text-xs op-60>{{ props.version }}</span>
        <div ml-auto flex items-center gap-2>
            <slot name="actions" />
        </div>
    </header>
</template>

<script setup lang="ts">
    interface AppTitleBarProps {
        title: string;
        version?: string;
    }

    const props = defineProps<AppTitleBarProps>();
</script>

<style lang="scss" scoped>
    .app-title-bar {
        height: var(--mt-titlebar-height);
        background: var(--background-800);
        border-bottom: 1px solid var(--background-700);
        user-select: none;
    }

    .app-title-bar__icon {
        color: var(--max-primary-400);
    }

    .app-title-bar__title {
        color: var(--background-50);
    }

    .app-title-bar__version {
        color: var(--max-primary-300);
        font-family: var(--mt-font-mono);
    }
</style>
```

### 5.3. `src/components/shell/AppEmptyState.vue`

```vue
<template>
    <section class="app-empty-state" flex flex-col items-center justify-center gap-3 p-6 text-center>
        <div class="app-empty-state__badge" flex items-center justify-center w-12 h-12 rounded-full>
            <MaxIcon :icon="props.icon ?? 'mdi:console-line'" class="app-empty-state__icon" />
        </div>
        <h2 class="app-empty-state__title" text-base font-semibold m-0>{{ props.title }}</h2>
        <p v-if="props.description" class="app-empty-state__description" text-sm op-75 m-0>{{ props.description }}</p>
        <div v-if="slots.default" mt-2>
            <slot />
        </div>
    </section>
</template>

<script setup lang="ts">
    import { useSlots } from 'vue';

    interface AppEmptyStateProps {
        icon?: string;
        title: string;
        description?: string;
    }

    const props = defineProps<AppEmptyStateProps>();
    const slots = useSlots();
</script>

<style lang="scss" scoped>
    .app-empty-state {
        width: 100%;
        height: 100%;
        color: var(--background-100);
    }

    .app-empty-state__badge {
        background: var(--background-800);
        border: 1px solid var(--background-700);
    }

    .app-empty-state__icon {
        color: var(--max-primary-400);
        font-size: 1.5rem;
    }

    .app-empty-state__title {
        color: var(--background-50);
    }

    .app-empty-state__description {
        max-width: 28rem;
        color: var(--background-300);
    }
</style>
```

### 5.4. `src/components/shell/AppShell.vue`

```vue
<template>
    <div class="app-shell" :data-loading="props.loading ? 'true' : 'false'">
        <div class="app-shell__titlebar">
            <slot name="titlebar" />
        </div>
        <div class="app-shell__body">
            <aside v-if="slots.sidebar" class="app-shell__sidebar">
                <slot name="sidebar" />
            </aside>
            <main class="app-shell__workspace">
                <slot name="workspace" />
            </main>
        </div>
        <footer v-if="slots.statusbar" class="app-shell__statusbar">
            <slot name="statusbar" />
        </footer>
    </div>
</template>

<script setup lang="ts">
    import { useSlots } from 'vue';

    interface AppShellProps {
        loading?: boolean;
    }

    const props = defineProps<AppShellProps>();
    const slots = useSlots();
</script>

<style lang="scss" scoped>
    .app-shell {
        display: grid;
        grid-template-rows: var(--mt-titlebar-height) 1fr auto;
        width: 100vw;
        height: 100vh;
        overflow: hidden;
        background: var(--background-900);
        color: var(--background-100);
    }

    .app-shell__body {
        display: grid;
        grid-template-columns: auto 1fr;
        min-height: 0;
        overflow: hidden;
    }

    .app-shell__sidebar {
        border-right: 1px solid var(--background-700);
        background: var(--background-800);
        min-height: 0;
        overflow: hidden;
    }

    .app-shell__workspace {
        display: flex;
        flex-direction: column;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
        background: var(--background-900);
    }

    .app-shell__statusbar {
        height: var(--mt-statusbar-height);
        border-top: 1px solid var(--background-700);
        background: var(--background-800);
    }
</style>
```

### 5.5. `src/app/App.vue`

```vue
<template>
    <AppShell :loading="appStore.status === 'loading'">
        <template #titlebar>
            <AppTitleBar :title="appTitle" :version="appStore.versionLabel" />
        </template>
        <template #workspace>
            <AppEmptyState v-if="appStore.status === 'error'" icon="mdi:alert-circle-outline" title="Falha ao inicializar o MaxTerminal" :description="errorDescription">
                <div flex flex-col items-center gap-2>
                    <span v-if="errorCode" class="app-error-code" text-xs px-2 py-0.5 rounded>{{ errorCode }}</span>
                    <button type="button" class="app-retry-btn" px-4 py-1.5 rounded text-sm font-medium cursor-pointer @click="handleRetry">
                        Tentar novamente
                    </button>
                </div>
            </AppEmptyState>
            <AppEmptyState v-else title="Nenhum projeto selecionado" description="Adicione ou selecione um projeto para iniciar suas sessões de terminal." />
        </template>
    </AppShell>
</template>

<script setup lang="ts">
    import { computed, onMounted } from 'vue';
    import AppEmptyState from '@/components/shell/AppEmptyState.vue';
    import AppShell from '@/components/shell/AppShell.vue';
    import AppTitleBar from '@/components/shell/AppTitleBar.vue';
    import { useIpcError } from '@/composables/useIpcError';
    import { useAppStore } from '@/stores/app';

    const appStore = useAppStore();
    const ipcError = useIpcError();

    const appTitle = computed<string>(() => appStore.info?.name ?? 'MaxTerminal');
    const errorDescription = computed<string>(() => ipcError.message(appStore.error));
    const errorCode = computed<string | null>(() => ipcError.code(appStore.error));

    async function handleRetry(): Promise<void> {
        await appStore.retry();
    }

    onMounted(async () => {
        await appStore.bootstrap();
    });
</script>

<style lang="scss" scoped>
    .app-error-code {
        background: var(--background-800);
        color: var(--max-danger-400);
        border: 1px solid var(--max-danger-500);
        font-family: var(--mt-font-mono);
    }

    .app-retry-btn {
        background: var(--max-primary-600);
        color: var(--background-50);
        border: 1px solid var(--max-primary-500);

        &:hover {
            background: var(--max-primary-500);
        }

        &:focus-visible {
            @include mt-focus-ring;
        }
    }
</style>
```

### 5.6. `src/app/plugins.ts` e `src/app/main.ts`

`src/app/plugins.ts`:
- Isola o registro do `createPinia()` integrado ao plugin real exportado por `@maxvue/max-pinia` e o registro dos componentes/estilos de `@maxvue/max-components-ui` (confirmados na inspeção dos `.d.ts` das libs locais).

`src/app/main.ts`:
```ts
import 'virtual:uno.css';
import '@/styles/main.scss';
import { createApp } from 'vue';
import App from './App.vue';
import { installGlobalErrorHandlers } from './logger';
import { registerAppPlugins } from './plugins';

const app = createApp(App);
installGlobalErrorHandlers(app);
registerAppPlugins(app);
app.mount('#app');
```

---

## 6. Especificação dos Testes Unitários Frontend (`tests/`)

| Arquivo de Teste | Cenários Obrigatórios |
|------------------|-----------------------|
| `tests/ipc/core.spec.ts` | 1. `call` resolve `data` quando o wrapper retorna `{ status: 'ok', data }`.<br>2. `call` e `unwrap` lançam `MtIpcError` preservando `code`, `message`, `details` e `command` quando o wrapper retorna `{ status: 'error', error }`.<br>3. `call` converte rejeição de Promise, exceção síncrona e payloads inválidos (`null`, `{}`, `{ status: 'unknown' }`) em `MtIpcError` com `code === 'ipc.transport'` e mensagem estática em `pt-BR` (nunca repassa texto cru da exceção).<br>4. `useIpcError()` retorna mensagem mapeada por `code`, fallback para `err.message` em `MtIpcError` e mensagem genérica para erros desconhecidos. |
| `tests/stores/app.spec.ts` | 1. `bootstrap()` com `mockIPC` respondendo `app_info` preenche `info`, `status === 'ready'`, `isReady === true`, `versionLabel === 'v0.1.0'` e `isDebug`.<br>2. Chamadas concorrentes a `bootstrap()` durante `loading` compartilham a mesma Promise em voo e disparam exatamente **1** invocação IPC.<br>3. `bootstrap()` quando já `ready` é no-op.<br>4. `registerBootstrapStep` executa passos na ordem exata de registro antes de marcar `ready`; rejeita nomes vazios, duplicados ou registrados após o início do bootstrap.<br>5. Em falha de `app_info` (via `mockIPC`), `status === 'error'`, `error` é `MtIpcError`, `logger.report('bootstrap_failed')` é acionado, e `retry()` recupera para `ready`. |
| `tests/app/logger.spec.ts` | 1. Deduplica eventos idênticos emitidos na mesma microtask.<br>2. Respeita fila máxima de 64 itens e mantém no máximo 1 chamada IPC em voo por vez.<br>3. Falha na chamada `logFrontend` não lança exceção não tratada nem gera recursão de log.<br>4. `installGlobalErrorHandlers` captura erros do Vue (`vue_error`), `error` (`window_error`) e `unhandledrejection` (`unhandled_rejection`) enviando **apenas** o enum (sentinelas `"BEARER_SECRET_999"` na mensagem do `Error` nunca chegam ao IPC) e a função de cleanup remove todos os listeners de forma idempotente. |
| `tests/components/shell/AppShell.spec.ts` | 1. `AppShell` renderiza `titlebar`, `workspace` e oculta `sidebar`/`statusbar` quando vazios; renderiza ambos quando preenchidos.<br>2. `App.vue` com `mockIPC` de sucesso exibe `"MaxTerminal"`, `"v0.1.0"` e `"Nenhum projeto selecionado"`.<br>3. `App.vue` com `mockIPC` de falha exibe a mensagem em `pt-BR`, o `code` do `MtError` e o botão `"Tentar novamente"`, que ao ser clicado executa `retry()` e transita para o estado pronto. |
