//! Teste de garantia de não-intrusão em `$HOME` (`injection_no_dotfile_changes.rs`).
//!
//! Verifica que `inject_for_spawn` para Bash, Zsh e Fish nunca cria, edita ou toca em
//! qualquer arquivo dentro do `$HOME` do usuário, e que os diretórios e scripts gerados
//! em `<runtime_dir>/shell-integration/<pty_id>` possuem permissões `0700` e `0600`.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use mt_core::ids::PtyId;
use mt_pty::{PreparedSpawn, PtyOwner, PtySize};
use mt_shell::{cleanup_injection_dir, inject_for_spawn};
use sha2::{Digest, Sha256};

fn snapshot_dir_hashes(dir: &Path) -> BTreeMap<PathBuf, String> {
    let mut map = BTreeMap::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return map;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            let bytes = fs::read(&path).unwrap();
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            map.insert(path, hex::encode(hasher.finalize()));
        } else if path.is_dir() {
            map.extend(snapshot_dir_hashes(&path));
        }
    }
    map
}

#[test]
fn injection_never_modifies_or_creates_files_in_home_and_enforces_0700_0600() {
    let fake_home = tempfile::tempdir().unwrap();
    let runtime_dir = tempfile::tempdir().unwrap();

    // Cria dotfiles do usuário com conteúdo prévio conhecido
    fs::write(fake_home.path().join(".bashrc"), "export MY_BASH_VAR=1\n").unwrap();
    fs::write(
        fake_home.path().join(".bash_profile"),
        "export MY_PROFILE_VAR=1\n",
    )
    .unwrap();
    fs::write(fake_home.path().join(".zshenv"), "export MY_ZSHENV=1\n").unwrap();
    fs::write(fake_home.path().join(".zshrc"), "export MY_ZSHRC=1\n").unwrap();
    let fish_cfg_dir = fake_home.path().join(".config/fish");
    fs::create_dir_all(&fish_cfg_dir).unwrap();
    fs::write(fish_cfg_dir.join("config.fish"), "set -gx MY_FISH_VAR 1\n").unwrap();

    let before_hashes = snapshot_dir_hashes(fake_home.path());
    assert_eq!(before_hashes.len(), 5);

    for bin in ["/bin/bash", "/bin/zsh", "/usr/bin/fish"] {
        let pty_id = PtyId::new();
        let mut env = BTreeMap::new();
        env.insert(
            "HOME".to_string(),
            fake_home.path().to_string_lossy().into_owned(),
        );

        let mut prepared = PreparedSpawn {
            pty_id,
            executable: PathBuf::from(bin),
            args: Vec::new(),
            cwd: fake_home.path().to_path_buf(),
            env,
            size: PtySize::default(),
            owner: PtyOwner::default(),
        };

        let outcome = inject_for_spawn(&mut prepared, runtime_dir.path(), true);
        assert!(outcome.injected, "Falhou ao injetar para {bin}");

        let pty_dir = outcome.runtime_subdir.unwrap();
        let dir_mode = fs::metadata(&pty_dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            dir_mode, 0o700,
            "Diretório de {bin} deve ter permissão 0700"
        );

        for entry in fs::read_dir(&pty_dir).unwrap().flatten() {
            let fmode = entry.metadata().unwrap().permissions().mode() & 0o777;
            assert_eq!(
                fmode,
                0o600,
                "Arquivo {:?} de {bin} deve ter permissão 0600",
                entry.path()
            );
        }

        cleanup_injection_dir(runtime_dir.path(), pty_id);
        assert!(!pty_dir.exists());
    }

    let after_hashes = snapshot_dir_hashes(fake_home.path());
    assert_eq!(
        before_hashes, after_hashes,
        "Dotfiles em $HOME não podem sofrer nenhuma alteração ou adição!"
    );
}
