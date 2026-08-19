//! Typed RON loaders and registries — owns thiserror parse errors for templates.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DataError {
    #[error("ron parse error: {0}")]
    Ron(#[from] ron::Error),
    #[error("ron spanned error: {0}")]
    Spanned(#[from] ron::error::SpannedError),
}
