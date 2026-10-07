//! The command line: `laravel-rust new <name> [options]`,
//! `laravel-rust list`, `laravel-rust help`, and `laravel-rust --version`,
//! parsed the way Symfony Console parses them (options may appear anywhere,
//! `--` ends them).

use crate::output::{green, yellow};

/// The installer's version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What to do.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// List the available commands.
    List,
    /// Show the installer's version.
    Version,
    /// Describe a command.
    Help(String),
    /// Create a new application.
    New(NewOptions),
}

/// The options of `laravel-rust new`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct NewOptions {
    pub name: Option<String>,
    pub database: Option<String>,
    pub npm: Option<bool>,
    pub force: bool,
    pub path: Option<String>,
}

/// A parsed command line.
#[derive(Debug, PartialEq, Eq)]
pub struct Cli {
    pub command: Command,
    /// `-n` / `--no-interaction` was not given.
    pub interaction: bool,
    /// `--ansi` (`Some(true)`) or `--no-ansi` (`Some(false)`).
    pub ansi: Option<bool>,
}

/// An option, as it appears in help output.
struct OptionHelp {
    short: Option<char>,
    long: &'static str,
    value: Option<&'static str>,
    description: &'static str,
}

const GLOBAL_OPTIONS: &[OptionHelp] = &[
    OptionHelp {
        short: Some('h'),
        long: "help",
        value: None,
        description: "Display help for the given command. When no command is given display help for the list command",
    },
    OptionHelp {
        short: Some('V'),
        long: "version",
        value: None,
        description: "Display this application version",
    },
    OptionHelp {
        short: None,
        long: "ansi|--no-ansi",
        value: None,
        description: "Force (or disable --no-ansi) ANSI output",
    },
    OptionHelp {
        short: Some('n'),
        long: "no-interaction",
        value: None,
        description: "Do not ask any interactive question",
    },
];

const NEW_OPTIONS: &[OptionHelp] = &[
    OptionHelp {
        short: None,
        long: "database",
        value: Some("DATABASE"),
        description: "The database driver your application will use. Possible values are: sqlite, mysql, mariadb, pgsql",
    },
    OptionHelp {
        short: None,
        long: "npm",
        value: None,
        description: "Install and build NPM dependencies",
    },
    OptionHelp {
        short: None,
        long: "no-npm",
        value: None,
        description: "Do not install and build NPM dependencies",
    },
    OptionHelp {
        short: None,
        long: "path",
        value: Some("PATH"),
        description: "The framework checkout the application should depend on",
    },
    OptionHelp {
        short: Some('f'),
        long: "force",
        value: None,
        description: "Forces install even if the directory already exists",
    },
];

const COMMANDS: [(&str, &str); 3] = [
    ("help", "Display help for a command"),
    ("list", "List commands"),
    ("new", "Create a new Laravel application"),
];

/// Parse the arguments (without the program name).
pub fn parse(arguments: &[String]) -> Result<Cli, String> {
    let mut positional = Vec::new();
    let mut options: Vec<(String, Option<String>)> = Vec::new();
    let mut tokens = arguments.iter();

    while let Some(token) = tokens.next() {
        if token == "--" {
            positional.extend(tokens.by_ref().cloned());
            break;
        }

        if let Some(long) = token.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((name, value)) => (name.to_string(), Some(value.to_string())),
                None => (long.to_string(), None),
            };
            let value = match (takes_value(&name), value) {
                (true, None) => match tokens.next() {
                    Some(value) => Some(value.clone()),
                    None => return Err(format!("The \"--{name}\" option requires a value")),
                },
                (false, Some(_)) if is_known(&name) => {
                    return Err(format!("The \"--{name}\" option does not accept a value"));
                }
                (_, value) => value,
            };
            options.push((name, value));
        } else if let Some(short) = token.strip_prefix('-').filter(|short| !short.is_empty()) {
            for flag in short.chars() {
                let name = match flag {
                    'h' => "help",
                    'V' => "version",
                    'n' => "no-interaction",
                    'f' => "force",
                    'v' => "verbose",
                    'q' => "quiet",
                    _ => return Err(format!("The \"-{flag}\" option does not exist")),
                };
                options.push((name.to_string(), None));
            }
        } else {
            positional.push(token.clone());
        }
    }

    let has = |name: &str| options.iter().any(|(option, _)| option == name);
    let ansi = options
        .iter()
        .rev()
        .find_map(|(name, _)| match name.as_str() {
            "ansi" => Some(true),
            "no-ansi" => Some(false),
            _ => None,
        });
    let interaction = !has("no-interaction");
    let cli = |command| Cli {
        command,
        interaction,
        ansi,
    };

    if has("version") {
        return Ok(cli(Command::Version));
    }

    let mut positional = positional.into_iter();
    let command = positional.next();

    let allowed = |name: &str| {
        GLOBAL_OPTIONS.iter().any(|option| option.long == name)
            || ["ansi", "no-ansi", "verbose", "quiet"].contains(&name)
    };

    match command.as_deref() {
        None | Some("list") => {
            reject_unknown(&options, |name| allowed(name))?;
            Ok(cli(Command::List))
        }
        Some("help") => {
            reject_unknown(&options, |name| allowed(name))?;
            let topic = positional.next().unwrap_or_else(|| "help".into());
            find_command(&topic)?;
            Ok(cli(Command::Help(topic)))
        }
        Some(name) if has("help") => {
            find_command(name)?;
            Ok(cli(Command::Help(name.to_string())))
        }
        Some("new") => {
            reject_unknown(&options, |name| {
                allowed(name) || NEW_OPTIONS.iter().any(|option| option.long == name)
            })?;

            let name = positional.next();
            if positional.next().is_some() {
                return Err(
                    "Too many arguments to \"new\" command, expected arguments \"name\"".into(),
                );
            }

            let mut new = NewOptions {
                name,
                ..NewOptions::default()
            };
            for (option, value) in options {
                match option.as_str() {
                    "database" => new.database = value,
                    "path" => new.path = value,
                    "npm" => new.npm = Some(true),
                    "no-npm" => new.npm = Some(false),
                    "force" => new.force = true,
                    _ => {}
                }
            }
            Ok(cli(Command::New(new)))
        }
        Some(other) => Err(format!("Command \"{other}\" is not defined")),
    }
}

