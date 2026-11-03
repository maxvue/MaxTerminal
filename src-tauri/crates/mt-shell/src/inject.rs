//! Injeção não intrusiva de scripts de integração OSC 133 para Bash, Zsh e Fish (Fase 05).
//!
//! Nunca modifica arquivos em `$HOME` do usuário. Todos os shims e scripts são gravados em
//! um diretório temporário privado por PTY (`0700` para diretórios, `0600` para arquivos)
//! sob `<runtime_dir>/shell-integration/<pty_id>`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use mt_core::shell::ShellKind;
use mt_pty::PreparedSpawn;
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::kind::{can_inject_shell_integration, detect_shell_kind};

const BASH_INTEGRATION_SCRIPT: &str = include_str!("../scripts/bash/mt-integration.bash");
const ZSH_ENV_SCRIPT: &str = include_str!("../scripts/zsh/.zshenv");
const ZSH_PROFILE_SCRIPT: &str = include_str!("../scripts/zsh/.zprofile");
const ZSH_RC_SCRIPT: &str = include_str!("../scripts/zsh/.zshrc");
const ZSH_LOGIN_SCRIPT: &str = include_str!("../scripts/zsh/.zlogin");
const ZSH_INTEGRATION_SCRIPT: &str = include_str!("../scripts/zsh/mt-integration.zsh");
const FISH_INTEGRATION_SCRIPT: &str = include_str!("../scripts/fish/mt-integration.fish");

/// Resultado da tentativa de injeção de scripts para um `ResolvedSpawn`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectionOutcome {
    /// Família de shell detectada.
    pub shell_kind: ShellKind,
    /// Indica se os scripts OSC 133 foram injetados com sucesso.
    pub injected: bool,
    /// Nonce aleatório CSPRNG de 128 bits (32 caracteres hex) gerado para autenticar os marcadores OSC 133.
    pub nonce: Option<String>,
    /// Diretório temporário criado para este PTY (removido no encerramento do PTY).
    pub runtime_subdir: Option<PathBuf>,
    /// Motivo caso a injeção tenha sido ignorada ou degradada para fallback `/proc`.
    pub skip_reason: Option<String>,
    /// Versão (hash SHA-256 curto) dos scripts embarcados.
    pub scripts_version: String,
}

/// Gera um nonce criptograficamente seguro de 128 bits (16 bytes -> 32 caracteres hexadecimais).
#[must_use]
pub fn generate_shell_nonce() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Calcula o hash SHA-256 curto (12 hex chars) do conjunto de scripts embarcados.
#[must_use]
pub fn embedded_scripts_version() -> String {
    let mut hasher = Sha256::new();
    hasher.update(BASH_INTEGRATION_SCRIPT.as_bytes());
    hasher.update(ZSH_ENV_SCRIPT.as_bytes());
    hasher.update(ZSH_PROFILE_SCRIPT.as_bytes());
    hasher.update(ZSH_RC_SCRIPT.as_bytes());
    hasher.update(ZSH_LOGIN_SCRIPT.as_bytes());
    hasher.update(ZSH_INTEGRATION_SCRIPT.as_bytes());
    hasher.update(FISH_INTEGRATION_SCRIPT.as_bytes());
    let digest = hasher.finalize();
    hex::encode(&digest[..6])
}

