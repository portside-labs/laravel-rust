//! Laravel-flavoured terminal output: colors, the logo, and the console
//! components (`INFO`, `WARN`, `ERROR` lines and `DONE` tasks) you know
//! from Artisan.

use std::fmt::{Display, Write as _};
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::terminal;

static COLORS: AtomicBool = AtomicBool::new(false);

/// How many newlines the output currently ends with (Symfony starts at 1).
static NEW_LINES: AtomicUsize = AtomicUsize::new(1);

const LARAVEL_WORDMARK: [&str; 6] = [
    "██╗       █████╗  ██████╗   █████╗  ██╗   ██╗ ███████╗ ██╗",
    "██║      ██╔══██╗ ██╔══██╗ ██╔══██╗ ██║   ██║ ██╔════╝ ██║",
    "██║      ███████║ ██████╔╝ ███████║ ██║   ██║ █████╗   ██║",
    "██║      ██╔══██║ ██╔══██╗ ██╔══██║ ╚██╗ ██╔╝ ██╔══╝   ██║",
    "███████╗ ██║  ██║ ██║  ██║ ██║  ██║  ╚████╔╝  ███████╗ ███████╗",
    "╚══════╝ ╚═╝  ╚═╝ ╚═╝  ╚═╝ ╚═╝  ╚═╝   ╚═══╝   ╚══════╝ ╚══════╝",
];
const BANNER_COLORS: [u8; LARAVEL_WORDMARK.len()] = [209, 203, 202, 208, 214, 215];

/// Turn colors on or off for everything written from now on.
pub fn set_colors(enabled: bool) {
    COLORS.store(enabled, Ordering::Relaxed);
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

pub fn cyan(text: impl Display) -> String {
    paint(text, "36", "39")
}

pub fn gray(text: impl Display) -> String {
    paint(text, "90", "39")
}

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

/// A terminal hyperlink (OSC 8), falling back to the plain URL.
pub fn link(text: &str, url: &str) -> String {
    if colors() {
        format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
    } else {
        url.to_string()
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

/// Introduce Laravel Rust, using a compact banner in narrow terminals and logs.
pub fn logo() {
    write(&format!(
        "{}{}\n",
        margin_top(),
        render_banner(terminal::width(), colors())
    ));
}

fn render_banner(columns: usize, decorated: bool) -> String {
    let title = bold("Laravel Rust");
    if !decorated
        || LARAVEL_WORDMARK
            .iter()
            .any(|row| row.chars().count() + 2 >= columns)
    {
        return format!("  {title}\n");
    }

    let mut banner = String::new();
    for (row, color) in LARAVEL_WORDMARK.into_iter().zip(BANNER_COLORS) {
        writeln!(banner, "  {}", paint(row, &format!("38;5;{color}"), "39")).unwrap();
    }
    writeln!(banner, "  {title}").unwrap();
    banner
}

/// Show the new application's location and the commands to start it.
pub fn application_summary(name: &str, complete: bool) {
    write(&format!(
        "{}{}\n",
        margin_top(),
        render_summary(name, complete, terminal::width(), colors())
    ));
}

fn render_summary(name: &str, complete: bool, columns: usize, decorated: bool) -> String {
    let title = if complete {
        "Application ready"
    } else {
        "Application created"
    };
    let details = [
        format!("Path  ./{name}"),
        format!("cd {name}"),
        "cargo artisan dev".into(),
    ];
    let length = |text: &str| text.chars().count();
    let width = details
        .iter()
        .map(|line| length(line))
        .max()
        .unwrap_or(0)
        .max(length(title) + 3);
    let heading = if complete {
        green(bold(title))
    } else {
        yellow(bold(title))
    };
    if !decorated || width + 6 > columns {
        return format!("  {heading}\n    {}\n", details.join("\n    "));
    }

    let mut summary = format!("  ┌ {heading} {}┐\n", "─".repeat(width - length(title)));
    for detail in details {
        writeln!(summary, "  │ {detail:width$} │").unwrap();
    }
    writeln!(summary, "  └{}┘", "─".repeat(width + 2)).unwrap();
    summary
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
    line(&task_result(description, started.elapsed(), result.is_ok()));
    result
}

/// The time and final status of a task, aligned to the terminal's width.
pub fn task_result(description: &str, elapsed: Duration, succeeded: bool) -> String {
    let runtime = runtime_for_humans(elapsed);
    let width = terminal::width().min(150);
    let dots = width.saturating_sub(description.chars().count() + runtime.len() + 10);
    let status = if succeeded {
        green(bold("DONE"))
    } else {
        paint(bold("FAIL"), "31", "39")
    };
    format!("{} {} {status}", gray(".".repeat(dots)), gray(runtime))
}

fn runtime_for_humans(elapsed: Duration) -> String {
    let milliseconds = elapsed.as_secs_f64() * 1000.0;
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
    fn the_banner_fits_narrow_and_plain_terminals() {
        assert_eq!(render_banner(50, true), "  Laravel Rust\n");
        assert_eq!(render_banner(80, false), "  Laravel Rust\n");
        let banner = render_banner(80, true);
        assert!(banner.starts_with("  ██╗       █████╗"));
        assert!(banner.ends_with("  Laravel Rust\n"));
        assert!(banner.lines().all(|line| line.chars().count() < 80));
        assert_eq!(banner.lines().count(), 7);
    }

    #[test]
    fn the_summary_keeps_commands_readable_in_each_layout() {
        let wide = render_summary("podcasts", true, 80, true);
        assert!(wide.starts_with("  ┌ Application ready "));
        assert!(wide.contains("cd podcasts"));
        assert!(wide.contains("cargo artisan dev"));
        assert!(
            wide.lines()
                .all(|line| line.chars().count() == wide.lines().next().unwrap().chars().count())
        );

        let narrow = render_summary("my-application-with-a-longer-name", false, 32, true);
        assert!(narrow.starts_with("  Application created\n"));
        assert!(narrow.contains("cd my-application-with-a-longer-name"));
        assert!(!narrow.contains('│'));
    }
}