fn takes_value(name: &str) -> bool {
    NEW_OPTIONS
        .iter()
        .any(|option| option.long == name && option.value.is_some())
}

fn is_known(name: &str) -> bool {
    NEW_OPTIONS
        .iter()
        .chain(GLOBAL_OPTIONS)
        .any(|option| option.long == name)
        || ["ansi", "no-ansi", "verbose", "quiet"].contains(&name)
}

fn reject_unknown(
    options: &[(String, Option<String>)],
    allowed: impl Fn(&str) -> bool,
) -> Result<(), String> {
    match options.iter().find(|(name, _)| !allowed(name)) {
        Some((name, _)) => Err(format!("The \"--{name}\" option does not exist")),
        None => Ok(()),
    }
}

fn find_command(name: &str) -> Result<(), String> {
    if COMMANDS.iter().any(|(command, _)| *command == name) {
        Ok(())
    } else {
        Err(format!("Command \"{name}\" is not defined"))
    }
}

/// `Laravel Installer 0.1.0`
pub fn version() -> String {
    format!("Laravel Installer {}\n", green(VERSION))
}

/// `-f, --force` / `    --database=DATABASE`
fn synopsis(option: &OptionHelp) -> String {
    let short = option
        .short
        .map_or("    ".to_string(), |short| format!("-{short}, "));
    let value = option
        .value
        .map_or(String::new(), |value| format!("={value}"));
    format!("{short}--{}{value}", option.long)
}

fn synopsis_width(options: &[&OptionHelp]) -> usize {
    options
        .iter()
        .map(|option| synopsis(option).chars().count())
        .max()
        .unwrap_or(0)
}

fn render_options(options: &[&OptionHelp]) -> String {
    let width = synopsis_width(options);

    options
        .iter()
        .map(|option| {
            let synopsis = synopsis(option);
            let padding = " ".repeat(width - synopsis.chars().count());
            format!("  {}{padding}  {}\n", green(&synopsis), option.description)
        })
        .collect()
}

/// The command list (`laravel-rust`, `laravel-rust list`).
pub fn list() -> String {
    let width = COMMANDS
        .iter()
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(0);
    let commands: String = COMMANDS
        .iter()
        .map(|(name, description)| {
            format!(
                "  {}{}  {description}\n",
                green(name),
                " ".repeat(width - name.len())
            )
        })
        .collect();

    format!(
        "{}\n\n{}\n  command [options] [arguments]\n\n{}\n{}\n\n{}\n{commands}",
        version().trim_end(),
        yellow("Usage:"),
        yellow("Options:"),
        render_options(&GLOBAL_OPTIONS.iter().collect::<Vec<_>>()).trim_end(),
        yellow("Available commands:"),
    )
}

