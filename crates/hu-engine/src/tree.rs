use nlh::{PerPlayer, Player};

use crate::storage::{StorageRef, StorageSpan};

pub type NodeId = u32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    Action,
    Chance,
    Terminal,
}

/// A public-tree node. Children occupy the contiguous index range
/// `[first_child, first_child + num_children)`.
#[derive(Clone, Copy, Debug)]
pub struct Node {
    pub kind: NodeKind,
    /// Acting player for `Action` nodes.
    pub player: Player,
    pub num_children: u16,
    pub first_child: u32,
    /// `Action`: index into `storage_refs`. `Chance`: index of the node's
    /// first entry in `deals` (one per child). `Terminal`: terminal id passed
    /// to the [`crate::TerminalEvaluator`].
    pub aux: u32,
}

/// How one player's reach vector transforms across a chance branch.
///
/// `Mask` covers public-card deals (hold'em turn/river, stud upcards): the
/// reach vector is multiplied elementwise by a shared 0/1 card-removal mask.
/// `Transition` covers private-state transitions (draw-game hand exchanges):
/// a sparse linear map, possibly changing the vector's dimension. Keeping
/// both in the engine from day one is deliberate — see
/// `docs/architecture.md` on Draw/Stud extensibility.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReachMap {
    Identity,
    /// Index into [`PublicTree::masks`].
    Mask(u32),
    /// Index into [`PublicTree::transitions`].
    Transition(u32),
}

/// One chance branch: multiplicity weight (suit-isomorphism classes fold
/// their class size in here) and per-player reach maps.
#[derive(Clone, Copy, Debug)]
pub struct Deal {
    pub weight: f32,
    pub maps: PerPlayer<ReachMap>,
}

/// Sparse linear map between private-state spaces, used forward for reach
/// vectors and backward for value vectors.
#[derive(Clone, Debug)]
pub struct SparseTransition {
    pub in_dim: u32,
    pub out_dim: u32,
    /// `(in_index, out_index, weight)` triples.
    pub entries: Vec<(u32, u32, f32)>,
}

impl SparseTransition {
    /// reach_out[j] = sum_i w_ij * reach_in[i]
    pub fn apply_forward(&self, reach_in: &[f32], reach_out: &mut [f32]) {
        debug_assert_eq!(reach_in.len(), self.in_dim as usize);
        debug_assert_eq!(reach_out.len(), self.out_dim as usize);
        reach_out.fill(0.0);
        for &(i, j, w) in &self.entries {
            reach_out[j as usize] += w * reach_in[i as usize];
        }
    }

    /// value_in[i] = sum_j w_ij * value_out[j]
    pub fn apply_backward(&self, value_out: &[f32], value_in: &mut [f32]) {
        debug_assert_eq!(value_out.len(), self.out_dim as usize);
        debug_assert_eq!(value_in.len(), self.in_dim as usize);
        value_in.fill(0.0);
        for &(i, j, w) in &self.entries {
            value_in[i as usize] += w * value_out[j as usize];
        }
    }
}

/// Compiled public tree consumed by the solver. Built once per game
/// configuration; immutable during solving.
pub struct PublicTree {
    pub nodes: Vec<Node>,
    /// One entry per chance-node child, indexed by `Node::aux + child_pos`.
    pub deals: Vec<Deal>,
    pub storage_refs: Vec<StorageRef>,
    /// Action node ids in storage-ref order (DFS preorder). A subtree's
    /// `StorageSpan::sref_start..sref_end` selects its action nodes.
    pub(crate) storage_nodes: Vec<NodeId>,
    /// Shared card-removal masks referenced by [`ReachMap::Mask`].
    pub masks: Vec<Vec<f32>>,
    /// Private-state transitions referenced by [`ReachMap::Transition`].
    pub transitions: Vec<SparseTransition>,
    /// Private-state dimension per player at the root.
    pub root_dims: PerPlayer<u32>,
    /// Total f32 elements needed per storage buffer (regrets, strategy sum).
    pub storage_len: usize,
    /// Builder-owned tag per node (see [`TempNode`]).
    pub tags: Vec<u32>,
    /// Per-node subtree storage footprint, indexed by [`NodeId`]. DFS
    /// preorder allocation means every subtree's storage is a single
    /// contiguous range, so a node's span always covers the union of its
    /// children's spans.
    pub storage_spans: Vec<StorageSpan>,
    /// Whether this node is a [`NodeKind::Chance`] node or has one anywhere
    /// in its subtree, indexed by [`NodeId`]. A chance node with enough
    /// children may run its children in parallel (see [`crate::ParConfig`]),
    /// which `StorageView::split`s whatever view it's handed — including,
    /// via the ordinary Rust reborrow an unsplit ancestor passes down, the
    /// *ancestor's own* view. `false` here is the solver's guarantee that no
    /// descendant can ever do that, so an action node can keep reusing its
    /// ambient storage view across its children (and after them, for its
    /// own regret/strategy update) with no protective split of its own.
    pub subtree_has_chance: Vec<bool>,
}

