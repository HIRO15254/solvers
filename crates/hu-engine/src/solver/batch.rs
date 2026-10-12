//! Non-recursive CFR and exact EV/BR below the chance-free action split bound.
use super::*;

#[derive(Clone, Copy, Default)]
struct Reaches {
    my: usize,
    opp: usize,
    zero_opp: bool,
}

#[derive(Default)]
struct Metadata {
    nodes: Vec<Reaches>,
    terminals: Vec<u32>,
    reaches: Vec<&'static [f32]>,
    outputs: Vec<&'static mut [f32]>,
}

thread_local! {
    // Lease metadata just like worker scratch: no RefCell borrow crosses an
    // evaluator call, which may itself use Rayon or reenter on this worker.
    static METADATA: std::cell::RefCell<Vec<Metadata>> = const { std::cell::RefCell::new(Vec::new()) };
}

struct MetadataLease(Metadata);
impl Drop for MetadataLease {
    fn drop(&mut self) {
        METADATA.with(|pool| pool.borrow_mut().push(std::mem::take(&mut self.0)));
    }
}
impl MetadataLease {
    fn take() -> Self {
        Self(METADATA.with(|pool| pool.borrow_mut().pop().unwrap_or_default()))
    }
}

// Empty reference vectors contain no borrowed data. In-place collect recycles
// their allocation across lifetimes; the standard library keeps the same layout.
// Debug checks verify reuse, including after evaluator calls and size changes.
fn shared_rows<'b>(mut rows: Vec<&[f32]>) -> Vec<&'b [f32]> {
    rows.clear();
    let allocation = rows.as_ptr();
    let result: Vec<_> = rows.into_iter().map(|_| unreachable!()).collect();
    debug_assert_eq!(allocation, result.as_ptr());
    result
}
fn mutable_rows<'b>(mut rows: Vec<&mut [f32]>) -> Vec<&'b mut [f32]> {
    rows.clear();
    let allocation = rows.as_ptr();
    let result: Vec<_> = rows.into_iter().map(|_| unreachable!()).collect();
    debug_assert_eq!(allocation, result.as_ptr());
    result
}

fn reach<'a>(rows: &'a [f32], root: &'a [f32], offset: usize) -> &'a [f32] {
    if offset == usize::MAX {
        root
    } else {
        &rows[offset..offset + root.len()]
    }
}

/// Siblings are reserved consecutively before their descendants are filled.
/// A chance-free subtree is its root plus one descendant interval.
struct Subtree {
    root: NodeId,
    first: usize,
    end: usize,
}
impl Subtree {
    fn new(tree: &PublicTree, root: NodeId) -> Self {
        let node = tree.node(root);
        let first = node.first_child as usize;
        let mut end = first + node.num_children as usize;
        let mut id = first;
        while id < end {
            let node = &tree.nodes[id];
            if node.kind == NodeKind::Action {
                end = end.max(node.first_child as usize + node.num_children as usize);
            } else {
                debug_assert_eq!(node.kind, NodeKind::Terminal);
            }
            id += 1;
        }
        Self { root, first, end }
    }
    fn walk(&self) -> impl DoubleEndedIterator<Item = NodeId> {
        std::iter::once(self.root).chain(self.first as u32..self.end as u32)
    }
    fn index(&self, id: NodeId) -> usize {
        if id == self.root {
            0
        } else {
            id as usize - self.first + 1
        }
    }
    fn count(&self) -> usize {
        self.end - self.first + 1
    }
}

