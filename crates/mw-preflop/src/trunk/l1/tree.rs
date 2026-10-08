use crate::{
    BettingState, ExternalSamplingGame, FeatureHashAbstraction, HoldemGame, SeatVec, Street,
    betting::HandPhase,
};
use anyhow::{Result, ensure};

pub struct Subtree {
    pub active: [usize; 2],
    pub nodes: Vec<Node>,
}

pub struct Node {
    pub parent: Option<(usize, usize)>,
    pub actor: Option<usize>,
    pub street: Street,
    pub labels: Vec<String>,
    pub children: Vec<usize>,
    /// Whole-table chip utilities: one fold vector or win/tie/loss vectors.
    pub payoffs: Vec<Vec<f64>>,
}

impl Subtree {
    pub(crate) fn build(
        game: &HoldemGame<FeatureHashAbstraction>,
        root: BettingState,
        active: [usize; 2],
    ) -> Result<Self> {
        let mut tree = Self {
            active,
            nodes: Vec::new(),
        };
        tree.visit(game, root, None)?;
        let reference = tree
            .nodes
            .iter()
            .find(|n| n.actor.is_none())
            .unwrap()
            .payoffs[0]
            .clone();
        for node in &tree.nodes {
            for u in &node.payoffs {
                for s in 0..game.num_players() {
                    if !active.contains(&s) {
                        ensure!(
                            u[s].to_bits() == reference[s].to_bits(),
                            "folded payoff changes in L1 subtree"
                        );
                    }
                }
            }
        }
        Ok(tree)
    }

    fn visit(
        &mut self,
        game: &HoldemGame<FeatureHashAbstraction>,
        state: BettingState,
        parent: Option<(usize, usize)>,
    ) -> Result<usize> {
        let z = self.nodes.len();
        let actor = if state.phase == HandPhase::Betting {
            let seat = game
                .actor(&state)
                .ok_or_else(|| anyhow::anyhow!("postflop decision without actor"))?;
            Some(
                self.active
                    .iter()
                    .position(|&s| s == seat)
                    .ok_or_else(|| anyhow::anyhow!("postflop actor outside active pair"))?,
            )
        } else {
            None
        };
        let actions = if actor.is_some() {
            game.node_actions(&state)
        } else {
            Vec::new()
        };
        ensure!(
            actor.is_none() || !actions.is_empty(),
            "postflop decision without actions"
        );
        let payoffs = if actor.is_some() {
            Vec::new()
        } else if matches!(state.phase, HandPhase::Uncontested { .. }) {
            vec![game.l0_uncontested_utilities(&state)?]
        } else {
            [[2, 1], [1, 1], [1, 2]]
                .iter()
                .map(|r| {
                    let ranks = SeatVec::try_new(
                        (0..game.num_players())
                            .map(|s| self.active.iter().position(|&p| p == s).map(|i| r[i]))
                            .collect(),
                    )?;
                    game.l0_ranked_utilities(&state, &ranks)
                })
                .collect::<Result<_>>()?
        };
        self.nodes.push(Node {
            parent,
            actor,
            street: state.street,
            labels: actions
                .iter()
                .map(HoldemGame::<FeatureHashAbstraction>::action_label)
                .collect(),
            children: Vec::new(),
            payoffs,
        });
        for a in 0..actions.len() {
            let child = self.visit(
                game,
                game.next_state_with(&state, &actions, a),
                Some((z, a)),
            )?;
            self.nodes[z].children.push(child);
        }
        Ok(z)
    }
}
