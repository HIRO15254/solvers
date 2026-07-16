//! Short-hand text syntax for `multiway::config::SizeSpec`, used by the
//! Setup tab so a betting-size list edits as one comma-separated line
//! instead of a nested TOML table.
//!
//! Grammar (whitespace around commas/tokens is ignored):
//!   `2.5bb`     -> ToBb { value: 2.5 }
//!   `50%`       -> PotAfterCall { fraction: 0.5 }
//!   `x3`        -> PreviousBetMultiple { factor: 3.0 }
//!   `minraise`  -> MinRaise
//!   `stack:0.8` -> StackFraction { fraction: 0.8 }

use multiway::config::SizeSpec;

/// Parses a comma-separated size list. An empty (or all-whitespace) string
/// parses to an empty list rather than an error, matching an unset
/// `Vec<SizeSpec>` field.
pub fn parse_sizes(text: &str) -> Result<Vec<SizeSpec>, String> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    text.split(',')
        .map(|token| parse_one(token.trim()))
        .collect()
}

fn parse_one(token: &str) -> Result<SizeSpec, String> {
    if token.is_empty() {
        return Err("empty size entry".to_string());
    }
    let lower = token.to_ascii_lowercase();
    if lower == "minraise" || lower == "min-raise" {
        return Ok(SizeSpec::MinRaise);
    }
    if let Some(rest) = lower.strip_prefix("stack:") {
        return parse_number(rest, token).map(|fraction| SizeSpec::StackFraction { fraction });
    }
    if let Some(rest) = lower.strip_prefix('x') {
        return parse_number(rest, token).map(|factor| SizeSpec::PreviousBetMultiple { factor });
    }
    if let Some(rest) = lower.strip_suffix('%') {
        return parse_number(rest, token).map(|percent| SizeSpec::PotAfterCall {
            fraction: percent / 100.0,
        });
    }
    if let Some(rest) = lower.strip_suffix("bb") {
        return parse_number(rest, token).map(|value| SizeSpec::ToBb { value });
    }
    Err(format!(
        "unrecognized size '{token}' (expected e.g. 2.5bb, 50%, x3, minraise, stack:0.8)"
    ))
}

fn parse_number(text: &str, original: &str) -> Result<f64, String> {
    text.trim()
        .parse::<f64>()
        .map_err(|_| format!("invalid number in size '{original}'"))
}

/// Renders a size list back to the short-hand syntax, inverse of
/// [`parse_sizes`] (up to numeric formatting, e.g. `3.0` renders as `x3`).
pub fn render_sizes(sizes: &[SizeSpec]) -> String {
    sizes.iter().map(render_one).collect::<Vec<_>>().join(", ")
}

fn render_one(size: &SizeSpec) -> String {
    match *size {
        SizeSpec::ToBb { value } => format!("{}bb", fmt_num(value)),
        SizeSpec::PotAfterCall { fraction } => format!("{}%", fmt_num(fraction * 100.0)),
        SizeSpec::PreviousBetMultiple { factor } => format!("x{}", fmt_num(factor)),
        SizeSpec::MinRaise => "minraise".to_string(),
        SizeSpec::StackFraction { fraction } => format!("stack:{}", fmt_num(fraction)),
    }
}

/// Formats a size number without a redundant trailing `.0` (Rust's `f64`
/// `Display` already picks the shortest round-tripping representation).
fn fmt_num(value: f64) -> String {
    format!("{value}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_size_kind_round_trips_through_parse_and_render() {
        let cases: [(&str, SizeSpec); 5] = [
            ("2.5bb", SizeSpec::ToBb { value: 2.5 }),
            ("50%", SizeSpec::PotAfterCall { fraction: 0.5 }),
            ("x3", SizeSpec::PreviousBetMultiple { factor: 3.0 }),
            ("minraise", SizeSpec::MinRaise),
            ("stack:0.8", SizeSpec::StackFraction { fraction: 0.8 }),
        ];
        for (text, expected) in cases {
            let parsed = parse_sizes(text).unwrap();
            assert_eq!(parsed, vec![expected.clone()], "parsing '{text}'");
            let rendered = render_sizes(&parsed);
            assert_eq!(rendered, text, "rendering {expected:?}");
            // And the rendered text parses back to the same value.
            assert_eq!(parse_sizes(&rendered).unwrap(), vec![expected]);
        }
    }

    #[test]
    fn comma_separated_list_parses_in_order() {
        let parsed = parse_sizes(" 2.5bb , x3, minraise ,stack:0.5 ").unwrap();
        assert_eq!(
            parsed,
            vec![
                SizeSpec::ToBb { value: 2.5 },
                SizeSpec::PreviousBetMultiple { factor: 3.0 },
                SizeSpec::MinRaise,
                SizeSpec::StackFraction { fraction: 0.5 },
            ]
        );
        assert_eq!(render_sizes(&parsed), "2.5bb, x3, minraise, stack:0.5");
    }

    #[test]
    fn empty_text_parses_to_an_empty_list() {
        assert_eq!(parse_sizes("").unwrap(), Vec::new());
        assert_eq!(parse_sizes("   ").unwrap(), Vec::new());
        assert_eq!(render_sizes(&[]), "");
    }

    #[test]
    fn unrecognized_tokens_are_rejected_with_a_helpful_message() {
        let error = parse_sizes("2.5bb, potato").unwrap_err();
        assert!(error.contains("potato"));
    }
}