/// Build-time tree description, converted by [`PublicTree::compile`].
/// `tag` is an opaque builder-owned id copied into [`PublicTree::tags`],
/// letting builders attach metadata (histories, action labels) to compiled
/// node ids without the engine knowing about it.
pub enum TempNode {
    Action {
        player: Player,
        children: Vec<TempNode>,
        tag: u32,
    },
    Chance {
        deals: Vec<(f32, PerPlayer<ReachMap>, TempNode)>,
        tag: u32,
    },
    Terminal {
        id: u32,
        tag: u32,
    },
}

pub struct TreeSpec {
    pub root: TempNode,
    pub masks: Vec<Vec<f32>>,
    pub transitions: Vec<SparseTransition>,
    pub root_dims: PerPlayer<u32>,
}

/// Layout from which [`PublicTree::from_shape`] derives every other field of
/// a compiled tree. [`PublicTree::compile`] assigns child ranges, storage
/// refs, deal offsets, spans and chance flags in one deterministic walk, so
/// node kinds and child counts by id fix them all. Builder tags are not part
/// of a shape.
#[derive(Clone, Debug)]
pub struct TreeShape {
    /// Every node by id.
    pub nodes: Vec<ShapeNode>,
    /// `Node::aux` of the terminal nodes, ascending by node id.
    pub terminal_ids: Vec<u32>,
    /// Chance branches in [`PublicTree::deals`] order.
    pub deals: Vec<Deal>,
    pub masks: Vec<Vec<f32>>,
    pub transitions: Vec<SparseTransition>,
    pub root_dims: PerPlayer<u32>,
}

/// One node of a [`TreeShape`]. `player` is `P0` except at action nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapeNode {
    pub kind: NodeKind,
    pub player: Player,
    pub num_children: u16,
}

/// Why a [`TreeShape`] describes no compiled tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapeError(pub &'static str);

impl std::fmt::Display for ShapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "inconsistent tree shape: {}", self.0)
    }
}

impl std::error::Error for ShapeError {}

/// Next unassigned node id and deal entry while placing a [`TreeShape`].
struct ShapeCursor {
    node: usize,
    deal: usize,
}

impl PublicTree {
    pub fn compile(spec: TreeSpec) -> PublicTree {
        let mut tree = PublicTree {
            nodes: Vec::new(),
            deals: Vec::new(),
            storage_refs: Vec::new(),
            storage_nodes: Vec::new(),
            masks: spec.masks,
            transitions: spec.transitions,
            root_dims: spec.root_dims,
            storage_len: 0,
            tags: Vec::new(),
            storage_spans: Vec::new(),
            subtree_has_chance: Vec::new(),
        };
        tree.nodes.push(placeholder_node());
        tree.tags.push(0);
        tree.storage_spans.push(StorageSpan::default());
        tree.subtree_has_chance.push(false);
        let root_dims = tree.root_dims;
        tree.fill(0, &spec.root, root_dims);
        tree
    }

