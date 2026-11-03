import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
    commands,
    type ArtisanCatalog,
    type ArtisanCommandForm,
    type ArtisanCreateResult,
    type ArtisanPreview,
    type ArtisanRoot,
    type CatalogUpdatedPayload,
    type CommandCreatedPayload,
    type DiscoveryFailedPayload,
    type ProjectId,
    type ResolvedShortcut,
    type ShortcutId,
    type ShortcutList,
    type ValidationReport
} from '@/types/bindings';
import { unwrap } from './core';

export const quickCommandsIpc = {
    resolveRoot: (cwd: string, projectId: ProjectId | null): Promise<ArtisanRoot | null> =>
        unwrap(commands.artisanResolveRoot(cwd, projectId), 'artisan_resolve_root'),
    catalogGet: (root: string): Promise<ArtisanCatalog> =>
        unwrap(commands.artisanCatalogGet(root), 'artisan_catalog_get'),
    catalogRefresh: (root: string): Promise<ArtisanCatalog> =>
        unwrap(commands.artisanCatalogRefresh(root), 'artisan_catalog_refresh'),
    formSchema: (): Promise<Record<string, unknown>> =>
        unwrap(commands.artisanFormSchema(), 'artisan_form_schema') as Promise<Record<string, unknown>>,
    validateForm: (root: string, form: ArtisanCommandForm): Promise<ValidationReport> =>
        unwrap(commands.artisanValidateForm(root, form), 'artisan_validate_form'),
    preview: (root: string, form: ArtisanCommandForm): Promise<ArtisanPreview> =>
        unwrap(commands.artisanPreviewCommand(root, form), 'artisan_preview_command'),
    create: (
        root: string,
        form: ArtisanCommandForm,
        expectedRevision: string
    ): Promise<ArtisanCreateResult> =>
        unwrap(
            commands.artisanCreateCommand(root, form, expectedRevision),
            'artisan_create_command'
        ),
    maxcodePrompt: (root: string, form: ArtisanCommandForm): Promise<string> =>
        unwrap(
            commands.artisanFormatMaxcodePrompt(root, form),
            'artisan_format_maxcode_prompt'
        ),
    shortcutList: (cwd: string): Promise<ShortcutList> =>
        unwrap(commands.shortcutList(cwd), 'shortcut_list'),
    shortcutResolve: (cwd: string, id: ShortcutId): Promise<ResolvedShortcut> =>
        unwrap(commands.shortcutResolve(cwd, id), 'shortcut_resolve')
};

export const onCatalogUpdated = (
    cb: (payload: CatalogUpdatedPayload) => void
): Promise<UnlistenFn> =>
    listen<CatalogUpdatedPayload>('artisan://catalog-updated', (event) => cb(event.payload));

export const onDiscoveryFailed = (
    cb: (payload: DiscoveryFailedPayload) => void
): Promise<UnlistenFn> =>
    listen<DiscoveryFailedPayload>('artisan://discovery-failed', (event) => cb(event.payload));

export const onCommandCreated = (
    cb: (payload: CommandCreatedPayload) => void
): Promise<UnlistenFn> =>
    listen<CommandCreatedPayload>('artisan://command-created', (event) => cb(event.payload));
