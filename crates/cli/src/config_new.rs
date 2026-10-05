use std::path::Path;

use anyhow::{Context, Result};
use clap::ValueEnum;

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum ConfigTemplate {
    #[default]
    Minimal,
    Full,
}

/// Select the product that will solve the common-input spot.
#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum ConfigProduct {
    #[default]
    P2,
    P1,
}

const P2_MINIMAL: &str = r#"# Six players, 100 BB each; the omitted tree uses the default menus.
schema = "solvers.nlh/v1"

[table]
players = 6
stack_bb = 100
"#;

const P1_MINIMAL: &str = r#"# A small heads-up river spot; the line derives the pot and remaining stacks.
schema = "solvers.nlh/v1"

[table]
players = 2
stack_bb = 10
ante_bb = 1

[spot]
line = "BTN c, BB x / BB x, BTN x / BB x, BTN x"
board = "2c 7d 9h Js Qs"

[ranges]
BTN = "AA,KK"
BB = "AA,KK"

# Keep this tutorial tree and its solve small.
[tree]
script = '''river { replace bet [50] replace raise [50] }
'''
include_allin = false

[tree.max_aggressive_actions]
river = 2

[solver.stop]
max_iterations = 100
"#;

fn normalized(product: ConfigProduct, raw: &str) -> Result<String> {
    let document = spot::Document::parse(raw, Path::new("template.toml"))?;
    Ok(match product {
        ConfigProduct::P1 => document.normalize(&hu_postflop::input::P1Sections)?,
        ConfigProduct::P2 => document.normalize(&mw_preflop::input::P2Sections)?,
    })
}

fn render(product: ConfigProduct, template: ConfigTemplate) -> Result<String> {
    let minimal = match product {
        ConfigProduct::P1 => P1_MINIMAL,
        ConfigProduct::P2 => P2_MINIMAL,
    };
    let effective =
        normalized(product, minimal).context("validating built-in solvers.nlh/v1 template")?;
    if matches!(template, ConfigTemplate::Minimal) {
        return Ok(minimal.into());
    }
    // Render the effective document itself so new default keys cannot drift
    // from the full template. Optional values without defaults stay omitted.
    let mut rendered = String::new();
    for line in effective.lines() {
        if line.starts_with('[') && line.ends_with(']') {
            let section = &line[1..line.len() - 1];
            let description = match section {
                "table" => "Table size, stacks and forced contributions in BB.",
                "economics" => "Cash or tournament utility.",
                "spot" => "Action line and board select the starting spot.",
                "ranges" => "Starting ranges keyed by fixed table position.",
                "tree" => "Betting menus and source-order script rules.",
                "tree.max_aggressive_actions" => "Per-street aggressive-action limits.",
                "solver" => "Product solver settings.",
                "solver.algorithm" => "Regret-discount schedule.",
                "solver.discount" => "Periodic regret-discount cadence.",
                "solver.pruning" => "Regret-based sampling pruning.",
                "solver.abstraction" => "Postflop hand abstraction.",
                "solver.abstraction.buckets" => "Postflop bucket counts by street.",
                "solver.parallel" => "Chance-node parallelism thresholds.",
                "solver.stop" => "Stopping limits and evaluation cadence.",
                "run" => "Resource limits and checkpoint cadence.",
                "output" => "Solution artifact representation.",
                _ => "Effective section values.",
            };
            rendered.push_str("# ");
            rendered.push_str(description);
            rendered.push('\n');
        }
        rendered.push_str(line);
        rendered.push('\n');
    }
    Ok(rendered)
}

