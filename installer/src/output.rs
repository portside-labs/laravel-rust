//! Laravel-flavoured terminal output: colors, the logo, and the console
//! components (`INFO`, `WARN`, `ERROR` lines and `DONE` tasks) you know
//! from Artisan.

use std::fmt::Display;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use crate::terminal;

static COLORS: AtomicBool = AtomicBool::new(false);
static TRUECOLOR: AtomicBool = AtomicBool::new(true);

/// How many newlines the output currently ends with (Symfony starts at 1).
static NEW_LINES: AtomicUsize = AtomicUsize::new(1);

/// The Laravel logo, as printed by `laravel/installer`.
pub const LOGO: [&str; 6] = [
    "██╗      █████╗ ██████╗  █████╗ ██╗   ██╗███████╗██╗",
    "██║     ██╔══██╗██╔══██╗██╔══██╗██║   ██║██╔════╝██║",
    "██║     ███████║██████╔╝███████║██║   ██║█████╗  ██║",
    "██║     ██╔══██║██╔══██╗██╔══██║╚██╗ ██╔╝██╔══╝  ██║",
    "███████╗██║  ██║██║  ██║██║  ██║ ╚████╔╝ ███████╗███████╗",
    "╚══════╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝  ╚═══╝  ╚══════╝╚══════╝",
];

/// Turn colors on or off for everything written from now on.
pub fn set_colors(enabled: bool) {
    COLORS.store(enabled, Ordering::Relaxed);
    // Apple's Terminal only learned 24-bit color recently; give it the
    // closest 256-color red instead.
    let apple_terminal =
        std::env::var("TERM_PROGRAM").is_ok_and(|program| program == "Apple_Terminal");
    TRUECOLOR.store(!apple_terminal, Ordering::Relaxed);
}

/// Determine whether colors are on.
pub fn colors() -> bool {
    COLORS.load(Ordering::Relaxed)
}

/// Wrap the text in the given SGR codes when colors are on.
pub fn paint(text: impl Display, open: &str, close: &str) -> String {
    if colors() {
        format!("\x1b[{open}m{text}\x1b[{close}m")
    } else {
        text.to_string()
    }
}

#[cfg(unix)]
pub fn red(text: impl Display) -> String {
    paint(text, "31", "39")
}

pub fn green(text: impl Display) -> String {
    paint(text, "32", "39")
}

pub fn yellow(text: impl Display) -> String {
    paint(text, "33", "39")
}

#[cfg(unix)]
pub fn cyan(text: impl Display) -> String {
    paint(text, "36", "39")
}

pub fn gray(text: impl Display) -> String {
    paint(text, "90", "39")
}

#[cfg(unix)]
pub fn dim(text: impl Display) -> String {
    paint(text, "2", "22")
}

pub fn bold(text: impl Display) -> String {
    paint(text, "1", "22")
}

#[cfg(unix)]
pub fn strikethrough(text: impl Display) -> String {
    paint(text, "9", "29")
}

/// Laravel red: `#FF2D20`.
pub fn laravel_red(text: impl Display) -> String {
    if TRUECOLOR.load(Ordering::Relaxed) {
        paint(text, "38;2;255;45;32", "39")
    } else {
        paint(text, "38;5;196", "39")
    }
}

/// A terminal hyperlink (OSC 8), falling back to the plain text.
pub fn link(text: &str, url: &str) -> String {
    if colors() {
        format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
    } else {
        text.to_string()
    }
}

/// Count the newlines a piece of output ends with.
fn trailing_new_lines(text: &str) -> Option<usize> {
    if text.is_empty() {
        return None;
    }
    Some(
        text.chars()
            .rev()
            .take_while(|character| *character == '\n')
            .count(),
    )
}

/// Write to standard output, remembering how many newlines it ends with.
pub fn write(text: &str) {
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.flush();

    if let Some(count) = trailing_new_lines(text) {
        if count == text.chars().count() {
            NEW_LINES.fetch_add(count, Ordering::Relaxed);
        } else {
            NEW_LINES.store(count, Ordering::Relaxed);
        }
    }
}

/// Write a line to standard output.
pub fn line(text: &str) {
    write(&format!("{text}\n"));
}

/// Write to standard error (used for errors).
fn write_error(text: &str) {
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(text.as_bytes());
    let _ = stderr.flush();
    if let Some(count) = trailing_new_lines(text) {
        NEW_LINES.store(count, Ordering::Relaxed);
    }
}

