import { TerminalApi, createTerminal, type Message, type TerminalMessageClass } from 'vue-web-terminal';
import type { PaneId } from '@/types/bindings';
import type {
    CommandBlockModel,
    Disposable,
    TerminalContext
} from '@/types/terminal';

/** Entrada projetada pelo `VueWebTerminalAdapter` para renderização segura na camada de blocos. */
export interface VueWebTerminalProjectedEntry {
    key: string;
    blockId: CommandBlockModel['id'];
    type: 'normal' | 'ansi';
    class: 'success' | 'error' | 'warning' | 'info' | 'system';
    tag: string;
    content: string;
    command: string;
    cwd: string | null;
    status: CommandBlockModel['status'];
    exitCode: number | null;
    durationMs: number | null;
}

/** Opções de construção do `VueWebTerminalAdapter`. */
export interface VueWebTerminalAdapterOptions {
    paneId: PaneId;
    getContext: () => TerminalContext;
    onInputIntent?: ((commandText: string) => Promise<void>) | undefined;
}

const MAX_COMMAND_CHARS = 4096;

function stripHtmlTags(raw: string): string {
    return raw.replace(/[<>]/g, '');
}

function truncateText(raw: string, maxChars: number): string {
    if (raw.length <= maxChars) {
        return raw;
    }
    return `${raw.slice(0, maxChars)}…`;
}

function mapStatusToClassAndTag(
    block: CommandBlockModel
): { msgClass: TerminalMessageClass; tag: string } {
    switch (block.status) {
        case 'running':
            return { msgClass: 'info', tag: 'EXEC' };
        case 'success':
            return { msgClass: 'success', tag: 'OK' };
        case 'error':
            return { msgClass: 'error', tag: `ERRO ${block.exitCode ?? 1}` };
        case 'aborted':
            return { msgClass: 'warning', tag: ' INTERROMPIDO' };
    }
}

/**
 * Adaptador que encapsula `vue-web-terminal` conforme o ADR-04-01:
 * projeta `CommandBlockModel` em estruturas de mensagem validadas da biblioteca
 * sem criar segundo leitor de PTY, sem instanciar emulador paralelo e sem usar `v-html`.
 */
export class VueWebTerminalAdapter implements Disposable {
    private readonly opts: VueWebTerminalAdapterOptions;
    private readonly instanceInstalled: boolean;
    private disposed = false;

    public constructor(opts: VueWebTerminalAdapterOptions) {
        this.opts = opts;
        // Verifica disponibilidade real da API pública de vue-web-terminal sem montar emulador paralelo.
        const pluginInstance = typeof createTerminal === 'function' ? createTerminal() : null;
        this.instanceInstalled = Boolean(pluginInstance && TerminalApi);
    }

    /** Retorna metadados comprovando a vinculação com o pacote real `vue-web-terminal`. */
    public getLibraryMetadata(): { packageName: 'vue-web-terminal'; available: boolean } {
        return {
            packageName: 'vue-web-terminal',
            available: this.instanceInstalled
        };
    }

    /** Projeta um `CommandBlockModel` para uma entrada estruturada compatível com `Message` de `vue-web-terminal`. */
    public projectBlock(
        block: CommandBlockModel,
        outputPreview?: string
    ): VueWebTerminalProjectedEntry {
        const { msgClass, tag } = mapStatusToClassAndTag(block);
        const safeCommand = truncateText(block.command, MAX_COMMAND_CHARS);
        const safeContent = stripHtmlTags(
            truncateText(outputPreview ?? block.command, MAX_COMMAND_CHARS)
        );
        const durationMs =
            block.finishedAt !== null ? Math.max(0, block.finishedAt - block.startedAt) : null;

        // Valida conformidade estrutural com o contrato Message de vue-web-terminal (nunca type: 'html').
        const vwtMessage: Message = {
            type: 'normal',
            class: msgClass,
            tag: tag.trim(),
            content: safeContent
        };

        return {
            key: `${this.opts.paneId}:${block.id}`,
            blockId: block.id,
            type: vwtMessage.type === 'ansi' ? 'ansi' : 'normal',
            class: msgClass,
            tag: String(vwtMessage.tag ?? ''),
            content: String(vwtMessage.content ?? ''),
            command: safeCommand,
            cwd: block.cwd,
            status: block.status,
            exitCode: block.exitCode,
            durationMs
        };
    }

    /** Projeta uma coleção ordenada de blocos do painel. */
    public projectBlocks(blocks: readonly CommandBlockModel[]): VueWebTerminalProjectedEntry[] {
        if (this.disposed) {
            return [];
        }
        return blocks.map((block) => this.projectBlock(block));
    }

    /**
     * Encaminha qualquer intenção de comando (`exec-cmd`) exclusivamente para o `InputPipeline` único da instância,
     * sem criar leitor nem eco local duplicado.
     */
    public async handleExecCmd(
        _cmdKey: string,
        command: string,
        success?: (msg?: unknown) => void,
        failed?: (msg?: string) => void
    ): Promise<void> {
        if (this.disposed) {
            failed?.('Adaptador de blocos já foi descartado.');
            return;
        }
        const trimmed = command.trim();
        if (trimmed.length === 0) {
            success?.();
            return;
        }
        try {
            const _ctx = this.opts.getContext();
            await this.opts.onInputIntent?.(command);
            success?.();
        } catch (err: unknown) {
            failed?.(err instanceof Error ? err.message : 'Falha ao enviar comando ao terminal.');
        }
    }

    public isDisposed(): boolean {
        return this.disposed;
    }

    public dispose(): void {
        this.disposed = true;
    }
}

/** Factory canônica do `VueWebTerminalAdapter` (invocada exclusivamente por `useCommandBlocks`). */
export function createVueWebTerminalAdapter(
    opts: VueWebTerminalAdapterOptions
): VueWebTerminalAdapter {
    return new VueWebTerminalAdapter(opts);
}
