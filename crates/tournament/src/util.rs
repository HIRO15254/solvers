//! Dependency-free line-parsing helpers shared by the hand-history and
//! summary parsers.

use cards::Card;

use crate::model::DateTime;

/// Parses a `u64` after stripping comma thousands separators
/// (`"3,409,235"` -> `3409235`).
pub(crate) fn parse_amount(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit() || b == b',') {
        return None;
    }
    let cleaned: String = s.chars().filter(|&c| c != ',').collect();
    if cleaned.is_empty() {
        return None;
    }
    cleaned.parse::<u64>().ok()
}

/// Strips an optional trailing `" and is all-in"` suffix, returning the
/// remainder and whether the suffix was present.
pub(crate) fn strip_all_in(s: &str) -> (&str, bool) {
    match s.strip_suffix(" and is all-in") {
        Some(rest) => (rest, true),
        None => (s, false),
    }
}

/// Parses a bracketed, space-separated card list like `"[4d Th]"`. Fails
/// if the brackets are missing or any token isn't a valid card.
pub(crate) fn parse_bracket_group(s: &str) -> Result<Vec<Card>, String> {
    let Some(inner) = s.strip_prefix('[').and_then(|s| s.strip_suffix(']')) else {
        return Err(format!("expected a bracketed card list, got {s:?}"));
    };
    inner
        .split_whitespace()
        .map(|tok| {
            tok.parse::<Card>()
                .map_err(|_| format!("invalid card {tok:?}"))
        })
        .collect()
}

/// Parses a fixed-shape `YYYY/MM/DD HH:MM:SS` timestamp.
pub(crate) fn parse_datetime(s: &str) -> Option<DateTime> {
    // "YYYY/MM/DD HH:MM:SS" is exactly 19 ASCII bytes.
    if s.len() != 19 || !s.is_ascii() {
        return None;
    }
    let bytes = s.as_bytes();
    if bytes[4] != b'/'
        || bytes[7] != b'/'
        || bytes[10] != b' '
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let year = s[0..4].parse::<u16>().ok()?;
    let month = s[5..7].parse::<u8>().ok()?;
    let day = s[8..10].parse::<u8>().ok()?;
    let hour = s[11..13].parse::<u8>().ok()?;
    let minute = s[14..16].parse::<u8>().ok()?;
    let second = s[17..19].parse::<u8>().ok()?;
    Some(DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
    })
}

/// Parses a leading `"(<amount>)"` group, returning the amount and the
/// remainder of the string after the closing paren.
pub(crate) fn parse_leading_paren_amount(s: &str) -> Option<(u64, &str)> {
    let rest = s.strip_prefix('(')?;
    let close = rest.find(')')?;
    let amount = parse_amount(&rest[..close])?;
    Some((amount, &rest[close + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comma_amounts() {
        assert_eq!(parse_amount("3,409,235"), Some(3_409_235));
        assert_eq!(parse_amount("0"), Some(0));
        assert_eq!(parse_amount(""), None);
        assert_eq!(parse_amount("12a"), None);
    }

    #[test]
    fn strips_all_in_suffix() {
        assert_eq!(strip_all_in("500 and is all-in"), ("500", true));
        assert_eq!(strip_all_in("500"), ("500", false));
    }

    #[test]
    fn parses_bracket_groups() {
        assert_eq!(
            parse_bracket_group("[4d Th]").unwrap(),
            vec!["4d".parse().unwrap(), "Th".parse().unwrap()]
        );
        assert!(parse_bracket_group("4d Th").is_err());
        assert!(parse_bracket_group("[4d Xx]").is_err());
    }

    #[test]
    fn parses_datetimes() {
        let dt = parse_datetime("2026/06/13 16:48:10").unwrap();
        assert_eq!(dt.year, 2026);
        assert_eq!(dt.month, 6);
        assert_eq!(dt.day, 13);
        assert_eq!(dt.hour, 16);
        assert_eq!(dt.minute, 48);
        assert_eq!(dt.second, 10);
        assert_eq!(dt.to_string(), "2026/06/13 16:48:10");
        assert!(parse_datetime("2026/06/13").is_none());
    }

    #[test]
    fn parses_leading_paren_amounts() {
        assert_eq!(
            parse_leading_paren_amount("(1,368) from pot"),
            Some((1_368, " from pot"))
        );
        assert_eq!(parse_leading_paren_amount("no paren"), None);
    }
}
