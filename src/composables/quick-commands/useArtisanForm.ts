import { computed, ref, watch, type ComputedRef, type Ref } from 'vue';
import {
    suggestClassNameFromSignature,
    useQuickCommandsStore
} from '@/stores/quickCommands';
import type {
    ArtisanCommandForm,
    ArtisanParam,
    FieldIssue,
    ScheduleSpec
} from '@/types/bindings';

export interface UseArtisanFormReturn {
    form: ComputedRef<ArtisanCommandForm>;
    classNameTouched: Ref<boolean>;
    issuesByPath: ComputedRef<Record<string, FieldIssue[]>>;
    generalIssues: ComputedRef<FieldIssue[]>;
    canCreate: ComputedRef<boolean>;
    hasAiSnapshot: ComputedRef<boolean>;
    setClassName: (value: string) => void;
    setSignatureName: (value: string) => void;
    setDescription: (value: string) => void;
    setParams: (params: ArtisanParam[]) => void;
    setIsolatable: (value: boolean) => void;
    setPromptsForMissingInput: (value: boolean) => void;
    setSchedule: (schedule: ScheduleSpec | null) => void;
    setObjective: (value: string) => void;
    issuesFor: (path: string) => FieldIssue[];
    firstErrorMessageFor: (path: string) => string | null;
    focusFirstInvalidField: (container: HTMLElement | null) => void;
}

export function useArtisanForm(): UseArtisanFormReturn {
    const store = useQuickCommandsStore();
    const classNameTouched = ref<boolean>(false);

    // Quando o modal abre para uma nova sessão/contexto ou recebe sugestão da IA, sincroniza classNameTouched
    watch(
        () => store.modal.contextRevision,
        () => {
            const f = store.modal.form;
            if (!f.className) {
                classNameTouched.value = false;
            } else {
                const autoName = suggestClassNameFromSignature(f.signatureName);
                classNameTouched.value = f.className !== autoName;
            }
        },
        { immediate: true }
    );

    watch(
        () => store.modal.aiSnapshot,
        (snap) => {
            if (snap !== null) {
                classNameTouched.value = true;
            }
        }
    );

    const form = computed<ArtisanCommandForm>(() => store.modal.form);
    const canCreate = computed<boolean>(() => store.canCreate);
    const hasAiSnapshot = computed<boolean>(() => store.modal.aiSnapshot !== null);

    const issuesByPath = computed<Record<string, FieldIssue[]>>(() => {
        const map: Record<string, FieldIssue[]> = {};
        const issues = store.modal.report?.issues ?? [];
        for (const issue of issues) {
            const key = issue.path || 'general';
            const list = map[key] ?? [];
            list.push(issue);
            map[key] = list;
        }
        return map;
    });

    const generalIssues = computed<FieldIssue[]>(() => issuesByPath.value.general ?? []);

    function setClassName(value: string): void {
        classNameTouched.value = value.trim().length > 0;
        store.updateForm({ className: value });
    }

    function setSignatureName(value: string): void {
        const patch: Partial<ArtisanCommandForm> = { signatureName: value };
        if (!classNameTouched.value) {
            patch.className = suggestClassNameFromSignature(value);
        }
        store.updateForm(patch);
    }

    function setDescription(value: string): void {
        store.updateForm({ description: value });
    }

    function setParams(params: ArtisanParam[]): void {
        store.updateForm({ params });
    }

    function setIsolatable(value: boolean): void {
        store.updateForm({ isolatable: value });
    }

    function setPromptsForMissingInput(value: boolean): void {
        store.updateForm({ promptsForMissingInput: value });
    }

    function setSchedule(schedule: ScheduleSpec | null): void {
        store.updateForm({ schedule });
    }

    function setObjective(value: string): void {
        store.updateForm({ objective: value });
    }

    function issuesFor(path: string): FieldIssue[] {
        return issuesByPath.value[path] ?? [];
    }

    function firstErrorMessageFor(path: string): string | null {
        const list = issuesFor(path);
        const found = list.find((i) => i.severity === 'error');
        return found ? found.message : null;
    }

    function focusFirstInvalidField(container: HTMLElement | null): void {
        if (!container) {
            return;
        }
        const issues = store.modal.report?.issues ?? [];
        const firstErr = issues.find((i) => i.severity === 'error');
        if (!firstErr) {
            return;
        }
        const escaped = firstErr.path.replace(/"/g, '\\"');
        const el = container.querySelector<HTMLElement>(`[data-field-path="${escaped}"]`);
        if (el && typeof el.focus === 'function') {
            el.focus();
        }
    }

    return {
        form,
        classNameTouched,
        issuesByPath,
        generalIssues,
        canCreate,
        hasAiSnapshot,
        setClassName,
        setSignatureName,
        setDescription,
        setParams,
        setIsolatable,
        setPromptsForMissingInput,
        setSchedule,
        setObjective,
        issuesFor,
        firstErrorMessageFor,
        focusFirstInvalidField
    };
}
