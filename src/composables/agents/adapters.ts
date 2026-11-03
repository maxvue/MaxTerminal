import type {
    AttentionKind,
    AttentionSnapshot_Serialize,
    AttentionSource,
    ContextUsage_Serialize,
    Field_Serialize,
    MeasurementQuality,
    MetricState_Serialize,
    ModelInfo_Serialize,
    QuotaWindow_Serialize,
    TelemetrySourceKind
} from '@/types/bindings';

/** Tom semântico de barra de progresso da StatusLine (`--max-primary-500`, `--max-warning-500`, `--max-danger-500`). */
export type ProgressTone = 'primary' | 'warning' | 'danger' | 'neutral';

/** Projeção reativa individual de um campo de telemetria para renderização acessível na StatusLine. */
export interface TelemetryFieldProjection {
    display: string;
    label: string;
    isAvailable: boolean;
    isStale: boolean;
    isEstimated: boolean;
    isConfigured: boolean;
    ageSeconds: number | null;
    percent: number | null;
    tone: ProgressTone;
    quality: MeasurementQuality | null;
    qualityLabel: string | null;
    source: TelemetrySourceKind | null;
    sourceLabel: string | null;
    resetText: string | null;
    tooltip: string;
}

/** Rótulo legível em `pt-BR` para a qualidade da medição (`CA-34`). */
export function formatQualityLabel(quality: MeasurementQuality): string {
    switch (quality) {
        case 'reported':
            return 'Fonte estruturada';
        case 'derived':
            return 'Derivado da sessão';
        case 'estimated':
            return 'Estimado';
        case 'configured':
            return 'Modelo configurado';
    }
}

/** Rótulo legível em `pt-BR` para a fonte de telemetria (`CA-34`). */
export function formatTelemetrySourceLabel(source: TelemetrySourceKind): string {
    switch (source) {
        case 'claudeStatusLine':
            return 'Hook statusLine do Claude Code';
        case 'claudeTranscript':
            return 'Transcript JSONL do Claude Code';
        case 'codexSession':
            return 'Sessão JSONL do Codex';
        case 'outputHeuristic':
            return 'Heurística opt-in de saída do PTY';
        case 'configured':
            return 'Configuração da aba/template';
    }
}

/** Calcula o tom semântico de uso percentual (`< 70%` primary, `70..89%` warning, `>= 90%` danger). */
export function resolveProgressTone(percent: number | null): ProgressTone {
    if (percent === null || !Number.isFinite(percent)) {
        return 'neutral';
    }
    if (percent >= 90) {
        return 'danger';
    }
    if (percent >= 70) {
        return 'warning';
    }
    return 'primary';
}

/** Formata percentual finito (`0..=100`) em `pt-BR`, com prefixo `~` quando estimado. */
export function formatPercentDisplay(percent: number | null, estimated = false): string {
    if (percent === null || !Number.isFinite(percent) || percent < 0 || percent > 100) {
        return '—';
    }
    const rounded = Math.round(percent * 10) / 10;
    const text = Number.isInteger(rounded) ? `${rounded}%` : `${rounded.toFixed(1).replace('.', ',')}%`;
    return estimated ? `~${text}` : text;
}

/** Formata o tempo restante até o reset de uma janela de cota em `pt-BR`. */
export function formatRelativeReset(resetsAtMs: number | null, nowMs: number): string | null {
    if (resetsAtMs === null || !Number.isFinite(resetsAtMs)) {
        return null;
    }
    const diffMs = resetsAtMs - nowMs;
    if (diffMs <= 0) {
        return 'reiniciando';
    }
    const totalMinutes = Math.ceil(diffMs / 60_000);
    if (totalMinutes < 60) {
        return `reinicia em ${totalMinutes}min`;
    }
    const hours = Math.floor(totalMinutes / 60);
    const mins = totalMinutes % 60;
    if (hours < 24) {
        return mins > 0 ? `reinicia em ${hours}h ${mins}min` : `reinicia em ${hours}h`;
    }
    const days = Math.floor(hours / 24);
    const remHours = hours % 24;
    return remHours > 0 ? `reinicia em ${days}d ${remHours}h` : `reinicia em ${days}d`;
}

/** Formata a idade de observação em segundos de forma legível (`"Observado há Xs"`). */
export function formatObservedAge(ageSeconds: number): string {
    const safeSec = Math.max(0, Math.floor(ageSeconds));
    return `Observado há ${safeSec}s`;
}

