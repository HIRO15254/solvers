//! `inspect`: solve a postflop config, then explore the resulting average
//! strategy interactively — a small line-based REPL over stdin/stdout, no
//! extra crate dependencies (no rustyline).

use std::io::{self, BufRead, Write};
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use cards::{Card, NUM_COMBOS, PerPlayer, Player, combo_cards};
use engine::{F32Storage, NodeId, NodeKind, PublicTree, ReachMap, Solver};
use game::PayoffPipeline;
use holdem::{PostflopEvaluator, PostflopNodeInfo, class_average, class_weights, range_equity};

use crate::config::{GameSection, SolveConfig};
use crate::postflop_setup;

const RANKS: [char; 13] = [
    'A', 'K', 'Q', 'J', 'T', '9', '8', '7', '6', '5', '4', '3', '2',
];

pub fn run(
    config_path: &Path,
    iterations: Option<u64>,
    target_nash_conv: Option<f64>,
) -> Result<()> {
    let raw = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    let config: SolveConfig = toml::from_str(&raw).context("parsing config")?;

    let GameSection::Postflop {
        board,
        oop_range,
        ip_range,
        pot,
        effective_stack,
        iso_merging,
        bets,
    } = config.game
    else {
        return Err(anyhow!("inspect only supports kind = \"postflop\" configs"));
    };

    let pf_config = postflop_setup::build_postflop_config(
        &board,
        &oop_range,
        &ip_range,
        pot,
        effective_stack,
        iso_merging,
        bets,
    )?;
    let board_cards = pf_config.board.clone();

    let estimate = holdem::memory_usage(&pf_config);
    postflop_setup::print_memory_estimate(estimate);

    let rake = postflop_setup::build_rake(&config.rake);
    let utility = postflop_setup::build_utility(&config.utility);
    let pipeline = PayoffPipeline {
        rake: rake.as_ref(),
        utility: utility.as_ref(),
    };
    let pf_game = holdem::build_postflop_game(&pf_config, pipeline);
    let node_info: Vec<PostflopNodeInfo> = pf_game.node_info.clone();

    let schedule = postflop_setup::build_schedule(&config.algorithm);
    let schedule_name = schedule.name();

    let mut run_cfg = config.run;
    if let Some(it) = iterations {
        run_cfg.iterations = it;
    }
    if let Some(t) = target_nash_conv {
        run_cfg.target_nash_conv = Some(t);
    }

    let mut solver = Solver::<_, F32Storage>::new(pf_game.game, schedule, Some(run_cfg.iterations));
    println!(
        "game=postflop schedule={} iterations={}",
        schedule_name, run_cfg.iterations
    );
    let start = Instant::now();
    crate::solve::run_loop(&mut solver, &run_cfg, &mut crate::solve::RunHooks::none())?;
    let elapsed = start.elapsed();
    crate::solve::print_done(&solver, elapsed);

    let no_color = std::env::var_os("NO_COLOR").is_some();
    let mut repl = Repl {
        solver: &solver,
        node_info: &node_info,
        board: board_cards,
        stack: Vec::new(),
        current: 0,
        history: String::new(),
        equity_cache: None,
        no_color,
    };
    repl.interact()
}

struct Repl<'a> {
    solver: &'a Solver<PostflopEvaluator, F32Storage>,
    node_info: &'a [PostflopNodeInfo],
    board: Vec<Card>,
    /// Ancestors: (node id, that node's history string).
    stack: Vec<(NodeId, String)>,
    current: NodeId,
    /// Current node's history; empty string at root.
    history: String,
    equity_cache: Option<PerPlayer<Vec<f32>>>,
    no_color: bool,
}

