//! Laravel Prompts, drawn the way the default theme draws them: a box with
//! the question as its title, redrawn in place as keys are pressed, and
//! collapsed to the answer once it's submitted.

use super::Cancelled;
use crate::output::{self, cyan, dim, gray, green, paint, red, strikethrough, yellow};
use crate::terminal::{self, Key, RawMode};

/// What a prompt looks like right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Active,
    Error,
    Submit,
    Cancel,
}

/// The message shown when a prompt is cancelled.
const CANCELLED: &str = "  Cancelled.";

/// The width of a box's content for a terminal of the given width: the box
/// is 62 columns inside (or the terminal's width minus 6, when that's less),
/// with a space of padding on either side of the content.
pub fn content_width(columns: usize) -> usize {
    62.min(columns.saturating_sub(6)).saturating_sub(2).max(10)
}

/// The width of text as it appears on screen, ignoring escape sequences.
pub fn visible_width(text: &str) -> usize {
    strip_escapes(text).chars().count()
}

/// Shorten text to the given width, ending it with `…` when it's too long.
pub fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(width.saturating_sub(1)).collect();
    truncated.push('…');
    truncated
}

/// Remove escape sequences from text.
pub fn strip_escapes(text: &str) -> String {
    let mut stripped = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character == '\x1b' {
            for next in characters.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            stripped.push(character);
        }
    }
    stripped
}

/// A label as shown while the prompt is active (keeping its styling when it
/// fits) and once it's answered (plain, so it can be dimmed).
fn labels(label: &str, width: usize) -> (String, String) {
    let plain = truncate(&strip_escapes(label), width);
    let styled = if visible_width(label) <= width {
        label.to_string()
    } else {
        plain.clone()
    };
    (styled, plain)
}

fn pad(text: &str, width: usize) -> String {
    format!(
        "{text}{}",
        " ".repeat(width.saturating_sub(visible_width(text)))
    )
}

/// Draw a box (Laravel Prompts' `DrawsBoxes::box`).
fn draw_box(title: &str, body: &[String], border: fn(String) -> String, width: usize) -> String {
    let title_width = visible_width(title);
    let mut lines = vec![format!(
        "{} {title} {}",
        border(" ┌".into()),
        border(format!(
            "{}┐",
            "─".repeat(width.saturating_sub(title_width))
        ))
    )];

    for line in body {
        lines.push(format!(
            "{} {} {}",
            border(" │".into()),
            pad(line, width),
            border("│".into())
        ));
    }

    lines.push(border(format!(" └{}┘", "─".repeat(width + 2))));

    let mut rendered = lines.join("\n");
    rendered.push('\n');
    rendered
}

fn gray_border(text: String) -> String {
    gray(text)
}

fn red_border(text: String) -> String {
    red(text)
}

fn yellow_border(text: String) -> String {
    yellow(text)
}

/// Render a select prompt.
pub fn render_select(
    label: &str,
    options: &[&str],
    highlighted: usize,
    state: State,
    width: usize,
) -> String {
    let (styled, label) = labels(label, width);
    let option = |index: usize| truncate(options[index], width.saturating_sub(4));

    match state {
        State::Submit => draw_box(&dim(&label), &[option(highlighted)], gray_border, width) + "\n",
        State::Cancel => {
            let body: Vec<String> = (0..options.len())
                .map(|index| {
                    let marker = if index == highlighted {
                        "› ●"
                    } else {
                        "  ○"
                    };
                    dim(format!("{marker} {}", strikethrough(option(index))))
                })
                .collect();
            draw_box(&label, &body, red_border, width) + &red(CANCELLED) + "\n\n"
        }
        State::Active | State::Error => {
            let body: Vec<String> = (0..options.len())
                .map(|index| {
                    if index == highlighted {
                        format!("{} {} {}", cyan("›"), green("●"), option(index))
                    } else {
                        format!("  {} {}", dim("○"), dim(option(index)))
                    }
                })
                .collect();
            draw_box(&cyan(&styled), &body, gray_border, width) + "\n"
        }
    }
}

