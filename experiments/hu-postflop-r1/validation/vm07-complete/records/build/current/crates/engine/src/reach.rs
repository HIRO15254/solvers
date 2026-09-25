//! Poker-agnostic reach, path, and subtree-pairing utilities layered on top
//! of [`PublicTree`]. Everything here is query-layer machinery (see
//! `docs/architecture.md`'s "viewer is a query layer" principle): none of it
//! runs on the O(iterations x nodes x hands) hot path the solver walks, so
//! it favors plain `Vec` allocation and straightforward recursion over the
//! scratch-pool/view-splitting discipline in `solver.rs`. Two consumers
//! drive the design: an offline viewer reconstructing per-hand reach at an
//! arbitrary node for display, and river re-solve tooling (see
//! `holdem::viewer`) that needs a trunk subtree's entry reach to seed a
//! fresh subgame.

use cards::{PerPlayer, Player};

use crate::storage::StorageRef;
use crate::tree::{NodeId, NodeKind, PublicTree, ReachMap};

/// `parent[c]` is the parent node id of every node `c`; `parent[root]` is
/// the `NodeId::MAX` sentinel (the root has no parent). One linear pass over
/// every node's [`PublicTree::children`] range: `PublicTree` is a tree (no
/// shared children), so each non-root node is written exactly once, giving
/// O(nodes) total work.
pub fn parent_array(tree: &PublicTree) -> Vec<NodeId> {
    let mut parents = vec![NodeId::MAX; tree.nodes.len()];
    for id in 0..tree.nodes.len() as NodeId {
        for child in tree.children(id) {
            parents[child as usize] = id;
        }
    }
    parents
}

/// Node ids from the root (inclusive) to `target` (inclusive), obtained by
/// following `parents` upward from `target` and reversing. `parents` must be
/// a [`parent_array`] of the tree `target` belongs to (or anything sharing
/// its shape/sentinel convention).
pub fn path_from_root(parents: &[NodeId], target: NodeId) -> Vec<NodeId> {
    assert!(
        (target as usize) < parents.len(),
        "target node id {target} out of bounds for a {}-node parent array",
        parents.len()
    );
    let mut path = vec![target];
    let mut cur = target;
    while parents[cur as usize] != NodeId::MAX {
        cur = parents[cur as usize];
        path.push(cur);
    }
    path.reverse();
    path
}

/// Mapped dimension of one player's reach vector across a chance deal.
/// Mirrors `PublicTree`'s private `mapped_dim` (used at compile time to size
/// child storage): `Identity`/`Mask` never change the dimension; only a
/// `Transition` can, per [`ReachMap`]'s doc comment.
fn mapped_len(tree: &PublicTree, map: ReachMap, in_len: usize) -> usize {
    match map {
        ReachMap::Identity | ReachMap::Mask(_) => in_len,
        ReachMap::Transition(t) => tree.transitions[t as usize].out_dim as usize,
    }
}

/// Both players' per-hand reach vectors at `target`, obtained by walking
/// root -> target and folding in each action's strategy column or each
/// chance deal's reach map along the way.
///
/// At an `Action` node on the path: `avg_strategy` is called to fill an
/// `A*H` action-major buffer with the node's strategy (normalized average
/// strategy for a converged/converging solve, or current strategy for a
/// live one — the caller picks by choosing what `avg_strategy` reads), then
/// the *acting* player's reach is multiplied elementwise by the column for
/// the action actually taken on the path (`child_pos = next - node.first_child`,
/// valid because a node's children occupy a contiguous id range). This
/// mirrors exactly how `cfr_pass`/`value_pass` (see `crate::solver`) fold a
/// strategy column into a child's reach: the opponent's reach is untouched
/// at an action node, full stop, regardless of whose turn it is.
///
/// At a `Chance` node on the path: both players' reach vectors are passed
/// through [`PublicTree::map_reach_into`] for the taken deal (`Mask` zeroes
/// card-incompatible hands; `Transition` may additionally change the
/// vector's dimension, so the output buffer at each hop is sized via
/// [`mapped_len`], never assumed to match the input). The deal's scalar
/// `weight` is deliberately **excluded** from this fold: `weight` is a
/// single number shared by every hand pair crossing that branch (a folded
/// chance-probability / suit-isomorphism-class-size constant), so it cannot
/// distinguish one hand from another and therefore cannot affect a
/// range-vs-range subgame reconstruction; assigning it to one player's
/// reach rather than the other's — or splitting it between them — would be
/// an arbitrary modeling choice this function refuses to make. (`weight`
/// matters only when *aggregating* per-hand counterfactual values back up
/// through the tree, see `PublicTree::accumulate_values` — not what this
/// function does.)
pub fn reach_at<F>(
    tree: &PublicTree,
    root_ranges: PerPlayer<&[f32]>,
    target: NodeId,
    mut avg_strategy: F,
) -> PerPlayer<Vec<f32>>
where
    F: FnMut(NodeId, StorageRef, &mut [f32]),
{
    let parents = parent_array(tree);
    let path = path_from_root(&parents, target);

    let mut reach: PerPlayer<Vec<f32>> = PerPlayer::new(
        root_ranges[Player::P0].to_vec(),
        root_ranges[Player::P1].to_vec(),
    );

    for hop in path.windows(2) {
        let (current, next) = (hop[0], hop[1]);
        let node = *tree.node(current);
        let child_pos = (next - node.first_child) as usize;
        match node.kind {
            NodeKind::Action => {
                let sref = tree.storage_ref(&node);
                let mut sigma = vec![0.0f32; sref.len()];
                avg_strategy(current, sref, &mut sigma);
                let num_hands = sref.num_hands as usize;
                let column = &sigma[child_pos * num_hands..(child_pos + 1) * num_hands];
                let acting = &mut reach[node.player];
                debug_assert_eq!(acting.len(), num_hands, "reach/storage hand-count mismatch");
                for (r, &c) in acting.iter_mut().zip(column) {
                    *r *= c;
                }
            }
            NodeKind::Chance => {
                let deal = *tree.deal(&node, child_pos);
                for p in Player::BOTH {
                    let map = deal.maps[p];
                    let out_len = mapped_len(tree, map, reach[p].len());
                    let mut out = vec![0.0f32; out_len];
                    tree.map_reach_into(map, &reach[p], &mut out);
                    reach[p] = out;
                }
            }
            NodeKind::Terminal => unreachable!("a terminal node has no children to descend into"),
        }
    }

    reach
}

