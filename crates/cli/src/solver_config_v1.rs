//! The `solvers.toy/v1`, `solvers.postflop/v1`, and `solvers.preflop-hu/v1`
//! contracts.
//!
//! These three families share every section except `[game]`, so they share
//! one document (`docs/solver-config-v1.jp.md`) and one module. What makes
//! them contracts rather than version markers is what this module adds over
//! plain deserialization:
//!
//! * **A key belongs to exactly one family.** They used to share one struct,
//!   so a postflop config could set `stop_dev_gain` or `sweeps` -- multiway
//!   knobs -- and have them silently ignored. Each family now names the keys
//!   it honours and rejects the rest.
//! * **Defaults are explicit in the effective config.** Every optional key
//!   with a default is declared with that default, so normalizing is a
//!   round-trip and the result says what the run will actually do.
//! * **Failures carry codes.** `SLV###`, the way Multiway Preflop has
//!   `MWP###`, so a client can branch on the kind of problem.
//!
//! Multiway Preflop stays in `multiway_v1`: its surface is a different shape
//! that lowers into the same internal config, and it has its own document.

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::config::{
    AlgorithmSection, BetsSection, GameSection, PostflopSection, RakeSection, RunSection,
    SolveConfig, StorageKind, UtilitySection,
};

pub const SCHEMA_TOY: &str = "solvers.toy/v1";
pub const SCHEMA_POSTFLOP: &str = "solvers.postflop/v1";
pub const SCHEMA_PREFLOP_HU: &str = "solvers.preflop-hu/v1";

/// Every schema this module owns.
pub const SCHEMAS: [&str; 3] = [SCHEMA_TOY, SCHEMA_POSTFLOP, SCHEMA_PREFLOP_HU];

pub fn owns(schema: &str) -> bool {
    SCHEMAS.contains(&schema)
}

// --- run sections -----------------------------------------------------------

/// Run controls every family here honours.
///
/// Multiway's sampling knobs (`sweeps`, `evaluation_*`, `stop_*`,
/// `sweep_batch`, `max_memory_bytes`, `checkpoint_every`) are absent on
/// purpose: these games are solved by the exact vector engine, which does
/// none of those things, and accepting them would mean accepting a lie.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunToy {
    /// Total iteration budget.
    pub iterations: u64,
    /// Exploitability check cadence, in iterations.
    #[serde(default = "default_check_every")]
    pub check_every: u64,
    /// Regret/strategy storage backend.
    #[serde(default)]
    pub storage: StorageKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// Stop once NashConv drops below this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_nash_conv: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threads: Option<usize>,
}

/// [`RunToy`] plus the tree-parallelism knobs, which only mean something
/// for a game with chance nodes.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunHeadsUp {
    pub iterations: u64,
    #[serde(default = "default_check_every")]
    pub check_every: u64,
    #[serde(default)]
    pub storage: StorageKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_nash_conv: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threads: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub par_chance_depth: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub par_min_children: Option<usize>,
}

fn default_check_every() -> u64 {
    25
}

impl RunToy {
    fn lower(self) -> RunSection {
        RunHeadsUp {
            iterations: self.iterations,
            check_every: self.check_every,
            storage: self.storage,
            seed: self.seed,
            target_nash_conv: self.target_nash_conv,
            threads: self.threads,
            par_chance_depth: None,
            par_min_children: None,
        }
        .lower()
    }
}

impl RunHeadsUp {
    /// Fills the shared internal run section, leaving every multiway field
    /// at the value that means "not applicable".
    fn lower(self) -> RunSection {
        RunSection {
            iterations: self.iterations,
            check_every: self.check_every,
            storage: self.storage,
            seed: self.seed,
            target_nash_conv: self.target_nash_conv,
            threads: self.threads,
            par_chance_depth: self.par_chance_depth,
            par_min_children: self.par_min_children,
            sweeps: None,
            max_memory_bytes: None,
            checkpoint_every: None,
            evaluation_samples: None,
            evaluation_cadence: None,
            sweep_batch: None,
            stop_dev_gain: None,
            stop_confirmations: None,
            stop_eval_period_secs: None,
            stop_br_traversals: None,
        }
    }
}

// --- families ---------------------------------------------------------------