/// Help for a command (`laravel-rust help new`, `laravel-rust new --help`).
pub fn help(command: &str) -> String {
    let (description, usage, arguments, options): (&str, &str, &str, Vec<&OptionHelp>) =
        match command {
            "new" => (
                "Create a new Laravel application",
                "new [options] [--] [<name>]",
                "name",
                NEW_OPTIONS.iter().chain(GLOBAL_OPTIONS).collect(),
            ),
            "list" => ("List commands", "list", "", GLOBAL_OPTIONS.iter().collect()),
            _ => (
                "Display help for a command",
                "help [<command_name>]",
                "command_name",
                GLOBAL_OPTIONS.iter().collect(),
            ),
        };

    let mut rendered = format!(
        "{}\n  {description}\n\n{}\n  {usage}\n\n",
        yellow("Description:"),
        yellow("Usage:")
    );
    if !arguments.is_empty() {
        let width = synopsis_width(&options);
        let explanation = if arguments == "name" {
            "The name of the application"
        } else {
            "The command name"
        };
        rendered.push_str(&format!(
            "{}\n  {}{}  {explanation}\n\n",
            yellow("Arguments:"),
            green(arguments),
            " ".repeat(width.saturating_sub(arguments.len()))
        ));
    }
    rendered.push_str(&format!(
        "{}\n{}",
        yellow("Options:"),
        render_options(&options)
    ));
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(arguments: &str) -> Result<Cli, String> {
        parse(
            &arguments
                .split_whitespace()
                .map(String::from)
                .collect::<Vec<_>>(),
        )
    }

    fn new(arguments: &str) -> NewOptions {
        match parse_args(arguments).unwrap().command {
            Command::New(options) => options,
            command => panic!("Expected the new command, got {command:?}"),
        }
    }

    #[test]
    fn the_new_command_is_parsed() {
        assert_eq!(
            new("new podcasts --database=pgsql --no-npm --force"),
            NewOptions {
                name: Some("podcasts".into()),
                database: Some("pgsql".into()),
                npm: Some(false),
                force: true,
                path: None,
            }
        );
        assert_eq!(
            new("new --database mysql podcasts").database.as_deref(),
            Some("mysql")
        );
        assert_eq!(
            new("new podcasts --path=../laravel-rust").path.as_deref(),
            Some("../laravel-rust")
        );
        assert_eq!(new("new podcasts --no-npm --npm").npm, Some(true));
        assert_eq!(new("new").name, None);
        assert!(new("-f new podcasts").force);

        let cli = parse_args("new podcasts -n --ansi").unwrap();
        assert!(!cli.interaction);
        assert_eq!(cli.ansi, Some(true));
        assert!(parse_args("new podcasts").unwrap().interaction);
    }

    #[test]
    fn other_commands_are_parsed() {
        assert_eq!(parse_args("").unwrap().command, Command::List);
        assert_eq!(parse_args("list").unwrap().command, Command::List);
        assert_eq!(parse_args("--help").unwrap().command, Command::List);
        assert_eq!(parse_args("-V").unwrap().command, Command::Version);
        assert_eq!(parse_args("--version").unwrap().command, Command::Version);
        assert_eq!(
            parse_args("new --help").unwrap().command,
            Command::Help("new".into())
        );
        assert_eq!(
            parse_args("help new").unwrap().command,
            Command::Help("new".into())
        );
    }

    #[test]
    fn mistakes_are_reported() {
        assert_eq!(
            parse_args("new podcasts --dev").unwrap_err(),
            "The \"--dev\" option does not exist"
        );
        assert_eq!(
            parse_args("new podcasts -x").unwrap_err(),
            "The \"-x\" option does not exist"
        );
        assert_eq!(
            parse_args("new podcasts --database").unwrap_err(),
            "The \"--database\" option requires a value"
        );
        assert_eq!(
            parse_args("new podcasts --force=yes").unwrap_err(),
            "The \"--force\" option does not accept a value"
        );
        assert_eq!(
            parse_args("new a b").unwrap_err(),
            "Too many arguments to \"new\" command, expected arguments \"name\""
        );
        assert_eq!(
            parse_args("create podcasts").unwrap_err(),
            "Command \"create\" is not defined"
        );
        assert_eq!(
            parse_args("list --force").unwrap_err(),
            "The \"--force\" option does not exist"
        );
    }

    #[test]
    fn usage_is_listed() {
        let list = list();
        assert!(list.starts_with("Laravel Installer 0.1.0\n\nUsage:\n  command [options] [arguments]\n\nOptions:\n  -h, --help "));
        assert!(
            list.contains("\n      --ansi|--no-ansi  Force (or disable --no-ansi) ANSI output\n")
        );
        assert!(list.ends_with("Available commands:\n  help  Display help for a command\n  list  List commands\n  new   Create a new Laravel application\n"));

        let help = help("new");
        assert!(help.contains("Usage:\n  new [options] [--] [<name>]\n"));
        assert!(help.contains("      --database=DATABASE  The database driver"));
        assert!(help.contains("  -f, --force              Forces install"));
        assert!(help.contains(&format!(
            "Arguments:\n  name{}The name of the application\n",
            " ".repeat(21)
        )));
    }
}
