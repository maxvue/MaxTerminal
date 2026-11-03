-- Migration 0010 (Fase 16): Aba Fixa de GitHub Issues (Padrão MaxCode)

CREATE TABLE github_repos (
    repo_key          TEXT PRIMARY KEY,
    host              TEXT NOT NULL CHECK (host = 'github.com'),
    owner             TEXT NOT NULL,
    repo              TEXT NOT NULL,
    html_url          TEXT NOT NULL,
    default_branch    TEXT,
    open_issues_count INTEGER NOT NULL DEFAULT 0 CHECK (open_issues_count >= 0),
    created_at_ms     INTEGER NOT NULL,
    updated_at_ms     INTEGER NOT NULL
) STRICT;

CREATE TABLE github_project_repos (
    project_id      TEXT PRIMARY KEY
        REFERENCES projects(id) ON DELETE CASCADE,
    repo_key        TEXT
        REFERENCES github_repos(repo_key) ON DELETE SET NULL,
    remote_name     TEXT,
    raw_remote_url  TEXT,
    state           TEXT NOT NULL CHECK (state IN (
                        'git_hub', 'non_git_hub_remote', 'no_remotes', 'not_git_repo', 'error'
                    )),
    error_message   TEXT,
    resolved_at_ms  INTEGER NOT NULL
) STRICT;

CREATE INDEX idx_github_project_repos_repo_key
    ON github_project_repos(repo_key)
    WHERE repo_key IS NOT NULL;

CREATE TABLE github_sync_state (
    repo_key              TEXT PRIMARY KEY
        REFERENCES github_repos(repo_key) ON DELETE CASCADE,
    transport_used        TEXT CHECK (transport_used IN ('gh_cli', 'rest', 'cache')),
    last_sync_at_ms       INTEGER,
    last_success_at_ms    INTEGER,
    last_error_code       TEXT,
    last_error_message    TEXT,
    rate_limit_limit      INTEGER,
    rate_limit_remaining  INTEGER,
    rate_limit_used       INTEGER,
    rate_limit_reset_ms   INTEGER,
    retry_after_until_ms  INTEGER,
    labels_checked_at_ms  INTEGER,
    missing_labels_count  INTEGER NOT NULL DEFAULT 0 CHECK (missing_labels_count >= 0)
) STRICT;

CREATE TABLE github_issues (
    repo_key            TEXT NOT NULL
        REFERENCES github_repos(repo_key) ON DELETE CASCADE,
    issue_number        INTEGER NOT NULL CHECK (issue_number > 0),
    node_id             TEXT,
    title               TEXT NOT NULL,
    body_sanitized      TEXT NOT NULL DEFAULT '',
    state               TEXT NOT NULL CHECK (state IN ('open', 'closed')),
    state_reason        TEXT,
    author_login        TEXT NOT NULL,
    author_avatar_url   TEXT,
    assignees_json      TEXT NOT NULL DEFAULT '[]',
    milestone_title     TEXT,
    comments_count      INTEGER NOT NULL DEFAULT 0 CHECK (comments_count >= 0),
    html_url            TEXT NOT NULL,
    has_plan            INTEGER NOT NULL DEFAULT 0 CHECK (has_plan IN (0, 1)),
    plan_source         TEXT CHECK (plan_source IS NULL OR plan_source IN ('body', 'comment')),
    plan_comment_id     INTEGER,
    plan_markdown       TEXT,
    plan_updated_at_ms  INTEGER,
    created_at_ms       INTEGER NOT NULL,
    updated_at_ms       INTEGER NOT NULL,
    closed_at_ms        INTEGER,
    synced_at_ms        INTEGER NOT NULL,
    detail_synced_at_ms INTEGER,
    PRIMARY KEY (repo_key, issue_number)
) STRICT;

CREATE INDEX idx_github_issues_repo_state_updated
    ON github_issues(repo_key, state, updated_at_ms DESC);

CREATE INDEX idx_github_issues_repo_has_plan
    ON github_issues(repo_key, state, has_plan);

