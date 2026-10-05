//! Common-input P2 rule evaluation over the shared betting state.
use crate::betting::{BettingState, SeatStatus};
use crate::types::SeatId;
use nlh::position_name;
use nlh::script::{ActionKind, Condition, Dialect, Value, VarSource};

/// A common-input rule. The compiled condition is cached and never reparsed
/// during tree construction. Its text, effect and sizes define game identity.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NlhTreeRule {
    pub street: nlh::Street,
    pub condition: String,
    pub effect: nlh::script::Effect,
    pub action: Option<ActionKind>,
    pub sizes: Vec<nlh::SizeSpec>,
    #[serde(skip)]
    compiled: std::sync::OnceLock<Condition<spot::TreeVar>>,
}

impl NlhTreeRule {
    pub fn from_compiled(rule: nlh::script::Rule<spot::TreeVar>) -> Self {
        let condition = rule.condition.to_string();
        let compiled = std::sync::OnceLock::new();
        let _ = compiled.set(rule.condition);
        Self {
            street: rule.street,
            condition,
            effect: rule.effect,
            action: rule.action,
            sizes: rule.sizes,
            compiled,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), crate::config::ConfigError> {
        use crate::config::ConfigError;
        let invalid = |message: &str| ConfigError::TreeRule(message.into());
        // Typed lowering installs the compiler's condition directly. Display's
        // constant spelling is diagnostic text, so it must not be reparsed.
        use spot::TreeVar::*;
        let dialect = Dialect {
            vars: &[
                Aggressions,
                Raises,
                Unopened,
                Players,
                Position,
                InPosition,
                Spr,
                Pot,
                ToCall,
                FacingPct,
                Cbet,
                Donk,
                Limpers,
                Flats,
                Squeeze,
                OpenColdCalls,
                PreflopParticipant,
                InPositionToLastAggressor,
                LastPreflopAggressorPosition,
            ],
            ..spot::NLH_V1
        };
        let condition = match self.compiled.get() {
            Some(cached) if cached.to_string() == self.condition => cached.clone(),
            Some(_) => {
                return Err(invalid(
                    "condition changed after compilation; construct a new rule",
                ));
            }
            None => match self.condition.as_str() {
                "always" => Condition::Const(true),
                "never" => Condition::Const(false),
                _ => Condition::parse(&self.condition, &dialect)
                    .map_err(|e| ConfigError::TreeRule(e.to_string()))?,
            },
        };
        fn board(c: &Condition<spot::TreeVar>) -> bool {
            match c {
                Condition::Const(_) => false,
                Condition::Truth(v)
                | Condition::Compare { var: v, .. }
                | Condition::Member { var: v, .. } => v.is_board_var(),
                Condition::Not(c) => board(c),
                Condition::And(a, b) | Condition::Or(a, b) => board(a) || board(b),
            }
        }
        if board(&condition) {
            return Err(invalid("board variables are unsupported by P2"));
        }
        if self.effect == nlh::script::Effect::Checkdown {
            if self.action.is_some() || !self.sizes.is_empty() {
                return Err(invalid("checkdown must omit action and sizes"));
            }
        } else if self.action.is_none() {
            return Err(invalid("tree rule requires an action"));
        }
        for size in &self.sizes {
            crate::config::validate_size_spec(size)?;
        }
        let _ = self.compiled.set(condition);
        Ok(())
    }

    pub(crate) fn matches(&self, state: &BettingState, actor: SeatId) -> bool {
        self.street == state.street
            && self
                .compiled
                .get()
                .expect("validated common-input condition")
                .eval(&NlhContext { state, actor })
    }
}

/// Evaluate the product-neutral vocabulary on P2's own shared betting state.
/// Legacy history selectors retain their meanings; cbet/donk use the preceding street.
pub struct NlhContext<'a> {
    pub state: &'a BettingState,
    pub actor: SeatId,
}
impl VarSource<spot::TreeVar> for NlhContext<'_> {
    fn value(&self, var: spot::TreeVar) -> Value {
        use spot::TreeVar::*;
        let (state, actor) = (self.state, self.actor);
        match var {
            Position => Value::Text(position_name(actor, state.button, state.num_seats())),
            InPosition => Value::Bool(is_in_position(state, actor)),
            InPositionToLastAggressor => {
                Value::Bool(is_in_position_to_last_aggressor(state, actor))
            }
            LastPreflopAggressorPosition => Value::Text(last_preflop_aggressor_position(state)),
            PreflopParticipant => Value::Bool(state.preflop_participants.contains(actor)),
            OpenColdCalls => Value::Number(f64::from(state.preflop_open_cold_calls)),
            Players => Value::Number(state.non_folded_mask().len() as f64),
            Limpers => Value::Number(f64::from(state.preflop_limpers)),
            Flats => Value::Number(f64::from(state.preflop_flats)),
            Aggressions | Raises => Value::Number(f64::from(state.aggressive_actions)),
            Unopened => Value::Bool(state.aggressive_actions == 0),
            Squeeze => Value::Bool(
                state.street == nlh::Street::Preflop
                    && state.aggressive_actions > 0
                    && state.preflop_flats > 0,
            ),
            Spr => Value::Number(spr(state, actor)),
            Pot => Value::Number(state.pot_size().as_bb()),
            ToCall => Value::Number(state.amount_to_call(actor).as_bb()),
            FacingPct => Value::Number(if state.pot_size().raw() == 0 {
                0.0
            } else {
                state.amount_to_call(actor).raw() as f64 / state.pot_size().raw() as f64 * 100.0
            }),
            Cbet | Donk => Value::Bool(
                state.street != nlh::Street::Preflop
                    && state.aggressive_actions == 0
                    && state.previous_street_aggressor.is_some_and(|seat| {
                        if var == Cbet {
                            seat == actor
                        } else {
                            seat != actor
                        }
                    }),
            ),
            _ => unreachable!("P2 rejects board variables before evaluation"),
        }
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

/// Returns the fixed table position of the most recent preflop aggressor.
/// The value remains available after the street advances, so postflop rules
/// can key off the last preflop raiser. An unopened hand evaluates to empty text.
fn last_preflop_aggressor_position(state: &BettingState) -> &'static str {
    state
        .last_preflop_aggressor
        .map(|aggressor| position_name(aggressor, state.button, state.num_seats()))
        .unwrap_or("")
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
    use crate::betting::BettingMenu;
    use crate::config::{
        AbstractionConfig, AnteConfig, BettingConfig, BlindConfig, MultiwayConfig, RuleAction,
        RuleEffect, SeatConfig,
    };

    fn rule(
        condition: &str,
        street: nlh::Street,
        effect: RuleEffect,
        action: RuleAction,
    ) -> NlhTreeRule {
        let rule = NlhTreeRule::from_compiled(nlh::script::Rule {
            street,
            condition: Condition::parse(condition, &spot::NLH_V1).unwrap(),
            effect,
            action: Some(action),
            sizes: Vec::new(),
        });
        rule.validate().unwrap();
        rule
    }
    fn matches(rule: &NlhTreeRule, state: &BettingState, actor: SeatId) -> bool {
        rule.matches(state, actor)
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
        let state = BettingState::from_config(&config.validated().unwrap()).unwrap();
        let raise_rule = rule(
            "unopened && position in [\"UTG\", \"HJ\"] && players == 6 && spr > 1",
            nlh::Street::Preflop,
            RuleEffect::Replace,
            RuleAction::Raise,
        );
        assert!(matches(&raise_rule, &state, state.to_act.unwrap()));
        let squeeze_rule = rule(
            "!squeeze && flats == 0",
            nlh::Street::Preflop,
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
        let mut state = BettingState::from_config(&config.validated().unwrap()).unwrap();

        // In a six-handed table with BTN=0, fixed postflop order is
        // SB(1), BB(2), UTG(3), HJ(4), CO(5), BTN(0).
        state.last_preflop_aggressor = Some(SeatId(3));
        let in_position_rule = rule(
            "in_position_to_last_aggressor",
            nlh::Street::Preflop,
            RuleEffect::Remove,
            RuleAction::Call,
        );
        assert!(matches(&in_position_rule, &state, SeatId(5)));

        state.last_preflop_aggressor = Some(SeatId(0));
        let opener_position_rule = rule(
            "last_preflop_aggressor_position == \"BTN\"",
            nlh::Street::Preflop,
            RuleEffect::Remove,
            RuleAction::Call,
        );
        assert!(matches(&opener_position_rule, &state, SeatId(1)));
        state.street = crate::types::Street::Flop;
        let postflop_opener_position_rule = rule(
            "last_preflop_aggressor_position == \"BTN\"",
            nlh::Street::Flop,
            RuleEffect::Remove,
            RuleAction::Call,
        );
        assert!(matches(&postflop_opener_position_rule, &state, SeatId(1)));
        state.last_preflop_aggressor = None;
        let unopened_position_rule = rule(
            "last_preflop_aggressor_position == \"\"",
            nlh::Street::Flop,
            RuleEffect::Remove,
            RuleAction::Call,
        );
        assert!(matches(&unopened_position_rule, &state, SeatId(1)));
        state.street = crate::types::Street::Preflop;
        state.last_preflop_aggressor = Some(SeatId(0));
        for actor in [SeatId(1), SeatId(2)] {
            assert!(!matches(&in_position_rule, &state, actor));
        }

        state.street = crate::types::Street::Flop;
        let postflop_rule = rule(
            "in_position_to_last_aggressor",
            nlh::Street::Flop,
            RuleEffect::Remove,
            RuleAction::Call,
        );
        assert!(!matches(&postflop_rule, &state, SeatId(0)));
        state.street = crate::types::Street::Preflop;

        state.preflop_participants.insert(SeatId(3));
        state.preflop_open_cold_calls = 2;
        let participant_rule = rule(
            "preflop_participant && open_cold_calls == 2",
            nlh::Street::Preflop,
            RuleEffect::Remove,
            RuleAction::Call,
        );
        assert!(matches(&participant_rule, &state, SeatId(3)));

        let participant_only_rule = rule(
            "preflop_participant",
            nlh::Street::Preflop,
            RuleEffect::Remove,
            RuleAction::Call,
        );
        assert!(!matches(&participant_only_rule, &state, SeatId(4)));
    }
}
