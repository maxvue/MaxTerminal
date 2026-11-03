<template>
    <div class="skills-sync-panel" data-testid="skills-sync-panel">
        <header class="skills-sync-panel__header">
            <div>
                <h3 class="skills-sync-panel__title">Sincronização MaxSkills</h3>
                <p class="skills-sync-panel__subtitle">{{ catalog ? `Fonte: ${catalog.sourceDir} (${catalog.skills.length} skills)` : 'Catálogo local de skills para Agentes CLI.' }}</p>
            </div>
            <button type="button" class="skills-sync-panel__btn" :disabled="isScanning" data-testid="skills-scan-btn" @click="onScan">
                <MaxIcon icon="lucide:folder-sync" />
                <span>{{ isScanning ? 'Escaneando...' : 'Escanear MaxSkills' }}</span>
            </button>
        </header>

        <div v-if="activeError" class="skills-sync-panel__error" role="alert" data-testid="skills-sync-error">
            <MaxIcon icon="lucide:alert-circle" />
            <span>{{ activeError.message }}</span>
        </div>

        <section class="skills-sync-panel__new-target">
            <h4 class="skills-sync-panel__section-title">Configurar Alvo de Sincronização</h4>
            <div class="skills-sync-panel__form-row">
                <select v-model="draftAgent" class="skills-sync-panel__select" aria-label="Agente CLI" data-testid="target-agent-select">
                    <option value="claude-code">Claude Code</option>
                    <option value="codex">Codex</option>
                    <option value="agy">Antigravity (agy)</option>
                    <option value="opencode">OpenCode</option>
                </select>
                <select v-model="draftScope" class="skills-sync-panel__select" aria-label="Escopo" data-testid="target-scope-select">
                    <option v-if="props.projectId" value="project">Projeto Atual</option>
                    <option value="global">Global (Usuário)</option>
                </select>
                <select v-model="draftMode" class="skills-sync-panel__select" aria-label="Modo de sincronização" data-testid="target-mode-select">
                    <option value="symlink">Symlink</option>
                    <option value="copy">Cópia com Manifesto</option>
                </select>
                <input v-if="draftAgent === 'agy'" v-model="draftDestOverride" type="text" class="skills-sync-panel__input" placeholder="Destino explícito (ex.: .agent/skills)" data-testid="target-dest-override">
                <button type="button" class="skills-sync-panel__btn skills-sync-panel__btn--primary" data-testid="target-save-btn" @click="onSaveTarget">
                    <MaxIcon icon="lucide:plus" />
                    <span>Salvar Alvo</span>
                </button>
            </div>
        </section>

        <section v-if="skillsStore.targets.length > 0" class="skills-sync-panel__targets">
            <div v-for="target in skillsStore.targets" :key="target.id ?? `${target.agent}-${target.scope}`" class="skills-sync-panel__target-card" :data-testid="`skill-target-${target.id}`">
                <div class="skills-sync-panel__target-head">
                    <div class="skills-sync-panel__target-meta">
                        <strong>{{ target.agent }}</strong>
                        <span class="skills-sync-panel__badge">{{ target.scope === 'project' ? 'Projeto' : 'Global' }}</span>
                        <span class="skills-sync-panel__badge">{{ target.mode === 'symlink' ? 'Symlink' : 'Cópia' }}</span>
                        <code v-if="target.destDir" class="skills-sync-panel__dest">{{ target.destDir }}</code>
                    </div>
                    <div class="skills-sync-panel__target-actions">
                        <button v-if="target.id !== null" type="button" class="skills-sync-panel__btn" :data-testid="`plan-target-${target.id}`" @click="onPlanTarget(target.id)">Simular (Dry-Run)</button>
                        <button v-if="target.id !== null" type="button" class="skills-sync-panel__btn skills-sync-panel__btn--danger" :data-testid="`remove-target-${target.id}`" @click="onRemoveTarget(target.id)">Remover</button>
                    </div>
                </div>

                <div v-if="target.id !== null && planFor(target.id)" class="skills-sync-panel__plan" :data-testid="`sync-plan-${target.id}`">
                    <div v-if="planFor(target.id)!.warnings.length > 0" class="skills-sync-panel__warnings">
                        <p v-for="(warn, wIdx) in planFor(target.id)!.warnings" :key="wIdx" class="skills-sync-panel__warning-item">{{ warn }}</p>
                    </div>
                    <p class="skills-sync-panel__plan-summary">{{ formatPlanSummary(planFor(target.id)!) }}</p>
                    <SkillConflictList :conflicts="conflictsFor(planFor(target.id)!)" :model-value="resolutionsByTarget[target.id] ?? {}" @update:model-value="setResolutions(target.id, $event)" />
                    <div class="skills-sync-panel__apply-row">
                        <button type="button" class="skills-sync-panel__btn skills-sync-panel__btn--primary" :data-testid="`apply-plan-${target.id}`" @click="onApplyPlan(target.id, planFor(target.id)!)">Aplicar Sincronização</button>
                    </div>
                </div>

                <div v-if="target.id !== null && resultFor(target.id)" class="skills-sync-panel__result" :data-testid="`sync-result-${target.id}`">
                    <span>Aplicadas: {{ resultFor(target.id)!.applied }} · Mantidas: {{ resultFor(target.id)!.skipped }} · Backups: {{ resultFor(target.id)!.backups.length }}</span>
                </div>
            </div>
        </section>
    </div>
