//! Shared UI helpers for the Skills CLI.
//!
//! Centralises colour styles, the ASCII logo, formatting utilities, and
//! custom interactive prompt components (search-multiselect, fzf search)
//! so that every subcommand produces a consistent visual experience
//! matching the official `TypeScript` `@clack/prompts` aesthetic.

use std::collections::HashSet;
use std::io::{self, Write};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal,
};

// ── ANSI Constants (matching TS cli.ts) ──────────────────────────────

/// ANSI logo lines for the skills banner.
const LOGO_LINES: &[&str] = &[
    "███████╗██╗  ██╗██╗██╗     ██╗     ███████╗",
    "██╔════╝██║ ██╔╝██║██║     ██║     ██╔════╝",
    "███████╗█████╔╝ ██║██║     ██║     ███████╗",
    "╚════██║██╔═██╗ ██║██║     ██║     ╚════██║",
    "███████║██║  ██╗██║███████╗███████╗███████║",
    "╚══════╝╚═╝  ╚═╝╚═╝╚══════╝╚══════╝╚══════╝",
];

/// 256-color middle grays – visible on both light and dark backgrounds.
const GRAYS: &[&str] = &[
    "\x1b[38;5;250m",
    "\x1b[38;5;248m",
    "\x1b[38;5;245m",
    "\x1b[38;5;243m",
    "\x1b[38;5;240m",
    "\x1b[38;5;238m",
];

const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[38;5;102m";
const TEXT: &str = "\x1b[38;5;145m";

// ── Clack-style symbols (matching TS @clack/prompts) ─────────────────

const S_STEP_ACTIVE: &str = "\x1b[32m◆\x1b[0m";
const S_STEP_SUBMIT: &str = "\x1b[32m◇\x1b[0m";
const S_STEP_CANCEL: &str = "\x1b[31m■\x1b[0m";
const S_RADIO_ACTIVE: &str = "\x1b[32m●\x1b[0m";
const S_RADIO_INACTIVE: &str = "\x1b[2m○\x1b[0m";
#[allow(dead_code)]
const S_CHECKBOX_ACTIVE: &str = "\x1b[32m◻\x1b[0m";
#[allow(dead_code)]
const S_CHECKBOX_INACTIVE: &str = "\x1b[2m◻\x1b[0m";
const S_BULLET: &str = "\x1b[32m•\x1b[0m";
const S_BAR: &str = "\x1b[2m│\x1b[0m";
const S_BAR_H: &str = "\x1b[2m─\x1b[0m";

// ── Logo & Banner ────────────────────────────────────────────────────

/// Print the SKILLS ASCII logo with a gradient effect.
pub fn show_logo() {
    println!();
    for (i, line) in LOGO_LINES.iter().enumerate() {
        let gray = GRAYS.get(i).unwrap_or(&GRAYS[0]);
        println!("{gray}{line}{RESET}");
    }
}

/// Print the full banner (logo + usage hints).
pub fn show_banner(_version: &str) {
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
        "  {DIM}${RESET} {TEXT}skills experimental_install{RESET} {DIM}Restore from skills-lock.json{RESET}"
    );
    println!(
        "  {DIM}${RESET} {TEXT}skills init {DIM}[name]{RESET}          {DIM}Create a new skill{RESET}"
    );
    println!(
        "  {DIM}${RESET} {TEXT}skills experimental_sync{RESET}    {DIM}Sync skills from node_modules{RESET}"
    );
    println!();
    println!("{DIM}try:{RESET} skills add vercel-labs/agent-skills");
    println!();
    println!("Discover more skills at {TEXT}https://skills.sh/{RESET}");
    println!();
}

// ── Path & Formatting Utilities ──────────────────────────────────────

/// Shorten a path for display by replacing the home directory with `~`.
#[must_use]
pub fn shorten_path(path: &std::path::Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(suffix) = path.strip_prefix(&home)
    {
        return format!("~/{}", suffix.display());
    }
    path.display().to_string()
}

/// Format a list of items as a comma-separated string suitable for display.
#[must_use]
pub fn format_list(items: &[String]) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        _ => {
            let last = &items[items.len() - 1];
            let rest = &items[..items.len() - 1];
            format!("{} and {last}", rest.join(", "))
        }
    }
}

// ── Styled Output Helpers ────────────────────────────────────────────

/// Print a styled note box (header + body) matching clack's `p.note()`.
#[allow(dead_code)]
pub fn print_note(header: &str, body: &str) {
    use console::style;
    println!();
    println!("  {}", style(header).bold());
    for line in body.lines() {
        println!("  {}", style(line).dim());
    }
    println!();
}

