//! Editable mirror of `cli::config::SolveConfig` for `kind =
//! "preflop-multiway"`, plus the bidirectional `Model <-> SolveConfig <->
//! TOML` conversions the Setup tab needs for presets, import/export, and
//! live validation (see `docs/native-gui-plan.md` section F).
//!
//! Betting sizes are held as the short-hand text from `size_lexer` rather
//! than `Vec<SizeSpec>` so a `TextEdit` can bind directly to them; they are
//! parsed lazily by [`model_to_solve_config`].

use cli::config::{
    AlgorithmSection, GameSection, OutsidePlayerSection, RakeSection, RunSection, SolveConfig,
    StorageKind, UtilitySection,
};
use multiway::SeatId;
use multiway::config::{
    AbstractionConfig, AbstractionKind, ActiveOpponentBucketConfig, AnteConfig, BettingConfig,
    BlindConfig, MultiwayConfig, RecallMode, SeatConfig, StreetBettingConfig,
};

use crate::size_lexer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnteKind {
    None,
    Each,
    BigBlind,
}

#[derive(Clone, Debug)]
pub struct AnteModel {
    pub kind: AnteKind,
    pub amount_bb: f64,
}

#[derive(Clone, Debug)]
pub struct StreetBettingModel {
    pub bet_sizes: String,
    /// `None` = inherit `bet_sizes` (preflop only); `Some(text)` overrides,
    /// including `Some(String::new())` to disable isolate sizing entirely.
    pub isolate_sizes: Option<String>,
    pub raise_sizes: String,
    pub max_aggressive_actions: u8,
    pub include_allin: bool,
    pub allin_threshold: Option<f64>,
    /// HRC-style check-down threshold; `None` = unlimited (postflop only).
    pub max_betting_players: Option<u8>,
}

#[derive(Clone, Debug)]
pub struct BettingModel {
    pub allow_limp: bool,
    pub preflop: StreetBettingModel,
    pub flop: StreetBettingModel,
    pub turn: StreetBettingModel,
    pub river: StreetBettingModel,
}

#[derive(Clone, Debug)]
pub struct SeatModel {
    pub name: String,
    pub stack_bb: f64,
    pub range: String,
    pub betting_override: Option<BettingModel>,
}

#[derive(Clone, Debug)]
pub struct ActiveOpponentBucketModel {
    pub active_opponents: u8,
    pub flop_buckets: u16,
    pub turn_buckets: u16,
    pub river_buckets: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecallKind {
    Full,
    Street,
}

/// Mirrors `multiway::config::AbstractionKind`; see
/// [`AbstractionModel::kind`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbstractionBackendKind {
    RolloutKmeans,
    Ehs2Table,
}

#[derive(Clone, Debug)]
pub struct AbstractionModel {
    pub flop_buckets: u16,
    pub turn_buckets: u16,
    pub river_buckets: u16,
    pub rollout_samples: u32,
    pub seed: u64,
    pub active_opponent_buckets: Vec<ActiveOpponentBucketModel>,
    /// Empty means "no artifact cache" (`None`).
    pub artifact_cache: String,
    pub recall: RecallKind,
    /// Selects the postflop card-abstraction backend; see
    /// `multiway::config::AbstractionConfig::kind`. `RolloutKmeans` ignores
    /// nothing new; `Ehs2Table` ignores `rollout_samples`/`seed` and
    /// rejects non-empty `active_opponent_buckets`.
    pub kind: AbstractionBackendKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UtilityKind {
    ChipEv,
    TournamentIcm,
}

#[derive(Clone, Debug)]
pub struct OutsideFieldModel {
    pub name: String,
    pub stack_bb: f64,
}

#[derive(Clone, Debug)]
pub struct UtilityModel {
    pub kind: UtilityKind,
    pub outside_field: Vec<OutsideFieldModel>,
    /// Comma- or newline-separated payouts, best finish first.
    pub payouts_text: String,
    pub samples: u64,
    pub seed: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RakeKind {
    None,
    PercentCap,
    GgPreflop,
}

#[derive(Clone, Debug)]
pub struct RakeModel {
    pub kind: RakeKind,
    pub rate: f64,
    pub cap_bb: f64,
    pub no_flop_no_drop: bool,
    pub exempt_pot_bb: f64,
}

#[derive(Clone, Debug)]
pub struct AlgorithmModel {
    pub seed: u64,
    pub exploration_epsilon: f64,
    pub discount_every: u64,
    pub discount_until: u64,
    /// See `multiway::solver::SolverConfig::traverser_vector`. Only valid
    /// when `abstraction.recall == RecallKind::Street`.
    pub traverser_vector: bool,
}

#[derive(Clone, Debug)]
pub struct RunModel {
    pub sweeps: u64,
    pub seed: Option<u64>,
    pub check_every: u64,
    /// `0` means "auto" (`RunSection::threads = None`).
    pub threads: usize,
    pub checkpoint_every: Option<u64>,
    pub evaluation_samples: Option<u64>,
    pub evaluation_cadence: Option<u64>,
    /// Complete sweeps run per parallel drive iteration against the same
    /// strategy snapshot; `1` is bit-identical to a pre-batching solve. See
    /// `multiway::solver::SolverConfig::sweep_batch`.
    pub sweep_batch: u64,
    /// `0` means "unset" (`RunSection::max_memory_bytes = None`).
    pub max_memory_mib: u64,
    pub storage: StorageKind,
    /// Convergence stop rule (`RunSection::stop_dev_gain` and friends); see
    /// `QualityPreset` for the Auto-mode mapping. `None` disables the rule,
    /// same as an omitted TOML key.
    pub stop_dev_gain: Option<f64>,
    pub stop_confirmations: Option<u32>,
    pub stop_eval_period_secs: Option<f64>,
    /// GUI-only wall-clock cap, in minutes; never written to the TOML (see
    /// `worker::RunTarget::max_wall_time_secs`). Available in both Auto (the
    /// derived-settings panel) and Advanced (its own optional Run field).
    pub max_wall_time_minutes: Option<f64>,
    /// GUI-only artifact destinations; not part of `SolveConfig`/presets,
    /// same as the CLI's `solve --checkpoint`/`--sol` flags.
    pub output_path: String,
    pub checkpoint_path: String,
    /// Empty means "start a fresh solve" (no checkpoint to resume).
    pub resume_from: String,
}

/// Auto-mode solve-quality presets: how tightly the convergence stop rule
/// (`RunModel::stop_dev_gain` and friends) is set. See
/// `apply_auto_derivation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QualityPreset {
    Fast,
    Normal,
    High,
}

impl QualityPreset {
    pub const ALL: [QualityPreset; 3] = [
        QualityPreset::Fast,
        QualityPreset::Normal,
        QualityPreset::High,
    ];

