//! Private read-only production views. Observations are not approval/custody.
pub mod film;
#[cfg(windows)]
pub mod host;
pub mod model;
pub mod spoken;
pub use model::*;
use reel_workflow::{Error, ErrorKind};
pub(crate) fn error(kind: ErrorKind, code: &str) -> Error {
    Error {
        kind,
        code: code.into(),
    }
}
