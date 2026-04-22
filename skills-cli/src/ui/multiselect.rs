//! Interactive search-multiselect prompt.
//!
//! Rendered as a scrollable list with incremental substring search, optional
//! pinned "locked" section (items that are always part of the result), and
//! keyboard-driven navigation. Used by `skills add` to pick agents + skills.

use std::collections::HashSet;
use std::io;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal,
};

use super::input::RawModeGuard;
use super::render::{PromptState, render_lines};
use super::style::{
    BOLD, CURSOR_ARROW, FAINT, GREEN, RESET, REVERSE, S_BAR, S_BAR_H, S_BULLET, S_RADIO_ACTIVE,
    S_RADIO_INACTIVE, STRIKE, UNDERLINE,
};

/// One selectable item.
#[derive(Clone, Debug)]
pub(crate) struct SearchItem {
    /// Value returned when the item is chosen.
    pub value: String,
    /// Display label.
    pub label: String,
    /// Optional hint text shown dimmed next to the label.
    pub hint: Option<String>,
}

/// A locked section whose items are always included in the result.
#[derive(Debug)]
pub(crate) struct LockedSection {
    /// Section title (e.g. "Universal agents").
    pub title: String,
    /// Items that are always selected.
    pub items: Vec<SearchItem>,
}

/// Options for the search-multiselect prompt.
#[derive(Debug)]
pub(crate) struct SearchMultiselectOptions {
    /// Prompt message.
    pub message: String,
    /// Selectable items.
    pub items: Vec<SearchItem>,
    /// Maximum visible items in the viewport.
    pub max_visible: usize,
    /// Initially selected values.
    pub initial_selected: Vec<String>,
    /// Whether at least one item must be selected.
    pub required: bool,
    /// Optional locked section shown above the list.
    pub locked_section: Option<LockedSection>,
}

/// Result of the search-multiselect prompt.
#[derive(Debug)]
pub(crate) enum SearchMultiselectResult {
    /// User confirmed selection.
    Selected(Vec<String>),
    /// User cancelled (ESC or Ctrl-C).
    Cancelled,
}

/// Run the interactive search-multiselect prompt.
///
/// # Errors
///
/// Returns an error if terminal raw mode or I/O operations fail.
#[allow(
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::indexing_slicing,
    clippy::shadow_unrelated,
    reason = "TUI event loop; closure params intentionally shadow outer state"
)]
pub(crate) fn search_multiselect(
    opts: &SearchMultiselectOptions,
) -> io::Result<SearchMultiselectResult> {
    let mut stdout = io::stdout();
    let mut query = String::new();
    let mut cursor: usize = 0;
    let mut selected: HashSet<String> = opts.initial_selected.iter().cloned().collect();
    let mut height: u16 = 0;

    let locked_values: Vec<String> = opts
        .locked_section
        .as_ref()
        .map(|ls| ls.items.iter().map(|i| i.value.clone()).collect())
        .unwrap_or_default();

    let render = |stdout: &mut io::Stdout,
                  state,
                  query: &str,
                  cursor,
                  selected: &HashSet<String>,
                  height: &mut u16| {
        let lines = build_lines(
            state,
            &opts.message,
            query,
            cursor,
            selected,
            &opts.items,
            opts.locked_section.as_ref(),
            opts.max_visible,
        );
        render_lines(stdout, &lines, height)
    };

    terminal::enable_raw_mode()?;
    let guard = RawModeGuard::new();

    // Drain stale key events left by the previous prompt (e.g.
    // `cliclack::multiselect`). Without this, a residual Enter keypress can
    // auto-confirm this prompt instantly on Windows.
    while event::poll(Duration::from_millis(50))? {
        let _ = event::read()?;
    }

    render(
        &mut stdout,
        PromptState::Active,
        &query,
        cursor,
        &selected,
        &mut height,
    )?;

    loop {
        if !event::poll(Duration::from_millis(100))? {
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

        let filtered = filtered_indices(&opts.items, &query);

        match code {
            KeyCode::Enter => {
                if opts.required && selected.is_empty() && locked_values.is_empty() {
                    continue;
                }
                render(
                    &mut stdout,
                    PromptState::Submit,
                    &query,
                    cursor,
                    &selected,
                    &mut height,
                )?;
                drop(guard);
                let mut result = locked_values;
                for item in &opts.items {
                    if selected.contains(&item.value) {
                        result.push(item.value.clone());
                    }
                }
                return Ok(SearchMultiselectResult::Selected(result));
            }
            KeyCode::Esc | KeyCode::Char('c')
                if code == KeyCode::Esc || modifiers.contains(KeyModifiers::CONTROL) =>
            {
                render(
                    &mut stdout,
                    PromptState::Cancel,
                    &query,
                    cursor,
                    &selected,
                    &mut height,
                )?;
                drop(guard);
                return Ok(SearchMultiselectResult::Cancelled);
            }
            KeyCode::Up => cursor = cursor.saturating_sub(1),
            KeyCode::Down if !filtered.is_empty() => {
                cursor = (cursor + 1).min(filtered.len() - 1);
            }
            KeyCode::Char(' ') => {
                if let Some(&idx) = filtered.get(cursor) {
                    let val = &opts.items[idx].value;
                    if !selected.remove(val) {
                        selected.insert(val.clone());
                    }
                }
            }
            KeyCode::Backspace => {
                query.pop();
                cursor = 0;
            }
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                query.push(c);
                cursor = 0;
            }
            _ => {}
        }

        render(
            &mut stdout,
            PromptState::Active,
            &query,
            cursor,
            &selected,
            &mut height,
        )?;
    }
}

