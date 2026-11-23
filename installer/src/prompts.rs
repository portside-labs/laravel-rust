//! The installer's interactive questions.

#[cfg(any(not(unix), test))]
mod plain;
#[cfg(unix)]
mod unix;

#[cfg(not(unix))]
pub use plain::{confirm, select, text};
#[cfg(unix)]
pub use unix::{confirm, select, text};

/// The user cancelled a question.
#[derive(Debug, PartialEq, Eq)]
pub struct Cancelled;