/// Styled heading for section output.
#[allow(dead_code)]
pub fn print_heading(text: &str) {
    use console::style;
    println!("  {}", style(text).bold());
}

/// Styled dim text.
#[allow(dead_code)]
pub fn print_dim(text: &str) {
    use console::style;
    println!("  {}", style(text).dim());
}

/// Print a success line with green check mark.
pub fn print_success(text: &str) {
    use console::style;
    println!("  {} {text}", style("✓").green());
}

/// Print a warning line with yellow marker.
#[allow(dead_code)]
pub fn print_warning(text: &str) {
    use console::style;
    println!("  {} {text}", style("⚠").yellow());
}

/// Print an error line with red marker.
pub fn print_error(text: &str) {
    use console::style;
    println!("  {} {text}", style("✗").red());
}

/// Print a cyan info bullet.
#[allow(dead_code)]
pub fn print_info(label: &str, value: &str) {
    use console::style;
    println!("    {} {}", style(label).cyan(), value);
}

/// Print a dim source line.
#[allow(dead_code)]
pub fn print_source(source: &str) {
    use console::style;
    println!("      {}", style(format!("source: {source}")).dim());
}

// ── Search Multiselect Component ─────────────────────────────────────
//
// A raw-terminal interactive prompt matching the TS `search-multiselect.ts`:
// - Locked section (always-selected items displayed above)
// - Fuzzy search filtering
// - Up/Down navigation, Space toggle, Enter confirm, Esc cancel
// - Scrollable viewport with overflow indicators

/// A single selectable item.
#[derive(Clone)]
pub struct SearchItem {
    /// The value returned on selection.
    pub value: String,
    /// Display label.
    pub label: String,
    /// Optional hint text shown dimmed next to the label.
    pub hint: Option<String>,
}

/// A locked section whose items are always included in the result.
pub struct LockedSection {
    /// Section title (e.g. "Universal agents").
    pub title: String,
    /// Items that are always selected.
    pub items: Vec<SearchItem>,
}

