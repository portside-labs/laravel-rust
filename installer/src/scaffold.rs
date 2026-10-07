//! Turning the skeleton into a new application: names, `Cargo.toml`, the
//! crate's identifier, and the environment file.

use std::fs;
use std::io;
use std::path::Path;

use base64::Engine;

use crate::framework::Framework;
use crate::skeleton;
use crate::toml;

/// The databases an application can start with: `(driver, label)`.
pub const DATABASES: [(&str, &str); 4] = [
    ("sqlite", "SQLite"),
    ("mysql", "MySQL"),
    ("mariadb", "MariaDB"),
    ("pgsql", "PostgreSQL"),
];

/// Names a crate can't have: Rust's keywords, the standard library, the
/// test harness, and the crates every application depends on.
const RESERVED: &[&str] = &[
    "abstract",
    "alloc",
    "as",
    "async",
    "await",
    "become",
    "box",
    "break",
    "const",
    "continue",
    "core",
    "crate",
    "do",
    "dyn",
    "else",
    "enum",
    "extern",
    "false",
    "final",
    "fn",
    "for",
    "gen",
    "if",
    "impl",
    "in",
    "laravel",
    "laravel_build",
    "let",
    "loop",
    "macro",
    "match",
    "mod",
    "move",
    "mut",
    "override",
    "priv",
    "proc_macro",
    "pub",
    "ref",
    "return",
    "self",
    "Self",
    "serde",
    "static",
    "std",
    "struct",
    "super",
    "test",
    "tokio",
    "trait",
    "true",
    "try",
    "type",
    "typeof",
    "unsafe",
    "unsized",
    "use",
    "virtual",
    "where",
    "while",
    "yield",
];

/// Check an application name: it becomes a directory, a Cargo package, and
/// a Rust crate, so it must work as all three.
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Please provide a name for your application".into());
    }

    let valid_characters = name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_');
    if !valid_characters {
        return Err("The name may only contain letters, numbers, dashes, and underscores".into());
    }
    if !name.starts_with(|character: char| character.is_ascii_alphabetic()) {
        return Err("The name must start with a letter".into());
    }

    if RESERVED.contains(&crate_identifier(name).as_str()) {
        return Err(format!(
            "The name [{name}] is reserved. Please choose another name"
        ));
    }

    Ok(())
}

/// The crate's identifier in Rust code: `my-app` → `my_app`.
pub fn crate_identifier(name: &str) -> String {
    name.replace('-', "_")
}

