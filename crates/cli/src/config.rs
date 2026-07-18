use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// One experiment = one TOML file. Unknown fields are rejected so typos
/// fail loudly instead of silently running a different experiment.
#[derive(Deserialize, Serialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct SolveConfig {
    pub game: GameSection,
    #[serde(default)]
    pub rake: RakeSection,
    #[serde(default)]
    pub utility: UtilitySection,
    #[serde(default)]
    pub algorithm: AlgorithmSection,
    pub run: RunSection,
}

#[derive(Deserialize, Serialize, Debug)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "kebab-case")]
// This is a config struct deserialized once per run and then destructured
// away; the size difference between variants never sits on a hot path, so
// boxing `bets` to appease the lint would just add indirection for nothing.
#[allow(clippy::large_enum_variant)]
pub enum GameSection {
    Kuhn,
    Leduc,
    Postflop {
        /// Whitespace-separated cards, e.g. "Ks 7h 2d" (flop), "...  Js"
        /// (turn), or "... Tc" (river). Board length picks the starting
        /// street.
        board: String,
        /// Out-of-position player's range (acts first postflop), e.g.
        /// "22+,A2s+,AA:0.5".
        oop_range: String,
        /// In-position player's range.
        ip_range: String,
        pot: u32,
        effective_stack: u32,
        #[serde(default = "default_true")]
        iso_merging: bool,
        bets: BetsSection,
    },
    Preflop {
        /// Per-player starting stack, in big blinds.
        effective_stack_bb: f64,
        /// Small blind size, in big blinds (SB = `Player::P0`).
        #[serde(default = "default_sb_bb")]
        sb_bb: f64,
        /// SB's (or BB's iso-raise) first-raise raise-to sizes, in big
        /// blinds.
        #[serde(default = "default_open_sizes_bb")]
        open_sizes_bb: Vec<f64>,
        /// Reraise-to factors per raise level (see `preflop::PreflopConfig`
        /// docs); an empty outer list means sized reraises are never offered
        /// (all-in only).
        #[serde(default = "default_raise_factors")]
        raise_factors: Vec<Vec<f64>>,
        #[serde(default = "default_preflop_max_raises")]
        max_raises: u32,
        #[serde(default = "default_true")]
        include_allin: bool,
        #[serde(default = "default_true")]
        allow_limp: bool,
        /// SB's range spec (e.g. "22+,A2s+"); `None` is the full range.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sb_range: Option<String>,
        /// BB's range spec; `None` is the full range.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bb_range: Option<String>,
        /// Per-player equity-realization factors for non-all-in
        /// continuations (see `preflop::EquityShowdown`).
        #[serde(default = "default_equity_realization")]
        equity_realization: [f64; 2],
        /// Disk cache path for the exact 169x169 equity table; `None`
        /// recomputes it in memory every run.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        equity_cache: Option<PathBuf>,
        /// Optional bucketed blueprint postflop model. Absence of the whole
        /// section keeps today's behavior: the 169-class trunk's
        /// continuations resolve via `preflop::EquityShowdown`, with no
        /// postflop betting tree at all.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        postflop: Option<PostflopSection>,
    },
    PreflopMultiway(multiway::MultiwayConfig),
}