    pub fn label(self) -> &'static str {
        match self {
            QualityPreset::Fast => "Fast",
            QualityPreset::Normal => "Normal",
            QualityPreset::High => "High",
        }
    }

    /// Convergence-threshold size as a fraction of the shortest seat's
    /// starting stack. Relative rather than absolute (bb) because the scale
    /// of exploitable EV differences -- and of the evaluation estimator's
    /// variance -- both shrink with stack depth: a flat 0.25 bb is a tight
    /// bar at 100bb but a loose one for a 15bb push/fold spot, and a
    /// stack-relative bar keeps the required evaluation sample count roughly
    /// depth-independent.
    pub fn stack_fraction(self) -> f64 {
        match self {
            QualityPreset::Fast => 0.005,
            QualityPreset::Normal => 0.0025,
            QualityPreset::High => 0.001,
        }
    }

    /// Resolved `stop_dev_gain` threshold for `model`, in the run's utility
    /// unit: [`Self::stack_fraction`] of the shortest starting stack for
    /// chip-EV (so Normal at 100bb is the familiar 0.25 bb), and the same
    /// fraction of the shortest seat's chip-proportional share of the total
    /// payouts under tournament ICM (a deliberate chip-chip proxy for that
    /// seat's baseline equity -- exact ICM equity is not worth computing for
    /// a stopping threshold). Floored well above zero so a degenerate model
    /// mid-edit can never produce an impossible threshold.
    pub fn stop_dev_gain_for(self, model: &Model) -> f64 {
        let min_stack = model
            .seats
            .iter()
            .map(|seat| seat.stack_bb)
            .fold(f64::INFINITY, f64::min);
        let min_stack = if min_stack.is_finite() && min_stack > 0.0 {
            min_stack
        } else {
            100.0
        };
        let scale = match model.utility.kind {
            UtilityKind::ChipEv => min_stack,
            UtilityKind::TournamentIcm => {
                let table_chips: f64 = model.seats.iter().map(|seat| seat.stack_bb).sum();
                let outside_chips: f64 = model
                    .utility
                    .outside_field
                    .iter()
                    .map(|player| player.stack_bb)
                    .sum();
                let total_chips = table_chips + outside_chips;
                let total_payouts: f64 = parse_payouts(&model.utility.payouts_text)
                    .map(|payouts| payouts.iter().sum())
                    .unwrap_or(0.0);
                if total_chips > 0.0 && total_payouts > 0.0 {
                    min_stack / total_chips * total_payouts
                } else {
                    min_stack
                }
            }
        };
        (self.stack_fraction() * scale).max(1.0e-6)
    }

    pub fn stop_confirmations(self) -> u32 {
        2
    }

    pub fn stop_eval_period_secs(self) -> f64 {
        30.0
    }
}

#[derive(Clone, Debug)]
pub struct Model {
    pub seats: Vec<SeatModel>,
    pub button: usize,
    pub small_bb: f64,
    pub big_bb: f64,
    pub ante: AnteModel,
    pub betting: BettingModel,
    pub abstraction: AbstractionModel,
    pub utility: UtilityModel,
    pub rake: RakeModel,
    pub algorithm: AlgorithmModel,
    pub run: RunModel,
}

// --- position labels -------------------------------------------------------

/// Early-position labels (offsets 3.. from the button) for an `n`-seat
/// table, standard poker nomenclature.
fn early_labels(n: usize) -> &'static [&'static str] {
    match n {
        4 => &["CO"],
        5 => &["HJ", "CO"],
        6 => &["UTG", "HJ", "CO"],
        7 => &["UTG", "UTG1", "HJ", "CO"],
        8 => &["UTG", "UTG1", "LJ", "HJ", "CO"],
        9 => &["UTG", "UTG1", "UTG2", "LJ", "HJ", "CO"],
        _ => &[],
    }
}

/// Standard position name for a seat `offset_from_button` seats after the
/// button, at an `n`-seat table.
pub fn position_label(n: usize, offset_from_button: usize) -> String {
    if n == 2 {
        return if offset_from_button == 0 {
            "BTN/SB"
        } else {
            "BB"
        }
        .to_string();
    }
    match offset_from_button {
        0 => "BTN".to_string(),
        1 => "SB".to_string(),
        2 => "BB".to_string(),
        offset => early_labels(n)
            .get(offset - 3)
            .copied()
            .unwrap_or("UTG")
            .to_string(),
    }
}

pub fn seat_position_name(n: usize, button: usize, seat_index: usize) -> String {
    debug_assert!(button < n);
    let offset = (seat_index + n - button) % n;
    position_label(n, offset)
}

/// Overwrites every seat's `name` with its standard position label for the
/// given `button`. Called after a seat-count or button change.
pub fn relabel_positions(seats: &mut [SeatModel], button: usize) {
    let n = seats.len();
    for (index, seat) in seats.iter_mut().enumerate() {
        seat.name = seat_position_name(n, button, index);
    }
}

/// The standard button seat index for an `n`-seat table generated fresh
/// (matches the built-in presets: `BTN` sits at offset `n - 3` for `n >=
/// 3`, or seat `0` heads-up).
pub fn default_button(n: usize) -> usize {
    if n <= 2 { 0 } else { n - 3 }
}

// --- defaults ---------------------------------------------------------------

