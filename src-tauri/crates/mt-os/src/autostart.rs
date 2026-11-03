//! Sincronização do arquivo `.desktop` de inicialização automática (`AutostartSync` e `AutostartStatus`).

use std::env;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use mt_core::error::MtResult;
use serde::{Deserialize, Serialize};

use crate::error::{codes, os_error};

/// Nome do arquivo XDG Autostart gerenciado pelo `MaxTerminal`.
pub const AUTOSTART_DESKTOP_FILENAME: &str = "maxterminal.desktop";

/// Estado real da entrada XDG Autostart no sistema de arquivos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AutostartStatus {
    /// `true` se o arquivo `.desktop` existe e não está marcado com `Hidden=true`.
    pub enabled: bool,
    /// Caminho completo do arquivo `.desktop` quando existente.
    pub desktop_file: Option<String>,
}

/// Gerenciador de sincronização do arquivo `$XDG_CONFIG_HOME/autostart/maxterminal.desktop`.
#[derive(Debug, Clone)]
pub struct AutostartSync {
    autostart_dir: PathBuf,
    exec_path: PathBuf,
}

impl AutostartSync {
    /// Cria uma instância resolvendo `$XDG_CONFIG_HOME/autostart` (ou `~/.config/autostart`) e o executável atual.
    pub fn from_system_env() -> MtResult<Self> {
        let base_config = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(dirs::config_dir)
            .ok_or_else(|| {
                os_error(
                    codes::AUTOSTART_FAILED,
                    "Não foi possível resolver o diretório XDG_CONFIG_HOME para autostart.",
                )
            })?;

        let exec_path = env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| env::current_exe().ok())
            .unwrap_or_else(|| PathBuf::from("/usr/bin/maxterminal"));

        Ok(Self {
            autostart_dir: base_config.join("autostart"),
            exec_path,
        })
    }

    /// Cria uma instância com diretório e caminho de executável explícitos (para testes isolados).
    #[must_use]
    pub const fn new(autostart_dir: PathBuf, exec_path: PathBuf) -> Self {
        Self {
            autostart_dir,
            exec_path,
        }
    }

    /// Caminho completo do arquivo `maxterminal.desktop`.
    #[must_use]
    pub fn desktop_file_path(&self) -> PathBuf {
        self.autostart_dir.join(AUTOSTART_DESKTOP_FILENAME)
    }

    /// Lê o status real do arquivo `.desktop` no disco.
    pub fn status(&self) -> MtResult<AutostartStatus> {
        let path = self.desktop_file_path();
        match fs::read_to_string(&path) {
            Ok(content) => {
                let is_hidden = content.lines().any(|line| {
                    let trimmed = line.trim();
                    trimmed.eq_ignore_ascii_case("Hidden=true")
                        || trimmed.eq_ignore_ascii_case("X-GNOME-Autostart-enabled=false")
                });
                if is_hidden {
                    Ok(AutostartStatus {
                        enabled: false,
                        desktop_file: None,
                    })
                } else {
                    Ok(AutostartStatus {
                        enabled: true,
                        desktop_file: Some(path.to_string_lossy().into_owned()),
                    })
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(AutostartStatus {
                enabled: false,
                desktop_file: None,
            }),
            Err(err) => Err(os_error(
                codes::AUTOSTART_FAILED,
                format!("Falha ao consultar o arquivo de inicialização automática: {err}"),
            )),
        }
    }

    /// Aplica atomicamente a configuração de `autostart` e `start_minimized` no disco.
    pub fn apply(&self, enabled: bool, start_minimized: bool) -> MtResult<AutostartStatus> {
        let path = self.desktop_file_path();
        if !enabled {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => {
                    return Err(os_error(
                        codes::AUTOSTART_FAILED,
                        format!("Falha ao remover o arquivo de inicialização automática: {err}"),
                    ));
                }
            }
            return Ok(AutostartStatus {
                enabled: false,
                desktop_file: None,
            });
        }

        let content = render_desktop_entry(&self.exec_path, start_minimized)?;
        fs::create_dir_all(&self.autostart_dir).map_err(|e| {
            os_error(
                codes::AUTOSTART_FAILED,
                format!("Falha ao criar o diretório de autostart: {e}"),
            )
        })?;

        let tmp_path = self.autostart_dir.join(format!(
            ".{AUTOSTART_DESKTOP_FILENAME}.tmp.{}",
            std::process::id()
        ));

        {
            let mut file = fs::File::create(&tmp_path).map_err(|e| {
                os_error(
                    codes::AUTOSTART_FAILED,
                    format!("Falha ao criar arquivo temporário de autostart: {e}"),
                )
            })?;
            file.write_all(content.as_bytes()).map_err(|e| {
                os_error(
                    codes::AUTOSTART_FAILED,
                    format!("Falha ao gravar conteúdo do arquivo de autostart: {e}"),
                )
            })?;
            let _ = file.set_permissions(fs::Permissions::from_mode(0o644));
            file.sync_all().map_err(|e| {
                os_error(
                    codes::AUTOSTART_FAILED,
                    format!("Falha ao sincronizar arquivo de autostart no disco: {e}"),
                )
            })?;
        }

        fs::rename(&tmp_path, &path).map_err(|e| {
            let _ = fs::remove_file(&tmp_path);
            os_error(
                codes::AUTOSTART_FAILED,
                format!("Falha ao publicar o arquivo de autostart: {e}"),
            )
        })?;

        Ok(AutostartStatus {
            enabled: true,
            desktop_file: Some(path.to_string_lossy().into_owned()),
        })
    }
}

