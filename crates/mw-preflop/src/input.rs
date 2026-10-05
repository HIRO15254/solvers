//! P2-owned common-input settings and typed lowering. No method choices live in `spot`.
use crate::config::{
    AbstractionConfig, AbstractionKind, AnteConfig, BettingConfig, BlindConfig, ForcedBetConfig,
    MultiwayConfig, RecallMode, SeatConfig, StackRatio, StreetBettingConfig,
};
use crate::solver::{DEFAULT_PRUNE_SKIP_PROBABILITY, DEFAULT_PRUNE_THRESHOLD, SolverConfig};
use economics::{RakeConfig, UtilityConfig};
use serde::{Deserialize, Serialize};
use spot::{Code, Product, ProductSections, Spot, SpotError};

pub mod session;
pub use session::{Session, build_session};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    #[default]
    RangeVector,
    SingleHand,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AbstractionKindSetting {
    #[default]
    Ehs2Percentile,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Buckets {
    pub flop: u16,
    pub turn: u16,
    pub river: u16,
}
impl Default for Buckets {
    fn default() -> Self {
        Self {
            flop: 128,
            turn: 128,
            river: 128,
        }
    }
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Abstraction {
    pub kind: AbstractionKindSetting,
    pub buckets: Buckets,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Discount {
    Periodic {
        #[serde(default = "discount_every")]
        every_sweeps: u64,
        #[serde(default = "discount_until")]
        until_sweeps: u64,
    },
    None,
}
fn discount_every() -> u64 {
    10_000
}
fn discount_until() -> u64 {
    10_000_000
}
impl Default for Discount {
    fn default() -> Self {
        Self::Periodic {
            every_sweeps: discount_every(),
            until_sweeps: discount_until(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Pruning {
    #[default]
    RegretBased,
    None,
}

/// Numeric targets preserve the legacy semantics: BB in cash, fraction of prizes in ICM.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum Target {
    Name(String),
    Value(f64),
}
impl Default for Target {
    fn default() -> Self {
        Self::Name("default".into())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Stop {
    pub target: Target,
    pub max_sweeps: u64,
    pub check_every_sweeps: u64,
    pub confirmations: u32,
    pub evaluation_samples: u64,
    pub deviator_traversals: u64,
}
impl Default for Stop {
    fn default() -> Self {
        Self {
            target: Target::default(),
            max_sweeps: 5_000_000,
            check_every_sweeps: 10_000,
            confirmations: 3,
            evaluation_samples: 4096,
            deviator_traversals: 20_000,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Solver {
    pub kind: Kind,
    pub seed: u64,
    pub opponent_exploration: f64,
    pub batch_sweeps: u64,
    pub abstraction: Abstraction,
    pub discount: Discount,
    pub pruning: Pruning,
    pub stop: Stop,
}
impl Default for Solver {
    fn default() -> Self {
        Self {
            kind: Kind::default(),
            seed: 0,
            opponent_exploration: 0.0,
            batch_sweeps: 1,
            abstraction: Abstraction::default(),
            discount: Discount::default(),
            pruning: Pruning::default(),
            stop: Stop::default(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProbabilityEncoding {
    #[default]
    U16,
    F32,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Output {
    pub probability_encoding: ProbabilityEncoding,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub solver: Solver,
    pub output: Output,
}

fn invalid(key: &str, message: impl Into<String>) -> SpotError {
    SpotError::new(Code::NLH003, key, message)
}
fn decode_error(section: &str, error: toml::de::Error) -> SpotError {
    let message = error.to_string();
    let key = message
        .split("in `")
        .nth(1)
        .and_then(|s| s.split('`').next());
    let path = key.map_or_else(|| section.to_owned(), |key| format!("{section}.{key}"));
    let code = if message.contains("invalid type") || message.contains("did not match any variant")
    {
        Code::NLH002
    } else {
        Code::NLH003
    };
    SpotError::new(code, path, message)
}
fn keys(
    spot: &Spot,
    table: &toml::Table,
    path: &str,
    allowed: &[&str],
    other: &[&str],
) -> Result<(), SpotError> {
    for key in table.keys() {
        if !allowed.contains(&key.as_str()) {
            let path = format!("{path}.{key}");
            return Err(if other.contains(&key.as_str()) {
                spot::other_product_key(spot, path)
            } else {
                SpotError::new(Code::NLH002, path, "unknown key")
            });
        }
    }
    Ok(())
}
fn nested<'a>(
    table: &'a toml::Table,
    key: &str,
    path: &str,
) -> Result<Option<&'a toml::Table>, SpotError> {
    table
        .get(key)
        .map(|v| {
            v.as_table()
                .ok_or_else(|| SpotError::new(Code::NLH002, path, "expected a table"))
        })
        .transpose()
}

impl Settings {
    pub fn parse(
        spot: &Spot,
        solver: &toml::Table,
        output: &toml::Table,
    ) -> Result<Self, SpotError> {
        require_p2(spot)?;
        keys(
            spot,
            solver,
            "solver",
            &[
                "kind",
                "seed",
                "opponent_exploration",
                "batch_sweeps",
                "abstraction",
                "discount",
                "pruning",
                "stop",
            ],
            &["iso_merging", "storage", "algorithm", "parallel"],
        )?;
        keys(
            spot,
            output,
            "output",
            &["probability_encoding"],
            &["solution_streets"],
        )?;
        for (name, allowed, other) in [
            ("abstraction", &["kind", "buckets"][..], &[][..]),
            (
                "discount",
                &["kind", "every_sweeps", "until_sweeps"][..],
                &[][..],
            ),
            ("pruning", &["kind"][..], &[][..]),
            (
                "stop",
                &[
                    "target",
                    "max_sweeps",
                    "check_every_sweeps",
                    "confirmations",
                    "evaluation_samples",
                    "deviator_traversals",
                ][..],
                &["max_iterations", "check_every"][..],
            ),
        ] {
            if let Some(t) = nested(solver, name, &format!("solver.{name}"))? {
                keys(spot, t, &format!("solver.{name}"), allowed, other)?;
            }
        }
        if let Some(t) = nested(solver, "abstraction", "solver.abstraction")?
            && let Some(b) = nested(t, "buckets", "solver.abstraction.buckets")?
        {
            keys(
                spot,
                b,
                "solver.abstraction.buckets",
                &["flop", "turn", "river"],
                &[],
            )?;
        }
        // Check each leaf before serde decoding so type diagnostics retain its full path.
        fn types(table: &toml::Table, path: &str) -> Result<(), SpotError> {
            for (key, value) in table {
                let path = format!("{path}.{key}");
                let correct = match key.as_str() {
                    "abstraction" | "buckets" | "discount" | "pruning" | "stop" => value.is_table(),
                    "kind" | "probability_encoding" => value.is_str(),
                    "opponent_exploration" => value.is_float() || value.is_integer(),
                    "target" => value.is_str() || value.is_float() || value.is_integer(),
                    _ => value.is_integer(),
                };
                if !correct {
                    return Err(SpotError::new(Code::NLH002, path, "wrong TOML type"));
                }
                if let Some(table) = value.as_table() {
                    types(table, &path)?;
                }
            }
            Ok(())
        }
        types(solver, "solver")?;
        types(output, "output")?;
        // An empty enum table means its documented default, like an omitted table.
        let mut values = solver.clone();
        for (name, kind) in [("discount", "periodic"), ("pruning", "regret-based")] {
            if let Some(toml::Value::Table(t)) = values.get_mut(name) {
                t.entry("kind".to_owned())
                    .or_insert_with(|| toml::Value::String(kind.into()));
                if t.get("kind").and_then(toml::Value::as_str) == Some("none") {
                    keys(spot, t, &format!("solver.{name}"), &["kind"], &[])?;
                }
            }
        }
        let settings = Self {
            solver: toml::Value::Table(values)
                .try_into()
                .map_err(|e: toml::de::Error| decode_error("solver", e))?,
            output: toml::Value::Table(output.clone())
                .try_into()
                .map_err(|e: toml::de::Error| decode_error("output", e))?,
        };
        settings.validate(spot)?;
        Ok(settings)
    }

    fn validate(&self, spot: &Spot) -> Result<(), SpotError> {
        require_p2(spot)?;
        let s = &self.solver;
        if !s.opponent_exploration.is_finite() || !(0.0..=1.0).contains(&s.opponent_exploration) {
            return Err(invalid(
                "solver.opponent_exploration",
                "must be finite and in [0, 1]",
            ));
        }
        if s.kind == Kind::SingleHand && s.pruning == Pruning::RegretBased {
            return Err(invalid(
                "solver.pruning.kind",
                "regret-based pruning requires range-vector",
            ));
        }
        for (key, value) in [
            ("solver.batch_sweeps", s.batch_sweeps),
            ("solver.stop.max_sweeps", s.stop.max_sweeps),
            ("solver.stop.check_every_sweeps", s.stop.check_every_sweeps),
            ("solver.stop.confirmations", u64::from(s.stop.confirmations)),
            ("solver.stop.evaluation_samples", s.stop.evaluation_samples),
            (
                "solver.stop.deviator_traversals",
                s.stop.deviator_traversals,
            ),
            (
                "solver.abstraction.buckets.flop",
                u64::from(s.abstraction.buckets.flop),
            ),
            (
                "solver.abstraction.buckets.turn",
                u64::from(s.abstraction.buckets.turn),
            ),
            (
                "solver.abstraction.buckets.river",
                u64::from(s.abstraction.buckets.river),
            ),
        ] {
            if value == 0 {
                return Err(invalid(key, "must be positive"));
            }
        }
        if let Discount::Periodic { every_sweeps, .. } = s.discount
            && every_sweeps == 0
        {
            return Err(invalid("solver.discount.every_sweeps", "must be positive"));
        }
        resolve_target(&spot.economics.utility, &s.stop.target)?;
        Ok(())
    }
}

fn require_p2(spot: &Spot) -> Result<(), SpotError> {
    if spot.product != Product::MultiwayPreflop {
        return Err(SpotError::new(
            Code::NLH005,
            "spot",
            "P2 requires a preflop root",
        ));
    }
    Ok(())
}

pub fn resolve_target(utility: &UtilityConfig, target: &Target) -> Result<f64, SpotError> {
    let scale = match utility {
        UtilityConfig::ChipEv => 1.0,
        UtilityConfig::TournamentIcm { payouts, .. } => payouts.iter().sum(),
    };
    let value = match target {
        Target::Name(name) if name == "default" => match utility {
            UtilityConfig::ChipEv => 0.05,
            UtilityConfig::TournamentIcm { .. } => 0.0001,
        },
        Target::Name(name) if name.parse::<f64>().is_ok() => {
            return Err(SpotError::new(
                Code::NLH002,
                "solver.stop.target",
                "expected a number, not a numeric string",
            ));
        }
        Target::Name(_) => {
            return Err(invalid(
                "solver.stop.target",
                "expected default or a positive number",
            ));
        }
        Target::Value(value) => *value,
    };
    let target = value * scale;
    if value.is_finite() && value > 0.0 && target.is_finite() && target > 0.0 {
        Ok(target)
    } else {
        Err(invalid("solver.stop.target", "must be finite and positive"))
    }
}

pub struct P2Sections;
impl ProductSections for P2Sections {
    fn normalize(
        &self,
        spot: &Spot,
        solver: &toml::Table,
        output: &toml::Table,
    ) -> Result<(toml_edit::Table, toml_edit::Table), SpotError> {
        let settings = Settings::parse(spot, solver, output)?;
        fn table(value: &impl Serialize) -> Result<toml_edit::Table, SpotError> {
            toml_edit::ser::to_document(value)
                .map(|doc| doc.as_table().clone())
                .map_err(|e| invalid("solver", e.to_string()))
        }
        let mut solver = table(&settings.solver)?;
        for key in ["abstraction", "discount", "pruning", "stop"] {
            let item = solver.remove(key).expect("serialized P2 section");
            solver.insert(
                key,
                toml_edit::Item::Table(item.into_table().expect("serialized table")),
            );
        }
        Ok((solver, table(&settings.output)?))
    }
}

#[derive(Clone, Debug)]
pub struct Run {
    pub threads: usize,
    pub memory_bytes: u64,
    pub max_time_seconds: Option<f64>,
    pub checkpoint_interval_seconds: f64,
    pub stop: Stop,
    pub dev_gain_threshold: f64,
    pub evaluation_seed: u64,
}

#[derive(Clone, Debug)]
pub struct Lowered {
    pub game: MultiwayConfig,
    pub utility: UtilityConfig,
    pub rake: RakeConfig,
    pub solver: SolverConfig,
    pub run: Run,
    pub output: Output,
}

/// Pure lowering; session construction accepts this typed result directly.
pub fn lower(spot: &Spot, settings: &Settings) -> Result<Lowered, SpotError> {
    settings.validate(spot)?;
    let street = |cap| -> Result<StreetBettingConfig, SpotError> {
        Ok(StreetBettingConfig {
            bet_sizes: Vec::new(),
            isolate_sizes: None,
            raise_sizes: Vec::new(),
            max_aggressive_actions: u8::try_from(cap)
                .map_err(|_| invalid("tree.max_aggressive_actions", "exceeds the P2 u8 limit"))?,
            include_allin: spot.tree.include_allin,
            allin_threshold: spot.tree.allin_threshold,
            reraise_jam_above_actor_starting_stack: None,
            max_betting_players: None,
        })
    };
    let caps = &spot.tree.max_aggressive_actions;
    let mut betting = BettingConfig {
        allow_limp: true,
        preflop: street(caps.preflop)?,
        flop: street(caps.flop)?,
        turn: street(caps.turn)?,
        river: street(caps.river)?,
        rules: Vec::new(),
        nlh_rules: spot
            .tree
            .compiled
            .rules
            .iter()
            .cloned()
            .map(crate::tree_rules::NlhTreeRule::from_compiled)
            .collect(),
    };
    betting.preflop.reraise_jam_above_actor_starting_stack = spot
        .tree
        .preflop_reraise_jam_above_stack
        .as_ref()
        .map(|r| {
            Ok(StackRatio {
                numerator: u32::try_from(r.numerator).map_err(|_| {
                    invalid(
                        "tree.preflop_reraise_jam_above_stack",
                        "exceeds the P2 u32 limit",
                    )
                })?,
                denominator: u32::try_from(r.denominator).map_err(|_| {
                    invalid(
                        "tree.preflop_reraise_jam_above_stack",
                        "exceeds the P2 u32 limit",
                    )
                })?,
            })
        })
        .transpose()?;
    let buckets = &settings.solver.abstraction.buckets;
    let setup = &spot.table.setup;
    let game = MultiwayConfig {
        seats: spot
            .table
            .positions
            .seats()
            .map(|seat| SeatConfig {
                name: None,
                stack_bb: spot.table.stacks[seat].as_bb(),
                range: if spot.ranges[seat].text == "random" {
                    String::new()
                } else {
                    spot.ranges[seat].text.clone()
                },
                betting: None,
            })
            .collect(),
        // The forced-bet vector is authoritative, as in the old-family adapter.
        button: setup.button,
        blinds: BlindConfig::default(),
        ante: AnteConfig::None,
        betting,
        forced_bets: Some(ForcedBetConfig {
            blinds_bb: setup.forced_blinds.iter().map(|v| v.as_bb()).collect(),
            antes_bb: setup.forced_antes.iter().map(|v| v.as_bb()).collect(),
            common_ante_bb: setup.common_ante.as_bb(),
            nominal_big_blind_bb: 1.0,
            first_to_act: setup.preflop_first_to_act,
            straddles: setup
                .straddles
                .iter()
                .map(|&(seat, amount)| (seat, amount.as_bb()))
                .collect(),
        }),
        abstraction: AbstractionConfig {
            flop_buckets: buckets.flop,
            turn_buckets: buckets.turn,
            river_buckets: buckets.river,
            kind: AbstractionKind::Ehs2Table,
            recall: RecallMode::Street,
            rollout_samples: 512,
            points_per_bucket: 8,
            kmeans_iterations: 20,
            seed: 0,
            active_opponent_buckets: Vec::new(),
            artifact_cache: None,
        },
    };
    game.validate_economics(&spot.economics.utility, &spot.economics.rake)
        .map_err(|e| invalid("tree", e.to_string()))?;
    let s = &settings.solver;
    let threads = match spot.run.threads {
        Some(n) => usize::try_from(n).map_err(|_| invalid("run.threads", "exceeds usize"))?,
        None => std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1)
            .min(
                game.seats
                    .len()
                    .saturating_mul(usize::try_from(s.batch_sweeps).unwrap_or(usize::MAX)),
            ),
    };
    let memory_bytes = spot.run.memory_bytes.unwrap_or(6 * 1024 * 1024 * 1024);
    let (discount_every, discount_until) = match s.discount {
        Discount::Periodic {
            every_sweeps,
            until_sweeps,
        } => (every_sweeps, until_sweeps),
        Discount::None => (u64::MAX, 0),
    };
    let prune = s.pruning == Pruning::RegretBased;
    let stakes: f64 = match &spot.economics.utility {
        UtilityConfig::ChipEv => game.seats.iter().map(|s| s.stack_bb).sum(),
        UtilityConfig::TournamentIcm { payouts, .. } => payouts.iter().sum(),
    };
    let solver = SolverConfig {
        seed: s.seed,
        max_memory_bytes: memory_bytes,
        max_traversal_depth: 512,
        exploration_epsilon: s.opponent_exploration,
        discount_every,
        discount_until,
        sweep_batch: s.batch_sweeps,
        traverser_vector: s.kind == Kind::RangeVector,
        prune,
        prune_threshold: if prune {
            -10.0 * stakes
        } else {
            DEFAULT_PRUNE_THRESHOLD
        },
        prune_skip_probability: DEFAULT_PRUNE_SKIP_PROBABILITY,
    };
    if prune && (!solver.prune_threshold.is_finite() || solver.prune_threshold >= 0.0) {
        return Err(invalid(
            "solver.pruning",
            "derived regret threshold must be finite and negative",
        ));
    }
    Ok(Lowered {
        game,
        utility: spot.economics.utility.clone(),
        rake: spot.economics.rake.clone(),
        solver,
        run: Run {
            threads,
            memory_bytes,
            max_time_seconds: spot.run.max_time_seconds,
            checkpoint_interval_seconds: spot.run.checkpoint_interval_seconds,
            stop: s.stop.clone(),
            dev_gain_threshold: resolve_target(&spot.economics.utility, &s.stop.target)?,
            evaluation_seed: solver.seed ^ 0x6576_616c_7561_7465,
        },
        output: settings.output.clone(),
    })
}
