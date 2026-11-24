//! The application skeleton, embedded by `build.rs` from `../skeleton`.

/// A file of the skeleton.
pub struct File {
    /// The path relative to the application's root, `/`-separated.
    pub path: &'static str,
    pub contents: &'static [u8],
    #[cfg_attr(not(unix), allow(dead_code))]
    pub executable: bool,
}

include!(concat!(env!("OUT_DIR"), "/skeleton.rs"));

/// Find a skeleton file by its path.
pub fn file(path: &str) -> Option<&'static [u8]> {
    FILES
        .iter()
        .find(|file| file.path == path)
        .map(|file| file.contents)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::*;

    #[test]
    fn the_skeleton_is_embedded() {
        assert!(file("Cargo.toml").is_some());
        assert!(file("artisan.rs").is_some());
        assert!(file(".env.example").is_some());
        assert!(file(".env").is_none());
        assert!(CARGO_LOCK.contains("name = \"laravel\""));
        assert!(WORKSPACE_MANIFEST.contains("[workspace.dependencies]"));
    }

    /// The embedded files are exactly the ones Git tracks.
    #[test]
    fn the_embedded_files_match_git() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let Ok(output) = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["ls-files", "skeleton"])
            .output()
        else {
            return;
        };
        if !output.status.success() {
            return;
        }

        let tracked: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| line.trim_start_matches("skeleton/").to_string())
            .filter(|path| root.join("skeleton").join(path).is_file())
            .collect();
        let mut embedded: Vec<String> = FILES.iter().map(|file| file.path.to_string()).collect();
        embedded.sort();
        let mut tracked = tracked;
        tracked.sort();

        assert_eq!(embedded, tracked);
    }
}
