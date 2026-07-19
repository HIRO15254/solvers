//! Parser for GGPoker hand-history files: a sequence of hand blocks,
//! separated by blank lines, each describing one dealt hand start to
//! finish.

use cards::{Card, Street};

use crate::model::{
    Action, ActionRecord, DateTime, GameType, Hand, ParseError, PotSummary, Seat, SeatOutcome,
    SeatResult,
};
use crate::util::{
    parse_amount, parse_bracket_group, parse_datetime, parse_leading_paren_amount, strip_all_in,
};

/// Parses every hand in `text`. Hands are returned in file order, which for
/// this corpus is **newest first** (descending); callers that need
/// chronological order should sort (see `load::load_dir`, which does this
/// when merging into a [`crate::Tournament`]).
pub fn parse_hand_history(text: &str) -> Result<Vec<Hand>, ParseError> {
    split_into_blocks(text)
        .into_iter()
        .map(|block| parse_hand_block(&block))
        .collect()
}

/// Splits `text` into hand blocks: maximal runs of non-blank lines, with
/// any run of one or more blank lines treated as a separator.
fn split_into_blocks(text: &str) -> Vec<Vec<(usize, &str)>> {
    let mut blocks = Vec::new();
    let mut current: Vec<(usize, &str)> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line_no = i + 1;
        if line.trim().is_empty() {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
        } else {
            current.push((line_no, line));
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    blocks
}

/// A cursor over one hand block's non-blank lines, with helpers that turn
/// "ran out of lines" / "wrong line" into a [`ParseError`] pointing at the
/// right place.
struct Cursor<'a> {
    lines: &'a [(usize, &'a str)],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(lines: &'a [(usize, &'a str)]) -> Self {
        Cursor { lines, pos: 0 }
    }

    fn peek(&self) -> Option<(usize, &'a str)> {
        self.lines.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<(usize, &'a str)> {
        let item = self.peek();
        if item.is_some() {
            self.pos += 1;
        }
        item
    }

    /// The line number to blame when we run out of input: the last line of
    /// the block, or 0 if the block was somehow empty.
    fn eof_line(&self) -> usize {
        self.lines.last().map_or(0, |(n, _)| *n)
    }

    /// Advances and returns the next line, or a "ran out of input" error
    /// naming what was expected.
    fn take(&mut self, expected: &str) -> Result<(usize, &'a str), ParseError> {
        self.advance().ok_or_else(|| {
            ParseError::at(
                self.eof_line(),
                "",
                format!("expected {expected}, got end of hand"),
            )
        })
    }

    /// Advances past a line that must equal `literal` exactly.
    fn expect(&mut self, literal: &str) -> Result<(), ParseError> {
        let (line_no, line) = self.take(&format!("{literal:?}"))?;
        if line != literal {
            return Err(ParseError::at(
                line_no,
                line,
                format!("expected {literal:?}"),
            ));
        }
        Ok(())
    }
}

