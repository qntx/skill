//! Cursor-anchored line renderer used by interactive prompts.
//!
//! Both [`super::multiselect`] and [`super::fzf`] repaint a fixed block of
//! lines on every keystroke. This module owns the machinery that clears the
//! previously-rendered lines before drawing the new ones so the prompt never
//! leaves scroll-back artefacts behind.

use std::io::{self, Write};

use super::style::{S_STEP_ACTIVE, S_STEP_CANCEL, S_STEP_SUBMIT};

/// Lifecycle state of an interactive prompt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PromptState {
    /// Prompt is accepting input.
    Active,
    /// User has confirmed a selection.
    Submit,
    /// User has cancelled (ESC or Ctrl-C).
    Cancel,
}

impl PromptState {
    /// Leading glyph associated with this state.
    pub(crate) const fn icon(self) -> &'static str {
        match self {
            Self::Active => S_STEP_ACTIVE,
            Self::Submit => S_STEP_SUBMIT,
            Self::Cancel => S_STEP_CANCEL,
        }
    }
}

/// Clear `height` previously rendered lines, leaving the cursor at the top.
fn clear_rendered(stdout: &mut io::Stdout, height: u16) -> io::Result<()> {
    if height > 0 {
        write!(stdout, "\x1b[{height}A")?;
        for _ in 0..height {
            write!(stdout, "\x1b[2K\x1b[1B")?;
        }
        write!(stdout, "\x1b[{height}A")?;
    }
    Ok(())
}

/// Write `lines` to stdout and update `height` for later clearing.
pub(crate) fn render_lines(
    stdout: &mut io::Stdout,
    lines: &[String],
    height: &mut u16,
) -> io::Result<()> {
    clear_rendered(stdout, *height)?;
    for line in lines {
        write!(stdout, "\x1b[2K{line}\r\n")?;
    }
    *height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    stdout.flush()
}