function buildUnavailableProjection(
    label: string,
    metricState?: MetricState_Serialize | undefined
): TelemetryFieldProjection {
    const reason = metricState?.reason?.trim();
    return {
        display: '—',
        label,
        isAvailable: false,
        isStale: false,
        isEstimated: false,
        isConfigured: false,
        ageSeconds: null,
        percent: null,
        tone: 'neutral',
        quality: null,
        qualityLabel: null,
        source: null,
        sourceLabel: null,
        resetText: null,
        tooltip: reason && reason.length > 0 ? reason : 'Indisponível para este agente'
    };
}

function buildTooltipLines(
    title: string,
    field: Field_Serialize<unknown>,
    ageSeconds: number,
    isStale: boolean,
    extraLines: string[] = []
): string {
    const qLabel = formatQualityLabel(field.quality);
    const sLabel = formatTelemetrySourceLabel(field.source);
    const ageText = formatObservedAge(ageSeconds);
    const lines = [
        title,
        `Qualidade: ${qLabel}`,
        `Fonte: ${sLabel}`,
        isStale ? `${ageText} (desatualizado)` : ageText,
        ...extraLines
    ];
    return lines.join(' • ');
}

/** Constrói a projeção do campo `model` distinguindo `"Modelo ativo"` de `"Modelo configurado"` (`CA-19`, `CA-34`). */
export function projectModelField(
    field: Field_Serialize<ModelInfo_Serialize> | null | undefined,
    metricState: MetricState_Serialize | undefined,
    nowMs: number,
    staleAfterS = 60
): TelemetryFieldProjection {
    if (!field || !field.value.displayName.trim()) {
        return buildUnavailableProjection('Modelo', metricState);
    }
    const isConfigured = field.quality === 'configured';
    const isEstimated = field.quality === 'estimated';
    const label = isConfigured ? 'Modelo configurado' : 'Modelo ativo';
    const ageSeconds = Math.max(0, Math.floor((nowMs - field.observedAtMs) / 1000));
    const isStale = !isConfigured && ageSeconds > staleAfterS;
    const rawName = field.value.displayName.trim();
    const display = isEstimated ? `~${rawName}` : rawName;
    const extra: string[] = [];
    if (field.value.id && field.value.id !== rawName) {
        extra.push(`ID: ${field.value.id}`);
    }
    return {
        display,
        label,
        isAvailable: true,
        isStale,
        isEstimated,
        isConfigured,
        ageSeconds,
        percent: null,
        tone: 'neutral',
        quality: field.quality,
        qualityLabel: formatQualityLabel(field.quality),
        source: field.source,
        sourceLabel: formatTelemetrySourceLabel(field.source),
        resetText: null,
        tooltip: buildTooltipLines(`${label}: ${rawName}`, field, ageSeconds, isStale, extra)
    };
}

/** Constrói a projeção do campo `context` (`%` da janela de contexto — `CA-19`, `CA-34`). */
export function projectContextField(
    field: Field_Serialize<ContextUsage_Serialize> | null | undefined,
    metricState: MetricState_Serialize | undefined,
    nowMs: number,
    staleAfterS = 60
): TelemetryFieldProjection {
    if (
        !field ||
        field.value.percent === null ||
        !Number.isFinite(field.value.percent) ||
        field.value.percent < 0 ||
        field.value.percent > 100
    ) {
        return buildUnavailableProjection('Contexto', metricState);
    }
    const isEstimated = field.quality === 'estimated';
    const percent = field.value.percent;
    const display = formatPercentDisplay(percent, isEstimated);
    const ageSeconds = Math.max(0, Math.floor((nowMs - field.observedAtMs) / 1000));
    const isStale = ageSeconds > staleAfterS;
    const extra: string[] = [];
    if (field.value.usedTokens !== null && field.value.windowTokens !== null) {
        extra.push(
            `${field.value.usedTokens.toLocaleString('pt-BR')} / ${field.value.windowTokens.toLocaleString('pt-BR')} tokens`
        );
    }
    return {
        display,
        label: isEstimated ? 'Contexto (Estimado)' : 'Contexto',
        isAvailable: true,
        isStale,
        isEstimated,
        isConfigured: false,
        ageSeconds,
        percent,
        tone: resolveProgressTone(percent),
        quality: field.quality,
        qualityLabel: formatQualityLabel(field.quality),
        source: field.source,
        sourceLabel: formatTelemetrySourceLabel(field.source),
        resetText: null,
        tooltip: buildTooltipLines(`Janela de contexto: ${display}`, field, ageSeconds, isStale, extra)
    };
}

