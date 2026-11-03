//! Testes de `LinkResolver::join` e `LinkResolver::resolve` (`mt-os`).

#![allow(clippy::unwrap_used)]

use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use mt_core::error::MtResult;
use mt_core::ids::{PaneId, PtyId, TabId};
use mt_os::{LinkCandidate, LinkContext, LinkResolver, TabCwdProvider, codes, os_error};
use tempfile::TempDir;

struct MockCwdProvider {
    cwd: Option<PathBuf>,
    expected_epoch: AtomicU32,
    bump_after_first_call: bool,
}

impl TabCwdProvider for MockCwdProvider {
    fn cwd(&self, context: &LinkContext) -> MtResult<Option<PathBuf>> {
        let cur = self.expected_epoch.load(Ordering::Acquire);
        if context.cwd_epoch != cur {
            return Err(os_error(
                codes::LINK_CONTEXT_STALE,
                "Contexto de CWD obsoleto.",
            ));
        }
        if self.bump_after_first_call {
            self.expected_epoch.fetch_add(1, Ordering::AcqRel);
        }
        Ok(self.cwd.clone())
    }
}

fn sample_ctx() -> LinkContext {
    LinkContext {
        tab_id: TabId::new(),
        pane_id: PaneId::new(),
        pty_id: PtyId::new(),
        cwd_epoch: 1,
    }
}

#[test]
fn pure_join_expands_tilde_relative_and_rejects_urls() {
    let cwd = Path::new("/home/user/proj");
    let home = Path::new("/home/user");

    assert_eq!(
        LinkResolver::join(Some(cwd), home, "src/main.rs"),
        Some(PathBuf::from("/home/user/proj/src/main.rs"))
    );
    assert_eq!(
        LinkResolver::join(Some(cwd), home, "./a/../b/file.ts"),
        Some(PathBuf::from("/home/user/proj/b/file.ts"))
    );
    assert_eq!(
        LinkResolver::join(Some(cwd), home, "~/notes/todo.md"),
        Some(PathBuf::from("/home/user/notes/todo.md"))
    );
    assert_eq!(
        LinkResolver::join(None, home, "/etc/hosts"),
        Some(PathBuf::from("/etc/hosts"))
    );
    assert_eq!(LinkResolver::join(None, home, "relative/no_cwd.rs"), None);
    assert_eq!(LinkResolver::join(Some(cwd), home, "~other/file.txt"), None);
    assert_eq!(
        LinkResolver::join(Some(cwd), home, "https://example.com/a.rs"),
        None
    );
}

#[tokio::test]
async fn resolve_filters_regular_files_symlinks_permissions_and_rejects_oversized_batch() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let home_dir = root.join("home");
    let proj_dir = root.join("proj");
    std::fs::create_dir_all(proj_dir.join("src")).unwrap();
    std::fs::create_dir_all(&home_dir).unwrap();

    let main_rs = proj_dir.join("src/main.rs");
    std::fs::write(&main_rs, "fn main() {}\n").unwrap();

    let home_file = home_dir.join("config.toml");
    std::fs::write(&home_file, "key = 1\n").unwrap();

    let symlink_file = proj_dir.join("link_main.rs");
    symlink(&main_rs, &symlink_file).unwrap();

    let no_read_file = proj_dir.join("secret.txt");
    std::fs::write(&no_read_file, "no read").unwrap();
    std::fs::set_permissions(&no_read_file, std::fs::Permissions::from_mode(0o000)).unwrap();

    let provider = Arc::new(MockCwdProvider {
        cwd: Some(proj_dir.clone()),
        expected_epoch: AtomicU32::new(1),
        bump_after_first_call: false,
    });

    let resolver = LinkResolver::new(provider, home_dir);
    let ctx = sample_ctx();

    let candidates = vec![
        LinkCandidate {
            key: String::from("k1"),
            path: String::from("src/main.rs"),
            line: Some(42),
            column: Some(5),
        },
        LinkCandidate {
            key: String::from("k2"),
            path: String::from("~/config.toml"),
            line: Some(1),
            column: None,
        },
        LinkCandidate {
            key: String::from("k3"),
            path: String::from("./link_main.rs"),
            line: None,
            column: None,
        },
        LinkCandidate {
            key: String::from("k4"),
            path: String::from("secret.txt"),
            line: Some(10),
            column: None,
        },
        // Diretório -> deve ser ignorado
        LinkCandidate {
            key: String::from("k_dir"),
            path: String::from("src"),
            line: None,
            column: None,
        },
        // Inexistente -> ignorado
        LinkCandidate {
            key: String::from("k_missing"),
            path: String::from("src/missing.rs"),
            line: Some(1),
            column: None,
        },
        // Linha 0 -> inválido
        LinkCandidate {
            key: String::from("k_line0"),
            path: String::from("src/main.rs"),
            line: Some(0),
            column: None,
        },
        // Overflow (> 10^7) -> inválido
        LinkCandidate {
            key: String::from("k_overflow"),
            path: String::from("src/main.rs"),
            line: Some(10_000_001),
            column: None,
        },
    ];

    let items = resolver.resolve(&ctx, candidates).await.unwrap();
    // Restaura permissão para limpeza do TempDir
    let _ = std::fs::set_permissions(&no_read_file, std::fs::Permissions::from_mode(0o644));

    let keys: Vec<&str> = items.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(keys, vec!["k1", "k2", "k3", "k4"]);

    // Lote com 65 itens deve retornar os.link_rejected
    let oversized: Vec<LinkCandidate> = (0..65)
        .map(|i| LinkCandidate {
            key: format!("k{i}"),
            path: String::from("src/main.rs"),
            line: None,
            column: None,
        })
        .collect();
    let err = resolver.resolve(&ctx, oversized).await.unwrap_err();
    assert_eq!(err.code.0, codes::LINK_REJECTED);
}

#[tokio::test]
async fn resolve_detects_stale_cwd_epoch_during_io() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.txt");
    std::fs::write(&file, "ok").unwrap();

    let provider = Arc::new(MockCwdProvider {
        cwd: Some(tmp.path().to_path_buf()),
        expected_epoch: AtomicU32::new(1),
        bump_after_first_call: true,
    });

    let resolver = LinkResolver::new(provider, tmp.path().to_path_buf());
    let ctx = sample_ctx();
    let err = resolver
        .resolve(
            &ctx,
            vec![LinkCandidate {
                key: String::from("k1"),
                path: String::from("a.txt"),
                line: None,
                column: None,
            }],
        )
        .await
        .unwrap_err();
    assert_eq!(err.code.0, codes::LINK_CONTEXT_STALE);
}
