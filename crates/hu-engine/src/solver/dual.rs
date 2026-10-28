//! Exact no-profile EV/BR for both seats with a single strategy read per node.
//! Each output row is [EV0, BR0, EV1, BR1]; reductions preserve child order.

use super::*;

#[derive(Default)]
struct Metadata {
    offsets: Vec<(usize, usize)>,
    terminals: Vec<u32>,
    reaches: Vec<&'static [f32]>,
    outputs: Vec<&'static mut [f32]>,
}

thread_local! {
    static METADATA: std::cell::RefCell<Vec<Metadata>> = const { std::cell::RefCell::new(Vec::new()) };
}

// Lease outside the RefCell borrow: terminal evaluators may reenter on this worker.
struct MetadataLease(Metadata);
impl MetadataLease {
    fn take() -> Self {
        Self(METADATA.with(|pool| pool.borrow_mut().pop().unwrap_or_default()))
    }
}
impl Drop for MetadataLease {
    fn drop(&mut self) {
        METADATA.with(|pool| pool.borrow_mut().push(std::mem::take(&mut self.0)));
    }
}

struct Ctx<'a, E, S> {
    game: &'a CompiledGame<E>,
    storage: &'a S,
    par: ParConfig,
}

pub(super) fn evaluate<E: TerminalEvaluator, S: Storage>(
    game: &CompiledGame<E>,
    storage: &S,
    par: ParConfig,
) -> PerPlayer<(f64, f64)> {
    let ctx = Ctx { game, storage, par };
    with_worker_scratch(|scratch| {
        let d0 = game.tree.root_dims[Player::P0] as usize;
        let d1 = game.tree.root_dims[Player::P1] as usize;
        let mut values = scratch.take(2 * (d0 + d1));
        pass(
            &ctx,
            scratch,
            0,
            PerPlayer::new(&game.root_ranges[Player::P0], &game.root_ranges[Player::P1]),
            &mut values,
            par.chance_depth,
            false,
        );
        let aggregate = |p: Player, row: &[f32]| -> f64 {
            game.root_ranges[p]
                .iter()
                .zip(row)
                .map(|(&r, &v)| r as f64 * v as f64)
                .sum::<f64>()
                / game.normalizer
        };
        let result = PerPlayer::new(
            (
                aggregate(Player::P0, &values[..d0]),
                aggregate(Player::P0, &values[d0..2 * d0]),
            ),
            (
                aggregate(Player::P1, &values[2 * d0..2 * d0 + d1]),
                aggregate(Player::P1, &values[2 * d0 + d1..]),
            ),
        );
        scratch.put(values);
        result
    })
}