fn matches_query(item: &SearchItem, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let lq = query.to_lowercase();
    item.label.to_lowercase().contains(&lq) || item.value.to_lowercase().contains(&lq)
}

fn filtered_indices(items: &[SearchItem], query: &str) -> Vec<usize> {
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| matches_query(item, query))
        .map(|(i, _)| i)
        .collect()
}

fn visible_range(total: usize, cursor: usize, max_vis: usize) -> (usize, usize) {
    if total <= max_vis {
        return (0, total);
    }
    let start = cursor
        .saturating_sub(max_vis / 2)
        .min(total.saturating_sub(max_vis));
    (start, total.min(start + max_vis))
}

fn collect_labels<'a>(
    locked: Option<&'a LockedSection>,
    items: &'a [SearchItem],
    selected: &HashSet<String>,
) -> Vec<&'a str> {
    let mut labels: Vec<&str> = Vec::new();
    if let Some(ls) = locked {
        labels.extend(ls.items.iter().map(|i| i.label.as_str()));
    }
    labels.extend(
        items
            .iter()
            .filter(|i| selected.contains(&i.value))
            .map(|i| i.label.as_str()),
    );
    labels
}

fn format_summary(labels: &[&str]) -> String {
    if labels.is_empty() {
        return "(none)".to_owned();
    }
    if labels.len() <= 3 {
        return labels.join(", ");
    }
    let shown = labels.get(..3).unwrap_or(labels);
    format!(
        "{} +{} more",
        shown.join(", "),
        labels.len().saturating_sub(3)
    )
}

