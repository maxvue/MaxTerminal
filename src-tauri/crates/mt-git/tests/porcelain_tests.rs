//! Testes unitários do parser `porcelain=v2` e `worktree list --porcelain` (`mt-git`).
#![allow(clippy::unwrap_used)]

use std::path::Path;

use mt_git::{GitBranch, WorktreeKind, parse_status_porcelain_v2, parse_worktree_list_porcelain};

#[test]
fn parses_named_branch_with_upstream_and_mixed_changes() {
    let raw = concat!(
        "# branch.oid 0123456789abcdef0123456789abcdef01234567\0",
        "# branch.head feat/indicators\0",
        "# branch.upstream origin/feat/indicators\0",
        "# branch.ab +3 -1\0",
        "1 M. N... 100644 100644 100644 aaaa bbbb src/main.rs\0",
        "1 .M N... 100644 100644 100644 aaaa bbbb src/lib.rs\0",
        "1 MM N... 100644 100644 100644 aaaa bbbb src/both.rs\0",
        "2 R. N... 100644 100644 100644 aaaa bbbb R100 src/new name.rs\0src/old name.rs\0",
        "u UU N... 100644 100644 100644 100644 a b c conflict.rs\0",
        "? untracked_1.txt\0",
        "? path with spaces/untracked_2.txt\0",
    );

    let parsed = parse_status_porcelain_v2(raw.as_bytes()).unwrap();
    assert_eq!(
        parsed.branch,
        GitBranch::Named {
            name: "feat/indicators".to_string(),
            oid: Some("01234567".to_string()),
        }
    );
    let up = parsed.upstream.unwrap();
    assert_eq!(up.name, "origin/feat/indicators");
    assert_eq!(up.ahead, 3);
    assert_eq!(up.behind, 1);

    // Staged: M. (1) + MM (1) + R. (1) = 3
    assert_eq!(parsed.changes.staged, 3);
    // Unstaged: .M (1) + MM (1) = 2
    assert_eq!(parsed.changes.unstaged, 2);
    assert_eq!(parsed.changes.conflicted, 1);
    assert_eq!(parsed.changes.untracked, 2);
    assert!(parsed.changes.dirty);
    assert!(!parsed.changes.summary_truncated);
}

#[test]
fn parses_unborn_branch_and_clean_tree() {
    let raw = "# branch.oid (initial)\0# branch.head main\0";
    let parsed = parse_status_porcelain_v2(raw.as_bytes()).unwrap();
    assert_eq!(
        parsed.branch,
        GitBranch::Unborn {
            name: "main".to_string(),
        }
    );
    assert!(!parsed.changes.dirty);
    assert_eq!(parsed.changes.staged, 0);
    assert_eq!(parsed.changes.unstaged, 0);
    assert_eq!(parsed.changes.untracked, 0);
    assert_eq!(parsed.changes.conflicted, 0);
}

#[test]
fn parses_detached_head() {
    let raw = "# branch.oid deadbeefcafebabe11223344556677889900aabb\0# branch.head (detached)\0";
    let parsed = parse_status_porcelain_v2(raw.as_bytes()).unwrap();
    assert_eq!(
        parsed.branch,
        GitBranch::Detached {
            oid: "deadbeef".to_string(),
        }
    );
    assert!(!parsed.changes.dirty);
}

#[test]
fn parses_worktree_list_porcelain_and_classifies_kinds() {
    let raw = concat!(
        "worktree /repo/project\0",
        "HEAD 1111222233334444555566667777888899990000\0",
        "branch refs/heads/main\0",
        "\0",
        "worktree /repo/project/.worktrees/wt-alpha\0",
        "HEAD aaaabbbbccccddddeeeeffff0000111122223333\0",
        "branch refs/heads/wt/wt-alpha\0",
        "\0",
        "worktree /repo/project/.max-code-worktrees/wt-8f33ab43\0",
        "HEAD 9999888877776666555544443333222211110000\0",
        "detached\0",
        "locked em uso pelo agente\0",
        "\0",
        "worktree /tmp/external-wt\0",
        "HEAD 1234567812345678123456781234567812345678\0",
        "branch refs/heads/ext\0",
        "prunable gitdir file points to non-existent location\0",
        "\0",
    );

    let list = parse_worktree_list_porcelain(raw.as_bytes(), Path::new("/repo/project")).unwrap();
    assert_eq!(list.len(), 4);

    assert!(list[0].is_main);
    assert_eq!(list[0].kind, WorktreeKind::Main);
    assert_eq!(list[0].branch.as_deref(), Some("main"));
    assert_eq!(list[0].head_oid, "11112222");

    assert!(!list[1].is_main);
    assert_eq!(list[1].kind, WorktreeKind::MaxTerminal);
    assert_eq!(list[1].name, "wt-alpha");
    assert_eq!(list[1].branch.as_deref(), Some("wt/wt-alpha"));

    assert!(!list[2].is_main);
    assert_eq!(list[2].kind, WorktreeKind::MaxCode);
    assert_eq!(list[2].name, "wt-8f33ab43");
    assert!(list[2].detached);
    assert_eq!(list[2].locked.as_deref(), Some("em uso pelo agente"));

    assert!(!list[3].is_main);
    assert_eq!(list[3].kind, WorktreeKind::External);
    assert!(list[3].prunable.is_some());
}