/// Render a confirm prompt.
pub fn render_confirm(label: &str, confirmed: bool, state: State, width: usize) -> String {
    let (styled, label) = labels(label, width);
    let (yes, no) = ("Yes", "No");

    match state {
        State::Submit => {
            draw_box(
                &dim(&label),
                &[(if confirmed { yes } else { no }).to_string()],
                gray_border,
                width,
            ) + "\n"
        }
        State::Cancel => {
            let body = if confirmed {
                format!("● {} / ○ {}", strikethrough(yes), strikethrough(no))
            } else {
                format!("○ {} / ● {}", strikethrough(yes), strikethrough(no))
            };
            draw_box(&label, &[dim(body)], red_border, width) + &red(CANCELLED) + "\n\n"
        }
        State::Active | State::Error => {
            let body = if confirmed {
                format!("{} {yes} {}", green("●"), dim(format!("/ ○ {no}")))
            } else {
                format!("{} {} {no}", dim(format!("○ {yes} /")), green("●"))
            };
            draw_box(&cyan(&styled), &[body], gray_border, width) + "\n"
        }
    }
}

/// A text prompt's input, with the cursor drawn as an inverted character.
fn value_with_cursor(value: &str, cursor: usize, placeholder: &str, width: usize) -> String {
    let inverse = |text: String| paint(text, "7", "27");

    if value.is_empty() {
        let mut characters = placeholder.chars();
        let first = characters.next().map_or(" ".to_string(), String::from);
        return dim(format!(
            "{}{}",
            inverse(first),
            truncate(characters.as_str(), width.saturating_sub(2))
        ));
    }

    let characters: Vec<char> = value.chars().collect();
    // Keep the cursor in view when the value is wider than the box.
    let visible = width.saturating_sub(1);
    let start = (cursor + 1).saturating_sub(visible);
    let before: String = characters[start..cursor].iter().collect();
    let at = characters
        .get(cursor)
        .map_or(" ".to_string(), char::to_string);
    let after: String = characters
        .iter()
        .skip(cursor + 1)
        .take(visible.saturating_sub(cursor - start + 1))
        .collect();
    format!("{before}{}{after}", inverse(at))
}

/// What's typed into a text prompt.
#[derive(Clone, Copy)]
pub struct TextInput<'a> {
    pub value: &'a str,
    /// The cursor's position, in characters.
    pub cursor: usize,
    pub placeholder: &'a str,
    pub error: Option<&'a str>,
}

/// Render a text prompt (`columns` is the terminal's width).
pub fn render_text(
    label: &str,
    input: &TextInput,
    state: State,
    width: usize,
    columns: usize,
) -> String {
    let (styled, label) = labels(label, width);
    let TextInput {
        value,
        cursor,
        placeholder,
        error,
    } = *input;

    match state {
        State::Submit => {
            draw_box(&dim(&label), &[truncate(value, width)], gray_border, width) + "\n"
        }
        State::Cancel => {
            let shown = if value.is_empty() { placeholder } else { value };
            draw_box(
                &label,
                &[strikethrough(dim(truncate(shown, width)))],
                red_border,
                width,
            ) + &red(CANCELLED)
                + "\n\n"
        }
        State::Error => {
            let warning = yellow(format!(
                "  ⚠ {}",
                truncate(error.unwrap_or_default(), columns.saturating_sub(5))
            ));
            draw_box(
                &label,
                &[value_with_cursor(value, cursor, placeholder, width)],
                yellow_border,
                width,
            ) + &warning
                + "\n"
        }
        State::Active => {
            draw_box(
                &cyan(&styled),
                &[value_with_cursor(value, cursor, placeholder, width)],
                gray_border,
                width,
            ) + "\n"
        }
    }
}

/// Redraws a prompt in place.
struct Renderer {
    margin: String,
    previous_lines: usize,
}

impl Renderer {
    fn new() -> Self {
        Renderer {
            margin: output::margin_top(),
            previous_lines: 0,
        }
    }

    fn render(&mut self, frame: &str) {
        let frame = format!("{}{frame}", self.margin);
        let erase = if self.previous_lines > 0 {
            format!("\x1b[{}A\r\x1b[J", self.previous_lines)
        } else {
            String::new()
        };
        output::write(&format!("{erase}{frame}"));
        self.previous_lines = frame.matches('\n').count();
    }
}

fn box_width() -> usize {
    content_width(terminal::width())
}

