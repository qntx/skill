//! Shared UI primitives for the Skills CLI.
//!
//! Provides the ASCII logo, formatting utilities, and custom interactive
//! prompt components (search-multiselect, fzf search) that give every
//! subcommand a consistent visual experience.

use std::collections::HashSet;
use std::io::{self, Write};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal,
};

const LOGO_LINES: &[&str] = &[
    "███████╗██╗  ██╗██╗██╗     ██╗     ███████╗",
    "██╔════╝██║ ██╔╝██║██║     ██║     ██╔════╝",
    "███████╗█████╔╝ ██║██║     ██║     ███████╗",
    "╚════██║██╔═██╗ ██║██║     ██║     ╚════██║",
    "███████║██║  ██╗██║███████╗███████╗███████║",
    "╚══════╝╚═╝  ╚═╝╚═╝╚══════╝╚══════╝╚══════╝",
];

const GRAYS: &[&str] = &[
    "\x1b[38;5;250m",
    "\x1b[38;5;248m",
    "\x1b[38;5;245m",
    "\x1b[38;5;243m",
    "\x1b[38;5;240m",
    "\x1b[38;5;238m",
];

pub(crate) const RESET: &str = "\x1b[0m";
pub(crate) const DIM: &str = "\x1b[38;5;102m";
pub(crate) const TEXT: &str = "\x1b[38;5;145m";
pub(crate) const GREEN: &str = "\x1b[32m";
pub(crate) const YELLOW: &str = "\x1b[33m";
pub(crate) const CYAN: &str = "\x1b[36m";
pub(crate) const BOLD: &str = "\x1b[1m";

const S_STEP_ACTIVE: &str = "\x1b[32m◆\x1b[0m";
const S_STEP_SUBMIT: &str = "\x1b[32m◇\x1b[0m";
const S_STEP_CANCEL: &str = "\x1b[31m■\x1b[0m";
const S_RADIO_ACTIVE: &str = "\x1b[32m●\x1b[0m";
const S_RADIO_INACTIVE: &str = "\x1b[2m○\x1b[0m";
const S_BULLET: &str = "\x1b[32m•\x1b[0m";
const S_BAR: &str = "\x1b[2m│\x1b[0m";
const S_BAR_H: &str = "\x1b[2m─\x1b[0m";

#[derive(Clone, Copy, PartialEq, Eq)]
enum PromptState {
    Active,
    Submit,
    Cancel,
}

impl PromptState {
    const fn icon(self) -> &'static str {
        match self {
            Self::Active => S_STEP_ACTIVE,
            Self::Submit => S_STEP_SUBMIT,
            Self::Cancel => S_STEP_CANCEL,
        }
    }
}

/// Print the SKILLS ASCII logo with a gradient effect.
pub(crate) fn show_logo() {
    println!();
    for (i, line) in LOGO_LINES.iter().enumerate() {
        let first = GRAYS.first().unwrap_or(&"");
        let gray = GRAYS.get(i).unwrap_or(first);
        println!("{gray}{line}{RESET}");
    }
}

/// Print the full banner (logo + version + usage hints).
pub(crate) fn show_banner(_version: &str) {
    show_logo();
    println!();
    println!("{DIM}The open agent skills ecosystem{RESET}");
    println!();
    println!(
        "  {DIM}${RESET} {TEXT}skills add {DIM}<package>{RESET}        {DIM}Add a new skill{RESET}"
    );
    println!(
        "  {DIM}${RESET} {TEXT}skills remove{RESET}               {DIM}Remove installed skills{RESET}"
    );
    println!(
        "  {DIM}${RESET} {TEXT}skills list{RESET}                 {DIM}List installed skills{RESET}"
    );
    println!(
        "  {DIM}${RESET} {TEXT}skills find {DIM}[query]{RESET}         {DIM}Search for skills{RESET}"
    );
    println!();
    println!(
        "  {DIM}${RESET} {TEXT}skills check{RESET}                {DIM}Check for updates{RESET}"
    );
    println!(
        "  {DIM}${RESET} {TEXT}skills update{RESET}               {DIM}Update all skills{RESET}"
    );
    println!();
    println!(
        "  {DIM}${RESET} {TEXT}skills experimental_install{RESET}  {DIM}Restore from skills-lock.json{RESET}"
    );
    println!(
        "  {DIM}${RESET} {TEXT}skills init {DIM}[name]{RESET}          {DIM}Create a new skill{RESET}"
    );
    println!(
        "  {DIM}${RESET} {TEXT}skills experimental_sync{RESET}     {DIM}Sync skills from node_modules{RESET}"
    );
    println!();
    println!("{DIM}try:{RESET} skills add vercel-labs/agent-skills");
    println!();
    println!("Discover more skills at {TEXT}https://skills.sh/{RESET}");
    println!();
}

