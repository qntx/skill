//! Terminal control sequence sanitization (CWE-150).
//!
//! Strips ANSI escape sequences and control characters from untrusted strings
//! (skill `name` / `description`, remote index entries, repository metadata)
//! before they are rendered to the terminal. Without this mitigation a
//! malicious skill author can forge CLI output, clear the screen, or trick
//! users into approving installations they did not intend.
//!
//! Coverage follows the TypeScript reference at
//! `3rdparty/skills/src/sanitize.ts`:
//!
//! - CSI (`ESC [ ... <final-byte>`)
//! - OSC (`ESC ] ... (BEL | ESC \)`)
//! - DCS / APC / PM / SOS (`ESC {P,X,_,^} ... ESC \`)
//! - Bare `ESC <intermediate>? <final>` two-byte forms
//! - C0 controls (`0x00..0x1f`, except `\t`, `\n`, `\r`)
//! - DEL (`0x7f`) and C1 controls (`0x80..0x9f`)

/// ESC byte (`0x1b`) — the start of every ANSI escape sequence.
const ESC: u8 = 0x1b;
/// BEL byte (`0x07`) — one of the two legal OSC terminators.
const BEL: u8 = 0x07;
/// Literal `\` following ESC; the other half of the ST terminator.
const ST_SUFFIX: u8 = b'\\';

/// Strip terminal control sequences from `input`.
///
/// Preserves printable characters, tabs, newlines, and carriage returns.
/// The returned `String` never contains any of the escape forms enumerated
/// in the module docs.
///
/// # Examples
///
/// ```
/// use skill::sanitize::sanitize_metadata;
///
/// // Printable Unicode is preserved:
/// assert_eq!(sanitize_metadata("你好 🌟 café"), "你好 🌟 café");
///
/// // ANSI escape sequences are stripped:
/// assert_eq!(sanitize_metadata("\x1b[2Jpwned"), "pwned");
///
/// // Legacy 8-bit C1 controls are stripped too:
/// assert_eq!(sanitize_metadata("a\u{0080}b"), "ab");
/// ```
#[must_use]
pub fn sanitize_metadata(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i: usize = 0;

    while let Some(&b) = bytes.get(i) {
        if b == ESC {
            i = skip_escape_sequence(bytes, i);
            continue;
        }

        // Decode one UTF-8 code point starting at `i`. On malformed input,
        // advance a single byte so the loop always terminates.
        let tail = input.get(i..).unwrap_or_default();
        let Some(ch) = tail.chars().next() else {
            break;
        };
        let width = ch.len_utf8();

        if !should_strip_char(ch) {
            out.push(ch);
        }
        i = i.saturating_add(width);
    }

    out
}

/// Skip the escape sequence that begins at `bytes[start]`.
///
/// Returns the index of the first byte after the sequence. Always makes at
/// least one byte of forward progress so the outer loop terminates.
fn skip_escape_sequence(bytes: &[u8], start: usize) -> usize {
    debug_assert_eq!(
        bytes.get(start),
        Some(&ESC),
        "skip_escape_sequence must be called at an ESC byte"
    );

    let Some(&second) = bytes.get(start.saturating_add(1)) else {
        // Bare trailing ESC at EOF: drop it.
        return bytes.len();
    };

    match second {
        b'[' => skip_csi(bytes, start.saturating_add(2)),
        b']' => skip_osc(bytes, start.saturating_add(2)),
        b'P' | b'X' | b'_' | b'^' => skip_string_terminator(bytes, start.saturating_add(2)),
        _ => {
            // Two-byte ESC <final> form, optionally with an intermediate byte
            // in `0x20..=0x2f`.
            let mut i = start.saturating_add(1);
            while let Some(&b) = bytes.get(i) {
                if (0x20..=0x2f).contains(&b) {
                    i = i.saturating_add(1);
                } else {
                    break;
                }
            }
            i.saturating_add(1).min(bytes.len())
        }
    }
}