// A seat can be pruned only when its opponent's reach is zero. The other seat
// may still need the subtree, even if its own reach is zero.
#[allow(clippy::too_many_arguments)]
fn pass<E: TerminalEvaluator, S: Storage>(
    ctx: &Ctx<'_, E, S>,
    scratch: &mut Scratch,
    id: NodeId,
    reach: PerPlayer<&[f32]>,
    out: &mut [f32],
    budget: u32,
    parallel_ancestor: bool,
) {
    let tree = &ctx.game.tree;
    let d0 = reach[Player::P0].len();
    let d1 = reach[Player::P1].len();
    let width = 2 * (d0 + d1);
    if all_zero(reach[Player::P0]) && all_zero(reach[Player::P1]) {
        return;
    }
    let node = *tree.node(id);
    if node.kind == NodeKind::Action
        && d0 != 0
        && d1 != 0
        && !tree.subtree_has_chance[id as usize]
        && subtree_elements(tree, id) < 2 * ACTION_PAR_MIN_ELEMENTS
    {
        batch(ctx, scratch, id, reach, out);
        return;
    }
    match node.kind {
        NodeKind::Terminal => {
            if !all_zero(reach[Player::P1]) {
                let (ev, br) = out[..2 * d0].split_at_mut(d0);
                ctx.game
                    .evaluator
                    .eval(node.aux, Player::P0, reach[Player::P1], ev);
                br.copy_from_slice(ev);
            }
            if !all_zero(reach[Player::P0]) {
                let (ev, br) = out[2 * d0..].split_at_mut(d1);
                ctx.game
                    .evaluator
                    .eval(node.aux, Player::P1, reach[Player::P0], ev);
                br.copy_from_slice(ev);
            }
        }
        NodeKind::Chance => {
            let n = node.num_children as usize;
            if parallel_ancestor {
                serial_chance(ctx, scratch, id, reach, out, budget);
                return;
            }
            let parallel =
                rayon::current_num_threads() > 1 && budget > 0 && n >= ctx.par.min_children;
            let mut rows = Vec::with_capacity(n);
            let total =
                2 * (chance_len(tree, id, Player::P0, d0) + chance_len(tree, id, Player::P1, d1));
            let mut flat = scratch.take(total);
            let mut rest = flat.as_mut_slice();
            for pos in 0..n {
                let deal = *tree.deal(&node, pos);
                let a = tree.mapped_dim(deal.maps[Player::P0], d0 as u32) as usize;
                let b = tree.mapped_dim(deal.maps[Player::P1], d1 as u32) as usize;
                let (row, tail) = rest.split_at_mut(2 * (a + b));
                rest = tail;
                rows.push(row);
            }
            let visit = |scratch: &mut Scratch, pos: usize, row: &mut [f32]| {
                let deal = *tree.deal(&node, pos);
                let a = tree.mapped_dim(deal.maps[Player::P0], d0 as u32) as usize;
                let b = tree.mapped_dim(deal.maps[Player::P1], d1 as u32) as usize;
                let mut r0 = scratch.take_overwrite(a);
                let mut r1 = scratch.take_overwrite(b);
                tree.map_reach_into(deal.maps[Player::P0], reach[Player::P0], &mut r0);
                tree.map_reach_into(deal.maps[Player::P1], reach[Player::P1], &mut r1);
                pass(
                    ctx,
                    scratch,
                    node.first_child + pos as u32,
                    PerPlayer::new(&r0, &r1),
                    row,
                    budget.saturating_sub(1),
                    parallel,
                );
                scratch.put(r1);
                scratch.put(r0);
            };
            if parallel {
                rows.into_par_iter()
                    .enumerate()
                    .for_each(|(i, row)| with_worker_scratch(|s| visit(s, i, row)));
            } else {
                for (i, row) in rows.into_iter().enumerate() {
                    visit(scratch, i, row);
                }
            }
            let mut offset = 0;
            for pos in 0..n {
                let deal = *tree.deal(&node, pos);
                for p in Player::BOTH {
                    let dim = if p == Player::P0 { d0 } else { d1 };
                    let len = tree.mapped_dim(deal.maps[p], dim as u32) as usize;
                    let start = if p == Player::P0 { 0 } else { 2 * d0 };
                    for channel in 0..2 {
                        tree.accumulate_values_with_scratch(
                            deal.maps[p],
                            deal.weight,
                            &flat[offset..offset + len],
                            &mut out[start + channel * dim..start + (channel + 1) * dim],
                            scratch,
                        );
                        offset += len;
                    }
                }
            }
            scratch.put(flat);
        }
        NodeKind::Action => {
            let sref = tree.storage_ref(&node);
            let hands = sref.num_hands as usize;
            let n = node.num_children as usize;
            let mut sigma = scratch.take_overwrite(sref.len());
            ctx.storage.average_strategy(sref, sref.index, &mut sigma);
            let mut flat = scratch.take(width * n);
            let visit = |scratch: &mut Scratch, a: usize, row: &mut [f32]| {
                let mut next = scratch.take_overwrite(hands);
                mul_into(&mut next, reach[node.player], &sigma[a * hands..]);
                let r = if node.player == Player::P0 {
                    PerPlayer::new(next.as_slice(), reach[Player::P1])
                } else {
                    PerPlayer::new(reach[Player::P0], next.as_slice())
                };
                pass(
                    ctx,
                    scratch,
                    node.first_child + a as u32,
                    r,
                    row,
                    budget,
                    parallel_ancestor,
                );
                scratch.put(next);
            };
            if width != 0 && parallel_actions(tree, id) {
                flat.par_chunks_mut(width)
                    .enumerate()
                    .for_each(|(a, row)| with_worker_scratch(|s| visit(s, a, row)));
            } else {
                for (a, row) in flat.chunks_exact_mut(width).enumerate() {
                    visit(scratch, a, row);
                }
            }
            reduce(node.player, reach, &sigma, &flat, out, n);
            scratch.put(flat);
            scratch.put(sigma);
        }
    }
}