/** Constrói a projeção de uma janela de cota (`5h` ou `Semanal` — `CA-19`, `CA-34`). */
export function projectQuotaField(
    windowLabel: 'Cota 5h' | 'Cota semanal',
    field: Field_Serialize<QuotaWindow_Serialize> | null | undefined,
    metricState: MetricState_Serialize | undefined,
    nowMs: number,
    staleAfterS = 60
): TelemetryFieldProjection {
    if (
        !field ||
        field.value.usedPercent === null ||
        !Number.isFinite(field.value.usedPercent) ||
        field.value.usedPercent < 0 ||
        field.value.usedPercent > 100
    ) {
        return buildUnavailableProjection(windowLabel, metricState);
    }
    if (field.value.resetsAtMs !== null && field.value.resetsAtMs <= nowMs) {
        return buildUnavailableProjection(windowLabel, {
            availability: 'awaitingData',
            reason: 'Janela de cota reiniciada — aguardando nova medição',
            checkedAtMs: nowMs
        });
    }
    const isEstimated = field.quality === 'estimated';
    const percent = field.value.usedPercent;
    const display = formatPercentDisplay(percent, isEstimated);
    const ageSeconds = Math.max(0, Math.floor((nowMs - field.observedAtMs) / 1000));
    const isStale = ageSeconds > staleAfterS;
    const resetText = formatRelativeReset(field.value.resetsAtMs, nowMs);
    const extra: string[] = ['Escopo: conta/provedor'];
    if (resetText) {
        extra.push(resetText);
    }
    return {
        display,
        label: windowLabel,
        isAvailable: true,
        isStale,
        isEstimated,
        isConfigured: false,
        ageSeconds,
        percent,
        tone: resolveProgressTone(percent),
        quality: field.quality,
        qualityLabel: formatQualityLabel(field.quality),
        source: field.source,
        sourceLabel: formatTelemetrySourceLabel(field.source),
        resetText,
        tooltip: buildTooltipLines(`${windowLabel}: ${display}`, field, ageSeconds, isStale, extra)
    };
}

/** Constrói a projeção do campo `costUsd` (opcional). */
export function projectCostField(
    field: Field_Serialize<number | null> | null | undefined,
    metricState: MetricState_Serialize | undefined,
    nowMs: number,
    staleAfterS = 60
): TelemetryFieldProjection {
    if (!field || field.value === null || !Number.isFinite(field.value) || field.value < 0) {
        return buildUnavailableProjection('Custo', metricState);
    }
    const isEstimated = field.quality === 'estimated';
    const formatted = `US$ ${field.value.toFixed(2).replace('.', ',')}`;
    const display = isEstimated ? `~${formatted}` : formatted;
    const ageSeconds = Math.max(0, Math.floor((nowMs - field.observedAtMs) / 1000));
    const isStale = ageSeconds > staleAfterS;
    return {
        display,
        label: 'Custo',
        isAvailable: true,
        isStale,
        isEstimated,
        isConfigured: false,
        ageSeconds,
        percent: null,
        tone: 'neutral',
        quality: field.quality,
        qualityLabel: formatQualityLabel(field.quality),
        source: field.source,
        sourceLabel: formatTelemetrySourceLabel(field.source),
        resetText: null,
        tooltip: buildTooltipLines(`Custo da sessão: ${display}`, field, ageSeconds, isStale)
    };
}

/** Rótulo em `pt-BR` para a fonte de atenção (`AttentionSource`). */
export function formatAttentionSourceLabel(source: AttentionSource): string {
    switch (source) {
        case 'nativeHook':
            return 'Hook nativo estruturado';
        case 'codexNotify':
            return 'Notificação Codex CLI';
        case 'oscNotification':
            return 'Sinal OSC/BEL do terminal';
        case 'pattern':
            return 'Heurística de padrão + silêncio';
    }
}

/** Rótulo em `pt-BR` diferenciando espera confirmada (`Awaiting`) de indício heurístico (`Suspected`) (`CA-22`, `CA-23`). */
export function formatAttentionBadgeLabel(
    snapshot: AttentionSnapshot_Serialize | null | undefined
): string | null {
    if (!snapshot) {
        return null;
    }
    const st = snapshot.state;
    if (st.state === 'awaiting') {
        return st.kind === 'awaitingApproval' ? 'Aguardando aprovação' : 'Aguardando resposta';
    }
    if (st.state === 'suspected') {
        return 'Possivelmente aguardando intervenção';
    }
    return null;
}

/** Rótulo curto para `AttentionKind`. */
export function formatAttentionKindLabel(kind: AttentionKind): string {
    return kind === 'awaitingApproval' ? 'Aprovação de permissão' : 'Entrada de resposta';
}