/// `[game.postflop]`: extends the preflop trunk with a bucketed blueprint
/// postflop model (`abstraction::Ehs2Abstraction` + `BlueprintArtifacts`,
/// wired through `preflop::build_blueprint_game`).
///
/// Bucket-count defaults (50/20/8) are deliberately coarser on later
/// streets: the deliverable of this model is preflop ranges, so turn/river
/// fidelity is traded for tree storage and artifact build time.
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct PostflopSection {
    /// Postflop model kind. Validated at solve time (not parse time) so the
    /// error message can name the one supported value; only `"bucketed"` is
    /// implemented.
    pub model: String,
    #[serde(default = "default_flop_buckets")]
    pub flop_buckets: u32,
    #[serde(default = "default_turn_buckets")]
    pub turn_buckets: u32,
    #[serde(default = "default_river_buckets")]
    pub river_buckets: u32,
    /// Pot-fraction bet/raise sizes on the flop, same for both players.
    #[serde(default)]
    pub bets_flop: Vec<f64>,
    /// Pot-fraction bet/raise sizes on the turn, same for both players.
    #[serde(default)]
    pub bets_turn: Vec<f64>,
    /// Pot-fraction bet/raise sizes on the river, same for both players.
    #[serde(default)]
    pub bets_river: Vec<f64>,
    #[serde(default = "default_postflop_max_raises")]
    pub max_raises: u32,
    #[serde(default = "default_true")]
    pub include_allin: bool,
    /// Disk cache path for the EHS² bucket abstraction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abstraction_cache: Option<PathBuf>,
    /// Disk cache path for the derived blueprint artifacts (T1/T2/T3
    /// transitions plus river bucket-vs-bucket equity).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifacts_cache: Option<PathBuf>,
}

