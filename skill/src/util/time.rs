//! Lightweight time-formatting helpers.
//!
//! The lock files need second-precision ISO 8601 UTC timestamps but we want
//! to avoid pulling in a full date/time crate (`chrono`, `time`, `jiff`) for
//! this single use case. The implementation below is Howard Hinnant's
//! `civil_from_days` algorithm — ~30 lines of pure integer arithmetic, no
//! leap-second handling required for this application.

use std::time::{SystemTime, UNIX_EPOCH};

/// ISO 8601 UTC timestamp with second precision (`YYYY-MM-DDTHH:MM:SSZ`).
///
/// Matches the output of JavaScript `new Date().toISOString()` truncated to
/// seconds, which is what the reference TS CLI writes into lock files.
#[must_use]
pub(crate) fn iso8601_now() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = dur.as_secs();

    // Days / time decomposition (no leap-second handling needed here).
    let days = total_secs / 86_400;
    let time_of_day = total_secs % 86_400;
    let hh = time_of_day / 3_600;
    let mm = (time_of_day % 3_600) / 60;
    let ss = time_of_day % 60;

    // Civil date from day count (Howard Hinnant's `civil_from_days`).
    // All arithmetic stays in i64 to avoid wrapping casts.
    #[allow(
        clippy::cast_possible_wrap,
        reason = "u64 days fits in i64 for valid timestamps"
    )]
    let z = (days as i64) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let yr = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dd = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { yr + 1 } else { yr };

    format!("{year:04}-{month:02}-{dd:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso8601_now_format_matches_expected_shape() {
        let s = iso8601_now();
        let bytes = s.as_bytes();
        assert_eq!(bytes.len(), 20, "ISO 8601 Z-terminated length");
        assert_eq!(bytes.get(4), Some(&b'-'));
        assert_eq!(bytes.get(7), Some(&b'-'));
        assert_eq!(bytes.get(10), Some(&b'T'));
        assert_eq!(bytes.get(13), Some(&b':'));
        assert_eq!(bytes.get(16), Some(&b':'));
        assert_eq!(bytes.get(19), Some(&b'Z'));
    }

    #[test]
    fn iso8601_now_year_is_reasonable() {
        let s = iso8601_now();
        let year: u32 = s
            .get(..4)
            .and_then(|ys| ys.parse().ok())
            .expect("4-digit year prefix");
        assert!((2024..=2100).contains(&year), "year looks sane: {year}");
    }
}
