import type { Disposable, TerminalTheme, TerminalThemeProvider } from '@/types/terminal';
import { createMaxNavyTealTheme } from './defaultTheme';

/** Interface estendida do serviço global de tema do terminal. */
export interface ThemeService extends TerminalThemeProvider {
    setThemeProvider(provider: TerminalThemeProvider | null): Disposable;
    notifyThemeChanged(theme?: TerminalTheme): void;
}

class DefaultThemeService implements ThemeService {
    private externalProvider: TerminalThemeProvider | null = null;
    private externalSub: Disposable | null = null;
    private currentFallbackTheme: TerminalTheme | null = null;
    private readonly listeners = new Set<(theme: TerminalTheme) => void>();

    public getActiveTheme(): TerminalTheme {
        if (this.externalProvider) {
            return this.externalProvider.getActiveTheme();
        }
        if (!this.currentFallbackTheme) {
            this.currentFallbackTheme = createMaxNavyTealTheme();
        }
        return this.currentFallbackTheme;
    }

    public onChange(cb: (theme: TerminalTheme) => void): Disposable {
        this.listeners.add(cb);
        return {
            dispose: () => {
                this.listeners.delete(cb);
            }
        };
    }

    public setThemeProvider(provider: TerminalThemeProvider | null): Disposable {
        if (this.externalSub) {
            this.externalSub.dispose();
            this.externalSub = null;
        }
        this.externalProvider = provider;
        if (provider) {
            this.externalSub = provider.onChange((theme) => {
                this.emit(theme);
            });
            this.emit(provider.getActiveTheme());
        } else {
            this.currentFallbackTheme = createMaxNavyTealTheme();
            this.emit(this.currentFallbackTheme);
        }

        return {
            dispose: () => {
                if (this.externalProvider === provider) {
                    this.setThemeProvider(null);
                }
            }
        };
    }

    public notifyThemeChanged(theme?: TerminalTheme): void {
        if (theme) {
            this.currentFallbackTheme = theme;
            this.emit(theme);
            return;
        }
        this.currentFallbackTheme = createMaxNavyTealTheme();
        this.emit(this.getActiveTheme());
    }

    private emit(theme: TerminalTheme): void {
        for (const listener of this.listeners) {
            listener(theme);
        }
    }
}

/** Instância singleton do serviço de tema do terminal (plugável pela Fase 10). */
export const themeService: ThemeService = new DefaultThemeService();
