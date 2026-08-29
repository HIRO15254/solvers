//! Multiway's tree-rule condition dialect and evaluator.
//!
//! This is a thin adapter over `cards::script`'s generic condition grammar
//! (`Vars`, `Condition<V>`, `Dialect<V>`, `VarSource<V>`) -- the same front
//! end postflop's `.tree` scripts use -- providing the fourteen variables
//! multiway's `when` strings and `.mwtree` scripts read. `TreeRule::compiled`
//! (`config.rs`) is what makes evaluation compile-once: a condition string
//! is parsed here exactly once, the first time it is checked (in practice,
//! at config validation, since `MultiwayConfig::validate` calls it on every
//! rule up front), and cached from then on -- `matches` below never
//! reparses a string per decision node the way the old flat scanner did.
//!
//! Reusing `cards::script` also retires three bugs the old hand-rolled
//! byte-position parser carried: it stripped comments with
//! `line.split('#')`, so a `#` inside a string literal truncated the line;
//! it cast `bytes[index] as char`, so a non-ASCII byte got mangled into
//! latin-1 instead of erroring; and its effect grammar only recognized
//! `checkdown` via a whole-body string equality check rather than a real
//! grammar rule.

use std::collections::BTreeMap;

use cards::SizeUnit;
use cards::script::{
    ActionKind, Condition, Dialect, Script, ScriptError, Value, VarKind, VarSource, Vars,
};

use crate::betting::{BettingState, SeatStatus};
use crate::config::{RuleStreet, TreeRule};
use crate::types::SeatId;

/// One named variable multiway's tree-rule conditions can read -- the same
/// fourteen the old `context_value` resolved, ported onto `cards::script`'s
/// generic condition grammar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MultiwayVar {
    Position,
    InPosition,
    InPositionToLastAggressor,
    PreflopParticipant,
    OpenColdCalls,
    Players,
    Limpers,
    Flats,
    Aggressions,
    Unopened,
    Squeeze,
    Cbet,
    Donk,
    Spr,
}

impl Vars for MultiwayVar {
    fn name(self) -> &'static str {
        use MultiwayVar::*;
        match self {
            Position => "position",
            InPosition => "in_position",
            InPositionToLastAggressor => "in_position_to_last_aggressor",
            PreflopParticipant => "preflop_participant",
            OpenColdCalls => "open_cold_calls",
            Players => "players",
            Limpers => "limpers",
            Flats => "flats",
            Aggressions => "aggressions",
            Unopened => "unopened",
            Squeeze => "squeeze",
            Cbet => "cbet",
            Donk => "donk",
            Spr => "spr",
        }
    }

    fn kind(self) -> VarKind {
        use MultiwayVar::*;
        match self {
            Position => VarKind::Text,
            InPosition
            | InPositionToLastAggressor
            | PreflopParticipant
            | Unopened
            | Squeeze
            | Cbet
            | Donk => VarKind::Bool,
            OpenColdCalls | Players | Limpers | Flats | Aggressions | Spr => VarKind::Number,
        }
    }
}

/// Every [`MultiwayVar`], in the order [`MULTIWAY`] exposes them.
const ALL_MULTIWAY_VARS: &[MultiwayVar] = &[
    MultiwayVar::Position,
    MultiwayVar::InPosition,
    MultiwayVar::InPositionToLastAggressor,
    MultiwayVar::PreflopParticipant,
    MultiwayVar::OpenColdCalls,
    MultiwayVar::Players,
    MultiwayVar::Limpers,
    MultiwayVar::Flats,
    MultiwayVar::Aggressions,
    MultiwayVar::Unopened,
    MultiwayVar::Squeeze,
    MultiwayVar::Cbet,
    MultiwayVar::Donk,
    MultiwayVar::Spr,
];

