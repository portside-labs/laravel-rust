//! The Laravel installer, for Rust.
//!
//! ```text
//! curl -fsSL https://portsidelabs.io/laravel-rust/install.sh | sh
//! laravel-rust new example-app
//! ```
//!
//! The command is `laravel-rust`, so it sits alongside the PHP installer's
//! `laravel` command. `laravel-rust new` copies the application skeleton
//! (embedded in this binary at build time), names the crate after the
//! application, writes its `.env`, and builds it — so a new application is
//! one command away.

mod cli;
// Shared with build.rs, which uses it to decide what to embed.
#[cfg(test)]
mod filter;
mod framework;
mod new;
mod output;
mod prompts;
mod scaffold;
mod skeleton;
mod terminal;
mod toml;

use std::process::ExitCode;

use cli::Command;

fn main() -> ExitCode {
    #[cfg(unix)]
    terminal::install_panic_hook();
    #[cfg(windows)]
    let _console = terminal::enable_ansi();

    let arguments: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(run(&arguments) as u8)
}

/// Decide whether to use colors: `--ansi` / `--no-ansi`, then `NO_COLOR` /
/// `FORCE_COLOR`, then whether standard output is a terminal.
fn use_colors(ansi: Option<bool>) -> bool {
    ansi.unwrap_or_else(|| {
        if std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()) {
            false
        } else if std::env::var_os("FORCE_COLOR")
            .is_some_and(|value| !value.is_empty() && value != "0")
        {
            true
        } else {
            terminal::supports_ansi()
        }
    })
}

fn run(arguments: &[String]) -> i32 {
    let cli = match cli::parse(arguments) {
        Ok(cli) => cli,
        Err(message) => {
            let ansi = arguments
                .iter()
                .rev()
                .find_map(|argument| match argument.as_str() {
                    "--ansi" => Some(true),
                    "--no-ansi" => Some(false),
                    _ => None,
                });
            output::set_colors(use_colors(ansi));
            output::error(&message);
            return 1;
        }
    };

    output::set_colors(use_colors(cli.ansi));

    match cli.command {
        Command::Version => output::write(&cli::version()),
        Command::List => output::write(&cli::list()),
        Command::Help(command) => output::write(&cli::help(&command)),
        Command::New(options) => {
            let interactive = cli.interaction && terminal::is_interactive();
            match new::run(options, interactive) {
                Ok(()) => {}
                Err(new::Failure::Cancelled) => return 1,
                Err(new::Failure::Error(message)) => {
                    output::error(&message);
                    return 1;
                }
            }
        }
    }

    0
}
