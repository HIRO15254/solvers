//! Strict hand-line spelling and menu-independent replay through the NLH rules.
use crate::*;
use nlh::betting::{BettingState, IllegalMove, Move, SeatStatus};
use nlh::{Card, MwChips, NoStreetPolicy, SeatId, Street};

fn line_error(message: impl Into<String>) -> SpotError {
    SpotError::new(Code::NLH004, "spot.line", message)
}

pub(crate) fn amount_text(amount: MwChips) -> String {
    let whole = amount.0 / 1000;
    let fraction = amount.0 % 1000;
    if fraction == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{fraction:03}")
            .trim_end_matches('0')
            .to_owned()
    }
}

fn move_text(mv: Move) -> String {
    match mv {
        Move::Fold => "implicit fold (omit this player)".into(),
        Move::Check => "x".into(),
        Move::Call => "c".into(),
        Move::BetTo(v) => format!("b{}", amount_text(v)),
        Move::RaiseTo(v) => format!("r{}", amount_text(v)),
        Move::AllIn => "a".into(),
    }
}

fn parse_move(text: &str) -> Result<Move, SpotError> {
    // Recognize common aliases only to explain their canonical spelling.
    let lower = text.to_ascii_lowercase();
    if lower != text
        && let Ok(mv) = parse_move(&lower)
    {
        return Err(line_error(format!(
            "use {} (lowercase move symbols)",
            move_text(mv)
        )));
    }
    let alias = match text {
        "check" => Some(Move::Check),
        "call" => Some(Move::Call),
        "all-in" | "allin" => Some(Move::AllIn),
        "fold" => Some(Move::Fold),
        _ => [("bet ", true), ("raise ", false)]
            .into_iter()
            .find_map(|(prefix, bet)| {
                let number = text.strip_prefix(prefix)?;
                let amount = crate::parse::decimal_chips(number, "spot.line", false).ok()?;
                Some(if bet {
                    Move::BetTo(amount)
                } else {
                    Move::RaiseTo(amount)
                })
            }),
    };
    if let Some(mv) = alias {
        return Err(line_error(format!(
            "use {} instead of a word form",
            move_text(mv)
        )));
    }
    match text {
        "x" => Ok(Move::Check),
        "c" => Ok(Move::Call),
        "a" => Ok(Move::AllIn),
        "f" => Err(line_error(
            "fold is implicit; omit this player instead of writing f",
        )),
        _ => {
            let (prefix, number) = text.split_at(text.chars().next().map_or(0, char::len_utf8));
            if prefix != "b" && prefix != "r" {
                return Err(line_error(
                    "use position and x, c, b<amount>, r<amount>, or a; separate actions with ', ' and streets with ' / '",
                ));
            }
            if let Some(unsigned) = number.strip_prefix('+')
                && let Ok(amount) = crate::parse::decimal_chips(unsigned, "spot.line", false)
            {
                return Err(line_error(format!(
                    "use {prefix}{}, without a sign",
                    amount_text(amount)
                )));
            }
            let amount = crate::parse::decimal_chips(number, "spot.line", false)
                .map_err(|e| line_error(e.message))?;
            let canonical = amount_text(amount);
            if number != canonical {
                return Err(line_error(format!(
                    "use {prefix}{canonical}, the shortest decimal BB spelling"
                )));
            }
            Ok(if prefix == "b" {
                Move::BetTo(amount)
            } else {
                Move::RaiseTo(amount)
            })
        }
    }
}

fn apply(
    state: &mut BettingState,
    table: &Table,
    mv: Move,
    implicit: bool,
    actions: &mut Vec<LineAction>,
) -> Result<(), SpotError> {
    let seat = state.actor().map_err(|e| line_error(e.to_string()))?;
    let action = state.resolve_move(mv).map_err(|e| {
        let correct = match &e {
            IllegalMove::WrongMove {
                correct: Move::BetTo(to) | Move::RaiseTo(to),
                ..
            } if *to == state.maximum_target(seat) => Some("a".into()),
            IllegalMove::WrongMove { correct, .. }
            | IllegalMove::MustBeAllIn { correct, .. }
            | IllegalMove::AllInIsCall { correct, .. } => Some(move_text(*correct)),
            _ => None,
        };
        line_error(match correct {
            Some(correct) => format!("{}: {e}; use {correct}", table.positions[seat]),
            None => format!("{}: {e}", table.positions[seat]),
        })
    })?;
    actions.push(LineAction {
        street: state.street,
        seat,
        position: table.positions[seat].clone(),
        action: action.clone(),
        implicit,
    });
    state
        .apply_action(action, &NoStreetPolicy)
        .map_err(|e| line_error(e.to_string()))
}