/// Options for the search multiselect prompt.
pub struct SearchMultiselectOptions {
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
pub enum SearchMultiselectResult {
    /// User confirmed selection.
    Selected(Vec<String>),
    /// User cancelled.
    Cancelled,
}

/// Run the interactive search multiselect prompt.
///
/// # Errors
///
/// Returns an error if terminal raw mode or I/O operations fail.
#[allow(clippy::cognitive_complexity)]
pub fn search_multiselect(opts: &SearchMultiselectOptions) -> io::Result<SearchMultiselectResult> {
    let mut stdout = io::stdout();
    let mut query = String::new();
    let mut cursor_pos: usize = 0;
    let mut selected: HashSet<String> = opts.initial_selected.iter().cloned().collect();
    let max_visible = opts.max_visible;

    let locked_values: Vec<String> = opts
        .locked_section
        .as_ref()
        .map(|ls| ls.items.iter().map(|i| i.value.clone()).collect())
        .unwrap_or_default();

    let filter = |item: &SearchItem, q: &str| -> bool {
        if q.is_empty() {
            return true;
        }
        let lq = q.to_lowercase();
        item.label.to_lowercase().contains(&lq) || item.value.to_lowercase().contains(&lq)
    };

    let get_filtered = |items: &[SearchItem], q: &str| -> Vec<usize> {
        items
            .iter()
            .enumerate()
            .filter(|(_, item)| filter(item, q))
            .map(|(i, _)| i)
            .collect()
    };

    // Build render lines for a given state
    let build_lines = |state: &str,
                       query: &str,
                       cursor_pos: usize,
                       selected: &HashSet<String>,
                       items: &[SearchItem],
                       locked: &Option<LockedSection>,
                       _locked_values: &[String],
                       max_vis: usize|
     -> Vec<String> {
        let filtered = get_filtered(items, query);
        let mut lines = Vec::new();

        // Header
        let icon = match state {
            "submit" => S_STEP_SUBMIT,
            "cancel" => S_STEP_CANCEL,
            _ => S_STEP_ACTIVE,
        };
        lines.push(format!("{icon}  \x1b[1m{}\x1b[0m", opts.message));

        if state == "active" {
            // Locked section
            if let Some(ls) = locked
                && !ls.items.is_empty()
            {
                lines.push(S_BAR.to_string());
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
                lines.push(S_BAR.to_string());
                lines.push(format!(
                    "{S_BAR}  {S_BAR_H}{S_BAR_H} \x1b[1mAdditional agents\x1b[0m {}",
                    S_BAR_H.repeat(29)
                ));
            }

            // Search input
            lines.push(format!(
                "{S_BAR}  \x1b[2mSearch:\x1b[0m {query}\x1b[7m \x1b[0m"
            ));
            // Hint
            lines.push(format!(
                "{S_BAR}  \x1b[2m↑↓ move, space select, enter confirm\x1b[0m"
            ));
            lines.push(S_BAR.to_string());

            // Items
            if filtered.is_empty() {
                lines.push(format!("{S_BAR}  \x1b[2mNo matches found\x1b[0m"));
            } else {
                let visible_start = if filtered.len() <= max_vis {
                    0
                } else {
                    cursor_pos
                        .saturating_sub(max_vis / 2)
                        .min(filtered.len().saturating_sub(max_vis))
                };
                let visible_end = filtered.len().min(visible_start + max_vis);

                for (vi, &idx) in filtered
                    .iter()
                    .enumerate()
                    .take(visible_end)
                    .skip(visible_start)
                {
                    let item = &items[idx];
                    let is_selected = selected.contains(&item.value);
                    let is_cursor = vi == cursor_pos;

                    let radio = if is_selected {
                        S_RADIO_ACTIVE
                    } else {
                        S_RADIO_INACTIVE
                    };
                    let label = if is_cursor {
                        format!("\x1b[4m{}\x1b[0m", item.label)
                    } else {
                        item.label.clone()
                    };
                    let hint = item
                        .hint
                        .as_ref()
                        .map(|h| format!(" \x1b[2m({h})\x1b[0m"))
                        .unwrap_or_default();
                    let prefix = if is_cursor { "\x1b[36m❯\x1b[0m" } else { " " };

                    lines.push(format!("{S_BAR} {prefix} {radio} {label}{hint}"));
                }

                // Overflow indicators
                let hidden_before = visible_start;
                let hidden_after = filtered.len().saturating_sub(visible_end);
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

            // Selected summary
            lines.push(S_BAR.to_string());
            let mut all_labels: Vec<&str> = Vec::new();
            if let Some(ls) = locked {
                for item in &ls.items {
                    all_labels.push(&item.label);
                }
            }
            for item in items {
                if selected.contains(&item.value) {
                    all_labels.push(&item.label);
                }
            }
            if all_labels.is_empty() {
                lines.push(format!("{S_BAR}  \x1b[2mSelected: (none)\x1b[0m"));
            } else {
                let summary = if all_labels.len() <= 3 {
                    all_labels.join(", ")
                } else {
                    format!(
                        "{} +{} more",
                        all_labels[..3].join(", "),
                        all_labels.len() - 3
                    )
                };
                lines.push(format!("{S_BAR}  \x1b[32mSelected:\x1b[0m {summary}"));
            }

            lines.push("\x1b[2m└\x1b[0m".to_string());
        } else if state == "submit" {
            let mut all_labels: Vec<&str> = Vec::new();
            if let Some(ls) = locked {
                for item in &ls.items {
                    all_labels.push(&item.label);
                }
            }
            for item in items {
                if selected.contains(&item.value) {
                    all_labels.push(&item.label);
                }
            }
            lines.push(format!("{S_BAR}  \x1b[2m{}\x1b[0m", all_labels.join(", ")));
        } else if state == "cancel" {
            lines.push(format!("{S_BAR}  \x1b[9m\x1b[2mCancelled\x1b[0m"));
        }

        lines
    };

    // Enable raw mode
    terminal::enable_raw_mode()?;

    let mut last_height: u16 = 0;

    let render = |stdout: &mut io::Stdout,
                  state: &str,
                  query: &str,
                  cursor_pos: usize,
                  selected: &HashSet<String>,
                  last_height: &mut u16|
     -> io::Result<()> {
        // Clear previous render
        if *last_height > 0 {
            write!(stdout, "\x1b[{}A", *last_height)?;
            for _ in 0..*last_height {
                write!(stdout, "\x1b[2K\x1b[1B")?;
            }
            write!(stdout, "\x1b[{}A", *last_height)?;
        }

        let lines = build_lines(
            state,
            query,
            cursor_pos,
            selected,
            &opts.items,
            &opts.locked_section,
            &locked_values,
            max_visible,
        );

        for line in &lines {
            write!(stdout, "\x1b[2K{line}\r\n")?;
        }
        #[allow(clippy::cast_possible_truncation)]
        {
            *last_height = lines.len() as u16;
        }
        stdout.flush()?;
        Ok(())
    };

    // Initial render
    render(
        &mut stdout,
        "active",
        &query,
        cursor_pos,
        &selected,
        &mut last_height,
    )?;

    loop {
        if event::poll(std::time::Duration::from_millis(100))?
            && let Event::Key(KeyEvent {
                code, modifiers, ..
            }) = event::read()?
        {
            let filtered = get_filtered(&opts.items, &query);

            match code {
                KeyCode::Enter => {
                    if opts.required && selected.is_empty() && locked_values.is_empty() {
                        continue;
                    }
                    render(
                        &mut stdout,
                        "submit",
                        &query,
                        cursor_pos,
                        &selected,
                        &mut last_height,
                    )?;
                    terminal::disable_raw_mode()?;
                    let mut result = locked_values.clone();
                    // Preserve order from items list
                    for item in &opts.items {
                        if selected.contains(&item.value) {
                            result.push(item.value.clone());
                        }
                    }
                    return Ok(SearchMultiselectResult::Selected(result));
                }
                KeyCode::Esc => {
                    render(
                        &mut stdout,
                        "cancel",
                        &query,
                        cursor_pos,
                        &selected,
                        &mut last_height,
                    )?;
                    terminal::disable_raw_mode()?;
                    return Ok(SearchMultiselectResult::Cancelled);
                }
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                    render(
                        &mut stdout,
                        "cancel",
                        &query,
                        cursor_pos,
                        &selected,
                        &mut last_height,
                    )?;
                    terminal::disable_raw_mode()?;
                    return Ok(SearchMultiselectResult::Cancelled);
                }
                KeyCode::Up => {
                    cursor_pos = cursor_pos.saturating_sub(1);
                }
                KeyCode::Down => {
                    if !filtered.is_empty() {
                        cursor_pos = (cursor_pos + 1).min(filtered.len() - 1);
                    }
                }
                KeyCode::Char(' ') => {
                    if let Some(&idx) = filtered.get(cursor_pos) {
                        let val = &opts.items[idx].value;
                        if selected.contains(val) {
                            selected.remove(val);
                        } else {
                            selected.insert(val.clone());
                        }
                    }
                }
                KeyCode::Backspace => {
                    query.pop();
                    cursor_pos = 0;
                }
                KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                    query.push(c);
                    cursor_pos = 0;
                }
                _ => {}
            }

            render(
                &mut stdout,
                "active",
                &query,
                cursor_pos,
                &selected,
                &mut last_height,
            )?;
        }
    }
}