/// Drain any stale key events from the terminal input buffer.
///
/// On Windows, crossterm generates both `Press` and `Release` events for every
/// keypress. When a previous prompt (e.g. `cliclack::multiselect`) consumes the
/// `Press` event, the `Release` event remains buffered. If the next prompt reads
/// it without filtering, it auto-confirms instantly.
///
/// Call this before any `cliclack` `.interact()` call to prevent cascading
/// auto-confirmation on Windows.
pub(crate) fn drain_input_events() {
    let _ = terminal::enable_raw_mode();
    while event::poll(std::time::Duration::from_millis(10)).unwrap_or(false) {
        let _ = event::read();
    }
    let _ = terminal::disable_raw_mode();
}

/// Shorten a path for display by replacing the home directory with `~`
/// and the current directory with `.`.
#[must_use]
#[allow(dead_code, reason = "utility available for future use")]
pub(crate) fn shorten_path(path: &std::path::Path) -> String {
    shorten_path_with_cwd(path, &std::env::current_dir().unwrap_or_default())
}

/// Shorten a path relative to a given `cwd`.
#[must_use]
pub(crate) fn shorten_path_with_cwd(path: &std::path::Path, cwd: &std::path::Path) -> String {
    // Check cwd first so project-relative paths take priority over home-relative.
    if let Ok(suffix) = path.strip_prefix(cwd) {
        return if suffix.as_os_str().is_empty() {
            ".".to_owned()
        } else {
            format!(".{}{}", std::path::MAIN_SEPARATOR, suffix.display())
        };
    }
    if let Some(home) = dirs::home_dir()
        && let Ok(suffix) = path.strip_prefix(&home)
    {
        return if suffix.as_os_str().is_empty() {
            "~".to_owned()
        } else {
            format!("~{}{}", std::path::MAIN_SEPARATOR, suffix.display())
        };
    }
    path.display().to_string()
}

/// Format items as `"a, b, c"`, truncating with `"+N more"` when needed.
///
/// Matches the `TypeScript` `formatList(items, maxShow = 5)` behaviour.
#[must_use]
pub(crate) fn format_list(items: &[String]) -> String {
    format_list_max(items, 5)
}

/// Format with a custom truncation threshold.
#[must_use]
pub(crate) fn format_list_max(items: &[String], max_show: usize) -> String {
    if items.is_empty() {
        return String::new();
    }
    if items.len() <= max_show {
        return items.join(", ");
    }
    let shown = items.get(..max_show).unwrap_or(items);
    let remaining = items.len().saturating_sub(max_show);
    format!("{} +{remaining} more", shown.join(", "))
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
fn render_lines(stdout: &mut io::Stdout, lines: &[String], height: &mut u16) -> io::Result<()> {
    clear_rendered(stdout, *height)?;
    for line in lines {
        write!(stdout, "\x1b[2K{line}\r\n")?;
    }
    *height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    stdout.flush()
}

/// A single selectable item.
#[derive(Clone)]
pub(crate) struct SearchItem {
    /// The value returned on selection.
    pub value: String,
    /// Display label.
    pub label: String,
    /// Optional hint text shown dimmed next to the label.
    pub hint: Option<String>,
}

/// A locked section whose items are always included in the result.
pub(crate) struct LockedSection {
    /// Section title (e.g. "Universal agents").
    pub title: String,
    /// Items that are always selected.
    pub items: Vec<SearchItem>,
}

/// Options for the search multiselect prompt.
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

/// Result of the search multiselect prompt.
pub(crate) enum SearchMultiselectResult {
    /// User confirmed selection.
    Selected(Vec<String>),
    /// User cancelled.
    Cancelled,
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
fn build_multiselect_lines(
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
    let mut lines = vec![format!("{}  \x1b[1m{message}\x1b[0m", state.icon())];

    match state {
        PromptState::Active => {
            if let Some(ls) = locked
                && !ls.items.is_empty()
            {
                lines.push(S_BAR.to_owned());
                lines.push(format!(
                        "{S_BAR}  {S_BAR_H}{S_BAR_H} \x1b[1m{}\x1b[0m \x1b[2m── always included\x1b[0m {}{S_BAR_H}",
                        ls.title,
                        S_BAR_H.repeat(12)
                    ));
                for item in &ls.items {
                    lines.push(format!(
                        "{S_BAR}    {S_BULLET} \x1b[1m{}\x1b[0m",
                        item.label
                    ));
                }
                lines.push(S_BAR.to_owned());
                lines.push(format!(
                    "{S_BAR}  {S_BAR_H}{S_BAR_H} \x1b[1mAdditional agents\x1b[0m {}",
                    S_BAR_H.repeat(29)
                ));
            }

            lines.push(format!(
                "{S_BAR}  \x1b[2mSearch:\x1b[0m {query}\x1b[7m \x1b[0m"
            ));
            lines.push(format!(
                "{S_BAR}  \x1b[2m↑↓ move, space select, enter confirm\x1b[0m"
            ));
            lines.push(S_BAR.to_owned());

            if filtered.is_empty() {
                lines.push(format!("{S_BAR}  \x1b[2mNo matches found\x1b[0m"));
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
                        format!("\x1b[4m{}\x1b[0m", item.label)
                    } else {
                        item.label.clone()
                    };
                    let hint = item
                        .hint
                        .as_ref()
                        .map_or(String::new(), |h| format!(" \x1b[2m({h})\x1b[0m"));
                    let arrow = if is_cur { "\x1b[36m❯\x1b[0m" } else { " " };
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
                    lines.push(format!("{S_BAR}  \x1b[2m{}\x1b[0m", parts.join("  ")));
                }
            }

            lines.push(S_BAR.to_owned());
            let labels = collect_labels(locked, items, selected);
            if labels.is_empty() {
                lines.push(format!("{S_BAR}  \x1b[2mSelected: (none)\x1b[0m"));
            } else {
                lines.push(format!(
                    "{S_BAR}  \x1b[32mSelected:\x1b[0m {}",
                    format_summary(&labels)
                ));
            }
            lines.push("\x1b[2m└\x1b[0m".to_owned());
        }
        PromptState::Submit => {
            let labels = collect_labels(locked, items, selected);
            lines.push(format!("{S_BAR}  \x1b[2m{}\x1b[0m", labels.join(", ")));
        }
        PromptState::Cancel => {
            lines.push(format!("{S_BAR}  \x1b[9m\x1b[2mCancelled\x1b[0m"));
        }
    }

    lines
}

