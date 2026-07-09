//! Chance-sampled MCCFR driver: external-ish sampling that samples only
//! chance nodes (both players' action nodes stay full vector-form, exactly
//! like [`crate::solver::cfr_pass`] — the public-tree representation already
//! carries every hand simultaneously, so there is no separate "infoset" to
//! sample at an action node the way sequence-form MCCFR does), paired with
//! Linear-CFR-style batched early discounting and negative-regret pruning.
//!
//! One [`McSolver::step`] plays both players' roles once (alternating
//! updates, same convention as [`crate::solver::Solver::step`]), each via one
//! recursive sampled walk seeded by a single persistent `ChaCha20Rng` so a
//! whole run is reproducible from `(seed, iteration count)` alone, and a
//! snapshot mid-run can resume bit-for-bit (see [`McSolverState`]).

use cards::{PerPlayer, Player};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;

use crate::schedule::Discounts;
use crate::scratch::Scratch;
use crate::solver::{CompiledGame, ParConfig, TerminalEvaluator, ValueCtx, br_pass, ev_pass};
use crate::storage::{StateMismatch, Storage, StorageRef, StorageState};
use crate::tree::{NodeId, NodeKind, PublicTree};

/// Sampled-pass configuration: batched early discounting and negative-regret
/// pruning. See [`McSolver`] for the algorithm these parameters drive.
#[derive(Clone, Copy, Debug)]
pub struct McCfg {
    pub seed: u64,
    /// Batched early discounting (Pluribus-style): while `iteration <
    /// discount_until`, every `discount_every` iterations scale ALL
    /// accumulated regrets and strategy sums by d/(d+1), where d is the
    /// number of discount events so far (1-indexed). Approximates Linear
    /// CFR without per-update discounting (which would be wrong under
    /// sampling: unvisited nodes must not miss discounts).
    pub discount_every: u64,
    pub discount_until: u64,
    /// Negative-regret pruning: at the updating player's action nodes, an
    /// action whose accumulated regret is below this threshold is skipped
    /// with probability `prune_skip_prob` (regret left untouched, no
    /// recursion), EXCEPT at nodes whose subtree contains no chance node
    /// (`PublicTree::subtree_has_chance` false = the "final street", where
    /// regrets move fast and pruning is unsound in practice — this is the
    /// poker-agnostic encoding of Pluribus's "never prune on the last
    /// round"). None disables pruning entirely.
    pub prune_threshold: Option<f32>,
    pub prune_skip_prob: f64,
}

impl Default for McCfg {
    fn default() -> Self {
        McCfg {
            seed: 0,
            discount_every: 100,
            discount_until: 10_000,
            prune_threshold: None,
            prune_skip_prob: 0.95,
        }
    }
}

/// Checkpointable solver state: iteration count, RNG state, and the storage
/// backend's contents. The RNG state is the ChaCha20 stream's seed plus its
/// 128-bit word position — together enough to reconstruct the exact
/// generator (`ChaCha20Rng::from_seed(rng_seed)` then
/// `set_word_pos(rng_word_pos)`) and resume sampling bit-identically.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct McSolverState {
    pub iteration: u64,
    pub rng_seed: [u8; 32],
    pub rng_word_pos: u128,
    pub storage: StorageState,
}

/// Chance-sampled MCCFR solver with alternating updates.
pub struct McSolver<E, S> {
    game: CompiledGame<E>,
    storage: S,
    scratch: Scratch,
    cfg: McCfg,
    iteration: u64,
    rng: ChaCha20Rng,
}

/// Read-only context threaded through [`mccfr_pass`]: the sampled-pass
/// analogue of [`crate::solver::PassCtx`], plus the pruning knobs.
struct McPassCtx<'w, E> {
    tree: &'w PublicTree,
    evaluator: &'w E,
    p: Player,
    /// Always the unit schedule (see [`McSolver::step`]): batched
    /// discounting happens separately, once per iteration, via
    /// [`Storage::scale_all`], not per-update — per-update discounting
    /// under sampling would only discount visited nodes, silently
    /// preserving stale, undiscounted regret at unvisited ones.
    discounts: Discounts,
    prune_threshold: Option<f32>,
    prune_skip_prob: f64,
}

