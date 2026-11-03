import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from '@/ipc/core';
import {
    commands,
    type BugCyclePreview,
    type BugCycleRunState,
    type CanonicalLabelSpec,
    type CreateIssueInput,
    type CreateIssueWorktreeRequest,
    type GithubAuthChangedPayload,
    type GithubAuthStatus,
    type GithubBugCycleProgressEventPayload,
    type GithubIssuesChangedPayload,
    type GithubIssueUpdatedPayload,
    type GithubLabelsSyncedPayload,
    type GithubRateLimitPayload,
    type IssueCounts,
    type IssueDetail,
    type IssueFilter,
    type IssueListPage,
    type IssueSummary,
    type IssueWorktreeResolution,
    type LabelEnsureReport,
    type LabelUpdatePatch,
    type OpenIssueTabReceipt,
    type OpenIssueTabRequest,
    type ProjectId,
    type RepoLabel,
    type RepoResolution,
    type SendIssueToMaxCodeReceipt,
    type SendIssueToMaxCodeRequest,
    type StartBugCycleRequest
} from '@/types/bindings';

export const githubIpc = {
    repoResolve(projectId: ProjectId): Promise<RepoResolution> {
        return unwrap(commands.githubRepoResolve(projectId), 'github_repo_resolve');
    },
    authStatus(forceProbe: boolean | null = null): Promise<GithubAuthStatus> {
        return unwrap(commands.githubAuthStatus(forceProbe), 'github_auth_status');
    },
    authSetToken(token: string): Promise<GithubAuthStatus> {
        return unwrap(commands.githubAuthSetToken(token), 'github_auth_set_token');
    },
    authClearToken(): Promise<GithubAuthStatus> {
        return unwrap(commands.githubAuthClearToken(), 'github_auth_clear_token');
    },
    issuesList(
        projectId: ProjectId,
        filter: IssueFilter | null = null,
        _forceRefresh = false
    ): Promise<IssueListPage> {
        return unwrap(commands.githubIssuesList(projectId, filter), 'github_issues_list');
    },
    issueDetail(
        projectId: ProjectId,
        issueNumber: number,
        forceRefresh = false
    ): Promise<IssueDetail> {
        return unwrap(
            commands.githubIssueDetail(projectId, issueNumber, forceRefresh),
            'github_issue_detail'
        );
    },
    issueCounts(projectId: ProjectId): Promise<IssueCounts> {
        return unwrap(commands.githubIssueCounts(projectId), 'github_issue_counts');
    },
    refresh(projectId: ProjectId): Promise<IssueListPage> {
        return unwrap(commands.githubRefresh(projectId, null, true), 'github_refresh');
    },
    labelsCatalog(): Promise<CanonicalLabelSpec[]> {
        return unwrap(commands.githubLabelsCatalog(), 'github_labels_catalog');
    },
    repoLabels(projectId: ProjectId, forceRefresh = false): Promise<RepoLabel[]> {
        return unwrap(
            commands.githubRepoLabels(projectId, forceRefresh),
            'github_repo_labels'
        );
    },
    labelsEnsure(projectId: ProjectId, dryRun = false): Promise<LabelEnsureReport> {
        return unwrap(
            commands.githubLabelsEnsure(projectId, dryRun),
            'github_labels_ensure'
        );
    },
    issueCreate(input: CreateIssueInput): Promise<IssueSummary> {
        return unwrap(commands.githubIssueCreate(input), 'github_issue_create');
    },
    issueLabelsUpdate(patch: LabelUpdatePatch): Promise<IssueSummary> {
        return unwrap(commands.githubIssueLabelsUpdate(patch), 'github_issue_labels_update');
    },
    issueSendMaxcode(request: SendIssueToMaxCodeRequest): Promise<SendIssueToMaxCodeReceipt> {
        return unwrap(
            commands.githubIssueSendMaxcode(request),
            'github_issue_send_maxcode'
        );
    },
    issueWorktreeResolve(
        projectId: ProjectId,
        issueNumber: number
    ): Promise<IssueWorktreeResolution> {
        return unwrap(
            commands.githubIssueWorktreeResolve(projectId, issueNumber),
            'github_issue_worktree_resolve'
        );
    },
    issueWorktreeCreate(
        request: CreateIssueWorktreeRequest
    ): Promise<IssueWorktreeResolution> {
        return unwrap(
            commands.githubIssueWorktreeCreate(request),
            'github_issue_worktree_create'
        );
    },
    issueOpenTab(request: OpenIssueTabRequest): Promise<OpenIssueTabReceipt> {
        return unwrap(commands.githubIssueOpenTab(request), 'github_issue_open_tab');
    },
    bugCyclePreview(
        projectId: ProjectId,
        includeReplanAndRejected = true
    ): Promise<BugCyclePreview> {
        return unwrap(
            commands.githubBugCyclePreview(projectId, includeReplanAndRejected),
            'github_bug_cycle_preview'
        );
    },
    bugCycleStart(request: StartBugCycleRequest): Promise<BugCycleRunState> {
        return unwrap(commands.githubBugCycleStart(request), 'github_bug_cycle_start');
    },
    bugCycleStatus(
        projectId: ProjectId,
        _action: string | null = null
    ): Promise<BugCycleRunState | null> {
        return unwrap(commands.githubBugCycleStatus(projectId), 'github_bug_cycle_status');
    },
    onIssuesChanged(handler: (payload: GithubIssuesChangedPayload) => void): Promise<UnlistenFn> {
        return listen<GithubIssuesChangedPayload>('github:issues-changed', (e) =>
            handler(e.payload)
        );
    },
    onIssueUpdated(handler: (payload: GithubIssueUpdatedPayload) => void): Promise<UnlistenFn> {
        return listen<GithubIssueUpdatedPayload>('github:issue-updated', (e) =>
            handler(e.payload)
        );
    },
    onLabelsSynced(handler: (payload: GithubLabelsSyncedPayload) => void): Promise<UnlistenFn> {
        return listen<GithubLabelsSyncedPayload>('github:labels-synced', (e) =>
            handler(e.payload)
        );
    },
    onAuthChanged(handler: (payload: GithubAuthChangedPayload) => void): Promise<UnlistenFn> {
        return listen<GithubAuthChangedPayload>('github:auth-changed', (e) =>
            handler(e.payload)
        );
    },
    onBugCycleProgress(
        handler: (payload: GithubBugCycleProgressEventPayload) => void
    ): Promise<UnlistenFn> {
        return listen<GithubBugCycleProgressEventPayload>('github:bug-cycle-progress', (e) =>
            handler(e.payload)
        );
    },
    onRateLimit(handler: (payload: GithubRateLimitPayload) => void): Promise<UnlistenFn> {
        return listen<GithubRateLimitPayload>('github:rate-limit', (e) => handler(e.payload));
    }
};
