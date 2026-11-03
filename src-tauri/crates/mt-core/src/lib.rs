//! Tipos de domínio compartilhados do `MaxTerminal`: identificadores UUID v7,
//! erros estruturados, resolução de caminhos XDG e metadados da aplicação.

pub mod app;
pub mod error;
pub use error as errors;
pub mod ids;
pub mod paths;
pub mod shell;
pub mod workspace;

pub use error::{ErrorCode, MtError, MtResult};
pub use ids::{
    CommandBlockId, CommandGroupId, PaneId, ProjectId, PtyId, SessionId, TabId, TabSessionId,
    TemplateId,
};
pub use paths::AppPaths;
