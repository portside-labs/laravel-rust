//! Terminal detection and sizing for the installer.

use std::io::{self, IsTerminal};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::{Key, RawMode, install_panic_hook, read_keys};
#[cfg(windows)]
pub use windows::{enable_ansi, supports_ansi};

#[cfg(not(windows))]
pub fn supports_ansi() -> bool {
    io::stdout().is_terminal()
}

/// Determine whether both standard input and output are terminals.
pub fn is_interactive() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

/// The terminal's width in columns (80 when it can't be determined).
pub fn width() -> usize {
    #[cfg(unix)]
    let columns = unix::width();
    #[cfg(windows)]
    let columns = windows::width();
    #[cfg(not(any(unix, windows)))]
    let columns = None;

    columns
        .or_else(|| {
            std::env::var("COLUMNS")
                .ok()
                .and_then(|columns| columns.parse().ok())
                .filter(|columns| *columns > 0)
        })
        .unwrap_or(80)
}
