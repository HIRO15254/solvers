//! Interactive hand-line replay and legal moves, backed exclusively by NLH state.
use crate::{Code, LineAction, SpotError, Table, parse, replay};
use nlh::betting::{Action, BettingState, Move, SeatStatus};
use nlh::{MwChips, NoStreetPolicy, SeatId, Street};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineCursor {
    pub moves: Vec<String>,
    pub line: String,
    pub actions: Vec<LineAction>,
    pub street: Street,
    pub pot: f64,
    pub button: SeatId,
    pub seats: Vec<LineSeat>,
    pub complete: bool,
    pub board_cards: Option<usize>,
    pub players: usize,
    pub next: Option<LineNext>,
    pub terminal: bool,
}

/// Seat state in the table's fixed clockwise seat-ID order; all amounts are BB.
#[derive(Clone, Debug, Serialize)]
pub struct LineSeat {
    pub seat: SeatId,
    pub position: String,
    pub stack: f64,
    pub wager: f64,
    pub committed: f64,
    pub status: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineNext {
    pub seat: SeatId,
    pub position: String,
    pub street: Street,
    pub to_call: f64,
    pub stack: f64,
    pub wager: f64,
    pub options: Vec<LineOption>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LineOptionKind {
    Fold,
    Check,
    Call,
    Bet,
    Raise,
    AllIn,
}

#[derive(Clone, Debug, Serialize)]
pub struct LineOption {
    pub kind: LineOptionKind,
    pub token: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

fn bb(chips: MwChips) -> f64 {
    chips.0 as f64 / 1000.0
}

fn token(action: &Action) -> String {
    match action {
        Action::Fold => "f".into(),
        Action::Check => "x".into(),
        Action::Call { .. } => "c".into(),
        Action::BetTo { all_in: true, .. } | Action::RaiseTo { all_in: true, .. } => "a".into(),
        Action::BetTo { to, .. } => format!("b{}", replay::amount_text(*to)),
        Action::RaiseTo { to, .. } => format!("r{}", replay::amount_text(*to)),
    }
}

fn line_text(actions: &[LineAction]) -> String {
    let mut text = String::new();
    let mut previous = None;
    for action in actions {
        if matches!(action.action, Action::Fold) {
            continue;
        }
        if let Some(street) = previous {
            text.push_str(if street == action.street { ", " } else { " / " });
        }
        text.push_str(&action.position);
        text.push(' ');
        text.push_str(&token(&action.action));
        previous = Some(action.street);
    }
    text
}

fn indexed(index: usize, error: SpotError) -> SpotError {
    SpotError::new(
        Code::NLH004,
        "spot.line",
        format!("move {index}: {}", error.message),
    )
}

/// Replay explicit builder moves, or the config's strict `spot.line` when absent.
/// Only `[table]` is validated; all other settings may still be mid-edit.
/// All-in calls use `c`, just as the strict line grammar does; `a` is aggressive.
pub fn line_cursor(text: &str, moves: Option<&[String]>) -> Result<LineCursor, SpotError> {
    let (table, line) = parse::line_input(text, moves.is_none())?;
    let (state, actions) = if let Some(moves) = moves {
        let mut state = BettingState::new(&table.setup, &NoStreetPolicy)
            .map_err(|error| crate::error::value_error("table", error.to_string()))?;
        let mut actions = Vec::with_capacity(moves.len());
        for (index, spelling) in moves.iter().enumerate() {
            let result = if spelling == "f" {
                if state.street != Street::Preflop {
                    Err(SpotError::new(
                        Code::NLH004,
                        "spot.line",
                        "postflop fold cannot be represented in spot.line",
                    ))
                } else {
                    replay::implicit_fold(&mut state, &table, &mut actions)
                }
            } else {
                replay::parse_move(spelling)
                    .and_then(|mv| replay::apply(&mut state, &table, mv, false, &mut actions))
            };
            result.map_err(|error| indexed(index + 1, error))?;
        }
        (state, actions)
    } else {
        replay::line(&table, &line)?
    };
    let terminal = state.phase.is_terminal();
    let complete = !terminal
        && state.street != Street::Preflop
        && state.active_mask().len() >= 2
        && actions
            .last()
            .is_some_and(|action| action.street != state.street);
    let next = if terminal {
        None
    } else {
        state.actor().ok().map(|seat| next(&state, &table, seat))
    };
    Ok(LineCursor {
        moves: actions.iter().map(|action| token(&action.action)).collect(),
        line: line_text(&actions),
        actions,
        street: state.street,
        pot: bb(state.pot_size()),
        button: state.button,
        seats: state
            .seats
            .seats()
            .map(|seat| LineSeat {
                seat,
                position: table.positions[seat].clone(),
                stack: bb(state.seats[seat].remaining),
                wager: bb(state.current_wager(seat)),
                committed: bb(state.seats[seat].total_committed()),
                status: match state.seats[seat].status {
                    SeatStatus::Active => "active",
                    SeatStatus::Folded => "folded",
                    SeatStatus::AllIn => "allIn",
                },
            })
            .collect(),
        complete,
        board_cards: complete.then(|| state.street.index() + 2),
        players: state.non_folded_mask().len(),
        next,
        terminal,
    })
}

fn next(state: &BettingState, table: &Table, seat: SeatId) -> LineNext {
    let mut options = Vec::with_capacity(4);
    let mut add = |kind, token, amount, min, max| {
        options.push(LineOption {
            kind,
            token,
            amount,
            min,
            max,
        });
    };
    if state.street == Street::Preflop && state.resolve_move(Move::Fold).is_ok() {
        add(LineOptionKind::Fold, "f", None, None, None);
    }
    if state.resolve_move(Move::Check).is_ok() {
        add(LineOptionKind::Check, "x", None, None, None);
    }
    if let Ok(Action::Call { amount, .. }) = state.resolve_move(Move::Call) {
        add(LineOptionKind::Call, "c", Some(bb(amount)), None, None);
    }
    if state.can_raise(seat) {
        let minimum = state.minimum_full_target();
        let maximum = state.maximum_target(seat);
        // The maximum non-all-in target is one chip below the stack target.
        // Probe both bounds so the rules, not the GUI or this cursor, decide.
        if let Some(raw_maximum) = maximum.0.checked_sub(1) {
            let maximum = MwChips(raw_maximum);
            let (kind, token, mv) = if state.bet_to_match == MwChips::ZERO {
                (LineOptionKind::Bet, "b", Move::BetTo as fn(MwChips) -> Move)
            } else {
                (
                    LineOptionKind::Raise,
                    "r",
                    Move::RaiseTo as fn(MwChips) -> Move,
                )
            };
            if minimum <= maximum
                && state.resolve_move(mv(minimum)).is_ok()
                && state.resolve_move(mv(maximum)).is_ok()
            {
                add(kind, token, None, Some(bb(minimum)), Some(bb(maximum)));
            }
        }
    }
    if state.resolve_move(Move::AllIn).is_ok() {
        add(
            LineOptionKind::AllIn,
            "a",
            Some(bb(state.maximum_target(seat))),
            None,
            None,
        );
    }
    LineNext {
        seat,
        position: table.positions[seat].clone(),
        street: state.street,
        to_call: bb(state.amount_to_call(seat)),
        stack: bb(state.seats[seat].remaining),
        wager: bb(state.current_wager(seat)),
        options,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "[table]\nplayers = 6\nstack_bb = 100\n[spot]\nline = 'BTN r2.5, BB c'\n";

    fn explicit(config: &str, moves: &str) -> LineCursor {
        let tokens: Vec<String> = if moves.is_empty() {
            Vec::new()
        } else {
            moves.split(',').map(str::to_owned).collect()
        };
        line_cursor(config, Some(&tokens)).unwrap()
    }

    #[test]
    fn six_max_root_options() {
        let cursor = explicit(CONFIG, "");
        assert_eq!(cursor.line, "");
        assert!(!cursor.complete);
        assert!(!cursor.terminal);
        assert_eq!(cursor.board_cards, None);
        assert_eq!(cursor.players, 6);
        assert_eq!(cursor.pot, 1.5);
        assert_eq!(cursor.button, SeatId(0));
        assert_eq!(
            cursor
                .seats
                .iter()
                .map(|seat| seat.seat)
                .collect::<Vec<_>>(),
            (0..6).map(SeatId).collect::<Vec<_>>()
        );
        assert_eq!(cursor.seats[0].position, "BTN");
        assert_eq!(cursor.seats[1].wager, 0.5);
        assert_eq!(cursor.seats[1].stack, 99.5);
        assert_eq!(cursor.seats[2].wager, 1.0);
        assert_eq!(cursor.seats[2].stack, 99.0);
        assert!(cursor.seats.iter().all(|seat| seat.status == "active"));
        assert_eq!(
            cursor.seats.iter().map(|seat| seat.committed).sum::<f64>(),
            cursor.pot
        );
        let next = cursor.next.unwrap();
        assert_eq!(next.position, "UTG");
        assert_eq!(next.to_call, 1.0);
        assert_eq!(next.stack, 100.0);
        assert_eq!(next.wager, 0.0);
        assert_eq!(
            next.options
                .iter()
                .map(|option| (option.kind, option.token))
                .collect::<Vec<_>>(),
            [
                (LineOptionKind::Fold, "f"),
                (LineOptionKind::Call, "c"),
                (LineOptionKind::Raise, "r"),
                (LineOptionKind::AllIn, "a"),
            ]
        );
        assert_eq!(next.options[1].amount, Some(1.0));
        assert_eq!(next.options[2].min, Some(2.0));
        assert_eq!(next.options[2].max, Some(99.999));
        assert_eq!(next.options[3].amount, Some(100.0));
    }

    #[test]
    fn explicit_folds_transition_and_canonical_streets() {
        let partial = explicit(CONFIG, "f,f,f,r2.5,f");
        assert_eq!(partial.line, "BTN r2.5");
        assert_eq!(partial.players, 2);
        assert!(!partial.complete);
        assert_eq!(partial.next.unwrap().position, "BB");
        assert!(
            partial
                .actions
                .iter()
                .filter(|action| action.implicit)
                .all(|action| matches!(action.action, Action::Fold))
        );
        let flop = explicit(CONFIG, "f,f,f,r2.5,f,c");
        assert_eq!(flop.line, "BTN r2.5, BB c");
        assert_eq!(flop.street, Street::Flop);
        assert!(flop.complete);
        assert_eq!(flop.board_cards, Some(3));
        assert_eq!(flop.pot, 5.5);
        assert!(flop.seats.iter().all(|seat| seat.wager == 0.0));
        assert_eq!(flop.seats[0].stack, 97.5);
        assert_eq!(flop.seats[2].stack, 97.5);
        assert_eq!(flop.seats[0].status, "active");
        assert_eq!(flop.seats[2].status, "active");
        assert_eq!(flop.seats[1].status, "folded");
        assert!(flop.seats[3..].iter().all(|seat| seat.status == "folded"));
        assert_eq!(
            flop.seats.iter().map(|seat| seat.committed).sum::<f64>(),
            flop.pot
        );
        let next = flop.next.unwrap();
        assert_eq!(next.position, "BB");
        assert_eq!(next.stack, 97.5);
        assert_eq!(next.options[1].kind, LineOptionKind::Bet);
        assert_eq!(next.options[1].min, Some(1.0));
        assert_eq!(next.options[1].max, Some(97.499));
        let turn = explicit(CONFIG, "f,f,f,r2.5,f,c,x,b1.8,c");
        assert_eq!(turn.line, "BTN r2.5, BB c / BB x, BTN b1.8, BB c");
        assert_eq!(turn.street, Street::Turn);
        assert!(turn.complete);
        assert_eq!(turn.board_cards, Some(4));
        let partial_flop = explicit(CONFIG, "f,f,f,r2.5,f,c,x");
        assert_eq!(partial_flop.line, "BTN r2.5, BB c / BB x");
        assert!(!partial_flop.complete);
        assert_eq!(partial_flop.board_cards, None);
    }

    #[test]
    fn strict_config_round_trip_includes_implicit_folds() {
        let cursor = line_cursor(CONFIG, None).unwrap();
        assert_eq!(cursor.moves, ["f", "f", "f", "r2.5", "f", "c"]);
        assert_eq!(cursor.line, "BTN r2.5, BB c");
        assert_eq!(
            line_cursor(CONFIG, Some(&cursor.moves)).unwrap().line,
            cursor.line
        );
        let turn_config = CONFIG.replace("BTN r2.5, BB c", "BTN r2.5, BB c / BB x, BTN b1.8, BB c");
        let turn = line_cursor(&turn_config, None).unwrap();
        assert_eq!(
            turn.moves,
            ["f", "f", "f", "r2.5", "f", "c", "x", "b1.8", "c"]
        );
        assert_eq!(turn.line, "BTN r2.5, BB c / BB x, BTN b1.8, BB c");
    }

    #[test]
    fn errors_name_move_index_and_reason() {
        for (moves, index, reason) in [
            ("f,f,f,c,f,f", 6, "not facing a bet"),
            ("f,f,f,r2.5,f,c,f", 7, "postflop fold"),
            ("f,f,f,r2.50", 4, "shortest decimal"),
            ("f,f,f,r1.5", 4, "minimum full target"),
            ("x", 1, "use c"),
        ] {
            let tokens = moves.split(',').map(str::to_owned).collect::<Vec<_>>();
            let error = line_cursor(CONFIG, Some(&tokens)).unwrap_err();
            assert_eq!(error.code, Code::NLH004);
            assert!(error.message.contains(&format!("move {index}:")), "{error}");
            assert!(error.message.contains(reason), "{error}");
        }
    }

    #[test]
    fn all_in_call_uses_c_and_terminal_has_no_next() {
        let config = "[table]\nplayers = 2\nstack_bb = 100\n[table.stacks_bb]\nBB = 10\n";
        let cursor = explicit(config, "r20");
        let next = cursor.next.unwrap();
        assert_eq!(next.to_call, 19.0);
        assert_eq!(next.options.len(), 2);
        assert_eq!(next.options[1].kind, LineOptionKind::Call);
        assert_eq!(next.options[1].token, "c");
        assert_eq!(next.options[1].amount, Some(9.0));
        let terminal = explicit(config, "r20,c");
        assert!(terminal.terminal);
        assert!(!terminal.complete);
        assert!(terminal.next.is_none());
        assert_eq!(terminal.board_cards, None);
        assert_eq!(terminal.seats[1].status, "allIn");
        assert_eq!(terminal.seats[1].stack, 0.0);
        assert_eq!(
            terminal
                .seats
                .iter()
                .map(|seat| seat.committed)
                .sum::<f64>(),
            terminal.pot
        );
        let error = line_cursor(config, Some(&["r20".into(), "a".into()])).unwrap_err();
        assert!(error.message.contains("move 2:"));
        assert!(error.message.contains("use c"));
    }

    #[test]
    fn seat_commitments_include_antes_and_blinds() {
        let config = CONFIG.replace("stack_bb = 100", "stack_bb = 100\nante_bb = 0.125");
        let cursor = explicit(&config, "");
        assert_eq!(cursor.seats[0].committed, 0.125);
        assert_eq!(cursor.seats[0].wager, 0.0);
        assert_eq!(cursor.seats[1].committed, 0.625);
        assert_eq!(cursor.seats[1].wager, 0.5);
        assert_eq!(cursor.seats[2].committed, 1.125);
        assert_eq!(cursor.seats[2].wager, 1.0);
        assert_eq!(
            cursor.seats.iter().map(|seat| seat.committed).sum::<f64>(),
            cursor.pot
        );
    }

    #[test]
    fn only_table_and_optional_line_are_parsed() {
        let config = format!(
            "{CONFIG}board = 7\n[ranges]\nBTN = 42\n[tree]\nunknown = false\n[solver]\nmethod = 'bad'\n"
        );
        assert!(line_cursor(&config, None).unwrap().complete);
        let invalid_line = config.replace("line = 'BTN r2.5, BB c'", "line = 42");
        assert_eq!(explicit(&invalid_line, "").street, Street::Preflop);
        assert!(line_cursor(&invalid_line, None).is_err());
        let unfinished = format!("{CONFIG}board = \"unfinished\n[ranges]\nBTN = [\n");
        assert!(line_cursor(&unfinished, None).unwrap().complete);
        for (suffix, code, key) in [
            ("unknown = 1", Code::NLH002, "table.unknown"),
            ("sb_bb = 2", Code::NLH003, "table.sb_bb"),
            ("ante_bb = 0.0001", Code::NLH003, "table.ante_bb"),
        ] {
            let error = line_cursor(
                &format!("[table]\nplayers = 6\nstack_bb = 100\n{suffix}"),
                Some(&[]),
            )
            .unwrap_err();
            assert_eq!(error.code, code);
            assert_eq!(error.key.as_deref(), Some(key));
        }
    }
}