</template>

<script setup lang="ts">
    import { computed, onMounted, ref } from 'vue';
    import { useSkillsStore } from '@/stores/skills';
    import type {
        AgentKind,
        ConflictResolution,
        MtError_Serialize,
        ProjectId,
        SkillCatalog,
        SyncAction,
        SyncMode,
        SyncPlan,
        SyncResult,
        SyncScope
    } from '@/types/bindings';
    import SkillConflictList, { type ConflictSyncAction } from './SkillConflictList.vue';

    export interface SkillsSyncPanelProps {
        projectId: ProjectId | null;
    }

    const props = defineProps<SkillsSyncPanelProps>();

    const skillsStore = useSkillsStore();

    const draftAgent = ref<AgentKind>('claude-code');
    const draftScope = ref<SyncScope>(props.projectId ? 'project' : 'global');
    const draftMode = ref<SyncMode>('symlink');
    const draftDestOverride = ref<string>('');
    const resolutionsByTarget = ref<Record<number, Record<string, ConflictResolution>>>({});

    const catalog = computed<SkillCatalog | null>(() => skillsStore.catalogFor(props.projectId));

    const isScanning = computed<boolean>(() => {
        const key = `scan:${props.projectId ?? '__global__'}`;
        return Boolean(skillsStore.loadingByKey[key]);
    });

    const activeError = computed<MtError_Serialize | null>(() => {
        for (const err of Object.values(skillsStore.errorsByKey)) {
            if (err) {
                return err;
            }
        }
        return null;
    });

    onMounted(() => {
        void skillsStore.init();
        void skillsStore.loadTargets(props.projectId).catch(() => {
            // Ignora falha silenciosa no boot inicial.
        });
    });

    function planFor(targetId: number): SyncPlan | undefined {
        return skillsStore.plansByTarget[targetId];
    }

    function resultFor(targetId: number): SyncResult | undefined {
        return skillsStore.syncResults[targetId];
    }

    function conflictsFor(plan: SyncPlan): ConflictSyncAction[] {
        return plan.actions.filter(
            (a): a is ConflictSyncAction => a.action === 'conflict'
        );
    }

    function setResolutions(targetId: number, value: Record<string, ConflictResolution>): void {
        resolutionsByTarget.value = {
            ...resolutionsByTarget.value,
            [targetId]: value
        };
    }

    function formatPlanSummary(plan: SyncPlan): string {
        let createCount = 0;
        let updateCount = 0;
        let unchangedCount = 0;
        let conflictCount = 0;
        let pruneCount = 0;
        for (const act of plan.actions) {
            if (act.action === 'create') createCount += 1;
            else if (act.action === 'update') updateCount += 1;
            else if (act.action === 'unchanged') unchangedCount += 1;
            else if (act.action === 'conflict') conflictCount += 1;
            else if (act.action === 'prune') pruneCount += 1;
        }
        return `Criar: ${createCount} · Atualizar: ${updateCount} · Inalteradas: ${unchangedCount} · Conflitos: ${conflictCount} · Remover: ${pruneCount}`;
    }

    async function onScan(): Promise<void> {
        try {
            await skillsStore.scan(props.projectId);
        } catch {
            // Erro registrado na store.
        }
    }

    async function onSaveTarget(): Promise<void> {
        try {
            await skillsStore.upsertTarget({
                id: null,
                agent: draftAgent.value,
                scope: draftScope.value,
                projectId: draftScope.value === 'project' ? props.projectId : null,
                mode: draftMode.value,
                autoSync: false,
                enabled: true,
                selection: { kind: 'recommended' },
                projectRoot: null,
                destOverride: draftDestOverride.value.trim() || null,
                destDir: null
            });
            draftDestOverride.value = '';
        } catch {
            // Erro registrado na store.
        }
    }

    async function onPlanTarget(targetId: number): Promise<void> {
        try {
            const plan = await skillsStore.planSync(targetId);
            const initial: Record<string, ConflictResolution> = {};
            for (const item of plan.actions as SyncAction[]) {
                if (item.action === 'conflict') {
                    initial[item.skill] = 'keep';
                }
            }
            setResolutions(targetId, initial);
        } catch {
            // Erro registrado na store.
        }
    }

    async function onApplyPlan(targetId: number, plan: SyncPlan): Promise<void> {
        try {
            await skillsStore.applySync(
                plan.planId,
                resolutionsByTarget.value[targetId] ?? {},
                targetId
            );
        } catch {
            // Erro registrado na store.
        }
    }

    async function onRemoveTarget(targetId: number): Promise<void> {
        try {
            await skillsStore.removeTarget(targetId, false);
        } catch {
            // Erro registrado na store.
        }
    }