/// Multiway's tree-script dialect: every [`MultiwayVar`], all four streets
/// (unlike postflop, multiway rules can fire preflop), every [`ActionKind`]
/// (multiway rules do add/remove fold/check/call candidates, unlike
/// postflop's), and big-blind-denominated sizes.
pub(crate) static MULTIWAY: Dialect<MultiwayVar> = Dialect {
    vars: ALL_MULTIWAY_VARS,
    streets: &[
        ("preflop", cards::Street::Preflop),
        ("flop", cards::Street::Flop),
        ("turn", cards::Street::Turn),
        ("river", cards::Street::River),
    ],
    actions: &[
        ActionKind::Fold,
        ActionKind::Check,
        ActionKind::Call,
        ActionKind::Bet,
        ActionKind::Raise,
    ],
    unit: SizeUnit::Bb,
};

impl VarSource<MultiwayVar> for (&BettingState, SeatId) {
    fn value(&self, var: MultiwayVar) -> Value {
        let (state, actor) = *self;
        use MultiwayVar::*;
        match var {
            Position => Value::Text(position_name(actor, state.button, state.num_seats())),
            InPosition => Value::Bool(is_in_position(state, actor)),
            InPositionToLastAggressor => {
                Value::Bool(is_in_position_to_last_aggressor(state, actor))
            }
            PreflopParticipant => Value::Bool(state.preflop_participants.contains(actor)),
            OpenColdCalls => Value::Number(f64::from(state.preflop_open_cold_calls)),
            Players => Value::Number(state.non_folded_mask().len() as f64),
            Limpers => Value::Number(f64::from(state.preflop_limpers)),
            Flats => Value::Number(f64::from(state.preflop_flats)),
            Aggressions => Value::Number(f64::from(state.aggressive_actions)),
            Unopened => Value::Bool(state.aggressive_actions == 0),
            Squeeze => Value::Bool(
                state.street == crate::types::Street::Preflop
                    && state.aggressive_actions > 0
                    && state.preflop_flats > 0,
            ),
            Cbet => Value::Bool(
                state.street != crate::types::Street::Preflop
                    && state.aggressive_actions == 0
                    && state.last_preflop_aggressor == Some(actor),
            ),
            Donk => Value::Bool(
                state.street != crate::types::Street::Preflop
                    && state.aggressive_actions == 0
                    && state.last_preflop_aggressor.is_some()
                    && state.last_preflop_aggressor != Some(actor),
            ),
            Spr => Value::Number(spr(state, actor)),
        }
    }
}

/// Compiles one multiway condition string (a `[[..rules]] when = "..."`
/// value, or a rendered `.mwtree` rule condition -- see
/// `crate::config::TreeRule::compiled`) against [`MULTIWAY`].
pub(crate) fn compile(source: &str) -> Result<Condition<MultiwayVar>, ScriptError> {
    Condition::parse(source, &MULTIWAY)
}

/// Compiles a whole `.mwtree` script against [`MULTIWAY`], applying
/// `overrides` to any `param` it declares, and lowers the result straight to
/// [`TreeRule`]s -- the same `cards::script` front end (tokenizing,
/// substitution, nesting, `if`/`else`, `param`/`define`) postflop's `.tree`
/// scripts compile through, so this is the whole answer to "put multiway's
/// tree script on the same front end as postflop's". This is the only
/// caller of [`crate::config::TreeRule::from_compiled`]: each compiled
/// rule's condition is installed into the returned `TreeRule` directly, not
/// reparsed from `Display`-rendered text.
///
/// Every rule gets `priority` 100 (ties with every other script rule, so
/// `Vec<TreeRule>`'s sort-by-`(priority, source_order)` is a no-op and
/// source order -- the flattened script's own order -- decides, matching
/// `cards::script::Rule<V>`'s own "no priority field, source order is the
/// whole model") and `source_order` set to its position in the flattened
/// list, matching what the old flat scanner already did (it hardcoded the
/// same 100 for the same reason).
pub fn compile_script(
    source: &str,
    overrides: &BTreeMap<String, String>,
) -> Result<Vec<TreeRule>, ScriptError> {
    let script = Script::compile(source, overrides, &MULTIWAY)?;
    Ok(script
        .rules
        .into_iter()
        .enumerate()
        .map(|(index, rule)| {
            TreeRule::from_compiled(
                100,
                index as u32,
                to_rule_street(rule.street),
                rule.condition,
                rule.effect,
                rule.action,
                rule.sizes,
            )
        })
        .collect())
}

