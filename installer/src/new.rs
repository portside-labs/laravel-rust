//! `laravel-rust new`: create a new Laravel application.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use crate::cli::NewOptions;
use crate::framework::Framework;
use crate::output::{self, AssetSetup, bold, gray, link};
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
        None => false,
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

    let assets = if !npm {
        AssetSetup::Skipped
    } else if install_npm_dependencies(&directory, interactive) {
        AssetSetup::Installed
    } else {
        AssetSetup::Failed
    };

    let built = build(&directory, &name, driver, interactive);

    // `key:generate` normally sets the key; make sure there is one either way.
    let _ = scaffold::ensure_app_key(&directory);
    let app_configured = built?;
    output::application_summary(&name, app_configured, assets);
    output::line(&format!(
        "  {}",
        link("Documentation", "https://laravel.com/docs")
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

/// Run a command, giving it terminal input only during an interactive install.
fn execute(
    description: &str,
    display: &str,
    command: &mut Command,
    interactive: bool,
) -> Result<bool, std::io::Error> {
    output::write(&format!(
        "{}  {}\n",
        output::margin_top(),
        bold(description)
    ));
    output::line(&format!("  {}", gray(format!("> {display}"))));
    if !interactive {
        command.stdin(Stdio::null());
    }
    let started = Instant::now();
    let result = command.status().map(|status| status.success());
    output::assume_new_lines(1);
    let status = output::task_result(description, started.elapsed(), matches!(result, Ok(true)));
    output::line(&format!("  {description} {status}"));
    result
}

/// Find npm on the `PATH`, including the Windows launcher installed by Node.
fn find_npm(paths: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    #[cfg(windows)]
    let names = ["npm.exe", "npm.cmd", "npm.bat"];
    #[cfg(not(windows))]
    let names = ["npm"];

    paths
        .into_iter()
        .flat_map(|directory| names.into_iter().map(move |name| directory.join(name)))
        .find(|candidate| candidate.is_file())
        .and_then(|candidate| std::path::absolute(candidate).ok())
}

/// `npm install` and `npm run build`.
fn install_npm_dependencies(directory: &Path, interactive: bool) -> bool {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let Some(npm) = find_npm(std::env::split_paths(&path)) else {
        output::warn("Unable to find [npm]. Skipping [npm install] and [npm run build]");
        return false;
    };

    for (description, display, arguments) in [
        (
            "Installing frontend dependencies",
            "npm install",
            &["install"][..],
        ),
        (
            "Building frontend assets",
            "npm run build",
            &["run", "build"][..],
        ),
    ] {
        let succeeded = execute(
            description,
            display,
            Command::new(&npm).args(arguments).current_dir(directory),
            interactive,
        );
        match succeeded {
            Ok(true) => {}
            Ok(false) => {
                output::warn(&format!(
                    "[{display}] failed. You can run it again from your application's directory"
                ));
                return false;
            }
            Err(error) => {
                output::warn(&format!("Unable to run [{display}]: {error}"));
                return false;
            }
        }
    }
    true
}

/// The `cargo` to run: the one running us (when there is one), or the PATH's.
fn cargo() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

/// Build the application, then set its key and (for SQLite) migrate.
fn build(directory: &Path, name: &str, database: &str, interactive: bool) -> Result<bool, String> {
    let (color, ansi) = if output::colors() {
        ("--color=always", "--ansi")
    } else {
        ("--color=never", "--no-ansi")
    };

    let built = execute(
        "Building Artisan",
        "cargo build --bin artisan",
        Command::new(cargo())
            .args(["build", "--bin", "artisan", color])
            .current_dir(directory),
        interactive,
    );
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

    let mut commands = vec![("Generating the application key", "key:generate")];
    if database == "sqlite" {
        commands.push(("Running SQLite migrations", "migrate"));
    }

    let mut configured = true;
    for (description, artisan) in commands {
        let mut arguments = vec![artisan, ansi];
        if !interactive {
            arguments.push("--no-interaction");
        }
        let display = format!("cargo artisan {}", arguments.join(" "));
        let succeeded = execute(
            description,
            &display,
            Command::new(cargo())
                .args(["run", "--quiet", "--bin", "artisan", color, "--"])
                .args(arguments)
                .current_dir(directory),
            interactive,
        );
        if !matches!(succeeded, Ok(true)) {
            configured = false;
            output::warn(&format!(
                "[{display}] did not succeed. You can run it again from your application's directory"
            ));
        }
    }

    Ok(configured)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn npm_uses_the_windows_launcher_from_the_selected_directory() {
        let mut bytes = [0; 8];
        getrandom::fill(&mut bytes).unwrap();
        let root =
            std::env::temp_dir().join(format!("laravel-npm-{:x}", u64::from_ne_bytes(bytes)));
        let bin = root.join("bin with spaces");
        let app = root.join("application");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir(&app).unwrap();
        fs::write(bin.join("npm"), "#!/bin/sh\n").unwrap();
        fs::write(bin.join("npm.cmd"), "@echo off\r\n>> npm.log echo %*\r\n").unwrap();

        let npm = find_npm([bin.clone()]).unwrap();
        assert_eq!(npm, bin.join("npm.cmd"));
        for arguments in [&["install"][..], &["run", "build"][..]] {
            assert!(
                Command::new(&npm)
                    .args(arguments)
                    .current_dir(&app)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let log = app.join("npm.log");
        assert_eq!(
            fs::read_to_string(&log)
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            ["install", "run build"]
        );

        fs::remove_file(log).unwrap();
        fs::remove_file(bin.join("npm")).unwrap();
        fs::remove_file(npm).unwrap();
        fs::remove_dir(app).unwrap();
        fs::remove_dir(bin).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