/// RAII guard that ensures `terminal::disable_raw_mode()` is called on drop,
/// even if the function returns early via `?` or panics. Optionally restores
/// cursor visibility when `hide_cursor` was set.
struct RawModeGuard {
    restore_cursor: bool,
}

impl RawModeGuard {
    const fn new() -> Self {
        Self {
            restore_cursor: false,
        }
    }

    const fn with_hidden_cursor() -> Self {
        Self {
            restore_cursor: true,
        }
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if self.restore_cursor {
            let _ = write!(io::stdout(), "\x1b[?25h");
            let _ = io::stdout().flush();
        }
        let _ = terminal::disable_raw_mode();
    }
}

/// Run the interactive search multiselect prompt.
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
        let lines = build_multiselect_lines(
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

    // Drain any stale key events left in the terminal buffer by the previous
    // prompt (e.g. cliclack::multiselect). Without this, a residual Enter
    // keypress can auto-confirm this prompt instantly.
    while event::poll(std::time::Duration::from_millis(50))? {
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
        if !event::poll(std::time::Duration::from_millis(100))? {
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

        // On Windows, crossterm generates both Press and Release events for
        // every keypress. Only process Press to avoid duplicate handling.
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

/// A search result item for the fzf prompt.
#[derive(Clone)]
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
pub(crate) enum FzfResult {
    /// User selected an item.
    Selected(String),
    /// User cancelled.
    Cancelled,
}

#[allow(
    clippy::excessive_nesting,
    reason = "TUI render logic with inline formatting"
)]
fn build_fzf_lines(
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
            lines.push(format!("{TEXT}{message}{RESET} {query}\x1b[1m_\x1b[0m"));
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
                    let arrow = if is_cur { "\x1b[1m>\x1b[0m" } else { " " };
                    let name = if is_cur {
                        format!("\x1b[1m{}\x1b[0m", item.label)
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
                        format!(" \x1b[36m{}\x1b[0m", item.description)
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

/// Run an interactive fzf-style search prompt.
///
/// `search_fn` is called with the current query and should return results.
///
/// # Errors
///
/// Returns an error if terminal I/O fails.
/// Adaptive debounce delay matching TS find.ts behavior.
const fn debounce_delay(query_len: usize) -> u64 {
    match query_len {
        0..=2 => 250,
        3..=4 => 200,
        _ => 150,
    }
}

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
    let mut last_input = std::time::Instant::now();

    terminal::enable_raw_mode()?;
    let guard = RawModeGuard::with_hidden_cursor();

    // Drain stale key events from previous prompts.
    while event::poll(std::time::Duration::from_millis(50))? {
        let _ = event::read()?;
    }

    write!(stdout, "\x1b[?25l")?;
    stdout.flush()?;

    let cleanup = |stdout: &mut io::Stdout| -> io::Result<()> {
        write!(stdout, "\x1b[?25h")?;
        stdout.flush()
    };

    render_lines(
        &mut stdout,
        &build_fzf_lines(
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
                    &build_fzf_lines(
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
                    &build_fzf_lines(
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
                std::time::Duration::from_millis(100)
            } else {
                std::time::Duration::from_millis(delay - elapsed)
            }
        } else {
            std::time::Duration::from_millis(100)
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

        // On Windows, crossterm generates both Press and Release events for
        // every keypress. Only process Press to avoid duplicate handling.
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
                        &build_fzf_lines(
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
                    &build_fzf_lines(
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
                last_input = std::time::Instant::now();
            }
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                query.push(c);
                pending_search = true;
                last_input = std::time::Instant::now();
            }
            _ => {}
        }

        if !pending_search {
            render_lines(
                &mut stdout,
                &build_fzf_lines(
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