fn implicit_fold(
    state: &mut BettingState,
    table: &Table,
    actions: &mut Vec<LineAction>,
) -> Result<(), SpotError> {
    let actor = state.actor().map_err(|e| line_error(e.to_string()))?;
    if state.amount_to_call(actor) == MwChips::ZERO {
        return Err(line_error(format!(
            "{} is not facing a bet; write {} x (or a legal raise), no implicit fold is allowed",
            table.positions[actor], table.positions[actor]
        )));
    }
    apply(state, table, Move::Fold, true, actions)
}

pub(crate) fn line(
    table: &Table,
    text: &str,
) -> Result<(BettingState, Vec<LineAction>), SpotError> {
    let mut state = BettingState::new(&table.setup, &NoStreetPolicy)
        .map_err(|e| crate::error::value_error("table", e.to_string()))?;
    let mut actions = Vec::new();
    if text.is_empty() {
        return Ok((state, actions));
    }
    for (index, group) in text.split(" / ").enumerate() {
        let expected = [Street::Preflop, Street::Flop, Street::Turn, Street::River]
            .get(index)
            .copied()
            .ok_or_else(|| line_error("no street follows river"))?;
        if group.is_empty() {
            return Err(line_error(
                "a line cannot end with ' / ' or contain an empty street",
            ));
        }
        if state.phase.is_terminal() || state.street != expected {
            return Err(line_error(
                "' / ' must occur exactly after a closed street with a following decision",
            ));
        }
        for token in group.split(", ") {
            let (position, spelling) = token.split_once(' ').ok_or_else(|| {
                line_error("use POSITION move, with ', ' between actions and ' / ' between streets")
            })?;
            let seat = table
                .positions
                .seats()
                .find(|s| table.positions[*s] == position)
                .ok_or_else(|| {
                    let upper = position.to_ascii_uppercase();
                    let correct = if table.positions.iter().any(|p| p == &upper) {
                        format!("use {upper}")
                    } else {
                        format!(
                            "use one of {}",
                            table
                                .positions
                                .iter()
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    };
                    line_error(format!("unknown position {position:?}; {correct}"))
                })?;
            let mv = parse_move(spelling)?;
            if state.street != expected || state.phase.is_terminal() {
                return Err(line_error(
                    "street is closed; write ' / ' before the next street's action",
                ));
            }
            if expected == Street::Preflop {
                while state.to_act != Some(seat)
                    && state.street == expected
                    && !state.phase.is_terminal()
                {
                    implicit_fold(&mut state, table, &mut actions)?;
                }
            }
            if state.to_act != Some(seat) || state.street != expected || state.phase.is_terminal() {
                return Err(line_error(format!(
                    "{position} cannot act here; follow the street's action order"
                )));
            }
            apply(&mut state, table, mv, false, &mut actions)?;
        }
        if expected == Street::Preflop {
            while state.street == expected && !state.phase.is_terminal() {
                implicit_fold(&mut state, table, &mut actions)?;
            }
        } else if state.street == expected && !state.phase.is_terminal() {
            return Err(line_error(
                "postflop street must be closed at the end of the line or before ' / '",
            ));
        }
    }
    Ok((state, actions))
}

pub(crate) fn board(text: Option<&str>) -> Result<Vec<Card>, SpotError> {
    let Some(text) = text else {
        return Ok(Vec::new());
    };
    let mut cards = Vec::new();
    for token in text.split(' ') {
        let card: Card = token.parse().map_err(|e: nlh::ParseCardError| {
            crate::error::value_error("spot.board", e.to_string())
        })?;
        if card.to_string() != token || cards.contains(&card) {
            return Err(crate::error::value_error(
                "spot.board",
                "use distinct cards in strict 'Ks 7h 2d' format",
            ));
        }
        cards.push(card);
    }
    if !(3..=5).contains(&cards.len()) {
        return Err(crate::error::value_error(
            "spot.board",
            "board must have 3 through 5 cards",
        ));
    }
    Ok(cards)
}

pub(crate) fn product(
    state: &BettingState,
    line: &str,
    board: &[Card],
) -> Result<Product, SpotError> {
    let no_decision = || {
        SpotError::new(
            Code::NLH005,
            "spot",
            "no decision left: hand has ended or fewer than two postflop players can still act",
        )
    };
    if state.phase.is_terminal() {
        return Err(no_decision());
    }
    if line.is_empty() && board.is_empty() {
        return Ok(Product::MultiwayPreflop);
    }
    if state.active_mask().len() < 2 {
        return Err(no_decision());
    }
    if board.is_empty() {
        return Err(SpotError::new(
            Code::NLH005,
            "spot",
            "line without board: starting partway through preflop is unsupported",
        ));
    }
    if line.is_empty() || board.len() != state.street.index() + 2 {
        return Err(SpotError::new(
            Code::NLH004,
            "spot.board",
            format!(
                "board count must match closed streets: expected {} cards after the line",
                state.street.index() + 2
            ),
        ));
    }
    if state.non_folded_mask().len() >= 3 {
        return Err(SpotError::new(
            Code::NLH005,
            "spot",
            "3+ players remain: multiway postflop is unsupported",
        ));
    }
    Ok(Product::HuPostflop)
}

pub(crate) fn context(
    table: &Table,
    state: &BettingState,
    board: Vec<Card>,
    actions: Vec<LineAction>,
) -> Result<StartState, SpotError> {
    let refunds = if actions.is_empty() {
        nlh::SeatVec::new_unchecked(vec![MwChips::ZERO; state.num_seats()])
    } else {
        nlh::settlement::build_pots(state)
            .map_err(|e| line_error(e.to_string()))?
            .refunds
    };
    let players: Vec<_> = (1..=state.num_seats())
        .map(|i| state.button.advance(i, state.num_seats()))
        .filter(|s| state.seats[*s].status != SeatStatus::Folded)
        .collect();
    let player = |seat: SeatId| PostflopPlayer {
        seat,
        position: table.positions[seat].clone(),
        preflop: PreflopFacts {
            limpers: state.preflop_limpers,
            flats: state.preflop_flats,
            squeeze: false,
            open_cold_calls: state.preflop_open_cold_calls,
            preflop_participant: state.preflop_participants.contains(seat),
            in_position_to_last_aggressor: false,
            last_preflop_aggressor_position: state
                .last_preflop_aggressor
                .map(|s| table.positions[s].clone())
                .unwrap_or_default(),
        },
    };
    let postflop = state.street != Street::Preflop && players.len() == 2;
    let seats: Vec<_> = state
        .seats
        .seats()
        .map(|s| StartSeat {
            seat: s,
            position: table.positions[s].clone(),
            starting_stack: state.seats[s].starting_stack,
            remaining_stack: state.seats[s].remaining + refunds[s],
            total_contribution: state.seats[s].total_committed() - refunds[s],
            folded: state.seats[s].status == SeatStatus::Folded,
            refund: refunds[s],
        })
        .collect();
    Ok(StartState {
        street: state.street,
        board,
        pot: seats
            .iter()
            .fold(MwChips::ZERO, |sum, s| sum + s.total_contribution),
        folded_seats: seats.iter().filter(|s| s.folded).map(|s| s.seat).collect(),
        oop: postflop.then(|| player(players[0])),
        ip: postflop.then(|| player(players[1])),
        effective_stack: postflop.then(|| {
            seats[players[0].index()]
                .remaining_stack
                .min(seats[players[1].index()].remaining_stack)
        }),
        previous_street_aggressor: state.previous_street_aggressor,
        seats,
        actions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nlh::betting::Action;
    #[test]
    fn short_allin_call_refunds_uncalled_wager() {
        let text = "schema = 'solvers.nlh/v1'\n[table]\nplayers = 2\nstack_bb = 100\n[table.stacks_bb]\nBB = 10";
        let doc = Document::parse(text, std::path::Path::new("config.toml")).unwrap();
        let (state, actions) = line(&doc.spot.table, "BTN r20, BB c").unwrap();
        assert!(matches!(
            actions[1].action,
            Action::Call { all_in: true, .. }
        ));
        let context = context(&doc.spot.table, &state, Vec::new(), actions).unwrap();
        assert_eq!(context.pot, MwChips(20_000));
        assert_eq!(context.seats[0].refund, MwChips(10_000));
        assert_eq!(context.seats[0].remaining_stack, MwChips(90_000));
        assert_eq!(context.seats[0].total_contribution, MwChips(10_000));
        assert_eq!(
            product(&state, "BTN r20, BB c", &board(Some("Ks 7h 2d")).unwrap())
                .unwrap_err()
                .code,
            Code::NLH005
        );
    }
}
