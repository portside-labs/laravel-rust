//! Sizing for the native Windows console.

use std::io;
use std::os::windows::io::{AsRawHandle, RawHandle};
use std::sync::OnceLock;

use windows_sys::Win32::System::Console::{
    CONSOLE_SCREEN_BUFFER_INFO, CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT,
    ENABLE_PROCESSED_OUTPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode,
    GetConsoleScreenBufferInfo, SetConsoleCtrlHandler, SetConsoleMode,
};

/// Read the visible window width, rather than the backing buffer width.
pub fn width() -> Option<usize> {
    let handle = io::stdout().as_raw_handle();
    let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
    // SAFETY: stdout owns the handle and the console call only writes to `info`.
    if unsafe { GetConsoleScreenBufferInfo(handle, &mut info) } == 0 {
        return None;
    }
    let columns = i32::from(info.srWindow.Right) - i32::from(info.srWindow.Left) + 1;
    (columns > 0).then_some(columns as usize)
}

// The console control handler can run on a different thread.
static ORIGINAL_MODES: OnceLock<[Option<(usize, u32)>; 2]> = OnceLock::new();

/// Restore the console's output modes when the installer exits.
pub struct ConsoleOutput(bool);

impl Drop for ConsoleOutput {
    fn drop(&mut self) {
        if self.0 {
            restore_modes();
            // SAFETY: this removes the same handler that was registered at startup.
            unsafe { SetConsoleCtrlHandler(Some(on_cancel), 0) };
        }
    }
}

/// Enable ANSI colors on consoles that support virtual terminal output.
pub fn enable_ansi() -> ConsoleOutput {
    let handles = [io::stdout().as_raw_handle(), io::stderr().as_raw_handle()];
    let modes = handles.map(|handle| {
        let mode = console_mode(handle)?;
        (mode != ansi_mode(mode)).then_some((handle as usize, mode))
    });
    if modes.iter().all(Option::is_none) || ORIGINAL_MODES.set(modes).is_err() {
        return ConsoleOutput(false);
    }
    // SAFETY: the handler restores output modes and leaves cancellation to Windows.
    if unsafe { SetConsoleCtrlHandler(Some(on_cancel), 1) } == 0 {
        return ConsoleOutput(false);
    }
    for (handle, mode) in modes.into_iter().flatten() {
        // SAFETY: `console_mode` confirmed that this is a live console handle.
        unsafe { SetConsoleMode(handle as RawHandle, ansi_mode(mode)) };
    }
    ConsoleOutput(true)
}

fn ansi_mode(mode: u32) -> u32 {
    mode | ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING
}

unsafe extern "system" fn on_cancel(event: u32) -> windows_sys::core::BOOL {
    if matches!(event, CTRL_C_EVENT | CTRL_BREAK_EVENT | CTRL_CLOSE_EVENT) {
        restore_modes();
    }
    0
}

fn restore_modes() {
    if let Some(modes) = ORIGINAL_MODES.get() {
        for (handle, mode) in modes.iter().rev().flatten() {
            // SAFETY: the handles belong to the process and remain valid until exit.
            unsafe { SetConsoleMode(*handle as RawHandle, *mode) };
        }
    }
}

/// Determine whether ANSI output will be interpreted by the console.
pub fn supports_ansi() -> bool {
    console_mode(io::stdout().as_raw_handle())
        .is_some_and(|mode| mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING != 0)
}

fn console_mode(handle: RawHandle) -> Option<u32> {
    let mut mode = 0;
    // SAFETY: the console API only writes to `mode` and validates the handle.
    (unsafe { GetConsoleMode(handle, &mut mode) } != 0).then_some(mode)
}