/// Skip a CSI sequence: parameter bytes in `0x30..=0x3f`, intermediate bytes
/// in `0x20..=0x2f`, terminated by a final byte in `0x40..=0x7e`.
fn skip_csi(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    while let Some(&b) = bytes.get(i) {
        if (0x40..=0x7e).contains(&b) {
            return i.saturating_add(1);
        }
        i = i.saturating_add(1);
    }
    bytes.len()
}

/// Skip an OSC sequence terminated by BEL (`0x07`) or ST (`ESC \`).
fn skip_osc(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    while let Some(&b) = bytes.get(i) {
        if b == BEL {
            return i.saturating_add(1);
        }
        if b == ESC && bytes.get(i.saturating_add(1)) == Some(&ST_SUFFIX) {
            return i.saturating_add(2);
        }
        i = i.saturating_add(1);
    }
    bytes.len()
}

/// Skip a DCS / APC / PM / SOS sequence terminated by ST (`ESC \`).
fn skip_string_terminator(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    while let Some(&b) = bytes.get(i) {
        if b == ESC && bytes.get(i.saturating_add(1)) == Some(&ST_SUFFIX) {
            return i.saturating_add(2);
        }
        i = i.saturating_add(1);
    }
    bytes.len()
}

/// Whether a Unicode scalar value is a control character that must be
/// stripped from terminal-bound output.
///
/// Covers the full C0 set except whitespace that terminals handle safely
/// (`\t`, `\n`, `\r`), plus DEL and the C1 set (U+0080..U+009F) which are
/// used by legacy 8-bit terminals to trigger escape sequences.
const fn should_strip_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x00..=0x08 | 0x0b | 0x0c | 0x0e..=0x1f | 0x7f..=0x9f
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_plain_ascii() {
        assert_eq!(sanitize_metadata("hello world"), "hello world");
    }

    #[test]
    fn preserves_unicode() {
        assert_eq!(sanitize_metadata("你好 🌟 café"), "你好 🌟 café");
    }

    #[test]
    fn preserves_whitespace() {
        assert_eq!(sanitize_metadata("a\tb\nc\rd"), "a\tb\nc\rd");
    }

    #[test]
    fn strips_csi_clear_screen() {
        let malicious = "before\u{001b}[2J\u{001b}[Hafter";
        assert_eq!(sanitize_metadata(malicious), "beforeafter");
    }

    #[test]
    fn strips_csi_sgr_colors() {
        let malicious = "\u{001b}[31;1mred bold\u{001b}[0m";
        assert_eq!(sanitize_metadata(malicious), "red bold");
    }

    #[test]
    fn strips_osc_with_bel() {
        let malicious = "name\u{001b}]0;pwned\u{0007}end";
        assert_eq!(sanitize_metadata(malicious), "nameend");
    }

    #[test]
    fn strips_osc_with_st() {
        let malicious = "name\u{001b}]0;pwned\u{001b}\\end";
        assert_eq!(sanitize_metadata(malicious), "nameend");
    }

    #[test]
    fn strips_dcs_sequence() {
        let malicious = "x\u{001b}P1;2qpayload\u{001b}\\y";
        assert_eq!(sanitize_metadata(malicious), "xy");
    }

    #[test]
    fn strips_two_byte_escape() {
        let malicious = "pre\u{001b}cpost";
        assert_eq!(sanitize_metadata(malicious), "prepost");
    }

    #[test]
    fn strips_bare_trailing_escape() {
        assert_eq!(sanitize_metadata("text\u{001b}"), "text");
    }

    #[test]
    fn strips_c0_controls() {
        let malicious = "a\u{0000}b\u{0008}c\u{001f}d";
        assert_eq!(sanitize_metadata(malicious), "abcd");
    }

    #[test]
    fn strips_del_and_c1() {
        let malicious = "a\u{007f}b\u{0080}c\u{009f}d";
        assert_eq!(sanitize_metadata(malicious), "abcd");
    }

    #[test]
    fn handles_empty_input() {
        assert_eq!(sanitize_metadata(""), "");
    }

    #[test]
    fn handles_only_controls() {
        assert_eq!(sanitize_metadata("\u{001b}[2J\u{001b}[H"), "");
    }

    #[test]
    fn handles_unterminated_csi() {
        assert_eq!(sanitize_metadata("\u{001b}[31;1m"), "");
    }
}