</script>

<style lang="scss" scoped>
    .skills-sync-panel {
        display: flex;
        flex-direction: column;
        gap: 1rem;
        padding: 1rem;
        color: var(--background-100);
    }

    .skills-sync-panel__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 1rem;
    }

    .skills-sync-panel__title {
        margin: 0;
        font-size: 1rem;
        font-weight: 600;
    }

    .skills-sync-panel__subtitle {
        margin: 0.25rem 0 0;
        font-size: 0.7813rem;
        color: var(--background-400);
    }

    .skills-sync-panel__error {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        padding: 0.5rem 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--max-danger-500);
        background: rgb(239 68 68 / 15%);
        color: var(--max-danger-300);
        font-size: 0.8125rem;
    }

    .skills-sync-panel__new-target,
    .skills-sync-panel__target-card {
        display: flex;
        flex-direction: column;
        gap: 0.625rem;
        padding: 0.75rem;
        border-radius: 0.5rem;
        border: 1px solid var(--background-700);
        background: var(--background-850);
    }

    .skills-sync-panel__section-title {
        margin: 0;
        font-size: 0.8125rem;
        font-weight: 600;
        color: var(--background-200);
    }

    .skills-sync-panel__form-row {
        display: flex;
        flex-wrap: wrap;
        gap: 0.5rem;
        align-items: center;
    }

    .skills-sync-panel__select,
    .skills-sync-panel__input {
        padding: 0.3125rem 0.5rem;
        border-radius: 0.25rem;
        border: 1px solid var(--background-600);
        background: var(--background-800);
        color: var(--background-100);
        font-size: 0.7813rem;
    }

    .skills-sync-panel__btn {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        padding: 0.3125rem 0.625rem;
        border-radius: 0.25rem;
        border: 1px solid var(--background-600);
        background: var(--background-800);
        color: var(--background-100);
        font-size: 0.7813rem;
        font-weight: 600;
        cursor: pointer;
    }

    .skills-sync-panel__btn--primary {
        border-color: var(--max-primary-500);
        background: rgb(20 184 166 / 18%);
        color: var(--max-primary-300);
    }

    .skills-sync-panel__btn--danger {
        border-color: var(--max-danger-500);
        color: var(--max-danger-300);
    }

    .skills-sync-panel__targets {
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
    }

    .skills-sync-panel__target-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
        flex-wrap: wrap;
    }

    .skills-sync-panel__target-meta {
        display: inline-flex;
        align-items: center;
        gap: 0.5rem;
        flex-wrap: wrap;
        font-size: 0.8125rem;
    }

    .skills-sync-panel__badge {
        padding: 0.125rem 0.375rem;
        border-radius: 0.25rem;
        background: var(--background-700);
        font-size: 0.6875rem;
        color: var(--background-200);
    }

    .skills-sync-panel__dest {
        font-size: 0.75rem;
        color: var(--background-400);
    }

    .skills-sync-panel__target-actions {
        display: inline-flex;
        gap: 0.375rem;
    }

    .skills-sync-panel__plan {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
        padding-top: 0.5rem;
        border-top: 1px solid var(--background-700);
    }

    .skills-sync-panel__warnings {
        padding: 0.375rem 0.5rem;
        border-radius: 0.25rem;
        background: rgb(245 158 11 / 14%);
        color: var(--max-warning-300);
        font-size: 0.75rem;
    }

    .skills-sync-panel__warning-item {
        margin: 0;
    }

    .skills-sync-panel__plan-summary {
        margin: 0;
        font-size: 0.7813rem;
        color: var(--background-200);
    }

    .skills-sync-panel__apply-row {
        display: flex;
        justify-content: flex-end;
    }

    .skills-sync-panel__result {
        font-size: 0.75rem;
        color: var(--max-success-400);
    }
</style>
