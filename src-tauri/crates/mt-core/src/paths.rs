//! Resolução e criação segura dos diretórios XDG do `MaxTerminal`.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{MtError, MtResult, error::codes};

/// Nome do diretório da aplicação dentro das bases XDG.
pub const APP_DIR_NAME: &str = "maxterminal";

/// Abstração de leitura de variáveis de ambiente para permitir testes puros e determinísticos.
pub trait EnvSource {
    /// Obtém o valor de uma variável de ambiente, se definida.
    fn var(&self, key: &str) -> Option<String>;
}

/// Implementação real de `EnvSource` consultando `std::env::var`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemEnv;

impl EnvSource for SystemEnv {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

/// Diretórios resolvidos do `MaxTerminal` conforme a especificação XDG Base Directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPaths {
    /// `$XDG_CONFIG_HOME/maxterminal` ou `~/.config/maxterminal`.
    pub config_dir: PathBuf,
    /// `$XDG_DATA_HOME/maxterminal` ou `~/.local/share/maxterminal`.
    pub data_dir: PathBuf,
    /// `$XDG_STATE_HOME/maxterminal` ou `~/.local/state/maxterminal`.
    pub state_dir: PathBuf,
    /// `$XDG_CACHE_HOME/maxterminal` ou `~/.cache/maxterminal`.
    pub cache_dir: PathBuf,
    /// `$XDG_RUNTIME_DIR/maxterminal` quando `XDG_RUNTIME_DIR` é um caminho absoluto.
    pub runtime_dir: Option<PathBuf>,
    /// `state_dir/logs` para gravação dos arquivos rotativos de log.
    pub log_dir: PathBuf,
    /// `data_dir/maxterminal.db` (apenas exposto na Fase 01; criado somente na Fase 03).
    pub db_path: PathBuf,
}

impl AppPaths {
    /// Resolve todos os caminhos sem tocar no disco.
    ///
    /// - Se `MAXTERMINAL_HOME` estiver presente: exige que seja absoluto (caso seja vazio ou relativo,
    ///   retorna `core.invalid_argument`) e resolve `config`, `data`, `state`, `cache`, `state/logs` e
    ///   `data/maxterminal.db` dentro dele, com `runtime_dir = None`.
    /// - Caso contrário, avalia `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME`
    ///   e `XDG_RUNTIME_DIR`. Valores vazios ou relativos são ignorados conforme a especificação XDG.
    /// - `HOME` absoluto só é consultado se ao menos um dos 4 diretórios persistentes precisar de fallback.
    ///   Se um fallback for necessário e `HOME` estiver ausente ou relativo, retorna `core.paths_unresolved`.
    pub fn resolve(env: &impl EnvSource) -> MtResult<Self> {
        if let Some(override_raw) = env.var("MAXTERMINAL_HOME") {
            let trimmed = override_raw.trim();
            let base = PathBuf::from(trimmed);
            if trimmed.is_empty() || !base.is_absolute() {
                return Err(MtError::invalid_argument(
                    "MAXTERMINAL_HOME",
                    "O caminho em MAXTERMINAL_HOME deve ser absoluto.",
                ));
            }
            let config_dir = base.join("config");
            let data_dir = base.join("data");
            let state_dir = base.join("state");
            let cache_dir = base.join("cache");
            let log_dir = state_dir.join("logs");
            let db_path = data_dir.join("maxterminal.db");

            return Ok(Self {
                config_dir,
                data_dir,
                state_dir,
                cache_dir,
                runtime_dir: None,
                log_dir,
                db_path,
            });
        }

        let xdg_config = Self::parse_abs_dir(env.var("XDG_CONFIG_HOME"));
        let xdg_data = Self::parse_abs_dir(env.var("XDG_DATA_HOME"));
        let xdg_state = Self::parse_abs_dir(env.var("XDG_STATE_HOME"));
        let xdg_cache = Self::parse_abs_dir(env.var("XDG_CACHE_HOME"));
        let xdg_runtime = Self::parse_abs_dir(env.var("XDG_RUNTIME_DIR"));

        let needs_home = xdg_config.is_none()
            || xdg_data.is_none()
            || xdg_state.is_none()
            || xdg_cache.is_none();

        let home_dir = if needs_home {
            Some(Self::parse_abs_dir(env.var("HOME")).ok_or_else(|| {
                MtError::new(
                    codes::PATHS_UNRESOLVED,
                    "Não foi possível determinar o diretório HOME absoluto do usuário.",
                )
            })?)
        } else {
            None
        };

        let resolve_with_fallback = |xdg: Option<PathBuf>, rel_fallback: &str| -> PathBuf {
            if let Some(dir) = xdg {
                dir.join(APP_DIR_NAME)
            } else if let Some(ref home) = home_dir {
                home.join(rel_fallback).join(APP_DIR_NAME)
            } else {
                unreachable!("home_dir validado quando needs_home é verdadeiro")
            }
        };

        let config_dir = resolve_with_fallback(xdg_config, ".config");
        let data_dir = resolve_with_fallback(xdg_data, ".local/share");
        let state_dir = resolve_with_fallback(xdg_state, ".local/state");
        let cache_dir = resolve_with_fallback(xdg_cache, ".cache");
        let runtime_dir = xdg_runtime.map(|dir| dir.join(APP_DIR_NAME));
        let log_dir = state_dir.join("logs");
        let db_path = data_dir.join("maxterminal.db");

        Ok(Self {
            config_dir,
            data_dir,
            state_dir,
            cache_dir,
            runtime_dir,
            log_dir,
            db_path,
        })
    }