    /// Rebuilds the tree [`Self::compile`] produced from the same layout,
    /// field for field, except that every tag is zero. Fails instead of
    /// panicking on a shape no compile can produce: unreachable or missing
    /// nodes, unused or missing deals, terminal ids that do not match the
    /// terminals, or reach maps whose dimensions disagree.
    pub fn from_shape(shape: TreeShape) -> Result<PublicTree, ShapeError> {
        let count = shape.nodes.len();
        if count == 0 {
            return Err(ShapeError("a tree needs a root"));
        }
        for tr in &shape.transitions {
            if tr
                .entries
                .iter()
                .any(|&(i, j, _)| i >= tr.in_dim || j >= tr.out_dim)
            {
                return Err(ShapeError("a transition entry is outside its dimensions"));
            }
        }
        let mut tree = PublicTree {
            nodes: vec![placeholder_node(); count],
            deals: shape.deals,
            storage_refs: Vec::new(),
            storage_nodes: Vec::new(),
            masks: shape.masks,
            transitions: shape.transitions,
            root_dims: shape.root_dims,
            storage_len: 0,
            tags: vec![0; count],
            storage_spans: vec![StorageSpan::default(); count],
            subtree_has_chance: vec![false; count],
        };
        let mut cursor = ShapeCursor { node: 1, deal: 0 };
        tree.place(0, &shape.nodes, &mut cursor, shape.root_dims)?;
        if cursor.node != count {
            return Err(ShapeError("some nodes are unreachable from the root"));
        }
        if cursor.deal != tree.deals.len() {
            return Err(ShapeError("some deals belong to no chance node"));
        }
        let mut ids = shape.terminal_ids.into_iter();
        for node in &mut tree.nodes {
            if node.kind == NodeKind::Terminal {
                node.aux = ids
                    .next()
                    .ok_or(ShapeError("fewer terminal ids than terminals"))?;
            }
        }
        if ids.next().is_some() {
            return Err(ShapeError("more terminal ids than terminals"));
        }
        Ok(tree)
    }

    /// [`Self::fill`] for a [`TreeShape`]: the same walk, reading each
    /// node's kind and child count instead of a [`TempNode`].
    fn place(
        &mut self,
        slot: usize,
        shape: &[ShapeNode],
        cursor: &mut ShapeCursor,
        dims: PerPlayer<u32>,
    ) -> Result<(), ShapeError> {
        let start = self.storage_len;
        let sref_start = self.storage_refs.len() as u32;
        let node = shape[slot];
        let count = node.num_children as usize;
        let mut has_chance = node.kind == NodeKind::Chance;
        if node.kind == NodeKind::Terminal {
            if count != 0 {
                return Err(ShapeError("a terminal has children"));
            }
            self.nodes[slot] = Node {
                kind: NodeKind::Terminal,
                player: Player::P0,
                num_children: 0,
                first_child: 0,
                aux: 0,
            };
        } else {
            if count == 0 {
                return Err(ShapeError("an action or chance node has no children"));
            }
            if shape.len() - cursor.node < count {
                return Err(ShapeError("children run past the last node"));
            }
            let first_child = cursor.node;
            cursor.node += count;
            if node.kind == NodeKind::Action {
                let num_hands = dims[node.player];
                let aux = self.storage_refs.len() as u32;
                self.storage_refs.push(StorageRef {
                    offset: self.storage_len,
                    num_actions: node.num_children,
                    num_hands,
                    index: aux,
                });
                self.storage_nodes.push(slot as NodeId);
                self.storage_len += count * num_hands as usize;
                self.nodes[slot] = Node {
                    kind: NodeKind::Action,
                    player: node.player,
                    num_children: node.num_children,
                    first_child: first_child as u32,
                    aux,
                };
                for child in first_child..first_child + count {
                    self.place(child, shape, cursor, dims)?;
                    has_chance |= self.subtree_has_chance[child];
                }
            } else {
                if self.deals.len() - cursor.deal < count {
                    return Err(ShapeError("chance nodes need more deals than exist"));
                }
                let aux = cursor.deal;
                cursor.deal += count;
                self.nodes[slot] = Node {
                    kind: NodeKind::Chance,
                    player: Player::P0,
                    num_children: node.num_children,
                    first_child: first_child as u32,
                    aux: aux as u32,
                };
                for pos in 0..count {
                    let maps = self.deals[aux + pos].maps;
                    let child_dims = PerPlayer::new(
                        self.checked_mapped_dim(maps[Player::P0], dims[Player::P0])?,
                        self.checked_mapped_dim(maps[Player::P1], dims[Player::P1])?,
                    );
                    self.place(first_child + pos, shape, cursor, child_dims)?;
                }
            }
        }
        self.storage_spans[slot] = StorageSpan {
            start,
            end: self.storage_len,
            sref_start,
            sref_end: self.storage_refs.len() as u32,
        };
        self.subtree_has_chance[slot] = has_chance;
        Ok(())
    }

