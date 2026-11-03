//! Política de variáveis de ambiente herdadas, validação e variáveis obrigatórias do `MaxTerminal`.

use std::collections::BTreeMap;
use std::ffi::OsString;

use mt_core::ids::PtyId;

use crate::error::{PtyError, PtyResult};
use crate::types::SpawnSpec;

/// Limite máximo de tamanho de um valor de variável de ambiente (128 KiB).
pub const MAX_ENV_VALUE_BYTES: usize = 128 * 1024;

/// Política de higienização do ambiente herdado do processo pai (extensível pela Fase 17 para `AppImage`).
pub trait EnvPolicy: Send + Sync + 'static {
    /// Modifica in-place o mapa de variáveis herdadas antes de aplicar `env_remove` e `env` do `SpawnSpec`.
    fn apply(&self, inherited: &mut BTreeMap<OsString, OsString>);
}

/// Variáveis exclusivas do runtime `AppImage` que nunca devem vazar para shells filhos (`CA-37`).
pub const APPIMAGE_DIRECT_VARS: &[&str] = &[
    "APPIMAGE",
    "APPDIR",
    "OWD",
    "ARGV0",
    "APPIMAGE_EXTRACT_AND_RUN",
];

/// Variáveis separadas por `:` que o runtime `AppImage` prefixa com `$APPDIR/...` (`CA-37`).
pub const APPIMAGE_COLON_PATH_VARS: &[&str] = &[
    "LD_LIBRARY_PATH",
    "PATH",
    "XDG_DATA_DIRS",
    "QT_PLUGIN_PATH",
    "PERL5LIB",
    "GSETTINGS_SCHEMA_DIR",
    "PYTHONPATH",
    "GI_TYPELIB_PATH",
    "GST_PLUGIN_SYSTEM_PATH",
    "GST_PLUGIN_SYSTEM_PATH_1_0",
];

/// Snapshot e higienizador de ambiente `AppImage` capturado no boot para garantir que
/// shells filhos nos PTYs recebam o ambiente limpo do host (`CA-37` — Fase 17).
#[derive(Debug, Clone, Default)]
pub struct AppImageEnvSnapshot {
    appdir: Option<String>,
    originals: BTreeMap<String, Option<String>>,
}

impl AppImageEnvSnapshot {
    /// Captura o snapshot a partir do ambiente do processo atual (`std::env::vars_os()`).
    #[must_use]
    pub fn from_current_process() -> Self {
        let map: BTreeMap<OsString, OsString> = std::env::vars_os().collect();
        Self::from_map(&map)
    }

    /// Constrói um snapshot a partir de um mapa de variáveis de ambiente.
    #[must_use]
    pub fn from_map(map: &BTreeMap<OsString, OsString>) -> Self {
        let appdir = map
            .get(std::ffi::OsStr::new("APPDIR"))
            .and_then(|v| v.to_str())
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);

        let mut originals = BTreeMap::new();
        for &var in APPIMAGE_COLON_PATH_VARS {
            let orig_key = format!("APPIMAGE_ORIGINAL_{var}");
            if let Some(orig_val) = map
                .get(std::ffi::OsStr::new(&orig_key))
                .and_then(|v| v.to_str())
            {
                originals.insert(var.to_string(), Some(orig_val.to_string()));
            }
        }

