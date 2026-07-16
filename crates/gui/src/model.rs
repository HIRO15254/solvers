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
    AbstractionConfig, ActiveOpponentBucketConfig, AnteConfig, BettingConfig, BlindConfig,
    MultiwayConfig, SeatConfig, StreetBettingConfig,
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
    /// GUI-only artifact destinations; not part of `SolveConfig`/presets,
    /// same as the CLI's `solve --checkpoint`/`--sol` flags.
    pub output_path: String,
    pub checkpoint_path: String,
    /// Empty means "start a fresh solve" (no checkpoint to resume).
    pub resume_from: String,
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
            },
            run: RunModel {
                sweeps: 100_000,
                seed: None,
                check_every: 25,
                threads: 0,
                checkpoint_every: None,
                evaluation_samples: None,
                evaluation_cadence: None,
                sweep_batch: 1,
                max_memory_mib: 0,
                storage: StorageKind::F32,
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
        // GUI-only fields carry over from whatever the user had configured;
        // a loaded preset/import never specifies them.
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
        },
        run: section_to_run_model(config.run, previous_run),
    })
}

pub fn toml_to_model(raw: &str, previous_run: &RunModel) -> Result<Model, String> {
    let config: SolveConfig = toml::from_str(raw).map_err(|error| error.to_string())?;
    solve_config_to_model(config, previous_run)
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
    fn sweep_batch_round_trips_through_toml_and_defaults_to_one() {
        let mut model = Model::new_default(6);
        assert_eq!(model.run.sweep_batch, 1);
        // The default (1) is omitted from the rendered TOML, same as every
        // other run field that is bit-identical to its historical behavior.
        let default_toml = model_to_toml(&model).unwrap();
        assert!(!default_toml.contains("sweep_batch"));

        model.run.sweep_batch = 8;
        let toml_text = model_to_toml(&model).unwrap();
        assert!(toml_text.contains("sweep_batch = 8"));
        let reparsed = toml_to_model(&toml_text, &model.run).unwrap();
        assert_eq!(reparsed.run.sweep_batch, 8);
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
}
