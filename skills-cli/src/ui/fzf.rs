//! Interactive fzf-style search prompt.
//!
//! Debounced live-search: the caller supplies a `search_fn` closure and this
//! prompt debounces keystrokes before invoking it, so an HTTP-backed search
//! doesn't fire on every character. Used by `skills find`.

use std::io::{self, Write};
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal,
};

use super::input::RawModeGuard;
use super::render::{PromptState, render_lines};
use super::style::{BOLD, CURSOR_HIDE, CURSOR_SHOW, CYAN, DIM, RESET, TEXT};

/// A search result item for the fzf prompt.
#[derive(Clone, Debug)]
pub(crate) struct FzfItem {
    /// Display label (skill name).
    pub label: String,
    /// Hint shown dim next to label (e.g. source/pkg).
    pub hint: String,
    /// Description shown as a badge (e.g. install count).
    pub description: String,
    /// Value returned on selection.
    pub value: String,
}

/// Result of the fzf search prompt.
#[derive(Debug)]
pub(crate) enum FzfResult {
    /// User selected an item.
    Selected(String),
    /// User cancelled (ESC or Ctrl-C).
    Cancelled,
}

/// Adaptive debounce delay based on query length, matching TS `find.ts`.
///
/// Short queries debounce longer (users are still typing); long queries
/// debounce briefly (users likely paused on a target term).
const fn debounce_delay(query_len: usize) -> u64 {
    match query_len {
        0..=2 => 250,
        3..=4 => 200,
        _ => 150,
    }
}

/// Run an interactive fzf-style search prompt.
///
/// `search_fn` is called with the current query (debounced) and should return
/// the ranked result list to display.
///
/// # Errors
///
/// Returns an error if terminal I/O fails.
#[allow(
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::shadow_unrelated,
    reason = "TUI event loop; closure params intentionally shadow outer state"
)]
pub(crate) fn fzf_search<F>(message: &str, search_fn: F) -> io::Result<FzfResult>
where
    F: Fn(&str) -> Vec<FzfItem>,
{
    let mut stdout = io::stdout();
    let mut query = String::new();
    let mut cursor: usize = 0;
    let mut results: Vec<FzfItem> = search_fn("");
    let max_visible: usize = 8;
    let mut height: u16 = 0;
    let mut pending_search = false;
    let mut last_input = Instant::now();

    terminal::enable_raw_mode()?;
    let guard = RawModeGuard::with_hidden_cursor();

    while event::poll(Duration::from_millis(50))? {
        let _ = event::read()?;
    }

    write!(stdout, "{CURSOR_HIDE}")?;
    stdout.flush()?;

    let cleanup = |stdout: &mut io::Stdout| -> io::Result<()> {
        write!(stdout, "{CURSOR_SHOW}")?;
        stdout.flush()
    };

    render_lines(
        &mut stdout,
        &build_lines(
            PromptState::Active,
            message,
            &query,
            cursor,
            &results,
            max_visible,
            false,
        ),
        &mut height,
    )?;

    loop {
        let poll_timeout = if pending_search {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "debounce millis fits in u64"
            )]
            let elapsed = last_input.elapsed().as_millis() as u64;
            let delay = debounce_delay(query.len());
            if elapsed >= delay {
                render_lines(
                    &mut stdout,
                    &build_lines(
                        PromptState::Active,
                        message,
                        &query,
                        cursor,
                        &results,
                        max_visible,
                        true,
                    ),
                    &mut height,
                )?;
                results = search_fn(&query);
                cursor = 0;
                pending_search = false;
                render_lines(
                    &mut stdout,
                    &build_lines(
                        PromptState::Active,
                        message,
                        &query,
                        cursor,
                        &results,
                        max_visible,
                        false,
                    ),
                    &mut height,
                )?;
                Duration::from_millis(100)
            } else {
                Duration::from_millis(delay - elapsed)
            }
        } else {
            Duration::from_millis(100)
        };

        if !event::poll(poll_timeout)? {
            continue;
        }
        let Event::Key(KeyEvent {
            code,
            modifiers,
            kind,
            ..
        }) = event::read()?
        else {
            continue;
        };
        if kind != KeyEventKind::Press {
            continue;
        }

        match code {
            KeyCode::Enter => {
                let max_cur = max_visible.min(results.len());
                if let Some(item) = results.get(cursor.min(max_cur.saturating_sub(1))) {
                    let value = item.value.clone();
                    render_lines(
                        &mut stdout,
                        &build_lines(
                            PromptState::Submit,
                            message,
                            &query,
                            cursor,
                            &results,
                            max_visible,
                            false,
                        ),
                        &mut height,
                    )?;
                    cleanup(&mut stdout)?;
                    drop(guard);
                    return Ok(FzfResult::Selected(value));
                }
            }
            KeyCode::Esc | KeyCode::Char('c')
                if code == KeyCode::Esc || modifiers.contains(KeyModifiers::CONTROL) =>
            {
                render_lines(
                    &mut stdout,
                    &build_lines(
                        PromptState::Cancel,
                        message,
                        &query,
                        cursor,
                        &results,
                        max_visible,
                        false,
                    ),
                    &mut height,
                )?;
                cleanup(&mut stdout)?;
                drop(guard);
                return Ok(FzfResult::Cancelled);
            }
            KeyCode::Up => cursor = cursor.saturating_sub(1),
            KeyCode::Down if !results.is_empty() => {
                let max_cur = max_visible.min(results.len());
                cursor = (cursor + 1).min(max_cur.saturating_sub(1));
            }
            KeyCode::Backspace => {
                query.pop();
                pending_search = true;
                last_input = Instant::now();
            }
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                query.push(c);
                pending_search = true;
                last_input = Instant::now();
            }
            _ => {}
        }

        if !pending_search {
            render_lines(
                &mut stdout,
                &build_lines(
                    PromptState::Active,
                    message,
                    &query,
                    cursor,
                    &results,
                    max_visible,
                    false,
                ),
                &mut height,
            )?;
        }
    }
}