impl<'a> Repl<'a> {
    fn interact(&mut self) -> Result<()> {
        let stdin = io::stdin();
        let mut line = String::new();
        loop {
            eprint!("> ");
            io::stdout().flush().ok();
            io::stderr().flush().ok();
            line.clear();
            let n = stdin.lock().read_line(&mut line)?;
            if n == 0 {
                break;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let mut parts = trimmed.splitn(2, char::is_whitespace);
            let cmd = parts.next().unwrap_or("");
            let arg = parts.next().unwrap_or("").trim();
            match cmd {
                "help" => self.cmd_help(),
                "show" => self.cmd_show(),
                "go" => {
                    if arg.is_empty() {
                        println!("error: usage: go <action|index>");
                    } else {
                        self.cmd_go(arg);
                    }
                }
                "up" => self.cmd_up(),
                "root" => self.cmd_root(),
                "grid" => {
                    if arg.is_empty() {
                        println!("error: usage: grid <action|index>");
                    } else {
                        self.cmd_grid(arg);
                    }
                }
                "range" => {
                    if arg.is_empty() {
                        println!("error: usage: range <oop|ip>");
                    } else {
                        self.cmd_range(arg);
                    }
                }
                "eq" => self.cmd_eq(),
                "combos" => {
                    if arg.is_empty() {
                        println!("error: usage: combos <class>");
                    } else {
                        self.cmd_combos(arg);
                    }
                }
                "ev" => self.cmd_ev(),
                "quit" | "exit" => break,
                other => println!("error: unknown command {other:?} (try 'help')"),
            }
        }
        Ok(())
    }

    fn cmd_help(&self) {
        println!("commands:");
        println!("  show                 - describe the current node");
        println!(
            "  go <action|index>    - descend into a child (action label/index, or a dealt card at a chance node)"
        );
        println!("  up                   - go back to the parent node");
        println!("  root                 - jump back to the root");
        println!("  grid <action|index>  - 13x13 grid of per-class action frequency");
        println!("  range <oop|ip>       - 13x13 grid of a player's root-range class weights");
        println!("  eq                   - 13x13 grid of oop's class-averaged equity vs ip");
        println!(
            "  combos <class>       - per-combo action probabilities for a range class (e.g. 'AA', 'AKs')"
        );
        println!("  ev                   - expected values, exploitability, nash_conv");
        println!("  help                 - this message");
        println!("  quit / exit          - leave the REPL");
    }

    fn cmd_show(&mut self) {
        println!(
            "history: {}",
            if self.history.is_empty() {
                "(root)"
            } else {
                self.history.as_str()
            }
        );
        let solver = self.solver;
        let node_info = self.node_info;
        let tree = &solver.game().tree;
        let node = *tree.node(self.current);
        match node.kind {
            NodeKind::Terminal => println!("kind: terminal"),
            NodeKind::Chance => {
                let children: Vec<NodeId> = tree.children(self.current).collect();
                println!("kind: chance ({} deals)", children.len());
                for pos in 0..children.len() {
                    let label = chance_child_label(tree, self.current, pos);
                    println!("  [{pos}] {label}");
                }
            }
            NodeKind::Action => {
                let side = if node.player == Player::P0 {
                    "oop"
                } else {
                    "ip"
                };
                println!("kind: action ({side} to act)");
                let tag = tree.tags[self.current as usize] as usize;
                let info = &node_info[tag];
                let sref = tree.storage_ref(&node);
                let avg = solver.average_strategy_at(self.current);
                let root_range = &solver.game().root_ranges[node.player];
                let freqs = postflop_setup::action_frequencies(
                    &avg,
                    root_range,
                    sref.num_actions as usize,
                    sref.num_hands as usize,
                );
                for (i, (label, freq)) in info.actions.iter().zip(freqs.iter()).enumerate() {
                    println!("  [{i}] {label}: {freq:.3}");
                }
            }
        }
    }

    fn cmd_go(&mut self, arg: &str) {
        let solver = self.solver;
        let node_info = self.node_info;
        let tree = &solver.game().tree;
        let node = *tree.node(self.current);
        match node.kind {
            NodeKind::Terminal => {
                println!("error: terminal node has no children");
            }
            NodeKind::Action => {
                let tag = tree.tags[self.current as usize] as usize;
                let info = &node_info[tag];
                let pos = match resolve_action(&info.actions, arg) {
                    Ok(p) => p,
                    Err(e) => {
                        println!("error: {e}");
                        return;
                    }
                };
                let label = info.actions[pos].clone();
                let Some(child_id) = tree.children(self.current).nth(pos) else {
                    println!("error: action index {pos} out of range");
                    return;
                };
                let new_history = if tree.node(child_id).kind == NodeKind::Action {
                    node_info[tree.tags[child_id as usize] as usize]
                        .history
                        .clone()
                } else {
                    format!("{}{}", self.history, history_token(&label))
                };
                self.stack.push((self.current, self.history.clone()));
                self.current = child_id;
                self.history = new_history;
            }
            NodeKind::Chance => {
                let children: Vec<NodeId> = tree.children(self.current).collect();
                let mut chosen: Option<(usize, String)> = None;
                for pos in 0..children.len() {
                    let label = chance_child_label(tree, self.current, pos);
                    if label.eq_ignore_ascii_case(arg) {
                        chosen = Some((pos, label));
                        break;
                    }
                }
                let (pos, label) = match chosen {
                    Some(c) => c,
                    None => match arg.parse::<usize>() {
                        Ok(idx) if idx < children.len() => {
                            (idx, chance_child_label(tree, self.current, idx))
                        }
                        Ok(idx) => {
                            println!(
                                "error: deal index {idx} out of range (0..{})",
                                children.len()
                            );
                            return;
                        }
                        Err(_) => {
                            println!("error: unknown deal {arg:?}");
                            return;
                        }
                    },
                };
                let child_id = children[pos];
                let new_history = if tree.node(child_id).kind == NodeKind::Action {
                    node_info[tree.tags[child_id as usize] as usize]
                        .history
                        .clone()
                } else {
                    format!("{}[{label}]", self.history)
                };
                self.stack.push((self.current, self.history.clone()));
                self.current = child_id;
                self.history = new_history;
            }
        }
    }

    fn cmd_up(&mut self) {
        match self.stack.pop() {
            Some((node, hist)) => {
                self.current = node;
                self.history = hist;
            }
            None => println!("error: already at root"),
        }
    }

    fn cmd_root(&mut self) {
        self.stack.clear();
        self.current = 0;
        self.history.clear();
    }

    fn cmd_grid(&mut self, arg: &str) {
        let solver = self.solver;
        let node_info = self.node_info;
        let tree = &solver.game().tree;
        let node = *tree.node(self.current);
        if node.kind != NodeKind::Action {
            println!("error: grid only works at an action node");
            return;
        }
        let tag = tree.tags[self.current as usize] as usize;
        let info = &node_info[tag];
        let pos = match resolve_action(&info.actions, arg) {
            Ok(p) => p,
            Err(e) => {
                println!("error: {e}");
                return;
            }
        };
        let sref = tree.storage_ref(&node);
        let num_hands = sref.num_hands as usize;
        let avg = solver.average_strategy_at(self.current);
        let root_range = &solver.game().root_ranges[node.player];
        let freqs = postflop_setup::action_frequencies(
            &avg,
            root_range,
            sref.num_actions as usize,
            num_hands,
        );
        let per_combo = &avg[pos * num_hands..(pos + 1) * num_hands];
        let weights = class_weights(root_range);
        let raw = class_average(root_range, per_combo);
        let mut values = [0.0f64; 169];
        for c in 0..169 {
            values[c] = raw[c] * 100.0;
        }
        println!("{} (overall freq {:.3})", info.actions[pos], freqs[pos]);
        self.render_grid(&weights, &values);
    }

    fn cmd_range(&mut self, arg: &str) {
        let solver = self.solver;
        let player = match arg {
            "oop" => Player::P0,
            "ip" => Player::P1,
            _ => {
                println!("error: expected 'oop' or 'ip'");
                return;
            }
        };
        let root_range = &solver.game().root_ranges[player];
        let w = class_weights(root_range);
        let max = w.iter().cloned().fold(0.0, f64::max);
        let mut values = [0.0f64; 169];
        if max > 0.0 {
            for c in 0..169 {
                if w[c] > 0.0 {
                    values[c] = w[c] / max * 100.0;
                }
            }
        }
        println!("{arg} root range (% of max class weight)");
        self.render_grid(&w, &values);
    }

    fn cmd_eq(&mut self) {
        let solver = self.solver;
        if self.equity_cache.is_none() {
            let eq = range_equity(&self.board, &solver.game().root_ranges);
            self.equity_cache = Some(eq);
        }
        let root_range = &solver.game().root_ranges[Player::P0];
        let equity = &self.equity_cache.as_ref().unwrap()[Player::P0];
        let weights = class_weights(root_range);
        let raw = class_average(root_range, equity);
        let mut values = [0.0f64; 169];
        for c in 0..169 {
            values[c] = raw[c] * 100.0;
        }
        println!("oop equity vs ip range (class-averaged, %)");
        self.render_grid(&weights, &values);
    }

    fn cmd_combos(&mut self, arg: &str) {
        let solver = self.solver;
        let node_info = self.node_info;
        let tree = &solver.game().tree;
        let node = *tree.node(self.current);
        if node.kind != NodeKind::Action {
            println!("error: combos only works at an action node");
            return;
        }
        let class_range = match arg.parse::<cards::Range>() {
            Ok(r) => r,
            Err(e) => {
                println!("error: parsing {arg:?}: {e}");
                return;
            }
        };
        let tag = tree.tags[self.current as usize] as usize;
        let info = &node_info[tag];
        let sref = tree.storage_ref(&node);
        let num_hands = sref.num_hands as usize;
        debug_assert_eq!(num_hands, NUM_COMBOS);
        let avg = solver.average_strategy_at(self.current);
        println!("combos {arg}:");
        for combo in 0..num_hands {
            if class_range.weight(combo) <= 0.0 {
                continue;
            }
            let (hi, lo) = combo_cards(combo);
            let parts: Vec<String> = info
                .actions
                .iter()
                .enumerate()
                .map(|(a, label)| format!("{label}={:.3}", avg[a * num_hands + combo]))
                .collect();
            println!("{hi}{lo} {}", parts.join(" "));
        }
    }

    fn cmd_ev(&mut self) {
        let solver = self.solver;
        let ev_oop = solver.expected_value(Player::P0);
        let ev_ip = solver.expected_value(Player::P1);
        let expl = solver.exploitability();
        let nash_conv = expl[Player::P0] + expl[Player::P1];
        println!(
            "ev_oop={ev_oop:.6} ev_ip={ev_ip:.6} expl_oop={:.3e} expl_ip={:.3e} nash_conv={nash_conv:.3e} iterations={}",
            expl[Player::P0],
            expl[Player::P1],
            solver.iteration(),
        );
    }

    fn render_grid(&self, weights: &[f64; 169], values: &[f64; 169]) {
        print!("    ");
        for &r in &RANKS {
            print!("{r:>3} ");
        }
        println!();
        for (r, &rank) in RANKS.iter().enumerate() {
            print!("{rank:>3} ");
            for c in 0..13 {
                let class = r * 13 + c;
                if weights[class] > 0.0 {
                    let cell = format!("{:>3} ", values[class].round() as i64);
                    print!("{}", self.colorize(&cell, values[class]));
                } else {
                    print!(" -- ");
                }
            }
            println!();
        }
    }

    fn colorize(&self, cell: &str, pct: f64) -> String {
        if self.no_color {
            return cell.to_string();
        }
        let t = (pct / 100.0).clamp(0.0, 1.0);
        let (r, g, b) = if t <= 0.5 {
            (255u8, (255.0 * (t / 0.5)).round() as u8, 0u8)
        } else {
            ((255.0 * (1.0 - (t - 0.5) / 0.5)).round() as u8, 255u8, 0u8)
        };
        format!("\x1b[48;2;{r};{g};{b}m\x1b[30m{cell}\x1b[0m")
    }
}

/// Resolves a `go`/`grid` argument against an action node's label list: an
/// exact label match first, else a positional index.
fn resolve_action(actions: &[String], arg: &str) -> Result<usize, String> {
    if let Some(p) = actions.iter().position(|a| a == arg) {
        return Ok(p);
    }
    match arg.parse::<usize>() {
        Ok(idx) if idx < actions.len() => Ok(idx),
        Ok(idx) => Err(format!(
            "action index {idx} out of range (0..{})",
            actions.len()
        )),
        Err(_) => Err(format!("unknown action {arg:?}")),
    }
}

/// Maps an action label to the history token the tree builder would have
/// appended: `check`->`"x"`, `fold`->`"f"`, `call`->`"c"`, and bet/raise
/// labels (`"bet 30"`, `"raise to 30"`) to `"b{amount}"` using the label's
/// last whitespace-separated token as the amount.
fn history_token(label: &str) -> String {
    match label {
        "check" => "x".to_string(),
        "fold" => "f".to_string(),
        "call" => "c".to_string(),
        _ => {
            let amount = label.rsplit(' ').next().unwrap_or(label);
            format!("b{amount}")
        }
    }
}

/// Identifies the single card a chance mask removes: the mask is 0 for
/// every combo containing that card and 1 otherwise.
fn identify_card(mask: &[f32]) -> Option<Card> {
    for idx in 0..52u8 {
        let card = Card::from_index(idx);
        let ok = (0..NUM_COMBOS).all(|combo| {
            let (c1, c2) = combo_cards(combo);
            let expect = if c1 == card || c2 == card { 0.0 } else { 1.0 };
            mask[combo] == expect
        });
        if ok {
            return Some(card);
        }
    }
    None
}

/// Label for a chance node's `pos`-th child: the dealt card, derived from
/// its reach mask (chance children are untagged in `node_info`, so this is
/// the only way to name them).
fn chance_child_label(tree: &PublicTree, node_id: NodeId, pos: usize) -> String {
    let node = tree.node(node_id);
    let deal = tree.deal(node, pos);
    match deal.maps[Player::P0] {
        ReachMap::Mask(m) => identify_card(&tree.masks[m as usize])
            .map(|c| c.to_string())
            .unwrap_or_else(|| format!("deal{pos}")),
        _ => format!("deal{pos}"),
    }
}
