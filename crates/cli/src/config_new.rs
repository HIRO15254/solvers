use std::path::Path;

use anyhow::{Context, Result};
use clap::ValueEnum;

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum ConfigTemplate {
    #[default]
    Minimal,
    Full,
}

/// Which family's template to print. Every family is its own contract, so
/// there is no single "the" template.
#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum ConfigSchema {
    #[default]
    MultiwayPreflop,
    Postflop,
}

const MINIMAL: &str = r#"schema = "solvers.multiway-preflop/v1"

[game]
seat_count = 6
button = 0

[game.defaults]
stack_bb = 100.0
range = "random"

[game.abstraction]
kind = "ehs2-percentile"
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
kind = "ehs2-percentile"

[game.abstraction.buckets]
flop = 128
turn = 128
river = 128

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

const POSTFLOP_MINIMAL: &str = r#"schema = "solvers.postflop/v1"

[game]
board = "Ks 7h 2d"
oop_range = "22+,A2s+,K9s+,QTs+,JTs,ATo+,KQo"
ip_range = "random"
pot = 50
effective_stack = 200

[game.tree]
kind = "script"
script = '''
flop {
  replace bet [50]
  replace raise [50]
}
'''

[run]
target_nash_conv = 0.05
"#;

const POSTFLOP_FULL: &str = r#"schema = "solvers.postflop/v1"

[game]
board = "Ks 7h 2d"
oop_range = "22+,A2s+,K9s+,QTs+,JTs,ATo+,KQo"
ip_range = "random"
pot = 50
effective_stack = 200
min_bet = 1
iso_merging = true
preflop_aggressor = "ip"

[game.tree]
kind = "script"
include_allin = true
allin_threshold = 0.8
script = '''
# c-bet size (pot %)
param cb = 33
# barrel size (pot %)
param barrel = 66

flop {
  when donk { remove bet }
  when cbet {
    replace bet [cb, 75]
  }
  when aggressions == 1 { replace raise [3x] }
}

turn {
  # `raise` only exists once someone has bet, so this sits outside the
  # `unopened` block -- nested, its condition would be
  # `aggressions == 0 && aggressions == 1` and it would never fire.
  when unopened { replace bet [barrel] }
  when aggressions == 1 { replace raise [2.5x] }
}

river when unopened {
  replace bet [75, a]
}
'''

[game.tree.max_aggressive_actions]
flop = 3
turn = 2
river = 2

[game.tree.params]
cb = 40

[rake]
kind = "generic"
rate = 0.05
cap = 6.0
when = "flop_dealt"
allocation = "main-first"
rounding = "down"
rounding_unit = 1.0

[utility]
kind = "chip-ev"

[algorithm]
schedule = "dcfr"

[run]
iterations = 1000000
max_time = "30m"
check_every = 25
storage = "f32"
target_nash_conv = 0.05
"#;

