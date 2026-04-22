//! ANSI terminal style escape sequences used across the CLI.
//!
//! Centralised here so future palette changes touch exactly one file.

/// Reset all attributes (colour, weight, underline).
pub(crate) const RESET: &str = "\x1b[0m";
/// Dim grey, used for secondary / helper text.
pub(crate) const DIM: &str = "\x1b[38;5;102m";
/// Neutral foreground for primary prose.
pub(crate) const TEXT: &str = "\x1b[38;5;145m";
/// Success / positive emphasis.
pub(crate) const GREEN: &str = "\x1b[32m";
/// Warning emphasis.
pub(crate) const YELLOW: &str = "\x1b[33m";
/// Accent colour used for interactive cursors and badges.
pub(crate) const CYAN: &str = "\x1b[36m";
/// Bold weight.
pub(crate) const BOLD: &str = "\x1b[1m";

// ── Interactive prompt glyphs (shared by multiselect + fzf) ──

/// Active prompt indicator (`◆`).
pub(crate) const S_STEP_ACTIVE: &str = "\x1b[32m◆\x1b[0m";
/// Submitted prompt indicator (`◇`).
pub(crate) const S_STEP_SUBMIT: &str = "\x1b[32m◇\x1b[0m";
/// Cancelled prompt indicator (`■`).
pub(crate) const S_STEP_CANCEL: &str = "\x1b[31m■\x1b[0m";
/// Selected radio / checkbox glyph.
pub(crate) const S_RADIO_ACTIVE: &str = "\x1b[32m●\x1b[0m";
/// Unselected radio / checkbox glyph.
pub(crate) const S_RADIO_INACTIVE: &str = "\x1b[2m○\x1b[0m";
/// Bullet used to mark locked items.
pub(crate) const S_BULLET: &str = "\x1b[32m•\x1b[0m";
/// Vertical bar used as the prompt's left gutter.
pub(crate) const S_BAR: &str = "\x1b[2m│\x1b[0m";
/// Horizontal bar used as section separators.
pub(crate) const S_BAR_H: &str = "\x1b[2m─\x1b[0m";