    /// [`Self::mapped_dim`] for untrusted maps.
    fn checked_mapped_dim(&self, map: ReachMap, dim: u32) -> Result<u32, ShapeError> {
        match map {
            ReachMap::Identity => Ok(dim),
            ReachMap::Mask(m) => match self.masks.get(m as usize) {
                Some(mask) if mask.len() == dim as usize => Ok(dim),
                Some(_) => Err(ShapeError(
                    "a mask's length differs from the reach dimension",
                )),
                None => Err(ShapeError("a deal names a missing mask")),
            },
            ReachMap::Transition(t) => match self.transitions.get(t as usize) {
                Some(tr) if tr.in_dim == dim => Ok(tr.out_dim),
                Some(_) => Err(ShapeError(
                    "a transition's input dimension differs from the reach dimension",
                )),
                None => Err(ShapeError("a deal names a missing transition")),
            },
        }
    }

    /// Recursively fills `slot` from `temp`, reserving contiguous child
    /// blocks. `dims` is the per-player private-state dimension on the path.
    fn fill(&mut self, slot: usize, temp: &TempNode, dims: PerPlayer<u32>) {
        let start = self.storage_len;
        let sref_start = self.storage_refs.len() as u32;
        let mut has_chance = matches!(temp, TempNode::Chance { .. });
        match temp {
            TempNode::Terminal { id, tag } => {
                self.tags[slot] = *tag;
                self.nodes[slot] = Node {
                    kind: NodeKind::Terminal,
                    player: Player::P0,
                    num_children: 0,
                    first_child: 0,
                    aux: *id,
                };
            }
            TempNode::Action {
                player,
                children,
                tag,
            } => {
                self.tags[slot] = *tag;
                let num_actions = children.len();
                assert!(num_actions >= 1, "action node must have children");
                let num_hands = dims[*player] as usize;
                let aux = self.storage_refs.len() as u32;
                let storage_ref = StorageRef {
                    offset: self.storage_len,
                    num_actions: num_actions as u16,
                    num_hands: num_hands as u32,
                    index: aux,
                };
                self.storage_len += num_actions * num_hands;
                self.storage_refs.push(storage_ref);
                self.storage_nodes.push(slot as NodeId);

                let first_child = self.reserve_children(num_actions);
                self.nodes[slot] = Node {
                    kind: NodeKind::Action,
                    player: *player,
                    num_children: num_actions as u16,
                    first_child,
                    aux,
                };
                for (i, child) in children.iter().enumerate() {
                    self.fill(first_child as usize + i, child, dims);
                    has_chance |= self.subtree_has_chance[first_child as usize + i];
                }
            }
            TempNode::Chance { deals, tag } => {
                self.tags[slot] = *tag;
                assert!(!deals.is_empty(), "chance node must have deals");
                let first_child = self.reserve_children(deals.len());
                let aux = self.deals.len() as u32;
                for (weight, maps, _) in deals {
                    self.deals.push(Deal {
                        weight: *weight,
                        maps: *maps,
                    });
                }
                self.nodes[slot] = Node {
                    kind: NodeKind::Chance,
                    player: Player::P0,
                    num_children: deals.len() as u16,
                    first_child,
                    aux,
                };
                for (i, (_, maps, child)) in deals.iter().enumerate() {
                    let child_dims = PerPlayer::new(
                        self.mapped_dim(maps[Player::P0], dims[Player::P0]),
                        self.mapped_dim(maps[Player::P1], dims[Player::P1]),
                    );
                    self.fill(first_child as usize + i, child, child_dims);
                }
            }
        }
        self.storage_spans[slot] = StorageSpan {
            start,
            end: self.storage_len,
            sref_start,
            sref_end: self.storage_refs.len() as u32,
        };
        self.subtree_has_chance[slot] = has_chance;
    }