        Self { appdir, originals }
    }

    /// Higieniza um mapa `BTreeMap<String, String>` removendo variáveis e prefixos do `AppImage`.
    pub fn sanitize_child_env(&self, env: &mut BTreeMap<String, String>) {
        let effective_appdir = self
            .appdir
            .clone()
            .or_else(|| env.get("APPDIR").cloned())
            .filter(|s| !s.is_empty());

        for &var in APPIMAGE_DIRECT_VARS {
            env.remove(var);
        }

        for &var in APPIMAGE_COLON_PATH_VARS {
            let orig_key = format!("APPIMAGE_ORIGINAL_{var}");
            let inline_orig = env.remove(&orig_key);
            if let Some(restored) =
                inline_orig.or_else(|| self.originals.get(var).cloned().flatten())
            {
                if restored.is_empty() && var != "PATH" {
                    env.remove(var);
                } else {
                    env.insert(var.to_string(), restored);
                }
                continue;
            }

            if let Some(ref appdir) = effective_appdir
                && let Some(current_val) = env.get(var).cloned()
            {
                let filtered: Vec<&str> = current_val
                    .split(':')
                    .filter(|segment| !segment.is_empty() && !segment.starts_with(appdir.as_str()))
                    .collect();
                if filtered.is_empty() && var != "PATH" {
                    env.remove(var);
                } else {
                    env.insert(var.to_string(), filtered.join(":"));
                }
            }
        }

        // Remove quaisquer outras chaves APPIMAGE_ORIGINAL_* remanescentes
        env.retain(|k, _| !k.starts_with("APPIMAGE_ORIGINAL_"));
    }

    /// Higieniza diretamente um mapa `BTreeMap<OsString, OsString>`.
    pub fn sanitize_os_map(&self, inherited: &mut BTreeMap<OsString, OsString>) {
        let mut string_map = BTreeMap::new();
        for (k, v) in inherited.iter() {
            if let (Some(ks), Some(vs)) = (k.to_str(), v.to_str()) {
                string_map.insert(ks.to_string(), vs.to_string());
            }
        }
        self.sanitize_child_env(&mut string_map);
        inherited.clear();
        for (k, v) in string_map {
            inherited.insert(OsString::from(k), OsString::from(v));
        }
    }
}

/// Política padrão de ambiente herdado da Fase 02 + Fase 17 (`AppImageEnvSnapshot`).
#[derive(Debug, Default)]
pub struct DefaultEnvPolicy;

impl EnvPolicy for DefaultEnvPolicy {
    fn apply(&self, inherited: &mut BTreeMap<OsString, OsString>) {
        let snapshot = AppImageEnvSnapshot::from_map(inherited);
        snapshot.sanitize_os_map(inherited);

        inherited.retain(|k, _| {
            let Some(key_str) = k.to_str() else {
                return false;
            };
            if matches!(
                key_str,
                "MAXTERMINAL_PTY_ID" | "VTE_VERSION" | "TERMINAL_EMULATOR"
            ) {
                return false;
            }
            if key_str.starts_with("WEBKIT_") || key_str.starts_with("GIO_LAUNCHED_DESKTOP_FILE") {
                return false;
            }
            true
        });
    }
}

/// Valida a chave de uma variável de ambiente segundo `^[A-Za-z_][A-Za-z0-9_]*$`.
pub fn validate_env_key(key: &str) -> PtyResult<()> {
    let bytes = key.as_bytes();
    let Some((&first, rest)) = bytes.split_first() else {
        return Err(PtyError::EnvInvalid(
            "Chave de variável de ambiente vazia".to_string(),
        ));
    };

    if !(first.is_ascii_alphabetic() || first == b'_') {
        return Err(PtyError::EnvInvalid(format!("Chave inválida: {key}")));
    }

    for &b in rest {
        if !(b.is_ascii_alphanumeric() || b == b'_') {
            return Err(PtyError::EnvInvalid(format!("Chave inválida: {key}")));
        }
    }

    Ok(())
}

/// Valida o valor de uma variável de ambiente (sem `\0` e `<= 128 KiB`).
pub fn validate_env_value(key: &str, value: &str) -> PtyResult<()> {
    if value.contains('\0') {
        return Err(PtyError::EnvInvalid(format!(
            "Valor contém byte nulo para a chave: {key}"
        )));
    }
    if value.len() > MAX_ENV_VALUE_BYTES {
        return Err(PtyError::EnvInvalid(format!(
            "Valor excede {MAX_ENV_VALUE_BYTES} bytes para a chave: {key}"
        )));
    }
    Ok(())
}

