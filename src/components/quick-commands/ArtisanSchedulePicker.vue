<template>
    <div class="schedule-picker" data-testid="artisan-schedule-picker">
        <MaxInputCheckbox :model-value="modelValue !== null" label="Agendamento (Schedule)" data-testid="schedule-enable-checkbox" @update:model-value="onToggleSchedule" />

        <div v-if="modelValue !== null" class="schedule-picker__body">
            <div class="schedule-picker__row">
                <div class="schedule-picker__field">
                    <span class="schedule-picker__label">Frequência</span>
                    <MaxInputSelect data-testid="schedule-frequency-select" aria-label="Frequência do agendamento" :model-value="modelValue.frequency.kind" :options="frequencySelectOptions" option-label="label" option-value="value" option-name="label" @change="onFrequencyKindChange" />
                </div>

                <div v-if="modelValue.frequency.kind === 'hourlyAt'" class="schedule-picker__field">
                    <span class="schedule-picker__label">Minuto (0–59)</span>
                    <MaxInputNumber :model-value="modelValue.frequency.minute" :min="0" :max="59" :step="1" aria-label="Minuto (0–59)" data-field-path="schedule.frequency.minute" class="schedule-picker__number" @update:model-value="onMinuteNumberChange" />
                </div>

                <div v-if="modelValue.frequency.kind === 'dailyAt' || modelValue.frequency.kind === 'weeklyOn' || modelValue.frequency.kind === 'monthlyOn'" class="schedule-picker__field">
                    <span class="schedule-picker__label">Horário (HH:MM)</span>
                    <MaxInputText :model-value="currentTimeValue" placeholder="08:00" aria-label="Horário (HH:MM)" data-field-path="schedule.frequency.time" class="schedule-picker__text-input" @update:model-value="onTimeStringChange" />
                </div>

                <div v-if="modelValue.frequency.kind === 'weeklyOn'" class="schedule-picker__field">
                    <span class="schedule-picker__label">Dia da semana</span>
                    <MaxInputSelect :model-value="modelValue.frequency.day" :options="weekdaySelectOptions" option-label="label" option-value="value" option-name="label" aria-label="Dia da semana" data-field-path="schedule.frequency.day" @change="onWeekdaySelectChange" />
                </div>

                <div v-if="modelValue.frequency.kind === 'monthlyOn'" class="schedule-picker__field">
                    <span class="schedule-picker__label">Dia do mês (1–31)</span>
                    <MaxInputNumber :model-value="modelValue.frequency.day" :min="1" :max="31" :step="1" aria-label="Dia do mês (1–31)" data-field-path="schedule.frequency.day" class="schedule-picker__number" @update:model-value="onMonthDayNumberChange" />
                </div>

                <template v-if="modelValue.frequency.kind === 'twiceDaily'">
                    <div class="schedule-picker__field">
                        <span class="schedule-picker__label">1ª hora (0–23)</span>
                        <MaxInputNumber :model-value="modelValue.frequency.first" :min="0" :max="23" :step="1" aria-label="1ª hora (0–23)" data-field-path="schedule.frequency.first" class="schedule-picker__number" @update:model-value="onTwiceFirstNumberChange" />
                    </div>
                    <div class="schedule-picker__field">
                        <span class="schedule-picker__label">2ª hora (0–23)</span>
                        <MaxInputNumber :model-value="modelValue.frequency.second" :min="0" :max="23" :step="1" aria-label="2ª hora (0–23)" data-field-path="schedule.frequency.second" class="schedule-picker__number" @update:model-value="onTwiceSecondNumberChange" />
                    </div>
                </template>

                <div v-if="modelValue.frequency.kind === 'cron'" class="schedule-picker__field schedule-picker__field--wide">
                    <span class="schedule-picker__label">Expressão Cron (5 campos)</span>
                    <MaxInputText :model-value="modelValue.frequency.expression" placeholder="0 8 * * 1" aria-label="Expressão Cron" data-testid="schedule-cron-input" data-field-path="schedule.frequency.expression" class="schedule-picker__text-input" @update:model-value="onCronTextChange" />
                </div>
            </div>

            <div v-if="scheduleIssues.length > 0" class="schedule-picker__errors">
                <MaxAlert v-for="(iss, idx) in scheduleIssues" :key="idx" severity="danger" :message="iss.message" class="schedule-picker__error-item" />
            </div>

            <div v-if="argumentParams.length > 0" class="schedule-picker__args">
                <span class="schedule-picker__subtitle">Valores dos argumentos no agendamento</span>
                <div class="schedule-picker__row">
                    <div v-for="(arg, idx) in argumentParams" :key="arg.name || idx" class="schedule-picker__field">
                        <span class="schedule-picker__label">{{ arg.name || `Argumento ${idx + 1}` }}{{ arg.optional ? ' (opcional)' : ' *' }}</span>
                        <MaxInputText :model-value="(modelValue.arguments ?? [])[idx] ?? ''" :data-field-path="`schedule.arguments[${idx}]`" :aria-label="arg.name || `Argumento ${idx + 1}`" class="schedule-picker__text-input" @update:model-value="(val) => onScheduleArgTextChange(idx, val)" />
                    </div>
                </div>
            </div>

            <div class="schedule-picker__modifiers">
                <MaxInputCheckbox :model-value="Boolean(modelValue.withoutOverlapping)" label="Sem sobreposição (withoutOverlapping)" class="schedule-picker__checkbox" @update:model-value="(val) => patchModifier('withoutOverlapping', val)" />
                <MaxInputCheckbox :model-value="Boolean(modelValue.onOneServer)" label="Em um único servidor (onOneServer)" class="schedule-picker__checkbox" @update:model-value="(val) => patchModifier('onOneServer', val)" />
                <MaxInputCheckbox :model-value="Boolean(modelValue.runInBackground)" label="Em segundo plano (runInBackground)" class="schedule-picker__checkbox" @update:model-value="(val) => patchModifier('runInBackground', val)" />
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxAlert, MaxInputCheckbox, MaxInputNumber, MaxInputSelect, MaxInputText } from '@maxvue/max-components-ui';
    import type {
        ArtisanParam,
        FieldIssue,
        ScheduleFrequency,
        ScheduleSpec
    } from '@/types/bindings';

    export interface ArtisanSchedulePickerProps {
        modelValue: ScheduleSpec | null;
        params: ArtisanParam[];
        issues: FieldIssue[];
    }

    export interface ArtisanSchedulePickerEmits {
        (e: 'update:modelValue', value: ScheduleSpec | null): void;
    }

    const props = defineProps<ArtisanSchedulePickerProps>();
    const emit = defineEmits<ArtisanSchedulePickerEmits>();

    const FREQUENCY_OPTIONS: { kind: ScheduleFrequency['kind']; label: string }[] = [
        { kind: 'everyMinute', label: 'A cada minuto' },
        { kind: 'everyFiveMinutes', label: 'A cada 5 minutos' },
        { kind: 'everyFifteenMinutes', label: 'A cada 15 minutos' },
        { kind: 'everyThirtyMinutes', label: 'A cada 30 minutos' },
        { kind: 'hourly', label: 'A cada hora' },
        { kind: 'hourlyAt', label: 'A cada hora no minuto…' },
        { kind: 'daily', label: 'Diariamente à meia-noite' },
        { kind: 'dailyAt', label: 'Diariamente às…' },
        { kind: 'twiceDaily', label: 'Duas vezes ao dia' },
        { kind: 'weekly', label: 'Semanalmente' },
        { kind: 'weeklyOn', label: 'Semanalmente no dia/hora…' },
        { kind: 'monthly', label: 'Mensalmente' },
        { kind: 'monthlyOn', label: 'Mensalmente no dia/hora…' },
        { kind: 'cron', label: 'Expressão Cron personalizada' }
    ];

    const frequencySelectOptions = computed(() =>
        FREQUENCY_OPTIONS.map((opt) => ({ label: opt.label, value: opt.kind }))
    );

    const WEEKDAYS = [
        { value: 0, label: 'Domingo' },
        { value: 1, label: 'Segunda-feira' },
        { value: 2, label: 'Terça-feira' },
        { value: 3, label: 'Quarta-feira' },
        { value: 4, label: 'Quinta-feira' },
        { value: 5, label: 'Sexta-feira' },
        { value: 6, label: 'Sábado' }
    ];

    const weekdaySelectOptions = WEEKDAYS.map((d) => ({ label: d.label, value: d.value }));

    const argumentParams = computed(() =>
        props.params.filter((p) => p.kind === 'argument')
    );

    const scheduleIssues = computed(() =>
        props.issues.filter((i) => i.path.startsWith('schedule'))
    );

    const currentTimeValue = computed<string>(() => {
        const freq = props.modelValue?.frequency;
        if (!freq) {
            return '08:00';
        }
        if (freq.kind === 'dailyAt' || freq.kind === 'weeklyOn' || freq.kind === 'monthlyOn') {
            return freq.time;
        }
        return '08:00';
    });

    function defaultFrequencyForKind(kind: ScheduleFrequency['kind']): ScheduleFrequency {
        switch (kind) {
            case 'hourlyAt':
                return { kind: 'hourlyAt', minute: 0 };
            case 'dailyAt':
                return { kind: 'dailyAt', time: '08:00' };
            case 'twiceDaily':
                return { kind: 'twiceDaily', first: 1, second: 13 };
            case 'weeklyOn':
                return { kind: 'weeklyOn', day: 1, time: '08:00' };
            case 'monthlyOn':
                return { kind: 'monthlyOn', day: 1, time: '08:00' };
            case 'cron':
                return { kind: 'cron', expression: '0 * * * *' };
            default:
                return { kind };
        }
    }

    function onToggleSchedule(valOrEv: unknown): void {
        const enabled = typeof valOrEv === 'boolean' ? valOrEv : Boolean((valOrEv as { target?: HTMLInputElement })?.target?.checked);
        if (!enabled) {
            emit('update:modelValue', null);
            return;
        }
        emit('update:modelValue', {
            frequency: { kind: 'dailyAt', time: '08:00' },
            arguments: argumentParams.value.map(() => ''),
            options: [],
            withoutOverlapping: false,
            onOneServer: false,
            runInBackground: false
        });
    }

    function updateFrequency(frequency: ScheduleFrequency): void {
        if (!props.modelValue) {
            return;
        }
        emit('update:modelValue', {
            ...props.modelValue,
            frequency
        });
    }

    function onFrequencyKindChange(valOrEv: unknown): void {
        const kind = (typeof valOrEv === 'string' ? valOrEv : (valOrEv as { target?: HTMLSelectElement })?.target?.value) as ScheduleFrequency['kind'];
        if (kind) {
            updateFrequency(defaultFrequencyForKind(kind));
        }
    }

    function onMinuteNumberChange(valOrEv: unknown): void {
        const raw = typeof valOrEv === 'number' ? valOrEv : Number((valOrEv as { target?: HTMLInputElement })?.target?.value ?? valOrEv);
        const minute = Number.isNaN(raw) ? 0 : Math.min(59, Math.max(0, Math.round(raw)));
        updateFrequency({ kind: 'hourlyAt', minute });
    }

    function onTimeStringChange(valOrEv: unknown): void {
        const time = typeof valOrEv === 'string' ? valOrEv : String((valOrEv as { target?: HTMLInputElement })?.target?.value ?? '');
        const freq = props.modelValue?.frequency;
        if (!freq) {
            return;
        }
        if (freq.kind === 'dailyAt') {
            updateFrequency({ kind: 'dailyAt', time });
        } else if (freq.kind === 'weeklyOn') {
            updateFrequency({ kind: 'weeklyOn', day: freq.day, time });
        } else if (freq.kind === 'monthlyOn') {
            updateFrequency({ kind: 'monthlyOn', day: freq.day, time });
        }
    }

    function onWeekdaySelectChange(valOrEv: unknown): void {
        const raw = typeof valOrEv === 'number' ? valOrEv : Number((valOrEv as { target?: HTMLSelectElement })?.target?.value ?? valOrEv);
        const day = Number.isNaN(raw) ? 0 : raw;
        const freq = props.modelValue?.frequency;
        if (freq?.kind === 'weeklyOn') {
            updateFrequency({ kind: 'weeklyOn', day, time: freq.time });
        }
    }

    function onMonthDayNumberChange(valOrEv: unknown): void {
        const raw = typeof valOrEv === 'number' ? valOrEv : Number((valOrEv as { target?: HTMLInputElement })?.target?.value ?? valOrEv);
        const day = Number.isNaN(raw) ? 1 : Math.min(31, Math.max(1, Math.round(raw)));
        const freq = props.modelValue?.frequency;
        if (freq?.kind === 'monthlyOn') {
            updateFrequency({ kind: 'monthlyOn', day, time: freq.time });
        }
    }

    function onTwiceFirstNumberChange(valOrEv: unknown): void {
        const raw = typeof valOrEv === 'number' ? valOrEv : Number((valOrEv as { target?: HTMLInputElement })?.target?.value ?? valOrEv);
        const first = Number.isNaN(raw) ? 1 : Math.min(23, Math.max(0, Math.round(raw)));
        const freq = props.modelValue?.frequency;
        if (freq?.kind === 'twiceDaily') {
            updateFrequency({ kind: 'twiceDaily', first, second: freq.second });
        }
    }

    function onTwiceSecondNumberChange(valOrEv: unknown): void {
        const raw = typeof valOrEv === 'number' ? valOrEv : Number((valOrEv as { target?: HTMLInputElement })?.target?.value ?? valOrEv);
        const second = Number.isNaN(raw) ? 13 : Math.min(23, Math.max(0, Math.round(raw)));
        const freq = props.modelValue?.frequency;
        if (freq?.kind === 'twiceDaily') {
            updateFrequency({ kind: 'twiceDaily', first: freq.first, second });
        }
    }

    function onCronTextChange(valOrEv: unknown): void {
        const expression = typeof valOrEv === 'string' ? valOrEv : String((valOrEv as { target?: HTMLInputElement })?.target?.value ?? '');
        updateFrequency({ kind: 'cron', expression });
    }

    function onScheduleArgTextChange(index: number, valOrEv: unknown): void {
        if (!props.modelValue) {
            return;
        }
        const val = typeof valOrEv === 'string' ? valOrEv : String((valOrEv as { target?: HTMLInputElement })?.target?.value ?? '');
        const nextArgs = [...(props.modelValue.arguments ?? [])];
        while (nextArgs.length <= index) {
            nextArgs.push('');
        }
        nextArgs[index] = val;
        emit('update:modelValue', {
            ...props.modelValue,
            arguments: nextArgs
        });
    }

    function patchModifier(
        key: 'withoutOverlapping' | 'onOneServer' | 'runInBackground',
        valOrEv: unknown
    ): void {
        if (!props.modelValue) {
            return;
        }
        const checked = typeof valOrEv === 'boolean' ? valOrEv : Boolean((valOrEv as { target?: HTMLInputElement })?.target?.checked);
        emit('update:modelValue', {
            ...props.modelValue,
            [key]: checked
        });
    }