/// What differed between two nodes [`pair_subtrees`] was pairing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MismatchReason {
    /// The two nodes are different [`NodeKind`]s.
    Kind { a: NodeKind, b: NodeKind },
    /// Both are `Action` nodes but for different acting players.
    Player { a: Player, b: Player },
    /// Different number of children.
    NumChildren { a: u16, b: u16 },
    /// Both are `Action` nodes with the same child count, but their storage
    /// refs disagree on `num_actions` (should be impossible given
    /// `PublicTree::compile`'s invariant that `num_actions == num_children`
    /// for every `Action` node — checked anyway, cheaply, for defense).
    NumActions { a: u16, b: u16 },
}

/// First point of divergence found by [`pair_subtrees`]: the two node ids
/// being compared (one from each tree) and what differed about them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SubtreeMismatch {
    pub a: NodeId,
    pub b: NodeId,
    pub reason: MismatchReason,
}

impl std::fmt::Display for SubtreeMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.reason {
            MismatchReason::Kind { a, b } => write!(
                f,
                "subtree mismatch at nodes ({}, {}): kind {a:?} vs {b:?}",
                self.a, self.b
            ),
            MismatchReason::Player { a, b } => write!(
                f,
                "subtree mismatch at nodes ({}, {}): acting player {a:?} vs {b:?}",
                self.a, self.b
            ),
            MismatchReason::NumChildren { a, b } => write!(
                f,
                "subtree mismatch at nodes ({}, {}): {a} children vs {b} children",
                self.a, self.b
            ),
            MismatchReason::NumActions { a, b } => write!(
                f,
                "subtree mismatch at nodes ({}, {}): {a} actions vs {b} actions",
                self.a, self.b
            ),
        }
    }
}

impl std::error::Error for SubtreeMismatch {}

/// Pairs the node ids of two structurally identical subtrees in DFS
/// preorder: `(kind, acting player for Action nodes, num_children)` must
/// match at every visited pair, and for `Action` pairs the underlying
/// storage refs' `num_actions` must match too (see
/// [`MismatchReason::NumActions`]). Returns every matched pair (including
/// the two roots) on full agreement, or the first point of divergence as an
/// `Err`.
///
/// This is the guard-test primitive behind `holdem::viewer`'s river-resolve
/// contract: two subtrees built from unrelated configs (a trunk's river
/// entry vs. a from-scratch river-start build) are asserted structurally
/// identical this way, node for node, forever.
pub fn pair_subtrees(
    a: &PublicTree,
    a_root: NodeId,
    b: &PublicTree,
    b_root: NodeId,
) -> Result<Vec<(NodeId, NodeId)>, SubtreeMismatch> {
    let mut pairs = Vec::new();
    pair_subtrees_into(a, a_root, b, b_root, &mut pairs)?;
    Ok(pairs)
}

