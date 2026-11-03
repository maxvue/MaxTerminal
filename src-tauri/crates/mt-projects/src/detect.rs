//! Auto-detecção de stack tecnológica (`FolderProbe` e `StackDetector`) sem execução de processos.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use mt_core::error::codes;

use crate::model::{ScanWarning, StackKind};

/// Limite máximo de bytes lidos de `package.json` (`1 MiB`).
const MAX_PACKAGE_JSON_BYTES: u64 = 1024 * 1024;

/// Regra unitária de detecção de stack avaliada sobre um [`FolderProbe`] já carregado.
pub trait StackRule: Send + Sync {
    /// Stack associada a esta regra.
    fn kind(&self) -> StackKind;
    /// Retorna `true` quando o snapshot da pasta satisfaz a regra.
    fn matches(&self, probe: &FolderProbe) -> bool;
}

/// Snapshot pré-lido e limitado dos arquivos marcadores na raiz de uma pasta.
#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct FolderProbe {
    /// Presença do arquivo regular `artisan`.
    pub has_artisan: bool,
    /// Presença do arquivo regular `composer.json`.
    pub has_composer_json: bool,
    /// JSON parseado de `package.json` (`None` se ausente, `> 1 MiB` ou inválido).
    pub package_json: Option<serde_json::Value>,
    /// Presença de `src-tauri/tauri.conf.json`, `src-tauri/tauri.conf.json5` ou `src-tauri/Tauri.toml`.
    pub has_tauri_conf: bool,
    /// Presença do arquivo regular `pyproject.toml`.
    pub has_pyproject: bool,
    /// Presença do arquivo regular `requirements.txt`.
    pub has_requirements: bool,
    /// Presença do arquivo regular `Cargo.toml`.
    pub has_cargo_toml: bool,
    /// Indica se todas as verificações de marcadores ocorreram sem erro de I/O/permissão.
    pub complete: bool,
    /// Avisos não fatais coletados durante a leitura dos marcadores.
    pub warnings: Vec<ScanWarning>,
}

impl FolderProbe {
    /// Lê os marcadores da pasta `dir` sem seguir links simbólicos em arquivos e sem bloquear em FIFOs.
    pub fn read(dir: &Path) -> std::io::Result<Self> {
        let dir_meta = fs::metadata(dir)?;
        if !dir_meta.is_dir() {
            return Err(std::io::Error::new(
                ErrorKind::NotADirectory,
                "O caminho avaliado não é um diretório.",
            ));
        }

        let mut probe = Self {
            complete: true,
            ..Self::default()
        };

        probe.has_artisan = check_regular_file(
            &dir.join("artisan"),
            &mut probe.complete,
            &mut probe.warnings,
        );
        probe.has_composer_json = check_regular_file(
            &dir.join("composer.json"),
            &mut probe.complete,
            &mut probe.warnings,
        );
        probe.has_pyproject = check_regular_file(
            &dir.join("pyproject.toml"),
            &mut probe.complete,
            &mut probe.warnings,
        );
        probe.has_requirements = check_regular_file(
            &dir.join("requirements.txt"),
            &mut probe.complete,
            &mut probe.warnings,
        );
        probe.has_cargo_toml = check_regular_file(
            &dir.join("Cargo.toml"),
            &mut probe.complete,
            &mut probe.warnings,
        );

        let tauri_dir = dir.join("src-tauri");
        let has_conf_json = check_regular_file(
            &tauri_dir.join("tauri.conf.json"),
            &mut probe.complete,
            &mut probe.warnings,
        );
        let has_conf_json5 = if has_conf_json {
            false
        } else {
            check_regular_file(
                &tauri_dir.join("tauri.conf.json5"),
                &mut probe.complete,
                &mut probe.warnings,
            )
        };
        let has_tauri_toml = if has_conf_json || has_conf_json5 {
            false
        } else {
            check_regular_file(
                &tauri_dir.join("Tauri.toml"),
                &mut probe.complete,
                &mut probe.warnings,
            )
        };
        probe.has_tauri_conf = has_conf_json || has_conf_json5 || has_tauri_toml;

        probe.package_json = read_safe_package_json(
            &dir.join("package.json"),
            &mut probe.complete,
            &mut probe.warnings,
        );

        Ok(probe)
    }
}