/// Constrói o ambiente final a partir de um mapa herdado explícito.
pub fn build_environment_from_inherited(
    pty_id: PtyId,
    spec: &SpawnSpec,
    policy: &dyn EnvPolicy,
    mut inherited: BTreeMap<OsString, OsString>,
) -> PtyResult<BTreeMap<String, String>> {
    policy.apply(&mut inherited);

    let mut env = BTreeMap::new();
    for (k, v) in inherited {
        if let (Some(ks), Some(vs)) = (k.to_str(), v.to_str())
            && validate_env_key(ks).is_ok()
            && validate_env_value(ks, vs).is_ok()
        {
            env.insert(ks.to_string(), vs.to_string());
        }
    }

    for rem_key in &spec.env_remove {
        validate_env_key(rem_key)?;
        env.remove(rem_key);
    }

    for (k, v) in &spec.env {
        validate_env_key(k)?;
        validate_env_value(k, v)?;
        env.insert(k.clone(), v.clone());
    }

    if !env.contains_key("LANG") {
        env.insert("LANG".to_string(), "C.UTF-8".to_string());
    }

    env.insert("TERM".to_string(), "xterm-256color".to_string());
    env.insert("COLORTERM".to_string(), "truecolor".to_string());
    env.insert("TERM_PROGRAM".to_string(), "MaxTerminal".to_string());
    env.insert(
        "TERM_PROGRAM_VERSION".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    );
    env.insert("MAXTERMINAL".to_string(), "1".to_string());
    env.insert("MAXTERMINAL_PTY_ID".to_string(), pty_id.to_string());

    Ok(env)
}