/// How many newlines the output ends with.
pub fn new_lines_written() -> usize {
    NEW_LINES.load(Ordering::Relaxed)
}

/// Record how the output ends after a child process wrote to the terminal.
pub fn assume_new_lines(count: usize) {
    NEW_LINES.store(count, Ordering::Relaxed);
}

/// Margin that leaves exactly one blank line above the next block.
pub fn margin_top() -> String {
    "\n".repeat(2usize.saturating_sub(new_lines_written()))
}

/// Print the Laravel logo, with a blank line above and below it.
pub fn logo() {
    let mut rendered = margin_top();
    for row in LOGO {
        rendered.push_str(&format!("  {}\n", laravel_red(row)));
    }
    rendered.push('\n');
    write(&rendered);
}

/// Highlight `[dynamic content]` in bold, like Laravel's components.
pub fn highlight(message: &str) -> String {
    let mut highlighted = String::new();
    let mut rest = message;
    while let Some(start) = rest.find('[') {
        let Some(length) = rest[start..].find(']') else {
            break;
        };
        highlighted.push_str(&rest[..start]);
        highlighted.push_str(&bold(&rest[start..=start + length]));
        rest = &rest[start + length + 1..];
    }
    highlighted.push_str(rest);
    highlighted
}

fn ensure_punctuation(message: &str) -> String {
    if message.ends_with(['.', '?', '!', ':']) {
        message.to_string()
    } else {
        format!("{message}.")
    }
}

/// Render a component line: `  INFO  Message.`
pub fn render_line(badge: &str, open: &str, message: &str) -> String {
    format!(
        "  {} {}\n",
        paint(format!(" {badge} "), open, "39;49"),
        highlight(&ensure_punctuation(message))
    )
}

/// `  INFO  Message.`
pub fn info(message: &str) {
    write(&format!(
        "{}{}\n",
        margin_top(),
        render_line("INFO", "37;44", message)
    ));
}

/// `  WARN  Message.`
pub fn warn(message: &str) {
    write(&format!(
        "{}{}\n",
        margin_top(),
        render_line("WARN", "30;43", message)
    ));
}

/// `  ERROR  Message.` (on standard error).
pub fn error(message: &str) {
    write_error(&format!(
        "{}{}\n",
        margin_top(),
        render_line("ERROR", "37;41", message)
    ));
}

/// Run a task, rendering `  Description ........ 1.23ms DONE`.
pub fn task<T, E>(description: &str, callback: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    write(&format!("  {description} "));
    let started = Instant::now();
    let result = callback();
    let runtime = format!(" {}", runtime_for_humans(started));

    let width = terminal::width().min(150);
    let dots = width.saturating_sub(description.chars().count() + runtime.chars().count() + 10);
    let status = if result.is_ok() {
        paint(bold("DONE"), "32", "39")
    } else {
        paint(bold("FAIL"), "31", "39")
    };
    line(&format!(
        "{}{} {status}",
        gray(".".repeat(dots)),
        gray(runtime)
    ));
    result
}

fn runtime_for_humans(started: Instant) -> String {
    let milliseconds = started.elapsed().as_secs_f64() * 1000.0;
    if milliseconds < 1000.0 {
        format!("{milliseconds:.2}ms")
    } else {
        format!("{:.2}s", milliseconds / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_content_is_highlighted() {
        // Colors are off in tests, so highlighting leaves the text alone.
        assert_eq!(
            highlight("Application ready in [podcasts]"),
            "Application ready in [podcasts]"
        );
        assert_eq!(
            render_line("INFO", "37;44", "Ready in [podcasts]"),
            "   INFO  Ready in [podcasts].\n"
        );
        assert_eq!(
            render_line("INFO", "37;44", "Start using:"),
            "   INFO  Start using:\n"
        );
    }

    #[test]
    fn trailing_new_lines_are_counted() {
        assert_eq!(trailing_new_lines(""), None);
        assert_eq!(trailing_new_lines("text"), Some(0));
        assert_eq!(trailing_new_lines("text\n\n"), Some(2));
        assert_eq!(trailing_new_lines("\n"), Some(1));
    }

    #[test]
    fn the_logo_spells_laravel() {
        let widths: Vec<usize> = LOGO.iter().map(|row| row.chars().count()).collect();
        assert_eq!(widths, [52, 52, 52, 52, 57, 57]);
    }
}