pub fn run(product: ConfigProduct, template: ConfigTemplate, out: Option<&Path>) -> Result<()> {
    let rendered = render(product, template)?;
    if let Some(path) = out {
        std::fs::write(path, &rendered)
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
    fn all_templates_validate_and_normalize_idempotently() {
        for product in [ConfigProduct::P1, ConfigProduct::P2] {
            for template in [ConfigTemplate::Minimal, ConfigTemplate::Full] {
                let raw = render(product, template).unwrap();
                let effective = normalized(product, &raw).unwrap();
                assert_eq!(normalized(product, &effective).unwrap(), effective);
                let document = spot::Document::parse(&raw, Path::new("template.toml")).unwrap();
                match product {
                    ConfigProduct::P1 => {
                        let settings = hu_postflop::input::Settings::parse(
                            &document.spot,
                            &document.solver,
                            &document.output,
                        )
                        .unwrap();
                        hu_postflop::input::lower(&document.spot, &settings).unwrap();
                    }
                    ConfigProduct::P2 => {
                        let settings = mw_preflop::input::Settings::parse(
                            &document.spot,
                            &document.solver,
                            &document.output,
                        )
                        .unwrap();
                        mw_preflop::input::lower(&document.spot, &settings).unwrap();
                    }
                }
            }
        }
    }

    #[test]
    fn full_templates_preserve_the_minimal_spot() {
        for product in [ConfigProduct::P1, ConfigProduct::P2] {
            let minimal = render(product, ConfigTemplate::Minimal).unwrap();
            let effective: toml::Value =
                toml::from_str(&normalized(product, &minimal).unwrap()).unwrap();
            let full: toml::Value =
                toml::from_str(&render(product, ConfigTemplate::Full).unwrap()).unwrap();
            assert_eq!(full, effective);
        }
    }

    #[test]
    fn full_templates_list_every_effective_key() {
        for product in [ConfigProduct::P1, ConfigProduct::P2] {
            let raw = render(product, ConfigTemplate::Full).unwrap();
            let full: toml::Value = toml::from_str(&raw).unwrap();
            let effective: toml::Value =
                toml::from_str(&normalized(product, &raw).unwrap()).unwrap();
            assert_eq!(full, effective);
            for (i, line) in raw.lines().enumerate() {
                if line.starts_with('[') && line.ends_with(']') {
                    assert!(raw.lines().nth(i - 1).unwrap().starts_with("# "));
                }
            }
        }
    }

    #[test]
    fn minimal_p1_template_solves() {
        let document = spot::Document::parse(P1_MINIMAL, Path::new("template.toml")).unwrap();
        let settings =
            hu_postflop::input::Settings::parse(&document.spot, &document.solver, &document.output)
                .unwrap();
        let config = hu_postflop::input::lower(&document.spot, &settings).unwrap();
        let payoff = hu_postflop::input::NlhPayoff::new(&document.spot).unwrap();
        let game = hu_postflop::try_build_postflop_game(&config, payoff.pipeline()).unwrap();
        let mut solver = hu_engine::Solver::<_, hu_engine::F32Storage>::new(
            game.game,
            Box::new(hu_engine::Dcfr::default()),
            Some(100),
        );
        solver.run(100);
        assert_eq!(solver.iteration(), 100);
        assert!(
            solver
                .exploitability()
                .0
                .iter()
                .all(|value| value.is_finite())
        );
    }

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
            "derive",
            // global and generated help
            "--help",
            "--version",
            "-h",
            "-V",
            "--cache-dir",
            "SOLVERS_CACHE_DIR",
            "NO_COLOR",
            "SOLVERSD_TOKEN",
            // per-command flags
            "--product",
            "--template",
            "--out",
            "--format",
            "--show-effective",
            "--write-effective",
            "--resources",
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
            "--line",
            "--board",
            "--base",
            // v1 validation fields and template values
            "p1",
            "p2",
            "minimal",
            "full",
            "ruleHitStatus",
            "not-checked",
            "complete",
            "incomplete",
            "warnings",
            "effectiveConfig",
            "memoryLimitBytes",
            "withinLimit",
            "configSchema",
            "gameKind",
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

    #[test]
    fn common_input_reference_covers_every_effective_key_and_literal() {
        let reference = include_str!("../../../docs/nlh-input-v1.jp.md");
        for token in [
            "schema",
            "meta",
            "name",
            "description",
            "derived_from",
            "run_id",
            "solution_hash",
            "line",
            "board",
            "table",
            "players",
            "stack_bb",
            "stacks_bb",
            "sb_bb",
            "ante_bb",
            "bb_ante_bb",
            "straddles_bb",
            "economics",
            "cash",
            "tournament",
            "rake",
            "rate",
            "cap_bb",
            "when",
            "allocation",
            "rounding",
            "rounding_unit_bb",
            "payouts",
            "outside_field_bb",
            "samples",
            "seed",
            "spot",
            "ranges",
            "random",
            "tree",
            "script",
            "source",
            "params",
            "include_allin",
            "allin_threshold",
            "preflop_reraise_jam_above_stack",
            "numerator",
            "denominator",
            "max_aggressive_actions",
            "preflop",
            "flop",
            "turn",
            "river",
            "solver",
            "iso_merging",
            "storage",
            "algorithm",
            "schedule",
            "alpha",
            "beta",
            "gamma",
            "pow4_reset",
            "gamma0",
            "vanilla",
            "cfr-plus",
            "dcfr",
            "linear-cfr",
            "hs-dcfr",
            "f32",
            "i16",
            "parallel",
            "chance_depth",
            "min_children",
            "kind",
            "range-vector",
            "single-hand",
            "opponent_exploration",
            "batch_sweeps",
            "abstraction",
            "ehs2-percentile",
            "buckets",
            "discount",
            "periodic",
            "none",
            "every_sweeps",
            "until_sweeps",
            "pruning",
            "regret-based",
            "stop",
            "target",
            "default",
            "off",
            "max_iterations",
            "check_every",
            "max_sweeps",
            "check_every_sweeps",
            "confirmations",
            "evaluation_samples",
            "deviator_traversals",
            "run",
            "threads",
            "memory",
            "auto",
            "max_time",
            "checkpoint_interval",
            "output",
            "probability_encoding",
            "u16",
            "solution_streets",
            "full",
            "no-rivers",
            "param",
            "define",
            "when",
            "if",
            "else",
            "add",
            "remove",
            "replace",
            "force",
            "checkdown",
            "bet",
            "raise",
            "fold",
            "check",
            "call",
        ] {
            assert!(
                reference.contains(token),
                "common input reference is missing {token}"
            );
        }
        fn check(value: &toml::Value, reference: &str) {
            match value {
                toml::Value::Table(table) => {
                    for (key, value) in table {
                        assert!(
                            reference.contains(key),
                            "common input reference is missing key {key}"
                        );
                        if !matches!(
                            key.as_str(),
                            "script" | "line" | "board" | "BTN" | "BB" | "SB" | "UTG" | "HJ" | "CO"
                        ) {
                            check(value, reference);
                        }
                    }
                }
                toml::Value::String(literal) => {
                    assert!(
                        reference.contains(literal),
                        "common input reference is missing literal {literal}"
                    );
                }
                _ => {}
            }
        }
        for product in [ConfigProduct::P1, ConfigProduct::P2] {
            let raw = render(product, ConfigTemplate::Full).unwrap();
            check(&toml::from_str::<toml::Value>(&raw).unwrap(), reference);
        }
    }
}