fn parse_hand_block(lines: &[(usize, &str)]) -> Result<Hand, ParseError> {
    let mut cur = Cursor::new(lines);

    let (header_line_no, header_line) = cur.take("hand header")?;
    let header = parse_header(header_line_no, header_line)?;

    let (table_line_no, table_line) = cur.take("table line")?;
    let (table, table_size, button_seat) = parse_table_line(table_line_no, table_line)?;

    let mut seats = Vec::new();
    while let Some((line_no, line)) = cur.peek() {
        if !line.starts_with("Seat ") {
            break;
        }
        seats.push(parse_seat_line(line_no, line)?);
        cur.advance();
    }

    let mut actions = Vec::new();
    while let Some((line_no, line)) = cur.peek() {
        if line == "*** HOLE CARDS ***" {
            break;
        }
        actions.push(parse_post_line(line_no, line)?);
        cur.advance();
    }
    cur.expect("*** HOLE CARDS ***")?;

    let mut hero_cards: Option<Vec<Card>> = None;
    while let Some((line_no, line)) = cur.peek() {
        let Some(rest) = line.strip_prefix("Dealt to ") else {
            break;
        };
        cur.advance();
        let Some((player, cards_part)) = rest.split_once(' ') else {
            return Err(ParseError::at(line_no, line, "malformed 'Dealt to' line"));
        };
        if cards_part.is_empty() {
            continue;
        }
        let cards =
            parse_bracket_group(cards_part).map_err(|msg| ParseError::at(line_no, line, msg))?;
        if player == "Hero" {
            if hero_cards.is_some() {
                return Err(ParseError::at(line_no, line, "Hero dealt cards twice"));
            }
            hero_cards = Some(cards);
        }
    }
    let hero_cards = hero_cards
        .ok_or_else(|| ParseError::at(cur.eof_line(), "", "no 'Dealt to Hero [..]' line found"))?;

    let mut board = Vec::new();
    let mut street = Street::Preflop;
    loop {
        let (line_no, line) = cur.take("an action line, street marker, or '*** SHOWDOWN ***'")?;
        if line == "*** SHOWDOWN ***" {
            break;
        }
        if let Some(rest) = line.strip_prefix("*** FLOP *** ") {
            let cards =
                parse_bracket_group(rest).map_err(|msg| ParseError::at(line_no, line, msg))?;
            if !board.is_empty() {
                return Err(ParseError::at(line_no, line, "flop dealt more than once"));
            }
            board = cards;
            street = Street::Flop;
            continue;
        }
        if let Some(rest) = line.strip_prefix("*** TURN *** ") {
            let (seen, new_card) = parse_two_bracket_groups(line_no, line, rest)?;
            if seen != board {
                return Err(ParseError::at(
                    line_no,
                    line,
                    "turn marker's board doesn't match cards dealt so far",
                ));
            }
            board.extend(new_card);
            street = Street::Turn;
            continue;
        }
        if let Some(rest) = line.strip_prefix("*** RIVER *** ") {
            let (seen, new_card) = parse_two_bracket_groups(line_no, line, rest)?;
            if seen != board {
                return Err(ParseError::at(
                    line_no,
                    line,
                    "river marker's board doesn't match cards dealt so far",
                ));
            }
            board.extend(new_card);
            street = Street::River;
            continue;
        }
        if let Some(rest) = line.strip_prefix("Uncalled bet (") {
            let close = rest
                .find(')')
                .ok_or_else(|| ParseError::at(line_no, line, "malformed 'Uncalled bet' line"))?;
            let amount = parse_amount(&rest[..close]).ok_or_else(|| {
                ParseError::at(line_no, line, "bad amount in 'Uncalled bet' line")
            })?;
            let player = rest[close + 1..]
                .strip_prefix(" returned to ")
                .ok_or_else(|| ParseError::at(line_no, line, "malformed 'Uncalled bet' line"))?;
            actions.push(ActionRecord {
                street,
                player: player.to_string(),
                action: Action::UncalledBetReturn { amount },
            });
            continue;
        }
        actions.push(parse_action_line(line_no, line, street)?);
    }

    while let Some((line_no, line)) = cur.peek() {
        if line == "*** SUMMARY ***" {
            break;
        }
        let idx = line.find(" collected ").ok_or_else(|| {
            ParseError::at(
                line_no,
                line,
                "expected a 'collected' line or '*** SUMMARY ***'",
            )
        })?;
        let player = &line[..idx];
        let rest = &line[idx + " collected ".len()..];
        let amount_str = rest
            .strip_suffix(" from pot")
            .ok_or_else(|| ParseError::at(line_no, line, "malformed collect line"))?;
        let amount = parse_amount(amount_str)
            .ok_or_else(|| ParseError::at(line_no, line, "bad amount in collect line"))?;
        actions.push(ActionRecord {
            street,
            player: player.to_string(),
            action: Action::Collect { amount },
        });
        cur.advance();
    }
    cur.expect("*** SUMMARY ***")?;

    let (pot_line_no, pot_line) = cur.take("pot summary line")?;
    let pot = parse_pot_line(pot_line_no, pot_line)?;

    if let Some((line_no, line)) = cur.peek() {
        if let Some(rest) = line.strip_prefix("Board ") {
            let seen =
                parse_bracket_group(rest).map_err(|msg| ParseError::at(line_no, line, msg))?;
            if seen != board {
                return Err(ParseError::at(
                    line_no,
                    line,
                    "'Board' line doesn't match cards dealt during the hand",
                ));
            }
            cur.advance();
        } else if !board.is_empty() {
            return Err(ParseError::at(
                line_no,
                line,
                "expected a 'Board' line (a flop was dealt)",
            ));
        }
    } else if !board.is_empty() {
        return Err(ParseError::at(
            cur.eof_line(),
            "",
            "expected a 'Board' line (a flop was dealt)",
        ));
    }

    let mut results = Vec::new();
    while let Some((line_no, line)) = cur.peek() {
        results.push(parse_seat_result_line(line_no, line)?);
        cur.advance();
    }
    if results.is_empty() {
        return Err(ParseError::at(
            cur.eof_line(),
            "",
            "hand has no seat result lines",
        ));
    }

    Ok(Hand {
        id: header.hand_id,
        tournament_id: header.tournament_id,
        tournament_name: header.tournament_name,
        game: header.game,
        level: header.level,
        small_blind: header.small_blind,
        big_blind: header.big_blind,
        ante: header.ante,
        played_at: header.played_at,
        table,
        table_size,
        button_seat,
        seats,
        hero_cards,
        actions,
        board,
        pot,
        results,
    })
}

struct Header {
    hand_id: u64,
    tournament_id: u64,
    tournament_name: String,
    game: GameType,
    level: u32,
    small_blind: u64,
    big_blind: u64,
    ante: u64,
    played_at: DateTime,
}

