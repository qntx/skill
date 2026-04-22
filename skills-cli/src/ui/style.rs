//! ANSI terminal style escape sequences used across the CLI.
//!
//! Centralised here so future palette changes touch exactly one file. Every
//! user-visible escape sequence **must** go through one of these constants —
//! never hard-code `"\x1b[…m"` inside command or UI code.

/// Reset all attributes (colour, weight, underline).
pub(crate) const RESET: &str = "\x1b[0m";

/// Red foreground, used for errors and critical warnings.
pub(crate) const RED: &str = "\x1b[31m";
/// Green foreground, used for success and positive emphasis.
pub(crate) const GREEN: &str = "\x1b[32m";
/// Yellow foreground, used for warnings.
pub(crate) const YELLOW: &str = "\x1b[33m";
/// Cyan foreground, accent colour for interactive cursors and badges.
pub(crate) const CYAN: &str = "\x1b[36m";

/// Dim grey (palette 102), for secondary / helper text.
pub(crate) const DIM: &str = "\x1b[38;5;102m";
/// Neutral mid-grey (palette 145), for primary prose.
pub(crate) const TEXT: &str = "\x1b[38;5;145m";

/// Bold weight modifier.
pub(crate) const BOLD: &str = "\x1b[1m";
/// Faint weight modifier (half-bright, distinct from the grey palette).
pub(crate) const FAINT: &str = "\x1b[2m";
/// Underline modifier.
pub(crate) const UNDERLINE: &str = "\x1b[4m";
/// Reverse video (swap fg / bg).
pub(crate) const REVERSE: &str = "\x1b[7m";
/// Strike-through (used for cancelled prompts).
pub(crate) const STRIKE: &str = "\x1b[9m";

/// Bold red, for critical-severity badges.
pub(crate) const BOLD_RED: &str = "\x1b[31m\x1b[1m";
/// Intro badge prefix: cyan background + black foreground (`  skills  ` ribbon).
pub(crate) const INTRO_TAG: &str = "\x1b[46m\x1b[30m";

/// Grey palette used for the SKILLS logo gradient (top → bottom, 6 rows).
pub(crate) const LOGO_GRAYS: &[&str] = &[
    "\x1b[38;5;250m",
    "\x1b[38;5;248m",
    "\x1b[38;5;245m",
    "\x1b[38;5;243m",
    "\x1b[38;5;240m",
    "\x1b[38;5;238m",
];

/// Active prompt indicator (`◆`).
pub(crate) const S_STEP_ACTIVE: &str = "\x1b[32m◆\x1b[0m";
/// Submitted prompt indicator (`◇`).
pub(crate) const S_STEP_SUBMIT: &str = "\x1b[32m◇\x1b[0m";
/// Cancelled prompt indicator (`■`).
pub(crate) const S_STEP_CANCEL: &str = "\x1b[31m■\x1b[0m";
/// Selected radio / checkbox glyph (`●`).
pub(crate) const S_RADIO_ACTIVE: &str = "\x1b[32m●\x1b[0m";
/// Unselected radio / checkbox glyph (`○`).
pub(crate) const S_RADIO_INACTIVE: &str = "\x1b[2m○\x1b[0m";
/// Bullet used to mark locked items (`•`).
pub(crate) const S_BULLET: &str = "\x1b[32m•\x1b[0m";
/// Vertical bar used as the prompt's left gutter (`│`).
pub(crate) const S_BAR: &str = "\x1b[2m│\x1b[0m";
/// Horizontal bar used as section separators (`─`).
pub(crate) const S_BAR_H: &str = "\x1b[2m─\x1b[0m";

/// Cursor arrow used in the fzf / multiselect prompts (`❯`).
pub(crate) const CURSOR_ARROW: &str = "\x1b[36m❯\x1b[0m";

/// Hide the terminal cursor.
pub(crate) const CURSOR_HIDE: &str = "\x1b[?25l";
/// Show the terminal cursor.
pub(crate) const CURSOR_SHOW: &str = "\x1b[?25h";
/// Clear the entire current line.
pub(crate) const CLEAR_LINE: &str = "\x1b[2K";
/// Clear from cursor to end of line.
pub(crate) const CLEAR_EOL: &str = "\x1b[K";
/// Move cursor down one line (CUD 1).
pub(crate) const CURSOR_DOWN_1: &str = "\x1b[1B";