/// Renderiza o conteúdo do arquivo `maxterminal.desktop` segundo a especificação XDG Desktop Entry.
pub fn render_desktop_entry(exec_path: &Path, start_minimized: bool) -> MtResult<String> {
    let exec_str = exec_path.to_str().ok_or_else(|| {
        os_error(
            codes::AUTOSTART_FAILED,
            "O caminho do executável contém bytes inválidos em UTF-8.",
        )
    })?;

    if exec_str.is_empty() || exec_str.chars().any(char::is_control) {
        return Err(os_error(
            codes::AUTOSTART_FAILED,
            "O caminho do executável é inválido para entrada Desktop Entry.",
        ));
    }

    let quoted_exec = quote_desktop_exec_arg(exec_str);
    let exec_line = if start_minimized {
        format!("{quoted_exec} --start-hidden")
    } else {
        quoted_exec
    };

    Ok(format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.0\n\
         Name=MaxTerminal\n\
         Comment=Emulador de terminal acelerado por GPU e orquestrador de Agentes de IA\n\
         Exec={exec_line}\n\
         Icon=maxterminal\n\
         Terminal=false\n\
         Categories=System;TerminalEmulator;Development;\n\
         StartupWMClass=maxterminal\n\
         X-GNOME-Autostart-enabled=true\n"
    ))
}

fn quote_desktop_exec_arg(arg: &str) -> String {
    let needs_quotes = arg.chars().any(|c| {
        matches!(
            c,
            ' ' | '\t'
                | '"'
                | '\''
                | '\\'
                | '>'
                | '<'
                | '~'
                | '|'
                | '&'
                | ';'
                | '$'
                | '*'
                | '?'
                | '#'
                | '('
                | ')'
                | '`'
        )
    });
    if !needs_quotes && !arg.contains('%') {
        return arg.to_owned();
    }
    let mut escaped = String::with_capacity(arg.len() + 8);
    escaped.push('"');
    for ch in arg.chars() {
        match ch {
            '"' | '`' | '$' | '\\' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            '%' => {
                escaped.push_str("%%");
            }
            c => escaped.push(c),
        }
    }
    escaped.push('"');
    escaped
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn autostart_resolves_pacman_appimage_and_local_binary_and_toggles_start_hidden() {
        let tmp = TempDir::new().unwrap();
        let autostart_dir = tmp.path().join("autostart");

        // 1. Pacote pacman/AUR (/usr/bin/maxterminal) sem --start-hidden
        let sync_pacman =
            AutostartSync::new(autostart_dir.clone(), PathBuf::from("/usr/bin/maxterminal"));
        let status_on = sync_pacman.apply(true, false).unwrap();
        assert!(status_on.enabled);
        let content_pacman = fs::read_to_string(sync_pacman.desktop_file_path()).unwrap();
        assert!(content_pacman.contains("Exec=/usr/bin/maxterminal\n"));

        // 2. AppImage com espaços no caminho e --start-hidden ativado
        let sync_appimage = AutostartSync::new(
            autostart_dir.clone(),
            PathBuf::from("/home/dev/Meus Apps/MaxTerminal_0.1.0_amd64.AppImage"),
        );
        let status_min = sync_appimage.apply(true, true).unwrap();
        assert!(status_min.enabled);
        let content_appimage = fs::read_to_string(sync_appimage.desktop_file_path()).unwrap();
        assert!(content_appimage.contains(
            "Exec=\"/home/dev/Meus Apps/MaxTerminal_0.1.0_amd64.AppImage\" --start-hidden\n"
        ));

        // 3. Desativar remove o arquivo .desktop de forma idempotente
        let status_off = sync_appimage.apply(false, true).unwrap();
        assert!(!status_off.enabled);
        assert!(!sync_appimage.desktop_file_path().exists());
        let status_off_again = sync_appimage.apply(false, false).unwrap();
        assert!(!status_off_again.enabled);
    }
}
