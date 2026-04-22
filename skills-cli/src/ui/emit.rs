//! Thin wrappers around `cliclack` logging and framing primitives.
//!
//! Every `cliclack::log::*`, `cliclack::intro`, `cliclack::outro`,
//! `cliclack::note`, … returns a `std::io::Result<()>` we intentionally
//! ignore at CLI output sites. Centralising these calls behind a single set
//! of helpers gives us:
//!
//! 1. A single audit point for output behaviour (quiet mode, TTY detection,
//!    later redirection to `tracing`).
//! 2. One consistent `let _ = …` suppression so command sites stay free of
//!    `let _ =`, `drop(…)`, or `#[allow(...)]` clutter.
//!
//! Always prefer these helpers over calling `cliclack::…` directly.

/// Print a boxed intro banner, e.g. `▗ skills ▖`.
pub(crate) fn intro(message: impl std::fmt::Display) {
    let _ = cliclack::intro(message.to_string());
}

/// Print a boxed outro banner on a successful flow.
pub(crate) fn outro(message: impl std::fmt::Display) {
    let _ = cliclack::outro(message.to_string());
}

/// Print a boxed outro banner on a cancelled / aborted flow.
pub(crate) fn outro_cancel(message: impl std::fmt::Display) {
    let _ = cliclack::outro_cancel(message.to_string());
}

/// Print a titled multi-line note box.
pub(crate) fn note(title: impl std::fmt::Display, body: impl std::fmt::Display) {
    let _ = cliclack::note(title.to_string(), body.to_string());
}

/// Emit an `info` step in the prompt chain.
pub(crate) fn info(message: impl std::fmt::Display) {
    let _ = cliclack::log::info(message.to_string());
}

/// Emit a `step` log line.
pub(crate) fn step(message: impl std::fmt::Display) {
    let _ = cliclack::log::step(message.to_string());
}

/// Emit a `success` log line.
pub(crate) fn success(message: impl std::fmt::Display) {
    let _ = cliclack::log::success(message.to_string());
}

/// Emit a `warning` log line.
pub(crate) fn warning(message: impl std::fmt::Display) {
    let _ = cliclack::log::warning(message.to_string());
}

/// Emit an `error` log line.
pub(crate) fn error(message: impl std::fmt::Display) {
    let _ = cliclack::log::error(message.to_string());
}

/// Emit a secondary `remark` line (grey-indented).
pub(crate) fn remark(message: impl std::fmt::Display) {
    let _ = cliclack::log::remark(message.to_string());
}