    /// Cria os diretórios persistentes (`config_dir`, `data_dir`, `state_dir`, `cache_dir`, `log_dir`)
    /// garantindo permissão `0o700` no Unix. Não cria `runtime_dir` nem o arquivo `db_path`.
    pub fn ensure_dirs(&self) -> MtResult<()> {
        let dirs = [
            &self.config_dir,
            &self.data_dir,
            &self.state_dir,
            &self.cache_dir,
            &self.log_dir,
        ];
        for dir in dirs {
            Self::ensure_single_dir(dir)?;
        }
        Ok(())
    }

    fn parse_abs_dir(val: Option<String>) -> Option<PathBuf> {
        let raw = val?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let path = PathBuf::from(trimmed);
        if path.is_absolute() { Some(path) } else { None }
    }

    fn ensure_single_dir(dir: &Path) -> MtResult<()> {
        if dir.exists() {
            let meta = fs::metadata(dir)?;
            if !meta.is_dir() {
                return Err(MtError::new(
                    codes::IO,
                    "O caminho configurado para diretório da aplicação não é uma pasta válida.",
                ));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = meta.permissions();
                if perms.mode() & 0o777 != 0o700 {
                    perms.set_mode(0o700);
                    fs::set_permissions(dir, perms)?;
                }
            }
            return Ok(());
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true).mode(0o700);
            builder.create(dir)?;
        }
        #[cfg(not(unix))]
        {
            fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

/// DTO serializável exposto ao frontend contendo os caminhos resolvidos da aplicação.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppPathsDto {
    /// Caminho do diretório de configuração.
    pub config_dir: String,
    /// Caminho do diretório de dados.
    pub data_dir: String,
    /// Caminho do diretório de estado.
    pub state_dir: String,
    /// Caminho do diretório de cache.
    pub cache_dir: String,
    /// Caminho do diretório de logs.
    pub log_dir: String,
    /// Caminho onde o banco SQLite será armazenado (Fase 03).
    pub db_path: String,
}

impl From<&AppPaths> for AppPathsDto {
    fn from(paths: &AppPaths) -> Self {
        Self {
            config_dir: paths.config_dir.to_string_lossy().into_owned(),
            data_dir: paths.data_dir.to_string_lossy().into_owned(),
            state_dir: paths.state_dir.to_string_lossy().into_owned(),
            cache_dir: paths.cache_dir.to_string_lossy().into_owned(),
            log_dir: paths.log_dir.to_string_lossy().into_owned(),
            db_path: paths.db_path.to_string_lossy().into_owned(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeEnv {
        vars: HashMap<String, String>,
    }

    impl FakeEnv {
        fn new() -> Self {
            Self::default()
        }

        fn set(mut self, key: &str, value: &str) -> Self {
            self.vars.insert(key.to_string(), value.to_string());
            self
        }
    }

    impl EnvSource for FakeEnv {
        fn var(&self, key: &str) -> Option<String> {
            self.vars.get(key).cloned()
        }
    }

    #[test]
    fn resolves_xdg_defaults_from_home() {
        let env = FakeEnv::new().set("HOME", "/home/tester");
        let paths =
            AppPaths::resolve(&env).unwrap_or_else(|e| panic!("Falha ao resolver caminhos: {e}"));

        assert_eq!(
            paths.config_dir,
            PathBuf::from("/home/tester/.config/maxterminal")
        );
        assert_eq!(
            paths.data_dir,
            PathBuf::from("/home/tester/.local/share/maxterminal")
        );
        assert_eq!(
            paths.state_dir,
            PathBuf::from("/home/tester/.local/state/maxterminal")
        );
        assert_eq!(
            paths.cache_dir,
            PathBuf::from("/home/tester/.cache/maxterminal")
        );
        assert_eq!(paths.runtime_dir, None);
        assert_eq!(
            paths.log_dir,
            PathBuf::from("/home/tester/.local/state/maxterminal/logs")
        );
        assert_eq!(
            paths.db_path,
            PathBuf::from("/home/tester/.local/share/maxterminal/maxterminal.db")
        );

        let dto = AppPathsDto::from(&paths);
        assert_eq!(dto.config_dir, "/home/tester/.config/maxterminal");
        assert_eq!(
            dto.db_path,
            "/home/tester/.local/share/maxterminal/maxterminal.db"
        );
    }

    #[test]
    fn resolves_explicit_xdg_without_home() {
        let env = FakeEnv::new()
            .set("XDG_CONFIG_HOME", "/custom/config")
            .set("XDG_DATA_HOME", "/custom/data")
            .set("XDG_STATE_HOME", "/custom/state")
            .set("XDG_CACHE_HOME", "/custom/cache")
            .set("XDG_RUNTIME_DIR", "/custom/runtime");

        let paths = AppPaths::resolve(&env)
            .unwrap_or_else(|e| panic!("Falha ao resolver caminhos explicitos: {e}"));

        assert_eq!(
            paths.config_dir,
            PathBuf::from("/custom/config/maxterminal")
        );
        assert_eq!(paths.data_dir, PathBuf::from("/custom/data/maxterminal"));
        assert_eq!(paths.state_dir, PathBuf::from("/custom/state/maxterminal"));
        assert_eq!(paths.cache_dir, PathBuf::from("/custom/cache/maxterminal"));
        assert_eq!(
            paths.runtime_dir,
            Some(PathBuf::from("/custom/runtime/maxterminal"))
        );
        assert_eq!(
            paths.log_dir,
            PathBuf::from("/custom/state/maxterminal/logs")
        );
        assert_eq!(
            paths.db_path,
            PathBuf::from("/custom/data/maxterminal/maxterminal.db")
        );
    }

    #[test]
    fn ignores_relative_or_empty_xdg_and_fails_without_home() {
        // XDG relativo deve ser ignorado e fallback para HOME usado
        let env_with_home = FakeEnv::new()
            .set("XDG_CONFIG_HOME", "caminho/relativo")
            .set("XDG_DATA_HOME", "   ")
            .set("HOME", "/home/fallback");

        let paths = AppPaths::resolve(&env_with_home)
            .unwrap_or_else(|e| panic!("Falha ao resolver com fallback: {e}"));
        assert_eq!(
            paths.config_dir,
            PathBuf::from("/home/fallback/.config/maxterminal")
        );
        assert_eq!(
            paths.data_dir,
            PathBuf::from("/home/fallback/.local/share/maxterminal")
        );

        // Se HOME estiver ausente ou for relativo, retorna core.paths_unresolved
        let env_no_home = FakeEnv::new().set("XDG_CONFIG_HOME", "caminho/relativo");
        let err_no_home = AppPaths::resolve(&env_no_home).unwrap_err();
        assert_eq!(err_no_home.code, codes::PATHS_UNRESOLVED);

        let env_rel_home = FakeEnv::new().set("HOME", "relativo/sem/barra");
        let err_rel_home = AppPaths::resolve(&env_rel_home).unwrap_err();
        assert_eq!(err_rel_home.code, codes::PATHS_UNRESOLVED);
    }

    #[test]
    fn resolves_and_validates_maxterminal_home_override() {
        let env = FakeEnv::new().set("MAXTERMINAL_HOME", "/tmp/mt-override");
        let paths =
            AppPaths::resolve(&env).unwrap_or_else(|e| panic!("Falha ao resolver override: {e}"));

        assert_eq!(paths.config_dir, PathBuf::from("/tmp/mt-override/config"));
        assert_eq!(paths.data_dir, PathBuf::from("/tmp/mt-override/data"));
        assert_eq!(paths.state_dir, PathBuf::from("/tmp/mt-override/state"));
        assert_eq!(paths.cache_dir, PathBuf::from("/tmp/mt-override/cache"));
        assert_eq!(paths.runtime_dir, None);
        assert_eq!(paths.log_dir, PathBuf::from("/tmp/mt-override/state/logs"));
        assert_eq!(
            paths.db_path,
            PathBuf::from("/tmp/mt-override/data/maxterminal.db")
        );

        // Caminho relativo deve falhar com core.invalid_argument
        let env_rel = FakeEnv::new().set("MAXTERMINAL_HOME", "relativo/teste");
        let err_rel = AppPaths::resolve(&env_rel).unwrap_err();
        assert_eq!(err_rel.code, codes::INVALID_ARGUMENT);

        // Caminho vazio deve falhar com core.invalid_argument
        let env_empty = FakeEnv::new().set("MAXTERMINAL_HOME", "   ");
        let err_empty = AppPaths::resolve(&env_empty).unwrap_err();
        assert_eq!(err_empty.code, codes::INVALID_ARGUMENT);
    }

    #[test]
    fn ensure_dirs_creates_0700_dirs_and_never_creates_db_file() {
        let unique_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let base_tmp = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!("mt_test_dirs_{unique_id}"));

        let env = FakeEnv::new().set("MAXTERMINAL_HOME", base_tmp.to_string_lossy().as_ref());
        let paths =
            AppPaths::resolve(&env).unwrap_or_else(|e| panic!("Falha ao resolver caminhos: {e}"));

        // Garante a criação dos diretórios
        paths
            .ensure_dirs()
            .unwrap_or_else(|e| panic!("Falha em ensure_dirs: {e}"));

        // Verifica existência dos 5 diretórios
        let dirs = [
            &paths.config_dir,
            &paths.data_dir,
            &paths.state_dir,
            &paths.cache_dir,
            &paths.log_dir,
        ];

        for d in dirs {
            assert!(d.exists(), "Diretório deve existir: {d:?}");
            assert!(d.is_dir(), "Caminho deve ser diretório: {d:?}");

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let meta =
                    fs::metadata(d).unwrap_or_else(|e| panic!("Metadata falhou para {d:?}: {e}"));
                let mode = meta.permissions().mode() & 0o777;
                assert_eq!(mode, 0o700, "Permissões de {d:?} devem ser 0o700");
            }
        }

        // Verifica estritamente que db_path NÃO foi criado
        assert!(
            !paths.db_path.exists(),
            "Arquivo db_path NUNCA deve ser criado na Fase 01!"
        );

        // Chamada repetida deve ser idempotente
        paths
            .ensure_dirs()
            .unwrap_or_else(|e| panic!("Segunda chamada a ensure_dirs falhou: {e}"));

        // Limpeza do teste
        let _ = fs::remove_dir_all(&base_tmp);
    }
}