</script>

<style lang="scss" scoped>
    .schedule-picker {
        display: flex;
        flex-direction: column;
        gap: 0.625rem;
        padding: 0.625rem;
        border: 1px solid var(--background-700);
        border-radius: 0.375rem;
        background: var(--background-900);

        &__toggle {
            display: inline-flex;
            align-items: center;
            gap: 0.5rem;
            cursor: pointer;
        }

        &__toggle-label {
            font-size: 0.8125rem;
            font-weight: 600;
            color: var(--background-100);
        }

        &__body {
            display: flex;
            flex-direction: column;
            gap: 0.625rem;
        }

        &__row {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(9rem, 1fr));
            gap: 0.5rem;
        }

        &__field {
            display: flex;
            flex-direction: column;
            gap: 0.25rem;

            &--wide {
                grid-column: span 2;
            }
        }

        &__label {
            font-size: 0.6875rem;
            color: var(--background-300);
        }

        &__subtitle {
            font-size: 0.75rem;
            font-weight: 600;
            color: var(--background-200);
        }

        &__input,
        &__select {
            padding: 0.3125rem 0.5rem;
            border: 1px solid var(--background-700);
            border-radius: 0.25rem;
            background: var(--background-950);
            color: var(--background-50);
            font-size: 0.75rem;
            font-family: var(--mt-font-mono);
        }

        &__errors {
            display: flex;
            flex-direction: column;
            gap: 0.25rem;
        }

        &__error-item {
            margin: 0;
            font-size: 0.6875rem;
            color: var(--max-danger-400);
        }

        &__args {
            display: flex;
            flex-direction: column;
            gap: 0.375rem;
        }

        &__modifiers {
            display: flex;
            flex-wrap: wrap;
            gap: 0.75rem;
        }

        &__checkbox {
            display: inline-flex;
            align-items: center;
            gap: 0.375rem;
            font-size: 0.75rem;
            color: var(--background-200);
            cursor: pointer;
        }
    }
</style>