impl<E: TerminalEvaluator, S: Storage> McSolver<E, S> {
    pub fn new(game: CompiledGame<E>, cfg: McCfg) -> Self {
        for (player, range) in [Player::P0, Player::P1]
            .into_iter()
            .zip(game.root_ranges.as_ref().0)
        {
            assert_eq!(
                range.len(),
                game.tree.root_dims[player] as usize,
                "root range length must match root dims"
            );
        }
        let storage = S::new(game.tree.storage_len, game.tree.storage_refs.len());
        let rng = ChaCha20Rng::seed_from_u64(cfg.seed);
        McSolver {
            game,
            storage,
            scratch: Scratch::new(),
            cfg,
            iteration: 0,
            rng,
        }
    }

    pub fn iteration(&self) -> u64 {
        self.iteration
    }

    pub fn game(&self) -> &CompiledGame<E> {
        &self.game
    }

    /// Snapshots the iteration count, RNG state, and storage backend
    /// contents for a checkpoint.
    pub fn state(&self) -> McSolverState {
        McSolverState {
            iteration: self.iteration,
            rng_seed: self.rng.get_seed(),
            rng_word_pos: self.rng.get_word_pos(),
            storage: self.storage.state(),
        }
    }

    /// Restores a previously snapshotted state. Fails (leaving `self`
    /// unchanged) if `state.storage` doesn't match this solver's storage
    /// backend.
    pub fn restore_state(&mut self, state: McSolverState) -> Result<(), StateMismatch> {
        self.storage.restore_state(state.storage)?;
        self.iteration = state.iteration;
        let mut rng = ChaCha20Rng::from_seed(state.rng_seed);
        rng.set_word_pos(state.rng_word_pos);
        self.rng = rng;
        Ok(())
    }

    /// One alternating chance-sampled iteration: a sampled regret/strategy
    /// update pass for each player in turn, using unit discounts (negative
    /// regrets persist — required both for Linear-CFR-with-sampling
    /// soundness and because pruning reads them), followed by the batched
    /// early-discount check.
    pub fn step(&mut self) {
        let t = self.iteration + 1;
        let unit_discounts = Discounts {
            pos: 1.0,
            neg: 1.0,
            avg: 1.0,
            floor_neg: false,
            reset_avg: false,
        };
        for p in Player::BOTH {
            let my_range = self.game.root_ranges[p].clone();
            let opp_range = self.game.root_ranges[p.opponent()].clone();
            let ctx = McPassCtx {
                tree: &self.game.tree,
                evaluator: &self.game.evaluator,
                p,
                discounts: unit_discounts,
                prune_threshold: self.cfg.prune_threshold,
                prune_skip_prob: self.cfg.prune_skip_prob,
            };
            let mut out = self.scratch.take(self.game.tree.root_dims[p] as usize);
            mccfr_pass(
                &ctx,
                &mut self.storage,
                &mut self.scratch,
                &mut self.rng,
                0,
                &my_range,
                &opp_range,
                &mut out,
            );
            self.scratch.put(out);
        }
        self.iteration = t;

        if t < self.cfg.discount_until && t.is_multiple_of(self.cfg.discount_every) {
            let d = (t / self.cfg.discount_every) as f64;
            let factor = (d / (d + 1.0)) as f32;
            self.storage.scale_all(factor, factor);
        }
    }

    pub fn run(&mut self, n: u64) {
        for _ in 0..n {
            self.step();
        }
    }

