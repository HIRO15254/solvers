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

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use cards::script::{ActionKind, Effect, ParamKind, ParamSchema, PostflopVar, Rule, Script};
use cards::{SizeSpec, SizeUnit, Street};
use serde::{Deserialize, Serialize};

use crate::config::{
    AlgorithmSection, GameSection, OutsidePlayerSection, PostflopSection, RakeSection, RunSection,
    SolveConfig, StorageKind, TreeSection, UtilitySection,
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
    /// Total iteration budget. A safety budget, not a convergence
    /// criterion: reaching it says nothing about how solved the game is.
    #[serde(default = "default_iterations")]
    pub iterations: u64,
    /// Cumulative solve-time budget, `s`/`m`/`h` suffixed as in Multiway
    /// Preflop's `run.max_time`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_time: Option<String>,
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
    #[serde(default = "default_iterations")]
    pub iterations: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_time: Option<String>,
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

/// The budget an omitted `iterations` means. Sized like Multiway Preflop's
/// `run.max_sweeps` default: large enough that a run stops on its
/// convergence target or its clock rather than on the counter, small enough
/// to be a real backstop.
fn default_iterations() -> u64 {
    1_000_000
}

impl RunToy {
    fn lower(self) -> Result<RunSection> {
        RunHeadsUp {
            iterations: self.iterations,
            max_time: self.max_time,
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

/// Parses a `run.max_time` duration into seconds. Same grammar as Multiway
/// Preflop's `run.max_time` and `run.checkpoint.interval`: a positive
/// integer with a lower-case `s`, `m`, or `h` suffix.
fn parse_max_time(value: &str) -> Result<u64> {
    for (suffix, factor) in [("s", 1), ("m", 60), ("h", 3_600)] {
        if let Some(number) = value.strip_suffix(suffix) {
            let count = number
                .parse::<u64>()
                .map_err(|_| anyhow!("SLV004: invalid max_time {value:?}"))?;
            if count == 0 {
                bail!("SLV004: max_time must be positive");
            }
            return count
                .checked_mul(factor)
                .ok_or_else(|| anyhow!("SLV004: max_time {value:?} overflows"));
        }
    }
    bail!("SLV004: invalid max_time {value:?}; use a positive count with an s, m, or h suffix")
}

impl RunHeadsUp {
    /// Fills the shared internal run section, leaving every multiway field
    /// at the value that means "not applicable".
    fn lower(self) -> Result<RunSection> {
        let max_time_secs = self.max_time.as_deref().map(parse_max_time).transpose()?;
        Ok(RunSection {
            iterations: self.iterations,
            max_time_secs,
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
        })
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
    /// Smallest legal opening bet and smallest legal raise increment. A
    /// postflop subgame has no blinds, so this is what the big blind would
    /// otherwise supply to the NLHE minimum-raise rule.
    #[serde(default = "default_min_bet")]
    pub min_bet: u32,
    /// Merge suit-isomorphic turn/river deals.
    #[serde(default = "default_true")]
    pub iso_merging: bool,
    /// The side that last bet or raised before this subgame began --
    /// `"oop"` / `"ip"` / `"none"` (the default). Exists only to define the
    /// tree script's `cbet`/`donk` variables on the starting street.
    #[serde(default = "default_preflop_aggressor")]
    pub preflop_aggressor: String,
    /// The betting tree. `kind = "none"` (the default) is a check-down: a
    /// config that offers no bets says so rather than failing to mention
    /// them.
    #[serde(default)]
    pub tree: TreeSection,
}

fn default_preflop_aggressor() -> String {
    "none".to_string()
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
fn default_min_bet() -> u32 {
    1
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

/// `[utility]` for `solvers.postflop/v1`.
///
/// It differs from the shared [`UtilitySection`] in exactly one place:
/// `tournament-icm`'s outside field is a plain list of stacks in the
/// config's own chip unit, matching Multiway Preflop's
/// `economics.outside_field_bb` rather than the lowered
/// `{ name, stack_bb }` tables.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "kebab-case")]
pub enum PostflopUtility {
    #[default]
    ChipEv,
    Icm {
        payouts: [f64; 2],
    },
    TournamentIcm {
        /// Remaining prizes, best finish first. Unpaid places may be
        /// omitted rather than written as zeros.
        payouts: Vec<f64>,
        /// Stacks of players away from this table, in the same chip unit as
        /// `effective_stack`.
        #[serde(default)]
        outside_field: Vec<f64>,
        #[serde(default = "default_icm_samples")]
        samples: u64,
        #[serde(default)]
        seed: u64,
    },
}

fn default_icm_samples() -> u64 {
    100_000
}

impl PostflopUtility {
    fn lower(self) -> UtilitySection {
        match self {
            PostflopUtility::ChipEv => UtilitySection::ChipEv,
            PostflopUtility::Icm { payouts } => UtilitySection::Icm { payouts },
            PostflopUtility::TournamentIcm {
                payouts,
                outside_field,
                samples,
                seed,
            } => UtilitySection::TournamentIcm {
                outside_field: outside_field
                    .into_iter()
                    .enumerate()
                    .map(|(index, stack)| OutsidePlayerSection {
                        name: format!("outside-{index}"),
                        stack_bb: stack,
                    })
                    .collect(),
                payouts,
                samples,
                seed,
            },
        }
    }
}

/// One config file, in whichever family it declared.
///
/// The three are separate types rather than one with an optional `[game]`,
/// so the compiler enforces that each family's run section travels with its
/// own game section.
// Parsed once per config file and destructured away immediately; the size
// difference between the three families never sits on a hot path, so boxing
// a variant to appease the lint would just add an allocation.
#[allow(clippy::large_enum_variant)]
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
    utility: PostflopUtility,
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

/// [`parse_family`] with a base directory to resolve a postflop
/// `[game.tree] source` against. `base_dir` is `None` when no config file
/// path is available (round-tripping already-inlined bytes, or a config
/// known not to use `source`); a `[game.tree] source` config parsed that
/// way fails explicitly rather than silently resolving against the current
/// directory.
fn parse_family_at(raw: &str, base_dir: Option<&Path>) -> Result<Family> {
    let schema = declared_schema(raw)?;
    // Parsing per declared family, rather than by trying each in turn,
    // means an error names the keys *that* family accepts instead of
    // reporting that the file matched none of three shapes.
    let mut family = match schema.as_str() {
        SCHEMA_TOY => Family::Toy(toml::from_str(raw).map_err(key_error)?),
        SCHEMA_POSTFLOP => {
            reject_retired_postflop_keys(raw)?;
            Family::Postflop(toml::from_str(raw).map_err(key_error)?)
        }
        SCHEMA_PREFLOP_HU => Family::PreflopHu(toml::from_str(raw).map_err(key_error)?),
        other => bail!(
            "SLV001: unsupported schema {other:?}; this parser handles {}",
            SCHEMAS.join(", ")
        ),
    };
    if let Family::Postflop(config) = &mut family {
        // Checked on what the author actually wrote, before `source` (if
        // any) is resolved into `script` below -- resolving first would
        // make a legitimate `source`-only config look like it named
        // neither.
        check_tree_shape(&config.game.tree)?;
        if config.game.tree.source.is_some() {
            let base_dir = base_dir.ok_or_else(|| {
                anyhow!(
                    "SLV004: [game.tree] source requires a config file path for relative \
                     resolution; run this through a command that knows the file's location"
                )
            })?;
            config.game.tree.resolve_source_at(base_dir)?;
        }
    }
    validate_semantics(&family)?;
    Ok(family)
}

fn parse_family(raw: &str) -> Result<Family> {
    parse_family_at(raw, None)
}

/// `source`/`script` are exclusive, and `kind = "none"` takes neither --
/// `docs/solver-config-v1.jp.md`'s `[game.tree]` chapter and its 廃止した key
/// table. An unrecognized `kind` is left to `validate_semantics` (`SLV004`,
/// a value problem rather than a shape one).
fn check_tree_shape(tree: &TreeSection) -> Result<()> {
    match tree.kind.as_str() {
        "none" => {
            if tree.source.is_some() || tree.script.is_some() {
                bail!(
                    "SLV002: [game.tree] kind = \"none\" takes neither source nor script; a \
                     check-down tree has no script to read. Set kind = \"script\" to use one."
                );
            }
        }
        "script" => {
            if tree.source.is_some() && tree.script.is_some() {
                bail!("SLV002: [game.tree] source and script are exclusive; write only one.");
            }
            if tree.source.is_none() && tree.script.is_none() {
                bail!(
                    "SLV002: [game.tree] kind = \"script\" requires source or script; write one \
                     of them, or set kind = \"none\" for a check-down tree."
                );
            }
        }
        _ => {}
    }
    Ok(())
}

/// Names the replacement for a key this contract retired.
///
/// `deny_unknown_fields` would already reject these, but "unknown field
/// `bets`" does not tell an author that the section moved and was renamed.
/// A retired key is an error either way; this only makes it an actionable
/// one. Run before the typed parse so the better message wins.
fn reject_retired_postflop_keys(raw: &str) -> Result<()> {
    let document: toml::Value = toml::from_str(raw).context("parsing TOML document")?;
    let Some(game) = document.get("game").and_then(toml::Value::as_table) else {
        return Ok(());
    };
    if let Some(bets) = game.get("bets") {
        let mut retired: Vec<&str> = Vec::new();
        if let Some(streets) = bets.as_table() {
            for street in streets.values().filter_map(toml::Value::as_table) {
                for (old, new) in [
                    ("oop", "oop_bet"),
                    ("ip", "ip_bet"),
                    ("max_raises", "max_aggressive_actions"),
                ] {
                    if street.contains_key(old) && !retired.contains(&new) {
                        retired.push(new);
                    }
                }
            }
        }
        let renames = if retired.is_empty() {
            String::new()
        } else {
            format!(" Inside it, rename to: {}.", retired.join(", "))
        };
        bail!(
            "SLV002: [game.bets] was renamed to [game.tree], which now matches Multiway \
             Preflop's [game.tree].{renames} Bet sizes are size literals such as \"50%pot\", \
             \"3x\", or \"allin\"; a bare fraction still means a pot fraction."
        );
    }
    if let Some(tree) = game.get("tree").and_then(toml::Value::as_table) {
        let retired_streets: Vec<&str> = ["flop", "turn", "river"]
            .into_iter()
            .filter(|street| tree.contains_key(*street))
            .collect();
        // The menu keys are also caught one level up, where an author who
        // deleted the `[game.tree.<street>]` headers but kept their contents
        // would land. `deny_unknown_fields` rejects those too, but only with
        // a field list -- it cannot say where each key went.
        let retired_menu_keys: Vec<&str> =
            ["oop_bet", "ip_bet", "oop_raise", "ip_raise", "oop_donk"]
                .into_iter()
                .filter(|key| tree.contains_key(*key))
                .collect();
        let sections = if !retired_streets.is_empty() {
            retired_streets
                .iter()
                .map(|street| format!("[game.tree.{street}]"))
                .collect::<Vec<_>>()
                .join(", ")
        } else if !retired_menu_keys.is_empty() {
            retired_menu_keys
                .iter()
                .map(|key| format!("[game.tree] {key}"))
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            String::new()
        };
        if !sections.is_empty() {
            bail!(
                "SLV002: {sections} was removed; every street's betting menu is now a tree \
                 script under [game.tree] (kind = \"script\", source or script). Inside it: \
                 oop_bet/ip_bet become `replace bet [...]`, oop_raise/ip_raise become `replace \
                 raise [...]` (raise level is the `aggressions` variable), oop_donk becomes \
                 `when donk {{ ... }}`, per-street max_aggressive_actions moves to the \
                 [game.tree.max_aggressive_actions] table, and per-street include_allin / \
                 allin_threshold move to the tree-level [game.tree] keys of the same name."
            );
        }
    }
    Ok(())
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

fn street_name(street: Street) -> &'static str {
    match street {
        Street::Flop => "flop",
        Street::Turn => "turn",
        Street::River => "river",
        Street::Preflop => unreachable!("a postflop board never starts preflop"),
    }
}

/// Every rule's sizes, checked against the compiled script's own rule list
/// rather than a TOML menu section -- the tree-script surface has no other
/// place a bad size literal's *value* (as opposed to its syntax, which
/// `SizeSpec::parse` already rejects at compile time) could hide.
fn validate_rule_sizes(rules: &[Rule<PostflopVar>]) -> Result<()> {
    for rule in rules {
        let label = format!(
            "a {} rule on {}",
            match rule.action {
                Some(cards::script::ActionKind::Bet) => "bet",
                Some(cards::script::ActionKind::Raise) => "raise",
                None => "checkdown",
                // `POSTFLOP`'s dialect (`cards::script::cond::POSTFLOP`)
                // lists only `bet`/`raise` as accepted action words, so a
                // compiled postflop rule never carries any other action.
                Some(cards::script::ActionKind::Fold)
                | Some(cards::script::ActionKind::Check)
                | Some(cards::script::ActionKind::Call) => {
                    unreachable!("postflop's dialect only accepts bet/raise actions")
                }
            },
            street_name(rule.street)
        );
        for size in &rule.sizes {
            check_size(&label, *size)?;
        }
    }
    Ok(())
}

/// Every literal carries a number the grammar accepted but the tree still
/// has to live with: a non-finite or non-positive fraction would resolve to
/// a nonsense chip target.
fn check_size(label: &str, size: SizeSpec) -> Result<()> {
    let value = match size {
        SizeSpec::PotAfterCall { fraction }
        | SizeSpec::StackFraction { fraction }
        | SizeSpec::EffectiveStackFraction { fraction } => fraction,
        SizeSpec::PreviousBetMultiple { factor } => factor,
        SizeSpec::ToChips { value } => value,
        SizeSpec::MinRaise | SizeSpec::AllIn | SizeSpec::GeometricAllInRemaining => 1.0,
        SizeSpec::GeometricAllIn { streets } => {
            if streets == 0 {
                bail!("SLV004: {label} geometric street count must be positive");
            }
            1.0
        }
        SizeSpec::ToBb { .. } => bail!(
            "SLV004: {label} uses a big-blind size literal; the postflop family has no blinds, \
             so absolute sizes are chips (for example \"40c\")"
        ),
    };
    if !positive(value) {
        bail!("SLV004: {label} has a bet size that is not a positive, finite number");
    }
    Ok(())
}

/// Postflop is the one heads-up family that accepts the multiway economics
/// models, so it is also the one that has to check their numbers.
fn validate_postflop_economics(rake: &RakeSection, utility: &PostflopUtility) -> Result<()> {
    match rake {
        RakeSection::None => {}
        RakeSection::PercentCap { rate, cap, .. } => {
            if !(rate.is_finite() && (0.0..=1.0).contains(rate)) {
                bail!("SLV004: rake.rate must be finite and within [0, 1]");
            }
            if !(cap.is_finite() && *cap >= 0.0) {
                bail!("SLV004: rake.cap must be finite and non-negative");
            }
        }
        RakeSection::GgPreflop { rate, cap, .. } => {
            if !(rate.is_finite() && (0.0..=1.0).contains(rate)) {
                bail!("SLV004: rake.rate must be finite and within [0, 1]");
            }
            if !(cap.is_finite() && *cap >= 0.0) {
                bail!("SLV004: rake.cap must be finite and non-negative");
            }
        }
        RakeSection::Generic {
            rate,
            cap,
            when,
            allocation,
            rounding,
            rounding_unit,
        } => {
            // Compiling here, through the same code the solve path uses,
            // is what stops a bad `when` from reaching the tree builder.
            crate::economics::GenericRake::compile(
                *rate,
                *cap,
                when,
                *allocation,
                *rounding,
                *rounding_unit,
            )
            .map_err(|error| anyhow!("SLV004: {error}"))?;
        }
    }
    match utility {
        PostflopUtility::ChipEv => {}
        PostflopUtility::Icm { payouts } => {
            for payout in payouts {
                if !payout.is_finite() || *payout < 0.0 {
                    bail!("SLV004: utility.payouts entries must be finite and non-negative");
                }
            }
            if payouts[0] < payouts[1] {
                bail!("SLV004: utility.payouts must be ordered best finish first");
            }
        }
        PostflopUtility::TournamentIcm {
            payouts,
            outside_field,
            samples,
            seed,
        } => {
            crate::economics::TournamentIcm::new(
                payouts.clone(),
                outside_field.clone(),
                *samples,
                *seed,
            )
            .map_err(|error| anyhow!("SLV004: {error}"))?;
        }
    }
    Ok(())
}

fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

/// Checks the things a type cannot: values in range, and sections whose
/// variant does not apply to this family.
fn validate_semantics(family: &Family) -> Result<()> {
    let algorithm = match family {
        Family::Toy(config) => &config.algorithm,
        Family::Postflop(config) => &config.algorithm,
        Family::PreflopHu(config) => &config.algorithm,
    };
    if matches!(algorithm, AlgorithmSection::ExternalSamplingMccfr { .. }) {
        bail!(
            "SLV003: schedule = \"external-sampling-mccfr\" is the multiway sampler; \
             these families are solved by the exact vector engine"
        );
    }
    // Only postflop grew the multiway economics models. Toy games have no
    // chip amounts to rake or to price with ICM, and the heads-up preflop
    // family still owns the equity-showdown model it was written for.
    let shared_economics = match family {
        Family::Toy(config) => Some(("toy", &config.rake, &config.utility)),
        Family::PreflopHu(config) => Some(("preflop-hu", &config.rake, &config.utility)),
        Family::Postflop(_) => None,
    };
    if let Some((label, rake, utility)) = shared_economics {
        if matches!(rake, RakeSection::Generic { .. }) {
            bail!(
                "SLV003: kind = \"generic\" rake belongs to schema = \
                 \"solvers.postflop/v1\" and Multiway Preflop, not to {label}"
            );
        }
        if matches!(utility, UtilitySection::TournamentIcm { .. }) {
            bail!(
                "SLV003: kind = \"tournament-icm\" belongs to schema = \
                 \"solvers.postflop/v1\" and Multiway Preflop, not to {label}; \
                 use kind = \"icm\" with two payouts"
            );
        }
        if label == "toy" && !matches!(rake, RakeSection::None) {
            bail!(
                "SLV003: a toy game has no chip amounts to rake; \
                 remove [rake] or set kind = \"none\""
            );
        }
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
            // The builder asserts distinct board cards. Checking it here is
            // what keeps the promise that `validate` never accepts a config
            // `solve` then rejects.
            let distinct: BTreeSet<_> = board.iter().copied().collect();
            if distinct.len() != board.len() {
                bail!("SLV004: board {:?} repeats a card", game.board);
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
            if game.min_bet == 0 {
                bail!("SLV004: min_bet must be positive");
            }
            crate::postflop_setup::parse_preflop_aggressor(&game.preflop_aggressor)
                .map_err(|error| anyhow!("SLV004: {error}"))?;
            if !matches!(game.tree.kind.as_str(), "none" | "script") {
                bail!(
                    "SLV004: unknown [game.tree] kind {:?}; the postflop family has \"none\" or \
                     \"script\"",
                    game.tree.kind
                );
            }
            if let Some(threshold) = game.tree.allin_threshold
                && !(threshold.is_finite() && threshold > 0.0 && threshold <= 1.0)
            {
                bail!("SLV004: [game.tree] allin_threshold must be finite and within (0, 1]");
            }
            // Compiling here, through the same code the tree builder uses,
            // is what stops a broken script from surviving normalization
            // and only failing once `solve` builds the tree.
            let script = game
                .tree
                .compiled_script()
                .map_err(|error| anyhow!("SLV004: {error}"))?;
            if let Some(script) = &script {
                for key in game.tree.params.keys() {
                    if !script.params.iter().any(|param| &param.name == key) {
                        bail!("SLV004: [game.tree.params] {key:?} does not name a declared param");
                    }
                }
            }
            let rules: &[Rule<PostflopVar>] = script.as_ref().map_or(&[], |script| &script.rules);
            let start = match board.len() {
                3 => Street::Flop,
                4 => Street::Turn,
                _ => Street::River,
            };
            for rule in rules {
                // A rule on a street this board already passed cannot be
                // reached, and silently dropping it would let a config
                // describe a tree it does not get. Say so instead.
                if rule.street < start {
                    bail!(
                        "SLV004: a tree-script rule targets {}, but a {}-card board starts on \
                         the {}; remove the rule or shorten the board",
                        street_name(rule.street),
                        board.len(),
                        street_name(start)
                    );
                }
            }
            validate_rule_sizes(rules)?;
            validate_postflop_economics(&config.rake, &config.utility)?;
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

fn lower_family(family: Family) -> Result<SolveConfig> {
    Ok(match family {
        Family::Toy(config) => SolveConfig {
            schema: Some(config.schema),
            game: match config.game {
                ToyGame::Kuhn => GameSection::Kuhn,
                ToyGame::Leduc => GameSection::Leduc,
            },
            rake: config.rake,
            utility: config.utility,
            algorithm: config.algorithm,
            run: config.run.lower()?,
        },
        Family::Postflop(config) => SolveConfig {
            schema: Some(config.schema),
            game: GameSection::Postflop {
                board: config.game.board,
                oop_range: config.game.oop_range,
                ip_range: config.game.ip_range,
                pot: config.game.pot,
                effective_stack: config.game.effective_stack,
                min_bet: config.game.min_bet,
                iso_merging: config.game.iso_merging,
                preflop_aggressor: config.game.preflop_aggressor,
                tree: config.game.tree,
            },
            rake: config.rake,
            utility: config.utility.lower(),
            algorithm: config.algorithm,
            run: config.run.lower()?,
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
            run: config.run.lower()?,
        },
    })
}

/// Parses a config and lowers it into the shared internal representation.
pub fn parse_and_lower(raw: &str) -> Result<SolveConfig> {
    lower_family(parse_family(raw)?)
}

/// [`parse_and_lower`], resolving a postflop `[game.tree] source` relative
/// to `config_path`'s directory.
pub fn parse_and_lower_at(raw: &str, config_path: &Path) -> Result<SolveConfig> {
    lower_family(parse_family_at(raw, Some(base_directory(config_path)))?)
}

fn base_directory(config_path: &Path) -> &Path {
    config_path.parent().unwrap_or_else(|| Path::new("."))
}

/// The config with every default written out.
///
/// Defaults live in the field declarations, so normalizing is deserialize +
/// serialize and is idempotent by construction: normalizing an effective
/// config returns the same bytes.
pub fn normalized_toml(raw: &str) -> Result<String> {
    let family = parse_family(raw)?;
    let text = toml::to_string_pretty(&family).context("serializing the effective config")?;
    crate::config::literalize_tree_script("SLV004", &text)
}

/// [`normalized_toml`], resolving a postflop `[game.tree] source` relative
/// to `config_path`'s directory.
pub fn normalized_toml_at(raw: &str, config_path: &Path) -> Result<String> {
    let family = parse_family_at(raw, Some(base_directory(config_path)))?;
    let text = toml::to_string_pretty(&family).context("serializing the effective config")?;
    crate::config::literalize_tree_script("SLV004", &text)
}

/// The same, as JSON, for `validate --format json --show-effective`.
pub fn normalized_json(raw: &str) -> Result<serde_json::Value> {
    let family = parse_family(raw)?;
    serde_json::to_value(&family).context("serializing the effective config")
}

/// [`normalized_json`], resolving a postflop `[game.tree] source` relative
/// to `config_path`'s directory.
pub fn normalized_json_at(raw: &str, config_path: &Path) -> Result<serde_json::Value> {
    let family = parse_family_at(raw, Some(base_directory(config_path)))?;
    serde_json::to_value(&family).context("serializing the effective config")
}

/// The schema a config declares, once it is known to parse.
pub fn declared(raw: &str) -> Result<String> {
    parse_family(raw)?;
    declared_schema(raw)
}

/// [`declared`], resolving a postflop `[game.tree] source` relative to
/// `config_path`'s directory.
pub fn declared_at(raw: &str, config_path: &Path) -> Result<String> {
    parse_family_at(raw, Some(base_directory(config_path)))?;
    declared_schema(raw)
}

fn game_kind_of(family: &Family) -> &'static str {
    match family {
        Family::Toy(config) => match config.game {
            ToyGame::Kuhn => "kuhn",
            ToyGame::Leduc => "leduc",
        },
        Family::Postflop(_) => "postflop",
        Family::PreflopHu(_) => "preflop",
    }
}

/// The `game.kind` a config declares, for the run manifest.
pub fn game_kind(raw: &str) -> Result<&'static str> {
    Ok(game_kind_of(&parse_family(raw)?))
}

/// [`game_kind`], resolving a postflop `[game.tree] source` relative to
/// `config_path`'s directory.
pub fn game_kind_at(raw: &str, config_path: &Path) -> Result<&'static str> {
    Ok(game_kind_of(&parse_family_at(
        raw,
        Some(base_directory(config_path)),
    )?))
}

// --- tree diagnostic (`validate` only) --------------------------------------
//
// Everything below renders a compiled tree script back to source-like JSON
// for `solvers validate`'s benefit -- see docs/solver-config-v1.jp.md's
// "param 宣言は変数スキーマである" section. It is read-only: nothing here feeds
// `normalized_toml`/`normalized_json`, which keep carrying the script body
// verbatim (`literalize_tree_script`) rather than this expanded form.

/// The `tree` diagnostic `validate --format json` reports: a postflop
/// config's compiled `param` schema and lowered `rule` list. `None` for the
/// toy and preflop-hu families (no `[game.tree]` at all) and for a postflop
/// config with `kind = "none"` (no script compiled, so nothing to report).
pub fn tree_diagnostic_at(raw: &str, config_path: &Path) -> Result<Option<serde_json::Value>> {
    let family = parse_family_at(raw, Some(base_directory(config_path)))?;
    let Family::Postflop(config) = &family else {
        return Ok(None);
    };
    let Some(script) = config.game.tree.compiled_script()? else {
        return Ok(None);
    };
    render_tree_diagnostic(&script).map(Some)
}

/// A `param` declaration, as reported by the `tree` diagnostic: the variable
/// schema a GUI would build a form from.
#[derive(Serialize)]
struct ParamDiagnostic {
    name: String,
    #[serde(rename = "type")]
    kind: &'static str,
    /// The *effective* default -- the script's own `param` value, overridden
    /// by `[game.tree.params]` where present (`ParamSchema::default` is
    /// already resolved that way; see `resolve_declarations` in
    /// `cards::script::parse`). A JSON number/bool for `number`/`bool`
    /// params, so a numeric default round-trips as JSON's own number type
    /// instead of a quoted string.
    default: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

/// One lowered rule, as reported by the `tree` diagnostic: what the tree
/// builder will actually apply, in source order, with its condition
/// rendered back to source-like text and its sizes rendered with
/// `SizeSpec::render` -- both so a reader can check this against their
/// script's `if`/`else` chain directly.
#[derive(Serialize)]
struct RuleDiagnostic {
    street: &'static str,
    condition: String,
    effect: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    action: Option<&'static str>,
    sizes: Vec<String>,
}

#[derive(Serialize)]
struct TreeDiagnostic {
    params: Vec<ParamDiagnostic>,
    rules: Vec<RuleDiagnostic>,
}

fn render_tree_diagnostic(script: &Script<PostflopVar>) -> Result<serde_json::Value> {
    let diagnostic = TreeDiagnostic {
        params: script.params.iter().map(render_param).collect(),
        rules: script.rules.iter().map(render_rule).collect(),
    };
    serde_json::to_value(diagnostic).context("serializing the tree diagnostic")
}

fn render_param(param: &ParamSchema) -> ParamDiagnostic {
    let (kind, default) = match param.kind {
        ParamKind::Number => ("number", param_number_value(&param.default)),
        ParamKind::Bool => ("bool", serde_json::Value::Bool(param.default == "true")),
        ParamKind::Token => ("token", serde_json::Value::String(param.default.clone())),
    };
    ParamDiagnostic {
        name: param.name.clone(),
        kind,
        default,
        description: param.description.clone(),
    }
}

/// Renders a `number`-kind param's default as a JSON number, preferring an
/// integer so a config that wrote a bare integer (`cb = 40`) round-trips as
/// JSON's own integer type rather than picking up a synthetic `.0`.
/// `ParamKind::Number` is only ever inferred when `default` already parses
/// as `f64` (`infer_param_kind` in `cards::script::parse`), so the `f64`
/// fallback parse never actually fails; `unwrap_or` covers it rather than
/// panicking on a diagnostic path.
fn param_number_value(default: &str) -> serde_json::Value {
    if let Ok(value) = default.parse::<i64>() {
        return serde_json::Value::from(value);
    }
    let value: f64 = default.parse().unwrap_or(0.0);
    serde_json::Number::from_f64(value)
        .map(serde_json::Value::Number)
        .unwrap_or(serde_json::Value::Null)
}

fn render_rule(rule: &Rule<PostflopVar>) -> RuleDiagnostic {
    RuleDiagnostic {
        street: street_name(rule.street),
        // `Condition<V>`'s own `Display` impl (`cards::script::cond`) is the
        // one condition-to-text renderer; multiway's `.mwtree` frontend
        // reuses the exact same impl for its own dialect.
        condition: rule.condition.to_string(),
        effect: effect_name(rule.effect),
        action: rule.action.map(action_name),
        sizes: rule
            .sizes
            .iter()
            .map(|size| size.render(SizeUnit::Chips))
            .collect(),
    }
}

/// Renders one lowered rule as the single line `crate::postflop_setup`'s
/// dead-rule warning names it with -- effect, action, and sizes exactly as
/// the `tree` diagnostic already renders them (via [`render_rule`], so
/// there is exactly one condition/size renderer, not two), plus the
/// condition itself: a reader has to be able to match this against their
/// own script's `when`/`if` chain, so the condition is the whole value of
/// the message.
pub(crate) fn render_rule_line(rule: &Rule<PostflopVar>) -> String {
    let diagnostic = render_rule(rule);
    let body = match diagnostic.action {
        Some(action) => format!(
            "{} {action} [{}]",
            diagnostic.effect,
            diagnostic.sizes.join(", ")
        ),
        // `Effect::Checkdown` carries neither action nor sizes.
        None => diagnostic.effect.to_string(),
    };
    format!("{body}  when {}", diagnostic.condition)
}

fn action_name(action: ActionKind) -> &'static str {
    match action {
        ActionKind::Bet => "bet",
        ActionKind::Raise => "raise",
        // See `validate_rule_sizes`: postflop's dialect never produces these.
        ActionKind::Fold | ActionKind::Check | ActionKind::Call => {
            unreachable!("postflop's dialect only accepts bet/raise actions")
        }
    }
}

fn effect_name(effect: Effect) -> &'static str {
    match effect {
        Effect::Add => "add",
        Effect::Remove => "remove",
        Effect::Replace => "replace",
        Effect::Force => "force",
        Effect::Checkdown => "checkdown",
    }
}

// A rule's `Condition<PostflopVar>` renders back to source-like text via its
// own `Display` impl in `cards::script::cond` -- see `render_rule` above.
// That impl is shared with multiway's `.mwtree` frontend, which has no other
// source text for a rule whose condition is a nested `when`/`if`
// composition.

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

[game.tree]
kind = "script"
script = '''
river { replace bet [50] }
'''

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
    fn the_multiway_sampler_is_refused() {
        let sampled = TOY.replace(
            "[run]",
            "[algorithm]\nschedule = \"external-sampling-mccfr\"\n\n[run]",
        );
        let error = parse_and_lower(&sampled).unwrap_err().to_string();
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

    /// The budget was mandatory for one release because it used to default
    /// to zero, which solved nothing and said so only by finishing
    /// instantly. It is optional again now that the default is a real
    /// safety budget rather than "do no work".
    #[test]
    fn an_omitted_budget_is_the_safety_budget_and_never_zero() {
        let without = TOY.replace("iterations = 10", "");
        let config = parse_and_lower(&without).expect("iterations is optional");
        assert_eq!(config.run.iterations, default_iterations());
        assert!(config.run.iterations > 0, "an omitted budget must solve");
    }

    /// The three renamed keys are errors that say what to write instead:
    /// a retired key must never be silently reinterpreted, and "unknown
    /// field `bets`" would not tell an author where the section went.
    #[test]
    fn the_retired_bets_section_names_its_replacement() {
        let legacy = r#"
schema = "solvers.postflop/v1"

[game]
board = "2c 7d 9h Js Qs"
oop_range = "22+"
ip_range = "22+"
pot = 20
effective_stack = 80

[game.bets.river]
oop = [0.5]
ip = [0.5]
max_raises = 2

[run]
iterations = 100
"#;
        let error = parse_and_lower(legacy).unwrap_err().to_string();
        assert!(error.contains("SLV002"), "{error}");
        assert!(error.contains("[game.tree]"), "{error}");
        assert!(error.contains("oop_bet"), "{error}");
        assert!(error.contains("ip_bet"), "{error}");
        assert!(error.contains("max_aggressive_actions"), "{error}");
    }

    /// The same menu keys with their `[game.tree.<street>]` headers deleted
    /// -- the shape an author lands on halfway through migrating -- must
    /// still name where each key went, not just list the valid fields the
    /// way `deny_unknown_fields` would.
    #[test]
    fn retired_menu_keys_directly_under_the_tree_table_name_their_replacement() {
        let halfway = r#"
schema = "solvers.postflop/v1"

[game]
board = "2c 7d 9h Js Qs"
oop_range = "22+"
ip_range = "22+"
pot = 20
effective_stack = 80

[game.tree]
kind = "script"
oop_bet = [50]
oop_donk = []

[run]
iterations = 100
"#;
        let error = parse_and_lower(halfway).unwrap_err().to_string();
        assert!(error.contains("SLV002"), "{error}");
        assert!(error.contains("oop_bet"), "{error}");
        assert!(error.contains("oop_donk"), "{error}");
        assert!(error.contains("replace bet"), "{error}");
        assert!(error.contains("when donk"), "{error}");
    }

    /// Every input that used to pass `validate` and then panic the builder.
    #[test]
    fn validate_rejects_what_the_builder_used_to_panic_on() {
        // An odd pot is now legal, so it is the control case.
        let odd = POSTFLOP.replace("pot = 20", "pot = 21");
        parse_and_lower(&odd).expect("an odd pot is legal");

        let repeated = POSTFLOP.replace("2c 7d 9h Js Qs", "2c 2c 9h Js Qs");
        let error = parse_and_lower(&repeated).unwrap_err().to_string();
        assert!(error.contains("SLV004"), "{error}");
        assert!(error.contains("repeats a card"), "{error}");

        let bad_when = POSTFLOP.replace(
            "[run]",
            "[rake]\nkind = \"generic\"\nrate = 0.05\nwhen = \"not a condition\"\n\n[run]",
        );
        let error = parse_and_lower(&bad_when).unwrap_err().to_string();
        assert!(error.contains("SLV004"), "{error}");

        let negative = POSTFLOP.replace("[50]", "[-1]");
        let error = parse_and_lower(&negative).unwrap_err().to_string();
        assert!(error.contains("positive"), "{error}");

        let empty_range = POSTFLOP.replace("oop_range = \"22+\"", "oop_range = \"\"");
        let error = parse_and_lower(&empty_range).unwrap_err().to_string();
        assert!(error.contains("SLV004"), "{error}");
    }

    /// A rule targeting a street the board has already passed describes a
    /// tree the config will not get, so it is an error rather than a silent
    /// no-op.
    #[test]
    fn a_menu_on_an_unreachable_street_is_rejected() {
        let unreachable = POSTFLOP.replace(
            "river { replace bet [50] }",
            "flop { replace bet [50] }\nriver { replace bet [50] }",
        );
        let error = parse_and_lower(&unreachable).unwrap_err().to_string();
        assert!(error.contains("SLV004"), "{error}");
        assert!(error.contains("flop"), "{error}");
        assert!(error.contains("river"), "{error}");
    }

    #[test]
    fn size_literals_and_bare_fractions_mean_the_same_tree() {
        let literal = POSTFLOP.replace("[50]", "[50%pot]");
        let from_fraction = parse_and_lower(POSTFLOP).unwrap();
        let from_literal = parse_and_lower(&literal).unwrap();
        let GameSection::Postflop { tree: fraction, .. } = from_fraction.game else {
            panic!("expected GameSection::Postflop");
        };
        let GameSection::Postflop { tree: literal, .. } = from_literal.game else {
            panic!("expected GameSection::Postflop");
        };
        assert_eq!(
            format!("{:?}", fraction.lower().unwrap()),
            format!("{:?}", literal.lower().unwrap())
        );

        // The bb literal belongs to the family that has blinds.
        let bb = POSTFLOP.replace("[50]", "[2.5bb]");
        let error = parse_and_lower(&bb).unwrap_err().to_string();
        assert!(error.contains("bb"), "{error}");
    }

    /// The budget stopped being mandatory, but it must not go back to
    /// meaning zero: an omitted `iterations` has to solve something.
    #[test]
    fn an_omitted_iteration_budget_falls_back_to_the_safety_budget() {
        let without = POSTFLOP.replace("iterations = 100", "target_nash_conv = 0.01");
        let config = parse_and_lower(&without).unwrap();
        assert_eq!(config.run.iterations, default_iterations());
        assert!(config.run.iterations > 0);
    }

    #[test]
    fn max_time_lowers_to_seconds_and_rejects_bad_durations() {
        let timed = POSTFLOP.replace("iterations = 100", "max_time = \"15m\"");
        assert_eq!(
            parse_and_lower(&timed).unwrap().run.max_time_secs,
            Some(900)
        );
        for bad in ["\"0s\"", "\"15\"", "\"1d\"", "\"-3m\""] {
            let broken = POSTFLOP.replace("iterations = 100", &format!("max_time = {bad}"));
            let error = parse_and_lower(&broken).unwrap_err().to_string();
            assert!(error.contains("SLV004"), "{bad}: {error}");
        }
    }

    /// The two economics models postflop borrowed from Multiway Preflop are
    /// accepted here and refused by the families that cannot use them.
    #[test]
    fn the_multiway_economics_models_are_postflop_only() {
        let raked = POSTFLOP.replace(
            "[run]",
            "[rake]\nkind = \"generic\"\nrate = 0.05\ncap = 4.0\n\n[run]",
        );
        let config = parse_and_lower(&raked).unwrap();
        assert!(matches!(config.rake, RakeSection::Generic { .. }));

        let icm = POSTFLOP.replace(
            "[run]",
            "[utility]\nkind = \"tournament-icm\"\npayouts = [1000.0, 600.0]\n\
             outside_field = [1800.0]\n\n[run]",
        );
        let config = parse_and_lower(&icm).unwrap();
        let UtilitySection::TournamentIcm { outside_field, .. } = &config.utility else {
            panic!("expected tournament ICM");
        };
        assert_eq!(outside_field.len(), 1);

        let borrowed = TOY.replace(
            "[run]",
            "[utility]\nkind = \"tournament-icm\"\npayouts = [1.0, 0.0]\n\n[run]",
        );
        let error = parse_and_lower(&borrowed).unwrap_err().to_string();
        assert!(error.contains("SLV003"), "{error}");
    }

    #[test]
    fn the_declared_kind_is_reported_for_the_manifest() {
        assert_eq!(game_kind(TOY).unwrap(), "kuhn");
        assert_eq!(game_kind(POSTFLOP).unwrap(), "postflop");
    }
}
