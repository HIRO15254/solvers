//! Parser for GGPoker tournament summary files: one small fixed-shape
//! record per tournament, giving Hero's placement and prize.

use crate::model::{GameType, ParseError, TournamentSummary, Winnings};
use crate::util::{parse_amount, parse_datetime};

/// Parses a tournament summary file. See the module docs / spec for the
/// exact 8-line shape expected.
pub fn parse_summary(text: &str) -> Result<TournamentSummary, ParseError> {
    let mut lines = text.lines().enumerate().map(|(i, l)| (i + 1, l));

    let (line_no, line) = next_line(&mut lines, "the 'Tournament #...' header line")?;
    let (id, name, game) = parse_header_line(line_no, line)?;

    let (line_no, line) = next_line(&mut lines, "the 'Buy-in: ...' line")?;
    let (buy_in_cents, fee_cents) = parse_buy_in_line(line_no, line)?;

    let (line_no, line) = next_line(&mut lines, "the '<n> Players' line")?;
    let players: u32 = line
        .strip_suffix(" Players")
        .and_then(parse_amount)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| ParseError::at(line_no, line, "expected '<n> Players'"))?;

    let (line_no, line) = next_line(&mut lines, "the 'Total Prize Pool: ...' line")?;
    let prize_pool_cents = line
        .strip_prefix("Total Prize Pool: ")
        .and_then(parse_dollar_cents)
        .ok_or_else(|| ParseError::at(line_no, line, "expected 'Total Prize Pool: $<amount>'"))?;

    let (line_no, line) = next_line(&mut lines, "the 'Tournament started ...' line")?;
    let started_at = line
        .strip_prefix("Tournament started ")
        .map(str::trim_end)
        .and_then(parse_datetime)
        .ok_or_else(|| ParseError::at(line_no, line, "expected 'Tournament started <datetime>'"))?;

    let (line_no, line) = next_line(&mut lines, "the '<place> : Hero, <prize>' line")?;
    let (hero_place, hero_ord, hero_prize) = parse_place_line(line_no, line)?;

    let (line_no, line) = next_line(&mut lines, "the 'You finished the tournament in ...' line")?;
    parse_finished_line(line_no, line, hero_place, hero_ord)?;

    let (line_no, line) = next_line(&mut lines, "the 'You received/made re-entries...' line")?;
    let (re_entries, total_received) = parse_received_line(line_no, line)?;

    // Any remaining lines must be blank (trailing blank lines are ignored;
    // anything else is unrecognized).
    for (line_no, line) in lines {
        if !line.trim().is_empty() {
            return Err(ParseError::at(
                line_no,
                line,
                "unexpected trailing line in summary",
            ));
        }
    }

    Ok(TournamentSummary {
        id,
        name,
        game,
        buy_in_cents,
        fee_cents,
        players,
        prize_pool_cents,
        started_at,
        hero_place,
        hero_prize,
        re_entries,
        total_received,
    })
}

fn next_line<'a>(
    lines: &mut impl Iterator<Item = (usize, &'a str)>,
    expected: &str,
) -> Result<(usize, &'a str), ParseError> {
    lines
        .next()
        .ok_or_else(|| ParseError::at(0, "", format!("expected {expected}, got end of file")))
}

/// `Tournament #<id>, <name>, <game>` — `<name>` may itself contain commas,
/// so the game is matched as the suffix after the *last* `", "`.
fn parse_header_line(line_no: usize, line: &str) -> Result<(u64, String, GameType), ParseError> {
    let err = |msg: &str| ParseError::at(line_no, line, msg);
    let rest = line
        .strip_prefix("Tournament #")
        .ok_or_else(|| err("missing 'Tournament #' prefix"))?;
    let (id_str, rest) = rest
        .split_once(", ")
        .ok_or_else(|| err("missing ', ' after tournament id"))?;
    let id = parse_amount(id_str).ok_or_else(|| err("bad tournament id"))?;
    let comma_idx = rest
        .rfind(", ")
        .ok_or_else(|| err("missing ', ' before game type"))?;
    let name = rest[..comma_idx].to_string();
    let game_str = &rest[comma_idx + 2..];
    let game = GameType::from_exact(game_str).ok_or_else(|| err("unrecognized game type"))?;
    Ok((id, name, game))
}

/// `Buy-in: $<a>[+$<b>]`
fn parse_buy_in_line(line_no: usize, line: &str) -> Result<(u64, u64), ParseError> {
    let err = |msg: &str| ParseError::at(line_no, line, msg);
    let rest = line
        .strip_prefix("Buy-in: ")
        .ok_or_else(|| err("missing 'Buy-in: ' prefix"))?;
    match rest.split_once('+') {
        Some((a, b)) => {
            let buy_in = parse_dollar_cents(a).ok_or_else(|| err("bad buy-in amount"))?;
            let fee = parse_dollar_cents(b).ok_or_else(|| err("bad fee amount"))?;
            Ok((buy_in, fee))
        }
        None => {
            let buy_in = parse_dollar_cents(rest).ok_or_else(|| err("bad buy-in amount"))?;
            Ok((buy_in, 0))
        }
    }
}

