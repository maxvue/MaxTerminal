//! Resolução de candidatos de links de arquivos no terminal (`LinkResolver`, `LinkContext` e `TabCwdProvider`).

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use futures_util::stream::{self, StreamExt};
use mt_core::error::MtResult;
use mt_core::ids::{PaneId, PtyId, TabId};
use serde::{Deserialize, Serialize};

use crate::error::{codes, os_error};

/// Limite máximo de número de linha ou coluna aceito (10^7).
pub const MAX_LINE_OR_COLUMN: u32 = 10_000_000;

/// Candidato de link extraído pelo parser do frontend a partir de uma linha do terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LinkCandidate {
    /// Chave opaca única do candidato na linha (inclui posição para ocorrências repetidas).
    pub key: String,
    /// Caminho bruto extraído (relativo, `~/...` ou absoluto).
    pub path: String,
    /// Linha 1-based opcional.
    #[specta(optional)]
    pub line: Option<u32>,
    /// Coluna 1-based opcional (inválida se `line` for `None`).
    #[specta(optional)]
    pub column: Option<u32>,
}

/// Link verificado no disco apontando para um arquivo regular existente.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLink {
    /// Chave correspondente ao `LinkCandidate.key`.
    pub key: String,
    /// Caminho absoluto canonicalizado no sistema de arquivos.
    pub absolute_path: String,
    /// Linha 1-based opcional.
    #[specta(optional)]
    pub line: Option<u32>,
    /// Coluna 1-based opcional.
    #[specta(optional)]
    pub column: Option<u32>,
}

/// Contexto de vínculo entre aba, painel, PTY vivo e época de CWD.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LinkContext {
    /// Identificador da aba.
    pub tab_id: TabId,
    /// Identificador do painel.
    pub pane_id: PaneId,
    /// Identificador do PTY ativo no painel.
    pub pty_id: PtyId,
    /// Época monotônica do CWD do painel (incrementada a cada mudança de diretório/PTY).
    pub cwd_epoch: u32,
}

/// Provedor de diretório de trabalho atual (`cwd`) por painel/PTY implementado no crate binário.
pub trait TabCwdProvider: Send + Sync + 'static {
    /// Verifica o contexto `(tab_id, pane_id, pty_id, cwd_epoch)` e retorna:
    /// - `Err(os.tab_not_found)` se a aba/painel não existir;
    /// - `Err(os.link_context_stale)` se `pty_id` ou `cwd_epoch` divergir do estado atual;
    /// - `Ok(Some(cwd))` se o diretório atual for conhecido, ou `Ok(None)` se desconhecido.
    fn cwd(&self, context: &LinkContext) -> MtResult<Option<PathBuf>>;
}

/// Resolvedor assíncrono de links do terminal com verificação de arquivo regular e proteção contra corrida de CWD.
pub struct LinkResolver {
    cwd_provider: Arc<dyn TabCwdProvider>,
    home: PathBuf,
}

impl LinkResolver {
    /// Quantidade máxima de candidatos por lote em `os_link_resolve`.
    pub const MAX_BATCH: usize = 64;
    /// Comprimento máximo em bytes de um caminho de candidato.
    pub const MAX_PATH_LEN: usize = 4096;
    /// Paralelismo máximo de chamadas `tokio::fs::metadata` simultâneas.
    pub const MAX_CONCURRENT_IO: usize = 16;

    /// Cria um novo `LinkResolver`.
    #[must_use]
    pub fn new(cwd_provider: Arc<dyn TabCwdProvider>, home: PathBuf) -> Self {
        Self { cwd_provider, home }
    }

    /// Função pura: combina `cwd`, `home` e `raw` sem tocar no disco:
    /// - `~` ou `~/...` → `home.join(...)`
    /// - absoluto (`/`) → mantém
    /// - relativo → `cwd.join(...)`
    ///
    /// Rejeita strings vazias, com NUL/caracteres de controle, URLs (`://`), `~outro_usuario`
    /// ou acima de `MAX_PATH_LEN`.
    #[must_use]
    pub fn join(cwd: Option<&Path>, home: &Path, raw: &str) -> Option<PathBuf> {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.len() > Self::MAX_PATH_LEN {
            return None;
        }
        if trimmed.bytes().any(|b| b < 0x20 || b == 0x7f) {
            return None;
        }
        if trimmed.contains("://") {
            return None;
        }

        let joined = if trimmed == "~" {
            home.to_path_buf()
        } else if let Some(rest) = trimmed.strip_prefix("~/") {
            home.join(rest)
        } else if trimmed.starts_with('~') {
            // Não expande ~outro_usuario
            return None;
        } else {
            let p = Path::new(trimmed);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                cwd?.join(p)
            }
        };