// Outer chance fan-out already keeps workers busy. Stream each inner chance
// child instead of retaining four channels for every child on every worker.
fn serial_chance<E: TerminalEvaluator, S: Storage>(
    ctx: &Ctx<'_, E, S>,
    scratch: &mut Scratch,
    id: NodeId,
    reach: PerPlayer<&[f32]>,
    out: &mut [f32],
    budget: u32,
) {
    let tree = &ctx.game.tree;
    let node = tree.node(id);
    let d0 = reach[Player::P0].len();
    let d1 = reach[Player::P1].len();
    for pos in 0..node.num_children as usize {
        let deal = *tree.deal(node, pos);
        let a = tree.mapped_dim(deal.maps[Player::P0], d0 as u32) as usize;
        let b = tree.mapped_dim(deal.maps[Player::P1], d1 as u32) as usize;
        let mut r0 = scratch.take_overwrite(a);
        let mut r1 = scratch.take_overwrite(b);
        tree.map_reach_into(deal.maps[Player::P0], reach[Player::P0], &mut r0);
        tree.map_reach_into(deal.maps[Player::P1], reach[Player::P1], &mut r1);
        let mut row = scratch.take(2 * (a + b));
        pass(
            ctx,
            scratch,
            node.first_child + pos as u32,
            PerPlayer::new(&r0, &r1),
            &mut row,
            budget.saturating_sub(1),
            true,
        );
        let mut offset = 0;
        for p in Player::BOTH {
            let (dim, len, start) = if p == Player::P0 {
                (d0, a, 0)
            } else {
                (d1, b, 2 * d0)
            };
            for channel in 0..2 {
                tree.accumulate_values_with_scratch(
                    deal.maps[p],
                    deal.weight,
                    &row[offset..offset + len],
                    &mut out[start + channel * dim..start + (channel + 1) * dim],
                    scratch,
                );
                offset += len;
            }
        }
        scratch.put(row);
        scratch.put(r1);
        scratch.put(r0);
    }
}

fn reduce(
    player: Player,
    reach: PerPlayer<&[f32]>,
    sigma: &[f32],
    children: &[f32],
    out: &mut [f32],
    actions: usize,
) {
    let d0 = reach[Player::P0].len();
    let d1 = reach[Player::P1].len();
    let width = 2 * (d0 + d1);
    for p in Player::BOTH {
        if all_zero(reach[p.opponent()]) {
            continue;
        }
        let dim = if p == Player::P0 { d0 } else { d1 };
        let start = if p == Player::P0 { 0 } else { 2 * d0 };
        if p == player {
            out[start + dim..start + 2 * dim].fill(f32::NEG_INFINITY);
        }
        for a in 0..actions {
            let row = &children[a * width + start..a * width + start + 2 * dim];
            let (ev, br) = out[start..start + 2 * dim].split_at_mut(dim);
            if p == player {
                let hands = reach[p].len();
                for ((v, &s), &c) in ev
                    .iter_mut()
                    .zip(&sigma[a * hands..(a + 1) * hands])
                    .zip(&row[..dim])
                {
                    *v += s * c;
                }
                for (v, &c) in br.iter_mut().zip(&row[dim..]) {
                    *v = v.max(c);
                }
            } else {
                for (v, &c) in ev.iter_mut().zip(&row[..dim]) {
                    *v += c;
                }
                for (v, &c) in br.iter_mut().zip(&row[dim..]) {
                    *v += c;
                }
            }
        }
    }
}