/// `<place><ord> : Hero, <prize>`, ordinal suffix not validated for
/// number/suffix agreement (the corpus has e.g. `43th`).
fn parse_place_line(line_no: usize, line: &str) -> Result<(u32, &str, Winnings), ParseError> {
    let err = |msg: &str| ParseError::at(line_no, line, msg);
    let digits_end = line
        .find(|c: char| !c.is_ascii_digit())
        .ok_or_else(|| err("expected a place number"))?;
    let place: u32 = line[..digits_end]
        .parse()
        .map_err(|_| err("bad place number"))?;
    let rest = &line[digits_end..];
    let ord = rest.get(..2).ok_or_else(|| err("missing ordinal suffix"))?;
    if !matches!(ord, "st" | "nd" | "rd" | "th") {
        return Err(err("unrecognized ordinal suffix"));
    }
    let rest = &rest[2..];
    let prize_str = rest
        .strip_prefix(" : Hero, ")
        .ok_or_else(|| err("expected ' : Hero, <prize>'"))?;
    let prize = parse_winnings(prize_str).ok_or_else(|| err("unrecognized prize amount"))?;
    Ok((place, ord, prize))
}

/// `You finished the tournament in <place><ord> place.` — must agree with
/// the place line (the corpus always has them match exactly).
fn parse_finished_line(
    line_no: usize,
    line: &str,
    place: u32,
    ord: &str,
) -> Result<(), ParseError> {
    let expected = format!("You finished the tournament in {place}{ord} place.");
    if line != expected {
        return Err(ParseError::at(
            line_no,
            line,
            format!("expected {expected:?} (to match the earlier place line)"),
        ));
    }
    Ok(())
}

/// `You received a total of <prize>.` or `You made <k> re-entries and
/// received a total of <prize>.`
fn parse_received_line(line_no: usize, line: &str) -> Result<(u32, Winnings), ParseError> {
    let err = |msg: &str| ParseError::at(line_no, line, msg);
    if let Some(rest) = line.strip_prefix("You received a total of ") {
        let prize_str = rest
            .strip_suffix('.')
            .ok_or_else(|| err("missing trailing '.'"))?;
        let prize = parse_winnings(prize_str).ok_or_else(|| err("unrecognized prize amount"))?;
        return Ok((0, prize));
    }
    if let Some(rest) = line.strip_prefix("You made ") {
        let (k_str, rest) = rest
            .split_once(" re-entries and received a total of ")
            .ok_or_else(|| {
                err("expected 'You made <k> re-entries and received a total of <prize>.'")
            })?;
        let re_entries: u32 = k_str.parse().map_err(|_| err("bad re-entries count"))?;
        let prize_str = rest
            .strip_suffix('.')
            .ok_or_else(|| err("missing trailing '.'"))?;
        let prize = parse_winnings(prize_str).ok_or_else(|| err("unrecognized prize amount"))?;
        return Ok((re_entries, prize));
    }
    Err(err(
        "expected 'You received a total of ...' or 'You made <k> re-entries ...'",
    ))
}

/// `$<x>` / `C$<x>` / `T$<x>` / `$<x> Entry` / `<n> chips`.
fn parse_winnings(s: &str) -> Option<Winnings> {
    if let Some(rest) = s.strip_prefix("C$") {
        return Some(Winnings::Coin(parse_dollar_cents_body(rest)?));
    }
    if let Some(rest) = s.strip_prefix("T$") {
        return Some(Winnings::TournamentDollars(parse_dollar_cents_body(rest)?));
    }
    if let Some(rest) = s.strip_prefix('$') {
        if let Some(amount) = rest.strip_suffix(" Entry") {
            return Some(Winnings::TicketEntry(parse_dollar_cents_body(amount)?));
        }
        return Some(Winnings::Dollars(parse_dollar_cents_body(rest)?));
    }
    if let Some(rest) = s.strip_suffix(" chips") {
        return Some(Winnings::Chips(parse_amount(rest)?));
    }
    None
}

/// Parses a dollar string with its leading `$` (or `C$`/`T$`) already
/// stripped: comma-grouped whole dollars, optional `.` plus up to 2
/// fractional digits.
fn parse_dollar_cents_body(s: &str) -> Option<u64> {
    let (whole, frac) = match s.split_once('.') {
        Some((w, f)) => (w, f),
        None => (s, ""),
    };
    let whole = parse_amount(whole)?;
    let cents = match frac.len() {
        0 => 0,
        1 => frac.parse::<u64>().ok()? * 10,
        2 => frac.parse::<u64>().ok()?,
        _ => return None,
    };
    Some(whole * 100 + cents)
}