pub(super) fn enabled() -> bool {
    #[cfg(test)]
    {
        !tests::recursive_mode()
    }
    #[cfg(not(test))]
    {
        true
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run<E: TerminalEvaluator, V: StorageView, const PRUNE: bool>(
    ctx: &PassCtx<'_, E>,
    storage: &mut V,
    scratch: &mut Scratch,
    root: NodeId,
    my_reach: &[f32],
    opp_reach: &[f32],
    out: &mut [f32],
) {
    let mut lease = MetadataLease::take();
    let metadata = &mut lease.0;
    let tree = ctx.tree;
    let subtree = Subtree::new(tree, root);
    let first = subtree.first;
    let count = subtree.count();
    let span = tree.storage_spans[root as usize];
    let actions = &tree.storage_nodes[span.sref_start as usize..span.sref_end as usize];
    let sigma_len = span.end - span.start;
    let own_len = my_reach.len();
    let mut sigma = scratch.take_overwrite(sigma_len);
    let mut scaled = scratch.take_overwrite(sigma_len);
    let mut values = scratch.take_overwrite(count * own_len);
    metadata.nodes.resize(count, Reaches::default());
    metadata.nodes[0] = Reaches {
        my: usize::MAX,
        opp: usize::MAX,
        zero_opp: false,
    };
    let walk = || subtree.walk();

    // Phase 1: storage preorder prepares each parent's strategies and scaled
    // reaches before its children. Value/reach metadata stays node-id-indexed.
    // Unchanged reaches are indices into ancestors (or the input rows).
    for &id in actions {
        let index = subtree.index(id);
        let node = tree.node(id);
        let sref = tree.storage_ref(node);
        let offset = sref.offset - span.start;
        let parent = metadata.nodes[index];
        let (previous, children) = scaled.split_at_mut(offset);
        let my = reach(previous, my_reach, parent.my);
        let opp = reach(previous, opp_reach, parent.opp);
        let zero_opp = PRUNE && node.player != ctx.p && all_zero(opp);
        metadata.nodes[index].zero_opp = zero_opp;
        let strategy = &mut sigma[offset..offset + sref.len()];
        if !zero_opp {
            storage.regret_matching_cfr(sref, sref.index, strategy, CfrPrecision::F32);
        }
        let length = sref.num_hands as usize;
        for (a, child) in tree.children(id).enumerate() {
            let mut next = parent;
            next.zero_opp = false;
            let start = a * length;
            if !zero_opp {
                let row = &mut children[start..start + length];
                if node.player == ctx.p {
                    mul_into(row, my, &strategy[start..start + length]);
                    next.my = offset + start;
                } else {
                    mul_into(row, opp, &strategy[start..start + length]);
                    next.opp = offset + start;
                }
            }
            metadata.nodes[child as usize - first + 1] = next;
        }
    }

    // Phase 2: the evaluator contract requires zero-initialized terminal rows,
    // including hands an evaluator leaves untouched. Pruned rows stay zero.
    metadata.terminals.clear();
    let mut reaches = shared_rows(std::mem::take(&mut metadata.reaches));
    let mut outputs = mutable_rows(std::mem::take(&mut metadata.outputs));
    let mut rows = values.chunks_exact_mut(own_len.max(1));
    for (index, id) in walk().enumerate() {
        let row = if own_len == 0 {
            &mut []
        } else {
            rows.next().unwrap()
        };
        let node = tree.node(id);
        if node.kind == NodeKind::Terminal {
            row.fill(0.0);
            let opp = reach(&scaled, opp_reach, metadata.nodes[index].opp);
            if !PRUNE || !all_zero(opp) {
                metadata.terminals.push(node.aux);
                reaches.push(opp);
                outputs.push(row);
            }
        }
    }
    ctx.evaluator
        .eval_cfr_batch(&metadata.terminals, ctx.p, &reaches, &mut outputs);
    metadata.outputs = mutable_rows(outputs);
    metadata.reaches = shared_rows(reaches);

    // Phase 3: reverse storage preorder is child-before-parent. Per-node
    // action order and update arguments are unchanged; refs are independent.
    for &id in actions.iter().rev() {
        let index = subtree.index(id);
        let node = tree.node(id);
        let sref = tree.storage_ref(node);
        let offset = sref.offset - span.start;
        let strategy = &sigma[offset..offset + sref.len()];
        let (parents, children) = values.split_at_mut((index + 1) * own_len);
        let value = &mut parents[index * own_len..];
        let child_row = |child: NodeId| {
            let start = (child as usize - first - index) * own_len;
            &children[start..start + own_len]
        };
        if node.player == ctx.p {
            if node.num_children == 0 {
                value.fill(0.0);
            }
            for (a, (child, row)) in tree
                .children(id)
                .zip(strategy.chunks_exact(own_len.max(1)))
                .enumerate()
            {
                for ((dst, &s), &v) in value.iter_mut().zip(row).zip(child_row(child)) {
                    if a == 0 {
                        *dst = s * v + 0.0;
                    } else {
                        *dst += s * v;
                    }
                }
            }
            // PublicTree::children is a consecutive node-id range, hence
            // the child value rows form one contiguous action-major block.
            debug_assert_eq!(tree.children(id).len(), sref.num_actions as usize);
            let start = if sref.is_empty() {
                0
            } else {
                (node.first_child as usize - first - index) * own_len
            };
            storage.update_regrets_from_values(
                sref,
                sref.index,
                &children[start..start + sref.len()],
                value,
                ctx.discounts,
            );
            // Phase 1 already wrote my_reach * sigma for every action.
            // Reuse those exact rows for the average update, after regrets.
            storage.accumulate_strategy(
                sref,
                sref.index,
                &scaled[offset..offset + sref.len()],
                ctx.discounts,
            );
        } else if !metadata.nodes[index].zero_opp && node.num_children != 0 {
            for (a, child) in tree.children(id).enumerate() {
                for (dst, &v) in value.iter_mut().zip(child_row(child)) {
                    if a == 0 {
                        *dst = v + 0.0;
                    } else {
                        *dst += v;
                    }
                }
            }
        } else {
            value.fill(0.0);
        }
        #[cfg(test)]
        record(id, &values[index * own_len..(index + 1) * own_len]);
    }
    out.copy_from_slice(&values[..own_len]);
    scratch.put(values);
    scratch.put(scaled);
    scratch.put(sigma);
}

/// Exact EV/BR for a small chance-free subtree, reduced in original child order.
/// Recording walks deliberately retain the recursive implementation.
pub(super) fn values<E: TerminalEvaluator, S: Storage, const EV: bool, const BR: bool>(
    ctx: &ValueCtx<'_, E, S>,
    scratch: &mut Scratch,
    root: NodeId,
    opp_reach: &[f32],
    ev: &mut [f32],
    br: &mut [f32],
) {
    let tree = ctx.tree;
    let subtree = Subtree::new(tree, root);
    let first = subtree.first;
    let count = subtree.count();
    let span = tree.storage_spans[root as usize];
    let sigma_len = span.end - span.start;
    let dim = if EV { ev.len() } else { br.len() };
    let channels = EV as usize + BR as usize;
    let mut sigma = scratch.take_overwrite(sigma_len);
    let mut scaled = scratch.take_overwrite(sigma_len);
    let mut values = scratch.take(count * dim * channels);
    let mut lease = MetadataLease::take();
    let metadata = &mut lease.0;
    metadata.nodes.resize(count, Reaches::default());
    metadata.nodes[0].opp = usize::MAX;
    let walk = || subtree.walk();
    for (index, id) in walk().enumerate() {
        let node = tree.node(id);
        if node.kind == NodeKind::Terminal {
            continue;
        }
        let sref = tree.storage_ref(node);
        let offset = sref.offset - span.start;
        let parent = metadata.nodes[index].opp;
        let (previous, children) = scaled.split_at_mut(offset);
        let opp = reach(previous, opp_reach, parent);
        let zero = all_zero(opp);
        metadata.nodes[index].zero_opp = zero;
        if !zero && (EV || node.player != ctx.p) {
            ctx.storage
                .average_strategy(sref, sref.index, &mut sigma[offset..offset + sref.len()]);
        }
        let hands = sref.num_hands as usize;
        for (a, child) in tree.children(id).enumerate() {
            let next = if node.player == ctx.p {
                parent
            } else {
                let row = &mut children[a * hands..(a + 1) * hands];
                if zero {
                    row.fill(0.0);
                } else {
                    mul_into(
                        row,
                        opp,
                        &sigma[offset + a * hands..offset + (a + 1) * hands],
                    );
                }
                offset + a * hands
            };
            metadata.nodes[child as usize - first + 1].opp = next;
        }
    }
    metadata.terminals.clear();
    let mut reaches = shared_rows(std::mem::take(&mut metadata.reaches));
    let mut outputs = mutable_rows(std::mem::take(&mut metadata.outputs));
    for ((index, id), row) in walk()
        .enumerate()
        .zip(values.chunks_exact_mut(dim * channels))
    {
        let node = tree.node(id);
        if node.kind == NodeKind::Terminal {
            let opp = reach(&scaled, opp_reach, metadata.nodes[index].opp);
            if !all_zero(opp) {
                metadata.terminals.push(node.aux);
                reaches.push(opp);
                outputs.push(&mut row[..dim]);
            }
        }
    }
    ctx.evaluator
        .eval_batch(&metadata.terminals, ctx.p, &reaches, &mut outputs);
    metadata.outputs = mutable_rows(outputs);
    metadata.reaches = shared_rows(reaches);
    for id in walk().rev() {
        let index = subtree.index(id);
        let node = tree.node(id);
        let width = dim * channels;
        let (parents, children) = values.split_at_mut((index + 1) * width);
        let row = &mut parents[index * width..];
        if node.kind == NodeKind::Terminal {
            if EV && BR {
                let (e, b) = row.split_at_mut(dim);
                b.copy_from_slice(e);
            }
            continue;
        }
        if metadata.nodes[index].zero_opp {
            continue;
        }
        let (e, b) = value_row::<EV, BR>(row, dim);
        let own = node.player == ctx.p;
        if own && BR {
            b.fill(f32::NEG_INFINITY);
        }
        let sref = tree.storage_ref(node);
        let offset = sref.offset - span.start;
        for (a, child) in tree.children(id).enumerate() {
            let start = (child as usize - first - index) * width;
            let row = &children[start..start + width];
            if EV {
                if own {
                    let strategy = &sigma[offset + a * dim..offset + (a + 1) * dim];
                    for ((dst, &s), &v) in e.iter_mut().zip(strategy).zip(&row[..dim]) {
                        *dst += s * v;
                    }
                } else {
                    for (dst, &v) in e.iter_mut().zip(&row[..dim]) {
                        *dst += v;
                    }
                }
            }
            if BR {
                let child = &row[EV as usize * dim..];
                if own {
                    for (dst, &v) in b.iter_mut().zip(child) {
                        *dst = dst.max(v);
                    }
                } else {
                    for (dst, &v) in b.iter_mut().zip(child) {
                        *dst += v;
                    }
                }
            }
        }
    }
    if EV {
        ev.copy_from_slice(&values[..dim]);
    }
    if BR {
        br.copy_from_slice(&values[EV as usize * dim..(EV as usize + 1) * dim]);
    }
    scratch.put(values);
    scratch.put(scaled);
    scratch.put(sigma);
}

#[cfg(test)]
pub(super) fn record(id: NodeId, values: &[f32]) {
    tests::record(id, values);
}
#[cfg(test)]
#[path = "batch_tests.rs"]
mod tests;
