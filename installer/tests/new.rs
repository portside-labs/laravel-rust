#![cfg(any(unix, windows))]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Installation {
    directory: PathBuf,
    cargo: PathBuf,
}

impl Installation {
    fn new() -> Self {
        let mut bytes = [0; 8];
        getrandom::fill(&mut bytes).unwrap();
        let directory =
            std::env::temp_dir().join(format!("laravel-new-{:x}", u64::from_ne_bytes(bytes)));
        fs::create_dir(&directory).unwrap();
        let cargo = launcher(&directory, "cargo", false);
        launcher(&directory, "npm", true);
        Self { directory, cargo }
    }

    fn run(&self, options: &[&str], npm_exit: &str) -> String {
        let input = self.directory.join("terminal-input");
        fs::write(&input, "unexpected input\n").unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_laravel-rust"))
            .args(["new", "example-app", "--no-interaction", "--no-ansi"])
            .args(options)
            .current_dir(&self.directory)
            .stdin(fs::File::open(input).unwrap())
            .env("CARGO", &self.cargo)
            .env("PATH", &self.directory)
            .env("TEST_NPM_EXIT", npm_exit)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn application_file(&self, name: &str) -> PathBuf {
        self.directory.join("example-app").join(name)
    }
}

impl Drop for Installation {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn launcher(directory: &Path, name: &str, is_npm: bool) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let path = directory.join(name);
        let exit = if is_npm { "$TEST_NPM_EXIT" } else { "0" };
        fs::write(
            &path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {name}-arguments\nif IFS= read -r answer; then\n    printf '%s\\n' \"$answer\" > {name}-stdin\nfi\nexit \"{exit}\"\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
    #[cfg(windows)]
    {
        let path = directory.join(format!("{name}.cmd"));
        let exit = if is_npm { "%TEST_NPM_EXIT%" } else { "0" };
        fs::write(
            &path,
            format!(
                "@echo off\r\n>> {name}-arguments echo %*\r\nset \"captured_input=\"\r\nset /p \"captured_input=\"\r\nif defined captured_input > {name}-stdin echo received input\r\nexit /b {exit}\r\n"
            ),
        )
        .unwrap();
        path
    }
}

#[test]
fn non_interactive_installs_run_artisan_without_bypassing_production_checks() {
    let installation = Installation::new();
    installation.run(&["--no-npm"], "0");

    let arguments = fs::read_to_string(installation.application_file("cargo-arguments")).unwrap();
    for command in ["key:generate", "migrate"] {
        let line = arguments
            .lines()
            .find(|line| line.split_whitespace().any(|arg| arg == command))
            .unwrap();
        let arguments: Vec<_> = line.split_whitespace().collect();
        assert!(arguments.contains(&"--no-interaction"), "{line}");
        assert!(!arguments.contains(&"--force"), "{line}");
    }
}

#[test]
fn non_interactive_installs_close_stdin_for_subprocesses() {
    for (option, command) in [("--no-npm", "cargo"), ("--npm", "npm")] {
        let installation = Installation::new();
        installation.run(&[option], "0");
        assert!(
            !installation
                .application_file(&format!("{command}-stdin"))
                .exists(),
            "{command} received input during a non-interactive installation"
        );
    }
}

#[test]
fn non_interactive_installs_only_run_npm_when_requested() {
    let default = Installation::new();
    default.run(&[], "0");
    assert!(!default.application_file("npm-arguments").exists());

    let explicit = Installation::new();
    let output = explicit.run(&["--npm"], "0");
    let arguments = fs::read_to_string(explicit.application_file("npm-arguments")).unwrap();
    assert_eq!(
        arguments.lines().collect::<Vec<_>>(),
        ["install", "run build"]
    );
    assert!(
        !output
            .split_once("Application ready\n")
            .unwrap()
            .1
            .contains("npm install")
    );
}

#[test]
fn the_summary_includes_asset_setup_when_npm_is_skipped_or_fails() {
    for (option, title) in [
        ("--no-npm", "Application ready"),
        ("--npm", "Application created"),
    ] {
        let installation = Installation::new();
        let output = installation.run(&[option], "1");
        let summary = output.split_once(title).unwrap().1;
        assert_eq!(
            summary
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .take(5)
                .collect::<Vec<_>>(),
            [
                "Path  ./example-app",
                "cd example-app",
                "npm install",
                "npm run build",
                "cargo artisan dev"
            ]
        );
    }
}