/// Ask the user to pick one of the options; returns the chosen index.
pub fn select(label: &str, options: &[&str], default: usize) -> Result<usize, Cancelled> {
    let Ok(raw) = RawMode::enable() else {
        return Ok(default);
    };
    let width = box_width();
    let count = options.len();
    let mut highlighted = default.min(count.saturating_sub(1));
    let mut renderer = Renderer::new();

    renderer.render(&render_select(
        label,
        options,
        highlighted,
        State::Active,
        width,
    ));

    loop {
        for key in terminal::read_keys().unwrap_or_else(|_| vec![Key::CtrlC]) {
            match key {
                Key::Up | Key::Left | Key::BackTab | Key::Char('k' | 'h') => {
                    highlighted = (highlighted + count - 1) % count;
                }
                Key::Down | Key::Right | Key::Tab | Key::Char('j' | 'l') => {
                    highlighted = (highlighted + 1) % count;
                }
                Key::Home => highlighted = 0,
                Key::End => highlighted = count - 1,
                Key::Enter => {
                    renderer.render(&render_select(
                        label,
                        options,
                        highlighted,
                        State::Submit,
                        width,
                    ));
                    drop(raw);
                    return Ok(highlighted);
                }
                Key::CtrlC => {
                    renderer.render(&render_select(
                        label,
                        options,
                        highlighted,
                        State::Cancel,
                        width,
                    ));
                    drop(raw);
                    return Err(Cancelled);
                }
                _ => {}
            }
        }
        renderer.render(&render_select(
            label,
            options,
            highlighted,
            State::Active,
            width,
        ));
    }
}

/// Ask the user a yes / no question.
pub fn confirm(label: &str, default: bool) -> Result<bool, Cancelled> {
    let Ok(raw) = RawMode::enable() else {
        return Ok(default);
    };
    let width = box_width();
    let mut confirmed = default;
    let mut renderer = Renderer::new();

    renderer.render(&render_confirm(label, confirmed, State::Active, width));

    loop {
        for key in terminal::read_keys().unwrap_or_else(|_| vec![Key::CtrlC]) {
            match key {
                Key::Char('y' | 'Y') => confirmed = true,
                Key::Char('n' | 'N') => confirmed = false,
                Key::Up
                | Key::Down
                | Key::Left
                | Key::Right
                | Key::Tab
                | Key::BackTab
                | Key::Char('h' | 'j' | 'k' | 'l') => confirmed = !confirmed,
                Key::Enter => {
                    renderer.render(&render_confirm(label, confirmed, State::Submit, width));
                    drop(raw);
                    return Ok(confirmed);
                }
                Key::CtrlC => {
                    renderer.render(&render_confirm(label, confirmed, State::Cancel, width));
                    drop(raw);
                    return Err(Cancelled);
                }
                _ => {}
            }
        }
        renderer.render(&render_confirm(label, confirmed, State::Active, width));
    }
}

