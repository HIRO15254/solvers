use cards::{PerPlayer, Player};

use crate::storage::StorageRef;

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

impl PublicTree {
    pub fn compile(spec: TreeSpec) -> PublicTree {
        let mut tree = PublicTree {
            nodes: Vec::new(),
            deals: Vec::new(),
            storage_refs: Vec::new(),
            masks: spec.masks,
            transitions: spec.transitions,
            root_dims: spec.root_dims,
            storage_len: 0,
            tags: Vec::new(),
        };
        tree.nodes.push(placeholder_node());
        tree.tags.push(0);
        let root_dims = tree.root_dims;
        tree.fill(0, &spec.root, root_dims);
        tree
    }

    /// Recursively fills `slot` from `temp`, reserving contiguous child
    /// blocks. `dims` is the per-player private-state dimension on the path.
    fn fill(&mut self, slot: usize, temp: &TempNode, dims: PerPlayer<u32>) {
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
                let storage_ref = StorageRef {
                    offset: self.storage_len,
                    num_actions: num_actions as u16,
                    num_hands: num_hands as u32,
                };
                self.storage_len += num_actions * num_hands;
                let aux = self.storage_refs.len() as u32;
                self.storage_refs.push(storage_ref);

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
    }

    fn reserve_children(&mut self, n: usize) -> u32 {
        let first = self.nodes.len() as u32;
        self.nodes.extend((0..n).map(|_| placeholder_node()));
        self.tags.extend((0..n).map(|_| 0));
        first
    }

    fn mapped_dim(&self, map: ReachMap, dim: u32) -> u32 {
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

    /// Applies one player's reach map for a deal. Returns the child-space
    /// reach vector (dimension may change under a transition).
    pub fn map_reach(&self, map: ReachMap, reach: &[f32]) -> Vec<f32> {
        match map {
            ReachMap::Identity => reach.to_vec(),
            ReachMap::Mask(m) => {
                let mask = &self.masks[m as usize];
                reach.iter().zip(mask).map(|(&r, &m)| r * m).collect()
            }
            ReachMap::Transition(t) => {
                let tr = &self.transitions[t as usize];
                let mut out = vec![0.0; tr.out_dim as usize];
                tr.apply_forward(reach, &mut out);
                out
            }
        }
    }

    /// Pulls child-space values back to parent space for a deal branch and
    /// accumulates them into `acc` with the given weight. For masks this also
    /// zeroes values on hands incompatible with the deal, which is exactly
    /// the card-removal semantics the counterfactual sum requires.
    pub fn accumulate_values(&self, map: ReachMap, weight: f32, child: &[f32], acc: &mut [f32]) {
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
                let mut back = vec![0.0; tr.in_dim as usize];
                tr.apply_backward(child, &mut back);
                for (a, &v) in acc.iter_mut().zip(&back) {
                    *a += weight * v;
                }
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