/// Prepara o diretório de integração e muta `PreparedSpawn` (argumentos e variáveis de ambiente)
/// sem tocar em nenhum dotfile do usuário em `$HOME`.
pub fn inject_for_spawn(
    spawn: &mut PreparedSpawn,
    runtime_base_dir: &Path,
    integration_enabled: bool,
) -> InjectionOutcome {
    let shell_kind = detect_shell_kind(&spawn.executable);
    let scripts_version = embedded_scripts_version();

    // Respeita opt-out explícito via request ou variável de ambiente
    let env_disabled = spawn
        .env
        .get("MAXTERMINAL_DISABLE_SHELL_INTEGRATION")
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));

    if !integration_enabled || env_disabled {
        return InjectionOutcome {
            shell_kind,
            injected: false,
            nonce: None,
            runtime_subdir: None,
            skip_reason: Some("integration_disabled".to_string()),
            scripts_version,
        };
    }

    if !can_inject_shell_integration(&shell_kind, &spawn.args) {
        return InjectionOutcome {
            shell_kind,
            injected: false,
            nonce: None,
            runtime_subdir: None,
            skip_reason: Some("non_interactive_or_custom_args".to_string()),
            scripts_version,
        };
    }

    let pty_dir = runtime_base_dir
        .join("shell-integration")
        .join(spawn.pty_id.to_string());

    if let Err(err) = create_secure_dir_all(&pty_dir) {
        tracing::warn!(
            pty_id = %spawn.pty_id,
            error = %err,
            "Falha ao criar diretório seguro de shell integration; degradando para fallback /proc"
        );
        return InjectionOutcome {
            shell_kind,
            injected: false,
            nonce: None,
            runtime_subdir: None,
            skip_reason: Some("scripts_unavailable".to_string()),
            scripts_version,
        };
    }

    let nonce = generate_shell_nonce();
    let is_login = spawn.args.iter().any(|a| a == "-l" || a == "--login");

    let write_res = match &shell_kind {
        ShellKind::Bash => setup_bash_injection(spawn, &pty_dir, is_login),
        ShellKind::Zsh => setup_zsh_injection(spawn, &pty_dir),
        ShellKind::Fish => setup_fish_injection(spawn, &pty_dir),
        ShellKind::Sh | ShellKind::Custom(_) => unreachable!(),
    };

    if let Err(err) = write_res {
        tracing::warn!(
            pty_id = %spawn.pty_id,
            error = %err,
            "Falha ao gravar scripts de shell integration; degradando para fallback /proc"
        );
        let _ = fs::remove_dir_all(&pty_dir);
        return InjectionOutcome {
            shell_kind,
            injected: false,
            nonce: None,
            runtime_subdir: None,
            skip_reason: Some("scripts_unavailable".to_string()),
            scripts_version,
        };
    }

    spawn
        .env
        .insert("MT_SHELL_NONCE".to_string(), nonce.clone());
    spawn.env.insert(
        "MT_SHELL_LOGIN".to_string(),
        if is_login { "1" } else { "0" }.to_string(),
    );
    spawn.env.insert(
        "MT_SHELL_RUNTIME_DIR".to_string(),
        pty_dir.to_string_lossy().into_owned(),
    );
    spawn
        .env
        .insert("WEZTERM_SHELL_SKIP_ALL".to_string(), "1".to_string());
    spawn
        .env
        .insert("TERM_PROGRAM".to_string(), "MaxTerminal".to_string());

    InjectionOutcome {
        shell_kind,
        injected: true,
        nonce: Some(nonce),
        runtime_subdir: Some(pty_dir),
        skip_reason: None,
        scripts_version,
    }
}

/// Remove o diretório temporário de integração associado a um PTY encerrado.
pub fn cleanup_injection_dir(runtime_base_dir: &Path, pty_id: mt_core::ids::PtyId) {
    let pty_dir = runtime_base_dir
        .join("shell-integration")
        .join(pty_id.to_string());
    if pty_dir.exists() {
        let _ = fs::remove_dir_all(&pty_dir);
    }
}

fn setup_bash_injection(
    spawn: &mut PreparedSpawn,
    pty_dir: &Path,
    _is_login: bool,
) -> std::io::Result<()> {
    let script_path = pty_dir.join("mt-integration.bash");
    write_secure_file(&script_path, BASH_INTEGRATION_SCRIPT.as_bytes())?;

    // Para permitir `--rcfile <script> -i`, remove `-l`/`--login` dos args (o próprio
    // `mt-integration.bash` lê `MT_SHELL_LOGIN=1` e carrega `/etc/profile` + `~/.bash_profile`
    // fielmente quando solicitado modo login).
    spawn.args.retain(|a| a != "-l" && a != "--login");

    let mut new_args = Vec::with_capacity(spawn.args.len() + 3);
    new_args.push("--rcfile".to_string());
    new_args.push(script_path.to_string_lossy().into_owned());
    if !spawn.args.iter().any(|a| a == "-i") {
        new_args.push("-i".to_string());
    }
    new_args.append(&mut spawn.args);
    spawn.args = new_args;

    Ok(())
}

