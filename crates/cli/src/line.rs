use anyhow::{Context, Result};
use std::path::Path;

use crate::validate::ValidationFormat;

pub fn run(path: &Path, moves: Option<&str>, format: ValidationFormat) -> Result<()> {
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let moves: Option<Vec<String>> = moves.map(|text| {
        if text.is_empty() {
            Vec::new()
        } else {
            text.split(',').map(str::to_owned).collect()
        }
    });
    let cursor = spot::line_cursor(&raw, moves.as_deref())?;
    match format {
        ValidationFormat::Json => {
            let mut value = serde_json::to_value(&cursor)?;
            crate::validate::diagnostic_bb(&mut value["actions"]);
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        ValidationFormat::Human => print_human(&cursor),
    }
    Ok(())
}

fn print_human(cursor: &spot::LineCursor) {
    println!(
        "line: {}",
        if cursor.line.is_empty() {
            "(empty)"
        } else {
            &cursor.line
        }
    );
    println!("street: {:?}", cursor.street);
    println!("pot: {} BB", cursor.pot);
    println!("players: {}", cursor.players);
    println!("button: {:?}", cursor.button);
    for seat in &cursor.seats {
        println!(
            "  {}: stack={} BB; wager={} BB; committed={} BB; {}",
            seat.position, seat.stack, seat.wager, seat.committed, seat.status
        );
    }
    println!("complete: {}", cursor.complete);
    println!("terminal: {}", cursor.terminal);
    if let Some(cards) = cursor.board_cards {
        println!("board cards: {cards}");
    }
    if let Some((first, rest)) = cursor.moves.split_first() {
        print!("moves: {first}");
        for token in rest {
            print!(",{token}");
        }
        println!();
    }
    if let Some(next) = &cursor.next {
        println!("next: {} ({:?})", next.position, next.street);
        println!(
            "  to call: {} BB; stack: {} BB; wager: {} BB",
            next.to_call, next.stack, next.wager
        );
        println!("options:");
        for option in &next.options {
            print!("  {:?} ({})", option.kind, option.token);
            if let Some(amount) = option.amount {
                print!(" {amount} BB");
            }
            if let Some(min) = option.min {
                print!(" min={min} BB");
            }
            if let Some(max) = option.max {
                print!(" max={max} BB");
            }
            println!();
        }
    }
}
