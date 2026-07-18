//! Single source of human-readable number formatting for the whole GUI:
//! percentages, EV/bb values, sample counts, throughput rates, elapsed wall
//! time, and memory sizes. Every view that used to inline its own `format!`
//! for one of these (Solve tab's throughput strip, EV tables, matrix cell/
//! tooltip probabilities, seat diagnostics...) should route through here
//! instead, so a formatting convention only needs to change in one place.

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

/// Formats a `[0, 1]`-ish fraction as a one-decimal percentage, e.g.
/// `0.625 -> "62.5%"`. Used for every strategy/aggregate-frequency
/// probability across the matrix cells, aggregate row, and EV tables.
pub fn percent(fraction: f64) -> String {
    if !fraction.is_finite() {
        return "-".to_string();
    }
    format!("{:.1}%", fraction * 100.0)
}

/// Formats an EV/utility value in "bb" units with a sign always shown (even
/// for exactly zero), e.g. `0.35 -> "+0.35 bb"`, `-1.2 -> "-1.20 bb"`. Used
/// for directional EV readouts (per-action estimates, seat EV, deviation
/// gain) where the sign itself is meaningful.
///
/// Note: the multiway solver also supports a tournament-ICM utility mode,
/// whose values are ICM equity shares rather than chip `bb` -- this helper
/// (and the GUI generally) does not yet distinguish the two, so the `bb`
/// suffix is technically a misnomer under ICM. That unit-awareness is out of
/// scope for this formatting pass; see the polish-pass report.
pub fn ev_bb(value: f64) -> String {
    if !value.is_finite() {
        return "-".to_string();
    }
    format!("{value:+.2} bb")
}

/// Formats a non-negative "bb" magnitude with two decimals and no forced
/// sign, e.g. a confidence-interval half-width: `0.35 -> "0.35 bb"`.
pub fn bb(value: f64) -> String {
    if !value.is_finite() {
        return "-".to_string();
    }
    format!("{value:.2} bb")
}

/// Formats elapsed wall-clock seconds at a resolution matched to its own
/// magnitude: sub-minute keeps a decimal (`"12.4s"`), sub-hour drops to
/// whole seconds (`"3m 05s"`), and an hour or more adds an hours field
/// (`"1h 02m 15s"`) -- all zero-padded past the leading unit so the strip
/// doesn't visually reflow as a run crosses a minute/hour boundary.
pub fn elapsed(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "-".to_string();
    }
    if seconds < 60.0 {
        return format!("{seconds:.1}s");
    }
    let total_secs = seconds.round() as u64;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours}h {minutes:02}m {secs:02}s")
    } else {
        format!("{minutes}m {secs:02}s")
    }
}

/// Formats a byte count as MiB (one decimal), switching to GiB above 1024
/// MiB -- the Solve tab's memory readout and any other memory-size display.
pub fn memory_mib(bytes: u64) -> String {
    let mib = bytes as f64 / (1024.0 * 1024.0);
    if mib >= 1024.0 {
        format!("{:.1} GiB", mib / 1024.0)
    } else {
        format!("{mib:.1} MiB")
    }
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

    #[test]
    fn percent_renders_one_decimal() {
        assert_eq!(percent(0.625), "62.5%");
        assert_eq!(percent(0.0), "0.0%");
        assert_eq!(percent(1.0), "100.0%");
    }

    #[test]
    fn percent_non_finite_does_not_panic() {
        assert_eq!(percent(f64::NAN), "-");
        assert_eq!(percent(f64::INFINITY), "-");
    }

    #[test]
    fn ev_bb_always_shows_a_sign() {
        assert_eq!(ev_bb(0.35), "+0.35 bb");
        assert_eq!(ev_bb(-1.2), "-1.20 bb");
        assert_eq!(ev_bb(0.0), "+0.00 bb");
    }

    #[test]
    fn bb_never_shows_a_sign() {
        assert_eq!(bb(0.35), "0.35 bb");
        assert_eq!(bb(-1.2), "-1.20 bb");
    }

    #[test]
    fn elapsed_scales_precision_with_magnitude() {
        assert_eq!(elapsed(12.4), "12.4s");
        assert_eq!(elapsed(59.94), "59.9s");
        assert_eq!(elapsed(185.0), "3m 05s");
        assert_eq!(elapsed(3735.0), "1h 02m 15s");
    }

    #[test]
    fn elapsed_rounds_seconds_at_the_minute_hour_scale() {
        // 125.6s rounds to 126s = 2m 06s, not 2m 05s.
        assert_eq!(elapsed(125.6), "2m 06s");
    }

    #[test]
    fn elapsed_non_finite_or_negative_does_not_panic() {
        assert_eq!(elapsed(f64::NAN), "-");
        assert_eq!(elapsed(-1.0), "-");
    }

    #[test]
    fn memory_switches_units_at_1024_mib() {
        assert_eq!(memory_mib(0), "0.0 MiB");
        assert_eq!(memory_mib(512 * 1024 * 1024), "512.0 MiB");
        assert_eq!(memory_mib(1536 * 1024 * 1024), "1.5 GiB");
    }
}