/// Ask the user for some text, validating it before it's accepted.
pub fn text(
    label: &str,
    placeholder: &str,
    validate: impl Fn(&str) -> Option<String>,
) -> Result<Option<String>, Cancelled> {
    let Ok(raw) = RawMode::enable() else {
        return Ok(None);
    };
    let columns = terminal::width();
    let width = content_width(columns);
    let mut value: Vec<char> = Vec::new();
    let mut cursor = 0;
    let mut error: Option<String> = None;
    let mut renderer = Renderer::new();

    let frame = |value: &[char], cursor: usize, error: &Option<String>, state: State| {
        let value: String = value.iter().collect();
        let input = TextInput {
            value: &value,
            cursor,
            placeholder,
            error: error.as_deref(),
        };
        render_text(label, &input, state, width, columns)
    };

    renderer.render(&frame(&value, cursor, &error, State::Active));

    loop {
        for key in terminal::read_keys().unwrap_or_else(|_| vec![Key::CtrlC]) {
            match key {
                Key::Char(character) => {
                    value.insert(cursor, character);
                    cursor += 1;
                    error = None;
                }
                Key::Backspace if cursor > 0 => {
                    cursor -= 1;
                    value.remove(cursor);
                    error = None;
                }
                Key::Left => cursor = cursor.saturating_sub(1),
                Key::Right => cursor = (cursor + 1).min(value.len()),
                Key::Home => cursor = 0,
                Key::End => cursor = value.len(),
                Key::Enter => {
                    let submitted: String = value.iter().collect();
                    match validate(submitted.trim()) {
                        Some(message) => error = Some(message),
                        None => {
                            renderer.render(&frame(&value, cursor, &None, State::Submit));
                            drop(raw);
                            return Ok(Some(submitted.trim().to_string()));
                        }
                    }
                }
                Key::CtrlC => {
                    renderer.render(&frame(&value, cursor, &error, State::Cancel));
                    drop(raw);
                    return Err(Cancelled);
                }
                _ => {}
            }
        }
        let state = if error.is_some() {
            State::Error
        } else {
            State::Active
        };
        renderer.render(&frame(&value, cursor, &error, state));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATABASES: [&str; 4] = ["SQLite", "MySQL", "MariaDB", "PostgreSQL"];
    const QUESTION: &str = "Which database will your application use?";

    #[test]
    fn boxes_fit_the_terminal() {
        assert_eq!(content_width(80), 60);
        assert_eq!(content_width(200), 60);
        assert_eq!(content_width(50), 42);
    }

    #[test]
    fn an_active_select_shows_every_option() {
        let expected = r#" ┌ Which database will your application use? ───────────────────┐
 │ › ● SQLite                                                   │
 │   ○ MySQL                                                    │
 │   ○ MariaDB                                                  │
 │   ○ PostgreSQL                                               │
 └──────────────────────────────────────────────────────────────┘

"#;
        assert_eq!(
            render_select(QUESTION, &DATABASES, 0, State::Active, 60),
            expected
        );
    }

    #[test]
    fn a_submitted_select_collapses_to_the_answer() {
        let expected = r#" ┌ Which database will your application use? ───────────────────┐
 │ PostgreSQL                                                   │
 └──────────────────────────────────────────────────────────────┘

"#;
        assert_eq!(
            render_select(QUESTION, &DATABASES, 3, State::Submit, 60),
            expected
        );
    }

    #[test]
    fn a_cancelled_select_says_so() {
        let rendered = render_select(QUESTION, &DATABASES, 1, State::Cancel, 60);
        assert!(rendered.contains(" │   ○ SQLite "));
        assert!(rendered.contains(" │ › ● MySQL "));
        assert!(
            rendered.ends_with("└\n  Cancelled.\n\n") || rendered.ends_with("┘\n  Cancelled.\n\n")
        );
    }

    #[test]
    fn confirm_prompts_toggle_between_yes_and_no() {
        let label = "Would you like to run npm install and npm run build?";
        let yes = render_confirm(label, true, State::Active, 60);
        assert!(yes.contains(" │ ● Yes / ○ No "));
        let no = render_confirm(label, false, State::Active, 60);
        assert!(no.contains(" │ ○ Yes / ● No "));

        let expected = r#" ┌ Would you like to run npm install and npm run build? ────────┐
 │ Yes                                                          │
 └──────────────────────────────────────────────────────────────┘

"#;
        assert_eq!(render_confirm(label, true, State::Submit, 60), expected);
    }

    #[test]
    fn long_labels_are_truncated() {
        let label = "x".repeat(80);
        let rendered = render_select(&label, &DATABASES, 0, State::Submit, 20);
        let top = rendered.lines().next().unwrap();
        assert_eq!(top, format!(" ┌ {}… ┐", "x".repeat(19)));
        assert!(
            rendered
                .lines()
                .filter(|line| !line.is_empty())
                .all(|line| visible_width(line) == 25)
        );
    }

    #[test]
    fn text_prompts_draw_a_cursor() {
        let label = "What is the name of your project?";
        let input = |value, cursor, error| TextInput {
            value,
            cursor,
            placeholder: "E.g. example-app",
            error,
        };

        let empty = render_text(label, &input("", 0, None), State::Active, 60, 80);
        assert!(empty.contains(" │ E.g. example-app "));

        let typed = render_text(label, &input("podcasts", 8, None), State::Active, 60, 80);
        assert!(typed.contains(" │ podcasts  "));

        let invalid = render_text(
            label,
            &input("1up", 3, Some("Invalid.")),
            State::Error,
            60,
            80,
        );
        assert!(invalid.ends_with("  ⚠ Invalid.\n"));

        let long = "x".repeat(100);
        let truncated = render_text(label, &input("", 0, Some(&long)), State::Error, 60, 80);
        assert!(truncated.ends_with(&format!("  ⚠ {}…\n", "x".repeat(74))));

        let submitted = render_text(label, &input("podcasts", 8, None), State::Submit, 60, 80);
        assert!(submitted.contains(" │ podcasts "));
    }

    #[test]
    fn escape_sequences_take_no_space() {
        assert_eq!(visible_width("\x1b[36mcyan\x1b[39m"), 4);
        assert_eq!(visible_width("plain"), 5);
        assert_eq!(truncate("Laravel", 5), "Lara…");
        assert_eq!(truncate("Laravel", 7), "Laravel");
    }
}