impl Model {
    pub fn new_default(seats: usize) -> Self {
        let seats = seats.clamp(multiway::types::MIN_SEATS, multiway::types::MAX_SEATS);
        let button = default_button(seats);
        let mut seat_models: Vec<SeatModel> = (0..seats)
            .map(|index| SeatModel {
                name: seat_position_name(seats, button, index),
                stack_bb: 100.0,
                range: String::new(),
                betting_override: None,
            })
            .collect();
        relabel_positions(&mut seat_models, button);
        Model {
            seats: seat_models,
            button,
            small_bb: 0.5,
            big_bb: 1.0,
            ante: AnteModel {
                kind: AnteKind::None,
                amount_bb: 0.0,
            },
            betting: config_betting_to_model(&BettingConfig::default()),
            abstraction: config_abstraction_to_model(&AbstractionConfig::default()),
            utility: UtilityModel {
                kind: UtilityKind::ChipEv,
                outside_field: Vec::new(),
                payouts_text: String::new(),
                samples: 100_000,
                seed: 0,
            },
            rake: RakeModel {
                kind: RakeKind::None,
                rate: 0.05,
                cap_bb: 3.0,
                no_flop_no_drop: true,
                exempt_pot_bb: 0.0,
            },
            algorithm: AlgorithmModel {
                seed: 0,
                exploration_epsilon: 0.06,
                discount_every: 100_000,
                discount_until: 10_000_000,
                traverser_vector: false,
            },
            run: RunModel {
                sweeps: 100_000,
                seed: None,
                check_every: 25,
                threads: 0,
                checkpoint_every: None,
                evaluation_samples: None,
                evaluation_cadence: None,
                // Written explicitly into every new config (1 is the "not
                // set" sentinel that omits the key): one sweep only yields
                // `seats` parallel traversals, so without batching a 6-max
                // table can never occupy more than 6 threads. 8 saturates
                // common core counts for 2..9 seats while staying a fixed,
                // machine-independent value inside the exported TOML.
                sweep_batch: 8,
                max_memory_mib: 0,
                storage: StorageKind::F32,
                stop_dev_gain: None,
                stop_confirmations: None,
                stop_eval_period_secs: None,
                max_wall_time_minutes: None,
                output_path: "./runs/solution.mwsol".to_string(),
                checkpoint_path: "./runs/checkpoint.mwckpt".to_string(),
                resume_from: String::new(),
            },
        }
    }
}

// --- StreetBettingConfig <-> StreetBettingModel -----------------------------

fn street_model_to_config(model: &StreetBettingModel) -> Result<StreetBettingConfig, String> {
    Ok(StreetBettingConfig {
        bet_sizes: size_lexer::parse_sizes(&model.bet_sizes)?,
        isolate_sizes: model
            .isolate_sizes
            .as_deref()
            .map(size_lexer::parse_sizes)
            .transpose()?,
        raise_sizes: size_lexer::parse_sizes(&model.raise_sizes)?,
        max_aggressive_actions: model.max_aggressive_actions,
        include_allin: model.include_allin,
        allin_threshold: model.allin_threshold,
        max_betting_players: model.max_betting_players,
    })
}

fn config_street_to_model(config: &StreetBettingConfig) -> StreetBettingModel {
    StreetBettingModel {
        bet_sizes: size_lexer::render_sizes(&config.bet_sizes),
        isolate_sizes: config
            .isolate_sizes
            .as_ref()
            .map(|sizes| size_lexer::render_sizes(sizes)),
        raise_sizes: size_lexer::render_sizes(&config.raise_sizes),
        max_aggressive_actions: config.max_aggressive_actions,
        include_allin: config.include_allin,
        allin_threshold: config.allin_threshold,
        max_betting_players: config.max_betting_players,
    }
}

fn betting_model_to_config(model: &BettingModel) -> Result<BettingConfig, String> {
    Ok(BettingConfig {
        allow_limp: model.allow_limp,
        preflop: street_model_to_config(&model.preflop)?,
        flop: street_model_to_config(&model.flop)?,
        turn: street_model_to_config(&model.turn)?,
        river: street_model_to_config(&model.river)?,
    })
}

fn config_betting_to_model(config: &BettingConfig) -> BettingModel {
    BettingModel {
        allow_limp: config.allow_limp,
        preflop: config_street_to_model(&config.preflop),
        flop: config_street_to_model(&config.flop),
        turn: config_street_to_model(&config.turn),
        river: config_street_to_model(&config.river),
    }
}

// --- seats -------------------------------------------------------------

fn seat_model_to_config(model: &SeatModel) -> Result<SeatConfig, String> {
    Ok(SeatConfig {
        name: (!model.name.trim().is_empty()).then(|| model.name.clone()),
        stack_bb: model.stack_bb,
        range: model.range.clone(),
        betting: model
            .betting_override
            .as_ref()
            .map(betting_model_to_config)
            .transpose()?,
    })
}

fn config_seat_to_model(config: &SeatConfig) -> SeatModel {
    SeatModel {
        name: config.name.clone().unwrap_or_default(),
        stack_bb: config.stack_bb,
        range: config.range.clone(),
        betting_override: config.betting.as_ref().map(config_betting_to_model),
    }
}

// --- abstraction ---------------------------------------------------------

fn abstraction_model_to_config(model: &AbstractionModel) -> AbstractionConfig {
    AbstractionConfig {
        flop_buckets: model.flop_buckets,
        turn_buckets: model.turn_buckets,
        river_buckets: model.river_buckets,
        rollout_samples: model.rollout_samples,
        seed: model.seed,
        active_opponent_buckets: model
            .active_opponent_buckets
            .iter()
            .map(|entry| ActiveOpponentBucketConfig {
                active_opponents: entry.active_opponents,
                flop_buckets: entry.flop_buckets,
                turn_buckets: entry.turn_buckets,
                river_buckets: entry.river_buckets,
            })
            .collect(),
        artifact_cache: (!model.artifact_cache.trim().is_empty())
            .then(|| model.artifact_cache.clone().into()),
        recall: recall_model_to_config(model.recall),
        kind: abstraction_kind_model_to_config(model.kind),
    }
}

fn config_abstraction_to_model(config: &AbstractionConfig) -> AbstractionModel {
    AbstractionModel {
        flop_buckets: config.flop_buckets,
        turn_buckets: config.turn_buckets,
        river_buckets: config.river_buckets,
        rollout_samples: config.rollout_samples,
        seed: config.seed,
        active_opponent_buckets: config
            .active_opponent_buckets
            .iter()
            .map(|entry| ActiveOpponentBucketModel {
                active_opponents: entry.active_opponents,
                flop_buckets: entry.flop_buckets,
                turn_buckets: entry.turn_buckets,
                river_buckets: entry.river_buckets,
            })
            .collect(),
        artifact_cache: config
            .artifact_cache
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        recall: config_recall_to_model(config.recall),
        kind: config_abstraction_kind_to_model(config.kind),
    }
}

fn recall_model_to_config(kind: RecallKind) -> RecallMode {
    match kind {
        RecallKind::Full => RecallMode::Full,
        RecallKind::Street => RecallMode::Street,
    }
}

fn abstraction_kind_model_to_config(kind: AbstractionBackendKind) -> AbstractionKind {
    match kind {
        AbstractionBackendKind::RolloutKmeans => AbstractionKind::RolloutKmeans,
        AbstractionBackendKind::Ehs2Table => AbstractionKind::Ehs2Table,
    }
}