    fn reserve_children(&mut self, n: usize) -> u32 {
        let first = self.nodes.len() as u32;
        self.nodes.extend((0..n).map(|_| placeholder_node()));
        self.tags.extend((0..n).map(|_| 0));
        self.storage_spans
            .extend((0..n).map(|_| StorageSpan::default()));
        self.subtree_has_chance.extend((0..n).map(|_| false));
        first
    }

    /// Dimension one player's reach vector maps into across a chance deal.
    /// `Identity`/`Mask` never change it; a `Transition` may (see
    /// [`ReachMap`]'s doc comment) — this is the single source of truth for
    /// that, used both at compile time (here, sizing each chance child's
    /// subtree) and by every solver walk that recurses across a chance node
    /// (`crate::solver::cfr_pass`/`value_pass`),
    /// which must size their per-deal scratch buffers identically or trip
    /// `SparseTransition::apply_forward`'s dimension assertions.
    pub fn mapped_dim(&self, map: ReachMap, dim: u32) -> u32 {
        match map {
            ReachMap::Identity => dim,
            ReachMap::Mask(m) => {
                assert_eq!(
                    self.masks[m as usize].len(),
                    dim as usize,
                    "mask dimension mismatch"
                );
                dim
            }
            ReachMap::Transition(t) => {
                let tr = &self.transitions[t as usize];
                assert_eq!(tr.in_dim, dim, "transition input dimension mismatch");
                tr.out_dim
            }
        }
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn children(&self, id: NodeId) -> std::ops::Range<u32> {
        let node = self.node(id);
        node.first_child..node.first_child + node.num_children as u32
    }

    pub fn deal(&self, node: &Node, child_pos: usize) -> &Deal {
        &self.deals[node.aux as usize + child_pos]
    }

    pub fn storage_ref(&self, node: &Node) -> StorageRef {
        self.storage_refs[node.aux as usize]
    }

    /// Applies one player's reach map for a deal into a caller-provided
    /// buffer (non-allocating). `out`'s length must already match the
    /// mapped dimension (unchanged from `reach.len()` for `Identity`/`Mask`;
    /// the transition's `out_dim` for `Transition`).
    pub fn map_reach_into(&self, map: ReachMap, reach: &[f32], out: &mut [f32]) {
        match map {
            ReachMap::Identity => out.copy_from_slice(reach),
            ReachMap::Mask(m) => {
                let mask = &self.masks[m as usize];
                for ((o, &r), &m) in out.iter_mut().zip(reach).zip(mask) {
                    *o = r * m;
                }
            }
            ReachMap::Transition(t) => {
                self.transitions[t as usize].apply_forward(reach, out);
            }
        }
    }

    /// Pulls child-space values back to parent space for a deal branch and
    /// accumulates them into `acc` with the given weight. For masks this also
    /// zeroes values on hands incompatible with the deal, which is exactly
    /// the card-removal semantics the counterfactual sum requires.
    pub fn accumulate_values(&self, map: ReachMap, weight: f32, child: &[f32], acc: &mut [f32]) {
        if matches!(map, ReachMap::Transition(_)) {
            crate::scratch::with_worker_scratch(|scratch| {
                self.accumulate_values_with_scratch(map, weight, child, acc, scratch);
            });
        } else {
            self.accumulate_values_with_scratch(
                map,
                weight,
                child,
                acc,
                &mut crate::Scratch::new(),
            );
        }
    }

    /// As [`Self::accumulate_values`], reusing the backward-map buffer.
    pub fn accumulate_values_with_scratch(
        &self,
        map: ReachMap,
        weight: f32,
        child: &[f32],
        acc: &mut [f32],
        scratch: &mut crate::Scratch,
    ) {
        match map {
            ReachMap::Identity => {
                for (a, &v) in acc.iter_mut().zip(child) {
                    *a += weight * v;
                }
            }
            ReachMap::Mask(m) => {
                let mask = &self.masks[m as usize];
                for ((a, &v), &m) in acc.iter_mut().zip(child).zip(mask) {
                    *a += weight * m * v;
                }
            }
            ReachMap::Transition(t) => {
                let tr = &self.transitions[t as usize];
                let mut back = scratch.take(tr.in_dim as usize);
                tr.apply_backward(child, &mut back);
                for (a, &v) in acc.iter_mut().zip(&back) {
                    *a += weight * v;
                }
                scratch.put(back);
            }
        }
    }
}

fn placeholder_node() -> Node {
    Node {
        kind: NodeKind::Terminal,
        player: Player::P0,
        num_children: 0,
        first_child: 0,
        aux: u32::MAX,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal(id: u32) -> TempNode {
        TempNode::Terminal { id, tag: 0 }
    }

    fn action(player: Player, children: Vec<TempNode>) -> TempNode {
        TempNode::Action {
            player,
            children,
            tag: 0,
        }
    }

    /// Root is a chance node with two deals, each into a 2-action node.
    /// Spans must tile: root's span covers the whole arena, and its
    /// children's (the two action nodes') spans are disjoint and union to
    /// exactly the root's span.
    #[test]
    fn spans_tile_across_chance_subtree() {
        let branch =
            |base_id: u32| action(Player::P0, vec![terminal(base_id), terminal(base_id + 1)]);
        let identity_maps = PerPlayer::new(ReachMap::Identity, ReachMap::Identity);
        let root = TempNode::Chance {
            deals: vec![
                (0.5, identity_maps, branch(0)),
                (0.5, identity_maps, branch(2)),
            ],
            tag: 0,
        };
        let tree = PublicTree::compile(TreeSpec {
            root,
            masks: Vec::new(),
            transitions: Vec::new(),
            root_dims: PerPlayer::new(2, 2),
        });

        let root_node = tree.node(0);
        assert_eq!(root_node.kind, NodeKind::Chance);

        let root_span = tree.storage_spans[0];
        assert_eq!(root_span.start, 0);
        assert_eq!(root_span.end, tree.storage_len);

        let child_ids: Vec<NodeId> = tree.children(0).collect();
        assert_eq!(child_ids.len(), 2);
        let child_spans: Vec<StorageSpan> = child_ids
            .iter()
            .map(|&id| tree.storage_spans[id as usize])
            .collect();

        // Each child span is non-empty (both branches are 2-action nodes
        // with 2 hands each).
        assert!(child_spans[0].start < child_spans[0].end);
        assert!(child_spans[1].start < child_spans[1].end);

        // Disjoint and ascending.
        assert!(child_spans[0].end <= child_spans[1].start);

        // Union covers the chance subtree's own span exactly.
        assert_eq!(child_spans[0].start, root_span.start);
        assert_eq!(child_spans[1].end, root_span.end);

        // Same tiling holds for the storage_refs index ranges.
        assert_eq!(root_span.sref_start, 0);
        assert_eq!(root_span.sref_end, tree.storage_refs.len() as u32);
        assert!(child_spans[0].sref_end <= child_spans[1].sref_start);
        assert_eq!(child_spans[0].sref_start, root_span.sref_start);
        assert_eq!(child_spans[1].sref_end, root_span.sref_end);
    }

    fn shape_of(tree: &PublicTree) -> TreeShape {
        TreeShape {
            nodes: tree
                .nodes
                .iter()
                .map(|n| ShapeNode {
                    kind: n.kind,
                    player: n.player,
                    num_children: n.num_children,
                })
                .collect(),
            terminal_ids: tree
                .nodes
                .iter()
                .filter(|n| n.kind == NodeKind::Terminal)
                .map(|n| n.aux)
                .collect(),
            deals: tree.deals.clone(),
            masks: tree.masks.clone(),
            transitions: tree.transitions.clone(),
            root_dims: tree.root_dims,
        }
    }

    /// Action root over a chance node whose branches mask one player and
    /// shrink the other through a transition, then act again.
    fn mixed_tree() -> PublicTree {
        let maps = |t| PerPlayer::new(ReachMap::Mask(0), ReachMap::Transition(t));
        let after = |id| {
            action(
                Player::P1,
                vec![terminal(id), action(Player::P0, vec![terminal(id + 1)])],
            )
        };
        let root = action(
            Player::P0,
            vec![
                terminal(7),
                TempNode::Chance {
                    deals: vec![(2.0, maps(0), after(10)), (1.0, maps(1), after(20))],
                    tag: 5,
                },
            ],
        );
        PublicTree::compile(TreeSpec {
            root,
            masks: vec![vec![1.0, 0.0, 1.0]],
            transitions: vec![
                SparseTransition {
                    in_dim: 2,
                    out_dim: 1,
                    entries: vec![(0, 0, 1.0), (1, 0, 0.5)],
                },
                SparseTransition {
                    in_dim: 2,
                    out_dim: 3,
                    entries: vec![(1, 2, 1.0)],
                },
            ],
            root_dims: PerPlayer::new(3, 2),
        })
    }

    #[test]
    fn from_shape_rebuilds_the_compiled_tree() {
        let tree = mixed_tree();
        let rebuilt = PublicTree::from_shape(shape_of(&tree)).unwrap();
        let key = |n: &Node| (n.kind, n.player, n.num_children, n.first_child, n.aux);
        assert_eq!(
            tree.nodes.iter().map(key).collect::<Vec<_>>(),
            rebuilt.nodes.iter().map(key).collect::<Vec<_>>()
        );
        let sref = |r: &StorageRef| (r.offset, r.num_actions, r.num_hands, r.index);
        assert_eq!(
            tree.storage_refs.iter().map(sref).collect::<Vec<_>>(),
            rebuilt.storage_refs.iter().map(sref).collect::<Vec<_>>()
        );
        assert_eq!(tree.storage_nodes, rebuilt.storage_nodes);
        for (index, &id) in tree.storage_nodes.iter().enumerate() {
            let node = tree.node(id);
            assert_eq!(node.kind, NodeKind::Action);
            assert_eq!(node.aux as usize, index);
        }
        let span = |s: &StorageSpan| (s.start, s.end, s.sref_start, s.sref_end);
        assert_eq!(
            tree.storage_spans.iter().map(span).collect::<Vec<_>>(),
            rebuilt.storage_spans.iter().map(span).collect::<Vec<_>>()
        );
        assert_eq!(tree.storage_len, rebuilt.storage_len);
        assert_eq!(tree.subtree_has_chance, rebuilt.subtree_has_chance);
        assert!(rebuilt.tags.iter().all(|&t| t == 0));
        // The second branch's transition grows P1's dimension to 3.
        let second = tree
            .children(tree.children(0).nth(1).unwrap())
            .nth(1)
            .unwrap();
        assert_eq!(rebuilt.storage_ref(rebuilt.node(second)).num_hands, 3);
    }

    #[test]
    fn from_shape_rejects_inconsistent_shapes() {
        let tree = mixed_tree();
        let reject = |edit: &dyn Fn(&mut TreeShape)| {
            let mut shape = shape_of(&tree);
            edit(&mut shape);
            assert!(PublicTree::from_shape(shape).is_err());
        };
        reject(&|s| s.nodes.push(s.nodes[1]));
        reject(&|s| {
            s.nodes.pop();
        });
        reject(&|s| {
            s.deals.pop();
        });
        reject(&|s| s.deals.push(s.deals[0]));
        reject(&|s| {
            s.terminal_ids.pop();
        });
        reject(&|s| s.terminal_ids.push(0));
        reject(&|s| s.masks[0].push(1.0));
        reject(&|s| s.transitions[0].in_dim = 3);
        reject(&|s| s.transitions[1].entries.push((2, 0, 1.0)));
        reject(&|s| s.deals[0].maps[Player::P0] = ReachMap::Mask(4));
        reject(&|s| s.nodes[0].num_children = 0);
    }
}
