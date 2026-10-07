//! P1-owned sections and pure lowering of the common Spot IR.
//! Amounts in the lowered tree are milli-BB; economics and reporting are separate.
use crate::{PerStreet, PostflopConfig, StreetTree};
pub use hu_engine::CfrPrecision;
use nlh::script::{Rule, Value, VarSource};
use nlh::{Chips, PerPlayer, Player, Street};
use serde::{Deserialize, Serialize};
use spot::{Code, PostflopPlayer, Product, ProductSections, Spot, SpotError, TreeVar};

mod payoff;
mod resources;
pub use payoff::{NlhPayoff, NlhRake, NlhUtility};
pub use resources::{
    MemoryLimitError, check_memory_limit, physical_memory_bytes, resolve_memory_limit,
};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Storage {
    #[default]
    F32,
    I16,
    #[serde(rename = "i16-f32avg")]
    I16F32Avg,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SolutionStreets {
    #[default]
    Full,
    NoRivers,
}

/// P1 schedule parameters and public input defaults.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields, tag = "schedule", rename_all = "kebab-case")]
pub enum Algorithm {
    Vanilla,
    CfrPlus,
    Dcfr {
        #[serde(default = "alpha")]
        alpha: f64,
        #[serde(default = "beta")]
        beta: f64,
        #[serde(default = "gamma")]
        gamma: f64,
        #[serde(default)]
        pow4_reset: bool,
    },
    LinearCfr,
    HsDcfr {
        #[serde(default = "gamma0")]
        gamma0: f64,
    },
}
fn alpha() -> f64 {
    1.25
}
fn beta() -> f64 {
    0.5
}
fn gamma() -> f64 {
    4.0
}
fn gamma0() -> f64 {
    30.0
}
impl Default for Algorithm {
    fn default() -> Self {
        Self::Dcfr {
            alpha: alpha(),
            beta: beta(),
            gamma: gamma(),
            pow4_reset: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Stop {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub max_iterations: u64,
    pub check_every: u64,
}
impl Default for Stop {
    fn default() -> Self {
        Self {
            target: None,
            max_iterations: 1_000_000,
            check_every: 25,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Parallel {
    pub chance_depth: u32,
    pub min_children: usize,
}
impl Default for Parallel {
    fn default() -> Self {
        Self {
            chance_depth: 2,
            min_children: 12,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Solver {
    pub iso_merging: bool,
    pub storage: Storage,
    #[serde(default = "default_cfr_precision")]
    pub cfr_precision: CfrPrecision,
    pub algorithm: Algorithm,
    pub stop: Stop,
    pub parallel: Parallel,
}
fn default_cfr_precision() -> CfrPrecision {
    CfrPrecision::F32
}
impl Default for Solver {
    fn default() -> Self {
        Self {
            iso_merging: true,
            storage: Storage::default(),
            cfr_precision: default_cfr_precision(),
            algorithm: Algorithm::default(),
            stop: Stop::default(),
            parallel: Parallel::default(),
        }
    }
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Output {
    pub solution_streets: SolutionStreets,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub solver: Solver,
    pub output: Output,
}

fn invalid(key: &str, message: impl Into<String>) -> SpotError {
    SpotError::new(Code::NLH003, key, message)
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
fn section<'a>(
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
fn decode<T: serde::de::DeserializeOwned>(value: toml::Value, key: &str) -> Result<T, SpotError> {
    value.try_into().map_err(|e| invalid(key, e.to_string()))
}

fn field<T: serde::de::DeserializeOwned>(
    table: &toml::Table,
    name: &str,
    path: &str,
    default: T,
    valid_type: fn(&toml::Value) -> bool,
) -> Result<T, SpotError> {
    match table.get(name) {
        None => Ok(default),
        Some(value) if valid_type(value) => decode(value.clone(), path),
        Some(_) => Err(SpotError::new(Code::NLH002, path, "wrong TOML type")),
    }
}
fn number(value: &toml::Value) -> bool {
    value.is_float() || value.is_integer()
}
fn parameter(table: &toml::Table, name: &str, default: f64) -> Result<f64, SpotError> {
    let path = format!("solver.algorithm.{name}");
    let value = field(table, name, &path, default, number)?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(invalid(&path, "must be finite"))
    }
}

impl Settings {
    pub fn parse(
        spot: &Spot,
        solver: &toml::Table,
        output: &toml::Table,
    ) -> Result<Self, SpotError> {
        if spot.product != Product::HuPostflop {
            return Err(SpotError::new(
                Code::NLH005,
                "spot",
                "P1 settings require a HU postflop spot",
            ));
        }
        keys(
            spot,
            solver,
            "solver",
            &[
                "iso_merging",
                "storage",
                "cfr_precision",
                "algorithm",
                "stop",
                "parallel",
            ],
            &[
                "kind",
                "seed",
                "opponent_exploration",
                "batch_sweeps",
                "abstraction",
                "discount",
                "pruning",
            ],
        )?;
        keys(
            spot,
            output,
            "output",
            &["solution_streets"],
            &["probability_encoding"],
        )?;
        if let Some(t) = section(solver, "stop", "solver.stop")? {
            keys(
                spot,
                t,
                "solver.stop",
                &["target", "max_iterations", "check_every"],
                &[
                    "max_sweeps",
                    "check_every_sweeps",
                    "confirmations",
                    "evaluation_samples",
                    "deviator_traversals",
                ],
            )?;
        }
        if let Some(t) = section(solver, "parallel", "solver.parallel")? {
            keys(
                spot,
                t,
                "solver.parallel",
                &["chance_depth", "min_children"],
                &[],
            )?;
        }
        let mut settings = Solver {
            iso_merging: field(
                solver,
                "iso_merging",
                "solver.iso_merging",
                true,
                toml::Value::is_bool,
            )?,
            storage: field(
                solver,
                "storage",
                "solver.storage",
                Storage::default(),
                toml::Value::is_str,
            )?,
            cfr_precision: field(
                solver,
                "cfr_precision",
                "solver.cfr_precision",
                default_cfr_precision(),
                toml::Value::is_str,
            )?,
            ..Solver::default()
        };
        if let Some(t) = section(solver, "algorithm", "solver.algorithm")? {
            let schedule = t
                .get("schedule")
                .map(|v| {
                    v.as_str().ok_or_else(|| {
                        SpotError::new(
                            Code::NLH002,
                            "solver.algorithm.schedule",
                            "expected a string",
                        )
                    })
                })
                .transpose()?
                .unwrap_or("dcfr");
            let allowed: &[&str] = match schedule {
                "vanilla" | "cfr-plus" | "linear-cfr" => &["schedule"],
                "dcfr" => &["schedule", "alpha", "beta", "gamma", "pow4_reset"],
                "hs-dcfr" => &["schedule", "gamma0"],
                _ => {
                    return Err(invalid(
                        "solver.algorithm.schedule",
                        "expected vanilla, cfr-plus, dcfr, linear-cfr or hs-dcfr",
                    ));
                }
            };
            keys(spot, t, "solver.algorithm", allowed, &[])?;
            settings.algorithm = match schedule {
                "vanilla" => Algorithm::Vanilla,
                "cfr-plus" => Algorithm::CfrPlus,
                "linear-cfr" => Algorithm::LinearCfr,
                "dcfr" => Algorithm::Dcfr {
                    alpha: parameter(t, "alpha", alpha())?,
                    beta: parameter(t, "beta", beta())?,
                    gamma: parameter(t, "gamma", gamma())?,
                    pow4_reset: field(
                        t,
                        "pow4_reset",
                        "solver.algorithm.pow4_reset",
                        false,
                        toml::Value::is_bool,
                    )?,
                },
                "hs-dcfr" => Algorithm::HsDcfr {
                    gamma0: parameter(t, "gamma0", gamma0())?,
                },
                _ => unreachable!("validated schedule"),
            };
        }
        if let Some(t) = section(solver, "stop", "solver.stop")? {
            settings.stop = Stop {
                target: field(t, "target", "solver.stop.target", None, toml::Value::is_str)?,
                max_iterations: field(
                    t,
                    "max_iterations",
                    "solver.stop.max_iterations",
                    settings.stop.max_iterations,
                    toml::Value::is_integer,
                )?,
                check_every: field(
                    t,
                    "check_every",
                    "solver.stop.check_every",
                    settings.stop.check_every,
                    toml::Value::is_integer,
                )?,
            };
        }
        if let Some(t) = section(solver, "parallel", "solver.parallel")? {
            settings.parallel = Parallel {
                chance_depth: field(
                    t,
                    "chance_depth",
                    "solver.parallel.chance_depth",
                    settings.parallel.chance_depth,
                    toml::Value::is_integer,
                )?,
                min_children: field(
                    t,
                    "min_children",
                    "solver.parallel.min_children",
                    settings.parallel.min_children,
                    toml::Value::is_integer,
                )?,
            };
        }
        let output = Output {
            solution_streets: field(
                output,
                "solution_streets",
                "output.solution_streets",
                SolutionStreets::default(),
                toml::Value::is_str,
            )?,
        };
        for (key, value) in [
            ("solver.stop.max_iterations", settings.stop.max_iterations),
            ("solver.stop.check_every", settings.stop.check_every),
            (
                "solver.parallel.min_children",
                settings.parallel.min_children as u64,
            ),
        ] {
            if value == 0 {
                return Err(invalid(key, "must be positive"));
            }
        }
        if let Some(target) = &settings.stop.target {
            resolve_target(spot, target)?;
        }
        Ok(Self {
            solver: settings,
            output,
        })
    }
}

/// Absolute exploitability (NashConv / 2) threshold, in BB or prize units.
pub fn resolve_target(spot: &Spot, target: &str) -> Result<f64, SpotError> {
    let tournament = matches!(
        spot.economics.utility,
        economics::UtilityConfig::TournamentIcm { .. }
    );
    let amount_and_basis = if tournament {
        let economics::UtilityConfig::TournamentIcm { payouts, .. } = &spot.economics.utility
        else {
            unreachable!()
        };
        target
            .strip_suffix("%prizes")
            .map(|s| (s, payouts.iter().sum::<f64>() / 100.0))
    } else {
        target
            .strip_suffix("%pot")
            .map(|s| (s, spot.context.pot.as_bb() / 100.0))
            .or_else(|| target.strip_suffix("bb").map(|s| (s, 1.0)))
    };
    if let Some((s, basis)) = amount_and_basis {
        let mut parts = s.split('.');
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
        let decimal = digits(parts.next().unwrap_or_default())
            && parts.next().is_none_or(digits)
            && parts.next().is_none();
        if decimal && let Ok(value) = s.parse::<f64>() {
            let threshold = value * basis;
            if value > 0.0 && threshold.is_finite() && threshold > 0.0 {
                return Ok(threshold);
            }
        }
    }
    Err(invalid(
        "solver.stop.target",
        "expected a positive plain-decimal target: cash N%pot or Nbb, tournament N%prizes",
    ))
}

/// Stateless normalization hook. Parsing is also available through `Settings::parse`.
pub struct P1Sections;
impl ProductSections for P1Sections {
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
        for key in ["algorithm", "stop", "parallel"] {
            let item = solver.remove(key).expect("serialized solver section");
            let nested = item.into_table().expect("serialized solver table");
            solver.insert(key, toml_edit::Item::Table(nested));
        }
        Ok((solver, table(&settings.output)?))
    }
}

/// Common rules carry actor-specific constants from the original line.
#[derive(Clone, Debug)]
pub struct NlhStreetRules {
    pub rules: Vec<Rule<TreeVar>>,
    pub players: PerPlayer<PostflopPlayer>,
}

/// Lower the validated IR without I/O or allocating a game tree.
/// The u32 builder boundary is checked for the largest possible terminal pot.
pub fn lower(spot: &Spot, settings: &Settings) -> Result<PostflopConfig, SpotError> {
    if spot.product != Product::HuPostflop {
        return Err(SpotError::new(
            Code::NLH005,
            "spot",
            "P1 requires a HU postflop spot",
        ));
    }
    let oop = spot
        .context
        .oop
        .as_ref()
        .ok_or_else(|| invalid("spot", "missing OOP"))?;
    let ip = spot
        .context
        .ip
        .as_ref()
        .ok_or_else(|| invalid("spot", "missing IP"))?;
    let stack = spot
        .context
        .effective_stack
        .ok_or_else(|| invalid("spot", "missing effective stack"))?
        .0;
    let pot = spot.context.pot.0;
    if stack
        .checked_mul(2)
        .and_then(|s| s.checked_add(pot))
        .is_none_or(|s| s > u32::MAX as u64)
    {
        return Err(invalid(
            "spot",
            "pot plus twice the effective stack exceeds the P1 u32 milli-BB limit",
        ));
    }
    let street = |street, cap| -> Result<StreetTree, SpotError> {
        Ok(StreetTree {
            nlh_rules: Some(Box::new(NlhStreetRules {
                rules: spot
                    .tree
                    .compiled
                    .rules
                    .iter()
                    .filter(|r| r.street == street && street >= spot.context.street)
                    .cloned()
                    .collect(),
                players: PerPlayer::new(oop.clone(), ip.clone()),
            })),
            rules: Vec::new(),
            max_aggressive_actions: u32::try_from(cap)
                .map_err(|_| invalid("tree.max_aggressive_actions", "exceeds the P1 u32 limit"))?,
            include_allin: spot.tree.include_allin,
            allin_threshold: spot.tree.allin_threshold,
        })
    };
    Ok(PostflopConfig {
        board: spot.context.board.clone(),
        ranges: PerPlayer::new(
            spot.ranges[oop.seat].range.clone(),
            spot.ranges[ip.seat].range.clone(),
        ),
        pot: Chips(pot as u32),
        effective_stack: Chips(stack as u32),
        streets: PerStreet {
            flop: street(Street::Flop, spot.tree.max_aggressive_actions.flop)?,
            turn: street(Street::Turn, spot.tree.max_aggressive_actions.turn)?,
            river: street(Street::River, spot.tree.max_aggressive_actions.river)?,
        },
        min_bet: Chips(1000),
        iso_merging: settings.solver.iso_merging,
        track_node_info: true,
        preflop_aggressor: spot.context.previous_street_aggressor.and_then(|s| {
            if s == oop.seat {
                Some(Player::P0)
            } else if s == ip.seat {
                Some(Player::P1)
            } else {
                None
            }
        }),
    })
}

pub(crate) struct NlhContext<'a> {
    pub common: nlh::script::RuleContext,
    pub player: &'a PostflopPlayer,
}
impl VarSource<TreeVar> for NlhContext<'_> {
    fn value(&self, var: TreeVar) -> Value {
        use TreeVar::*;
        let facts = &self.player.preflop;
        match var {
            Position => Value::Text(position_text(&self.player.position)),
            Limpers => Value::Number(facts.limpers.into()),
            Flats => Value::Number(facts.flats.into()),
            Squeeze => Value::Bool(facts.squeeze),
            OpenColdCalls => Value::Number(facts.open_cold_calls.into()),
            PreflopParticipant => Value::Bool(facts.preflop_participant),
            InPositionToLastAggressor => Value::Bool(facts.in_position_to_last_aggressor),
            LastPreflopAggressorPosition => {
                Value::Text(position_text(&facts.last_preflop_aggressor_position))
            }
            _ => {
                use nlh::script::PostflopVar as P;
                let old = match var {
                    Aggressions => P::Aggressions,
                    Raises => P::Raises,
                    Unopened => P::Unopened,
                    Players => P::Players,
                    InPosition => P::InPosition,
                    Spr => P::Spr,
                    Pot => P::Pot,
                    ToCall => P::ToCall,
                    FacingPct => P::FacingPct,
                    Cbet => P::Cbet,
                    Donk => P::Donk,
                    BoardCards => P::BoardCards,
                    BoardSuits => P::BoardSuits,
                    BoardRanks => P::BoardRanks,
                    StraightRanks => P::StraightRanks,
                    Paired => P::Paired,
                    Monotone => P::Monotone,
                    TwoTone => P::TwoTone,
                    Rainbow => P::Rainbow,
                    FlushPossible => P::FlushPossible,
                    StraightPossible => P::StraightPossible,
                    HighCard => P::HighCard,
                    LowCard => P::LowCard,
                    _ => unreachable!("handled above"),
                };
                self.common.value(old)
            }
        }
    }
}
fn position_text(text: &str) -> &'static str {
    match text {
        "BTN" => "BTN",
        "SB" => "SB",
        "BB" => "BB",
        "CO" => "CO",
        "HJ" => "HJ",
        "LJ" => "LJ",
        "UTG" => "UTG",
        "UTG1" => "UTG1",
        "UTG2" => "UTG2",
        "" => "",
        _ => unreachable!("Spot IR has a canonical position"),
    }
}
