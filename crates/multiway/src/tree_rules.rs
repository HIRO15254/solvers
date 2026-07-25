use crate::betting::{BettingState, SeatStatus};
use crate::config::TreeRule;
use crate::types::SeatId;

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Bool(bool),
    Number(f64),
    Text(String),
}

pub(crate) fn matches(
    rule: &TreeRule,
    state: &BettingState,
    actor: SeatId,
) -> Result<bool, String> {
    if !rule.street.matches(state.street) {
        return Ok(false);
    }
    let mut parser = Parser {
        source: rule.condition.as_bytes(),
        position: 0,
        state,
        actor,
    };
    let matched = parser.parse_or()?;
    parser.skip_space();
    if parser.position != parser.source.len() {
        return Err(format!(
            "unexpected token at byte {} in tree rule condition",
            parser.position
        ));
    }
    Ok(matched)
}
pub(crate) fn validate_condition(source: &str) -> Result<(), String> {
    use crate::config::{
        AbstractionConfig, AnteConfig, BettingConfig, BlindConfig, MultiwayConfig, RuleEffect,
        RuleStreet, SeatConfig,
    };
    let config = MultiwayConfig {
        seats: (0..2)
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
    }
    .validated()
    .map_err(|error| error.to_string())?;
    let state = BettingState::new(&config).map_err(|error| error.to_string())?;
    let rule = TreeRule {
        priority: 0,
        source_order: 0,
        street: RuleStreet::Preflop,
        condition: source.to_owned(),
        effect: RuleEffect::Checkdown,
        action: None,
        sizes: Vec::new(),
    };
    matches(
        &rule,
        &state,
        state.to_act.expect("heads-up root has actor"),
    )
    .map(|_| ())
}

struct Parser<'a> {
    source: &'a [u8],
    position: usize,
    state: &'a BettingState,
    actor: SeatId,
}

