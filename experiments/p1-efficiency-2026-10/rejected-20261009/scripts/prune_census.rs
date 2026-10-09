//! Research census (throwaway): how much CFR work sits under all-zero
//! reaches at given iterations. For each updating player it walks the tree
//! with the current (regret-matching) strategies and classifies terminal
//! kernel work and storage elements by whether the updater's reach and the
//! opponent's reach are all zero.
//!
//! ```text
//! cargo run --release -p hu-postflop --example prune_census -- CONFIG --at 10,50,100,200 [--threads N]
//! ```

use std::path::PathBuf;

use anyhow::{Context, Result};
use hu_engine::{F32Storage, NodeId, NodeKind, ParConfig, Solver, TerminalEvaluator};
use hu_postflop::{Player, prepare};

#[derive(Default, Clone, Copy)]
struct Stats {
    // index: 0 live, 1 updater zero only, 2 opponent zero only, 3 both zero
    terminal: [f64; 4],
    own_elems: [f64; 4],
    opp_elems: [f64; 4],
}

fn all_zero(v: &[f32]) -> bool {
    v.iter().all(|&x| x == 0.0)
}

fn walk<E: TerminalEvaluator>(
    solver: &Solver<E, F32Storage>,
    p: Player,
    node_id: NodeId,
    my: &[f32],
    opp: &[f32],
    stats: &mut Stats,
) {
    let tree = &solver.game().tree;
    let node = *tree.node(node_id);
    let cat = 2 * usize::from(all_zero(opp)) + usize::from(all_zero(my));
    match node.kind {
        NodeKind::Terminal => {
            stats.terminal[cat] += (my.len() + opp.len()) as f64;
        }
        NodeKind::Chance => {
            for (pos, child) in tree.children(node_id).enumerate() {
                let deal = *tree.deal(&node, pos);
                let my_next = tree.map_reach(deal.maps[p], my);
                let opp_next = tree.map_reach(deal.maps[p.opponent()], opp);
                walk(solver, p, child, &my_next, &opp_next, stats);
            }
        }
        NodeKind::Action => {
            let sref = tree.storage_ref(&node);
            let hands = sref.num_hands as usize;
            let sigma = solver.current_strategy_at(node_id);
            let own = node.player == p;
            if own {
                stats.own_elems[cat] += sref.len() as f64;
            } else {
                stats.opp_elems[cat] += sref.len() as f64;
            }
            for (a, child) in tree.children(node_id).enumerate() {
                let row = &sigma[a * hands..(a + 1) * hands];
                if own {
                    let next: Vec<f32> = my.iter().zip(row).map(|(r, s)| r * s).collect();
                    walk(solver, p, child, &next, opp, stats);
                } else {
                    let next: Vec<f32> = opp.iter().zip(row).map(|(r, s)| r * s).collect();
                    walk(solver, p, child, my, &next, stats);
                }
            }
        }
    }
}

fn frac(v: [f64; 4]) -> serde_json::Value {
    let total: f64 = v.iter().sum();
    serde_json::json!({
        "total": total,
        "live": v[0] / total,
        "myZero": v[1] / total,
        "oppZero": v[2] / total,
        "bothZero": v[3] / total,
    })
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut config = None;
    let mut at = vec![10u64, 50, 100, 200];
    let mut threads = std::thread::available_parallelism()?.get();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--at" => {
                at = args
                    .next()
                    .context("--at needs a value")?
                    .split(',')
                    .map(str::parse)
                    .collect::<Result<_, _>>()?
            }
            "--threads" => threads = args.next().context("--threads needs a value")?.parse()?,
            other => config = Some(PathBuf::from(other)),
        }
    }
    let config = config.context("missing CONFIG")?;
    let raw = std::fs::read_to_string(&config)?;
    let prepared = prepare::prepare(&raw, &config)?;
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build()?;
    pool.install(|| -> Result<()> {
        let game = hu_postflop::try_build_postflop_game(&prepared.config, prepared.payoff.pipeline())?;
        let mut solver = Solver::<_, F32Storage>::new(
            game.game,
            hu_postflop::run::schedule(&prepared.settings.solver.algorithm),
            Some(prepared.settings.solver.stop.max_iterations),
        );
        solver.set_cfr_precision(prepared.settings.solver.cfr_precision);
        solver.set_par(ParConfig {
            chance_depth: prepared.settings.solver.parallel.chance_depth,
            min_children: prepared.settings.solver.parallel.min_children,
        });
        for &target in &at {
            solver.run(target - solver.iteration());
            let mut out = serde_json::Map::new();
            out.insert("iteration".into(), target.into());
            let mut sum = Stats::default();
            for p in [Player::P0, Player::P1] {
                let mut stats = Stats::default();
                let ranges = &solver.game().root_ranges;
                let (my, opp) = (ranges[p].clone(), ranges[p.opponent()].clone());
                walk(&solver, p, 0, &my, &opp, &mut stats);
                for i in 0..4 {
                    sum.terminal[i] += stats.terminal[i];
                    sum.own_elems[i] += stats.own_elems[i];
                    sum.opp_elems[i] += stats.opp_elems[i];
                }
            }
            out.insert("terminal".into(), frac(sum.terminal));
            out.insert("ownElems".into(), frac(sum.own_elems));
            out.insert("oppElems".into(), frac(sum.opp_elems));
            let (_, expl) = solver.evaluate();
            out.insert("nashConv".into(), (expl[Player::P0] + expl[Player::P1]).into());
            println!("{}", serde_json::Value::Object(out));
        }
        Ok(())
    })
}