fn check_regular_file(path: &Path, complete: &mut bool, warnings: &mut Vec<ScanWarning>) -> bool {
    match fs::symlink_metadata(path) {
        Ok(meta) => meta.file_type().is_file(),
        Err(err) if err.kind() == ErrorKind::NotFound || err.kind() == ErrorKind::NotADirectory => {
            false
        }
        Err(err) => {
            *complete = false;
            warnings.push(ScanWarning {
                path: path.to_string_lossy().into_owned(),
                code: match err.kind() {
                    ErrorKind::PermissionDenied => codes::PERMISSION_DENIED.0.to_string(),
                    _ => codes::IO.0.to_string(),
                },
                message: String::from(
                    "Não foi possível verificar marcador do projeto por erro de leitura ou permissão.",
                ),
            });
            false
        }
    }
}

fn read_safe_package_json(
    path: &Path,
    complete: &mut bool,
    warnings: &mut Vec<ScanWarning>,
) -> Option<serde_json::Value> {
    let sym_meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(err) if err.kind() == ErrorKind::NotFound || err.kind() == ErrorKind::NotADirectory => {
            return None;
        }
        Err(err) => {
            *complete = false;
            warnings.push(ScanWarning {
                path: path.to_string_lossy().into_owned(),
                code: match err.kind() {
                    ErrorKind::PermissionDenied => codes::PERMISSION_DENIED.0.to_string(),
                    _ => codes::IO.0.to_string(),
                },
                message: String::from("Não foi possível inspecionar o arquivo package.json."),
            });
            return None;
        }
    };

    if !sym_meta.file_type().is_file() {
        return None;
    }

    if sym_meta.len() > MAX_PACKAGE_JSON_BYTES {
        warnings.push(ScanWarning {
            path: path.to_string_lossy().into_owned(),
            code: codes::INVALID_ARGUMENT.0.to_string(),
            message: String::from(
                "O arquivo package.json excede o limite de 1 MiB e foi ignorado.",
            ),
        });
        return None;
    }

    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)
    {
        Ok(f) => f,
        Err(err) => {
            if err.kind() == ErrorKind::PermissionDenied {
                *complete = false;
            }
            warnings.push(ScanWarning {
                path: path.to_string_lossy().into_owned(),
                code: codes::IO.0.to_string(),
                message: String::from(
                    "Não foi possível abrir o arquivo package.json com segurança.",
                ),
            });
            return None;
        }
    };

    let Ok(handle_meta) = file.metadata() else {
        *complete = false;
        return None;
    };

    if !handle_meta.file_type().is_file() || handle_meta.len() > MAX_PACKAGE_JSON_BYTES {
        return None;
    }

    let max_read = MAX_PACKAGE_JSON_BYTES + 1;
    let mut buf = Vec::with_capacity(
        usize::try_from(handle_meta.len().min(MAX_PACKAGE_JSON_BYTES)).unwrap_or(4096),
    );
    if file.take(max_read).read_to_end(&mut buf).is_err() {
        *complete = false;
        return None;
    }

    if u64::try_from(buf.len()).unwrap_or(u64::MAX) > MAX_PACKAGE_JSON_BYTES {
        warnings.push(ScanWarning {
            path: path.to_string_lossy().into_owned(),
            code: codes::INVALID_ARGUMENT.0.to_string(),
            message: String::from(
                "O arquivo package.json excedeu 1 MiB durante a leitura e foi ignorado.",
            ),
        });
        return None;
    }

    serde_json::from_slice::<serde_json::Value>(&buf).ok()
}

struct TauriRule;
impl StackRule for TauriRule {
    fn kind(&self) -> StackKind {
        StackKind::Tauri
    }
    fn matches(&self, probe: &FolderProbe) -> bool {
        probe.has_tauri_conf
    }
}

