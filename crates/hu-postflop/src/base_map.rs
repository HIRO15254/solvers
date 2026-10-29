use crate::{Player, PostflopConfig, index::GameIndex};
use anyhow::{Result, bail};
use hu_engine::{NodeKind, PublicTree};
use nlh::iso::{Board, SuitPerm, all_suit_perms, permute_card, permute_combo, stabilizer};

#[derive(Clone, Copy, Default)]
struct Entry {
    node: u32,
    perm: u8,
}

/// Mapping from profiled nodes to base nodes. An empty vector is the identity.
/// Each non-identity entry occupies eight bytes per profiled node.
pub(crate) struct BaseMap {
    entries: Vec<Entry>,
}

impl BaseMap {
    pub fn new(
        base: &crate::artifact::LoadedSol,
        tree: &PublicTree,
        index: &GameIndex,
        config: &PostflopConfig,
    ) -> Result<Self> {
        if base.config.board != config.board
            || base.config.pot != config.pot
            || base.config.effective_stack != config.effective_stack
            || Player::BOTH.into_iter().any(|seat| {
                base.game.index.hands.combos(seat) != index.hands.combos(seat)
                    || base.game.index.root_ranges[seat] != index.root_ranges[seat]
            })
        {
            bail!("NLH003: freeze_upstream solution does not match tree or hand support");
        }
        let view = crate::index::TreeView { tree, index };
        let old = base.view();
        if base.game.tree.nodes.len() == tree.nodes.len()
            && tree.nodes.iter().enumerate().all(|(id, node)| {
                let other = base.game.tree.node(id as u32);
                node.kind == other.kind
                    && node.player == other.player
                    && node.num_children == other.num_children
                    && (node.kind != NodeKind::Action
                        || view
                            .action_labels(id as u32)
                            .eq(old.action_labels(id as u32)))
                    && (node.kind != NodeKind::Chance
                        || (0..node.num_children as usize).all(|pos| {
                            view.deal_card(id as u32, pos) == old.deal_card(id as u32, pos)
                                && tree.deal(node, pos).weight
                                    == base.game.tree.deal(other, pos).weight
                        }))
            })
        {
            return Ok(Self {
                entries: Vec::new(),
            });
        }
        let perms = all_suit_perms();
        let allowed: Vec<_> = perms
            .iter()
            .copied()
            .filter(|perm| {
                (base.config.iso_merging || *perm == [0, 1, 2, 3])
                    && base
                        .config
                        .profile_suit_perms
                        .as_ref()
                        .is_none_or(|p| p.contains(perm))
                    && (0..nlh::NUM_COMBOS).all(|combo| {
                        Player::BOTH.into_iter().all(|seat| {
                            base.config.ranges[seat].weight(combo)
                                == base.config.ranges[seat].weight(permute_combo(perm, combo))
                        })
                    })
            })
            .collect();
        let mut entries = vec![Entry::default(); tree.nodes.len()];
        let mut board = config.board.clone();
        let mut walker = Walker {
            base,
            tree,
            view,
            allowed: &allowed,
            perms: &perms,
            entries: &mut entries,
        };
        walker.walk(0, 0, [0, 1, 2, 3], &mut board)?;
        Ok(Self { entries })
    }

    pub fn is_stored(&self, base: &crate::artifact::LoadedSol, node: u32) -> bool {
        base.is_stored(self.base_node(node))
    }

    fn base_node(&self, node: u32) -> u32 {
        if self.entries.is_empty() {
            node
        } else {
            self.entries[node as usize].node
        }
    }

    pub fn strategy(
        &self,
        base: &crate::artifact::LoadedSol,
        node: u32,
    ) -> Result<Option<Vec<f32>>> {
        let base_node = self.base_node(node);
        let Some(strategy) = base.stored_strategy(base_node)? else {
            return Ok(None);
        };
        if self.entries.is_empty() || self.entries[node as usize].perm == 0 {
            return Ok(Some(strategy));
        }
        let perm = all_suit_perms()[self.entries[node as usize].perm as usize];
        let actor = base.game.tree.node(base_node).player;
        let hands = &base.game.index.hands;
        let combos = hands.combos(actor);
        let mut result = vec![0.0; strategy.len()];
        for (i, &combo) in combos.iter().enumerate() {
            let j = hands
                .local(actor, permute_combo(&perm, combo as usize))
                .expect("base symmetry preserves hand support");
            for a in 0..strategy.len() / combos.len() {
                result[a * combos.len() + i] = strategy[a * combos.len() + j];
            }
        }
        Ok(Some(result))
    }
}

