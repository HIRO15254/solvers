//! Non-recursive f32 CFR for a chance-free subtree below the action split bound.
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
    struct Lease(Metadata);
    impl Drop for Lease {
        fn drop(&mut self) {
            METADATA.with(|pool| pool.borrow_mut().push(std::mem::take(&mut self.0)));
        }
    }
    let mut lease = Lease(METADATA.with(|pool| pool.borrow_mut().pop().unwrap_or_default()));
    let metadata = &mut lease.0;
    let tree = ctx.tree;
    let node = tree.node(root);
    let first = node.first_child as usize;
    let mut end = first + node.num_children as usize;
    let mut id = first;
    let mut work_len = 0;
    // fill reserves siblings before filling them: descendants occupy one
    // interval starting at first_child, excluding root's parent-block slot.
    // Extend its bound from child blocks in this metadata-only sizing walk.
    while id < end {
        let node = &tree.nodes[id];
        if node.kind == NodeKind::Action {
            end = end.max(node.first_child as usize + node.num_children as usize);
            if node.player == ctx.p {
                work_len = work_len.max(tree.storage_ref(node).len());
            }
        } else {
            debug_assert_eq!(node.kind, NodeKind::Terminal);
        }
        id += 1;
    }
    if node.player == ctx.p {
        work_len = work_len.max(tree.storage_ref(node).len());
    }
    let count = end - first + 1;
    let span = tree.storage_spans[root as usize];
    let sigma_len = span.end - span.start;
    let own_len = my_reach.len();
    let mut sigma = scratch.take_overwrite(sigma_len);
    let mut scaled = scratch.take_overwrite(sigma_len);
    let mut values = scratch.take(count * own_len);
    let mut work = scratch.take_overwrite(work_len);
    metadata.nodes.resize(count, Reaches::default());
    metadata.nodes[0] = Reaches {
        my: usize::MAX,
        opp: usize::MAX,
        zero_opp: false,
    };
    let walk = || std::iter::once(root).chain((first as u32)..(end as u32));

    // Phase 1: all strategies and action-scaled reaches, in parent-before-child
    // order. Unchanged reaches are indices into ancestors (or the input rows).
    for (index, id) in walk().enumerate() {
        let node = tree.node(id);
        if node.kind == NodeKind::Terminal {
            continue;
        }
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

    // Phase 2: one call, separate zeroed terminal rows. PRUNE leaves excluded
    // rows zero, without preventing updates at own nodes below a zero reach.
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

    // Phase 3: child-before-parent. Per-node action order and storage update
    // arguments/order match recursion; every storage ref is independent.
    for id in walk().rev() {
        let index = if id == root {
            0
        } else {
            id as usize - first + 1
        };
        let node = tree.node(id);
        if node.kind == NodeKind::Action {
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
                for (child, row) in tree.children(id).zip(strategy.chunks_exact(own_len.max(1))) {
                    for ((dst, &s), &v) in value.iter_mut().zip(row).zip(child_row(child)) {
                        *dst += s * v;
                    }
                }
                let tmp = &mut work[..sref.len()];
                for (child, row) in tree.children(id).zip(tmp.chunks_exact_mut(own_len.max(1))) {
                    for ((dst, &v), &cfv) in row.iter_mut().zip(child_row(child)).zip(value.iter())
                    {
                        *dst = v - cfv;
                    }
                }
                storage.update_regrets(sref, sref.index, tmp, ctx.discounts);
                // Phase 1 already wrote my_reach * sigma for every action.
                // Reuse those exact rows for the average update, after regrets.
                storage.accumulate_strategy(
                    sref,
                    sref.index,
                    &scaled[offset..offset + sref.len()],
                    ctx.discounts,
                );
            } else if !metadata.nodes[index].zero_opp {
                for child in tree.children(id) {
                    for (dst, &v) in value.iter_mut().zip(child_row(child)) {
                        *dst += v;
                    }
                }
            }
        }
        #[cfg(test)]
        record(id, &values[index * own_len..(index + 1) * own_len]);
    }
    out.copy_from_slice(&values[..own_len]);
    scratch.put(work);
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