impl Parser<'_> {
    fn parse_or(&mut self) -> Result<bool, String> {
        let mut value = self.parse_and()?;
        while self.consume("||") {
            let right = self.parse_and()?;
            value = value || right;
        }
        Ok(value)
    }

    fn parse_and(&mut self) -> Result<bool, String> {
        let mut value = self.parse_unary()?;
        while self.consume("&&") {
            let right = self.parse_unary()?;
            value = value && right;
        }
        Ok(value)
    }

    fn parse_unary(&mut self) -> Result<bool, String> {
        if self.consume("!") {
            return Ok(!self.parse_unary()?);
        }
        if self.consume("(") {
            let value = self.parse_or()?;
            if !self.consume(")") {
                return Err("missing ')' in tree rule condition".into());
            }
            return Ok(value);
        }
        self.parse_predicate()
    }

    fn parse_predicate(&mut self) -> Result<bool, String> {
        let identifier = self.identifier()?;
        let left = self.context_value(&identifier)?;
        if self.consume_keyword("in") {
            return self.parse_membership(left);
        }
        if let Some(operator) = self.comparison_operator() {
            let right = self.literal()?;
            return compare(left, right, operator);
        }
        match left {
            Value::Bool(value) => Ok(value),
            _ => Err(format!(
                "tree rule value {identifier:?} requires a comparison or 'in'"
            )),
        }
    }

    fn parse_membership(&mut self, left: Value) -> Result<bool, String> {
        if !self.consume("[") {
            return Err("tree rule 'in' requires an array".into());
        }
        let mut found = false;
        loop {
            if self.consume("]") {
                return Ok(found);
            }
            let candidate = self.literal()?;
            found |= left == candidate;
            if self.consume("]") {
                return Ok(found);
            }
            if !self.consume(",") {
                return Err("tree rule array requires ',' or ']'".into());
            }
        }
    }

    fn comparison_operator(&mut self) -> Option<&'static str> {
        ["<=", ">=", "==", "!=", "<", ">"]
            .into_iter()
            .find(|operator| self.consume(operator))
    }

    fn literal(&mut self) -> Result<Value, String> {
        self.skip_space();
        if self.source.get(self.position) == Some(&b'\"') {
            self.position += 1;
            let start = self.position;
            while self
                .source
                .get(self.position)
                .is_some_and(|byte| *byte != b'\"')
            {
                self.position += 1;
            }
            if self.source.get(self.position) != Some(&b'\"') {
                return Err("unterminated string in tree rule condition".into());
            }
            let text = std::str::from_utf8(&self.source[start..self.position])
                .map_err(|_| "tree rule condition is not UTF-8".to_string())?
                .to_owned();
            self.position += 1;
            return Ok(Value::Text(text));
        }
        let start = self.position;
        if self.source.get(self.position) == Some(&b'-') {
            self.position += 1;
        }
        while self
            .source
            .get(self.position)
            .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'.')
        {
            self.position += 1;
        }
        if self.position > start {
            let number = std::str::from_utf8(&self.source[start..self.position])
                .map_err(|_| "tree rule condition is not UTF-8".to_string())?
                .parse::<f64>()
                .map_err(|_| "invalid number in tree rule condition".to_string())?;
            if !number.is_finite() {
                return Err("tree rule number must be finite".into());
            }
            return Ok(Value::Number(number));
        }
        let word = self.identifier()?;
        match word.as_str() {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Ok(Value::Text(word)),
        }
    }

    fn context_value(&self, identifier: &str) -> Result<Value, String> {
        let active_players = self.state.non_folded_mask().len();
        Ok(match identifier {
            "position" => Value::Text(position_name(
                self.actor,
                self.state.button,
                self.state.num_seats(),
            )),
            "in_position" => Value::Bool(is_in_position(self.state, self.actor)),
            "in_position_to_last_aggressor" => {
                Value::Bool(is_in_position_to_last_aggressor(self.state, self.actor))
            }
            "preflop_participant" => {
                Value::Bool(self.state.preflop_participants.contains(self.actor))
            }
            "open_cold_calls" => Value::Number(f64::from(self.state.preflop_open_cold_calls)),
            "players" => Value::Number(active_players as f64),
            "limpers" => Value::Number(f64::from(self.state.preflop_limpers)),
            "flats" => Value::Number(f64::from(self.state.preflop_flats)),
            "aggressions" => Value::Number(f64::from(self.state.aggressive_actions)),
            "unopened" => Value::Bool(self.state.aggressive_actions == 0),
            "squeeze" => Value::Bool(
                self.state.street == crate::types::Street::Preflop
                    && self.state.aggressive_actions > 0
                    && self.state.preflop_flats > 0,
            ),
            "cbet" => Value::Bool(
                self.state.street != crate::types::Street::Preflop
                    && self.state.aggressive_actions == 0
                    && self.state.last_preflop_aggressor == Some(self.actor),
            ),
            "donk" => Value::Bool(
                self.state.street != crate::types::Street::Preflop
                    && self.state.aggressive_actions == 0
                    && self.state.last_preflop_aggressor.is_some()
                    && self.state.last_preflop_aggressor != Some(self.actor),
            ),
            "spr" => Value::Number(spr(self.state, self.actor)),
            other => return Err(format!("unknown tree rule identifier {other:?}")),
        })
    }

    fn identifier(&mut self) -> Result<String, String> {
        self.skip_space();
        let start = self.position;
        while self
            .source
            .get(self.position)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'-')
        {
            self.position += 1;
        }
        if start == self.position {
            return Err(format!(
                "expected tree rule expression at byte {}",
                self.position
            ));
        }
        std::str::from_utf8(&self.source[start..self.position])
            .map(str::to_owned)
            .map_err(|_| "tree rule condition is not UTF-8".to_string())
    }

    fn consume_keyword(&mut self, keyword: &str) -> bool {
        let before = self.position;
        if !self.consume(keyword) {
            return false;
        }
        if self
            .source
            .get(self.position)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            self.position = before;
            false
        } else {
            true
        }
    }

    fn consume(&mut self, token: &str) -> bool {
        self.skip_space();
        if self.source[self.position..].starts_with(token.as_bytes()) {
            self.position += token.len();
            true
        } else {
            false
        }
    }

    fn skip_space(&mut self) {
        while self
            .source
            .get(self.position)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.position += 1;
        }
    }
}