#[allow(
    clippy::excessive_nesting,
    reason = "TUI render logic with inline formatting"
)]
fn build_lines(
    state: PromptState,
    message: &str,
    query: &str,
    cursor: usize,
    results: &[FzfItem],
    max_visible: usize,
    loading: bool,
) -> Vec<String> {
    let mut lines = Vec::new();

    match state {
        PromptState::Active => {
            lines.push(format!("{TEXT}{message}{RESET} {query}{BOLD}_{RESET}"));
            lines.push(String::new());

            if query.is_empty() || query.len() < 2 {
                lines.push(format!("{DIM}Start typing to search (min 2 chars){RESET}"));
            } else if results.is_empty() && loading {
                lines.push(format!("{DIM}Searching...{RESET}"));
            } else if results.is_empty() {
                lines.push(format!("{DIM}No skills found{RESET}"));
            } else {
                let max_show = max_visible.min(results.len());
                for (i, item) in results.iter().take(max_show).enumerate() {
                    let is_cur = i == cursor;
                    let arrow: String = if is_cur {
                        format!("{BOLD}>{RESET}")
                    } else {
                        " ".to_owned()
                    };
                    let name = if is_cur {
                        format!("{BOLD}{}{RESET}", item.label)
                    } else {
                        format!("{TEXT}{}{RESET}", item.label)
                    };
                    let source = if item.hint.is_empty() {
                        String::new()
                    } else {
                        format!(" {DIM}{}{RESET}", item.hint)
                    };
                    let badge = if item.description.is_empty() {
                        String::new()
                    } else {
                        format!(" {CYAN}{}{RESET}", item.description)
                    };
                    let loading_mark = if loading && i == 0 {
                        format!(" {DIM}...{RESET}")
                    } else {
                        String::new()
                    };
                    lines.push(format!(" {arrow} {name}{source}{badge}{loading_mark}"));
                }
            }

            lines.push(String::new());
            lines.push(format!(
                "{DIM}up/down navigate | enter select | esc cancel{RESET}"
            ));
        }
        PromptState::Submit => {
            if let Some(item) = results.get(cursor) {
                lines.push(format!("{TEXT}{message}{RESET} {}", item.label));
            }
        }
        PromptState::Cancel => {}
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debounce_delay_short_query() {
        assert_eq!(debounce_delay(0), 250);
        assert_eq!(debounce_delay(2), 250);
    }

    #[test]
    fn test_debounce_delay_medium_query() {
        assert_eq!(debounce_delay(3), 200);
        assert_eq!(debounce_delay(4), 200);
    }

    #[test]
    fn test_debounce_delay_long_query() {
        assert_eq!(debounce_delay(5), 150);
        assert_eq!(debounce_delay(100), 150);
    }
}
