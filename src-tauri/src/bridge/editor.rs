//! Opener dedicado para `EditorKind::MaxCode` registrado em `OsIntegration::editor`.

use std::sync::Arc;

use async_trait::async_trait;
use mt_bridge::{BridgeService, OpenFilePayload};
use mt_core::error::{MtError, MtResult};
use mt_os::{EditorKind, EditorOpener, ResolvedLink};

/// Implementação de `EditorOpener` que utiliza o `BridgeService` para abrir arquivos no `MaxCode`.
pub struct BridgeEditorOpener {
    bridge: Arc<BridgeService>,
}

impl BridgeEditorOpener {
    /// Cria uma nova instância compartilhada de `BridgeEditorOpener`.
    #[must_use]
    pub fn new(bridge: Arc<BridgeService>) -> Arc<Self> {
        Arc::new(Self { bridge })
    }
}

#[async_trait]
impl EditorOpener for BridgeEditorOpener {
    fn handles(&self, kind: EditorKind) -> bool {
        kind == EditorKind::MaxCode
    }

    async fn open(&self, link: &ResolvedLink) -> MtResult<()> {
        self.bridge
            .open_file_in_maxcode(OpenFilePayload {
                file_path: link.absolute_path.clone(),
                line: link.line,
                column: link.column,
                project_path: None,
                focus_editor: true,
            })
            .await
            .map(|_| ())
            .map_err(MtError::from)
    }
}