// ── FZF-Style Search Prompt ──────────────────────────────────────────
//
// Matching the TS `find.ts` `runSearchPrompt` — real-time debounced
// search with API calls, up/down navigation, and enter to select.

/// A search result item for the fzf prompt.
#[derive(Clone)]
pub struct FzfItem {
    /// Display label.
    pub label: String,
    /// Description shown below label.
    pub description: String,
    /// Value returned on selection.
    pub value: String,
    /// Optional hint.
    #[allow(dead_code)]
    pub hint: Option<String>,
}

/// Result of the fzf search prompt.
pub enum FzfResult {
    /// User selected an item.
    Selected(String),
    /// User cancelled.
    Cancelled,
}

/// Run an interactive fzf-style search prompt.
///
/// `search_fn` is called with the current query and should return results.
///
/// # Errors
///
/// Returns an error if terminal I/O fails.
pub fn fzf_search<F>(message: &str, search_fn: F) -> io::Result<FzfResult>
where
    F: Fn(&str) -> Vec<FzfItem>,
{
    let mut stdout = io::stdout();
    let mut query = String::new();
    let mut cursor_pos: usize = 0;
    let mut results: Vec<FzfItem> = search_fn("");
    let max_visible: usize = 10;
    let mut last_height: u16 = 0;

    terminal::enable_raw_mode()?;

    let render_fzf = |stdout: &mut io::Stdout,
                      state: &str,
                      query: &str,
                      cursor_pos: usize,
                      results: &[FzfItem],
                      last_height: &mut u16|
     -> io::Result<()> {
        if *last_height > 0 {
            write!(stdout, "\x1b[{}A", *last_height)?;
            for _ in 0..*last_height {
                write!(stdout, "\x1b[2K\x1b[1B")?;
            }
            write!(stdout, "\x1b[{}A", *last_height)?;
        }

        let mut lines = Vec::new();

        let icon = match state {
            "submit" => S_STEP_SUBMIT,
            "cancel" => S_STEP_CANCEL,
            _ => S_STEP_ACTIVE,
        };
        lines.push(format!("{icon}  \x1b[1m{message}\x1b[0m"));

        if state == "active" {
            lines.push(format!(
                "{S_BAR}  \x1b[2mSearch:\x1b[0m {query}\x1b[7m \x1b[0m"
            ));
            lines.push(S_BAR.to_string());

            if results.is_empty() {
                if query.is_empty() {
                    lines.push(format!(
                        "{S_BAR}  \x1b[2mType to search for skills...\x1b[0m"
                    ));
                } else {
                    lines.push(format!("{S_BAR}  \x1b[2mNo results found\x1b[0m"));
                }
            } else {
                let visible_start = if results.len() <= max_visible {
                    0
                } else {
                    cursor_pos
                        .saturating_sub(max_visible / 2)
                        .min(results.len().saturating_sub(max_visible))
                };
                let visible_end = results.len().min(visible_start + max_visible);

                for (vi, item) in results
                    .iter()
                    .enumerate()
                    .take(visible_end)
                    .skip(visible_start)
                {
                    let is_cursor = vi == cursor_pos;
                    let prefix = if is_cursor { "\x1b[36m❯\x1b[0m" } else { " " };
                    let label = if is_cursor {
                        format!("\x1b[1m{}\x1b[0m", item.label)
                    } else {
                        item.label.clone()
                    };
                    let desc = if item.description.is_empty() {
                        String::new()
                    } else {
                        format!(" \x1b[2m- {}\x1b[0m", item.description)
                    };
                    lines.push(format!("{S_BAR} {prefix} {label}{desc}"));
                }

                let hidden_after = results.len().saturating_sub(visible_end);
                if hidden_after > 0 {
                    lines.push(format!("{S_BAR}  \x1b[2m↓ {hidden_after} more\x1b[0m"));
                }
            }

            lines.push(format!(
                "{S_BAR}  \x1b[2m{} result(s)\x1b[0m",
                results.len()
            ));
            lines.push("\x1b[2m└\x1b[0m".to_string());
        } else if state == "submit" {
            if let Some(item) = results.get(cursor_pos) {
                lines.push(format!("{S_BAR}  \x1b[2m{}\x1b[0m", item.label));
            }
        } else if state == "cancel" {
            lines.push(format!("{S_BAR}  \x1b[9m\x1b[2mCancelled\x1b[0m"));
        }

        for line in &lines {
            write!(stdout, "\x1b[2K{line}\r\n")?;
        }
        #[allow(clippy::cast_possible_truncation)]
        {
            *last_height = lines.len() as u16;
        }
        stdout.flush()?;
        Ok(())
    };

    render_fzf(
        &mut stdout,
        "active",
        &query,
        cursor_pos,
        &results,
        &mut last_height,
    )?;

    loop {
        if event::poll(std::time::Duration::from_millis(100))?
            && let Event::Key(KeyEvent {
                code, modifiers, ..
            }) = event::read()?
        {
            match code {
                KeyCode::Enter => {
                    if let Some(item) = results.get(cursor_pos) {
                        render_fzf(
                            &mut stdout,
                            "submit",
                            &query,
                            cursor_pos,
                            &results,
                            &mut last_height,
                        )?;
                        terminal::disable_raw_mode()?;
                        return Ok(FzfResult::Selected(item.value.clone()));
                    }
                }
                KeyCode::Esc => {
                    render_fzf(
                        &mut stdout,
                        "cancel",
                        &query,
                        cursor_pos,
                        &results,
                        &mut last_height,
                    )?;
                    terminal::disable_raw_mode()?;
                    return Ok(FzfResult::Cancelled);
                }
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                    render_fzf(
                        &mut stdout,
                        "cancel",
                        &query,
                        cursor_pos,
                        &results,
                        &mut last_height,
                    )?;
                    terminal::disable_raw_mode()?;
                    return Ok(FzfResult::Cancelled);
                }
                KeyCode::Up => {
                    cursor_pos = cursor_pos.saturating_sub(1);
                }
                KeyCode::Down => {
                    if !results.is_empty() {
                        cursor_pos = (cursor_pos + 1).min(results.len() - 1);
                    }
                }
                KeyCode::Backspace => {
                    query.pop();
                    cursor_pos = 0;
                    results = search_fn(&query);
                }
                KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                    query.push(c);
                    cursor_pos = 0;
                    results = search_fn(&query);
                }
                _ => {}
            }

            render_fzf(
                &mut stdout,
                "active",
                &query,
                cursor_pos,
                &results,
                &mut last_height,
            )?;
        }
    }
}
