<template>
    <div class="schedule-picker" data-testid="artisan-schedule-picker">
        <label class="schedule-picker__toggle">
            <input type="checkbox" data-testid="schedule-enable-checkbox" :checked="modelValue !== null" @change="onToggleSchedule">
            <span class="schedule-picker__toggle-label">Agendamento (Schedule)</span>
        </label>

        <div v-if="modelValue !== null" class="schedule-picker__body">
            <div class="schedule-picker__row">
                <label class="schedule-picker__field">
                    <span class="schedule-picker__label">Frequência</span>
                    <select class="schedule-picker__select" data-testid="schedule-frequency-select" :value="modelValue.frequency.kind" @change="onFrequencyKindChange">
                        <option v-for="opt in FREQUENCY_OPTIONS" :key="opt.kind" :value="opt.kind">{{ opt.label }}</option>
                    </select>
                </label>

                <label v-if="modelValue.frequency.kind === 'hourlyAt'" class="schedule-picker__field">
                    <span class="schedule-picker__label">Minuto (0–59)</span>
                    <input type="number" min="0" max="59" class="schedule-picker__input" data-field-path="schedule.frequency.minute" :value="modelValue.frequency.minute" @input="onMinuteInput">
                </label>

                <label v-if="modelValue.frequency.kind === 'dailyAt' || modelValue.frequency.kind === 'weeklyOn' || modelValue.frequency.kind === 'monthlyOn'" class="schedule-picker__field">
                    <span class="schedule-picker__label">Horário (HH:MM)</span>
                    <input type="text" class="schedule-picker__input" data-field-path="schedule.frequency.time" placeholder="08:00" :value="currentTimeValue" @input="onTimeInput">
                </label>

                <label v-if="modelValue.frequency.kind === 'weeklyOn'" class="schedule-picker__field">
                    <span class="schedule-picker__label">Dia da semana</span>
                    <select class="schedule-picker__select" data-field-path="schedule.frequency.day" :value="modelValue.frequency.day" @change="onWeekdayChange">
                        <option v-for="d in WEEKDAYS" :key="d.value" :value="d.value">{{ d.label }}</option>
                    </select>
                </label>

                <label v-if="modelValue.frequency.kind === 'monthlyOn'" class="schedule-picker__field">
                    <span class="schedule-picker__label">Dia do mês (1–31)</span>
                    <input type="number" min="1" max="31" class="schedule-picker__input" data-field-path="schedule.frequency.day" :value="modelValue.frequency.day" @input="onMonthDayInput">
                </label>

                <template v-if="modelValue.frequency.kind === 'twiceDaily'">
                    <label class="schedule-picker__field">
                        <span class="schedule-picker__label">1ª hora (0–23)</span>
                        <input type="number" min="0" max="23" class="schedule-picker__input" data-field-path="schedule.frequency.first" :value="modelValue.frequency.first" @input="onTwiceFirstInput">
                    </label>
                    <label class="schedule-picker__field">
                        <span class="schedule-picker__label">2ª hora (0–23)</span>
                        <input type="number" min="0" max="23" class="schedule-picker__input" data-field-path="schedule.frequency.second" :value="modelValue.frequency.second" @input="onTwiceSecondInput">
                    </label>
                </template>

                <label v-if="modelValue.frequency.kind === 'cron'" class="schedule-picker__field schedule-picker__field--wide">
                    <span class="schedule-picker__label">Expressão Cron (5 campos)</span>
                    <input type="text" class="schedule-picker__input" data-testid="schedule-cron-input" data-field-path="schedule.frequency.expression" placeholder="0 8 * * 1" :value="modelValue.frequency.expression" @input="onCronInput">
                </label>
            </div>

            <div v-if="scheduleIssues.length > 0" class="schedule-picker__errors">
                <p v-for="(iss, idx) in scheduleIssues" :key="idx" class="schedule-picker__error-item">{{ iss.message }}</p>
            </div>

            <div v-if="argumentParams.length > 0" class="schedule-picker__args">
                <span class="schedule-picker__subtitle">Valores dos argumentos no agendamento</span>
                <div class="schedule-picker__row">
                    <label v-for="(arg, idx) in argumentParams" :key="arg.name || idx" class="schedule-picker__field">
                        <span class="schedule-picker__label">{{ arg.name || `Argumento ${idx + 1}` }}{{ arg.optional ? ' (opcional)' : ' *' }}</span>
                        <input type="text" class="schedule-picker__input" :data-field-path="`schedule.arguments[${idx}]`" :value="(modelValue.arguments ?? [])[idx] ?? ''" @input="(ev) => onScheduleArgInput(idx, ev)">
                    </label>
                </div>
            </div>

            <div class="schedule-picker__modifiers">
                <label class="schedule-picker__checkbox">
                    <input type="checkbox" :checked="modelValue.withoutOverlapping" @change="(ev) => patchModifier('withoutOverlapping', ev)">
                    <span>Sem sobreposição (withoutOverlapping)</span>
                </label>
                <label class="schedule-picker__checkbox">
                    <input type="checkbox" :checked="modelValue.onOneServer" @change="(ev) => patchModifier('onOneServer', ev)">
                    <span>Em um único servidor (onOneServer)</span>
                </label>
                <label class="schedule-picker__checkbox">
                    <input type="checkbox" :checked="modelValue.runInBackground" @change="(ev) => patchModifier('runInBackground', ev)">
                    <span>Em segundo plano (runInBackground)</span>
                </label>
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
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

    const WEEKDAYS = [
        { value: 0, label: 'Domingo' },
        { value: 1, label: 'Segunda-feira' },
        { value: 2, label: 'Terça-feira' },
        { value: 3, label: 'Quarta-feira' },
        { value: 4, label: 'Quinta-feira' },
        { value: 5, label: 'Sexta-feira' },
        { value: 6, label: 'Sábado' }
    ];

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

    function onToggleSchedule(ev: Event): void {
        const enabled = (ev.target as HTMLInputElement).checked;
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

    function onFrequencyKindChange(ev: Event): void {
        const kind = (ev.target as HTMLSelectElement).value as ScheduleFrequency['kind'];
        updateFrequency(defaultFrequencyForKind(kind));
    }

    function onMinuteInput(ev: Event): void {
        const minute = Number((ev.target as HTMLInputElement).value);
        updateFrequency({ kind: 'hourlyAt', minute: Number.isNaN(minute) ? 0 : minute });
    }

    function onTimeInput(ev: Event): void {
        const time = (ev.target as HTMLInputElement).value;
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

    function onWeekdayChange(ev: Event): void {
        const day = Number((ev.target as HTMLSelectElement).value);
        const freq = props.modelValue?.frequency;
        if (freq?.kind === 'weeklyOn') {
            updateFrequency({ kind: 'weeklyOn', day, time: freq.time });
        }
    }

    function onMonthDayInput(ev: Event): void {
        const day = Number((ev.target as HTMLInputElement).value);
        const freq = props.modelValue?.frequency;
        if (freq?.kind === 'monthlyOn') {
            updateFrequency({ kind: 'monthlyOn', day: Number.isNaN(day) ? 1 : day, time: freq.time });
        }
    }

    function onTwiceFirstInput(ev: Event): void {
        const first = Number((ev.target as HTMLInputElement).value);
        const freq = props.modelValue?.frequency;
        if (freq?.kind === 'twiceDaily') {
            updateFrequency({ kind: 'twiceDaily', first: Number.isNaN(first) ? 1 : first, second: freq.second });
        }
    }

    function onTwiceSecondInput(ev: Event): void {
        const second = Number((ev.target as HTMLInputElement).value);
        const freq = props.modelValue?.frequency;
        if (freq?.kind === 'twiceDaily') {
            updateFrequency({ kind: 'twiceDaily', first: freq.first, second: Number.isNaN(second) ? 13 : second });
        }
    }

    function onCronInput(ev: Event): void {
        const expression = (ev.target as HTMLInputElement).value;
        updateFrequency({ kind: 'cron', expression });
    }

    function onScheduleArgInput(index: number, ev: Event): void {
        if (!props.modelValue) {
            return;
        }
        const val = (ev.target as HTMLInputElement).value;
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
        ev: Event
    ): void {
        if (!props.modelValue) {
            return;
        }
        const checked = (ev.target as HTMLInputElement).checked;
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