pub fn run(schema: ConfigSchema, template: ConfigTemplate, out: Option<&Path>) -> Result<()> {
    let rendered = match (schema, template) {
        (ConfigSchema::MultiwayPreflop, ConfigTemplate::Minimal) => MINIMAL,
        (ConfigSchema::MultiwayPreflop, ConfigTemplate::Full) => FULL,
        (ConfigSchema::Postflop, ConfigTemplate::Minimal) => POSTFLOP_MINIMAL,
        (ConfigSchema::Postflop, ConfigTemplate::Full) => POSTFLOP_FULL,
    };
    match schema {
        ConfigSchema::MultiwayPreflop => {
            crate::multiway_v1::validate_production_contract(rendered)
                .context("validating built-in production Multiway Preflop v1 template")?;
        }
        ConfigSchema::Postflop => {
            crate::solver_config_v1::parse_and_lower(rendered)
                .context("validating built-in postflop v1 template")?;
        }
    }
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
        for template in [MINIMAL, FULL] {
            crate::multiway_v1::parse_and_lower(template).unwrap();
            crate::multiway_v1::validate_production_contract(template).unwrap();
        }
    }

    /// The postflop templates must survive the same round trip the contract
    /// promises: parse, normalize, and reparse to the same bytes.
    #[test]
    fn postflop_templates_are_valid_and_normalize_idempotently() {
        for template in [POSTFLOP_MINIMAL, POSTFLOP_FULL] {
            crate::solver_config_v1::parse_and_lower(template).unwrap();
            let effective = crate::solver_config_v1::normalized_toml(template).unwrap();
            assert_eq!(
                crate::solver_config_v1::normalized_toml(&effective).unwrap(),
                effective
            );
        }
    }

    /// The CLI reference claims to cover every command and flag of both
    /// binaries. A new command or flag that skips the document fails here.
    #[test]
    fn the_cli_reference_covers_every_command() {
        let reference = include_str!("../../../docs/cli-reference.jp.md");
        let expected = [
            // solvers subcommands
            "config new",
            "validate",
            "solve",
            "resume",
            "status",
            "watch",
            "runs ls",
            "inspect",
            "report",
            "export",
            "compare",
            "evaluate",
            // global
            "--cache-dir",
            "SOLVERS_CACHE_DIR",
            "NO_COLOR",
            "SOLVERSD_TOKEN",
            // per-command flags
            "--schema",
            "--template",
            "--out",
            "--format",
            "--show-effective",
            "--write-effective",
            "--resources",
            "--history",
            "--sol-streets",
            "--threads",
            "--memory",
            "--max-time",
            "--max-sweeps",
            "--stop-target",
            "--evaluation-samples",
            "--evaluation-cadence",
            "--checkpoint-interval",
            "--from",
            "--poll-secs",
            "--iterations",
            "--target-nash-conv",
            "--sol",
            "--river-iterations",
            "--river-target",
            "--node",
            "--view",
            "--actor",
            "--samples",
            "--seed",
            "--br-traversals",
            "--boards",
            "--boards-file",
            "--output",
            "--cross-game",
            "--node",
            // inspect REPL
            "show",
            "`go <action\\|index>`",
            "up",
            "root",
            "grid",
            "range",
            "eq",
            "combos",
            "ev",
            "help",
            "quit",
            // exit codes and signals
            "`0`",
            "`1`",
            "`2`",
            "`3`",
            "`75`",
            "`130`",
            "SIGINT",
            // daemon
            "solversd",
            "--runs",
            "--bind",
            "--tls-cert",
            "--tls-key",
            "--solver",
            "--max-concurrent",
            "--token",
            "/v1/validate",
            "/v1/runs",
            "/v1/runs/{id}/events",
            "/v1/runs/{id}/artifacts",
            "/v1/runs/{id}/solution/{view}",
            "mean_strategy_l1",
            "max_ev_delta",
            "/v1/runs/{id}/cancel",
            "/v1/runs/{id}/resume",
        ];
        for token in expected {
            assert!(
                reference.contains(token),
                "the CLI reference is missing {token}"
            );
        }
    }

    /// The postflop reference claims to be complete, so hold it to that:
    /// every key, literal, CLI flag, and output field a user can reach has
    /// to appear in it. A new knob that skips the document fails here.
    #[test]
    fn the_postflop_reference_covers_the_whole_surface() {
        let reference = include_str!("../../../docs/solver-config-v1.jp.md");
        let expected = [
            // [game]
            "board",
            "oop_range",
            "ip_range",
            "pot",
            "effective_stack",
            "min_bet",
            "iso_merging",
            "preflop_aggressor",
            // [game.tree]
            "kind",
            "source",
            "script",
            "max_aggressive_actions",
            "include_allin",
            "allin_threshold",
            "params",
            // tree script grammar
            "param",
            "define",
            "checkdown",
            "aggressions",
            "unopened",
            "in_position",
            "cbet",
            "donk",
            "spr",
            "facing_pct",
            // board predicates
            "paired",
            "monotone",
            "flush_possible",
            "straight_possible",
            // size literals
            "20c",
            "3x",
            "`a`",
            "`e`",
            "3e",
            "min",
            "80%effective",
            "60%stack",
            // [rake] / [utility]
            "percent-cap",
            "gg-preflop",
            "generic",
            "rate",
            "cap",
            "when",
            "allocation",
            "rounding",
            "rounding_unit",
            "chip-ev",
            "icm",
            "tournament-icm",
            "payouts",
            "outside_field",
            "samples",
            "seed",
            // [run] / [algorithm]
            "iterations",
            "max_time",
            "check_every",
            "storage",
            "target_nash_conv",
            "threads",
            "par_chance_depth",
            "par_min_children",
            "schedule",
            // history grammar
            "r{到達額}",
            // outputs
            "manifest.json",
            "progress.jsonl",
            "events.jsonl",
            "run.json",
            "checkpoint.ckpt",
            "solution.sol",
            // the export views that replaced `strategy.json`
            "summary",
            "tree",
            "actions",
            "strategy",
            "range",
            "streets_stored",
            "stored_nodes",
            "ev_oop",
            "ev_ip",
            "exploitability",
            "oop_equity",
            "nash_conv",
            // the CLI facts that belong to the contract itself; every flag
            // is enumerated by the CLI reference and its own test
            "--show-effective",
            "--write-effective",
            "--threads",
            "--resources",
            // error codes
            "SLV001",
            "SLV002",
            "SLV003",
            "SLV004",
        ];
        for token in expected {
            assert!(
                reference.contains(token),
                "the postflop reference is missing {token}"
            );
        }
    }

    #[test]
    fn toml_reference_covers_the_public_surface() {
        let reference = include_str!("../../../docs/multiway-preflop-v1.jp.md");
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
            "ehs2-percentile",
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
