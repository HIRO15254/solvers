use crate::{Player, PostflopConfig, index::GameIndex};
use anyhow::{Result, bail};
use hu_engine::{NodeKind, PublicTree};

/// Validated mapping from profiled nodes to base solution nodes.
pub(crate) struct BaseMap;

impl BaseMap {
    pub fn new(
        base: &crate::artifact::LoadedSol,
        tree: &PublicTree,
        index: &GameIndex,
        config: &PostflopConfig,
    ) -> Result<Self> {
        if base.game.tree.nodes.len() != tree.nodes.len()
            && config
                .profile_suit_perms
                .as_ref()
                .is_some_and(|perms| perms.len() < 24)
        {
            return Err(spot::SpotError::new(spot::Code::NLH003, "profile.freeze_upstream", "base tree differs from the profiled tree; suit-isomorphism restriction from profile selectors cannot be combined with freeze_upstream").into());
        }
        if base.config.board != config.board
            || base.config.pot != config.pot
            || base.config.effective_stack != config.effective_stack
            || base.game.tree.nodes.len() != tree.nodes.len()
            || Player::BOTH
                .into_iter()
                .any(|seat| base.game.index.hands.combos(seat) != index.hands.combos(seat))
        {
            bail!("NLH003: freeze_upstream solution does not match tree or hand support");
        }
        let view = crate::index::TreeView { tree, index };
        for (id, node) in tree.nodes.iter().enumerate() {
            let other = base.game.tree.node(id as u32);
            if node.kind != other.kind
                || node.player != other.player
                || node.num_children != other.num_children
                || (node.kind == NodeKind::Action
                    && !view
                        .action_labels(id as u32)
                        .eq(base.view().action_labels(id as u32)))
            {
                bail!("NLH003: freeze_upstream solution has a different tree");
            }
        }
        Ok(Self)
    }

    pub fn is_stored(&self, base: &crate::artifact::LoadedSol, node: u32) -> bool {
        base.is_stored(node)
    }

    pub fn strategy(
        &self,
        base: &crate::artifact::LoadedSol,
        node: u32,
    ) -> Result<Option<Vec<f32>>> {
        base.stored_strategy(node)
    }
}
