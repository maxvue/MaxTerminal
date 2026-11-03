<template>
    <div class="tab-indicators">
        <AgentAttentionIndicator :tab-id="props.tab.id" />
        <!-- <AgentStatusLine :tab-id="props.tab.id" :compact="true" /> -->
        <WorktreeBadge :worktree="activeWorktree" />
        <GitIndicator :context="gitContext" @refresh="onRefreshGit" />
        <PortBadge :ports="tabPorts" :killing-by-port-id="portsStore.killingByPortId" @open-browser="onOpenBrowser" @kill-port="onKillPort" />
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import AgentAttentionIndicator from '@/components/agents/AgentAttentionIndicator.vue';
    import AgentStatusLine from '@/components/agents/AgentStatusLine.vue';
    import { useGitStore } from '@/stores/git';
    import { usePortsStore } from '@/stores/ports';
    import type {
        GitContext_Serialize,
        GitWorktree,
        ListeningPort_Serialize,
        TabView_Serialize
    } from '@/types/bindings';
    import GitIndicator from './GitIndicator.vue';
    import PortBadge from './PortBadge.vue';
    import WorktreeBadge from './WorktreeBadge.vue';

    export interface TabIndicatorsProps {
        tab: TabView_Serialize;
    }

    const props = defineProps<TabIndicatorsProps>();

    const gitStore = useGitStore();
    const portsStore = usePortsStore();

    const gitContext = computed<GitContext_Serialize | null>(() =>
        gitStore.contextForTab(props.tab.id)
    );

    const activeWorktree = computed<GitWorktree | null>(() => {
        const ctx = gitContext.value;
        if (ctx?.kind === 'repository') {
            return ctx.worktree;
        }
        return null;
    });

    const tabPorts = computed<ListeningPort_Serialize[]>(() =>
        portsStore.portsForTab(props.tab.id)
    );

    function onRefreshGit(path: string): void {
        void gitStore.refreshForPath(path, props.tab.activePaneId).catch(() => {
            // Erro já registrado na store.
        });
    }

    function onOpenBrowser(port: ListeningPort_Serialize): void {
        void portsStore.openInBrowser(port.paneId, port.port).catch(() => {
            // Erro já registrado na store.
        });
    }

    function onKillPort(port: ListeningPort_Serialize): void {
        void portsStore
            .killPort(props.tab.id, port.paneId, port.port, port.pid)
            .catch(() => {
                // Erro já registrado na store.
            });
    }
</script>

<style lang="scss" scoped>
    .tab-indicators {
        display: inline-flex;
        align-items: center;
        gap: 4px;
    }
</style>