// Chance-free subtrees use the same forward-reach / terminal-batch / reverse
// reduction ordering as batch::values, but share the strategy across seats.
// An offset of usize::MAX denotes that seat's root reach.
fn batch<E: TerminalEvaluator, S: Storage>(
    ctx: &Ctx<'_, E, S>,
    scratch: &mut Scratch,
    root: NodeId,
    reach0: PerPlayer<&[f32]>,
    out: &mut [f32],
) {
    let tree = &ctx.game.tree;
    let root_node = tree.node(root);
    let first = root_node.first_child as usize;
    let mut end = first + root_node.num_children as usize;
    let mut i = first;
    while i < end {
        let n = &tree.nodes[i];
        if n.kind == NodeKind::Action {
            end = end.max(n.first_child as usize + n.num_children as usize);
        }
        i += 1;
    }
    let count = end - first + 1;
    let walk = || std::iter::once(root).chain(first as u32..end as u32);
    let index = |id: NodeId| {
        if id == root {
            0
        } else {
            id as usize - first + 1
        }
    };
    let span = tree.storage_spans[root as usize];
    let len = span.end - span.start;
    let d0 = reach0[Player::P0].len();
    let d1 = reach0[Player::P1].len();
    let mut sigma = scratch.take_overwrite(len);
    let mut scaled = scratch.take_overwrite(len);
    // Share the node-value arena between the seats. Strategies and both
    // reaches remain available for both reverse reductions.
    let mut values = scratch.take(count * 2 * d0.max(d1));
    let mut lease = MetadataLease::take();
    let metadata = &mut lease.0;
    metadata.offsets.resize(count, (usize::MAX, usize::MAX));
    metadata.offsets[0] = (usize::MAX, usize::MAX);
    let offsets = &mut metadata.offsets;
    for (idx, id) in walk().enumerate() {
        let node = tree.node(id);
        if node.kind == NodeKind::Terminal {
            continue;
        }
        let sref = tree.storage_ref(node);
        let offset = sref.offset - span.start;
        let hands = sref.num_hands as usize;
        let parent = offsets[idx];
        let r0 = if parent.0 == usize::MAX {
            reach0[Player::P0]
        } else {
            &scaled[parent.0..parent.0 + d0]
        };
        let r1 = if parent.1 == usize::MAX {
            reach0[Player::P1]
        } else {
            &scaled[parent.1..parent.1 + d1]
        };
        let active = !all_zero(r0) || !all_zero(r1);
        if active {
            ctx.storage
                .average_strategy(sref, sref.index, &mut sigma[offset..offset + sref.len()]);
        }
        let own = if node.player == Player::P0 {
            parent.0
        } else {
            parent.1
        };
        for (a, child) in tree.children(id).enumerate() {
            let start = offset + a * hands;
            let (prev, rest) = scaled.split_at_mut(start);
            let root_reach = reach0[node.player];
            let r = if own == usize::MAX {
                root_reach
            } else {
                &prev[own..own + hands]
            };
            if active {
                mul_into(&mut rest[..hands], r, &sigma[start..start + hands]);
            } else {
                rest[..hands].fill(0.0);
            }
            offsets[index(child)] = if node.player == Player::P0 {
                (start, parent.1)
            } else {
                (parent.0, start)
            };
        }
    }
    for p in Player::BOTH {
        metadata.terminals.clear();
        let mut reaches = batch::shared_rows(std::mem::take(&mut metadata.reaches));
        let mut outputs = batch::mutable_rows(std::mem::take(&mut metadata.outputs));
        let (dim, start) = if p == Player::P0 {
            (d0, 0)
        } else {
            (d1, 2 * d0)
        };
        let width = 2 * dim;
        let values = &mut values[..count * width];
        if p == Player::P1 {
            values.fill(0.0);
        }
        for ((idx, id), row) in walk().enumerate().zip(values.chunks_exact_mut(width)) {
            let node = tree.node(id);
            if node.kind != NodeKind::Terminal {
                continue;
            }
            let offset = if p == Player::P0 {
                offsets[idx].1
            } else {
                offsets[idx].0
            };
            let r = if offset == usize::MAX {
                reach0[p.opponent()]
            } else {
                &scaled[offset..offset + reach0[p.opponent()].len()]
            };
            if !all_zero(r) {
                metadata.terminals.push(node.aux);
                reaches.push(r);
                outputs.push(&mut row[..dim]);
            }
        }
        ctx.game
            .evaluator
            .eval_batch(&metadata.terminals, p, &reaches, &mut outputs);
        metadata.outputs = batch::mutable_rows(outputs);
        metadata.reaches = batch::shared_rows(reaches);
        for id in walk().rev() {
            let idx = index(id);
            let node = tree.node(id);
            let (parents, children) = values.split_at_mut((idx + 1) * width);
            let row = &mut parents[idx * width..];
            if node.kind == NodeKind::Terminal {
                let (ev, br) = row.split_at_mut(dim);
                br.copy_from_slice(ev);
                continue;
            }
            let (o0, o1) = offsets[idx];
            let opp_offset = if p == Player::P0 { o1 } else { o0 };
            let opp = if opp_offset == usize::MAX {
                reach0[p.opponent()]
            } else {
                &scaled[opp_offset..opp_offset + reach0[p.opponent()].len()]
            };
            if all_zero(opp) {
                continue;
            }
            let sref = tree.storage_ref(node);
            let offset = sref.offset - span.start;
            let first_child = (node.first_child as usize - first - idx) * width;
            let children = &children[first_child..];
            let (ev, br) = row.split_at_mut(dim);
            if node.player == p {
                br.fill(f32::NEG_INFINITY);
            }
            for a in 0..node.num_children as usize {
                let child = &children[a * width..(a + 1) * width];
                if node.player == p {
                    let strategy = &sigma[offset + a * dim..offset + (a + 1) * dim];
                    for ((v, &s), &c) in ev.iter_mut().zip(strategy).zip(&child[..dim]) {
                        *v += s * c;
                    }
                    for (v, &c) in br.iter_mut().zip(&child[dim..]) {
                        *v = v.max(c);
                    }
                } else {
                    for (v, &c) in ev.iter_mut().zip(&child[..dim]) {
                        *v += c;
                    }
                    for (v, &c) in br.iter_mut().zip(&child[dim..]) {
                        *v += c;
                    }
                }
            }
        }
        out[start..start + width].copy_from_slice(&values[..width]);
    }
    scratch.put(values);
    scratch.put(scaled);
    scratch.put(sigma);
}