    /// Expected value of the average strategy profile for `p`, per deal —
    /// an exact (full-traversal) evaluation, reusing [`ev_pass`].
    pub fn expected_value(&self, p: Player) -> f64 {
        let par = ParConfig::default();
        let ctx = ValueCtx {
            tree: &self.game.tree,
            evaluator: &self.game.evaluator,
            storage: &self.storage,
            p,
            par,
        };
        let mut scratch = Scratch::new();
        let mut out = scratch.take(self.game.tree.root_dims[p] as usize);
        ev_pass(
            &ctx,
            &mut scratch,
            0,
            &self.game.root_ranges[p.opponent()],
            &mut out,
            par.chance_depth,
        );
        self.root_aggregate(p, &out)
    }

    /// Best-response value against the opponent's average strategy, per
    /// deal — an exact (full-traversal) evaluation, reusing [`br_pass`].
    pub fn best_response_value(&self, p: Player) -> f64 {
        let par = ParConfig::default();
        let ctx = ValueCtx {
            tree: &self.game.tree,
            evaluator: &self.game.evaluator,
            storage: &self.storage,
            p,
            par,
        };
        let mut scratch = Scratch::new();
        let mut out = scratch.take(self.game.tree.root_dims[p] as usize);
        br_pass(
            &ctx,
            &mut scratch,
            0,
            &self.game.root_ranges[p.opponent()],
            &mut out,
            par.chance_depth,
        );
        self.root_aggregate(p, &out)
    }

    /// Per-player exploitability `BR_p(avg_{-p}) - u_p(avg)`.
    pub fn exploitability(&self) -> PerPlayer<f64> {
        let ev0 = self.expected_value(Player::P0);
        let ev1 = if self.game.zero_sum {
            -ev0
        } else {
            self.expected_value(Player::P1)
        };
        PerPlayer::new(
            self.best_response_value(Player::P0) - ev0,
            self.best_response_value(Player::P1) - ev1,
        )
    }

    /// Normalized average strategy at an action node (`A*H`, action-major).
    pub fn average_strategy_at(&self, node: NodeId) -> Vec<f32> {
        self.node_strategy(node, |sref, out| {
            self.storage.average_strategy(sref, sref.index, out)
        })
    }

    /// Current (regret-matching) strategy at an action node.
    pub fn current_strategy_at(&self, node: NodeId) -> Vec<f32> {
        self.node_strategy(node, |sref, out| {
            self.storage.regret_matching(sref, sref.index, out)
        })
    }

    fn node_strategy(&self, node: NodeId, f: impl Fn(StorageRef, &mut [f32])) -> Vec<f32> {
        let n = self.game.tree.node(node);
        assert_eq!(n.kind, NodeKind::Action, "not an action node");
        let sref = self.game.tree.storage_ref(n);
        let mut out = vec![0.0; sref.len()];
        f(sref, &mut out);
        out
    }

    fn root_aggregate(&self, p: Player, values: &[f32]) -> f64 {
        let range = &self.game.root_ranges[p];
        let total: f64 = range
            .iter()
            .zip(values)
            .map(|(&r, &v)| r as f64 * v as f64)
            .sum();
        total / self.game.normalizer
    }
}

