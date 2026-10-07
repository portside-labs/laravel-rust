//! Where a new application gets the framework from.
//!
//! An installer built from a local checkout of the framework points new
//! applications at that checkout (`laravel = { path = "..." }`), so they
//! build against exactly the code the installer came from — offline, and
//! with any local changes. When that checkout is gone, or the installer was
//! installed with `cargo install --git` (as the install script does),
//! applications depend on the framework's Git repository instead.
//! `--path=<dir>` picks a checkout explicitly.

use std::path::{Path, PathBuf};

use crate::toml::quote;

/// The framework checkout this installer was built from: empty when Cargo
/// downloaded the framework to build it (see build.rs).
const BUILT_FROM: &str = env!("LARAVEL_FRAMEWORK_CHECKOUT");

/// The framework's repository.
pub const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

/// The source of the `laravel` crates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Framework {
    /// A local checkout of the framework (its root directory).
    Path(PathBuf),
    /// The framework's Git repository.
    Git(String),
}

impl Framework {
    /// Pick the framework source: `--path` when given, then the checkout
    /// the installer was built from, then the Git repository.
    pub fn resolve(path: Option<&str>, working_directory: &Path) -> Result<Framework, String> {
        if let Some(path) = path {
            let directory = working_directory.join(path);
            return match directory.canonicalize() {
                Ok(directory) if is_framework(&directory) => Ok(Framework::Path(directory)),
                _ => Err(format!(
                    "The directory [{path}] is not a checkout of the Laravel framework"
                )),
            };
        }

        Ok(Some(BUILT_FROM)
            .filter(|built_from| !built_from.is_empty())
            .and_then(|built_from| Path::new(built_from).canonicalize().ok())
            .filter(|directory| is_framework(directory))
            .map_or_else(|| Framework::Git(REPOSITORY.to_string()), Framework::Path))
    }

    /// The dependency spec for a framework crate, given its path relative to
    /// the framework's root (`.` for `laravel`, `illuminate/build`, ...).
    pub fn dependency(&self, relative: &str) -> Vec<(String, String)> {
        match self {
            Framework::Path(root) => {
                let relative = relative.trim_start_matches("./").trim_end_matches('/');
                let path = if relative.is_empty() || relative == "." {
                    root.clone()
                } else {
                    root.join(relative)
                };
                vec![("path".into(), quote(&path.to_string_lossy()))]
            }
            Framework::Git(url) => vec![("git".into(), quote(url))],
        }
    }
}

/// Determine whether the directory holds the framework (its root manifest
/// is the `laravel` package).
pub fn is_framework(directory: &Path) -> bool {
    let Ok(manifest) = std::fs::read_to_string(directory.join("Cargo.toml")) else {
        return false;
    };
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package
            && let Some(("name", value)) = line
                .split_once('=')
                .map(|(key, value)| (key.trim(), value.trim()))
        {
            return value == "\"laravel\"";
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checkout_the_installer_was_built_from_is_used() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        assert!(is_framework(root));
        assert!(!is_framework(Path::new(env!("CARGO_MANIFEST_DIR"))));
        assert_eq!(
            Framework::resolve(None, Path::new("/")).unwrap(),
            Framework::Path(root.canonicalize().unwrap())
        );
    }

    #[test]
    fn a_checkout_may_be_given_explicitly() {
        let installer = Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = installer.parent().unwrap().canonicalize().unwrap();
        assert_eq!(
            Framework::resolve(Some(".."), installer).unwrap(),
            Framework::Path(root)
        );
        assert!(Framework::resolve(Some("src"), installer).is_err());
        assert!(Framework::resolve(Some("/does/not/exist"), installer).is_err());
    }

    #[test]
    fn dependencies_point_at_the_framework() {
        let local = Framework::Path(PathBuf::from("/code/laravel-rust"));
        assert_eq!(
            local.dependency("."),
            [("path".to_string(), r#""/code/laravel-rust""#.to_string())]
        );
        assert_eq!(
            local.dependency("illuminate/build"),
            [(
                "path".to_string(),
                r#""/code/laravel-rust/illuminate/build""#.to_string()
            )]
        );

        let git = Framework::Git(REPOSITORY.into());
        assert_eq!(
            git.dependency("illuminate/build"),
            [(
                "git".to_string(),
                r#""https://github.com/portside-labs/laravel-rust""#.to_string()
            )]
        );
    }
}