fn compare(left: Value, right: Value, operator: &str) -> Result<bool, String> {
    Ok(match (left, right) {
        (Value::Number(left), Value::Number(right)) => match operator {
            "<=" => left <= right,
            ">=" => left >= right,
            "==" => left == right,
            "!=" => left != right,
            "<" => left < right,
            ">" => left > right,
            _ => unreachable!(),
        },
        (Value::Text(left), Value::Text(right)) => match operator {
            "==" => left == right,
            "!=" => left != right,
            _ => return Err("strings in tree rules support only == and !=".into()),
        },
        (Value::Bool(left), Value::Bool(right)) => match operator {
            "==" => left == right,
            "!=" => left != right,
            _ => return Err("booleans in tree rules support only == and !=".into()),
        },
        _ => return Err("tree rule comparison operands have different types".into()),
    })
}

fn position_name(actor: SeatId, button: SeatId, seats: usize) -> String {
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
        return labels[offset].into();
    }
    match offset {
        0 => "BTN".into(),
        1 => "SB".into(),
        2 => "BB".into(),
        _ => labels[offset - 3].into(),
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
        let rule = TreeRule {
            priority: 100,
            source_order: 0,
            street: RuleStreet::Preflop,
            condition: "unopened && position in [\"UTG\", \"HJ\"] && players == 6 && spr > 1"
                .into(),
            effect: RuleEffect::Replace,
            action: Some(RuleAction::Raise),
            sizes: Vec::new(),
        };
        assert!(matches(&rule, &state, state.to_act.unwrap()).unwrap());
        assert!(
            matches(
                &TreeRule {
                    condition: "!squeeze && flats == 0".into(),
                    ..rule
                },
                &state,
                state.to_act.unwrap(),
            )
            .unwrap()
        );
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
        let base_rule = TreeRule {
            priority: 0,
            source_order: 0,
            street: RuleStreet::Preflop,
            condition: String::new(),
            effect: RuleEffect::Remove,
            action: Some(RuleAction::Call),
            sizes: Vec::new(),
        };

        // In a six-handed table with BTN=0, fixed postflop order is
        // SB(1), BB(2), UTG(3), HJ(4), CO(5), BTN(0).
        state.last_preflop_aggressor = Some(SeatId(3));
        assert!(
            matches(
                &TreeRule {
                    condition: "in_position_to_last_aggressor".into(),
                    ..base_rule.clone()
                },
                &state,
                SeatId(5),
            )
            .unwrap()
        );
        state.last_preflop_aggressor = Some(SeatId(0));
        for actor in [SeatId(1), SeatId(2)] {
            assert!(
                !matches(
                    &TreeRule {
                        condition: "in_position_to_last_aggressor".into(),
                        ..base_rule.clone()
                    },
                    &state,
                    actor,
                )
                .unwrap()
            );
        }
        state.street = crate::types::Street::Flop;
        assert!(
            !matches(
                &TreeRule {
                    street: RuleStreet::Postflop,
                    condition: "in_position_to_last_aggressor".into(),
                    ..base_rule.clone()
                },
                &state,
                SeatId(0),
            )
            .unwrap()
        );
        state.street = crate::types::Street::Preflop;

        state.preflop_participants.insert(SeatId(3));
        state.preflop_open_cold_calls = 2;
        assert!(
            matches(
                &TreeRule {
                    condition: "preflop_participant && open_cold_calls == 2".into(),
                    ..base_rule.clone()
                },
                &state,
                SeatId(3),
            )
            .unwrap()
        );
        assert!(
            !matches(
                &TreeRule {
                    condition: "preflop_participant".into(),
                    ..base_rule
                },
                &state,
                SeatId(4),
            )
            .unwrap()
        );
    }
}