/// `Poker Hand #TM<hand_id>: Tournament #<tournament_id>, <name> <game> -
/// Level<level>(<sb>/<bb>[(<ante>)]) - YYYY/MM/DD HH:MM:SS`
fn parse_header(line_no: usize, line: &str) -> Result<Header, ParseError> {
    let err = |msg: &str| ParseError::at(line_no, line, msg);

    let rest = line
        .strip_prefix("Poker Hand #TM")
        .ok_or_else(|| err("missing 'Poker Hand #TM' prefix"))?;
    let (hand_id_str, rest) = rest
        .split_once(": Tournament #")
        .ok_or_else(|| err("missing ': Tournament #' separator"))?;
    let hand_id = parse_amount(hand_id_str).ok_or_else(|| err("bad hand id"))?;

    let (tournament_id_str, rest) = rest
        .split_once(", ")
        .ok_or_else(|| err("missing ', ' after tournament id"))?;
    let tournament_id = parse_amount(tournament_id_str).ok_or_else(|| err("bad tournament id"))?;

    // Peel off the fixed 19-char " - YYYY/MM/DD HH:MM:SS" suffix.
    if rest.len() < 19 + 3 {
        return Err(err("line too short to contain a trailing date"));
    }
    let (head, date_str) = rest.split_at(rest.len() - 19);
    let head = head
        .strip_suffix(" - ")
        .ok_or_else(|| err("missing ' - ' before the date"))?;
    let played_at = parse_datetime(date_str).ok_or_else(|| err("bad date"))?;

    // Peel off " - Level<level>(<sb>/<bb>[(<ante>)])" from the right; the
    // marker is `" - Level"` (tournament names may contain their own
    // `" - "`, but never that exact marker).
    let level_idx = head
        .rfind(" - Level")
        .ok_or_else(|| err("missing ' - Level' segment"))?;
    let name_and_game = &head[..level_idx];
    let level_part = &head[level_idx + " - Level".len()..];

    let paren_idx = level_part
        .find('(')
        .ok_or_else(|| err("missing '(' after level number"))?;
    let level: u32 = level_part[..paren_idx]
        .parse()
        .map_err(|_| err("bad level number"))?;
    let blinds_part = &level_part[paren_idx..];
    let blinds_inner = blinds_part
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .ok_or_else(|| err("malformed blinds group"))?;
    let (sb_str, bb_and_ante) = blinds_inner
        .split_once('/')
        .ok_or_else(|| err("missing '/' between blinds"))?;
    let small_blind = parse_amount(sb_str).ok_or_else(|| err("bad small blind"))?;
    let (bb_str, ante) = match bb_and_ante.find('(') {
        Some(idx) => {
            let bb_str = &bb_and_ante[..idx];
            let ante_str = bb_and_ante[idx..]
                .strip_prefix('(')
                .and_then(|s| s.strip_suffix(')'))
                .ok_or_else(|| err("malformed ante group"))?;
            let ante = parse_amount(ante_str).ok_or_else(|| err("bad ante"))?;
            (bb_str, ante)
        }
        None => (bb_and_ante, 0),
    };
    let big_blind = parse_amount(bb_str).ok_or_else(|| err("bad big blind"))?;

    let (game, tournament_name) = GameType::split_suffix(name_and_game)
        .ok_or_else(|| err("unrecognized game type suffix"))?;

    Ok(Header {
        hand_id,
        tournament_id,
        tournament_name: tournament_name.to_string(),
        game,
        level,
        small_blind,
        big_blind,
        ante,
        played_at,
    })
}

/// `Table '<table_name>' <k>-max Seat #<b> is the button`
fn parse_table_line(line_no: usize, line: &str) -> Result<(String, u8, u8), ParseError> {
    let err = |msg: &str| ParseError::at(line_no, line, msg);
    let rest = line
        .strip_prefix("Table '")
        .ok_or_else(|| err("missing \"Table '\" prefix"))?;
    let name_end = rest
        .find("' ")
        .ok_or_else(|| err("missing \"' \" after table name"))?;
    let table_name = rest[..name_end].to_string();
    let rest = &rest[name_end + 2..];
    let max_idx = rest
        .find("-max Seat #")
        .ok_or_else(|| err("missing '-max Seat #'"))?;
    let table_size: u8 = rest[..max_idx].parse().map_err(|_| err("bad table size"))?;
    let rest = &rest[max_idx + "-max Seat #".len()..];
    let button_str = rest
        .strip_suffix(" is the button")
        .ok_or_else(|| err("missing ' is the button' suffix"))?;
    let button_seat: u8 = button_str
        .parse()
        .map_err(|_| err("bad button seat number"))?;
    Ok((table_name, table_size, button_seat))
}

/// `Seat <n>: <player> (<chips> in chips)`
fn parse_seat_line(line_no: usize, line: &str) -> Result<Seat, ParseError> {
    let err = |msg: &str| ParseError::at(line_no, line, msg);
    let rest = line
        .strip_prefix("Seat ")
        .ok_or_else(|| err("missing 'Seat ' prefix"))?;
    let (seat_str, rest) = rest
        .split_once(": ")
        .ok_or_else(|| err("missing ': ' after seat number"))?;
    let seat: u8 = seat_str.parse().map_err(|_| err("bad seat number"))?;
    let paren_idx = rest
        .rfind(" (")
        .ok_or_else(|| err("missing ' (' before chip count"))?;
    let player = rest[..paren_idx].to_string();
    let chips_part = &rest[paren_idx + 2..];
    let chips_str = chips_part
        .strip_suffix(" in chips)")
        .ok_or_else(|| err("missing ' in chips)' suffix"))?;
    let chips = parse_amount(chips_str).ok_or_else(|| err("bad chip count"))?;
    Ok(Seat {
        seat,
        player,
        chips,
    })
}