fn pair_subtrees_into(
    a: &PublicTree,
    na: NodeId,
    b: &PublicTree,
    nb: NodeId,
    pairs: &mut Vec<(NodeId, NodeId)>,
) -> Result<(), SubtreeMismatch> {
    let node_a = *a.node(na);
    let node_b = *b.node(nb);
    if node_a.kind != node_b.kind {
        return Err(SubtreeMismatch {
            a: na,
            b: nb,
            reason: MismatchReason::Kind {
                a: node_a.kind,
                b: node_b.kind,
            },
        });
    }
    if node_a.kind == NodeKind::Action && node_a.player != node_b.player {
        return Err(SubtreeMismatch {
            a: na,
            b: nb,
            reason: MismatchReason::Player {
                a: node_a.player,
                b: node_b.player,
            },
        });
    }
    if node_a.num_children != node_b.num_children {
        return Err(SubtreeMismatch {
            a: na,
            b: nb,
            reason: MismatchReason::NumChildren {
                a: node_a.num_children,
                b: node_b.num_children,
            },
        });
    }
    if node_a.kind == NodeKind::Action {
        let sref_a = a.storage_ref(&node_a);
        let sref_b = b.storage_ref(&node_b);
        if sref_a.num_actions != sref_b.num_actions {
            return Err(SubtreeMismatch {
                a: na,
                b: nb,
                reason: MismatchReason::NumActions {
                    a: sref_a.num_actions,
                    b: sref_b.num_actions,
                },
            });
        }
    }
    pairs.push((na, nb));
    for (ca, cb) in a.children(na).zip(b.children(nb)) {
        pair_subtrees_into(a, ca, b, cb, pairs)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{SparseTransition, TempNode, TreeSpec};

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

    /// Root chance node, two deals (identity maps) each into a 2-action P0
    /// node with two terminal children. Mirrors the fixture in
    /// `tree.rs::tests::spans_tile_across_chance_subtree`. Node ids, by
    /// construction order: 0 = root (chance), 1/2 = the two action nodes,
    /// 3/4 = terminals under action node 1, 5/6 = terminals under 2.
    fn two_level_tree() -> PublicTree {
        let branch = |base: u32| action(Player::P0, vec![terminal(base), terminal(base + 1)]);
        let identity = PerPlayer::new(ReachMap::Identity, ReachMap::Identity);
        let root = TempNode::Chance {
            deals: vec![(0.5, identity, branch(0)), (0.5, identity, branch(2))],
            tag: 0,
        };
        PublicTree::compile(TreeSpec {
            root,
            masks: Vec::new(),
            transitions: Vec::new(),
            root_dims: PerPlayer::new(2, 2),
        })
    }

    #[test]
    fn parent_array_and_path_on_two_level_tree() {
        let tree = two_level_tree();
        let parents = parent_array(&tree);

        assert_eq!(parents[0], NodeId::MAX);
        assert_eq!(parents[1], 0);
        assert_eq!(parents[2], 0);
        assert_eq!(parents[3], 1);
        assert_eq!(parents[4], 1);
        assert_eq!(parents[5], 2);
        assert_eq!(parents[6], 2);

        assert_eq!(path_from_root(&parents, 0), vec![0]);
        assert_eq!(path_from_root(&parents, 4), vec![0, 1, 4]);
        assert_eq!(path_from_root(&parents, 5), vec![0, 2, 5]);
    }

    #[test]
    fn reach_at_root_is_root_ranges() {
        let tree = two_level_tree();
        let p0 = vec![0.3f32, 0.7];
        let p1 = vec![1.0f32, 1.0];
        let reach = reach_at(
            &tree,
            PerPlayer::new(p0.as_slice(), p1.as_slice()),
            0,
            |_id, _sref, _out| unreachable!("root has no incoming edge to fold in"),
        );
        assert_eq!(reach[Player::P0], p0);
        assert_eq!(reach[Player::P1], p1);
    }

    #[test]
    fn reach_at_through_one_action_node_scales_acting_player_only() {
        // Root is itself a 2-action P0 node with two terminal children —
        // isolates the "one action hop" case from any chance-node fold.
        let root = action(Player::P0, vec![terminal(0), terminal(1)]);
        let tree = PublicTree::compile(TreeSpec {
            root,
            masks: Vec::new(),
            transitions: Vec::new(),
            root_dims: PerPlayer::new(2, 2),
        });

        let p0 = vec![0.4f32, 0.6];
        let p1 = vec![1.0f32, 1.0];
        // Fixed strategy, ignoring node id/storage ref: action 0's column is
        // [0.9, 0.1], action 1's is [0.1, 0.9] (action-major, 2 hands each).
        let reach = reach_at(
            &tree,
            PerPlayer::new(p0.as_slice(), p1.as_slice()),
            1, // first child = action 0's terminal
            |_id, _sref, out: &mut [f32]| {
                out.copy_from_slice(&[0.9, 0.1, 0.1, 0.9]);
            },
        );
        assert_eq!(reach[Player::P0], vec![0.4 * 0.9, 0.6 * 0.1]);
        // Opponent (P1) is never touched at an action node.
        assert_eq!(reach[Player::P1], p1);
    }

    #[test]
    fn reach_at_through_mask_deal_zeroes_masked_entries_for_both_players() {
        let mask = vec![1.0f32, 0.0, 1.0];
        let mask_maps = PerPlayer::new(ReachMap::Mask(0), ReachMap::Mask(0));
        let root = TempNode::Chance {
            deals: vec![(1.0, mask_maps, terminal(0))],
            tag: 0,
        };
        let tree = PublicTree::compile(TreeSpec {
            root,
            masks: vec![mask],
            transitions: Vec::new(),
            root_dims: PerPlayer::new(3, 3),
        });

        let p0 = vec![0.2f32, 0.5, 0.8];
        let p1 = vec![1.0f32, 1.0, 1.0];
        let reach = reach_at(
            &tree,
            PerPlayer::new(p0.as_slice(), p1.as_slice()),
            1,
            |_id, _sref, _out| unreachable!("no action node on this path"),
        );
        assert_eq!(reach[Player::P0], vec![0.2, 0.0, 0.8]);
        assert_eq!(reach[Player::P1], vec![1.0, 0.0, 1.0]);
    }

    #[test]
    fn reach_at_through_transition_deal_matches_apply_forward() {
        let transition = SparseTransition {
            in_dim: 2,
            out_dim: 3,
            entries: vec![(0, 0, 0.5), (0, 1, 0.25), (1, 2, 1.0)],
        };
        let maps = PerPlayer::new(ReachMap::Transition(0), ReachMap::Transition(0));
        let root = TempNode::Chance {
            deals: vec![(1.0, maps, terminal(0))],
            tag: 0,
        };
        let tree = PublicTree::compile(TreeSpec {
            root,
            masks: Vec::new(),
            transitions: vec![transition.clone()],
            root_dims: PerPlayer::new(2, 2),
        });

        let p0 = vec![0.3f32, 0.7];
        let p1 = vec![0.4f32, 0.6];
        let reach = reach_at(
            &tree,
            PerPlayer::new(p0.as_slice(), p1.as_slice()),
            1,
            |_id, _sref, _out| unreachable!("no action node on this path"),
        );

        let mut expected_p0 = vec![0.0f32; 3];
        transition.apply_forward(&p0, &mut expected_p0);
        let mut expected_p1 = vec![0.0f32; 3];
        transition.apply_forward(&p1, &mut expected_p1);

        assert_eq!(reach[Player::P0].len(), 3);
        assert_eq!(reach[Player::P0], expected_p0);
        assert_eq!(reach[Player::P1], expected_p1);
    }

    #[test]
    fn pair_subtrees_identical_trees_pair_fully() {
        let make = || action(Player::P0, vec![terminal(0), terminal(1)]);
        let tree_a = PublicTree::compile(TreeSpec {
            root: make(),
            masks: Vec::new(),
            transitions: Vec::new(),
            root_dims: PerPlayer::new(2, 2),
        });
        let tree_b = PublicTree::compile(TreeSpec {
            root: make(),
            masks: Vec::new(),
            transitions: Vec::new(),
            root_dims: PerPlayer::new(2, 2),
        });

        let pairs = pair_subtrees(&tree_a, 0, &tree_b, 0).expect("identical trees must pair");
        assert_eq!(pairs.len(), tree_a.nodes.len());
        assert_eq!(pairs.len(), 3); // root + 2 terminals
        assert_eq!(pairs[0], (0, 0));
    }

    #[test]
    fn pair_subtrees_extra_child_errs_with_useful_message() {
        let tree_a = PublicTree::compile(TreeSpec {
            root: action(Player::P0, vec![terminal(0), terminal(1)]),
            masks: Vec::new(),
            transitions: Vec::new(),
            root_dims: PerPlayer::new(2, 2),
        });
        let tree_b = PublicTree::compile(TreeSpec {
            root: action(Player::P0, vec![terminal(0), terminal(1), terminal(2)]),
            masks: Vec::new(),
            transitions: Vec::new(),
            root_dims: PerPlayer::new(2, 2),
        });

        let err = pair_subtrees(&tree_a, 0, &tree_b, 0).expect_err("child counts differ");
        assert_eq!(err.a, 0);
        assert_eq!(err.b, 0);
        assert_eq!(err.reason, MismatchReason::NumChildren { a: 2, b: 3 });
        let msg = err.to_string();
        assert!(
            msg.contains('2') && msg.contains('3'),
            "message should mention both child counts: {msg}"
        );
    }
}