fn config_abstraction_kind_to_model(kind: AbstractionKind) -> AbstractionBackendKind {
    match kind {
        AbstractionKind::RolloutKmeans => AbstractionBackendKind::RolloutKmeans,
        AbstractionKind::Ehs2Table => AbstractionBackendKind::Ehs2Table,
    }
}

fn config_recall_to_model(mode: RecallMode) -> RecallKind {
    match mode {
        RecallMode::Full => RecallKind::Full,
        RecallMode::Street => RecallKind::Street,
    }
}

// --- utility/rake/ante ---------------------------------------------------

fn parse_payouts(text: &str) -> Result<Vec<f64>, String> {
    text.split([',', '\n'])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            entry
                .parse::<f64>()
                .map_err(|_| format!("invalid payout number '{entry}'"))
        })
        .collect()
}

fn render_payouts(payouts: &[f64]) -> String {
    payouts
        .iter()
        .map(|payout| format!("{payout}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn utility_model_to_section(model: &UtilityModel) -> Result<UtilitySection, String> {
    match model.kind {
        UtilityKind::ChipEv => Ok(UtilitySection::ChipEv),
        UtilityKind::TournamentIcm => Ok(UtilitySection::TournamentIcm {
            outside_field: model
                .outside_field
                .iter()
                .map(|player| OutsidePlayerSection {
                    name: player.name.clone(),
                    stack_bb: player.stack_bb,
                })
                .collect(),
            payouts: parse_payouts(&model.payouts_text)?,
            samples: model.samples,
            seed: model.seed,
        }),
    }
}

fn section_to_utility_model(section: UtilitySection) -> Result<UtilityModel, String> {
    match section {
        UtilitySection::ChipEv => Ok(UtilityModel {
            kind: UtilityKind::ChipEv,
            outside_field: Vec::new(),
            payouts_text: String::new(),
            samples: 100_000,
            seed: 0,
        }),
        UtilitySection::TournamentIcm {
            outside_field,
            payouts,
            samples,
            seed,
        } => Ok(UtilityModel {
            kind: UtilityKind::TournamentIcm,
            outside_field: outside_field
                .into_iter()
                .map(|player| OutsideFieldModel {
                    name: player.name,
                    stack_bb: player.stack_bb,
                })
                .collect(),
            payouts_text: render_payouts(&payouts),
            samples,
            seed,
        }),
        UtilitySection::Icm { .. } => {
            Err("legacy kind = \"icm\" utility is not supported by the multiway GUI".to_string())
        }
    }
}

/// Multiway-grid unit conversion shared by rake fields: config-facing chip
/// amounts sit on the same thousandths-of-a-big-blind grid `MwChips` uses
/// (see `cli::session::convert_rake`), while the GUI edits plain bb values.
const RAKE_CHIPS_PER_BB: f64 = multiway::types::CHIPS_PER_BB as f64;

fn rake_model_to_section(model: &RakeModel) -> RakeSection {
    match model.kind {
        RakeKind::None => RakeSection::None,
        RakeKind::PercentCap => RakeSection::PercentCap {
            rate: model.rate,
            cap: model.cap_bb * RAKE_CHIPS_PER_BB,
            no_flop_no_drop: model.no_flop_no_drop,
        },
        RakeKind::GgPreflop => RakeSection::GgPreflop {
            rate: model.rate,
            cap: model.cap_bb * RAKE_CHIPS_PER_BB,
            exempt_pot: (model.exempt_pot_bb * RAKE_CHIPS_PER_BB).round() as u32,
        },
    }
}

fn section_to_rake_model(section: RakeSection) -> RakeModel {
    match section {
        RakeSection::None => RakeModel {
            kind: RakeKind::None,
            rate: 0.05,
            cap_bb: 3.0,
            no_flop_no_drop: true,
            exempt_pot_bb: 0.0,
        },
        RakeSection::PercentCap {
            rate,
            cap,
            no_flop_no_drop,
        } => RakeModel {
            kind: RakeKind::PercentCap,
            rate,
            cap_bb: cap / RAKE_CHIPS_PER_BB,
            no_flop_no_drop,
            exempt_pot_bb: 0.0,
        },
        RakeSection::GgPreflop {
            rate,
            cap,
            exempt_pot,
        } => RakeModel {
            kind: RakeKind::GgPreflop,
            rate,
            cap_bb: cap / RAKE_CHIPS_PER_BB,
            no_flop_no_drop: true,
            exempt_pot_bb: f64::from(exempt_pot) / RAKE_CHIPS_PER_BB,
        },
    }
}

fn ante_model_to_config(model: &AnteModel) -> AnteConfig {
    match model.kind {
        AnteKind::None => AnteConfig::None,
        AnteKind::Each => AnteConfig::Each {
            amount_bb: model.amount_bb,
        },
        AnteKind::BigBlind => AnteConfig::BigBlind {
            amount_bb: model.amount_bb,
        },
    }
}

fn config_ante_to_model(config: &AnteConfig) -> AnteModel {
    match *config {
        AnteConfig::None => AnteModel {
            kind: AnteKind::None,
            amount_bb: 0.0,
        },
        AnteConfig::Each { amount_bb } => AnteModel {
            kind: AnteKind::Each,
            amount_bb,
        },
        AnteConfig::BigBlind { amount_bb } => AnteModel {
            kind: AnteKind::BigBlind,
            amount_bb,
        },
    }
}

// --- run ---------------------------------------------------------------

fn run_model_to_section(model: &RunModel) -> RunSection {
    RunSection {
        iterations: 0,
        sweeps: Some(model.sweeps),
        seed: model.seed,
        check_every: model.check_every,
        storage: model.storage,
        target_nash_conv: None,
        threads: (model.threads != 0).then_some(model.threads),
        par_chance_depth: None,
        par_min_children: None,
        max_memory_bytes: (model.max_memory_mib != 0).then_some(model.max_memory_mib * 1024 * 1024),
        checkpoint_every: model.checkpoint_every,
        evaluation_samples: model.evaluation_samples,
        evaluation_cadence: model.evaluation_cadence,
        sweep_batch: (model.sweep_batch != 1).then_some(model.sweep_batch),
        stop_dev_gain: model.stop_dev_gain,
        stop_confirmations: model.stop_confirmations,
        stop_eval_period_secs: model.stop_eval_period_secs,
    }
}

fn section_to_run_model(section: RunSection, previous: &RunModel) -> RunModel {
    RunModel {
        sweeps: section.sweeps.unwrap_or(section.iterations),
        seed: section.seed,
        check_every: section.check_every,
        threads: section.threads.unwrap_or(0),
        checkpoint_every: section.checkpoint_every,
        evaluation_samples: section.evaluation_samples,
        evaluation_cadence: section.evaluation_cadence,
        sweep_batch: section.sweep_batch.unwrap_or(1),
        max_memory_mib: section
            .max_memory_bytes
            .map(|bytes| bytes / (1024 * 1024))
            .unwrap_or(0),
        storage: section.storage,
        stop_dev_gain: section.stop_dev_gain,
        stop_confirmations: section.stop_confirmations,
        stop_eval_period_secs: section.stop_eval_period_secs,
        // GUI-only fields carry over from whatever the user had configured;
        // a loaded preset/import never specifies them.
        max_wall_time_minutes: previous.max_wall_time_minutes,
        output_path: previous.output_path.clone(),
        checkpoint_path: previous.checkpoint_path.clone(),
        resume_from: previous.resume_from.clone(),
    }
}

// --- top-level conversions ------------------------------------------------

pub fn model_to_solve_config(model: &Model) -> Result<SolveConfig, String> {
    let seats = model
        .seats
        .iter()
        .map(seat_model_to_config)
        .collect::<Result<Vec<_>, _>>()?;
    let game_config = MultiwayConfig {
        seats,
        button: SeatId::new_unchecked(model.button as u8),
        blinds: BlindConfig {
            small_bb: model.small_bb,
            big_bb: model.big_bb,
        },
        ante: ante_model_to_config(&model.ante),
        betting: betting_model_to_config(&model.betting)?,
        abstraction: abstraction_model_to_config(&model.abstraction),
    };
    Ok(SolveConfig {
        game: GameSection::PreflopMultiway(game_config),
        rake: rake_model_to_section(&model.rake),
        utility: utility_model_to_section(&model.utility)?,
        algorithm: AlgorithmSection::ExternalSamplingMccfr {
            seed: model.algorithm.seed,
            exploration_epsilon: model.algorithm.exploration_epsilon,
            discount_every: model.algorithm.discount_every,
            discount_until: model.algorithm.discount_until,
            traverser_vector: model.algorithm.traverser_vector,
        },
        run: run_model_to_section(&model.run),
    })
}

pub fn model_to_toml(model: &Model) -> Result<String, String> {
    let config = model_to_solve_config(model)?;
    toml::to_string(&config).map_err(|error| error.to_string())
}

/// Converts a parsed `SolveConfig` into a `Model`, keeping this model's
/// GUI-only run fields (artifact paths) since those never round-trip
/// through TOML.
pub fn solve_config_to_model(
    config: SolveConfig,
    previous_run: &RunModel,
) -> Result<Model, String> {
    let GameSection::PreflopMultiway(game) = config.game else {
        return Err(
            "only kind = \"preflop-multiway\" configs are supported by the native GUI".to_string(),
        );
    };
    let AlgorithmSection::ExternalSamplingMccfr {
        seed,
        exploration_epsilon,
        discount_every,
        discount_until,
        traverser_vector,
    } = config.algorithm
    else {
        return Err(
            "only schedule = \"external-sampling-mccfr\" is supported by the multiway GUI"
                .to_string(),
        );
    };
    Ok(Model {
        seats: game.seats.iter().map(config_seat_to_model).collect(),
        button: game.button.index(),
        small_bb: game.blinds.small_bb,
        big_bb: game.blinds.big_bb,
        ante: config_ante_to_model(&game.ante),
        betting: config_betting_to_model(&game.betting),
        abstraction: config_abstraction_to_model(&game.abstraction),
        utility: section_to_utility_model(config.utility)?,
        rake: section_to_rake_model(config.rake),
        algorithm: AlgorithmModel {
            seed,
            exploration_epsilon,
            discount_every,
            discount_until,
            traverser_vector,
        },
        run: section_to_run_model(config.run, previous_run),
    })
}

pub fn toml_to_model(raw: &str, previous_run: &RunModel) -> Result<Model, String> {
    let config: SolveConfig = toml::from_str(raw).map_err(|error| error.to_string())?;
    solve_config_to_model(config, previous_run)
}

// --- Auto mode -------------------------------------------------------------

/// Stable artifact-cache path Auto mode always writes into
/// `abstraction.artifact_cache`, so a re-solved Auto config warm-starts from
/// the same EHS² table cache instead of a machine/session-specific path.
pub const AUTO_ARTIFACT_CACHE: &str = "./runs/cache/ehs2.postcard";

/// Safety-cap sweep count Auto mode writes into `run.sweeps` when a
/// convergence stop rule is doing the real stopping (see
/// `apply_auto_derivation`): large enough that the stop rule always fires
/// first on any reasonable config, while still bounding a pathological run
/// that never converges.
pub const AUTO_SWEEPS_CAP: u64 = 50_000_000;

/// Mutates `model` in place to Auto mode's derived settings: pure function of
/// `model`'s current game definition (seats/betting/blinds/abstraction shape)
/// plus the caller-supplied machine facts. Called once, when the user
/// presses Solve in Auto mode (never silently while they type) -- see
/// `docs`'s "Auto mode" phase B plan. Machine detection (thread count, RAM)
/// happens in the GUI's setup view; this function only turns already-known
/// numbers into concrete model fields, so it stays deterministic and unit
/// testable.
///
/// Derives (via `cli::auto_run::derive_auto_run`) `run.sweep_batch` and a
/// uniform flop/turn/river bucket count fitting `memory_budget_bytes`, then
/// sets the rest of the Auto-mode preset: `abstraction.recall = "street"`,
/// `algorithm.traverser_vector = true`, `abstraction.kind = "ehs2-table"`
/// (clearing any `active_opponent_buckets`, which that backend rejects),
/// `abstraction.artifact_cache = AUTO_ARTIFACT_CACHE`, check-down
/// `max_betting_players = 2` on flop/turn/river (never preflop), `run.storage
/// = "i16"`, `run.sweeps = AUTO_SWEEPS_CAP`, `run.threads`/`run.sweep_batch`/
/// `run.max_memory_mib` from the derivation/budget, and the convergence stop
/// rule from `quality`.
pub fn apply_auto_derivation(
    model: &mut Model,
    threads: usize,
    memory_budget_bytes: u64,
    quality: QualityPreset,
) -> Result<(), String> {
    let config = model_to_solve_config(model)?;
    let GameSection::PreflopMultiway(game_config) = &config.game else {
        unreachable!("model_to_solve_config always emits PreflopMultiway")
    };
    let derivation = cli::auto_run::derive_auto_run(game_config, threads, memory_budget_bytes)
        .map_err(|error| error.to_string())?;

    model.abstraction.recall = RecallKind::Street;
    model.abstraction.kind = AbstractionBackendKind::Ehs2Table;
    model.abstraction.flop_buckets = derivation.flop_buckets;
    model.abstraction.turn_buckets = derivation.turn_buckets;
    model.abstraction.river_buckets = derivation.river_buckets;
    model.abstraction.active_opponent_buckets.clear();
    model.abstraction.artifact_cache = AUTO_ARTIFACT_CACHE.to_string();

    model.algorithm.traverser_vector = true;

    model.betting.flop.max_betting_players = Some(2);
    model.betting.turn.max_betting_players = Some(2);
    model.betting.river.max_betting_players = Some(2);

    model.run.storage = StorageKind::I16;
    model.run.sweeps = AUTO_SWEEPS_CAP;
    model.run.threads = threads;
    model.run.sweep_batch = derivation.sweep_batch;
    model.run.max_memory_mib = memory_budget_bytes / (1024 * 1024);
    let stop_dev_gain = quality.stop_dev_gain_for(model);
    model.run.stop_dev_gain = Some(stop_dev_gain);
    model.run.stop_confirmations = Some(quality.stop_confirmations());
    model.run.stop_eval_period_secs = Some(quality.stop_eval_period_secs());

    Ok(())
}

/// `true` when `model`'s run/abstraction shape already matches what
/// [`apply_auto_derivation`] would produce (modulo the machine-dependent
/// numbers: bucket counts, thread count, memory budget). Used to decide
/// whether a freshly loaded preset/TOML should default the Setup tab into
/// Auto or Advanced view: a config with any of these fields set differently
/// was clearly hand-tuned, so it opens in Advanced.
pub fn matches_auto_shape(model: &Model) -> bool {
    model.abstraction.recall == RecallKind::Street
        && model.abstraction.kind == AbstractionBackendKind::Ehs2Table
        && model.abstraction.active_opponent_buckets.is_empty()
        && model.algorithm.traverser_vector
        && model.betting.flop.max_betting_players == Some(2)
        && model.betting.turn.max_betting_players == Some(2)
        && model.betting.river.max_betting_players == Some(2)
        && model.run.storage == StorageKind::I16
}

/// Cheap validation: game/economics validation plus the run-section sanity
/// checks `cli::session::build_multiway_session` performs before it trains
/// the (potentially expensive) card abstraction. Returns human-readable
/// error strings, or an empty vec when the model is solve-ready.
pub fn validate(model: &Model) -> Vec<String> {
    let config = match model_to_solve_config(model) {
        Ok(config) => config,
        Err(error) => return vec![error],
    };
    let mut errors = Vec::new();
    let GameSection::PreflopMultiway(game_config) = &config.game else {
        unreachable!("model_to_solve_config always emits PreflopMultiway")
    };
    match cli::session::convert_utility(config.utility) {
        Ok(utility) => {
            let rake = cli::session::convert_rake(config.rake);
            if let Err(error) = game_config.validate_economics(&utility, &rake) {
                errors.push(error.to_string());
            }
        }
        Err(error) => errors.push(error.to_string()),
    }
    if let Err(error) = validate_run_section(&config.run) {
        errors.push(error);
    }
    if model.algorithm.traverser_vector && model.abstraction.recall != RecallKind::Street {
        errors.push(
            "algorithm.traverser_vector requires abstraction.recall = \"street\"".to_string(),
        );
    }
    errors
}

fn validate_run_section(run: &RunSection) -> Result<(), String> {
    let sweeps = run.sweeps.unwrap_or(run.iterations);
    if sweeps == 0 {
        return Err("run.sweeps must be positive".to_string());
    }
    if run.evaluation_cadence == Some(0) {
        return Err("run.evaluation_cadence must be positive when set".to_string());
    }
    if run.checkpoint_every == Some(0) {
        return Err("run.checkpoint_every must be positive when set".to_string());
    }
    if run.threads == Some(0) {
        return Err("run.threads must be positive when set".to_string());
    }
    if run.evaluation_samples == Some(0) {
        return Err("run.evaluation_samples must be positive when set".to_string());
    }
    if run.sweep_batch == Some(0) {
        return Err("run.sweep_batch must be positive when set".to_string());
    }
    if run
        .stop_dev_gain
        .is_some_and(|threshold| !threshold.is_finite() || threshold <= 0.0)
    {
        return Err("run.stop_dev_gain must be finite and positive when set".to_string());
    }
    if run.stop_confirmations == Some(0) {
        return Err("run.stop_confirmations must be positive when set".to_string());
    }
    if run
        .stop_eval_period_secs
        .is_some_and(|period| !period.is_finite() || period <= 0.0)
    {
        return Err("run.stop_eval_period_secs must be finite and positive when set".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets;

    #[test]
    fn fresh_model_round_trips_through_toml_and_validates() {
        let model = Model::new_default(6);
        let errors = validate(&model);
        assert!(
            errors.is_empty(),
            "unexpected validation errors: {errors:?}"
        );
        let toml_text = model_to_toml(&model).unwrap();
        let reparsed = toml_to_model(&toml_text, &model.run).unwrap();
        assert_eq!(reparsed.seats.len(), 6);
        assert_eq!(reparsed.button, model.button);
    }

    #[test]
    fn sweep_batch_round_trips_through_toml_and_new_setups_carry_it_explicitly() {
        let mut model = Model::new_default(6);
        // New setups default to 8 and WRITE it into the TOML: one sweep only
        // yields `seats` parallel traversals, so the historical default of 1
        // left most cores idle. The value must appear explicitly so results
        // stay machine-independent and reproducible from the config alone.
        assert_eq!(model.run.sweep_batch, 8);
        let default_toml = model_to_toml(&model).unwrap();
        assert!(default_toml.contains("sweep_batch = 8"));

        // 1 is the "unset" sentinel and is omitted, preserving byte-identical
        // serialization for configs that predate the field.
        model.run.sweep_batch = 1;
        let toml_text = model_to_toml(&model).unwrap();
        assert!(!toml_text.contains("sweep_batch"));
        let reparsed = toml_to_model(&toml_text, &model.run).unwrap();
        assert_eq!(reparsed.run.sweep_batch, 1);
    }

    #[test]
    fn recall_kind_round_trips_through_toml() {
        let mut model = Model::new_default(6);
        assert_eq!(model.abstraction.recall, RecallKind::Full);

        model.abstraction.recall = RecallKind::Street;
        let toml_text = model_to_toml(&model).unwrap();
        assert!(toml_text.contains("recall = \"street\""));
        let reparsed = toml_to_model(&toml_text, &model.run).unwrap();
        assert_eq!(reparsed.abstraction.recall, RecallKind::Street);
    }

    #[test]
    fn abstraction_kind_round_trips_through_toml_and_defaults_to_rollout_kmeans() {
        let mut model = Model::new_default(6);
        assert_eq!(
            model.abstraction.kind,
            AbstractionBackendKind::RolloutKmeans
        );
        // The default is omitted from the rendered TOML, same as every other
        // abstraction field that is bit-identical to its historical
        // behavior (game-fingerprint stability for pre-existing configs).
        // The full config TOML already contains the word "kind" from
        // unrelated tagged enums (`[game] kind = "preflop-multiway"`, rake/
        // utility `kind` tags), so check for the serialized variant name
        // specifically rather than the field name.
        let default_toml = model_to_toml(&model).unwrap();
        assert!(!default_toml.contains("rollout-kmeans"));

        model.abstraction.kind = AbstractionBackendKind::Ehs2Table;
        let toml_text = model_to_toml(&model).unwrap();
        assert!(toml_text.contains("kind = \"ehs2-table\""));
        let reparsed = toml_to_model(&toml_text, &model.run).unwrap();
        assert_eq!(reparsed.abstraction.kind, AbstractionBackendKind::Ehs2Table);
    }

    #[test]
    fn ehs2_table_with_active_opponent_buckets_is_a_validation_error() {
        let mut model = Model::new_default(6);
        model.abstraction.kind = AbstractionBackendKind::Ehs2Table;
        model
            .abstraction
            .active_opponent_buckets
            .push(ActiveOpponentBucketModel {
                active_opponents: 1,
                flop_buckets: 16,
                turn_buckets: 16,
                river_buckets: 16,
            });
        let errors = validate(&model);
        assert!(
            errors
                .iter()
                .any(|error| error
                    .contains("ehs2-table does not support per-opponent bucket budgets")),
            "expected an ehs2-table/active_opponent_buckets validation error, got {errors:?}"
        );

        model.abstraction.active_opponent_buckets.clear();
        assert!(validate(&model).is_empty());
    }

    #[test]
    fn traverser_vector_round_trips_through_toml_and_defaults_to_false() {
        let mut model = Model::new_default(6);
        assert!(!model.algorithm.traverser_vector);
        // The default (false) is omitted from the rendered TOML, same as
        // every other algorithm field that is bit-identical to its
        // historical behavior.
        let default_toml = model_to_toml(&model).unwrap();
        assert!(!default_toml.contains("traverser_vector"));

        model.abstraction.recall = RecallKind::Street;
        model.algorithm.traverser_vector = true;
        let toml_text = model_to_toml(&model).unwrap();
        assert!(toml_text.contains("traverser_vector = true"));
        let reparsed = toml_to_model(&toml_text, &model.run).unwrap();
        assert!(reparsed.algorithm.traverser_vector);
        assert!(validate(&reparsed).is_empty());
    }

    #[test]
    fn traverser_vector_without_street_recall_is_a_validation_error() {
        let mut model = Model::new_default(6);
        model.algorithm.traverser_vector = true;
        // `abstraction.recall` is still `Full` (the default).
        let errors = validate(&model);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("traverser_vector")),
            "expected a traverser_vector/recall validation error, got {errors:?}"
        );
    }

    #[test]
    fn built_in_presets_round_trip_model_to_toml_and_validate() {
        for &(name, toml_text) in presets::BUILT_IN {
            let placeholder_run = Model::new_default(6).run;
            let model = toml_to_model(toml_text, &placeholder_run).unwrap_or_else(|error| {
                panic!("preset {name} failed to parse into a model: {error}")
            });
            let rendered = model_to_toml(&model).unwrap_or_else(|error| {
                panic!("preset {name} model failed to render TOML: {error}")
            });
            let reparsed_config: SolveConfig = toml::from_str(&rendered)
                .unwrap_or_else(|error| panic!("preset {name} re-parse failed: {error}"));
            let GameSection::PreflopMultiway(game_config) = &reparsed_config.game else {
                panic!("preset {name} lost its PreflopMultiway game section");
            };
            let utility = cli::session::convert_utility(reparsed_config.utility)
                .unwrap_or_else(|error| panic!("preset {name} utility conversion failed: {error}"));
            let rake = cli::session::convert_rake(reparsed_config.rake);
            game_config
                .validate_economics(&utility, &rake)
                .unwrap_or_else(|error| panic!("preset {name} failed to validate: {error}"));
        }
    }

    #[test]
    fn position_labels_match_standard_nomenclature() {
        assert_eq!(position_label(2, 0), "BTN/SB");
        assert_eq!(position_label(2, 1), "BB");
        assert_eq!(position_label(6, 0), "BTN");
        assert_eq!(position_label(6, 1), "SB");
        assert_eq!(position_label(6, 2), "BB");
        assert_eq!(position_label(6, 3), "UTG");
        assert_eq!(position_label(6, 4), "HJ");
        assert_eq!(position_label(6, 5), "CO");
        assert_eq!(position_label(9, 3), "UTG");
        assert_eq!(position_label(9, 4), "UTG1");
        assert_eq!(position_label(9, 5), "UTG2");
        assert_eq!(position_label(9, 6), "LJ");
        assert_eq!(position_label(9, 7), "HJ");
        assert_eq!(position_label(9, 8), "CO");
    }

    #[test]
    fn seat_count_and_button_can_change_while_preserving_seat_data() {
        let mut model = Model::new_default(6);
        model.seats[0].range = "AA,KK".to_string();
        model.seats.push(SeatModel {
            name: String::new(),
            stack_bb: 50.0,
            range: String::new(),
            betting_override: None,
        });
        model.button = default_button(model.seats.len());
        relabel_positions(&mut model.seats, model.button);
        assert_eq!(model.seats.len(), 7);
        assert_eq!(model.seats[0].range, "AA,KK");
        assert_eq!(model.seats.last().unwrap().name, "BB");
    }

    #[test]
    fn unsupported_hu_config_is_rejected_with_a_clear_error() {
        let raw = r#"
[game]
kind = "preflop"
effective_stack_bb = 100.0

[run]
iterations = 10
"#;
        let placeholder_run = Model::new_default(2).run;
        let error = toml_to_model(raw, &placeholder_run).unwrap_err();
        assert!(error.contains("preflop-multiway"));
    }

    #[test]
    fn max_betting_players_round_trips_through_toml_and_defaults_to_unlimited() {
        let mut model = Model::new_default(6);
        assert!(model.betting.flop.max_betting_players.is_none());
        // The default (unlimited) is omitted from the rendered TOML, same as
        // every other betting field that is bit-identical to its historical
        // behavior (game-fingerprint stability for pre-existing configs).
        let default_toml = model_to_toml(&model).unwrap();
        assert!(!default_toml.contains("max_betting_players"));

        model.betting.flop.max_betting_players = Some(2);
        let toml_text = model_to_toml(&model).unwrap();
        assert!(toml_text.contains("max_betting_players = 2"));
        let reparsed = toml_to_model(&toml_text, &model.run).unwrap();
        assert_eq!(reparsed.betting.flop.max_betting_players, Some(2));
        assert!(validate(&reparsed).is_empty());
    }

    #[test]
    fn max_betting_players_validation_errors_surface_through_model_validate() {
        let mut model = Model::new_default(6);
        model.betting.preflop.max_betting_players = Some(2);
        let errors = validate(&model);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("max_betting_players")),
            "expected a preflop max_betting_players validation error, got {errors:?}"
        );
    }

    #[test]
    fn quality_presets_scale_the_threshold_with_the_shortest_stack() {
        // At the default 100bb table the resolved thresholds are the
        // familiar absolute values...
        let mut model = Model::new_default(6);
        assert!((QualityPreset::Fast.stop_dev_gain_for(&model) - 0.5).abs() < 1e-12);
        assert!((QualityPreset::Normal.stop_dev_gain_for(&model) - 0.25).abs() < 1e-12);
        assert!((QualityPreset::High.stop_dev_gain_for(&model) - 0.1).abs() < 1e-12);

        // ...and a short-stacked table tightens them proportionally: the
        // SHORTEST stack sets the scale, so one 20bb seat at an otherwise
        // 100bb table demands 5x tighter convergence.
        model.seats[0].stack_bb = 20.0;
        assert!((QualityPreset::Normal.stop_dev_gain_for(&model) - 0.05).abs() < 1e-12);

        // ICM: the scale becomes the shortest seat's chip-proportional share
        // of the total payouts (chip-chip proxy for its baseline equity).
        model.utility.kind = UtilityKind::TournamentIcm;
        model.utility.payouts_text = "600, 300, 100".to_string();
        let table_chips: f64 = model.seats.iter().map(|seat| seat.stack_bb).sum();
        let expected = 0.0025 * (20.0 / table_chips) * 1000.0;
        assert!((QualityPreset::Normal.stop_dev_gain_for(&model) - expected).abs() < 1e-9);

        for preset in QualityPreset::ALL {
            assert_eq!(preset.stop_confirmations(), 2);
            assert_eq!(preset.stop_eval_period_secs(), 30.0);
        }
    }

    #[test]
    fn apply_auto_derivation_lands_every_derived_field_and_writes_them_explicitly() {
        // 3 seats (rather than a bigger table) keeps the dense-arena builds
        // this derivation runs (one per bucket-ladder rung, see
        // `cli::auto_run::derive_auto_run`) cheap even at full default
        // betting sizes -- the same scale `cli::auto_run`'s own tests use.
        let mut model = Model::new_default(3);
        assert!(!matches_auto_shape(&model));

        apply_auto_derivation(&mut model, 8, 1024 * 1024 * 1024, QualityPreset::Normal)
            .expect("a fresh default model must derive cleanly");

        assert_eq!(model.abstraction.recall, RecallKind::Street);
        assert_eq!(model.abstraction.kind, AbstractionBackendKind::Ehs2Table);
        assert!(model.abstraction.active_opponent_buckets.is_empty());
        assert_eq!(model.abstraction.artifact_cache, AUTO_ARTIFACT_CACHE);
        assert!(model.algorithm.traverser_vector);
        assert_eq!(model.betting.flop.max_betting_players, Some(2));
        assert_eq!(model.betting.turn.max_betting_players, Some(2));
        assert_eq!(model.betting.river.max_betting_players, Some(2));
        assert_eq!(model.betting.preflop.max_betting_players, None);
        assert_eq!(model.run.storage, StorageKind::I16);
        assert_eq!(model.run.sweeps, AUTO_SWEEPS_CAP);
        assert_eq!(model.run.threads, 8);
        // 3 seats, 8 threads -> ceil(8/3) = 3.
        assert_eq!(model.run.sweep_batch, 3);
        assert_eq!(model.run.max_memory_mib, 1024);
        assert_eq!(model.run.stop_dev_gain, Some(0.25));
        assert_eq!(model.run.stop_confirmations, Some(2));
        assert_eq!(model.run.stop_eval_period_secs, Some(30.0));
        assert!(matches_auto_shape(&model));
        assert!(validate(&model).is_empty());

        let toml_text = model_to_toml(&model).unwrap();
        for needle in [
            "sweep_batch = 3",
            "kind = \"ehs2-table\"",
            "recall = \"street\"",
            "max_betting_players = 2",
            "storage = \"i16\"",
            "stop_dev_gain = 0.25",
            "stop_confirmations = 2",
            "stop_eval_period_secs = 30.0",
            "traverser_vector = true",
        ] {
            assert!(
                toml_text.contains(needle),
                "expected derived TOML to contain {needle:?}, got:\n{toml_text}"
            );
        }
        let reparsed = toml_to_model(&toml_text, &model.run).unwrap();
        assert!(matches_auto_shape(&reparsed));
        assert!(validate(&reparsed).is_empty());
    }

    #[test]
    fn apply_auto_derivation_is_pure_and_reproducible() {
        let mut first = Model::new_default(3);
        let mut second = first.clone();
        apply_auto_derivation(&mut first, 8, 512 * 1024 * 1024, QualityPreset::High).unwrap();
        apply_auto_derivation(&mut second, 8, 512 * 1024 * 1024, QualityPreset::High).unwrap();
        assert_eq!(
            model_to_toml(&first).unwrap(),
            model_to_toml(&second).unwrap()
        );
    }
}