/// `solvers.toy/v1` covers two games, so this family keeps a `kind`. The
/// other two do not: their schema already names the game, and carrying it
/// twice would be two things to keep in agreement.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "kebab-case")]
pub enum ToyGame {
    Kuhn,
    Leduc,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PostflopGame {
    /// Whitespace-separated board, 3 to 5 cards. Its length picks the
    /// starting street.
    pub board: String,
    /// Out-of-position range; this player acts first postflop.
    pub oop_range: String,
    pub ip_range: String,
    pub pot: u32,
    pub effective_stack: u32,
    /// Merge suit-isomorphic turn/river deals.
    #[serde(default = "default_true")]
    pub iso_merging: bool,
    pub bets: BetsSection,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreflopHuGame {
    pub effective_stack_bb: f64,
    #[serde(default = "default_sb_bb")]
    pub sb_bb: f64,
    #[serde(default = "default_open_sizes_bb")]
    pub open_sizes_bb: Vec<f64>,
    #[serde(default = "default_raise_factors")]
    pub raise_factors: Vec<Vec<f64>>,
    #[serde(default = "default_preflop_max_raises")]
    pub max_raises: u32,
    #[serde(default = "default_true")]
    pub include_allin: bool,
    #[serde(default = "default_true")]
    pub allow_limp: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sb_range: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bb_range: Option<String>,
    /// Per-player equity realization applied at the postflop boundary.
    #[serde(default = "default_equity_realization")]
    pub equity_realization: [f64; 2],
    /// Postflop model. Absent means the equity-showdown model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postflop: Option<PostflopSection>,
}

fn default_true() -> bool {
    true
}
fn default_sb_bb() -> f64 {
    0.5
}
fn default_open_sizes_bb() -> Vec<f64> {
    vec![2.5]
}
fn default_raise_factors() -> Vec<Vec<f64>> {
    vec![vec![3.0]]
}
fn default_preflop_max_raises() -> u32 {
    4
}
fn default_equity_realization() -> [f64; 2] {
    [1.0, 1.0]
}

/// One config file, in whichever family it declared.
///
/// The three are separate types rather than one with an optional `[game]`,
/// so the compiler enforces that each family's run section travels with its
/// own game section.
#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum Family {
    Toy(ToyConfig),
    Postflop(PostflopConfig),
    PreflopHu(PreflopHuConfig),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ToyConfig {
    schema: String,
    game: ToyGame,
    #[serde(default)]
    rake: RakeSection,
    #[serde(default)]
    utility: UtilitySection,
    #[serde(default)]
    algorithm: AlgorithmSection,
    run: RunToy,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PostflopConfig {
    schema: String,
    game: PostflopGame,
    #[serde(default)]
    rake: RakeSection,
    #[serde(default)]
    utility: UtilitySection,
    #[serde(default)]
    algorithm: AlgorithmSection,
    run: RunHeadsUp,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PreflopHuConfig {
    schema: String,
    game: PreflopHuGame,
    #[serde(default)]
    rake: RakeSection,
    #[serde(default)]
    utility: UtilitySection,
    #[serde(default)]
    algorithm: AlgorithmSection,
    run: RunHeadsUp,
}

/// Reads the `schema` key without committing to a family.
fn declared_schema(raw: &str) -> Result<String> {
    let document: toml::Value = toml::from_str(raw).context("parsing TOML document")?;
    if let Some(schema) = document.get("schema").and_then(toml::Value::as_str) {
        return Ok(schema.to_string());
    }
    // A lowered multiway config has no schema either, and saying "declare
    // one of these three" would send its author to the wrong place.
    let kind = document
        .get("game")
        .and_then(toml::Value::as_table)
        .and_then(|game| game.get("kind"))
        .and_then(toml::Value::as_str);
    if kind == Some("preflop-multiway") {
        bail!(
            "MWP003: hand-written lowered preflop-multiway configs are not accepted; \
             write schema = {:?} instead",
            crate::multiway_v1::SCHEMA
        );
    }
    bail!(
        "SLV001: config has no schema; declare one of {}",
        SCHEMAS.join(", ")
    )
}

fn parse_family(raw: &str) -> Result<Family> {
    let schema = declared_schema(raw)?;
    // Parsing per declared family, rather than by trying each in turn,
    // means an error names the keys *that* family accepts instead of
    // reporting that the file matched none of three shapes.
    let family = match schema.as_str() {
        SCHEMA_TOY => Family::Toy(toml::from_str(raw).map_err(key_error)?),
        SCHEMA_POSTFLOP => Family::Postflop(toml::from_str(raw).map_err(key_error)?),
        SCHEMA_PREFLOP_HU => Family::PreflopHu(toml::from_str(raw).map_err(key_error)?),
        other => bail!(
            "SLV001: unsupported schema {other:?}; this parser handles {}",
            SCHEMAS.join(", ")
        ),
    };
    validate_semantics(&family)?;
    Ok(family)
}

/// Turns serde's unknown-field message into something that says why.
///
/// The common case is a key that belongs to Multiway Preflop, since these
/// families used to share one struct that accepted every key and ignored the
/// ones it did not use.
fn key_error(error: toml::de::Error) -> anyhow::Error {
    let message = error.to_string();
    if message.contains("unknown field") {
        anyhow!(
            "SLV002: {message}\nA key from another family is an error here, not a no-op: \
             sampling controls such as `sweeps`, `evaluation_samples`, and `stop_dev_gain` \
             belong to schema = \"solvers.multiway-preflop/v1\"."
        )
    } else {
        anyhow!("SLV002: {message}")
    }
}

fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

/// Checks the things a type cannot: values in range, and sections whose
/// variant does not apply to this family.
fn validate_semantics(family: &Family) -> Result<()> {
    let (algorithm, utility) = match family {
        Family::Toy(config) => (&config.algorithm, &config.utility),
        Family::Postflop(config) => (&config.algorithm, &config.utility),
        Family::PreflopHu(config) => (&config.algorithm, &config.utility),
    };
    if matches!(algorithm, AlgorithmSection::ExternalSamplingMccfr { .. }) {
        bail!(
            "SLV003: schedule = \"external-sampling-mccfr\" is the multiway sampler; \
             these families are solved by the exact vector engine"
        );
    }
    if matches!(utility, UtilitySection::TournamentIcm { .. }) {
        bail!(
            "SLV003: kind = \"tournament-icm\" is the multiway payout model; \
             heads-up games use kind = \"icm\" with two payouts"
        );
    }

    match family {
        Family::Toy(_) => {}
        Family::Postflop(config) => {
            let game = &config.game;
            // Parsed with the same functions the solve path uses, so
            // `validate` cannot accept a board or range that `solve` then
            // rejects.
            let board = crate::postflop_setup::parse_board(&game.board)
                .map_err(|error| anyhow!("SLV004: {error}"))?;
            if !(3..=5).contains(&board.len()) {
                bail!(
                    "SLV004: board has {} cards; a postflop board is 3, 4, or 5",
                    board.len()
                );
            }
            crate::postflop_setup::parse_range("oop_range", &game.oop_range)
                .map_err(|error| anyhow!("SLV004: {error}"))?;
            crate::postflop_setup::parse_range("ip_range", &game.ip_range)
                .map_err(|error| anyhow!("SLV004: {error}"))?;
            if game.pot == 0 {
                bail!("SLV004: pot must be positive");
            }
            if game.effective_stack == 0 {
                bail!("SLV004: effective_stack must be positive");
            }
        }
        Family::PreflopHu(config) => {
            let game = &config.game;
            // `is_finite` first, so NaN and infinity are rejected rather
            // than slipping through a bare comparison.
            if !positive(game.effective_stack_bb) {
                bail!("SLV004: effective_stack_bb must be a positive, finite number");
            }
            if !positive(game.sb_bb) || game.sb_bb >= 1.0 {
                bail!("SLV004: sb_bb must be greater than 0 and less than the big blind");
            }
            for realization in game.equity_realization {
                if !positive(realization) {
                    bail!("SLV004: equity_realization entries must be positive, finite numbers");
                }
            }
            for (label, range) in [("sb_range", &game.sb_range), ("bb_range", &game.bb_range)] {
                if let Some(range) = range {
                    crate::postflop_setup::parse_range(label, range)
                        .map_err(|error| anyhow!("SLV004: {error}"))?;
                }
            }
        }
    }
    Ok(())
}

/// Parses a config and lowers it into the shared internal representation.
pub fn parse_and_lower(raw: &str) -> Result<SolveConfig> {
    Ok(match parse_family(raw)? {
        Family::Toy(config) => SolveConfig {
            schema: Some(config.schema),
            game: match config.game {
                ToyGame::Kuhn => GameSection::Kuhn,
                ToyGame::Leduc => GameSection::Leduc,
            },
            rake: config.rake,
            utility: config.utility,
            algorithm: config.algorithm,
            run: config.run.lower(),
        },
        Family::Postflop(config) => SolveConfig {
            schema: Some(config.schema),
            game: GameSection::Postflop {
                board: config.game.board,
                oop_range: config.game.oop_range,
                ip_range: config.game.ip_range,
                pot: config.game.pot,
                effective_stack: config.game.effective_stack,
                iso_merging: config.game.iso_merging,
                bets: config.game.bets,
            },
            rake: config.rake,
            utility: config.utility,
            algorithm: config.algorithm,
            run: config.run.lower(),
        },
        Family::PreflopHu(config) => SolveConfig {
            schema: Some(config.schema),
            game: GameSection::Preflop {
                effective_stack_bb: config.game.effective_stack_bb,
                sb_bb: config.game.sb_bb,
                open_sizes_bb: config.game.open_sizes_bb,
                raise_factors: config.game.raise_factors,
                max_raises: config.game.max_raises,
                include_allin: config.game.include_allin,
                allow_limp: config.game.allow_limp,
                sb_range: config.game.sb_range,
                bb_range: config.game.bb_range,
                equity_realization: config.game.equity_realization,
                equity_cache: None,
                postflop: config.game.postflop,
            },
            rake: config.rake,
            utility: config.utility,
            algorithm: config.algorithm,
            run: config.run.lower(),
        },
    })
}

/// The config with every default written out.
///
/// Defaults live in the field declarations, so normalizing is deserialize +
/// serialize and is idempotent by construction: normalizing an effective
/// config returns the same bytes.
pub fn normalized_toml(raw: &str) -> Result<String> {
    let family = parse_family(raw)?;
    toml::to_string_pretty(&family).context("serializing the effective config")
}

/// The same, as JSON, for `validate --format json --show-effective`.
pub fn normalized_json(raw: &str) -> Result<serde_json::Value> {
    let family = parse_family(raw)?;
    serde_json::to_value(&family).context("serializing the effective config")
}

/// The schema a config declares, once it is known to parse.
pub fn declared(raw: &str) -> Result<String> {
    parse_family(raw)?;
    declared_schema(raw)
}

/// The `game.kind` a config declares, for the run manifest.
pub fn game_kind(raw: &str) -> Result<&'static str> {
    Ok(match parse_family(raw)? {
        Family::Toy(config) => match config.game {
            ToyGame::Kuhn => "kuhn",
            ToyGame::Leduc => "leduc",
        },
        Family::Postflop(_) => "postflop",
        Family::PreflopHu(_) => "preflop",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const POSTFLOP: &str = r#"
schema = "solvers.postflop/v1"

[game]
board = "2c 7d 9h Js Qs"
oop_range = "22+"
ip_range = "22+"
pot = 20
effective_stack = 80

[game.bets.flop]
oop = [0.5]
ip = [0.5]
[game.bets.turn]
oop = [0.5]
ip = [0.5]
[game.bets.river]
oop = [0.5]
ip = [0.5]

[run]
iterations = 100
"#;

    const TOY: &str =
        "schema = \"solvers.toy/v1\"\n\n[game]\nkind = \"kuhn\"\n\n[run]\niterations = 10\n";

    /// The defect this contract exists to fix: a multiway sampling knob in a
    /// postflop config used to be accepted and ignored.
    #[test]
    fn a_key_from_another_family_is_rejected() {
        let mixed = POSTFLOP.replace("iterations = 100", "iterations = 100\nstop_dev_gain = 0.5");
        let error = parse_and_lower(&mixed).unwrap_err().to_string();
        assert!(error.contains("SLV002"), "{error}");
        assert!(error.contains("stop_dev_gain"), "{error}");
        assert!(error.contains("multiway-preflop"), "{error}");

        for key in ["sweeps = 5", "evaluation_samples = 8", "sweep_batch = 2"] {
            let mixed = TOY.replace("iterations = 10", &format!("iterations = 10\n{key}"));
            assert!(parse_and_lower(&mixed).is_err(), "{key} was accepted");
        }
    }

    #[test]
    fn a_missing_or_unknown_schema_says_which_ones_exist() {
        let error = parse_and_lower("[game]\nkind = \"kuhn\"\n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("SLV001"), "{error}");
        assert!(error.contains("solvers.toy/v1"), "{error}");

        let error = parse_and_lower("schema = \"nope/v1\"\n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("SLV001"), "{error}");
    }

    /// A family's own game section must match its schema.
    #[test]
    fn a_game_section_from_another_family_is_rejected() {
        let wrong = POSTFLOP.replace(
            "schema = \"solvers.postflop/v1\"",
            "schema = \"solvers.toy/v1\"",
        );
        assert!(parse_and_lower(&wrong).is_err());
    }

    /// Normalizing must be idempotent, or an effective config could not be
    /// resubmitted as one.
    #[test]
    fn normalizing_is_idempotent_and_writes_the_defaults_out() {
        let once = normalized_toml(POSTFLOP).unwrap();
        assert!(once.contains("iso_merging = true"), "{once}");
        assert!(once.contains("check_every = 25"), "{once}");
        assert!(once.contains("storage = "), "{once}");
        assert_eq!(normalized_toml(&once).unwrap(), once);

        // And it still means the same game.
        let from_source = parse_and_lower(POSTFLOP).unwrap();
        let from_effective = parse_and_lower(&once).unwrap();
        assert_eq!(
            format!("{:?}", from_source.game),
            format!("{:?}", from_effective.game)
        );
    }

    #[test]
    fn the_multiway_sampler_and_payout_model_are_refused() {
        let sampled = TOY.replace(
            "[run]",
            "[algorithm]\nschedule = \"external-sampling-mccfr\"\n\n[run]",
        );
        let error = parse_and_lower(&sampled).unwrap_err().to_string();
        assert!(error.contains("SLV003"), "{error}");

        let icm = TOY.replace(
            "[run]",
            "[utility]\nkind = \"tournament-icm\"\npayouts = [1.0, 0.0]\n\n[run]",
        );
        let error = parse_and_lower(&icm).unwrap_err().to_string();
        assert!(error.contains("SLV003"), "{error}");
    }

    /// `validate` must not accept a board or range that `solve` rejects, so
    /// both go through the same parsers.
    #[test]
    fn boards_and_ranges_are_checked_with_the_solve_path_parsers() {
        for (bad, needle) in [
            ("board = \"2c 7d\"", "board"),
            ("oop_range = \"not-a-range\"", "oop_range"),
            ("pot = 20", "pot"),
        ] {
            let broken = match needle {
                "pot" => POSTFLOP.replace("pot = 20", "pot = 0"),
                _ => POSTFLOP.replace(
                    match needle {
                        "board" => "board = \"2c 7d 9h Js Qs\"",
                        _ => "oop_range = \"22+\"",
                    },
                    bad,
                ),
            };
            let error = parse_and_lower(&broken).unwrap_err().to_string();
            assert!(error.contains("SLV004"), "{needle}: {error}");
        }
    }

    /// The contract restates the defaults the internal shape has carried,
    /// and the two must not drift: a config that omits a key has to mean
    /// the same thing whichever parser reads it.
    #[test]
    fn contract_defaults_match_the_internal_shape() {
        let minimal = "schema = \"solvers.preflop-hu/v1\"\n\n\
             [game]\neffective_stack_bb = 100.0\n\n[run]\niterations = 10\n";
        let internal = "[game]\nkind = \"preflop\"\neffective_stack_bb = 100.0\n\n\
             [run]\niterations = 10\n";

        let from_contract = parse_and_lower(minimal).unwrap();
        let from_internal: SolveConfig = toml::from_str(internal).unwrap();
        assert_eq!(
            format!("{:?}", from_contract.game),
            format!("{:?}", from_internal.game),
            "a contract default drifted from the internal one"
        );
        assert_eq!(from_contract.run.check_every, from_internal.run.check_every);
    }

    /// The budget used to default to zero, which solved nothing and said so
    /// only by finishing instantly.
    #[test]
    fn the_iteration_budget_is_required() {
        let without = TOY.replace("iterations = 10", "");
        let error = parse_and_lower(&without).unwrap_err().to_string();
        assert!(error.contains("iterations"), "{error}");
    }

    #[test]
    fn the_declared_kind_is_reported_for_the_manifest() {
        assert_eq!(game_kind(TOY).unwrap(), "kuhn");
        assert_eq!(game_kind(POSTFLOP).unwrap(), "postflop");
    }
}