        Some(normalize_lexical(&joined))
    }

    /// Valida coordenadas de linha e coluna (`1..=10_000_000`; `column` exige `line`).
    #[must_use]
    pub const fn validate_coords(line: Option<u32>, column: Option<u32>) -> bool {
        if let Some(l) = line {
            if l == 0 || l > MAX_LINE_OR_COLUMN {
                return false;
            }
        } else if column.is_some() {
            return false;
        }

        if let Some(c) = column
            && (c == 0 || c > MAX_LINE_OR_COLUMN)
        {
            return false;
        }
        true
    }

    /// Resolve um lote de até 64 candidatos retornando apenas aqueles que existem no disco e são arquivos regulares.
    pub async fn resolve(
        &self,
        context: &LinkContext,
        candidates: Vec<LinkCandidate>,
    ) -> MtResult<Vec<ResolvedLink>> {
        if candidates.len() > Self::MAX_BATCH {
            return Err(os_error(
                codes::LINK_REJECTED,
                format!(
                    "O lote de links ({}) excede o limite máximo de {} candidatos.",
                    candidates.len(),
                    Self::MAX_BATCH
                ),
            ));
        }

        // Verifica vínculo antes do I/O
        let cwd_opt = self.cwd_provider.cwd(context)?;
        let home = self.home.clone();

        let prepared: Vec<(LinkCandidate, PathBuf)> = candidates
            .into_iter()
            .filter(|c| Self::validate_coords(c.line, c.column))
            .filter_map(|c| {
                let target = Self::join(cwd_opt.as_deref(), &home, &c.path)?;
                Some((c, target))
            })
            .collect();

        let results: Vec<Option<ResolvedLink>> = stream::iter(prepared)
            .map(|(candidate, path)| async move {
                let canonical = tokio::fs::canonicalize(&path).await.ok()?;
                let meta = tokio::fs::metadata(&canonical).await.ok()?;
                if !meta.is_file() {
                    return None;
                }
                let abs_str = canonical.to_str()?.to_owned();
                if abs_str.starts_with('-') {
                    return None;
                }
                Some(ResolvedLink {
                    key: candidate.key,
                    absolute_path: abs_str,
                    line: candidate.line,
                    column: candidate.column,
                })
            })
            .buffered(Self::MAX_CONCURRENT_IO)
            .collect()
            .await;

        // Revalida o contexto após o I/O para detectar troca de CWD/PTY durante a resolução
        let _ = self.cwd_provider.cwd(context)?;

        Ok(results.into_iter().flatten().collect())
    }

    /// Resolve estritamente um único candidato para abertura no editor (`os_link_open`).
    pub async fn resolve_single(
        &self,
        context: &LinkContext,
        candidate: LinkCandidate,
    ) -> MtResult<ResolvedLink> {
        if !Self::validate_coords(candidate.line, candidate.column) {
            return Err(os_error(
                codes::LINK_REJECTED,
                "Coordenadas de linha ou coluna inválidas para abertura no editor.",
            ));
        }
        if candidate.path.trim().is_empty()
            || candidate.path.len() > Self::MAX_PATH_LEN
            || candidate.path.bytes().any(|b| b < 0x20 || b == 0x7f)
            || candidate.path.contains("://")
        {
            return Err(os_error(
                codes::LINK_REJECTED,
                "Caminho de link inválido ou rejeitado pela política de segurança.",
            ));
        }

        let cwd_opt = self.cwd_provider.cwd(context)?;
        let Some(joined) = Self::join(cwd_opt.as_deref(), &self.home, &candidate.path) else {
            return Err(os_error(
                codes::LINK_NOT_FOUND,
                "Não foi possível resolver o caminho relativo sem um diretório de trabalho ativo.",
            ));
        };

        let canonical = tokio::fs::canonicalize(&joined).await.map_err(|_| {
            os_error(
                codes::LINK_NOT_FOUND,
                format!(
                    "O arquivo '{}' não foi encontrado no disco.",
                    candidate.path
                ),
            )
        })?;

        let meta = tokio::fs::metadata(&canonical).await.map_err(|_| {
            os_error(
                codes::LINK_NOT_FOUND,
                format!("O arquivo '{}' não pôde ser acessado.", candidate.path),
            )
        })?;

        if !meta.is_file() {
            return Err(os_error(
                codes::LINK_REJECTED,
                "O caminho selecionado não é um arquivo regular.",
            ));
        }

        let _ = self.cwd_provider.cwd(context)?;

        let abs_str = canonical.to_str().ok_or_else(|| {
            os_error(
                codes::LINK_REJECTED,
                "O caminho do arquivo contém bytes não representáveis em UTF-8.",
            )
        })?;

        Ok(ResolvedLink {
            key: candidate.key,
            absolute_path: abs_str.to_owned(),
            line: candidate.line,
            column: candidate.column,
        })
    }
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !matches!(
                    out.components().next_back(),
                    Some(Component::RootDir) | None
                ) {
                    out.pop();
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}