#[allow(
    clippy::too_many_arguments,
    clippy::excessive_nesting,
    clippy::indexing_slicing,
    reason = "render function needs all display state; indices are pre-filtered"
)]
fn build_lines(
    state: PromptState,
    message: &str,
    query: &str,
    cursor: usize,
    selected: &HashSet<String>,
    items: &[SearchItem],
    locked: Option<&LockedSection>,
    max_vis: usize,
) -> Vec<String> {
    let filtered = filtered_indices(items, query);
    let mut lines = vec![format!("{}  {BOLD}{message}{RESET}", state.icon())];

    match state {
        PromptState::Active => {
            if let Some(ls) = locked
                && !ls.items.is_empty()
            {
                lines.push(S_BAR.to_owned());
                lines.push(format!(
                    "{S_BAR}  {S_BAR_H}{S_BAR_H} {BOLD}{}{RESET} {FAINT}── always included{RESET} {}{S_BAR_H}",
                    ls.title,
                    S_BAR_H.repeat(12)
                ));
                for item in &ls.items {
                    lines.push(format!("{S_BAR}    {S_BULLET} {BOLD}{}{RESET}", item.label));
                }
                lines.push(S_BAR.to_owned());
                lines.push(format!(
                    "{S_BAR}  {S_BAR_H}{S_BAR_H} {BOLD}Additional agents{RESET} {}",
                    S_BAR_H.repeat(29)
                ));
            }

            lines.push(format!(
                "{S_BAR}  {FAINT}Search:{RESET} {query}{REVERSE} {RESET}"
            ));
            lines.push(format!(
                "{S_BAR}  {FAINT}↑↓ move, space select, enter confirm{RESET}"
            ));
            lines.push(S_BAR.to_owned());

            if filtered.is_empty() {
                lines.push(format!("{S_BAR}  {FAINT}No matches found{RESET}"));
            } else {
                let (start, end) = visible_range(filtered.len(), cursor, max_vis);
                for (vi, &idx) in filtered.iter().enumerate().take(end).skip(start) {
                    let item = &items[idx];
                    let is_cur = vi == cursor;
                    let radio = if selected.contains(&item.value) {
                        S_RADIO_ACTIVE
                    } else {
                        S_RADIO_INACTIVE
                    };
                    let label = if is_cur {
                        format!("{UNDERLINE}{}{RESET}", item.label)
                    } else {
                        item.label.clone()
                    };
                    let hint = item
                        .hint
                        .as_ref()
                        .map_or(String::new(), |h| format!(" {FAINT}({h}){RESET}"));
                    let arrow = if is_cur { CURSOR_ARROW } else { " " };
                    lines.push(format!("{S_BAR} {arrow} {radio} {label}{hint}"));
                }

                let hidden_before = start;
                let hidden_after = filtered.len().saturating_sub(end);
                if hidden_before > 0 || hidden_after > 0 {
                    let mut parts = Vec::new();
                    if hidden_before > 0 {
                        parts.push(format!("↑ {hidden_before} more"));
                    }
                    if hidden_after > 0 {
                        parts.push(format!("↓ {hidden_after} more"));
                    }
                    lines.push(format!("{S_BAR}  {FAINT}{}{RESET}", parts.join("  ")));
                }
            }

            lines.push(S_BAR.to_owned());
            let labels = collect_labels(locked, items, selected);
            if labels.is_empty() {
                lines.push(format!("{S_BAR}  {FAINT}Selected: (none){RESET}"));
            } else {
                lines.push(format!(
                    "{S_BAR}  {GREEN}Selected:{RESET} {}",
                    format_summary(&labels)
                ));
            }
            lines.push(format!("{FAINT}└{RESET}"));
        }
        PromptState::Submit => {
            let labels = collect_labels(locked, items, selected);
            lines.push(format!("{S_BAR}  {FAINT}{}{RESET}", labels.join(", ")));
        }
        PromptState::Cancel => {
            lines.push(format!("{S_BAR}  {STRIKE}{FAINT}Cancelled{RESET}"));
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str) -> SearchItem {
        SearchItem {
            value: label.to_owned(),
            label: label.to_owned(),
            hint: None,
        }
    }

    #[test]
    fn test_matches_query_empty_matches_all() {
        assert!(matches_query(&item("anything"), ""));
    }

    #[test]
    fn test_matches_query_case_insensitive() {
        assert!(matches_query(&item("CursorAgent"), "cursor"));
        assert!(!matches_query(&item("claude"), "cursor"));
    }

    #[test]
    fn test_visible_range_fits_in_viewport() {
        assert_eq!(visible_range(3, 0, 5), (0, 3));
    }

    #[test]
    fn test_visible_range_centres_on_cursor() {
        // cursor mid-list: start = cursor - max/2, end = start + max
        assert_eq!(visible_range(20, 10, 6), (7, 13));
    }

    #[test]
    fn test_visible_range_clamps_at_end() {
        assert_eq!(visible_range(20, 19, 6), (14, 20));
    }

    #[test]
    fn test_format_summary_empty() {
        assert_eq!(format_summary(&[]), "(none)");
    }

    #[test]
    fn test_format_summary_under_three() {
        assert_eq!(format_summary(&["a", "b"]), "a, b");
    }

    #[test]
    fn test_format_summary_truncates_over_three() {
        assert_eq!(
            format_summary(&["a", "b", "c", "d", "e"]),
            "a, b, c +2 more"
        );
    }
}
