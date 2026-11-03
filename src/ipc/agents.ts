import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { call } from '@/ipc/core';
import {
    commands,
    type AckReason,
    type AgentCloseInspection_Serialize,
    type AgentInfo_Serialize,
    type AgentKind,
    type AgentsSettings_Deserialize,
    type AgentsSettings_Serialize,
    type AgentTabLink_Serialize,
    type AgentTabOpened_Serialize,
    type AgentTabOpenRequest_Deserialize,
    type AgentTelemetry_Serialize,
    type AgentTemplateOptions_Deserialize,
    type AgentTemplateOptions_Serialize,
    type AttentionSnapshot_Serialize,
    type ProjectId,
    type TabId,
    type TemplateId,
    type WorktreeProposal_Serialize,
    type WorktreeResolution
} from '@/types/bindings';

export const agentsIpc = {
    /** Lista os 4 agentes CLI suportados com metadados, instalação e capabilities (`agent_list`). */
    list: (): Promise<AgentInfo_Serialize[]> =>
        call('agent_list', () => commands.agentList()),

    /** Reexecuta a detecção de instalação/versão ignorando o cache (`agent_detect_refresh`). */
    detectRefresh: (): Promise<AgentInfo_Serialize[]> =>
        call('agent_detect_refresh', () => commands.agentDetectRefresh()),

    /** Retorna as configurações globais de agentes CLI (`agent_settings_get`). */
    settingsGet: (): Promise<AgentsSettings_Serialize> =>
        call('agent_settings_get', () => commands.agentSettingsGet()),

    /** Valida, persiste e publica as configurações de agentes CLI (`agent_settings_update`). */
    settingsUpdate: (settings: AgentsSettings_Deserialize): Promise<AgentsSettings_Serialize> =>
        call('agent_settings_update', () => commands.agentSettingsUpdate(settings)),

    /** Recarrega os descritores de agentes do diretório de configuração do usuário (`agent_descriptors_reload`). */
    descriptorsReload: (): Promise<AgentInfo_Serialize[]> =>
        call('agent_descriptors_reload', () => commands.agentDescriptorsReload()),

    /** Semeia de forma idempotente os 4 templates dedicados de agentes para o projeto (`agent_templates_ensure`). */
    templatesEnsure: (projectId: ProjectId): Promise<number> =>
        call('agent_templates_ensure', () => commands.agentTemplatesEnsure(projectId)),

    /** Obtém as opções específicas de agente configuradas para um template (`agent_template_options_get`). */
    templateOptionsGet: (templateId: TemplateId): Promise<AgentTemplateOptions_Serialize> =>
        call('agent_template_options_get', () => commands.agentTemplateOptionsGet(templateId)),

    /** Atualiza as opções específicas de agente de um template (`agent_template_options_set`). */
    templateOptionsSet: (
        templateId: TemplateId,
        options: AgentTemplateOptions_Deserialize
    ): Promise<AgentTemplateOptions_Serialize> =>
        call('agent_template_options_set', () => commands.agentTemplateOptionsSet(templateId, options)),

    /** Calcula uma proposta somente-leitura de criação de worktree isolada para o agente (`agent_worktree_propose`). */
    worktreePropose: (
        projectId: ProjectId,
        agent: AgentKind,
        hint: string | null = null
    ): Promise<WorktreeProposal_Serialize> =>
        call('agent_worktree_propose', () => commands.agentWorktreePropose(projectId, agent, hint)),

    /** Abre uma nova aba de Agente CLI no projeto com isolamento opcional em Git Worktree (`agent_tab_open`). */
    tabOpen: (request: AgentTabOpenRequest_Deserialize): Promise<AgentTabOpened_Serialize> =>
        call('agent_tab_open', () => commands.agentTabOpen(request)),

    /** Obtém o vínculo `agent_tab_links` associado a uma aba (`agent_tab_link_get`). */
    tabLinkGet: (tabId: TabId): Promise<AgentTabLink_Serialize | null> =>
        call('agent_tab_link_get', () => commands.agentTabLinkGet(tabId)),

    /** Executa inspeção Git e de processos para a worktree vinculada à aba (`agent_tab_close_check`). */
    tabCloseCheck: (tabId: TabId): Promise<AgentCloseInspection_Serialize | null> =>
        call('agent_tab_close_check', () => commands.agentTabCloseCheck(tabId)),

    /** Resolve uma worktree pendente pós-fechamento da aba validando `inspection_token` (`agent_worktree_resolve`). */
    worktreeResolve: (
        linkId: number,
        action: WorktreeResolution,
        inspectionToken: string
    ): Promise<AgentTabLink_Serialize> =>
        call('agent_worktree_resolve', () =>
            commands.agentWorktreeResolve(linkId, action, inspectionToken)
        ),

    /** Lista todas as worktrees de agentes fechadas e ainda pendentes de resolução (`agent_worktree_pending_list`). */
    worktreePendingList: (
        projectId: ProjectId | null = null
    ): Promise<AgentCloseInspection_Serialize[]> =>
        call('agent_worktree_pending_list', () => commands.agentWorktreePendingList(projectId)),

    /** Retorna o snapshot atual de telemetria (`StatusLine`) de uma aba (`agent_telemetry_get`). */
    telemetryGet: (tabId: TabId): Promise<AgentTelemetry_Serialize | null> =>
        call('agent_telemetry_get', () => commands.agentTelemetryGet(tabId)),

    /** Lista todos os snapshots ativos de telemetria (`agent_telemetry_list`). */
    telemetryList: (): Promise<AgentTelemetry_Serialize[]> =>
        call('agent_telemetry_list', () => commands.agentTelemetryList()),

    /** Retorna o snapshot atual de atenção correlacionada de uma aba (`agent_attention_get`). */
    attentionGet: (tabId: TabId): Promise<AttentionSnapshot_Serialize | null> =>
        call('agent_attention_get', () => commands.agentAttentionGet(tabId)),

    /** Reconhece visualmente um alerta de atenção sem resolver a espera real no agente (`agent_attention_ack`). */
    attentionAck: (
        tabId: TabId,
        runId: string,
        episode: number,
        reason: AckReason
    ): Promise<AttentionSnapshot_Serialize | null> =>
        call('agent_attention_ack', () =>
            commands.agentAttentionAck(tabId, runId, episode, reason)
        ),

    /** Escuta o evento global `agent://installations-changed`. */
    onInstallationsChanged: (cb: (agents: AgentInfo_Serialize[]) => void): Promise<UnlistenFn> =>
        listen<AgentInfo_Serialize[]>('agent://installations-changed', (ev) => cb(ev.payload)),

    /** Escuta o evento global `agent://settings-changed`. */
    onSettingsChanged: (cb: (settings: AgentsSettings_Serialize) => void): Promise<UnlistenFn> =>
        listen<AgentsSettings_Serialize>('agent://settings-changed', (ev) => cb(ev.payload)),

    /** Escuta o evento global `agent://telemetry`. */
    onTelemetry: (cb: (telemetry: AgentTelemetry_Serialize) => void): Promise<UnlistenFn> =>
        listen<AgentTelemetry_Serialize>('agent://telemetry', (ev) => cb(ev.payload)),

    /** Escuta o evento global `agent://attention`. */
    onAttention: (cb: (snapshot: AttentionSnapshot_Serialize) => void): Promise<UnlistenFn> =>
        listen<AttentionSnapshot_Serialize>('agent://attention', (ev) => cb(ev.payload)),

    /** Escuta o evento global `agent://link-changed`. */
    onLinkChanged: (cb: (link: AgentTabLink_Serialize) => void): Promise<UnlistenFn> =>
        listen<AgentTabLink_Serialize>('agent://link-changed', (ev) => cb(ev.payload)),

    /** Escuta o evento global `agent://worktree-close-pending`. */
    onWorktreeClosePending: (
        cb: (inspection: AgentCloseInspection_Serialize) => void
    ): Promise<UnlistenFn> =>
        listen<AgentCloseInspection_Serialize>('agent://worktree-close-pending', (ev) =>
            cb(ev.payload)
        )
};