/// Chance-sampled CFR update pass for player `p`, writing p's counterfactual
/// values (one per hand in p's current private-state space) into `out`.
///
/// Mirrors [`crate::solver::cfr_pass`] exactly at action nodes (full vector
/// recursion over every action, for both the updating and the opponent
/// player — the per-hand vector already IS the full distribution, so there
/// is nothing extra to sample there); the only sampling happens at chance
/// nodes, and only pruning (at the updating player's action nodes) adds
/// logic `cfr_pass` doesn't have.
///
/// No rayon here (see the module doc): a sampled path is a single chain, and
/// the RNG is strictly sequential, so parallel fan-out has nothing to give.
/// Operates on the storage backend directly (`&mut S`, not `S::View`) since
/// nothing ever splits it.
///
/// Every `out` parameter arrives pre-zeroed by the caller (the same
/// invariant `cfr_pass` relies on): Terminal and the updating player's
/// action node fully overwrite it, Chance and the opponent's action node
/// accumulate into it.
#[allow(clippy::too_many_arguments)]
fn mccfr_pass<E: TerminalEvaluator, S: Storage>(
    ctx: &McPassCtx<'_, E>,
    storage: &mut S,
    scratch: &mut Scratch,
    rng: &mut ChaCha20Rng,
    node_id: NodeId,
    my_reach: &[f32],
    opp_reach: &[f32],
    out: &mut [f32],
) {
    let node = *ctx.tree.node(node_id);
    match node.kind {
        NodeKind::Terminal => {
            ctx.evaluator.eval(node.aux, ctx.p, opp_reach, out);
        }
        NodeKind::Chance => {
            let num_children = node.num_children as usize;
            let total_weight: f32 = (0..num_children)
                .map(|pos| ctx.tree.deal(&node, pos).weight)
                .sum();
            debug_assert!(total_weight > 0.0, "chance node has zero total weight");
            // Sample exactly one child j w.p. w_j / W. `target` and the
            // walk below sum weights in the same left-to-right order as
            // `total_weight`, so IEEE754 summation lands on the identical
            // bit pattern by the last child — the strict `<` against
            // `total_weight` (from `gen_range`'s exclusive upper bound)
            // therefore always fires at or before the last index.
            let target = rng.gen_range(0.0..total_weight);
            let mut cum = 0.0f32;
            let mut chosen = num_children - 1;
            for pos in 0..num_children {
                cum += ctx.tree.deal(&node, pos).weight;
                if target < cum {
                    chosen = pos;
                    break;
                }
            }
            let deal = *ctx.tree.deal(&node, chosen);
            let child = node.first_child + chosen as u32;

            let mut my_next = scratch.take(my_reach.len());
            let mut opp_next = scratch.take(opp_reach.len());
            ctx.tree
                .map_reach_into(deal.maps[ctx.p], my_reach, &mut my_next);
            ctx.tree
                .map_reach_into(deal.maps[ctx.p.opponent()], opp_reach, &mut opp_next);
            let mut child_out = scratch.take(my_reach.len());
            mccfr_pass(
                ctx,
                storage,
                scratch,
                rng,
                child,
                &my_next,
                &opp_next,
                &mut child_out,
            );
            // Scale by W (not w_j): j was drawn w.p. w_j/W, so the
            // estimator W * v_j has expectation sum_j w_j * v_j — exactly
            // what the full traversal's `accumulate_values` loop over every
            // child computes. Reusing `accumulate_values` keeps the
            // reach-map back-mapping (mask zeroing / transition backward
            // map) identical to the full traversal.
            ctx.tree
                .accumulate_values(deal.maps[ctx.p], total_weight, &child_out, out);
            scratch.put(child_out);
            scratch.put(opp_next);
            scratch.put(my_next);
        }
        NodeKind::Action if node.player == ctx.p => {
            let sref = ctx.tree.storage_ref(&node);
            let (num_actions, num_hands) = (sref.num_actions as usize, sref.num_hands as usize);
            debug_assert_eq!(num_hands, my_reach.len());

            let mut sigma = scratch.take(sref.len());
            storage.regret_matching(sref, sref.index, &mut sigma);

            // Non-zero entries mark actions this visit skips (see
            // `McCfg::prune_threshold`). A scratch f32 buffer (0.0/1.0)
            // rather than a fresh `Vec<bool>` keeps this allocation-free in
            // steady state, same discipline as every other buffer here.
            let mut skip = scratch.take(num_actions);
            if let Some(t) = ctx.prune_threshold
                && ctx.tree.subtree_has_chance[node_id as usize]
            {
                let mut regrets = scratch.take(sref.len());
                storage.raw_regrets(sref, sref.index, &mut regrets);
                for a in 0..num_actions {
                    let row = &regrets[a * num_hands..(a + 1) * num_hands];
                    let prunable = row.iter().all(|&r| r < t);
                    if prunable && rng.gen_bool(ctx.prune_skip_prob) {
                        skip[a] = 1.0;
                    }
                }
                scratch.put(regrets);
                // Never produce an empty node: if pruning would skip every
                // action, fall back to traversing all of them.
                if skip.iter().all(|&s| s != 0.0) {
                    skip.fill(0.0);
                }
            }

            let mut cfvs = scratch.take(sref.len());
            let mut node_cfv = scratch.take(num_hands);
            let mut my_next = scratch.take(num_hands);

            for (a, child) in ctx.tree.children(node_id).enumerate() {
                if skip[a] != 0.0 {
                    // Left at zero: contributes nothing to the CFV blend
                    // below, exactly as if sigma_a were 0 this visit.
                    continue;
                }
                let row = &sigma[a * num_hands..(a + 1) * num_hands];
                for h in 0..num_hands {
                    my_next[h] = my_reach[h] * row[h];
                }
                let out_row = &mut cfvs[a * num_hands..(a + 1) * num_hands];
                mccfr_pass(
                    ctx, storage, scratch, rng, child, &my_next, opp_reach, out_row,
                );
            }

            for a in 0..num_actions {
                let row = &sigma[a * num_hands..(a + 1) * num_hands];
                for h in 0..num_hands {
                    node_cfv[h] += row[h] * cfvs[a * num_hands + h];
                }
            }

            // Instantaneous regret, computed in place.
            for a in 0..num_actions {
                for h in 0..num_hands {
                    cfvs[a * num_hands + h] -= node_cfv[h];
                }
            }
            // Skipped actions get no regret update this visit: re-zero
            // their row so `update_regrets` (unit discounts: factor 1.0,
            // delta 0) leaves the stored regret bit-for-bit unchanged.
            for a in 0..num_actions {
                if skip[a] != 0.0 {
                    cfvs[a * num_hands..(a + 1) * num_hands].fill(0.0);
                }
            }
            storage.update_regrets(sref, sref.index, &cfvs, &ctx.discounts);

            // Overwrite `cfvs` again as the reach-weighted strategy buffer.
            // Unaffected by pruning: this only reads `sigma`/`my_reach`,
            // both already known without visiting any child.
            for a in 0..num_actions {
                for h in 0..num_hands {
                    cfvs[a * num_hands + h] = my_reach[h] * sigma[a * num_hands + h];
                }
            }
            storage.accumulate_strategy(sref, sref.index, &cfvs, &ctx.discounts);

            out.copy_from_slice(&node_cfv);

            scratch.put(my_next);
            scratch.put(node_cfv);
            scratch.put(cfvs);
            scratch.put(skip);
            scratch.put(sigma);
        }
        NodeKind::Action => {
            // Opponent's node: full vector recursion, current strategy
            // scales the opponent reach, no sampling and no pruning (that
            // is only defined at the updating player's nodes).
            let sref = ctx.tree.storage_ref(&node);
            let num_hands = sref.num_hands as usize;
            debug_assert_eq!(num_hands, opp_reach.len());
            let mut sigma = scratch.take(sref.len());
            storage.regret_matching(sref, sref.index, &mut sigma);

            let mut opp_next = scratch.take(num_hands);
            let mut child_out = scratch.take(my_reach.len());
            for (a, child) in ctx.tree.children(node_id).enumerate() {
                let row = &sigma[a * num_hands..(a + 1) * num_hands];
                for h in 0..num_hands {
                    opp_next[h] = opp_reach[h] * row[h];
                }
                child_out.fill(0.0);
                mccfr_pass(
                    ctx,
                    storage,
                    scratch,
                    rng,
                    child,
                    my_reach,
                    &opp_next,
                    &mut child_out,
                );
                for h in 0..out.len() {
                    out[h] += child_out[h];
                }
            }
            scratch.put(child_out);
            scratch.put(opp_next);
            scratch.put(sigma);
        }
    }
}
