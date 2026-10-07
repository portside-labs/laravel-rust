//! Embed the application skeleton (and the framework's `Cargo.lock` and
//! workspace manifest) into the installer, so `laravel-rust new` works from
//! a single binary.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "src/filter.rs"]
mod filter;

fn main() {
    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .parent()
        .expect("the installer lives inside the framework")
        .to_path_buf();
    let skeleton = root.join("skeleton");

    assert!(
        skeleton.join("Cargo.toml").is_file(),
        "The application skeleton was not found at [{}].",
        skeleton.display()
    );

    println!("cargo:rerun-if-changed={}", skeleton.display());

    // New applications may depend on the checkout the installer was built
    // from (src/framework.rs). A copy Cargo downloaded for `cargo install
    // --git` (marked with `.cargo-ok`) doesn't count: Cargo cleans those up,
    // so applications depend on the Git repository instead.
    let checkout = if root.join(".cargo-ok").exists() {
        String::new()
    } else {
        root.display().to_string()
    };
    println!("cargo:rustc-env=LARAVEL_FRAMEWORK_CHECKOUT={checkout}");

    let mut files = Vec::new();
    collect(&skeleton, "", &mut files);
    files.sort();

    let mut generated = String::from(
        "/// The application skeleton, embedded at build time.\npub static FILES: &[File] = &[\n",
    );
    for relative in &files {
        let absolute = skeleton.join(relative);
        println!("cargo:rerun-if-changed={}", absolute.display());
        writeln!(
            generated,
            "    File {{ path: {relative:?}, contents: include_bytes!({:?}), executable: {} }},",
            absolute.display().to_string(),
            is_executable(&absolute),
        )
        .unwrap();
    }
    generated.push_str("];\n\n");

    for (constant, file, doc) in [
        (
            "CARGO_LOCK",
            "Cargo.lock",
            "The framework's `Cargo.lock`: the versions the framework is tested with.",
        ),
        (
            "WORKSPACE_MANIFEST",
            "Cargo.toml",
            "The framework's workspace manifest (shared dependency specs and profiles).",
        ),
    ] {
        let path = root.join(file);
        println!("cargo:rerun-if-changed={}", path.display());
        writeln!(
            generated,
            "/// {doc}\npub static {constant}: &str = include_str!({:?});\n",
            path.display().to_string()
        )
        .unwrap();
    }

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("skeleton.rs");
    fs::write(out, generated).expect("Unable to write the embedded skeleton.");
}

/// Collect the skeleton's files, relative to the skeleton and `/`-separated.
fn collect(directory: &Path, prefix: &str, files: &mut Vec<String>) {
    let mut entries: Vec<_> = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("Unable to read [{}]: {error}", directory.display()))
        .map(|entry| entry.expect("directory entry"))
        .collect();
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let file_type = entry.file_type().expect("file type");

        if file_type.is_dir() {
            if !filter::is_ignored_directory(&relative) {
                collect(&entry.path(), &relative, files);
            }
        } else if file_type.is_file() && filter::should_embed(&relative) {
            files.push(relative);
        }
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    false
}