/// Parses a `$<amount>` string (with the leading `$`).
fn parse_dollar_cents(s: &str) -> Option<u64> {
    parse_dollar_cents_body(s.strip_prefix('$')?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DateTime;

    #[test]
    fn paid_buy_in_with_fee_and_re_entries() {
        let text = "\
Tournament #111111111, Nightly $10 Turbo, Hold'em No Limit
Buy-in: $9.20+$0.80
823 Players
Total Prize Pool: $7,500
Tournament started 2026/03/04 20:00:00
15th : Hero, $12.34
You finished the tournament in 15th place.
You made 2 re-entries and received a total of $12.34.
";
        let summary = parse_summary(text).unwrap();
        assert_eq!(summary.id, 111_111_111);
        assert_eq!(summary.name, "Nightly $10 Turbo");
        assert_eq!(summary.game, GameType::Holdem);
        assert_eq!(summary.buy_in_cents, 920);
        assert_eq!(summary.fee_cents, 80);
        assert_eq!(summary.players, 823);
        assert_eq!(summary.prize_pool_cents, 750_000);
        assert_eq!(
            summary.started_at,
            DateTime {
                year: 2026,
                month: 3,
                day: 4,
                hour: 20,
                minute: 0,
                second: 0,
            }
        );
        assert_eq!(summary.hero_place, 15);
        assert_eq!(summary.hero_prize, Winnings::Dollars(1_234));
        assert_eq!(summary.re_entries, 2);
        assert_eq!(summary.total_received, Winnings::Dollars(1_234));
    }

    #[test]
    fn freeroll_with_coin_prize() {
        let text = "\
Tournament #222222222, Sunday Freeroll, Omaha No Limit
Buy-in: $0
5,000 Players
Total Prize Pool: $500
Tournament started 2026/03/05 12:00:00
1194th : Hero, C$12
You finished the tournament in 1194th place.
You received a total of C$12.
";
        let summary = parse_summary(text).unwrap();
        assert_eq!(summary.buy_in_cents, 0);
        assert_eq!(summary.fee_cents, 0);
        assert_eq!(summary.players, 5_000);
        assert_eq!(summary.hero_place, 1194);
        assert_eq!(summary.hero_prize, Winnings::Coin(1_200));
        assert_eq!(summary.re_entries, 0);
        assert_eq!(summary.total_received, Winnings::Coin(1_200));
    }

    #[test]
    fn ticket_entry_prize() {
        let text = "\
Tournament #333333333, Step 2 - $2 Spin & Gold, Hold'em No Limit
Buy-in: $2
3 Players
Total Prize Pool: $6
Tournament started 2026/03/06 08:30:00
1st : Hero, $10 Entry
You finished the tournament in 1st place.
You received a total of $10 Entry.
";
        let summary = parse_summary(text).unwrap();
        assert_eq!(summary.name, "Step 2 - $2 Spin & Gold");
        assert_eq!(summary.hero_place, 1);
        assert_eq!(summary.hero_prize, Winnings::TicketEntry(1_000));
        assert_eq!(summary.total_received, Winnings::TicketEntry(1_000));
    }

    #[test]
    fn chips_prize() {
        let text = "\
Tournament #444444444, 11 - 0.01 - 0.01 - 3max, Hold'em No Limit
Buy-in: $0.01
3 Players
Total Prize Pool: $0.03
Tournament started 2026/03/07 09:15:00
22th : Hero, 0 chips
You finished the tournament in 22th place.
You received a total of 0 chips.
";
        let summary = parse_summary(text).unwrap();
        assert_eq!(summary.hero_place, 22);
        assert_eq!(summary.hero_prize, Winnings::Chips(0));
        assert_eq!(summary.total_received, Winnings::Chips(0));
    }

    #[test]
    fn sloppy_ordinal_suffix_is_accepted() {
        let text = "\
Tournament #555555555, microFestival: Daily Freeroll, 3K GTD, Hold'em No Limit
Buy-in: $0
100 Players
Total Prize Pool: $0
Tournament started 2026/03/08 10:00:00
43th : Hero, $0
You finished the tournament in 43th place.
You received a total of $0.
";
        let summary = parse_summary(text).unwrap();
        assert_eq!(summary.name, "microFestival: Daily Freeroll, 3K GTD");
        assert_eq!(summary.hero_place, 43);
        assert_eq!(summary.hero_prize, Winnings::Dollars(0));
    }

    #[test]
    fn unrecognized_line_reports_correct_line_number() {
        let text = "\
Tournament #1, Test, Hold'em No Limit
Buy-in: $1
2 Players
Total Prize Pool: $2
Tournament started 2026/01/01 00:00:00
1st : Hero, $1
You finished the tournament in 1st place.
This is not a recognized final line.
";
        let err = parse_summary(text).unwrap_err();
        assert_eq!(err.line, 8);
    }
}
