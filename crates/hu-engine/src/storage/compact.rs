use super::*;
use crate::tree::{NodeId, PublicTree};
use std::sync::Arc;

/// Supplies average strategies for action references outside the compact scope.
pub type ExternalStrategyProvider = Arc<dyn Fn(StorageRef, &mut [f32]) + Send + Sync>;

#[derive(Clone, Copy)]
struct Entry {
    offset: usize,
    index: u32,
    selected: bool,
}

/// Storage for action references in selected subtrees, addressed by the original tree's refs.
/// The underlying arenas and per-node scales contain only selected action nodes.
pub struct CompactStorage<S> {
    inner: S,
    map: Vec<Entry>,
    external: Option<ExternalStrategyProvider>,
}

impl<S: Storage> CompactStorage<S> {
    /// Allocate storage for the union of the specified subtree roots.
    pub fn for_subtrees(tree: &PublicTree, roots: &[NodeId]) -> Self {
        let mut selected = vec![false; tree.storage_refs.len()];
        for &root in roots {
            let span = tree.storage_spans[root as usize];
            selected[span.sref_start as usize..span.sref_end as usize].fill(true);
        }
        let mut map = Vec::with_capacity(selected.len() + 1);
        let mut offset = 0;
        let mut index = 0;
        for (r, selected) in tree.storage_refs.iter().zip(selected) {
            map.push(Entry {
                offset,
                index,
                selected,
            });
            if selected {
                offset += r.len();
                index += 1;
            }
        }
        map.push(Entry {
            offset,
            index,
            selected: false,
        });
        Self {
            inner: S::new(offset, index as usize),
            map,
            external: None,
        }
    }

    /// Whether an original tree storage reference belongs to the selected scope.
    pub fn contains(&self, index: u32) -> bool {
        self.map
            .get(index as usize)
            .is_some_and(|entry| entry.selected)
    }

    /// Set the source of average strategies outside the selected scope.
    /// The callback must fill every entry or fail explicitly.
    pub fn set_external_strategy_provider(&mut self, provider: ExternalStrategyProvider) {
        self.external = Some(provider);
    }
}

fn translate(map: &[Entry], mut r: StorageRef, index: u32) -> (StorageRef, u32) {
    assert_eq!(r.index, index, "storage reference index mismatch");
    let entry = map[index as usize];
    assert!(entry.selected, "storage reference is outside compact scope");
    r.offset = entry.offset;
    r.index = entry.index;
    (r, entry.index)
}

fn translate_span(map: &[Entry], span: StorageSpan) -> StorageSpan {
    let start = map[span.sref_start as usize];
    let end = map[span.sref_end as usize];
    StorageSpan {
        start: start.offset,
        end: end.offset,
        sref_start: start.index,
        sref_end: end.index,
    }
}

/// A compact storage view retaining the original tree's addressing.
pub struct CompactView<'a, V> {
    inner: V,
    map: &'a [Entry],
    external: &'a Option<ExternalStrategyProvider>,
}