/// One of the three post lines, with an optional " and is all-in" suffix
/// that's accepted but not represented in `Action::Post*`.
fn parse_post_line(line_no: usize, line: &str) -> Result<ActionRecord, ParseError> {
    let err = |msg: &str| ParseError::at(line_no, line, msg);
    let (player, rest) = line
        .split_once(": ")
        .ok_or_else(|| err("expected '<player>: posts ...'"))?;
    let (rest, _all_in) = strip_all_in(rest);
    let action = if let Some(n) = rest.strip_prefix("posts the ante ") {
        Action::PostAnte(parse_amount(n).ok_or_else(|| err("bad ante amount"))?)
    } else if let Some(n) = rest.strip_prefix("posts small blind ") {
        Action::PostSmallBlind(parse_amount(n).ok_or_else(|| err("bad small blind amount"))?)
    } else if let Some(n) = rest.strip_prefix("posts big blind ") {
        Action::PostBigBlind(parse_amount(n).ok_or_else(|| err("bad big blind amount"))?)
    } else {
        return Err(err(
            "expected a post line ('posts the ante'/'posts small blind'/'posts big blind')",
        ));
    };
    Ok(ActionRecord {
        street: Street::Preflop,
        player: player.to_string(),
        action,
    })
}

/// Parses `"[a b c] [d]"` (as seen after stripping the `*** TURN ***`/
/// `*** RIVER ***` marker prefix) into `(first_group, second_group)`.
fn parse_two_bracket_groups(
    line_no: usize,
    line: &str,
    rest: &str,
) -> Result<(Vec<Card>, Vec<Card>), ParseError> {
    let sep = rest
        .find("] [")
        .ok_or_else(|| ParseError::at(line_no, line, "expected two bracketed card groups"))?;
    let first = &rest[..sep + 1];
    let second = &rest[sep + 2..];
    let first = parse_bracket_group(first).map_err(|msg| ParseError::at(line_no, line, msg))?;
    let second = parse_bracket_group(second).map_err(|msg| ParseError::at(line_no, line, msg))?;
    Ok((first, second))
}

/// One action line of the form `"<player>: <verb...>"`, for the verbs that
/// can appear between `*** HOLE CARDS ***` and `*** SHOWDOWN ***`.
fn parse_action_line(
    line_no: usize,
    line: &str,
    street: Street,
) -> Result<ActionRecord, ParseError> {
    let err = |msg: &str| ParseError::at(line_no, line, msg);
    let (player, rest) = line
        .split_once(": ")
        .ok_or_else(|| err("unrecognized line (expected an action, street marker, or SHOWDOWN)"))?;

    let action = if rest == "folds" {
        Action::Fold
    } else if rest == "checks" {
        Action::Check
    } else if let Some(n) = rest.strip_prefix("calls ") {
        let (n, all_in) = strip_all_in(n);
        Action::Call {
            amount: parse_amount(n).ok_or_else(|| err("bad call amount"))?,
            all_in,
        }
    } else if let Some(n) = rest.strip_prefix("bets ") {
        let (n, all_in) = strip_all_in(n);
        Action::Bet {
            amount: parse_amount(n).ok_or_else(|| err("bad bet amount"))?,
            all_in,
        }
    } else if let Some(n) = rest.strip_prefix("raises ") {
        let (n, all_in) = strip_all_in(n);
        let (by_str, to_str) = n
            .split_once(" to ")
            .ok_or_else(|| err("malformed raise (missing ' to ')"))?;
        Action::Raise {
            by: parse_amount(by_str).ok_or_else(|| err("bad raise-by amount"))?,
            to: parse_amount(to_str).ok_or_else(|| err("bad raise-to amount"))?,
            all_in,
        }
    } else if let Some(n) = rest.strip_prefix("shows ") {
        let close = n
            .find(']')
            .ok_or_else(|| err("malformed 'shows' (no closing ']')"))?;
        let cards = parse_bracket_group(&n[..=close]).map_err(|msg| err(&msg))?;
        let after = &n[close + 1..];
        let description = if after.is_empty() {
            None
        } else if let Some(desc) = after.strip_prefix(" (").and_then(|s| s.strip_suffix(')')) {
            Some(desc.to_string())
        } else {
            return Err(err("malformed 'shows' trailing description"));
        };
        Action::Show { cards, description }
    } else {
        return Err(err("unrecognized action verb"));
    };

    Ok(ActionRecord {
        street,
        player: player.to_string(),
        action,
    })
}

