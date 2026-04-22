//! Shared UI primitives for the Skills CLI.
//!
//! This module is organized by concern so each submodule fits in a single
//! screen and can be unit-tested in isolation:
//!
//! - `style`        — terminal colour / style escape constants
//! - `banner`       — ASCII logo and help banner rendering
//! - `format`       — pure string-formatting helpers (path, kebab, list)
//! - `input`        — raw-mode lifecycle and event draining
//! - `render`       — cursor-anchored line renderer + prompt state
//! - `multiselect`  — interactive search-multiselect prompt
//! - `fzf`          — interactive fzf-style search prompt

pub(crate) mod banner;
pub(crate) mod emit;
pub(crate) mod format;
pub(crate) mod fzf;
pub(crate) mod input;
pub(crate) mod multiselect;
pub(crate) mod render;
pub(crate) mod style;

// Re-exports: keep the same flat import surface that existing call sites use
// (`use crate::ui::{DIM, RESET, TEXT};` etc.) so the split is a pure
// structural refactor with zero downstream edits.
pub(crate) use banner::{show_banner, show_logo};
pub(crate) use format::{format_list, kebab_to_title, shorten_path_with_cwd};
pub(crate) use fzf::{FzfItem, FzfResult, fzf_search};
pub(crate) use input::drain_input_events;
pub(crate) use multiselect::{
    LockedSection, SearchItem, SearchMultiselectOptions, SearchMultiselectResult,
    search_multiselect,
};
pub(crate) use style::{
    BOLD, BOLD_RED, CLEAR_EOL, CYAN, DIM, GREEN, INTRO_TAG, RED, RESET, TEXT, YELLOW,
};
