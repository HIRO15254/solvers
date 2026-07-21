use std::path::Path;

use anyhow::{Context, Result};
use clap::ValueEnum;

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum ConfigTemplate {
    #[default]
    Minimal,
    Full,
}

const MINIMAL: &str = r#"schema = "solvers.multiway-preflop/v1"

[game]
seat_count = 6
button = 0

[game.defaults]
stack_bb = 100.0
range = "random"
"#;

const FULL: &str = r#"schema = "solvers.multiway-preflop/v1"

[game]
seat_count = 6
button = 0
standard_blinds = true
preflop_first_to_act = "utg"
common_ante_bb = 0.0

[game.defaults]
stack_bb = 100.0
range = "random"

[game.tree]
kind = "standard"

[game.abstraction]
kind = "multiway-rollout"
rollouts_per_state = 512
seed = 0

[game.abstraction.buckets]
flop = 64
turn = 64
river = 64

[game.information]
recall = "current-street"

[economics]
kind = "cash"

[solver]
kind = "range-vector"
seed = 0
opponent_exploration = 0.0
batch_sweeps = 1

[solver.discount]
kind = "periodic"
every_sweeps = 10000
until_sweeps = 10000000

[solver.pruning]
kind = "regret-based"

[run]
max_sweeps = 5000000

[run.resources]
threads = "auto"
memory = "auto"

[run.stop]
target = "default"
check_every_sweeps = 10000
confirmations = 3
evaluation_samples = 4096
deviator_traversals = 20000

[run.checkpoint]
interval = "15m"

[output]
probability_encoding = "u16"
"#;

pub fn run(template: ConfigTemplate, out: Option<&Path>) -> Result<()> {
    let rendered = match template {
        ConfigTemplate::Minimal => MINIMAL,
        ConfigTemplate::Full => FULL,
    };
    crate::multiway_v1::parse_and_lower(rendered)
        .context("validating built-in Multiway Preflop v1 template")?;
    if let Some(path) = out {
        std::fs::write(path, rendered)
            .with_context(|| format!("writing config template {}", path.display()))?;
    } else {
        print!("{rendered}");
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_and_full_templates_are_valid() {
        crate::multiway_v1::parse_and_lower(MINIMAL).unwrap();
        crate::multiway_v1::parse_and_lower(FULL).unwrap();
    }

    #[test]
    fn toml_reference_covers_the_public_surface() {
        let reference = include_str!("../../../docs/multiway-preflop-toml-reference.jp.md");
        for token in [
            "schema",
            "seat_count",
            "button",
            "standard_blinds",
            "preflop_first_to_act",
            "common_ante_bb",
            "stack_bb",
            "range",
            "blind_bb",
            "ante_bb",
            "priority",
            "street",
            "when",
            "effect",
            "action",
            "sizes",
            "source",
            "params",
            "rollouts_per_state",
            "opponent_buckets",
            "recall",
            "payouts",
            "outside_field_bb",
            "samples",
            "rate",
            "cap_bb",
            "allocation",
            "rounding_unit_bb",
            "rounding",
            "opponent_exploration",
            "batch_sweeps",
            "every_sweeps",
            "until_sweeps",
            "max_sweeps",
            "max_time",
            "threads",
            "memory",
            "target",
            "check_every_sweeps",
            "confirmations",
            "evaluation_samples",
            "deviator_traversals",
            "interval",
            "probability_encoding",
            "multiway-rollout",
            "ehs2-percentile",
            "current-street",
            "bucket-history",
            "tournament-icm",
            "range-vector",
            "single-hand",
            "regret-based",
            "u16",
            "f32",
        ] {
            assert!(
                reference.contains(&format!("`{token}`")) || reference.contains(token),
                "TOML reference is missing {token}"
            );
        }
    }
}
