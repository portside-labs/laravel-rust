//! Line-based prompts for native Windows terminals, like the Laravel installer.

use std::io::{BufRead, Write};

use super::Cancelled;
use crate::output::{bold, yellow};

#[cfg(not(unix))]
use crate::output;
#[cfg(not(unix))]
use std::io;

#[cfg(not(unix))]
pub fn text(
    label: &str,
    placeholder: &str,
    validate: impl Fn(&str) -> Option<String>,
) -> Result<Option<String>, Cancelled> {
    with_terminal(|input, output| ask_text(input, output, label, placeholder, validate)).map(Some)
}

#[cfg(not(unix))]
pub fn select(label: &str, options: &[&str], default: usize) -> Result<usize, Cancelled> {
    with_terminal(|input, output| ask_select(input, output, label, options, default))
}

#[cfg(not(unix))]
pub fn confirm(label: &str, default: bool) -> Result<bool, Cancelled> {
    with_terminal(|input, output| ask_confirm(input, output, label, default))
}

#[cfg(not(unix))]
fn with_terminal<T>(
    prompt: impl FnOnce(&mut io::StdinLock<'_>, &mut io::StdoutLock<'_>) -> Result<T, Cancelled>,
) -> Result<T, Cancelled> {
    let mut input = io::stdin().lock();
    let mut out = io::stdout().lock();
    write!(out, "{}", output::margin_top()).map_err(|_| Cancelled)?;
    let result = prompt(&mut input, &mut out);
    output::assume_new_lines(1);
    result
}

fn ask_text(
    input: &mut impl BufRead,
    output: &mut impl Write,
    label: &str,
    placeholder: &str,
    validate: impl Fn(&str) -> Option<String>,
) -> Result<String, Cancelled> {
    writeln!(output, "  {}", bold(label)).map_err(|_| Cancelled)?;
    if !placeholder.is_empty() {
        writeln!(output, "  {placeholder}").map_err(|_| Cancelled)?;
    }
    loop {
        let name = answer(input, output)?;
        match validate(&name) {
            None => return Ok(name),
            Some(error) => warning(output, &error)?,
        }
    }
}

fn ask_select(
    input: &mut impl BufRead,
    output: &mut impl Write,
    label: &str,
    options: &[&str],
    default: usize,
) -> Result<usize, Cancelled> {
    let default_label = options.get(default).ok_or(Cancelled)?;
    writeln!(output, "  {} [{default_label}]", bold(label)).map_err(|_| Cancelled)?;
    for (index, option) in options.iter().enumerate() {
        writeln!(output, "    [{index}] {option}").map_err(|_| Cancelled)?;
    }
    loop {
        let choice = answer(input, output)?;
        if choice.is_empty() {
            return Ok(default);
        }
        if let Ok(index) = choice.parse::<usize>()
            && index < options.len()
        {
            return Ok(index);
        }
        if let Some(index) = options
            .iter()
            .position(|option| option.eq_ignore_ascii_case(&choice))
        {
            return Ok(index);
        }
        warning(output, "Choose one of the listed options.")?;
    }
}

fn ask_confirm(
    input: &mut impl BufRead,
    output: &mut impl Write,
    label: &str,
    default: bool,
) -> Result<bool, Cancelled> {
    let default_label = if default { "yes" } else { "no" };
    writeln!(output, "  {} (yes/no) [{default_label}]", bold(label)).map_err(|_| Cancelled)?;
    loop {
        match answer(input, output)?.to_ascii_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => warning(output, "Answer yes or no.")?,
        }
    }
}

fn answer(input: &mut impl BufRead, output: &mut impl Write) -> Result<String, Cancelled> {
    write!(output, "  > ").map_err(|_| Cancelled)?;
    output.flush().map_err(|_| Cancelled)?;

    let mut value = String::new();
    if input.read_line(&mut value).map_err(|_| Cancelled)? == 0 {
        return Err(Cancelled);
    }
    Ok(value.trim().to_string())
}

fn warning(output: &mut impl Write, message: &str) -> Result<(), Cancelled> {
    writeln!(output, "  {} {message}", yellow("!")).map_err(|_| Cancelled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_retries_invalid_names_and_trims_the_answer() {
        let mut input = &b"\r\n  demo-app  \r\n"[..];
        let mut output = Vec::new();
        let value = ask_text(
            &mut input,
            &mut output,
            "Project name?",
            "E.g. example-app",
            |name| name.is_empty().then(|| "The name is required.".into()),
        );
        assert_eq!(value, Ok("demo-app".into()));
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("The name is required.")
        );
    }

    #[test]
    fn select_accepts_the_default_an_index_or_an_option_name() {
        let options = ["SQLite", "MySQL"];
        for (answers, expected) in [("\r\n", 0), ("1\r\n", 1), ("invalid\r\nMySQL\r\n", 1)] {
            let mut output = Vec::new();
            let value = ask_select(
                &mut answers.as_bytes(),
                &mut output,
                "Database?",
                &options,
                0,
            );
            assert_eq!(value, Ok(expected));
            let output = String::from_utf8(output).unwrap();
            assert!(output.contains("[0] SQLite"));
            assert_eq!(
                output.contains("Choose one of the listed options."),
                answers.starts_with("invalid")
            );
        }
    }

    #[test]
    fn confirm_uses_the_default_and_accepts_explicit_answers() {
        for (answers, default, expected) in [
            ("\r\n", true, true),
            ("NO\r\n", true, false),
            ("maybe\r\nY\r\n", false, true),
        ] {
            let value = ask_confirm(
                &mut answers.as_bytes(),
                &mut Vec::new(),
                "Run npm?",
                default,
            );
            assert_eq!(value, Ok(expected));
        }
    }

    #[test]
    fn end_of_input_cancels_instead_of_accepting_a_default() {
        assert_eq!(
            ask_confirm(&mut &b""[..], &mut Vec::new(), "Run npm?", true),
            Err(Cancelled)
        );
    }
}