/// A human friendly version of the name: `my-app` → `My App`.
pub fn headline(name: &str) -> String {
    name.split(['-', '_'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut characters = word.chars();
            characters
                .next()
                .map(|first| first.to_uppercase().chain(characters).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Replace a Rust identifier, leaving longer identifiers that contain it alone.
pub fn replace_identifier(source: &str, from: &str, to: &str) -> String {
    let is_identifier = |character: char| character.is_alphanumeric() || character == '_';
    let mut replaced = String::with_capacity(source.len());
    let mut rest = source;

    while let Some(index) = rest.find(from) {
        let before = rest[..index].chars().next_back();
        let after = rest[index + from.len()..].chars().next();
        replaced.push_str(&rest[..index]);
        if before.is_some_and(is_identifier) || after.is_some_and(is_identifier) {
            replaced.push_str(from);
        } else {
            replaced.push_str(to);
        }
        rest = &rest[index + from.len()..];
    }

    replaced.push_str(rest);
    replaced
}

/// The package name declared by a manifest.
pub fn package_name(manifest: &str) -> Option<String> {
    toml::table_entries(manifest, "package")
        .into_iter()
        .find(|(key, _)| key == "name")
        .map(|(_, value)| value.trim_matches('"').to_string())
}

fn is_dependency_table(table: &str) -> bool {
    table.ends_with("dependencies]") && !table.starts_with("[workspace")
}

/// Write the application's `Cargo.toml`: the skeleton's manifest with the
/// application's name, and concrete versions in place of the workspace's.
///
/// Dependencies the skeleton inherits from the framework's workspace get
/// the workspace's spec (so they never drift apart); framework crates point
/// at the framework source. The workspace's `[profile.*]` tables come along
/// too, so the application builds just like the skeleton does.
pub fn manifest(
    skeleton: &str,
    workspace: &str,
    name: &str,
    framework: &Framework,
    standalone: bool,
) -> Result<String, String> {
    let dependencies = toml::table_entries(workspace, "workspace.dependencies");
    let package = toml::table_entries(workspace, "workspace.package");
    let mut table = String::new();
    let mut lines = Vec::new();

    for line in skeleton.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            table = trimmed.to_string();
            lines.push(line.to_string());
            continue;
        }

        let Some((key, value)) = toml::key_value(trimmed) else {
            lines.push(line.to_string());
            continue;
        };

        if table == "[package]" {
            let line = match key.as_str() {
                "name" => format!("name = {}", toml::quote(name)),
                "description" => format!(
                    "description = {}",
                    toml::quote(&format!("The {} application.", headline(name)))
                ),
                _ => match key.strip_suffix(".workspace") {
                    Some(field) if value == "true" => {
                        let (_, value) =
                            package
                                .iter()
                                .find(|(key, _)| key == field)
                                .ok_or_else(|| {
                                    format!("The workspace does not define [package.{field}]")
                                })?;
                        format!("{field} = {value}")
                    }
                    _ => line.to_string(),
                },
            };
            lines.push(line);
            continue;
        }

        if !is_dependency_table(&table) {
            lines.push(line.to_string());
            continue;
        }

        let (dependency, extras) =
            match (key.strip_suffix(".workspace"), toml::inline_table(&value)) {
                (Some(dependency), _) if value == "true" => (dependency.to_string(), Vec::new()),
                (None, Some(pairs))
                    if pairs
                        .iter()
                        .any(|(key, value)| key == "workspace" && value == "true") =>
                {
                    let extras = pairs
                        .into_iter()
                        .filter(|(key, _)| key != "workspace")
                        .collect();
                    (key.clone(), extras)
                }
                _ => {
                    lines.push(line.to_string());
                    continue;
                }
            };

        let (_, spec) = dependencies
            .iter()
            .find(|(key, _)| *key == dependency)
            .ok_or_else(|| {
                format!("The workspace does not define the [{dependency}] dependency")
            })?;

        lines.push(format!(
            "{dependency} = {}",
            dependency_spec(spec, extras, framework)
        ));
    }

    let mut manifest = lines.join("\n").trim_end().to_string();
    manifest.push('\n');

    let profiles = toml::profile_tables(workspace);
    if !profiles.is_empty() {
        manifest.push('\n');
        manifest.push_str(&profiles);
        manifest.push('\n');
    }

    if standalone {
        manifest.push_str("\n# This application is a workspace of its own, even inside another project.\n[workspace]\n");
    }

    Ok(manifest)
}

/// Combine a workspace dependency spec with the member's additions.
fn dependency_spec(
    workspace: &str,
    extras: Vec<(String, String)>,
    framework: &Framework,
) -> String {
    let mut pairs = toml::inline_table(workspace)
        .unwrap_or_else(|| vec![("version".into(), workspace.to_string())]);

    if let Some((_, path)) = pairs.iter().find(|(key, _)| key == "path") {
        let mut source = framework.dependency(path.trim_matches('"'));
        source.extend(
            pairs
                .into_iter()
                .filter(|(key, _)| key != "path" && key != "version"),
        );
        pairs = source;
    }

    for (key, value) in extras {
        match pairs.iter_mut().find(|(existing, _)| *existing == key) {
            Some((_, existing)) if key == "features" => {
                let mut features = toml::string_array(existing);
                for feature in toml::string_array(&value) {
                    if !features.contains(&feature) {
                        features.push(feature);
                    }
                }
                *existing = toml::render_string_array(&features);
            }
            Some((_, existing)) => *existing = value,
            None => pairs.push((key, value)),
        }
    }

    match pairs.as_slice() {
        [(key, version)] if key == "version" => version.clone(),
        _ => toml::render_inline_table(&pairs),
    }
}

/// Point the environment at the chosen database, the way Laravel's
/// installer does: uncomment the connection settings for servers, use the
/// right port, and name the database after the application.
pub fn configure_database(environment: &str, database: &str, name: &str) -> String {
    const SETTINGS: [&str; 5] = [
        "DB_HOST=",
        "DB_PORT=",
        "DB_DATABASE=",
        "DB_USERNAME=",
        "DB_PASSWORD=",
    ];
    let database_name = crate_identifier(&name.to_lowercase());

    let lines: Vec<String> = environment
        .lines()
        .map(|line| {
            if line.starts_with("DB_CONNECTION=") {
                return format!("DB_CONNECTION={database}");
            }

            let uncommented = line.strip_prefix("# ").unwrap_or(line);
            if !SETTINGS
                .iter()
                .any(|setting| uncommented.starts_with(setting))
            {
                return line.to_string();
            }

            if database == "sqlite" {
                return format!("# {uncommented}");
            }

            match uncommented {
                "DB_PORT=3306" if database == "pgsql" => "DB_PORT=5432".to_string(),
                "DB_DATABASE=laravel" => format!("DB_DATABASE={database_name}"),
                _ => uncommented.to_string(),
            }
        })
        .collect();

    let mut configured = lines.join("\n");
    if environment.ends_with('\n') {
        configured.push('\n');
    }
    configured
}

/// A new application key: `base64:` and 32 random bytes.
pub fn generate_key() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("The operating system could not provide random bytes.");
    format!(
        "base64:{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// Set `APP_KEY` when it's empty. Returns `None` when there's nothing to do.
pub fn with_app_key(environment: &str, key: &str) -> Option<String> {
    let mut changed = false;
    let lines: Vec<String> = environment
        .lines()
        .map(|line| {
            if line.trim_end() == "APP_KEY=" && !changed {
                changed = true;
                format!("APP_KEY={key}")
            } else {
                line.to_string()
            }
        })
        .collect();

    changed.then(|| {
        let mut updated = lines.join("\n");
        if environment.ends_with('\n') {
            updated.push('\n');
        }
        updated
    })
}

/// Determine whether a directory is inside another Cargo workspace, where a
/// new package would be mistaken for one of its members.
pub fn inside_workspace(directory: &Path) -> bool {
    directory.ancestors().skip(1).any(|ancestor| {
        fs::read_to_string(ancestor.join("Cargo.toml"))
            .is_ok_and(|manifest| manifest.lines().any(|line| line.trim() == "[workspace]"))
    })
}

/// Everything that decides what a new application looks like.
pub struct Blueprint<'a> {
    pub name: &'a str,
    pub database: &'a str,
    pub framework: &'a Framework,
}

/// Write the application's files into the (new) directory.
pub fn copy_skeleton(directory: &Path, blueprint: &Blueprint) -> io::Result<()> {
    let skeleton_manifest = skeleton::file("Cargo.toml").expect("The skeleton has a Cargo.toml.");
    let skeleton_manifest = String::from_utf8_lossy(skeleton_manifest);
    let skeleton_crate = crate_identifier(&package_name(&skeleton_manifest).unwrap_or_default());
    let identifier = crate_identifier(blueprint.name);

    fs::create_dir_all(directory)?;
    let standalone = inside_workspace(&directory.canonicalize()?);

    for file in skeleton::FILES {
        let contents: Vec<u8> = match file.path {
            "Cargo.toml" => manifest(
                &skeleton_manifest,
                skeleton::WORKSPACE_MANIFEST,
                blueprint.name,
                blueprint.framework,
                standalone,
            )
            .map_err(io::Error::other)?
            .into_bytes(),
            ".env.example" => configure_database(
                &String::from_utf8_lossy(file.contents),
                blueprint.database,
                blueprint.name,
            )
            .into_bytes(),
            path if path.ends_with(".rs") => replace_identifier(
                &String::from_utf8_lossy(file.contents),
                &skeleton_crate,
                &identifier,
            )
            .into_bytes(),
            _ => file.contents.to_vec(),
        };

        let path = directory.join(file.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, contents)?;

        #[cfg(unix)]
        if file.executable {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        }
    }

    // Start from the exact versions the framework is tested with; Cargo
    // drops whatever the application doesn't use.
    fs::write(directory.join("Cargo.lock"), skeleton::CARGO_LOCK)
}

/// Create the `.env` file (from `.env.example`) and, for SQLite, the database.
pub fn configure_environment(directory: &Path, database: &str) -> io::Result<()> {
    fs::copy(directory.join(".env.example"), directory.join(".env"))?;

    if database == "sqlite" {
        let path = directory.join("database").join("database.sqlite");
        fs::create_dir_all(path.parent().unwrap_or(directory))?;
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
    }

    Ok(())
}

/// Make sure the application has a key (when `key:generate` didn't run).
pub fn ensure_app_key(directory: &Path) -> io::Result<bool> {
    let path = directory.join(".env");
    let environment = fs::read_to_string(&path)?;
    match with_app_key(&environment, &generate_key()) {
        Some(updated) => fs::write(path, updated).map(|_| true),
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    const SKELETON: &str = r#"[package]
name = "example-app"
version = "0.1.0"
edition = "2024"
description = "The skeleton application for the Laravel framework."
publish = false
default-run = "artisan"

[lib]
path = "lib.rs"

[[bin]]
name = "artisan"
path = "artisan.rs"

[[test]]
name = "feature"
path = "tests/feature/main.rs"

[dependencies]
laravel.workspace = true
serde.workspace = true
tokio = { workspace = true, features = ["test-util"] }
log = "0.4"

[build-dependencies]
laravel-build.workspace = true
"#;

    const WORKSPACE: &str = r#"[package]
name = "laravel"

[workspace]
members = ["skeleton"]

[workspace.dependencies]
laravel = { path = ".", version = "0.1.0" }
laravel-build = { path = "illuminate/build", version = "0.1.0" }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive", "rc"] }
regex = "1"

[dependencies]
serde.workspace = true

[profile.dev]
# Keep debug builds snappy.
debug = 1
"#;

    #[test]
    fn names_are_validated() {
        for name in ["podcasts", "my-app", "my_app", "App2", "a"] {
            assert_eq!(validate_name(name), Ok(()), "{name} should be valid");
        }
        for name in [
            "", "1app", "-app", "_app", "my app", "my.app", "../app", "app/", "café",
        ] {
            assert!(validate_name(name).is_err(), "{name} should be invalid");
        }
        for name in [
            "test",
            "laravel",
            "laravel-build",
            "fn",
            "self",
            "async",
            "std",
            "serde",
        ] {
            assert!(
                validate_name(name).unwrap_err().contains("is reserved"),
                "{name} should be reserved"
            );
        }
    }

    #[test]
    fn crate_identifiers_use_underscores() {
        assert_eq!(crate_identifier("podcasts"), "podcasts");
        assert_eq!(crate_identifier("my-app"), "my_app");
        assert_eq!(headline("my-app"), "My App");
        assert_eq!(headline("podcasts"), "Podcasts");
        assert_eq!(headline("big_new_thing"), "Big New Thing");
    }

    #[test]
    fn identifiers_are_replaced_as_whole_words() {
        let source = "let app = example_app::bootstrap::app();\nuse example_app_extra;\nnot_example_app();\n";
        assert_eq!(
            replace_identifier(source, "example_app", "podcasts"),
            "let app = podcasts::bootstrap::app();\nuse example_app_extra;\nnot_example_app();\n"
        );
    }

    #[test]
    fn the_manifest_is_made_standalone() {
        let framework = Framework::Path(PathBuf::from("/code/laravel-rust"));
        let manifest = manifest(SKELETON, WORKSPACE, "my-app", &framework, false).unwrap();

        assert!(manifest.contains("name = \"my-app\"\n"));
        assert!(manifest.contains("description = \"The My App application.\"\n"));
        assert!(manifest.contains("[lib]\npath = \"lib.rs\"\n"));
        assert!(manifest.contains("[[bin]]\nname = \"artisan\"\npath = \"artisan.rs\"\n"));
        assert!(manifest.contains("[[test]]\nname = \"feature\"\n"));
        assert!(manifest.contains("laravel = { path = \"/code/laravel-rust\" }\n"));
        assert!(
            manifest.contains("serde = { version = \"1\", features = [\"derive\", \"rc\"] }\n")
        );
        assert!(
            manifest
                .contains("tokio = { version = \"1\", features = [\"full\", \"test-util\"] }\n")
        );
        assert!(manifest.contains("log = \"0.4\"\n"));
        assert!(
            manifest
                .contains("laravel-build = { path = \"/code/laravel-rust/illuminate/build\" }\n")
        );
        assert!(manifest.ends_with("\n[profile.dev]\n# Keep debug builds snappy.\ndebug = 1\n"));
        assert!(!manifest.contains("workspace"));

        let git = Framework::Git("https://github.com/portside-labs/laravel-rust".into());
        let manifest = super::manifest(SKELETON, WORKSPACE, "podcasts", &git, true).unwrap();
        assert!(
            manifest.contains(
                "laravel = { git = \"https://github.com/portside-labs/laravel-rust\" }\n"
            )
        );
        assert!(manifest.contains(
            "laravel-build = { git = \"https://github.com/portside-labs/laravel-rust\" }\n"
        ));
        assert!(manifest.ends_with("\n[workspace]\n"));
    }

    #[test]
    fn unknown_workspace_dependencies_are_reported() {
        let framework = Framework::Git("x".into());
        let skeleton = "[dependencies]\nmissing.workspace = true\n";
        assert_eq!(
            manifest(skeleton, WORKSPACE, "podcasts", &framework, false).unwrap_err(),
            "The workspace does not define the [missing] dependency"
        );
    }

    #[test]
    fn the_real_skeleton_manifest_is_rewritten() {
        let skeleton = String::from_utf8_lossy(skeleton::file("Cargo.toml").unwrap()).into_owned();
        let framework = Framework::Git("https://github.com/portside-labs/laravel-rust".into());
        let manifest = manifest(
            &skeleton,
            skeleton::WORKSPACE_MANIFEST,
            "podcasts",
            &framework,
            false,
        )
        .unwrap();

        assert_eq!(package_name(&manifest).as_deref(), Some("podcasts"));
        assert!(!manifest.contains(".workspace"));
        assert!(!manifest.contains("workspace = true"));
        assert!(manifest.contains("serde = { version = \"1\", features = [\"derive\", \"rc\"] }"));
        assert!(manifest.contains("tokio = { version = \"1\", features = [\"full\"] }"));
        assert!(manifest.contains("[profile.dev]"));
    }

    #[test]
    fn sqlite_is_the_default_database() {
        let example = "DB_CONNECTION=sqlite\n# DB_HOST=127.0.0.1\n# DB_PORT=3306\n# DB_DATABASE=laravel\n# DB_USERNAME=root\n# DB_PASSWORD=\n";
        assert_eq!(configure_database(example, "sqlite", "podcasts"), example);

        let uncommented = "DB_CONNECTION=mysql\nDB_HOST=127.0.0.1\nDB_PORT=3306\n";
        assert_eq!(
            configure_database(uncommented, "sqlite", "podcasts"),
            "DB_CONNECTION=sqlite\n# DB_HOST=127.0.0.1\n# DB_PORT=3306\n"
        );
    }

    #[test]
    fn database_servers_are_configured() {
        let example = "APP_NAME=Laravel\nDB_CONNECTION=sqlite\n# DB_HOST=127.0.0.1\n# DB_PORT=3306\n# DB_DATABASE=laravel\n# DB_USERNAME=root\n# DB_PASSWORD=\n\nSESSION_DRIVER=database\n";

        assert_eq!(
            configure_database(example, "mysql", "My-App"),
            "APP_NAME=Laravel\nDB_CONNECTION=mysql\nDB_HOST=127.0.0.1\nDB_PORT=3306\nDB_DATABASE=my_app\nDB_USERNAME=root\nDB_PASSWORD=\n\nSESSION_DRIVER=database\n"
        );
        assert!(configure_database(example, "pgsql", "podcasts").contains(
            "DB_CONNECTION=pgsql\nDB_HOST=127.0.0.1\nDB_PORT=5432\nDB_DATABASE=podcasts\n"
        ));
        assert!(
            configure_database(example, "mariadb", "podcasts")
                .contains("DB_CONNECTION=mariadb\nDB_HOST=127.0.0.1\nDB_PORT=3306\n")
        );
    }

    #[test]
    fn application_keys_are_generated() {
        let key = generate_key();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(key.strip_prefix("base64:").unwrap())
            .unwrap();
        assert_eq!(bytes.len(), 32);
        assert_ne!(generate_key(), key);

        assert_eq!(
            with_app_key("APP_NAME=Laravel\nAPP_KEY=\nAPP_DEBUG=true\n", "base64:abc").as_deref(),
            Some("APP_NAME=Laravel\nAPP_KEY=base64:abc\nAPP_DEBUG=true\n")
        );
        assert_eq!(
            with_app_key("APP_KEY=base64:existing\n", "base64:abc"),
            None
        );
    }
}
