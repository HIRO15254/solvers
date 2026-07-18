//! `mw-eval`: a dev/measurement tool for strategy purification/thresholding
//! (Ganzfried & Sandholm, AAMAS 2012) against a multiway checkpoint.
//!
//! This restores a `.mwckpt` checkpoint's frozen average profile (no
//! further sweeps are run) and, for each requested purification threshold,
//! optionally trains fresh per-seat best-response deviators against that
//! SAME purified profile, then reports the purified profile's held-out
//! per-seat deviation-gain lower bound. It writes no artifacts -- only
//! stdout.

use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use rayon::prelude::*;

use crate::session;

#[allow(clippy::too_many_arguments)]
pub fn run(
    config_path: &Path,
    checkpoint_path: &Path,
    samples: u64,
    seed: u64,
    purify: &str,
    br_traversals: u64,
) -> Result<()> {
    let raw_bytes =
        std::fs::read(config_path).with_context(|| format!("reading {}", config_path.display()))?;
    let raw = std::str::from_utf8(&raw_bytes).context("config file is not valid UTF-8")?;
    let thresholds = parse_thresholds(purify)?;

    let mw_session = session::build_multiway_session(raw, Some(checkpoint_path))
        .context("restoring multiway session from checkpoint")?;
    let num_players = mw_session.game_config.seats.len();

    // Sized like the stop-rule's own deviator-training pool (see
    // `multiway_solve.rs`): one deterministic worker pool for every
    // per-seat burst below, rather than relying on rayon's ambient global
    // pool (whose thread count the config doesn't control).
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(mw_session.threads)
        .build()
        .map_err(|error| anyhow!("building deviator training thread pool: {error}"))?;

    for threshold in thresholds {
        let started = Instant::now();
        let deviators = if br_traversals > 0 {
            let training_seed = seed ^ 0x7075_7269 ^ u64::from(threshold.to_bits());
            let trained = pool.install(|| {
                (0..num_players)
                    .into_par_iter()
                    .map(|seat| {
                        mw_session.solver.train_deviator_purified(
                            seat,
                            br_traversals,
                            training_seed,
                            threshold,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()
            });
            Some(trained.context("training purified best-response deviators")?)
        } else {
            None
        };

        let evaluation = mw_session
            .solver
            .evaluate_average_profile_purified(samples, seed, deviators.as_deref(), threshold)
            .context("evaluating purified multiway profile")?;
        let elapsed = started.elapsed().as_secs_f64();

        let bounds = evaluation
            .deviation_gain_lower_bound
            .as_ref()
            .expect("evaluate_average_profile_purified always returns deviation_gain_lower_bound");
        let dev_up: Vec<f64> = bounds.iter().map(|estimate| estimate.ci95[1]).collect();
        let max_dev_up = dev_up.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let max_dev_mean = bounds
            .iter()
            .map(|estimate| estimate.mean)
            .fold(f64::NEG_INFINITY, f64::max);
        let dev_up_str = dev_up
            .iter()
            .map(|value| format!("{value:.3}"))
            .collect::<Vec<_>>()
            .join(",");

        println!(
            "purify={threshold:.3} maxDevUp={max_dev_up:.3} maxDevMean={max_dev_mean:.3} \
             devUp=[{dev_up_str}] elapsed={elapsed:.1}s"
        );
    }

    Ok(())
}

/// Parses a comma-separated list of purification thresholds, e.g.
/// `"0.0,0.02,0.05"`. Blank tokens (a trailing comma, or an all-whitespace
/// list) are skipped.
fn parse_thresholds(raw: &str) -> Result<Vec<f32>> {
    let thresholds: Vec<f32> = raw
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| {
            token
                .parse::<f32>()
                .with_context(|| format!("invalid --purify threshold {token:?}"))
        })
        .collect::<Result<_>>()?;
    if thresholds.is_empty() {
        return Err(anyhow!("--purify must list at least one threshold"));
    }
    Ok(thresholds)
}

#[cfg(test)]
mod tests {
    use super::parse_thresholds;

    #[test]
    fn parses_comma_separated_thresholds() {
        assert_eq!(
            parse_thresholds("0.0,0.02,0.05").unwrap(),
            vec![0.0, 0.02, 0.05]
        );
    }

    #[test]
    fn trims_whitespace_and_skips_blank_tokens() {
        assert_eq!(
            parse_thresholds(" 0.0 , 0.1 ,,0.2").unwrap(),
            vec![0.0, 0.1, 0.2]
        );
    }

    #[test]
    fn rejects_unparseable_tokens() {
        assert!(parse_thresholds("0.0,not-a-number").is_err());
    }

    #[test]
    fn rejects_an_all_blank_list() {
        assert!(parse_thresholds(" , ,").is_err());
    }
}
