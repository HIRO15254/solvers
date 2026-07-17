//! Human-readable number formatting shared by the Solve tab's throughput
//! readout and the per-node EV table (sample counts).

/// Formats a non-negative count with a `k`/`M`/`B` suffix above 1,000,
/// e.g. `1_390_000.0 -> "1.39M"`, `783_000.0 -> "783k"`, `42.0 -> "42"`.
/// At most 2 decimal digits are kept, and trailing zeros (and a trailing
/// decimal point) are trimmed, to keep the info-dense Solve tab strip
/// readable.
pub fn human_count(value: f64) -> String {
    const UNITS: [(f64, &str); 3] = [(1e9, "B"), (1e6, "M"), (1e3, "k")];
    if !value.is_finite() {
        return "-".to_string();
    }
    let magnitude = value.abs();
    for &(scale, suffix) in &UNITS {
        if magnitude >= scale {
            return format!(
                "{}{suffix}",
                trim_trailing_zeros(format!("{:.2}", value / scale))
            );
        }
    }
    format!("{value:.0}")
}

/// Strips a trailing `.00`/`.10`-style zero run (and the decimal point
/// itself, if nothing is left after it) from a fixed-precision number
/// string.
fn trim_trailing_zeros(mut text: String) -> String {
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    text
}

/// [`human_count`] plus a `/s` suffix, for rate readouts (sweeps/s,
/// traversals/s, hand-updates/s).
pub fn human_rate(value: f64) -> String {
    format!("{}/s", human_count(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_counts_render_without_a_suffix() {
        assert_eq!(human_count(0.0), "0");
        assert_eq!(human_count(42.0), "42");
        assert_eq!(human_count(999.0), "999");
    }

    #[test]
    fn large_counts_get_a_unit_suffix() {
        assert_eq!(human_count(1_390_000.0), "1.39M");
        assert_eq!(human_count(783_000.0), "783k");
        assert_eq!(human_count(2_500_000_000.0), "2.5B");
    }

    #[test]
    fn non_finite_values_do_not_panic() {
        assert_eq!(human_count(f64::NAN), "-");
        assert_eq!(human_count(f64::INFINITY), "-");
    }

    #[test]
    fn rate_appends_per_second_suffix() {
        assert_eq!(human_rate(1_390_000.0), "1.39M/s");
    }
}