macro_rules! compact_ops {
    ($ty:ty, $bound:ident) => {
        impl<$bound: StorageOps> StorageOps for $ty {
            fn regret_matching(&self, r: StorageRef, index: u32, out: &mut [f32]) {
                let (r, index) = translate(&self.map, r, index);
                self.inner.regret_matching(r, index, out);
            }
            fn regret_matching_cfr(
                &self,
                r: StorageRef,
                index: u32,
                out: &mut [f32],
                norm: CfrPrecision,
            ) {
                let (r, index) = translate(&self.map, r, index);
                self.inner.regret_matching_cfr(r, index, out, norm);
            }
            fn update_regrets(&mut self, r: StorageRef, index: u32, inst: &[f32], d: &Discounts) {
                let (r, index) = translate(&self.map, r, index);
                self.inner.update_regrets(r, index, inst, d);
            }
            fn accumulate_strategy(
                &mut self,
                r: StorageRef,
                index: u32,
                weighted: &[f32],
                d: &Discounts,
            ) {
                let (r, index) = translate(&self.map, r, index);
                self.inner.accumulate_strategy(r, index, weighted, d);
            }
            fn average_strategy(&self, r: StorageRef, index: u32, out: &mut [f32]) {
                if self.map[index as usize].selected {
                    let (r, index) = translate(&self.map, r, index);
                    self.inner.average_strategy(r, index, out);
                } else {
                    assert_eq!(r.index, index, "storage reference index mismatch");
                    self.external
                        .as_ref()
                        .expect("no external strategy provider for compact storage")(
                        r, out
                    );
                }
            }
        }
    };
}
compact_ops!(CompactStorage<S>, S);
compact_ops!(CompactView<'_, V>, V);

impl<'a, V: StorageView> StorageView for CompactView<'a, V> {
    fn split(&mut self, spans: &[StorageSpan]) -> Vec<Self> {
        let spans: Vec<_> = spans
            .iter()
            .copied()
            .map(|span| translate_span(self.map, span))
            .collect();
        self.inner
            .split(&spans)
            .into_iter()
            .map(|inner| Self {
                inner,
                map: self.map,
                external: self.external,
            })
            .collect()
    }
}

impl<S: Storage> Storage for CompactStorage<S> {
    type View<'a>
        = CompactView<'a, S::View<'a>>
    where
        S: 'a;
    fn new(_len: usize, _num_refs: usize) -> Self {
        panic!("CompactStorage requires for_subtrees and a tree")
    }
    fn release_regrets(&mut self) {
        self.inner.release_regrets();
    }
    fn regrets_released(&self) -> bool {
        self.inner.regrets_released()
    }
    fn view_mut(&mut self) -> Self::View<'_> {
        CompactView {
            inner: self.inner.view_mut(),
            map: &self.map,
            external: &self.external,
        }
    }
    fn bytes_for(len: usize, num_refs: usize) -> u64 {
        S::bytes_for(len, num_refs)
    }
    fn state(&self) -> StorageState {
        self.inner.state()
    }
    fn restore_state(&mut self, state: StorageState) -> Result<(), StateMismatch> {
        self.inner.restore_state(state)
    }
    fn arrays(&self) -> StorageArrays<'_> {
        self.inner.arrays()
    }
    fn arrays_mut(&mut self) -> StorageArraysMut<'_> {
        self.inner.arrays_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TempNode, TreeSpec};
    use nlh::{PerPlayer, Player};

    fn tree() -> PublicTree {
        fn branch() -> TempNode {
            TempNode::Action {
                player: Player::P1,
                tag: 0,
                children: vec![
                    TempNode::Terminal { id: 0, tag: 0 },
                    TempNode::Terminal { id: 1, tag: 0 },
                ],
            }
        }
        PublicTree::compile(TreeSpec {
            root: TempNode::Action {
                player: Player::P0,
                tag: 0,
                children: vec![branch(), branch(), branch()],
            },
            root_dims: PerPlayer::new(3, 3),
            masks: vec![],
            transitions: vec![],
        })
    }

    fn check<S: Storage>() {
        let tree = tree();
        let children: Vec<_> = tree.children(0).collect();
        let mut compact = CompactStorage::<S>::for_subtrees(&tree, &[children[0], children[2]]);
        let mut full = S::new(tree.storage_len, tree.storage_refs.len());
        let d = Discounts {
            pos: 1.0,
            neg: 1.0,
            avg: 1.0,
            floor_neg: false,
            reset_avg: false,
        };
        for &node in &[children[0], children[2]] {
            let r = tree.storage_ref(tree.node(node));
            let values: Vec<_> = (0..r.len()).map(|i| (i + 1) as f32).collect();
            full.update_regrets(r, r.index, &values, &d);
            full.accumulate_strategy(r, r.index, &values, &d);
            compact.update_regrets(r, r.index, &values, &d);
            compact.accumulate_strategy(r, r.index, &values, &d);
            let mut a = vec![0.0; r.len()];
            let mut b = a.clone();
            full.average_strategy(r, r.index, &mut a);
            compact.average_strategy(r, r.index, &mut b);
            assert_eq!(a, b);
        }
        let span = tree.storage_spans[children[2] as usize];
        let r = tree.storage_ref(tree.node(children[2]));
        let mut a = vec![0.0; r.len()];
        {
            let mut view = compact.view_mut();
            let mut split = view.split(&[span]);
            let values = vec![1.0; r.len()];
            split[0].update_regrets(r, r.index, &values, &d);
            full.update_regrets(r, r.index, &values, &d);
            let mut b = a.clone();
            split[0].regret_matching(r, r.index, &mut a);
            full.regret_matching(r, r.index, &mut b);
            assert_eq!(a, b);
        }
        let state = compact.state();
        let mut restored = CompactStorage::<S>::for_subtrees(&tree, &[children[0], children[2]]);
        restored.restore_state(state.clone()).unwrap();
        assert_eq!(restored.state(), state);
        let outside = tree.storage_ref(tree.node(children[1]));
        assert!(!restored.contains(outside.index));
        restored.set_external_strategy_provider(Arc::new(|_, out| out.fill(0.25)));
        restored.average_strategy(outside, outside.index, &mut a);
        assert!(a.iter().all(|&x| x == 0.25));
        assert!(
            S::bytes_for(2 * r.len(), 2) < S::bytes_for(tree.storage_len, tree.storage_refs.len())
        );
    }

    #[test]
    fn selected_refs_and_split_views_match_full_storage() {
        check::<F32Storage>();
        check::<I16Storage>();
        check::<MixedStorage>();
    }
}
