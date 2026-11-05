//! Terminal rendering and line-oriented navigation for saved P2 solutions.
use super::Color;
use anyhow::{Context, Result, bail};
use mw_preflop::browse::{Solution, class_combos, class_label};
use std::{
    io::{self, BufRead, IsTerminal, Write},
    path::Path,
};

pub(super) fn run(file: &Path, config: Option<&Path>, color: Color, selector: &str) -> Result<()> {
    let solution = Solution::load_with_config(file, config).map_err(|error| {
        if error.chain().any(|cause| {
            cause.is::<mw_preflop::mwsol::MwSolError>() || cause.is::<std::io::Error>()
        }) || format!("{error:#}").contains("removed config family")
        {
            error
        } else {
            crate::derive::InputError(error).into()
        }
    })?;
    let mut current = solution
        .select(selector)
        .map_err(crate::derive::InputError)?;
    let color = match color {
        Color::Always => true,
        Color::Never => false,
        Color::Auto => io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none(),
    };
    let interactive = io::stdin().is_terminal();
    show(&solution, current)?;
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    loop {
        if interactive {
            print!("browse> ");
            io::stdout().flush()?;
        }
        let Some(line) = lines.next() else {
            break;
        };
        let line = line?;
        if matches!(line.trim(), "quit" | "exit") {
            break;
        }
        if let Err(error) = command(&solution, &mut current, line.trim(), color) {
            println!("error: {error:#}");
        }
    }
    Ok(())
}
fn seat(solution: &Solution, actor: usize) -> String {
    solution.positions.get(actor).map_or_else(
        || format!("seat {actor}"),
        |p| format!("seat {actor} ({p})"),
    )
}
fn show(solution: &Solution, current: usize) -> Result<()> {
    let node = &solution.nodes[current];
    println!("{}", solution.kind);
    let path = display_path(solution, current);
    println!("path: {path}");
    println!("actor: {}", display_actor(solution, current));
    if let Some(pot) = node.pot_bb {
        println!(
            "pot: {pot:.3} BB; stacks: {:?} BB",
            node.stacks_bb.as_ref().unwrap()
        );
    } else {
        println!("pot/stacks/positions: not available");
    }
    legend(solution, current)
}
fn display_path(solution: &Solution, current: usize) -> String {
    let path = solution
        .path(current)
        .iter()
        .map(|&(n, a)| {
            format!(
                "{} {}",
                solution
                    .positions
                    .get(solution.nodes[n].actor.unwrap())
                    .cloned()
                    .unwrap_or_else(|| seat(solution, solution.nodes[n].actor.unwrap())),
                solution.nodes[n].actions[a]
            )
        })
        .collect::<Vec<_>>()
        .join(" / ");
    if path.is_empty() { "root".into() } else { path }
}
fn display_actor(solution: &Solution, current: usize) -> String {
    solution.nodes[current].actor.map_or_else(
        || "terminal / preflop boundary".into(),
        |a| seat(solution, a),
    )
}
fn legend(solution: &Solution, current: usize) -> Result<()> {
    let (frequencies, covered) = solution.frequencies(current)?;
    for (i, action) in solution.nodes[current].actions.iter().enumerate() {
        if covered > 0.0 {
            println!("  {i}: {action}  {:.2}%", frequencies[i] * 100.0);
        } else {
            println!("  {i}: {action}  not available");
        }
    }
    if solution.nodes[current].actor.is_some() {
        println!("known strategy/reach coverage: {covered:.3} combos");
    }
    Ok(())
}
fn command(solution: &Solution, current: &mut usize, line: &str, color: bool) -> Result<()> {
    let (cmd, arg) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let arg = arg.trim();
    match cmd {
        "" | "show" => show(solution, *current)?,
        "go" => {
            let action = solution.action(*current, arg)?;
            *current = solution.child(*current, action)?;
            show(solution, *current)?;
        }
        "up" => {
            *current = solution.nodes[*current]
                .parent
                .map_or(solution.root, |(n, _)| n);
            show(solution, *current)?;
        }
        "root" => {
            *current = solution.root;
            show(solution, *current)?;
        }
        "path" => println!("selector: {}", solution.selector(*current)),
        "strat" | "mix" => {
            println!(
                "path: {}; actor: {}",
                display_path(solution, *current),
                display_actor(solution, *current)
            );
            strategy_grid(solution, *current, None, color)?;
            legend(solution, *current)?;
        }
        "grid" => {
            let action = solution.action(*current, arg)?;
            strategy_grid(solution, *current, Some(action), color)?;
        }
        "range" => {
            let actor = if arg.is_empty() {
                solution.nodes[*current]
                    .actor
                    .context("terminal node: specify a seat")?
            } else {
                solution
                    .positions
                    .iter()
                    .position(|p| p == arg)
                    .or_else(|| arg.parse::<usize>().ok())
                    .context("range expects a seat index or known position")?
            };
            let range = solution.range(*current, actor)?;
            println!(
                "range {}: {:.3} known combos{}",
                seat(solution, actor),
                range.iter().flatten().sum::<f64>(),
                if range.iter().any(Option::is_none) {
                    " (incomplete; unknown classes omitted)"
                } else {
                    ""
                }
            );
            let density: Vec<_> = range
                .iter()
                .enumerate()
                .map(|(c, w)| w.map(|w| w / class_combos(c)))
                .collect();
            let max = density.iter().flatten().copied().fold(0.0, f64::max);
            render(|c| {
                density[c].filter(|w| *w > 0.0).map_or_else(
                    || format!("{:>8}", "--"),
                    |w| format!("{:>7.0}%", w / max * 100.0),
                )
            });
        }
        "combos" => {
            let class = (0..169)
                .find(|&c| class_label(c).eq_ignore_ascii_case(arg))
                .context("unknown class; use AA, AKs or AKo")?;
            let actor = solution.nodes[*current]
                .actor
                .context("terminal node has no strategy")?;
            let range = solution.range(*current, actor)?;
            println!(
                "{} (class unit): reach {:?} combos",
                class_label(class),
                range[class]
            );
            if let Some(row) = &solution.nodes[*current].probabilities[class] {
                for (a, p) in solution.nodes[*current].actions.iter().zip(row) {
                    println!("  {a}: {:.2}%", p * 100.0);
                }
            } else {
                println!("strategy: not available (unvisited)");
            }
        }
        "ev" => {
            if let Some(sweeps) = solution.sweeps {
                println!(
                    "saved artifact: sweeps={sweeps}; stop={}",
                    solution.stop_status.as_deref().unwrap_or("unknown")
                );
            }
            if solution.ev.is_empty() {
                println!("EV: not available");
            } else {
                println!(
                    "saved root summary ({}; not per-node EV)",
                    solution.ev_unit.unwrap_or("unknown units")
                );
                for e in &solution.ev {
                    println!(
                        "{}: profile EV {:?}; deviation {:?}; average positive regret {:.6}; strategy drift {:.6}",
                        seat(solution, usize::from(e.seat)),
                        e.profile_ev,
                        e.deviation_gain_lower_bound,
                        e.average_positive_regret,
                        e.strategy_drift_l1
                    );
                }
            }
        }
        "help" => println!(
            "show (or empty line), go <action|index>, <index>, up, root, path, strat/mix, grid <action|index>, range [seat|position], combos <class>, ev, help, quit/exit"
        ),
        _ if cmd.parse::<usize>().is_ok() && arg.is_empty() => {
            let action = solution.action(*current, cmd)?;
            *current = solution.child(*current, action)?;
            show(solution, *current)?;
        }
        _ => bail!("unknown command {cmd:?}; use help"),
    }
    Ok(())
}
fn render(mut cell: impl FnMut(usize) -> String) {
    const RANKS: &str = "AKQJT98765432";
    print!("    ");
    for r in RANKS.chars() {
        print!("{r:>8} ");
    }
    println!();
    for (row, rank) in RANKS.chars().enumerate() {
        print!("{rank:>3} ");
        for col in 0..13 {
            print!("{} ", cell(row * 13 + col));
        }
        println!();
    }
}
fn strategy_grid(
    solution: &Solution,
    current: usize,
    action: Option<usize>,
    color: bool,
) -> Result<()> {
    let node = &solution.nodes[current];
    let actor = node.actor.context("terminal node has no strategy")?;
    let range = solution.range(current, actor)?;
    // Stored labels keep the all-in flag and chip amounts that colors and short names use.
    let palette = palette(&node.stored_actions);
    let short: Vec<_> = node
        .stored_actions
        .iter()
        .enumerate()
        .map(|(i, a)| {
            if a.contains("all-in") || a == "a" {
                "AI".into()
            } else if a == "fold" {
                "F".into()
            } else if a == "check" {
                "X".into()
            } else if a.starts_with("call") {
                "C".into()
            } else {
                format!("R{i}")
            }
        })
        .collect();
    println!(
        "{}",
        action.map_or_else(
            || "strategy (dominant action and probability)".into(),
            |a| format!("grid {}", node.actions[a])
        )
    );
    render(|c| {
        let Some(row) = &node.probabilities[c] else {
            return format!("{:>8}", "--");
        };
        if !range[c].is_some_and(|w| w > 0.0) {
            return format!("{:>8}", "--");
        }
        let dominant = row
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .unwrap()
            .0;
        let text = match action {
            Some(a) => format!("{:>7.0}%", row[a] * 100.0),
            None => format!("{:>3}{:>4.0}%", short[dominant], row[dominant] * 100.0),
        };
        if !color {
            return text;
        }
        let mut rgb = [0.0; 3];
        for (i, p) in row.iter().enumerate() {
            let weight = action.map_or(*p, |a| if i == a { row[a] } else { 0.0 });
            for (channel, value) in rgb.iter_mut().enumerate() {
                *value += weight * palette[i][channel];
            }
        }
        format!(
            "\x1b[48;2;{};{};{}m\x1b[97m{text}\x1b[0m",
            rgb[0].round() as u8,
            rgb[1].round() as u8,
            rgb[2].round() as u8
        )
    });
    if action.is_none() {
        for (i, label) in short.iter().enumerate() {
            println!("{label} = {}", node.actions[i]);
        }
    }
    Ok(())
}
fn palette(actions: &[String]) -> Vec<[f64; 3]> {
    let mut raises: Vec<_> = actions
        .iter()
        .enumerate()
        .filter(|(_, a)| (a.starts_with("raise") || a.starts_with("bet")) && !a.contains("all-in"))
        .map(|(i, a)| {
            (
                i,
                a.split(':')
                    .nth(1)
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(i as f64),
            )
        })
        .collect();
    raises.sort_by(|a, b| a.1.total_cmp(&b.1));
    actions
        .iter()
        .enumerate()
        .map(|(i, a)| {
            if a.contains("all-in") || a == "a" {
                [130.0, 15.0, 25.0]
            } else if a == "fold" {
                [35.0, 90.0, 190.0]
            } else if a == "check" || a.starts_with("call") {
                [30.0, 155.0, 75.0]
            } else {
                let order = raises.iter().position(|(j, _)| *j == i).unwrap_or(0) as f64;
                let t = order / (raises.len().saturating_sub(1).max(1) as f64);
                [240.0, 145.0 - 110.0 * t, 30.0]
            }
        })
        .collect()
}