fn default_flop_buckets() -> u32 {
    50
}
fn default_turn_buckets() -> u32 {
    20
}
fn default_river_buckets() -> u32 {
    8
}
fn default_postflop_max_raises() -> u32 {
    2
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct BetsSection {
    #[serde(default)]
    pub flop: StreetBets,
    #[serde(default)]
    pub turn: StreetBets,
    #[serde(default)]
    pub river: StreetBets,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct StreetBets {
    /// Out-of-position bet sizes (no outstanding bet to face), as fractions
    /// of the pot after a call.
    #[serde(default)]
    pub oop: Vec<f64>,
    /// In-position bet sizes (no outstanding bet to face), as fractions of
    /// the pot after a call.
    #[serde(default)]
    pub ip: Vec<f64>,
    /// Out-of-position raise sizes when facing a bet. Falls back to `oop`
    /// when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oop_raise: Option<Vec<f64>>,
    /// In-position raise sizes when facing a bet. Falls back to `ip` when
    /// omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip_raise: Option<Vec<f64>>,
    #[serde(default = "default_max_raises")]
    pub max_raises: u32,
}

impl Default for StreetBets {
    fn default() -> Self {
        StreetBets {
            oop: Vec::new(),
            ip: Vec::new(),
            oop_raise: None,
            ip_raise: None,
            max_raises: default_max_raises(),
        }
    }
}

fn default_max_raises() -> u32 {
    2
}

#[derive(Deserialize, Serialize, Debug, Default)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "kebab-case")]
pub enum RakeSection {
    #[default]
    None,
    PercentCap {
        rate: f64,
        cap: f64,
        #[serde(default)]
        no_flop_no_drop: bool,
    },
    GgPreflop {
        rate: f64,
        cap: f64,
        exempt_pot: u32,
    },
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct OutsidePlayerSection {
    pub name: String,
    pub stack_bb: f64,
}

#[derive(Deserialize, Serialize, Debug, Default)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "kebab-case")]
pub enum UtilitySection {
    #[default]
    ChipEv,
    Icm {
        payouts: [f64; 2],
    },
    TournamentIcm {
        #[serde(default)]
        outside_field: Vec<OutsidePlayerSection>,
        payouts: Vec<f64>,
        #[serde(default = "default_icm_samples")]
        samples: u64,
        #[serde(default)]
        seed: u64,
    },
}

#[derive(Deserialize, Serialize, Debug)]
#[serde(deny_unknown_fields, tag = "schedule", rename_all = "kebab-case")]
pub enum AlgorithmSection {
    Vanilla,
    CfrPlus,
    Dcfr {
        #[serde(default = "default_alpha")]
        alpha: f64,
        #[serde(default)]
        beta: f64,
        #[serde(default = "default_gamma")]
        gamma: f64,
        #[serde(default = "default_true")]
        pow4_reset: bool,
    },
    LinearCfr,
    HsDcfr {
        #[serde(default = "default_gamma0")]
        gamma0: f64,
    },
    ExternalSamplingMccfr {
        #[serde(default)]
        seed: u64,
        #[serde(default = "default_exploration_epsilon")]
        exploration_epsilon: f64,
        #[serde(default = "default_discount_every")]
        discount_every: u64,
        #[serde(default = "default_discount_until")]
        discount_until: u64,
        /// Enables "vector-traverser" external sampling (see
        /// `multiway::solver::SolverConfig::traverser_vector`): one
        /// traversal updates every feasible hole combo of the sampled
        /// traverser seat at once, instead of only the one combo the deal
        /// sampler dealt it. Only valid with `game.abstraction.recall =
        /// "street"`. `false` (the default) is the original algorithm,
        /// byte-identical to before this field existed.
        #[serde(default, skip_serializing_if = "is_false")]
        traverser_vector: bool,
        /// Enables Pluribus-style regret-based pruning (see
        /// `multiway::solver::SolverConfig::prune`): in vector-traverser
        /// mode, zero-probability actions whose regret sits far below
        /// `prune_threshold` are skipped (with probability
        /// `prune_skip_probability`) rather than descended into. `false`
        /// (the default) is the original algorithm, byte-identical to
        /// before this field existed. Requires `traverser_vector = true`.
        #[serde(default, skip_serializing_if = "is_false")]
        prune: bool,
        /// Regret threshold below which a zero-probability action becomes
        /// prunable; must be finite and strictly negative. `None` (the
        /// default) derives it from the game's stakes at solve time when
        /// `prune` is enabled; ignored otherwise. See
        /// `crate::session::build_multiway_session` for the derivation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prune_threshold: Option<f64>,
        /// Probability that a prunable action is actually skipped on a
        /// given traversal (see
        /// `multiway::solver::SolverConfig::prune_skip_probability`). Omitted
        /// from serialized output at its default (the GUI never exposes this
        /// knob, so its exported configs always hit this default).
        #[serde(
            default = "default_prune_skip_probability",
            skip_serializing_if = "is_default_prune_skip_probability"
        )]
        prune_skip_probability: f64,
    },
}

impl Default for AlgorithmSection {
    fn default() -> Self {
        AlgorithmSection::Dcfr {
            alpha: default_alpha(),
            beta: 0.0,
            gamma: default_gamma(),
            pow4_reset: true,
        }
    }
}

fn default_alpha() -> f64 {
    1.5
}
fn default_gamma() -> f64 {
    3.0
}
fn default_exploration_epsilon() -> f64 {
    0.06
}
fn default_discount_every() -> u64 {
    100_000
}
fn default_discount_until() -> u64 {
    10_000_000
}
fn default_icm_samples() -> u64 {
    100_000
}
fn default_prune_skip_probability() -> f64 {
    multiway::solver::DEFAULT_PRUNE_SKIP_PROBABILITY
}
fn is_default_prune_skip_probability(value: &f64) -> bool {
    *value == default_prune_skip_probability()
}
/// `pub(crate)` (rather than private) so `bench` can build an `HsDcfr`
/// section with the same default `gamma0` the config schema would use.
pub(crate) fn default_gamma0() -> f64 {
    30.0
}
fn default_true() -> bool {
    true
}

fn is_false(value: &bool) -> bool {
    !*value
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

#[derive(Deserialize, Serialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct RunSection {
    /// HU iteration budget. Multiway configs may omit this and set
    /// `sweeps`; one sweep traverses once for every table seat.
    #[serde(default)]
    pub iterations: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sweeps: Option<u64>,
    /// Multiway root seed; overrides the algorithm seed when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// Exploitability check cadence, in iterations.
    #[serde(default = "default_check_every")]
    pub check_every: u64,
    /// Storage backend for regrets/strategy sums. `f32` is the plain
    /// backend; `i16` is the quantized backend (see `engine::I16Storage`),
    /// trading precision for ~4x less memory on large postflop trees.
    #[serde(default)]
    pub storage: StorageKind,
    /// Stop once NashConv (sum of per-player exploitabilities, in chips per
    /// deal) drops below this.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_nash_conv: Option<f64>,
    /// Worker count for deterministic parallel solve batches. Multiway uses
    /// one local traversal delta per seat and merges in sample-id order;
    /// defaults to rayon's available worker count when unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threads: Option<usize>,
    /// Overrides `ParConfig::chance_depth` (postflop only; default 2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub par_chance_depth: Option<u32>,
    /// Overrides `ParConfig::min_children` (postflop only; default 12).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub par_min_children: Option<usize>,
    /// Hard cap for lazily visited multiway policy storage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_memory_bytes: Option<u64>,
    /// Multiway checkpoint cadence in completed sweeps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_every: Option<u64>,
    /// Held-out worlds used for multiway profile evaluation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluation_samples: Option<u64>,
    /// Evaluation cadence in completed multiway sweeps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluation_cadence: Option<u64>,
    /// Complete sweeps run per parallel drive iteration against the same
    /// strategy snapshot (see `multiway::solver::SolverConfig::sweep_batch`).
    /// `None` (the default) is `1`, bit-identical to a solver built before
    /// this knob existed. Values above `1` trade slightly staler
    /// within-batch updates for restored parallel efficiency on tables whose
    /// per-seat traversal cost is imbalanced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sweep_batch: Option<u64>,
    /// Convergence-based stop threshold, in the run's own utility unit (bb
    /// for `[utility] kind = "chip-ev"`; tournament-utility units, compared
    /// as-is, for `kind = "tournament-icm"`). When set, `run.sweeps` becomes
    /// a *safety cap* rather than a target: the drive loop additionally
    /// evaluates the held-out average profile every
    /// `stop_eval_period_secs` of wall time and stops early, with completion
    /// status `"converged"`, once the maximum per-seat
    /// `deviation_gain_lower_bound` upper confidence bound stays below this
    /// threshold for `stop_confirmations` consecutive evaluations in a row.
    /// `None` (the default) never runs this check, so `run.sweeps` behaves
    /// exactly as before this field existed.
    ///
    /// Determinism note: this check fires on a *wall-clock* period, so the
    /// exact sweep count a converged run stops at is machine-dependent (a
    /// faster machine fits more sweeps into the same `stop_eval_period_secs`
    /// window before the first check, and every check thereafter). The
    /// stopped sweep count is always recorded in the run's artifacts. A
    /// bit-reproducible run must use a fixed `run.sweeps` with
    /// `stop_dev_gain` left unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_dev_gain: Option<f64>,
    /// Consecutive passing stop-rule evaluations required before stopping.
    /// Defaults to `2` when `stop_dev_gain` is set; ignored otherwise. Must
    /// be positive when supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_confirmations: Option<u32>,
    /// Wall-clock period, in seconds, between stop-rule evaluations.
    /// Defaults to `30.0` when `stop_dev_gain` is set; ignored otherwise.
    /// Must be positive when supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_eval_period_secs: Option<f64>,
}

fn default_check_every() -> u64 {
    25
}

/// Which `engine::Storage` backend a solve uses. Chosen once from the
/// config (`[run] storage = "f32" | "i16"`) and threaded through as a
/// generic parameter, so the solve path never pays for a `dyn` indirection
/// on the hot per-hand loop just to support both backends.
#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum StorageKind {
    #[default]
    F32,
    I16,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `SolveConfig` (and all its section types) must round-trip through
    /// `toml::to_string`, so a GUI can load a config, edit it, and export it
    /// as a preset using the exact same schema the CLI parses (see
    /// docs/native-gui-plan.md, section C/E).
    #[test]
    fn multiway_config_round_trips_through_toml_serialization() {
        let raw = include_str!("../../../examples/preflop_multiway_9max.toml");
        let original: SolveConfig = toml::from_str(raw).expect("parse original multiway config");
        let serialized = toml::to_string(&original).expect("serialize SolveConfig back to TOML");
        let reparsed: SolveConfig =
            toml::from_str(&serialized).expect("re-parse the serialized config");

        let SolveConfig {
            game,
            rake,
            utility,
            run,
            ..
        } = reparsed;
        let GameSection::PreflopMultiway(game_config) = game else {
            panic!("expected GameSection::PreflopMultiway after round-tripping");
        };
        let utility = crate::session::convert_utility(utility)
            .expect("re-parsed utility section converts cleanly");
        let rake = crate::session::convert_rake(rake);
        game_config
            .validate_economics(&utility, &rake)
            .expect("re-parsed multiway config must still validate");
        assert!(run.sweeps.is_some() || run.iterations > 0);
    }

    #[test]
    fn traverser_vector_defaults_to_false_and_is_omitted_when_unset() {
        let raw = include_str!("../../../examples/preflop_multiway_9max.toml");
        let mut config: SolveConfig = toml::from_str(raw).expect("parse example multiway config");
        let AlgorithmSection::ExternalSamplingMccfr {
            traverser_vector, ..
        } = &config.algorithm
        else {
            panic!("expected schedule = \"external-sampling-mccfr\"");
        };
        assert!(!traverser_vector);
        let serialized = toml::to_string(&config).unwrap();
        assert!(!serialized.contains("traverser_vector"));

        let AlgorithmSection::ExternalSamplingMccfr {
            traverser_vector, ..
        } = &mut config.algorithm
        else {
            unreachable!("checked above");
        };
        *traverser_vector = true;
        let serialized = toml::to_string(&config).unwrap();
        assert!(serialized.contains("traverser_vector = true"));
        let reparsed: SolveConfig =
            toml::from_str(&serialized).expect("re-parse the serialized config");
        let AlgorithmSection::ExternalSamplingMccfr {
            traverser_vector, ..
        } = reparsed.algorithm
        else {
            panic!("expected schedule = \"external-sampling-mccfr\"");
        };
        assert!(traverser_vector);
    }

    #[test]
    fn preflop_config_minimal_applies_defaults() {
        let raw = r#"
[game]
kind = "preflop"
effective_stack_bb = 100.0

[run]
iterations = 10
"#;
        let config: SolveConfig = toml::from_str(raw).expect("parse minimal preflop config");
        match config.game {
            GameSection::Preflop {
                effective_stack_bb,
                sb_bb,
                open_sizes_bb,
                raise_factors,
                max_raises,
                include_allin,
                allow_limp,
                sb_range,
                bb_range,
                equity_realization,
                equity_cache,
                postflop,
            } => {
                assert_eq!(effective_stack_bb, 100.0);
                assert_eq!(sb_bb, 0.5);
                assert_eq!(open_sizes_bb, vec![2.5]);
                assert_eq!(raise_factors, vec![vec![3.0]]);
                assert_eq!(max_raises, 4);
                assert!(include_allin);
                assert!(allow_limp);
                assert_eq!(sb_range, None);
                assert_eq!(bb_range, None);
                assert_eq!(equity_realization, [1.0, 1.0]);
                assert_eq!(equity_cache, None);
                assert!(postflop.is_none());
            }
            other => panic!("expected GameSection::Preflop, got {other:?}"),
        }
    }

    #[test]
    fn preflop_config_fully_specified_overrides_every_default() {
        let raw = r#"
[game]
kind = "preflop"
effective_stack_bb = 10.0
sb_bb = 0.5
open_sizes_bb = []
raise_factors = []
max_raises = 1
include_allin = true
allow_limp = false
sb_range = "22+,A2s+"
bb_range = "QQ+"
equity_realization = [0.9, 1.1]
equity_cache = ".cache/preflop_equity.bin"

[run]
iterations = 10
"#;
        let config: SolveConfig =
            toml::from_str(raw).expect("parse fully-specified preflop config");
        match config.game {
            GameSection::Preflop {
                effective_stack_bb,
                sb_bb,
                open_sizes_bb,
                raise_factors,
                max_raises,
                include_allin,
                allow_limp,
                sb_range,
                bb_range,
                equity_realization,
                equity_cache,
                postflop,
            } => {
                assert_eq!(effective_stack_bb, 10.0);
                assert_eq!(sb_bb, 0.5);
                assert_eq!(open_sizes_bb, Vec::<f64>::new());
                assert_eq!(raise_factors, Vec::<Vec<f64>>::new());
                assert_eq!(max_raises, 1);
                assert!(include_allin);
                assert!(!allow_limp);
                assert_eq!(sb_range.as_deref(), Some("22+,A2s+"));
                assert_eq!(bb_range.as_deref(), Some("QQ+"));
                assert_eq!(equity_realization, [0.9, 1.1]);
                assert_eq!(
                    equity_cache,
                    Some(PathBuf::from(".cache/preflop_equity.bin"))
                );
                assert!(postflop.is_none());
            }
            other => panic!("expected GameSection::Preflop, got {other:?}"),
        }
    }

    #[test]
    fn preflop_config_rejects_unknown_field() {
        let raw = r#"
[game]
kind = "preflop"
effective_stack_bb = 100.0
typo_field = 1

[run]
iterations = 10
"#;
        let result: Result<SolveConfig, _> = toml::from_str(raw);
        assert!(
            result.is_err(),
            "an unknown field in a preflop config must fail to parse"
        );
    }

    // --- [game.postflop] (bucketed blueprint model) -------------------------

    #[test]
    fn postflop_section_absent_by_default() {
        let raw = r#"
[game]
kind = "preflop"
effective_stack_bb = 100.0

[run]
iterations = 10
"#;
        let config: SolveConfig =
            toml::from_str(raw).expect("parse preflop config without [game.postflop]");
        match config.game {
            GameSection::Preflop { postflop, .. } => {
                assert!(postflop.is_none());
            }
            other => panic!("expected GameSection::Preflop, got {other:?}"),
        }
    }

    #[test]
    fn postflop_section_parses_fully_specified() {
        let raw = r#"
[game]
kind = "preflop"
effective_stack_bb = 100.0

[game.postflop]
model = "bucketed"
flop-buckets = 40
turn-buckets = 15
river-buckets = 6
bets-flop = [0.5]
bets-turn = [0.75]
bets-river = [0.75, 1.0]
max-raises = 3
include-allin = false
abstraction-cache = ".cache/ehs2.bin"
artifacts-cache = ".cache/blueprint.bin"

[run]
iterations = 10
"#;
        let config: SolveConfig =
            toml::from_str(raw).expect("parse fully-specified [game.postflop]");
        match config.game {
            GameSection::Preflop { postflop, .. } => {
                let section = postflop.expect("postflop section must be present");
                assert_eq!(section.model, "bucketed");
                assert_eq!(section.flop_buckets, 40);
                assert_eq!(section.turn_buckets, 15);
                assert_eq!(section.river_buckets, 6);
                assert_eq!(section.bets_flop, vec![0.5]);
                assert_eq!(section.bets_turn, vec![0.75]);
                assert_eq!(section.bets_river, vec![0.75, 1.0]);
                assert_eq!(section.max_raises, 3);
                assert!(!section.include_allin);
                assert_eq!(
                    section.abstraction_cache,
                    Some(PathBuf::from(".cache/ehs2.bin"))
                );
                assert_eq!(
                    section.artifacts_cache,
                    Some(PathBuf::from(".cache/blueprint.bin"))
                );
            }
            other => panic!("expected GameSection::Preflop, got {other:?}"),
        }
    }

    #[test]
    fn postflop_section_defaults() {
        let raw = r#"
[game]
kind = "preflop"
effective_stack_bb = 100.0

[game.postflop]
model = "bucketed"

[run]
iterations = 10
"#;
        let config: SolveConfig = toml::from_str(raw).expect("parse minimal [game.postflop]");
        match config.game {
            GameSection::Preflop { postflop, .. } => {
                let section = postflop.expect("postflop section must be present");
                assert_eq!(section.model, "bucketed");
                assert_eq!(section.flop_buckets, 50);
                assert_eq!(section.turn_buckets, 20);
                assert_eq!(section.river_buckets, 8);
                assert!(section.bets_flop.is_empty());
                assert!(section.bets_turn.is_empty());
                assert!(section.bets_river.is_empty());
                assert_eq!(section.max_raises, 2);
                assert!(section.include_allin);
                assert_eq!(section.abstraction_cache, None);
                assert_eq!(section.artifacts_cache, None);
            }
            other => panic!("expected GameSection::Preflop, got {other:?}"),
        }
    }

    #[test]
    fn postflop_section_rejects_unknown_field() {
        let raw = r#"
[game]
kind = "preflop"
effective_stack_bb = 100.0

[game.postflop]
model = "bucketed"
typo_field = 1

[run]
iterations = 10
"#;
        let result: Result<SolveConfig, _> = toml::from_str(raw);
        assert!(
            result.is_err(),
            "an unknown field in [game.postflop] must fail to parse"
        );
    }

    fn parse_bets(toml: &str) -> BetsSection {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wrapper {
            bets: BetsSection,
        }
        toml::from_str::<Wrapper>(toml).unwrap().bets
    }

    #[test]
    fn oop_raise_and_ip_raise_parse() {
        let bets = parse_bets(
            r#"
            [bets.flop]
            oop = [0.5]
            ip = [0.5]
            oop_raise = [1.0]
            ip_raise = [0.75]
            "#,
        );
        assert_eq!(bets.flop.oop_raise, Some(vec![1.0]));
        assert_eq!(bets.flop.ip_raise, Some(vec![0.75]));
    }

    #[test]
    fn stop_rule_keys_round_trip_and_absent_keys_serialize_byte_identically() {
        let raw = include_str!("../../../examples/preflop_multiway_3max_smoke.toml");
        let baseline: SolveConfig = toml::from_str(raw).expect("parse example multiway config");
        assert_eq!(baseline.run.stop_dev_gain, None);
        assert_eq!(baseline.run.stop_confirmations, None);
        assert_eq!(baseline.run.stop_eval_period_secs, None);
        let serialized = toml::to_string(&baseline).unwrap();
        assert!(!serialized.contains("stop_dev_gain"));
        assert!(!serialized.contains("stop_confirmations"));
        assert!(!serialized.contains("stop_eval_period_secs"));

        // Splice the three new keys into the example's existing `[run]`
        // table, right after its last key.
        let anchor = "evaluation_cadence = 1\n";
        let spliced = raw.replacen(
            anchor,
            &format!(
                "{anchor}stop_dev_gain = 0.05\nstop_confirmations = 3\nstop_eval_period_secs = 5.0\n"
            ),
            1,
        );
        assert_ne!(spliced, raw, "the splice anchor must have matched");

        let config: SolveConfig = toml::from_str(&spliced).expect("parse config with stop rule");
        assert_eq!(config.run.stop_dev_gain, Some(0.05));
        assert_eq!(config.run.stop_confirmations, Some(3));
        assert_eq!(config.run.stop_eval_period_secs, Some(5.0));

        let reserialized = toml::to_string(&config).unwrap();
        assert!(reserialized.contains("stop_dev_gain = 0.05"));
        assert!(reserialized.contains("stop_confirmations = 3"));
        assert!(reserialized.contains("stop_eval_period_secs = 5.0"));
        let reparsed: SolveConfig = toml::from_str(&reserialized).unwrap();
        assert_eq!(reparsed.run.stop_dev_gain, Some(0.05));
        assert_eq!(reparsed.run.stop_confirmations, Some(3));
        assert_eq!(reparsed.run.stop_eval_period_secs, Some(5.0));
    }

    #[test]
    fn omitted_oop_raise_and_ip_raise_default_to_none() {
        let bets = parse_bets(
            r#"
            [bets.flop]
            oop = [0.5]
            ip = [0.5]
            "#,
        );
        assert_eq!(bets.flop.oop_raise, None);
        assert_eq!(bets.flop.ip_raise, None);
    }
}
