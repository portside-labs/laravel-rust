//! Unix terminal sizing, raw mode, and key presses.
//!
//! Raw mode is entered through `termios` and always restored: when the
//! guard is dropped, when the process panics, and when it is interrupted
//! by a signal.

use std::io::{self, Write};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// The terminal settings from before raw mode was first entered.
static ORIGINAL: OnceLock<libc::termios> = OnceLock::new();

/// Whether raw mode (and a hidden cursor) is currently active.
static RAW: AtomicBool = AtomicBool::new(false);

const SHOW_CURSOR: &[u8] = b"\x1b[?25h";
const HIDE_CURSOR: &[u8] = b"\x1b[?25l";

/// The terminal's width in columns, when a console is available.
pub fn width() -> Option<usize> {
    for fd in [libc::STDOUT_FILENO, libc::STDERR_FILENO, libc::STDIN_FILENO] {
        // SAFETY: `winsize` is plain data and TIOCGWINSZ only writes into it.
        let mut size: libc::winsize = unsafe { std::mem::zeroed() };
        if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut size) } == 0 && size.ws_col > 0 {
            return Some(usize::from(size.ws_col));
        }
    }

    None
}

/// Raw mode, with the cursor hidden, for as long as the guard lives.
pub struct RawMode;

impl RawMode {
    /// Switch standard input to raw mode and hide the cursor.
    pub fn enable() -> io::Result<RawMode> {
        // SAFETY: `termios` is plain data that `tcgetattr` fills in.
        let mut current: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut current) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let original = *ORIGINAL.get_or_init(|| current);

        let mut raw = original;
        raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG | libc::IEXTEN);
        raw.c_iflag &= !(libc::IXON | libc::ICRNL);
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;

        install_handlers();
        RAW.store(true, Ordering::SeqCst);
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) } != 0 {
            RAW.store(false, Ordering::SeqCst);
            return Err(io::Error::last_os_error());
        }

        let mut stdout = io::stdout().lock();
        stdout.write_all(HIDE_CURSOR)?;
        stdout.flush()?;

        Ok(RawMode)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        restore();
    }
}

/// Restore the terminal's original settings and show the cursor.
///
/// Only async-signal-safe calls are made, so signal handlers can use it.
pub fn restore() {
    if !RAW.swap(false, Ordering::SeqCst) {
        return;
    }
    if let Some(original) = ORIGINAL.get() {
        // SAFETY: restoring settings previously read with `tcgetattr`.
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, original) };
    }
    // SAFETY: writing a static buffer to standard output.
    unsafe {
        libc::write(
            libc::STDOUT_FILENO,
            SHOW_CURSOR.as_ptr().cast(),
            SHOW_CURSOR.len(),
        )
    };
}

/// Restore the terminal before the default panic message is printed.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        default(info);
    }));
}

extern "C" fn on_signal(signal: libc::c_int) {
    restore();
    // SAFETY: reinstating the default disposition and re-raising the signal
    // so the process ends exactly as it would have without us.
    unsafe {
        libc::signal(signal, libc::SIG_DFL);
        libc::raise(signal);
    }
}

fn install_handlers() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT] {
            // SAFETY: `on_signal` only makes async-signal-safe calls.
            unsafe { libc::signal(signal, handler) };
        }
    });
}

/// A key press.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Tab,
    BackTab,
    Enter,
    Backspace,
    CtrlC,
    Escape,
    Char(char),
    Unknown,
}

/// Wait for the next key presses on standard input.
pub fn read_keys() -> io::Result<Vec<Key>> {
    let mut buffer = [0u8; 64];
    loop {
        // SAFETY: reading into a stack buffer of the given length.
        let read =
            unsafe { libc::read(libc::STDIN_FILENO, buffer.as_mut_ptr().cast(), buffer.len()) };
        if read > 0 {
            return Ok(parse_keys(&buffer[..read as usize]));
        }
        if read == 0 {
            // End of input: treat it like Ctrl+C rather than spinning.
            return Ok(vec![Key::CtrlC]);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

/// Decode raw terminal input into key presses.
pub fn parse_keys(bytes: &[u8]) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        let byte = bytes[index];
        index += 1;

        let key = match byte {
            0x03 => Key::CtrlC,
            b'\r' | b'\n' => Key::Enter,
            b'\t' => Key::Tab,
            0x7f | 0x08 => Key::Backspace,
            0x10 => Key::Up,    // Ctrl+P
            0x0e => Key::Down,  // Ctrl+N
            0x02 => Key::Left,  // Ctrl+B
            0x06 => Key::Right, // Ctrl+F
            0x01 => Key::Home,  // Ctrl+A
            0x05 => Key::End,   // Ctrl+E
            0x1b => match bytes.get(index) {
                Some(b'[') | Some(b'O') => {
                    let start = index + 1;
                    let end = bytes[start..]
                        .iter()
                        .position(|byte| (0x40..=0x7e).contains(byte))
                        .map(|offset| start + offset);
                    match end {
                        Some(end) => {
                            index = end + 1;
                            escape_sequence(&bytes[start..end], bytes[end])
                        }
                        None => {
                            index = bytes.len();
                            Key::Unknown
                        }
                    }
                }
                _ => Key::Escape,
            },
            _ if byte < 0x20 => Key::Unknown,
            _ => {
                let length = match byte {
                    0xc0..=0xdf => 2,
                    0xe0..=0xef => 3,
                    0xf0..=0xf7 => 4,
                    _ => 1,
                };
                let end = (index - 1 + length).min(bytes.len());
                let key = std::str::from_utf8(&bytes[index - 1..end])
                    .ok()
                    .and_then(|text| text.chars().next())
                    .map_or(Key::Unknown, Key::Char);
                index = end;
                key
            }
        };

        keys.push(key);
    }

    keys
}

fn escape_sequence(parameters: &[u8], last: u8) -> Key {
    match (parameters, last) {
        (_, b'A') => Key::Up,
        (_, b'B') => Key::Down,
        (_, b'C') => Key::Right,
        (_, b'D') => Key::Left,
        (_, b'H') | (b"1" | b"7", b'~') => Key::Home,
        (_, b'F') | (b"4" | b"8", b'~') => Key::End,
        (_, b'Z') => Key::BackTab,
        _ => Key::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_decoded() {
        assert_eq!(parse_keys(b"\x1b[A\x1b[B"), [Key::Up, Key::Down]);
        assert_eq!(parse_keys(b"\x1bOA\x1bOB"), [Key::Up, Key::Down]);
        assert_eq!(parse_keys(b"\x1b[C\x1b[D"), [Key::Right, Key::Left]);
        assert_eq!(parse_keys(b"\x1b[Z\t"), [Key::BackTab, Key::Tab]);
        assert_eq!(parse_keys(b"\x1b[1~\x1b[4~"), [Key::Home, Key::End]);
        assert_eq!(parse_keys(b"\r\n"), [Key::Enter, Key::Enter]);
        assert_eq!(parse_keys(b"\x03"), [Key::CtrlC]);
        assert_eq!(
            parse_keys(b"jky"),
            [Key::Char('j'), Key::Char('k'), Key::Char('y')]
        );
        assert_eq!(parse_keys("é".as_bytes()), [Key::Char('é')]);
        assert_eq!(parse_keys(b"\x1b"), [Key::Escape]);
        assert_eq!(parse_keys(b"\x7f"), [Key::Backspace]);
    }
}