CREATE TABLE github_issue_comments (
    repo_key        TEXT NOT NULL,
    issue_number    INTEGER NOT NULL,
    comment_id      INTEGER NOT NULL,
    author_login    TEXT NOT NULL,
    author_avatar   TEXT,
    body_sanitized  TEXT NOT NULL,
    html_url        TEXT NOT NULL,
    created_at_ms   INTEGER NOT NULL,
    updated_at_ms   INTEGER NOT NULL,
    synced_at_ms    INTEGER NOT NULL,
    PRIMARY KEY (repo_key, comment_id),
    FOREIGN KEY (repo_key, issue_number)
        REFERENCES github_issues(repo_key, issue_number) ON DELETE CASCADE
) STRICT;

CREATE INDEX idx_github_comments_issue_created
    ON github_issue_comments(repo_key, issue_number, created_at_ms ASC);

CREATE TABLE github_labels (
    repo_key        TEXT NOT NULL
        REFERENCES github_repos(repo_key) ON DELETE CASCADE,
    name_lower      TEXT NOT NULL,
    name            TEXT NOT NULL,
    color_hex       TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    canonical_group TEXT CHECK (canonical_group IS NULL
                        OR canonical_group IN ('state', 'closure', 'category')),
    synced_at_ms    INTEGER NOT NULL,
    PRIMARY KEY (repo_key, name_lower)
) STRICT;

CREATE TABLE github_issue_labels (
    repo_key     TEXT NOT NULL,
    issue_number INTEGER NOT NULL,
    label_lower  TEXT NOT NULL,
    label_name   TEXT NOT NULL,
    PRIMARY KEY (repo_key, issue_number, label_lower),
    FOREIGN KEY (repo_key, issue_number)
        REFERENCES github_issues(repo_key, issue_number) ON DELETE CASCADE
) STRICT;

CREATE INDEX idx_github_issue_labels_by_label
    ON github_issue_labels(repo_key, label_lower);

CREATE TABLE github_http_cache (
    cache_key       TEXT PRIMARY KEY,
    repo_key        TEXT NOT NULL
        REFERENCES github_repos(repo_key) ON DELETE CASCADE,
    etag            TEXT,
    last_modified   TEXT,
    status_code     INTEGER NOT NULL,
    updated_at_ms   INTEGER NOT NULL
) STRICT;

CREATE INDEX idx_github_http_cache_repo
    ON github_http_cache(repo_key);

CREATE TABLE github_bug_cycles (
    cycle_id           TEXT PRIMARY KEY,
    project_id         TEXT NOT NULL
        REFERENCES projects(id) ON DELETE CASCADE,
    repo_key           TEXT NOT NULL
        REFERENCES github_repos(repo_key) ON DELETE CASCADE,
    status             TEXT NOT NULL CHECK (status IN (
                           'queued', 'running', 'paused', 'completed', 'cancelled', 'failed'
                       )),
    total_issues       INTEGER NOT NULL CHECK (total_issues >= 0),
    completed_issues   INTEGER NOT NULL DEFAULT 0 CHECK (completed_issues >= 0),
    failed_issues      INTEGER NOT NULL DEFAULT 0 CHECK (failed_issues >= 0),
    current_issue      INTEGER,
    current_stage      TEXT,
    issue_numbers_json TEXT NOT NULL,
    skipped_json       TEXT NOT NULL DEFAULT '[]',
    error_message      TEXT,
    started_at_ms      INTEGER NOT NULL,
    updated_at_ms      INTEGER NOT NULL,
    finished_at_ms     INTEGER
) STRICT;

CREATE UNIQUE INDEX idx_github_one_active_cycle
    ON github_bug_cycles(repo_key)
    WHERE status IN ('queued', 'running', 'paused');

CREATE TABLE github_mutations (
    client_mutation_id TEXT PRIMARY KEY,
    repo_key           TEXT NOT NULL
        REFERENCES github_repos(repo_key) ON DELETE CASCADE,
    kind               TEXT NOT NULL CHECK (kind IN ('create_issue', 'update_labels', 'ensure_labels')),
    response_json      TEXT NOT NULL,
    created_at_ms      INTEGER NOT NULL
) STRICT;

CREATE INDEX idx_github_mutations_created
    ON github_mutations(created_at_ms);
