//! `inspect`: solve a postflop config, then explore the resulting average
//! strategy interactively — a small line-based REPL over stdin/stdout, no
//! extra crate dependencies (no rustyline).

use std::io::{self, BufRead, Write};
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use cards::{Card, NUM_COMBOS, PerPlayer, Player, combo_cards};
use engine::{CompiledGame, F32Storage, NodeId, NodeKind, PublicTree, ReachMap, Solver};
use game::PayoffPipeline;
use holdem::{PostflopEvaluator, PostflopNodeInfo, class_average, class_weights, range_equity};

use crate::config::GameSection;
use crate::postflop_setup;
use crate::sol::{LiveProvider, SolProvider, StrategyProvider};

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
    let config =
        crate::config::parse_solve_config_at(&raw, config_path).context("parsing config")?;

    match &config.game {
        GameSection::Postflop { .. } => {}
        GameSection::Preflop { .. } => {
            return Err(anyhow!(
                "inspect does not support preflop configs yet (kind = \"preflop\")"
            ));
        }
        _ => {
            return Err(anyhow!("inspect only supports kind = \"postflop\" configs"));
        }
    }
    let GameSection::Postflop {
        board,
        oop_range,
        ip_range,
        pot,
        effective_stack,
        iso_merging,
        min_bet,
        preflop_aggressor,
        tree,
    } = config.game
    else {
        unreachable!("checked above");
    };

    let pf_config = postflop_setup::build_postflop_config(
        &board,
        &oop_range,
        &ip_range,
        pot,
        effective_stack,
        iso_merging,
        min_bet,
        tree.lower()?,
        &preflop_aggressor,
    )?;
    let board_cards = pf_config.board.clone();

    let estimate = holdem::memory_usage(&pf_config);
    postflop_setup::print_memory_estimate(estimate);

    let rake = crate::economics::build_rake(&config.rake)?;
    let utility = crate::economics::build_utility(&config.utility)?;
    let pipeline = PayoffPipeline {
        rake: rake.as_ref(),
        utility: utility.as_ref(),
    };
    let ev_offset = postflop_setup::subgame_ev_offset(&pf_config, pipeline.utility);
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
    crate::solve::print_done(
        &solver,
        elapsed,
        postflop_setup::subgame_ev(crate::solve::solver_ev(&solver), ev_offset),
    );

    let no_color = std::env::var_os("NO_COLOR").is_some();
    let mut provider = LiveProvider {
        solver: &solver,
        ev_offset,
    };
    let mut repl = Repl {
        game: solver.game(),
        provider: &mut provider,
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

/// Loads a `.sol` viewer artifact and explores it interactively -- the same
/// REPL as [`run`], but sourcing strategies from
/// [`crate::sol::SolProvider`] (dequantized stored blocks, with river
/// subgames re-solved lazily) instead of a live [`Solver`].
pub fn run_sol(sol_path: &Path, river_iterations: u64, river_target: Option<f64>) -> Result<()> {
    let loaded = crate::sol::load_sol(sol_path, river_iterations, river_target)?;
    println!(
        "loaded {} (mode={:?}, iterations={}, nash_conv={:.3e})",
        sol_path.display(),
        loaded.mode,
        loaded.meta.iterations,
        loaded.meta.nash_conv,
    );

    let no_color = std::env::var_os("NO_COLOR").is_some();
    let board = loaded.board.clone();
    let node_info = &loaded.pf_game.node_info;
    let game = &loaded.pf_game.game;
    let mut provider = SolProvider::new(&loaded);
    let mut repl = Repl {
        game,
        provider: &mut provider,
        node_info,
        board,
        stack: Vec::new(),
        current: 0,
        history: String::new(),
        equity_cache: None,
        no_color,
    };
    repl.interact()
}

struct Repl<'a> {
    game: &'a CompiledGame<PostflopEvaluator>,
    provider: &'a mut dyn StrategyProvider,
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
    /// Both players' reach at the current node.
    ///
    /// Action frequencies and per-hand values are only meaningful against
    /// the range that actually arrives at a node. Weighting by the root
    /// range instead — which this used to do — answers a different
    /// question: it counts hands that could never have got here.
    fn reach_here(&mut self) -> Result<PerPlayer<Vec<f32>>> {
        let tree = &self.game.tree;
        let root_slices = PerPlayer::new(
            self.game.root_ranges[Player::P0].as_slice(),
            self.game.root_ranges[Player::P1].as_slice(),
        );
        let provider = &mut self.provider;
        let mut failure = None;
        let reach =
            engine::reach_at(
                tree,
                root_slices,
                self.current,
                |id, _sref, out| match provider.average_strategy(id) {
                    Ok(avg) => out.copy_from_slice(&avg),
                    Err(error) => failure = Some(error),
                },
            );
        match failure {
            Some(error) => Err(error),
            None => Ok(reach),
        }
    }

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
        println!(
            "note: iso-merged trunks list representative cards only for a merged deal \
             (marked with a trailing '*', e.g. 'Td*'); member remapping is deferred."
        );
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
        let node_info = self.node_info;
        let tree = &self.game.tree;
        let node = *tree.node(self.current);
        match node.kind {
            NodeKind::Terminal => println!("kind: terminal"),
            NodeKind::Chance => {
                let children: Vec<NodeId> = tree.children(self.current).collect();
                println!("kind: chance ({} deals)", children.len());
                for pos in 0..children.len() {
                    let label = chance_child_label(tree, node_info, self.current, pos);
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
                let avg = match self.provider.average_strategy(self.current) {
                    Ok(avg) => avg,
                    Err(e) => {
                        println!("error: {e}");
                        return;
                    }
                };
                let reach = match self.reach_here() {
                    Ok(reach) => reach,
                    Err(e) => {
                        println!("error: {e}");
                        return;
                    }
                };
                let weight = &reach[node.player];
                let freqs = postflop_setup::action_frequencies(
                    &avg,
                    weight,
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
        let node_info = self.node_info;
        let tree = &self.game.tree;
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
                    let label = chance_child_label(tree, node_info, self.current, pos);
                    // Accept the label with or without its trailing `*`
                    // (see `chance_child_label`'s doc comment): an
                    // iso-merged deal's representative card is still a
                    // sensible thing to type without the marker.
                    if label.eq_ignore_ascii_case(arg)
                        || label.trim_end_matches('*').eq_ignore_ascii_case(arg)
                    {
                        chosen = Some((pos, label));
                        break;
                    }
                }
                let (pos, label) = match chosen {
                    Some(c) => c,
                    None => match arg.parse::<usize>() {
                        Ok(idx) if idx < children.len() => {
                            (idx, chance_child_label(tree, node_info, self.current, idx))
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
                    format!("{}[{}]", self.history, label.trim_end_matches('*'))
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
        let node_info = self.node_info;
        let tree = &self.game.tree;
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
        let avg = match self.provider.average_strategy(self.current) {
            Ok(avg) => avg,
            Err(e) => {
                println!("error: {e}");
                return;
            }
        };
        let reach = match self.reach_here() {
            Ok(reach) => reach,
            Err(e) => {
                println!("error: {e}");
                return;
            }
        };
        let weight = &reach[node.player];
        let freqs =
            postflop_setup::action_frequencies(&avg, weight, sref.num_actions as usize, num_hands);
        let per_combo = &avg[pos * num_hands..(pos + 1) * num_hands];
        let weights = class_weights(weight);
        let raw = class_average(weight, per_combo);
        let mut values = [0.0f64; 169];
        for c in 0..169 {
            values[c] = raw[c] * 100.0;
        }
        println!("{} (overall freq {:.3})", info.actions[pos], freqs[pos]);
        self.render_grid(&weights, &values);
    }

    fn cmd_range(&self, arg: &str) {
        let player = match arg {
            "oop" => Player::P0,
            "ip" => Player::P1,
            _ => {
                println!("error: expected 'oop' or 'ip'");
                return;
            }
        };
        let root_range = &self.game.root_ranges[player];
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
        if self.equity_cache.is_none() {
            let eq = range_equity(&self.board, &self.game.root_ranges);
            self.equity_cache = Some(eq);
        }
        let root_range = &self.game.root_ranges[Player::P0];
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
        let node_info = self.node_info;
        let tree = &self.game.tree;
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
        let avg = match self.provider.average_strategy(self.current) {
            Ok(avg) => avg,
            Err(e) => {
                println!("error: {e}");
                return;
            }
        };
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
        println!("{}", self.provider.ev_line());
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
pub(crate) fn resolve_action(actions: &[String], arg: &str) -> Result<usize, String> {
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
/// labels (`"bet 30"`, `"raise to 30"`) to `"r{amount}"` using the label's
/// last whitespace-separated token as the amount. Bets and raises share one
/// token because both simply name the wager the actor moves to.
fn history_token(label: &str) -> String {
    match label {
        "check" => "x".to_string(),
        "fold" => "f".to_string(),
        "call" => "c".to_string(),
        _ => {
            let amount = label.rsplit(' ').next().unwrap_or(label);
            format!("r{amount}")
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

/// Label for a chance node's `pos`-th child: the dealt card. For a `Mask`
/// deal (the common unmerged case) this comes straight from the card-removal
/// mask via [`identify_card`]. For a `Transition` deal (an iso-merged class:
/// several structurally-equivalent cards folded into one branch) there is no
/// single mask to read a card from, so instead this reads the representative
/// card straight out of the child action node's own recorded history -- its
/// last bracketed `[Xy]` token is exactly the card the builder dealt to
/// reach it -- and appends `*` to mark that the branch stands in for a whole
/// merged class (see `cmd_help`'s iso-merging note). Falls back to a bare
/// `dealN` when neither source applies (e.g. an `Identity` map, which this
/// builder never uses at a chance node, or an untagged/non-action child).
pub(crate) fn chance_child_label(
    tree: &PublicTree,
    node_info: &[PostflopNodeInfo],
    node_id: NodeId,
    pos: usize,
) -> String {
    let node = tree.node(node_id);
    let deal = tree.deal(node, pos);
    match deal.maps[Player::P0] {
        ReachMap::Mask(m) => identify_card(&tree.masks[m as usize])
            .map(|c| c.to_string())
            .unwrap_or_else(|| format!("deal{pos}")),
        ReachMap::Transition(_) => tree
            .children(node_id)
            .nth(pos)
            .filter(|&child_id| tree.node(child_id).kind == NodeKind::Action)
            .and_then(|child_id| {
                let tag = tree.tags[child_id as usize] as usize;
                last_bracketed_card(&node_info[tag].history)
            })
            .map(|card| format!("{card}*"))
            .unwrap_or_else(|| format!("deal{pos}")),
        ReachMap::Identity => format!("deal{pos}"),
    }
}

/// Extracts the card inside the last `[Xy]` token in a history string (e.g.
/// `"xx[9d]"` -> `Some("9d")`), or `None` if there is no bracketed token.
fn last_bracketed_card(history: &str) -> Option<&str> {
    let start = history.rfind('[')?;
    let end = history[start..].find(']')? + start;
    Some(&history[start + 1..end])
}