struct Walker<'a> {
    base: &'a crate::artifact::LoadedSol,
    tree: &'a PublicTree,
    view: crate::index::TreeView<'a>,
    allowed: &'a [SuitPerm],
    perms: &'a [SuitPerm; 24],
    entries: &'a mut [Entry],
}

impl Walker<'_> {
    fn walk(
        &mut self,
        node: u32,
        base_node: u32,
        perm: SuitPerm,
        board: &mut Vec<nlh::Card>,
    ) -> Result<()> {
        let a = self.tree.node(node);
        let b = self.base.game.tree.node(base_node);
        if a.kind != b.kind || a.player != b.player {
            bail!("NLH003: freeze_upstream solution has a different tree");
        }
        self.entries[node as usize] = Entry {
            node: base_node,
            perm: self.perms.iter().position(|p| *p == perm).unwrap() as u8,
        };
        match a.kind {
            NodeKind::Terminal => {}
            NodeKind::Action => {
                if a.num_children != b.num_children
                    || !self
                        .view
                        .action_labels(node)
                        .eq(self.base.view().action_labels(base_node))
                {
                    bail!("NLH003: freeze_upstream solution has a different tree");
                }
                for pos in 0..a.num_children as u32 {
                    self.walk(a.first_child + pos, b.first_child + pos, perm, board)?;
                }
            }
            NodeKind::Chance => {
                let base_board: Vec<_> = board
                    .iter()
                    .map(|&card| permute_card(&perm, card))
                    .collect();
                let stab = stabilizer(&Board::new(&base_board[..3], &base_board[3..]));
                let stab: Vec<_> = stab
                    .into_iter()
                    .filter(|p| self.allowed.contains(p))
                    .collect();
                let old_view = self.base.view();
                let mut weights = vec![0.0f32; b.num_children as usize];
                let mut skipped_weight = 0.0f32;
                for pos in 0..a.num_children as usize {
                    let child = a.first_child + pos as u32;
                    let weight = self.tree.deal(a, pos).weight;
                    let Some(card) = self.view.deal_card(node, pos) else {
                        let span = self.tree.storage_spans[child as usize];
                        if span.sref_start != span.sref_end {
                            bail!("NLH003: freeze_upstream solution has a different tree");
                        }
                        skipped_weight += weight;
                        continue;
                    };
                    let mapped = permute_card(&perm, card);
                    let found = (0..b.num_children as usize).find_map(|j| {
                        let rep = old_view.deal_card(base_node, j)?;
                        stab.iter()
                            .find(|p| permute_card(p, mapped) == rep)
                            .map(|p| (j, *p))
                    });
                    let Some((j, next)) = found else {
                        bail!("NLH003: freeze_upstream solution has a different tree");
                    };
                    weights[j] += weight;
                    let composed = [
                        next[perm[0] as usize],
                        next[perm[1] as usize],
                        next[perm[2] as usize],
                        next[perm[3] as usize],
                    ];
                    board.push(card);
                    self.walk(child, b.first_child + j as u32, composed, board)?;
                    board.pop();
                }
                let mut old_skipped = 0.0f32;
                for (j, &weight) in weights.iter().enumerate() {
                    let expected = self.base.game.tree.deal(b, j).weight;
                    if old_view.deal_card(base_node, j).is_none() {
                        old_skipped += expected;
                    } else if (weight - expected).abs() > 1e-6 {
                        bail!("NLH003: freeze_upstream solution has a different tree");
                    }
                }
                if (skipped_weight - old_skipped).abs() > 1e-6 {
                    bail!("NLH003: freeze_upstream solution has a different tree");
                }
            }
        }
        Ok(())
    }
}
