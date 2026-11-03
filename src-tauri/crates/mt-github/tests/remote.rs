//! Testes de integração de resolução de remotes GitHub (`mt-github::remote`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec
)]

use mt_github::model::RepoResolution;
use mt_github::ports::GitRemoteEntry;
use mt_github::remote::{ParsedRemoteUrl, parse_remote_url, resolve_from_remotes};

#[test]
fn parses_all_github_remote_formats_and_strips_dot_git() {
    let cases = [
        (
            "https://github.com/Johnattas/MaxTerminal.git",
            "Johnattas",
            "MaxTerminal",
        ),
        (
            "https://github.com/Johnattas/MaxTerminal",
            "Johnattas",
            "MaxTerminal",
        ),
        (
            "http://github.com/maxvue/max-components-ui.git",
            "maxvue",
            "max-components-ui",
        ),
        (
            "git@github.com:Johnattas/MaxTerminal.git",
            "Johnattas",
            "MaxTerminal",
        ),
        (
            "git@github.com:Johnattas/MaxTerminal",
            "Johnattas",
            "MaxTerminal",
        ),
        (
            "ssh://git@github.com/Johnattas/MaxTerminal.git",
            "Johnattas",
            "MaxTerminal",
        ),
        (
            "git://github.com/Johnattas/MaxTerminal.git",
            "Johnattas",
            "MaxTerminal",
        ),
    ];

    for (raw, expected_owner, expected_repo) in cases {
        let parsed = parse_remote_url(raw);
        assert_eq!(
            parsed,
            ParsedRemoteUrl::GitHub {
                owner: expected_owner.to_string(),
                repo: expected_repo.to_string(),
            },
            "Falhou para URL: {raw}"
        );
    }
}

#[test]
fn distinguishes_non_github_remotes_and_invalid_urls() {
    assert_eq!(
        parse_remote_url("git@gitlab.com:org/repo.git"),
        ParsedRemoteUrl::OtherHost {
            host: String::from("gitlab.com"),
        }
    );
    assert_eq!(
        parse_remote_url("https://bitbucket.org/org/repo.git"),
        ParsedRemoteUrl::OtherHost {
            host: String::from("bitbucket.org"),
        }
    );
    assert_eq!(parse_remote_url(""), ParsedRemoteUrl::Invalid);
    assert_eq!(
        parse_remote_url("https://github.com/apenas-um-segmento"),
        ParsedRemoteUrl::Invalid
    );
}

#[test]
fn resolves_remote_priority_origin_then_upstream_then_alphabetical() {
    let remotes = vec![
        GitRemoteEntry {
            name: String::from("fork"),
            url: String::from("git@github.com:other/forked.git"),
        },
        GitRemoteEntry {
            name: String::from("upstream"),
            url: String::from("git@github.com:MaxVue/Core.git"),
        },
        GitRemoteEntry {
            name: String::from("origin"),
            url: String::from("https://github.com/Johnattas/MaxTerminal.git"),
        },
    ];

    let resolved = resolve_from_remotes(Some(&remotes), Some(String::from("main")));
    match resolved {
        RepoResolution::GitHub { identity } => {
            assert_eq!(identity.repo_key, "github.com/johnattas/maxterminal");
            assert_eq!(identity.owner, "Johnattas");
            assert_eq!(identity.repo, "MaxTerminal");
            assert_eq!(identity.remote_name, "origin");
            assert_eq!(identity.default_branch.as_deref(), Some("main"));
        }
        other => panic!("Esperava GitHub, obteve {other:?}"),
    }

    assert_eq!(resolve_from_remotes(None, None), RepoResolution::NotGitRepo);
    assert_eq!(
        resolve_from_remotes(Some(&[]), None),
        RepoResolution::NoRemotes
    );
}