/// Constrói o ambiente final herdando `std::env::vars_os()` do processo atual.
pub fn build_environment(
    pty_id: PtyId,
    spec: &SpawnSpec,
    policy: &dyn EnvPolicy,
) -> PtyResult<BTreeMap<String, String>> {
    let inherited: BTreeMap<OsString, OsString> = std::env::vars_os().collect();
    build_environment_from_inherited(pty_id, spec, policy, inherited)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn validates_env_keys_and_values() {
        assert!(validate_env_key("VALID_KEY_1").is_ok());
        assert!(validate_env_key("_LEADING_UNDERSCORE").is_ok());

        for bad_key in ["", "A=B", "1STARTS_WITH_DIGIT", "BAD-DASH", "A\0B", "A B"] {
            let err = validate_env_key(bad_key).unwrap_err();
            assert_eq!(err.code(), "pty.env_invalid");
        }

        assert!(validate_env_value("K", "valor normal").is_ok());
        assert_eq!(
            validate_env_value("K", "com\0nulo").unwrap_err().code(),
            "pty.env_invalid"
        );
        let huge = "x".repeat(MAX_ENV_VALUE_BYTES + 1);
        assert_eq!(
            validate_env_value("K", &huge).unwrap_err().code(),
            "pty.env_invalid"
        );
    }

    #[test]
    fn precedence_inherited_policy_remove_spec_and_forced_vars() {
        let pty_id = PtyId::new();
        let mut inherited = BTreeMap::new();
        inherited.insert(OsString::from("KEEP_ME"), OsString::from("1"));
        inherited.insert(OsString::from("REMOVE_ME"), OsString::from("old"));
        inherited.insert(OsString::from("OVERRIDE_ME"), OsString::from("old"));
        inherited.insert(
            OsString::from("WEBKIT_DISABLE_COMPOSITING_MODE"),
            OsString::from("1"),
        );
        inherited.insert(OsString::from("VTE_VERSION"), OsString::from("7000"));
        inherited.insert(OsString::from("TERM"), OsString::from("dumb"));

        let mut spec = SpawnSpec::default();
        spec.env_remove.push("REMOVE_ME".to_string());
        spec.env
            .insert("OVERRIDE_ME".to_string(), "new".to_string());
        spec.env.insert("TERM".to_string(), "vt100".to_string());

        let env =
            build_environment_from_inherited(pty_id, &spec, &DefaultEnvPolicy, inherited).unwrap();

        assert_eq!(env.get("KEEP_ME").map(String::as_str), Some("1"));
        assert!(!env.contains_key("REMOVE_ME"));
        assert!(!env.contains_key("WEBKIT_DISABLE_COMPOSITING_MODE"));
        assert!(!env.contains_key("VTE_VERSION"));
        assert_eq!(env.get("OVERRIDE_ME").map(String::as_str), Some("new"));
        assert_eq!(env.get("LANG").map(String::as_str), Some("C.UTF-8"));
        assert_eq!(env.get("TERM").map(String::as_str), Some("xterm-256color"));
        assert_eq!(env.get("COLORTERM").map(String::as_str), Some("truecolor"));
        assert_eq!(
            env.get("TERM_PROGRAM").map(String::as_str),
            Some("MaxTerminal")
        );
        assert_eq!(env.get("MAXTERMINAL").map(String::as_str), Some("1"));
        assert_eq!(
            env.get("MAXTERMINAL_PTY_ID").map(String::as_str),
            Some(pty_id.to_string().as_str())
        );
    }

    #[test]
    fn env_policy_appimage_snapshot_restores_originals_and_strips_appdir_prefixes() {
        // Caso 1: Restauração via APPIMAGE_ORIGINAL_*
        let mut env1 = BTreeMap::new();
        env1.insert(
            "APPIMAGE".to_string(),
            "/home/user/Applications/MaxTerminal.AppImage".to_string(),
        );
        env1.insert("APPDIR".to_string(), "/tmp/.mount_MaxTer123456".to_string());
        env1.insert("OWD".to_string(), "/home/user".to_string());
        env1.insert("ARGV0".to_string(), "MaxTerminal.AppImage".to_string());
        env1.insert("APPIMAGE_EXTRACT_AND_RUN".to_string(), "1".to_string());
        env1.insert(
            "LD_LIBRARY_PATH".to_string(),
            "/tmp/.mount_MaxTer123456/usr/lib:/opt/cuda/lib64".to_string(),
        );
        env1.insert(
            "APPIMAGE_ORIGINAL_LD_LIBRARY_PATH".to_string(),
            "/opt/cuda/lib64".to_string(),
        );
        env1.insert(
            "PATH".to_string(),
            "/tmp/.mount_MaxTer123456/usr/bin:/usr/local/bin:/usr/bin".to_string(),
        );
        env1.insert(
            "APPIMAGE_ORIGINAL_PATH".to_string(),
            "/usr/local/bin:/usr/bin".to_string(),
        );
        env1.insert(
            "SSH_AUTH_SOCK".to_string(),
            "/run/user/1000/ssh.sock".to_string(),
        );

        let snap1 = AppImageEnvSnapshot::default();
        snap1.sanitize_child_env(&mut env1);

        for k in APPIMAGE_DIRECT_VARS {
            assert!(!env1.contains_key(*k), "{k} deveria ter sido removido");
        }
        assert!(!env1.contains_key("APPIMAGE_ORIGINAL_LD_LIBRARY_PATH"));
        assert!(!env1.contains_key("APPIMAGE_ORIGINAL_PATH"));
        assert_eq!(
            env1.get("LD_LIBRARY_PATH").map(String::as_str),
            Some("/opt/cuda/lib64")
        );
        assert_eq!(
            env1.get("PATH").map(String::as_str),
            Some("/usr/local/bin:/usr/bin")
        );
        assert_eq!(
            env1.get("SSH_AUTH_SOCK").map(String::as_str),
            Some("/run/user/1000/ssh.sock")
        );

        // Caso 2: Remoção de prefixos $APPDIR quando APPIMAGE_ORIGINAL_* não está presente
        let mut env2 = BTreeMap::new();
        env2.insert("APPDIR".to_string(), "/tmp/.mount_MaxTer999".to_string());
        env2.insert(
            "LD_LIBRARY_PATH".to_string(),
            "/tmp/.mount_MaxTer999/usr/lib:/tmp/.mount_MaxTer999/lib".to_string(),
        );
        env2.insert(
            "XDG_DATA_DIRS".to_string(),
            "/tmp/.mount_MaxTer999/usr/share:/usr/local/share:/usr/share".to_string(),
        );
        env2.insert(
            "PATH".to_string(),
            "/tmp/.mount_MaxTer999/usr/bin:/home/user/.cargo/bin:/usr/bin".to_string(),
        );
        env2.insert("EDITOR".to_string(), "nvim".to_string());

        let snap2 = AppImageEnvSnapshot::default();
        snap2.sanitize_child_env(&mut env2);

        // Como todos os segmentos de LD_LIBRARY_PATH pertenciam ao APPDIR, a chave deve ser removida
        assert!(!env2.contains_key("LD_LIBRARY_PATH"));
        assert_eq!(
            env2.get("XDG_DATA_DIRS").map(String::as_str),
            Some("/usr/local/share:/usr/share")
        );
        assert_eq!(
            env2.get("PATH").map(String::as_str),
            Some("/home/user/.cargo/bin:/usr/bin")
        );
        assert_eq!(env2.get("EDITOR").map(String::as_str), Some("nvim"));
    }
}
