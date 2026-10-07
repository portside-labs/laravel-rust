//! `laravel-rust new`: create a new Laravel application.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cli::NewOptions;
use crate::framework::Framework;
use crate::output::{self, bold, gray, link};
use crate::prompts::{self, Cancelled};
use crate::scaffold::{self, Blueprint, DATABASES};

/// Why `laravel-rust new` stopped early.
pub enum Failure {
    /// Something went wrong: shown as an `ERROR` line.
    Error(String),
    /// The user pressed Ctrl+C at a prompt (the prompt said so already).
    Cancelled,
}

impl From<Cancelled> for Failure {
    fn from(_: Cancelled) -> Self {
        Failure::Cancelled
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Failure::Error(message)
    }
}

/// Create a new application.
pub fn run(options: NewOptions, interactive: bool) -> Result<(), Failure> {
    let working_directory = std::env::current_dir()
        .map_err(|error| format!("Unable to read the current directory: {error}"))?;

    output::logo();

    let name = match options.name {
        Some(name) => {
            scaffold::validate_name(&name)?;
            name
        }
        None if interactive => prompts::text(
            "What is the name of your project?",
            "E.g. example-app",
            |name| {
                scaffold::validate_name(name)
                    .and_then(|_| {
                        ensure_available(
                            &working_directory.join(name),
                            &working_directory,
                            options.force,
                        )
                    })
                    .err()
            },
        )?
        .ok_or_else(|| "Not enough arguments (missing: \"name\")".to_string())?,
        None => {
            return Err(Failure::Error(
                "Not enough arguments (missing: \"name\")".into(),
            ));
        }
    };

    let database = match options.database.as_deref() {
        Some(database) => DATABASES
            .iter()
            .position(|(driver, _)| *driver == database)
            .ok_or_else(|| {
                let drivers: Vec<&str> = DATABASES.iter().map(|(driver, _)| *driver).collect();
                format!(
                    "Invalid database driver [{database}]. Possible values are: {}",
                    drivers.join(", ")
                )
            })?,
        None => 0,
    };

    let framework = Framework::resolve(options.path.as_deref(), &working_directory)?;
    let directory = working_directory.join(&name);
    ensure_available(&directory, &working_directory, options.force)?;

    let database = if options.database.is_none() && interactive {
        let labels: Vec<&str> = DATABASES.iter().map(|(_, label)| *label).collect();
        prompts::select(
            "Which database will your application use?",
            &labels,
            database,
        )?
    } else {
        database
    };
    let (driver, _) = DATABASES[database];

    let npm = match options.npm {
        Some(npm) => npm,
        None if interactive => prompts::confirm(
            &format!(
                "Would you like to run {} and {}?",
                bold("npm install"),
                bold("npm run build")
            ),
            true,
        )?,
        None => true,
    };

    output::info(&format!("Creating a \"{name}\" application in [./{name}]"));

    if options.force && directory.symlink_metadata().is_ok() {
        remove(&directory)?;
    }

    let blueprint = Blueprint {
        name: &name,
        database: driver,
        framework: &framework,
    };
    output::task("Copying the application skeleton", || {
        scaffold::copy_skeleton(&directory, &blueprint)
    })
    .map_err(|error| format!("Unable to create the application: {error}"))?;
    output::task("Creating the environment file", || {
        scaffold::configure_environment(&directory, driver)
    })
    .map_err(|error| format!("Unable to create the environment file: {error}"))?;

    if npm {
        install_npm_dependencies(&directory);
    }

    let built = build(&directory, &name, driver);

    // `key:generate` normally sets the key; make sure there is one either way.
    let _ = scaffold::ensure_app_key(&directory);
    built?;

    output::write(&format!(
        "{}{}\n",
        output::margin_top(),
        output::render_line(
            "INFO",
            "37;44",
            &format!("Application ready in [{name}]. You can start your local development using:")
        )
    ));
    output::line(&format!("{} {}", gray("➜"), bold(format!("cd {name}"))));
    output::line(&format!("{} {}", gray("➜"), bold("cargo artisan dev")));
    output::line("");
    output::line(&format!(
        "  New to Laravel? Check out our {}. {}",
        link("documentation", "https://laravel.com/docs"),
        bold("Build something amazing!")
    ));
    output::line("");

    Ok(())
}

