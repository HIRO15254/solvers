//! `inspect`: solve a postflop config, then explore the resulting average
//! strategy interactively — a small line-based REPL over stdin/stdout, no
//! extra crate dependencies (no rustyline).

use std::io::{self, BufRead, Write};
use std::path::Path;

use anyhow::{Context, Result};
use hu_postflop::artifact::{SolProvider, StrategyProvider};
use hu_postflop::queries::{self, LiveProvider, chance_child_label, history_token, resolve_action};
use hu_postflop::{
    Card, CompiledGame, NodeId, NodeKind, PerPlayer, Player, PostflopEvaluator, PostflopNodeInfo,
};

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
    let p = hu_postflop::prepare::prepare(&raw, config_path)?;
    queries::with_live(
        &p,
        iterations,
        target_nash_conv,
        &crate::CLI_CANCEL,
        |solved| {
            crate::nlh_v1::print_done(&solved.summary);
            let mut provider = LiveProvider(solved);
            let mut repl = Repl {
                game: solved.game(),
                provider: &mut provider,
                node_info: &solved.node_info,
                board: solved.board.clone(),
                stack: Vec::new(),
                current: 0,
                history: String::new(),
                equity_cache: None,
                no_color: std::env::var_os("NO_COLOR").is_some(),
            };
            repl.interact()
        },
    )
}

/// Loads a `.sol` viewer artifact and explores it interactively -- the same
/// REPL as [`run`], but sourcing strategies from
/// [`hu_postflop::artifact::SolProvider`] (dequantized stored blocks, with river
/// subgames re-solved lazily) instead of a live solve.
pub fn run_sol(sol_path: &Path, river_iterations: u64, river_target: Option<f64>) -> Result<()> {
    let loaded = hu_postflop::artifact::load_sol(
        sol_path,
        river_iterations,
        river_target,
        &mut crate::nlh_v1::print_artifact,
    )?;
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
    let mut provider = SolProvider::new(&loaded, Box::new(crate::nlh_v1::print_artifact));
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
    equity_cache: Option<(NodeId, PerPlayer<Vec<f32>>)>,
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
        queries::reach_here(self.game, self.provider, self.current)
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
        println!(
            "  eq                   - 13x13 grid of oop equity vs ip at the current board and reach"
        );
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
                    let label = chance_child_label(self.game, node_info, self.current, pos);
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
                let freqs = queries::action_frequencies(self.game, self.current, &avg, &reach);
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
                    let label = chance_child_label(self.game, node_info, self.current, pos);
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
                        Ok(idx) if idx < children.len() => (
                            idx,
                            chance_child_label(self.game, node_info, self.current, idx),
                        ),
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
        let freqs = queries::action_frequencies(self.game, self.current, &avg, &reach);
        let grid = queries::action_grid(self.game, self.current, pos, &avg, &reach);
        println!("{} (overall freq {:.3})", info.actions[pos], freqs[pos]);
        self.render_grid(&grid.weights, &grid.values);
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
        let grid = queries::range_grid(self.game, player);
        println!("{arg} root range (% of max class weight)");
        self.render_grid(&grid.weights, &grid.values);
    }

    fn cmd_eq(&mut self) {
        let reach = match self.reach_here() {
            Ok(reach) => reach,
            Err(error) => {
                println!("error: {error}");
                return;
            }
        };
        if !self
            .equity_cache
            .as_ref()
            .is_some_and(|(id, _)| *id == self.current)
        {
            self.equity_cache = Some((
                self.current,
                queries::equity(self.game, &self.board, &self.history, &reach),
            ));
        }
        let grid = queries::equity_grid(self.game, &reach, &self.equity_cache.as_ref().unwrap().1);
        println!("oop equity vs ip at current node (class-averaged, %)");
        self.render_grid(&grid.weights, &grid.values);
    }

    fn cmd_combos(&mut self, arg: &str) {
        let node_info = self.node_info;
        let tree = &self.game.tree;
        let node = *tree.node(self.current);
        if node.kind != NodeKind::Action {
            println!("error: combos only works at an action node");
            return;
        }
        let class = match queries::parse_class(arg) {
            Ok(r) => r,
            Err(e) => {
                println!("error: {e}");
                return;
            }
        };
        let tag = tree.tags[self.current as usize] as usize;
        let info = &node_info[tag];
        let avg = match self.provider.average_strategy(self.current) {
            Ok(avg) => avg,
            Err(e) => {
                println!("error: {e}");
                return;
            }
        };
        println!("combos {arg}:");
        let rows = queries::combo_rows(
            &class,
            &self.game.evaluator.hands,
            node.player,
            info.actions.len(),
            &avg,
        );
        if rows.is_empty() {
            println!("error: {arg} is not in the acting player range");
            return;
        }
        for row in rows {
            let parts: Vec<String> = info
                .actions
                .iter()
                .zip(row.probabilities)
                .map(|(label, p)| format!("{label}={p:.3}"))
                .collect();
            println!("{} {}", row.combo, parts.join(" "));
        }
    }

    fn cmd_ev(&mut self) {
        let s = self.provider.ev_summary();
        if s.at_export {
            print!("at export: ");
        }
        println!(
            "ev_oop={:.6} ev_ip={:.6} expl_oop={:.3e} expl_ip={:.3e} nash_conv={:.3e} iterations={}",
            s.ev[0], s.ev[1], s.expl[0], s.expl[1], s.nash_conv, s.iterations
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
