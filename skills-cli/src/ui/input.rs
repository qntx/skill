//! Terminal raw-mode lifecycle and input-buffer draining.

use std::io::{self, Write};
use std::time::Duration;

use crossterm::{event, terminal};

use super::style::CURSOR_SHOW;

/// Drain any stale key events from the terminal input buffer.
///
/// On Windows, crossterm generates both `Press` and `Release` events for every
/// keypress. When a previous prompt (e.g. `cliclack::multiselect`) consumes the
/// `Press` event, the `Release` event remains buffered. If the next prompt
/// reads it without filtering it auto-confirms instantly.
///
/// Call this before any `cliclack` `.interact()` call to prevent cascading
/// auto-confirmation on Windows.
pub(crate) fn drain_input_events() {
    let _ = terminal::enable_raw_mode();
    while event::poll(Duration::from_millis(10)).unwrap_or(false) {
        let _ = event::read();
    }
    let _ = terminal::disable_raw_mode();
}

/// RAII guard that ensures `terminal::disable_raw_mode()` is called on drop,
/// even if the surrounding function returns early via `?` or panics.
///
/// When constructed with [`RawModeGuard::with_hidden_cursor`], the guard also
/// restores cursor visibility on drop.
pub(crate) struct RawModeGuard {
    restore_cursor: bool,
}

impl RawModeGuard {
    /// Create a guard without cursor-visibility handling.
    pub(crate) const fn new() -> Self {
        Self {
            restore_cursor: false,
        }
    }

    /// Create a guard that will also re-enable the cursor on drop.
    pub(crate) const fn with_hidden_cursor() -> Self {
        Self {
            restore_cursor: true,
        }
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if self.restore_cursor {
            let _ = write!(io::stdout(), "{CURSOR_SHOW}");
            let _ = io::stdout().flush();
        }
        let _ = terminal::disable_raw_mode();
    }
}