struct VscodeExtensionRule;
impl StackRule for VscodeExtensionRule {
    fn kind(&self) -> StackKind {
        StackKind::VscodeExtension
    }
    fn matches(&self, probe: &FolderProbe) -> bool {
        probe
            .package_json
            .as_ref()
            .and_then(|pkg| pkg.get("engines"))
            .and_then(|eng| eng.get("vscode"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|s| !s.trim().is_empty())
    }
}

struct LaravelRule;
impl StackRule for LaravelRule {
    fn kind(&self) -> StackKind {
        StackKind::Laravel
    }
    fn matches(&self, probe: &FolderProbe) -> bool {
        probe.has_artisan && probe.has_composer_json
    }
}

struct VueRule;
impl StackRule for VueRule {
    fn kind(&self) -> StackKind {
        StackKind::Vue
    }
    fn matches(&self, probe: &FolderProbe) -> bool {
        let Some(pkg) = probe.package_json.as_ref() else {
            return false;
        };
        for section in ["dependencies", "devDependencies", "peerDependencies"] {
            if pkg
                .get(section)
                .and_then(serde_json::Value::as_object)
                .is_some_and(|obj| obj.contains_key("vue"))
            {
                return true;
            }
        }
        false
    }
}

struct RustRule;
impl StackRule for RustRule {
    fn kind(&self) -> StackKind {
        StackKind::Rust
    }
    fn matches(&self, probe: &FolderProbe) -> bool {
        probe.has_cargo_toml
    }
}

struct PythonRule;
impl StackRule for PythonRule {
    fn kind(&self) -> StackKind {
        StackKind::Python
    }
    fn matches(&self, probe: &FolderProbe) -> bool {
        probe.has_pyproject || probe.has_requirements
    }
}

/// Resultado de uma avaliação do [`StackDetector`].
#[derive(Debug, Clone)]
pub struct DetectionReport {
    /// Stacks detectadas em ordem estrita de prioridade (nunca vazio; fallback `[StackKind::Unknown]`).
    pub detected: Vec<StackKind>,
    /// Indica se o probe de arquivos completou sem erros de permissão/I/O.
    pub complete: bool,
    /// Avisos coletados durante a leitura do diretório.
    pub warnings: Vec<ScanWarning>,
}

/// Motor de detecção de stack baseado em regras ordenadas por prioridade.
pub struct StackDetector {
    rules: Vec<Box<dyn StackRule>>,
}

impl Default for StackDetector {
    fn default() -> Self {
        Self::default_rules()
    }
}

impl std::fmt::Debug for StackDetector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StackDetector")
            .field("rules_count", &self.rules.len())
            .finish()
    }
}

impl StackDetector {
    /// Cria o detector com as 6 regras canônicas na ordem de prioridade da especificação.
    #[must_use]
    pub fn default_rules() -> Self {
        Self {
            rules: vec![
                Box::new(TauriRule),
                Box::new(VscodeExtensionRule),
                Box::new(LaravelRule),
                Box::new(VueRule),
                Box::new(RustRule),
                Box::new(PythonRule),
            ],
        }
    }

    /// Avalia um [`FolderProbe`] já carregado.
    #[must_use]
    pub fn detect_from_probe(&self, probe: &FolderProbe) -> Vec<StackKind> {
        let mut out: Vec<StackKind> = self
            .rules
            .iter()
            .filter(|rule| rule.matches(probe))
            .map(|rule| rule.kind())
            .collect();
        if out.is_empty() {
            out.push(StackKind::Unknown);
        }
        out
    }

    /// Lê os marcadores da pasta `dir` e retorna o [`DetectionReport`].
    #[must_use]
    pub fn detect(&self, dir: &Path) -> DetectionReport {
        match FolderProbe::read(dir) {
            Ok(probe) => {
                let detected = self.detect_from_probe(&probe);
                DetectionReport {
                    detected,
                    complete: probe.complete,
                    warnings: probe.warnings,
                }
            }
            Err(err) => DetectionReport {
                detected: vec![StackKind::Unknown],
                complete: false,
                warnings: vec![ScanWarning {
                    path: dir.to_string_lossy().into_owned(),
                    code: match err.kind() {
                        ErrorKind::PermissionDenied => codes::PERMISSION_DENIED.0.to_string(),
                        ErrorKind::NotFound => codes::PROJECT_PATH_NOT_FOUND.0.to_string(),
                        _ => codes::IO.0.to_string(),
                    },
                    message: String::from(
                        "Não foi possível inspecionar os arquivos da pasta do projeto.",
                    ),
                }],
            },
        }
    }
}