/// `cards::Street` has no `Postflop` catch-all (only the typed `[[..rules]]`
/// TOML surface's [`RuleStreet`] does, and `.mwtree` scripts dropped that
/// keyword -- see the module docs on `MULTIWAY`), so every variant maps
/// straight across.
fn to_rule_street(street: cards::Street) -> RuleStreet {
    match street {
        cards::Street::Preflop => RuleStreet::Preflop,
        cards::Street::Flop => RuleStreet::Flop,
        cards::Street::Turn => RuleStreet::Turn,
        cards::Street::River => RuleStreet::River,
    }
}

/// True when `rule` matches this decision node: its street gates first
/// (cheap, and checked outside the compiled condition since [`RuleStreet`]'s
/// `Postflop` catch-all has no equivalent single [`MultiwayVar`]), then its
/// condition, read from [`TreeRule::compiled`]'s cache rather than reparsed.
/// Infallible: every rule's condition was already compiled -- and would have
/// been rejected -- when the owning config was validated, so there is
/// nothing left to fail here (matching postflop's own `Condition::eval`).
pub(crate) fn matches(rule: &TreeRule, state: &BettingState, actor: SeatId) -> bool {
    rule.street.matches(state.street) && rule.compiled().eval(&(state, actor))
}

/// The identifier this seat plays under, computed from the button offset for
/// two through nine seats. A fixed small vocabulary, so every arm is
/// `&'static str` and nothing here allocates.
fn position_name(actor: SeatId, button: SeatId, seats: usize) -> &'static str {
    const LABELS: [&[&str]; 8] = [
        &["BTN", "BB"],
        &["BTN", "SB", "BB"],
        &["CO", "BTN", "SB", "BB"],
        &["HJ", "CO", "BTN", "SB", "BB"],
        &["UTG", "HJ", "CO", "BTN", "SB", "BB"],
        &["UTG", "LJ", "HJ", "CO", "BTN", "SB", "BB"],
        &["UTG", "UTG1", "LJ", "HJ", "CO", "BTN", "SB", "BB"],
        &["UTG", "UTG1", "UTG2", "LJ", "HJ", "CO", "BTN", "SB", "BB"],
    ];
    let labels = LABELS[seats - 2];
    let offset = (actor.index() + seats - button.index()) % seats;
    if seats == 2 {
        return labels[offset];
    }
    match offset {
        0 => "BTN",
        1 => "SB",
        2 => "BB",
        _ => labels[offset - 3],
    }
}

fn is_in_position(state: &BettingState, actor: SeatId) -> bool {
    if state.street == crate::types::Street::Preflop {
        return position_name(actor, state.button, state.num_seats()) == "BTN";
    }
    (0..state.num_seats())
        .map(|step| {
            state
                .button
                .advance(state.num_seats() - step, state.num_seats())
        })
        .find(|seat| state.seats[*seat].status != SeatStatus::Folded)
        == Some(actor)
}

fn is_in_position_to_last_aggressor(state: &BettingState, actor: SeatId) -> bool {
    if state.street != crate::types::Street::Preflop {
        return false;
    }
    let Some(aggressor) = state.last_preflop_aggressor else {
        return false;
    };
    if actor == aggressor {
        return false;
    }
    let seats = state.num_seats();
    let postflop_rank =
        |seat: SeatId| (seat.index() + seats - state.button.next(seats).index()) % seats;
    postflop_rank(actor) > postflop_rank(aggressor)
}

