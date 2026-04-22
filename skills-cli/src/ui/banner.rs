//! ASCII logo and help banner rendering.

use super::style::{DIM, RESET, TEXT};

/// The six-line SKILLS logotype, drawn in a top-down grey gradient.
const LOGO_LINES: &[&str] = &[
    "███████╗██╗  ██╗██╗██╗     ██╗     ███████╗",
    "██╔════╝██║ ██╔╝██║██║     ██║     ██╔════╝",
    "███████╗█████╔╝ ██║██║     ██║     ███████╗",
    "╚════██║██╔═██╗ ██║██║     ██║     ╚════██║",
    "███████║██║  ██╗██║███████╗███████╗███████║",
    "╚══════╝╚═╝  ╚═╝╚═╝╚══════╝╚══════╝╚══════╝",
];

/// Grey palette used for the per-line gradient (top → bottom).
const GRAYS: &[&str] = &[
    "\x1b[38;5;250m",
    "\x1b[38;5;248m",
    "\x1b[38;5;245m",
    "\x1b[38;5;243m",
    "\x1b[38;5;240m",
    "\x1b[38;5;238m",
];

/// Print the SKILLS ASCII logo with a gradient effect.
pub(crate) fn show_logo() {
    println!();
    for (i, line) in LOGO_LINES.iter().enumerate() {
        let first = GRAYS.first().unwrap_or(&"");
        let gray = GRAYS.get(i).unwrap_or(first);
        println!("{gray}{line}{RESET}");
    }
}

/// Print the full banner (logo + tagline + command quick-reference).
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
