//! Sizing for the native Windows console.

use std::io;
use std::os::windows::io::AsRawHandle;

use windows_sys::Win32::System::Console::{CONSOLE_SCREEN_BUFFER_INFO, GetConsoleScreenBufferInfo};

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