fn setup_zsh_injection(spawn: &mut PreparedSpawn, pty_dir: &Path) -> std::io::Result<()> {
    write_secure_file(&pty_dir.join(".zshenv"), ZSH_ENV_SCRIPT.as_bytes())?;
    write_secure_file(&pty_dir.join(".zprofile"), ZSH_PROFILE_SCRIPT.as_bytes())?;
    write_secure_file(&pty_dir.join(".zshrc"), ZSH_RC_SCRIPT.as_bytes())?;
    write_secure_file(&pty_dir.join(".zlogin"), ZSH_LOGIN_SCRIPT.as_bytes())?;
    write_secure_file(
        &pty_dir.join("mt-integration.zsh"),
        ZSH_INTEGRATION_SCRIPT.as_bytes(),
    )?;

    if let Some(orig_zdotdir) = spawn
        .env
        .get("ZDOTDIR")
        .cloned()
        .or_else(|| std::env::var("ZDOTDIR").ok())
        && !orig_zdotdir.is_empty()
    {
        spawn
            .env
            .insert("MT_ORIG_ZDOTDIR".to_string(), orig_zdotdir);
    }

    let shim_str = pty_dir.to_string_lossy().into_owned();
    spawn
        .env
        .insert("MT_ZSH_SHIM_DIR".to_string(), shim_str.clone());
    spawn.env.insert("ZDOTDIR".to_string(), shim_str);

    Ok(())
}

fn setup_fish_injection(spawn: &mut PreparedSpawn, pty_dir: &Path) -> std::io::Result<()> {
    let script_path = pty_dir.join("mt-integration.fish");
    write_secure_file(&script_path, FISH_INTEGRATION_SCRIPT.as_bytes())?;

    let existing_features = spawn.env.get("fish_features").cloned().unwrap_or_default();
    let merged_features = if existing_features.trim().is_empty() {
        "no-query-term,no-mark-prompt".to_string()
    } else {
        format!("{existing_features},no-query-term,no-mark-prompt")
    };
    spawn
        .env
        .insert("fish_features".to_string(), merged_features);

    let escaped_path = script_path.to_string_lossy().replace('\'', "'\\''");
    spawn.args.push("--init-command".to_string());
    spawn.args.push(format!("source '{escaped_path}'"));

    Ok(())
}

fn create_secure_dir_all(dir: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder.create(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn write_secure_file(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(content)?;
    file.sync_all()?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::BTreeMap;

    use mt_core::ids::PtyId;
    use mt_pty::{PtyOwner, PtySize};

    use super::*;

    fn sample_resolved(program: &str) -> PreparedSpawn {
        PreparedSpawn {
            pty_id: PtyId::new(),
            executable: PathBuf::from(program),
            args: vec!["-l".to_string()],
            cwd: PathBuf::from("/tmp"),
            env: BTreeMap::new(),
            size: PtySize::default(),
            owner: PtyOwner::default(),
        }
    }

    #[test]
    fn injects_bash_zsh_and_fish_with_strict_permissions() {
        let tmp = tempfile::tempdir().unwrap();

        let mut bash_spawn = sample_resolved("/bin/bash");
        let bash_out = inject_for_spawn(&mut bash_spawn, tmp.path(), true);
        assert!(bash_out.injected);
        assert_eq!(bash_out.nonce.as_ref().unwrap().len(), 32);
        assert_eq!(bash_spawn.args[0], "--rcfile");
        let dir = bash_out.runtime_subdir.unwrap();
        let dir_mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
        let file_mode = fs::metadata(dir.join("mt-integration.bash"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);

        let mut zsh_spawn = sample_resolved("/bin/zsh");
        let zsh_out = inject_for_spawn(&mut zsh_spawn, tmp.path(), true);
        assert!(zsh_out.injected);
        assert!(zsh_spawn.env.contains_key("ZDOTDIR"));

        let mut fish_spawn = sample_resolved("/usr/bin/fish");
        let fish_out = inject_for_spawn(&mut fish_spawn, tmp.path(), true);
        assert!(fish_out.injected);
        assert!(fish_spawn.args.iter().any(|a| a == "--init-command"));
    }
}