/// Fail when the directory already exists, unless `--force` may replace it
/// (which it never may when we're standing inside it).
fn ensure_available(directory: &Path, working_directory: &Path, force: bool) -> Result<(), String> {
    if directory.symlink_metadata().is_err() {
        return Ok(());
    }

    let name = directory
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !force {
        return Err(format!(
            "Application already exists at [./{name}]. Use [--force] to replace it"
        ));
    }

    let target = directory
        .canonicalize()
        .unwrap_or_else(|_| directory.to_path_buf());
    let here = working_directory
        .canonicalize()
        .unwrap_or_else(|_| working_directory.to_path_buf());
    if here.starts_with(&target) {
        return Err(format!(
            "Refusing to replace [./{name}] because it contains the current directory"
        ));
    }

    Ok(())
}

/// Remove an existing application (`--force`).
fn remove(directory: &Path) -> Result<(), String> {
    let metadata = directory
        .symlink_metadata()
        .map_err(|error| error.to_string())?;
    let removed = if metadata.is_dir() {
        std::fs::remove_dir_all(directory)
    } else {
        std::fs::remove_file(directory)
    };
    removed.map_err(|error| format!("Unable to remove [{}]: {error}", directory.display()))
}

/// Print a command the way Composer prints the scripts it runs, then run it
/// with the terminal handed over to it.
fn execute(display: &str, command: &mut Command) -> Result<bool, std::io::Error> {
    output::line(&format!("> {display}"));
    let status = command.status()?;
    Ok(status.success())
}

/// Find a program on the `PATH`.
fn which(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|directory| directory.join(program))
            .find(|candidate| candidate.is_file())
    })
}

/// `npm install` and `npm run build`.
fn install_npm_dependencies(directory: &Path) {
    let Some(npm) = which("npm") else {
        output::warn("Unable to find [npm]. Skipping [npm install] and [npm run build]");
        return;
    };

    output::write(&output::margin_top());
    for (display, arguments) in [
        ("npm install", &["install"][..]),
        ("npm run build", &["run", "build"][..]),
    ] {
        let succeeded = execute(
            display,
            Command::new(&npm).args(arguments).current_dir(directory),
        );
        output::assume_new_lines(1);
        match succeeded {
            Ok(true) => {}
            Ok(false) => {
                output::warn(&format!(
                    "[{display}] failed. You can run it again from your application's directory"
                ));
                return;
            }
            Err(error) => {
                output::warn(&format!("Unable to run [{display}]: {error}"));
                return;
            }
        }
    }
}

/// The `cargo` to run: the one running us (when there is one), or the PATH's.
fn cargo() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

/// Build the application, then set its key and (for SQLite) migrate.
fn build(directory: &Path, name: &str, database: &str) -> Result<(), String> {
    let (color, ansi) = if output::colors() {
        ("--color=always", "--ansi")
    } else {
        ("--color=never", "--no-ansi")
    };

    output::write(&output::margin_top());
    let built = execute(
        "cargo build --bin artisan",
        Command::new(cargo())
            .args(["build", "--bin", "artisan", color])
            .current_dir(directory),
    );
    output::assume_new_lines(1);
    match built {
        Ok(true) => {}
        Ok(false) => {
            return Err(format!(
                "The application could not be built. It's in [{name}]: fix the errors above, then run [cargo build]"
            ));
        }
        Err(error) => {
            return Err(format!(
                "Unable to run [cargo] ({error}). Install Rust from https://rustup.rs, then run [cargo build] in [{name}]"
            ));
        }
    }

    let mut commands = vec![vec!["key:generate", ansi]];
    if database == "sqlite" {
        commands.push(vec!["migrate", "--graceful", ansi]);
    }

    for arguments in commands {
        let display = format!("cargo artisan {}", arguments.join(" "));
        let succeeded = execute(
            &display,
            Command::new(cargo())
                .args(["run", "--quiet", "--bin", "artisan", color, "--"])
                .args(&arguments)
                .current_dir(directory),
        );
        // Artisan's components end with a blank line.
        output::assume_new_lines(2);
        if !matches!(succeeded, Ok(true)) {
            output::warn(&format!(
                "[{display}] did not succeed. You can run it again from your application's directory"
            ));
        }
    }

    Ok(())
}