/// `Total pot <t> | Rake <r> | Jackpot <j> | Bingo <b> | Fortune <f> | Tax <x>`
fn parse_pot_line(line_no: usize, line: &str) -> Result<PotSummary, ParseError> {
    let err = |msg: &str| ParseError::at(line_no, line, msg);
    let parts: Vec<&str> = line.split(" | ").collect();
    let [total, rake, jackpot, bingo, fortune, tax] = parts.as_slice() else {
        return Err(err("expected 6 ' | '-separated pot fields"));
    };
    let field = |part: &str, prefix: &str| -> Result<u64, ParseError> {
        let n = part
            .strip_prefix(prefix)
            .ok_or_else(|| err(&format!("expected {prefix:?} prefix")))?;
        parse_amount(n).ok_or_else(|| err(&format!("bad {prefix}amount")))
    };
    Ok(PotSummary {
        total: field(total, "Total pot ")?,
        rake: field(rake, "Rake ")?,
        jackpot: field(jackpot, "Jackpot ")?,
        bingo: field(bingo, "Bingo ")?,
        fortune: field(fortune, "Fortune ")?,
        tax: field(tax, "Tax ")?,
    })
}

/// `Seat <n>: <player>[ (button)][ (small blind)][ (big blind)] <outcome>`
fn parse_seat_result_line(line_no: usize, line: &str) -> Result<SeatResult, ParseError> {
    let err = |msg: &str| ParseError::at(line_no, line, msg);
    let rest = line
        .strip_prefix("Seat ")
        .ok_or_else(|| err("missing 'Seat ' prefix"))?;
    let (seat_str, rest) = rest
        .split_once(": ")
        .ok_or_else(|| err("missing ': ' after seat number"))?;
    let seat: u8 = seat_str.parse().map_err(|_| err("bad seat number"))?;
    let (player, mut rest) = rest
        .split_once(' ')
        .ok_or_else(|| err("missing outcome after player name"))?;

    let mut is_button = false;
    let mut is_small_blind = false;
    let mut is_big_blind = false;
    loop {
        if let Some(r) = rest.strip_prefix("(button) ") {
            is_button = true;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("(small blind) ") {
            is_small_blind = true;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("(big blind) ") {
            is_big_blind = true;
            rest = r;
        } else {
            break;
        }
    }

    let outcome = parse_seat_outcome(line_no, line, rest)?;
    Ok(SeatResult {
        seat,
        player: player.to_string(),
        is_button,
        is_small_blind,
        is_big_blind,
        outcome,
    })
}

fn parse_seat_outcome(line_no: usize, line: &str, rest: &str) -> Result<SeatOutcome, ParseError> {
    let err = |msg: &str| ParseError::at(line_no, line, msg);

    for (prefix, street) in [
        ("folded before Flop", Street::Preflop),
        ("folded on the Flop", Street::Flop),
        ("folded on the Turn", Street::Turn),
        ("folded on the River", Street::River),
    ] {
        if let Some(after) = rest.strip_prefix(prefix) {
            let didnt_bet = match after {
                "" => false,
                " (didn't bet)" => true,
                _ => return Err(err("unrecognized suffix after 'folded ...'")),
            };
            return Ok(SeatOutcome::Folded { street, didnt_bet });
        }
    }

    if let Some(after) = rest.strip_prefix("showed ") {
        let close = after
            .find(']')
            .ok_or_else(|| err("malformed 'showed' (no closing ']')"))?;
        let cards = parse_bracket_group(&after[..=close]).map_err(|msg| err(&msg))?;
        let tail = &after[close + 1..];
        if let Some(paren) = tail.strip_prefix(" and won ") {
            let (amount, after_amount) =
                parse_leading_paren_amount(paren).ok_or_else(|| err("malformed 'and won (n)'"))?;
            let description = if after_amount.is_empty() {
                None
            } else {
                Some(
                    after_amount
                        .strip_prefix(" with ")
                        .ok_or_else(|| err("unrecognized text after 'and won (n)'"))?
                        .to_string(),
                )
            };
            return Ok(SeatOutcome::ShowedWon {
                amount,
                cards,
                description,
            });
        }
        if let Some(description) = tail.strip_prefix(" and lost with ") {
            if description.is_empty() {
                return Err(err("missing description after 'and lost with'"));
            }
            return Ok(SeatOutcome::ShowedLost {
                cards,
                description: description.to_string(),
            });
        }
        if let Some(paren) = tail.strip_prefix(" and collected ") {
            let (amount, after_amount) = parse_leading_paren_amount(paren)
                .ok_or_else(|| err("malformed 'and collected (n)'"))?;
            if !after_amount.is_empty() {
                return Err(err("unexpected text after 'and collected (n)'"));
            }
            return Ok(SeatOutcome::Collected {
                amount,
                cards: Some(cards),
            });
        }
        return Err(err("unrecognized text after 'showed [..]'"));
    }

    for prefix in ["won ", "collected "] {
        if let Some(paren) = rest.strip_prefix(prefix) {
            let (amount, after_amount) = parse_leading_paren_amount(paren)
                .ok_or_else(|| err(&format!("malformed '{prefix}(n)'")))?;
            if !after_amount.is_empty() {
                return Err(err(&format!("unexpected text after '{prefix}(n)'")));
            }
            return Ok(SeatOutcome::Collected {
                amount,
                cards: None,
            });
        }
    }

    Err(ParseError::at(line_no, line, "unrecognized seat outcome"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(s: &str) -> Card {
        s.parse().unwrap()
    }

    #[test]
    fn holdem_hand_with_full_action_sequence() {
        let text = "\
Poker Hand #TM1000000001: Tournament #123456789, Test Championship Hold'em No Limit - Level1(100/200) - 2026/01/02 03:04:05
Table '1' 3-max Seat #2 is the button
Seat 1: Hero (10,000 in chips)
Seat 2: villain1 (10,000 in chips)
Seat 3: villain2 (10,000 in chips)
villain2: posts small blind 100
Hero: posts big blind 200
*** HOLE CARDS ***
Dealt to Hero [Ah Kh]
Dealt to villain1 
Dealt to villain2 
villain1: raises 400 to 600
villain2: folds
Hero: calls 400
*** FLOP *** [2c 7d 9h]
Hero: checks
villain1: bets 500
Hero: calls 500
*** TURN *** [2c 7d 9h] [Ks]
Hero: bets 1,000
villain1: raises 2,000 to 3,000
Hero: calls 2,000 and is all-in
*** RIVER *** [2c 7d 9h Ks] [3d]
villain1: shows [Ac Kc] (a pair of Kings)
Hero: shows [Ah Kh] (two pair, Kings and Aces)
*** SHOWDOWN ***
Hero collected 8,300 from pot
*** SUMMARY ***
Total pot 8,300 | Rake 0 | Jackpot 0 | Bingo 0 | Fortune 0 | Tax 0
Board [2c 7d 9h Ks 3d]
Seat 1: Hero (big blind) showed [Ah Kh] and won (8,300) with two pair, Kings and Aces
Seat 2: villain1 showed [Ac Kc] and lost with a pair of Kings
Seat 3: villain2 (small blind) folded before Flop
";
        let hands = parse_hand_history(text).unwrap();
        assert_eq!(hands.len(), 1);
        let hand = &hands[0];

        assert_eq!(hand.id, 1_000_000_001);
        assert_eq!(hand.tournament_id, 123_456_789);
        assert_eq!(hand.tournament_name, "Test Championship");
        assert_eq!(hand.game, GameType::Holdem);
        assert_eq!(hand.level, 1);
        assert_eq!(hand.small_blind, 100);
        assert_eq!(hand.big_blind, 200);
        assert_eq!(hand.ante, 0);
        assert_eq!(
            hand.played_at,
            DateTime {
                year: 2026,
                month: 1,
                day: 2,
                hour: 3,
                minute: 4,
                second: 5,
            }
        );
        assert_eq!(hand.table, "1");
        assert_eq!(hand.table_size, 3);
        assert_eq!(hand.button_seat, 2);
        assert_eq!(
            hand.seats,
            vec![
                Seat {
                    seat: 1,
                    player: "Hero".into(),
                    chips: 10_000
                },
                Seat {
                    seat: 2,
                    player: "villain1".into(),
                    chips: 10_000
                },
                Seat {
                    seat: 3,
                    player: "villain2".into(),
                    chips: 10_000
                },
            ]
        );
        assert_eq!(hand.hero_cards, vec![c("Ah"), c("Kh")]);
        assert_eq!(
            hand.board,
            vec![c("2c"), c("7d"), c("9h"), c("Ks"), c("3d")]
        );
        assert_eq!(
            hand.pot,
            PotSummary {
                total: 8_300,
                rake: 0,
                jackpot: 0,
                bingo: 0,
                fortune: 0,
                tax: 0,
            }
        );

        assert_eq!(hand.actions.len(), 14);
        assert_eq!(
            hand.actions[0],
            ActionRecord {
                street: Street::Preflop,
                player: "villain2".into(),
                action: Action::PostSmallBlind(100),
            }
        );
        assert_eq!(
            hand.actions[2],
            ActionRecord {
                street: Street::Preflop,
                player: "villain1".into(),
                action: Action::Raise {
                    by: 400,
                    to: 600,
                    all_in: false
                },
            }
        );
        assert_eq!(
            hand.actions[10],
            ActionRecord {
                street: Street::Turn,
                player: "Hero".into(),
                action: Action::Call {
                    amount: 2_000,
                    all_in: true
                },
            }
        );
        assert_eq!(
            hand.actions[11],
            ActionRecord {
                street: Street::River,
                player: "villain1".into(),
                action: Action::Show {
                    cards: vec![c("Ac"), c("Kc")],
                    description: Some("a pair of Kings".into()),
                },
            }
        );
        assert_eq!(
            hand.actions[13],
            ActionRecord {
                street: Street::River,
                player: "Hero".into(),
                action: Action::Collect { amount: 8_300 },
            }
        );

        assert_eq!(hand.results.len(), 3);
        assert_eq!(
            hand.results[0],
            SeatResult {
                seat: 1,
                player: "Hero".into(),
                is_button: false,
                is_small_blind: false,
                is_big_blind: true,
                outcome: SeatOutcome::ShowedWon {
                    amount: 8_300,
                    cards: vec![c("Ah"), c("Kh")],
                    description: Some("two pair, Kings and Aces".into()),
                },
            }
        );
        assert_eq!(
            hand.results[1],
            SeatResult {
                seat: 2,
                player: "villain1".into(),
                is_button: false,
                is_small_blind: false,
                is_big_blind: false,
                outcome: SeatOutcome::ShowedLost {
                    cards: vec![c("Ac"), c("Kc")],
                    description: "a pair of Kings".into(),
                },
            }
        );
        assert_eq!(
            hand.results[2],
            SeatResult {
                seat: 3,
                player: "villain2".into(),
                is_button: false,
                is_small_blind: true,
                is_big_blind: false,
                outcome: SeatOutcome::Folded {
                    street: Street::Preflop,
                    didnt_bet: false
                },
            }
        );
    }

    #[test]
    fn omaha_hand_has_four_hero_cards() {
        let text = "\
Poker Hand #TM178536654: Tournament #282036155, Step 0 - Avatar Race Omaha No Limit - Level1(5/10) - 2026/05/02 01:51:12
Table '1' 3-max Seat #2 is the button
Seat 1: Hero (100 in chips)
Seat 2: a9ca188c (100 in chips)
Seat 3: 49b0de4 (100 in chips)
49b0de4: posts small blind 5
Hero: posts big blind 10
*** HOLE CARDS ***
Dealt to Hero [2s 3s 4h 5h]
Dealt to a9ca188c 
Dealt to 49b0de4 
a9ca188c: raises 90 to 100 and is all-in
49b0de4: calls 95 and is all-in
Hero: calls 90 and is all-in
a9ca188c: shows [Kh Kd Qd Qs]
49b0de4: shows [Ad Ac Js Ts]
Hero: shows [2s 3s 4h 5h]
*** FLOP *** [Td Jc 9s]
*** TURN *** [Td Jc 9s] [2c]
*** RIVER *** [Td Jc 9s 2c] [3c]
*** SHOWDOWN ***
a9ca188c collected 300 from pot
*** SUMMARY ***
Total pot 300 | Rake 0 | Jackpot 0 | Bingo 0 | Fortune 0 | Tax 0
Board [Td Jc 9s 2c 3c]
Seat 1: Hero (big blind) showed [2s 3s 4h 5h] and lost with two pair, Threes and Twos
Seat 2: a9ca188c (button) showed [Kh Kd Qd Qs] and won (300) with a straight, King to Nine
Seat 3: 49b0de4 (small blind) showed [Ad Ac Js Ts] and lost with two pair, Jacks and Tens
";
        let hands = parse_hand_history(text).unwrap();
        assert_eq!(hands.len(), 1);
        let hand = &hands[0];
        assert_eq!(hand.game, GameType::Omaha);
        assert_eq!(hand.tournament_name, "Step 0 - Avatar Race");
        assert_eq!(hand.hero_cards, vec![c("2s"), c("3s"), c("4h"), c("5h")]);
        assert_eq!(hand.hero_cards.len(), 4);
        assert_eq!(hand.board.len(), 5);
    }

    #[test]
    fn flipout_hand_ante_only_with_side_pots() {
        let text = "\
Poker Hand #TM2000000002: Tournament #123456789, Test Flipout Hold'em No Limit - Level1(0/0(1,000)) - 2026/01/02 03:04:06
Table '2' 3-max Seat #1 is the button
Seat 1: Hero (1,000 in chips)
Seat 2: villain1 (500 in chips)
Seat 3: villain2 (2,000 in chips)
Hero: posts the ante 1,000
villain1: posts the ante 500
villain2: posts the ante 2,000
villain1: posts small blind 0
villain2: posts big blind 0
*** HOLE CARDS ***
Dealt to Hero [2c 2d]
Dealt to villain1 
Dealt to villain2 
Uncalled bet (1,000) returned to villain2
Hero: shows [2c 2d]
villain1: shows [3c 3d]
villain2: shows [4c 4d]
*** FLOP *** [5c 6c 7c]
*** TURN *** [5c 6c 7c] [8c]
*** RIVER *** [5c 6c 7c 8c] [9c]
*** SHOWDOWN ***
villain2 collected 1,500 from pot
villain2 collected 1,000 from pot
*** SUMMARY ***
Total pot 2,500 | Rake 0 | Jackpot 0 | Bingo 0 | Fortune 0 | Tax 0
Board [5c 6c 7c 8c 9c]
Seat 1: Hero showed [2c 2d] and lost with a pair of Twos
Seat 2: villain1 (small blind) showed [3c 3d] and lost with a pair of Threes
Seat 3: villain2 (big blind) showed [4c 4d] and won (2,500) with a pair of Fours
";
        let hands = parse_hand_history(text).unwrap();
        assert_eq!(hands.len(), 1);
        let hand = &hands[0];
        assert_eq!(hand.small_blind, 0);
        assert_eq!(hand.big_blind, 0);
        assert_eq!(hand.ante, 1_000);

        let posts: Vec<_> = hand
            .actions
            .iter()
            .filter(|a| matches!(a.action, Action::PostAnte(_)))
            .collect();
        assert_eq!(posts.len(), 3);

        // The uncalled bet appears before any voluntary action, at Preflop.
        let uncalled_idx = hand
            .actions
            .iter()
            .position(|a| matches!(a.action, Action::UncalledBetReturn { .. }))
            .expect("an UncalledBetReturn action");
        assert_eq!(hand.actions[uncalled_idx].street, Street::Preflop);
        assert_eq!(
            hand.actions[uncalled_idx].action,
            Action::UncalledBetReturn { amount: 1_000 }
        );

        let collects: Vec<u64> = hand
            .actions
            .iter()
            .filter_map(|a| match a.action {
                Action::Collect { amount } => Some(amount),
                _ => None,
            })
            .collect();
        assert_eq!(collects, vec![1_500, 1_000]);
        assert_eq!(collects.iter().sum::<u64>(), hand.pot.total);
    }

    #[test]
    fn fold_win_hand_has_no_board_and_didnt_bet_suffix() {
        let text = "\
Poker Hand #TM3000000003: Tournament #123456789, Test Fold Win Hold'em No Limit - Level2(50/100(10)) - 2026/01/02 03:04:07
Table '3' 3-max Seat #2 is the button
Seat 1: Hero (5,000 in chips)
Seat 2: villain1 (5,000 in chips)
Seat 3: villain2 (5,000 in chips)
Hero: posts the ante 10
villain1: posts the ante 10
villain2: posts the ante 10
villain2: posts small blind 50
Hero: posts big blind 100
*** HOLE CARDS ***
Dealt to Hero [2c 2d]
Dealt to villain1 
Dealt to villain2 
villain1: raises 200 to 300
villain2: folds
Hero: folds
Uncalled bet (200) returned to villain1
*** SHOWDOWN ***
villain1 collected 280 from pot
*** SUMMARY ***
Total pot 280 | Rake 0 | Jackpot 0 | Bingo 0 | Fortune 0 | Tax 0
Seat 1: Hero (big blind) folded before Flop (didn't bet)
Seat 2: villain1 (button) collected (280)
Seat 3: villain2 (small blind) folded before Flop
";
        let hands = parse_hand_history(text).unwrap();
        assert_eq!(hands.len(), 1);
        let hand = &hands[0];
        assert!(hand.board.is_empty());
        assert_eq!(
            hand.results[0].outcome,
            SeatOutcome::Folded {
                street: Street::Preflop,
                didnt_bet: true
            }
        );
        assert_eq!(
            hand.results[1].outcome,
            SeatOutcome::Collected {
                amount: 280,
                cards: None
            }
        );
        assert_eq!(
            hand.results[2].outcome,
            SeatOutcome::Folded {
                street: Street::Preflop,
                didnt_bet: false
            }
        );
    }

    #[test]
    fn unrecognized_line_reports_correct_line_number() {
        let text = "\
Poker Hand #TM1: Tournament #1, Test Hold'em No Limit - Level1(10/20) - 2026/01/02 03:04:05
Table '1' 2-max Seat #1 is the button
Seat 1: Hero (1,000 in chips)
Seat 2: villain1 (1,000 in chips)
Hero: posts small blind 10
villain1: posts big blind 20
*** HOLE CARDS ***
Dealt to Hero [2c 2d]
Dealt to villain1 
Hero: raises like a maniac
";
        let err = parse_hand_history(text).unwrap_err();
        assert_eq!(err.line, 10);
        assert_eq!(err.content, "Hero: raises like a maniac");
    }

    #[test]
    fn truncated_hand_reports_end_of_hand_error() {
        let text = "\
Poker Hand #TM1: Tournament #1, Test Hold'em No Limit - Level1(10/20) - 2026/01/02 03:04:05
Table '1' 2-max Seat #1 is the button
Seat 1: Hero (1,000 in chips)
Seat 2: villain1 (1,000 in chips)
";
        let err = parse_hand_history(text).unwrap_err();
        assert_eq!(err.line, 4);
    }

    #[test]
    fn bad_card_in_hole_cards_is_an_error() {
        let text = "\
Poker Hand #TM1: Tournament #1, Test Hold'em No Limit - Level1(10/20) - 2026/01/02 03:04:05
Table '1' 2-max Seat #1 is the button
Seat 1: Hero (1,000 in chips)
Seat 2: villain1 (1,000 in chips)
Hero: posts small blind 10
villain1: posts big blind 20
*** HOLE CARDS ***
Dealt to Hero [Xx 2d]
Dealt to villain1 
";
        let err = parse_hand_history(text).unwrap_err();
        assert_eq!(err.line, 8);
        assert!(err.message.contains("invalid card"));
    }
}
