//! Shared UI helpers: logo, banner, colors, path shortening.

use std::path::Path;

use console::Style;

/// ANSI logo lines for the skills banner.
const LOGO_LINES: &[&str] = &[
    "███████╗██╗  ██╗██╗██╗     ██╗     ███████╗",
    "██╔════╝██║ ██╔╝██║██║     ██║     ██╔════╝",
    "███████╗█████╔╝ ██║██║     ██║     ███████╗",
    "╚════██║██╔═██╗ ██║██║     ██║     ╚════██║",
    "███████║██║  ██╗██║███████╗███████╗███████║",
    "╚══════╝╚═╝  ╚═╝╚═╝╚══════╝╚══════╝╚══════╝",
];

/// 256-color grays visible on both light and dark backgrounds.
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

/// Print the ASCII logo.
pub fn show_logo() {
    println!();
    for (i, line) in LOGO_LINES.iter().enumerate() {
        let color = GRAYS.get(i).unwrap_or(&GRAYS[0]);
        println!("{color}{line}{RESET}");
    }
}

/// Print the full banner with usage hints.
pub fn show_banner(version: &str) {
    show_logo();
    println!();
    println!("{DIM}The open agent skills ecosystem  v{version}{RESET}");
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
        "  {DIM}${RESET} {TEXT}skills init {DIM}[name]{RESET}          {DIM}Create a new skill{RESET}"
    );
    println!();
    println!("{DIM}try:{RESET} skills add vercel-labs/agent-skills");
    println!();
    println!("Discover more skills at {TEXT}https://skills.sh/{RESET}");
    println!();
}

/// Shorten a path for display: replace home with `~`, cwd with `.`.
pub fn shorten_path(full_path: &Path, cwd: &Path) -> String {
    let full = full_path.to_string_lossy();
    let home = dirs::home_dir().unwrap_or_default();
    let home_str = home.to_string_lossy();
    let cwd_str = cwd.to_string_lossy();

    if full.starts_with(cwd_str.as_ref()) {
        return format!(".{}", &full[cwd_str.len()..]);
    }
    if full.starts_with(home_str.as_ref()) {
        return format!("~{}", &full[home_str.len()..]);
    }
    full.into_owned()
}

/// Format a list of items, truncating if too many.
pub fn format_list(items: &[String], max_show: usize) -> String {
    if items.len() <= max_show {
        items.join(", ")
    } else {
        let shown: Vec<_> = items.iter().take(max_show).map(String::as_str).collect();
        let remaining = items.len() - max_show;
        format!("{} +{remaining} more", shown.join(", "))
    }
}

/// Common styles.
#[must_use]
pub const fn style_green() -> Style {
    Style::new().green()
}

/// Red style for errors.
#[must_use]
pub const fn style_red() -> Style {
    Style::new().red()
}

/// Dim style for secondary text.
#[must_use]
pub const fn style_dim() -> Style {
    Style::new().dim()
}

/// Bold style for emphasis.
#[must_use]
pub const fn style_bold() -> Style {
    Style::new().bold()
}

/// Print a success message with a green checkmark.
pub fn print_success(msg: &str) {
    let green = style_green();
    println!("{} {msg}", green.apply_to("✓"));
}

/// Print an error message with a red cross.
pub fn print_error(msg: &str) {
    let red = style_red();
    eprintln!("{} {msg}", red.apply_to("✗"));
}

/// Print a note box (like `@clack/prompts` note).
pub fn print_note(content: &str, title: &str) {
    let dim = style_dim();
    let bold = style_bold();
    println!();
    println!("  {} {}", dim.apply_to("┌"), bold.apply_to(title));
    for line in content.lines() {
        println!("  {} {line}", dim.apply_to("│"));
    }
    println!("  {}", dim.apply_to("└"));
}