fn spr(state: &BettingState, actor: SeatId) -> f64 {
    let pot = state.pot_size().raw();
    if pot == 0 {
        return f64::INFINITY;
    }
    let opponent_max = state
        .seats
        .seats()
        .filter(|seat| *seat != actor && state.seats[*seat].status != SeatStatus::Folded)
        .map(|seat| state.seats[seat].remaining.raw())
        .max()
        .unwrap_or(0);
    state.seats[actor].remaining.raw().min(opponent_max) as f64 / pot as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AbstractionConfig, AnteConfig, BettingConfig, BlindConfig, MultiwayConfig, RuleAction,
        RuleEffect, RuleStreet, SeatConfig,
    };

    fn rule(
        condition: &str,
        street: RuleStreet,
        effect: RuleEffect,
        action: RuleAction,
    ) -> TreeRule {
        TreeRule::new(
            100,
            0,
            street,
            condition.to_string(),
            effect,
            Some(action),
            Vec::new(),
        )
    }

    #[test]
    fn selector_parser_supports_position_counts_flags_and_spr() {
        let config = MultiwayConfig {
            seats: (0..6)
                .map(|_| SeatConfig {
                    name: None,
                    stack_bb: 100.0,
                    range: String::new(),
                    betting: None,
                })
                .collect(),
            button: SeatId(0),
            blinds: BlindConfig::default(),
            ante: AnteConfig::None,
            betting: BettingConfig::default(),
            forced_bets: None,
            abstraction: AbstractionConfig::default(),
        };
        let state = BettingState::new(&config.validated().unwrap()).unwrap();
        let raise_rule = rule(
            "unopened && position in [\"UTG\", \"HJ\"] && players == 6 && spr > 1",
            RuleStreet::Preflop,
            RuleEffect::Replace,
            RuleAction::Raise,
        );
        assert!(matches(&raise_rule, &state, state.to_act.unwrap()));
        let squeeze_rule = rule(
            "!squeeze && flats == 0",
            RuleStreet::Preflop,
            RuleEffect::Replace,
            RuleAction::Raise,
        );
        assert!(matches(&squeeze_rule, &state, state.to_act.unwrap()));
    }

    #[test]
    fn new_preflop_selectors_track_pairwise_position_participation_and_cold_calls() {
        let config = MultiwayConfig {
            seats: (0..6)
                .map(|_| SeatConfig {
                    name: None,
                    stack_bb: 100.0,
                    range: String::new(),
                    betting: None,
                })
                .collect(),
            button: SeatId(0),
            blinds: BlindConfig::default(),
            ante: AnteConfig::None,
            betting: BettingConfig::default(),
            forced_bets: None,
            abstraction: AbstractionConfig::default(),
        };
        let mut state = BettingState::new(&config.validated().unwrap()).unwrap();

        // In a six-handed table with BTN=0, fixed postflop order is
        // SB(1), BB(2), UTG(3), HJ(4), CO(5), BTN(0).
        state.last_preflop_aggressor = Some(SeatId(3));
        let in_position_rule = rule(
            "in_position_to_last_aggressor",
            RuleStreet::Preflop,
            RuleEffect::Remove,
            RuleAction::Call,
        );
        assert!(matches(&in_position_rule, &state, SeatId(5)));

        state.last_preflop_aggressor = Some(SeatId(0));
        for actor in [SeatId(1), SeatId(2)] {
            assert!(!matches(&in_position_rule, &state, actor));
        }

        state.street = crate::types::Street::Flop;
        let postflop_rule = rule(
            "in_position_to_last_aggressor",
            RuleStreet::Postflop,
            RuleEffect::Remove,
            RuleAction::Call,
        );
        assert!(!matches(&postflop_rule, &state, SeatId(0)));
        state.street = crate::types::Street::Preflop;

        state.preflop_participants.insert(SeatId(3));
        state.preflop_open_cold_calls = 2;
        let participant_rule = rule(
            "preflop_participant && open_cold_calls == 2",
            RuleStreet::Preflop,
            RuleEffect::Remove,
            RuleAction::Call,
        );
        assert!(matches(&participant_rule, &state, SeatId(3)));

        let participant_only_rule = rule(
            "preflop_participant",
            RuleStreet::Preflop,
            RuleEffect::Remove,
            RuleAction::Call,
        );
        assert!(!matches(&participant_only_rule, &state, SeatId(4)));
    }
}
