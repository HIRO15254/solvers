use anyhow::{Result, ensure};
use rustc_hash::FxHashMap;
use serde::Serialize;
use std::sync::RwLock;

use crate::trunk::classes::Ordering3;
use crate::{BettingState, ExternalSamplingGame, FeatureHashAbstraction, HistoryKey, HoldemGame};
use crate::{SeatId, SeatStatus, SeatVec, Street, betting::HandPhase};

/// DFS arena; every parent precedes its children. Ordering slots use ascending
/// active seats, independently of the hero evaluating the terminal.
pub struct Tree {
    pub nodes: Vec<Node>,
    pub seats: usize,
    pub game_fingerprint: [u8; 32],
}

pub struct Node {
    pub parent: Option<(usize, usize)>,
    pub history: HistoryKey,
    pub actor: Option<usize>,
    pub labels: Vec<String>,
    pub children: Vec<usize>,
    pub terminal: Option<Terminal>,
}

pub struct Terminal {
    pub active: Vec<usize>,
    pub state: BettingState,
    /// One uncontested vector, three two-way vectors, or thirteen three-way
    /// vectors. Larger showdowns use the lazily filled ordering cache.
    pub payoffs: Vec<Vec<f64>>,
    pub(crate) cache: RwLock<FxHashMap<u64, [f64; 9]>>,
}

impl Terminal {
    pub(crate) fn ranked(
        &self,
        game: &HoldemGame<FeatureHashAbstraction>,
        ranks: &[u16],
    ) -> Result<Vec<f64>> {
        let ranks = SeatVec::try_new(
            (0..self.state.seats.len())
                .map(|i| self.active.iter().position(|&a| a == i).map(|a| ranks[a]))
                .collect(),
        )?;
        game.l0_ranked_utilities(&self.state, &ranks)
    }

    /// Translate hero-oriented T2/T3 ranks to the stored ascending-seat order.
    pub fn hero_payoffs(&self, hero: usize) -> Vec<f64> {
        let others: Vec<_> = self.active.iter().copied().filter(|&s| s != hero).collect();
        if self.active.len() == 2 {
            [[2, 1], [1, 1], [1, 2]]
                .iter()
                .map(|r| {
                    let stored: Vec<_> = self
                        .active
                        .iter()
                        .map(|&s| r[usize::from(s != hero)])
                        .collect();
                    let slot = if stored[0] > stored[1] {
                        0
                    } else if stored[0] == stored[1] {
                        1
                    } else {
                        2
                    };
                    self.payoffs[slot][hero]
                })
                .collect()
        } else {
            Ordering3::ALL
                .iter()
                .map(|o| {
                    let r = o.ranks();
                    let stored: Vec<_> = self
                        .active
                        .iter()
                        .map(|&s| {
                            r[if s == hero {
                                0
                            } else if s == others[0] {
                                1
                            } else {
                                2
                            }]
                        })
                        .collect();
                    self.payoffs[Ordering3::from_ranks(stored[0], stored[1], stored[2]).index()]
                        [hero]
                })
                .collect()
        }
    }
}

impl Tree {
    pub fn build(game: &HoldemGame<FeatureHashAbstraction>) -> Result<Self> {
        Self::build_root(game, game.root_state())
    }

    pub(super) fn build_root(
        game: &HoldemGame<FeatureHashAbstraction>,
        root: BettingState,
    ) -> Result<Self> {
        game.require_l0_chip_ev()?;
        let mut tree = Self {
            nodes: Vec::new(),
            seats: game.num_players(),
            game_fingerprint: game.game_fingerprint(),
        };
        tree.visit(game, root, HistoryKey::ROOT, None)?;
        Ok(tree)
    }

    fn visit(
        &mut self,
        game: &HoldemGame<FeatureHashAbstraction>,
        state: BettingState,
        history: HistoryKey,
        parent: Option<(usize, usize)>,
    ) -> Result<usize> {
        let index = self.nodes.len();
        let is_terminal = state.phase != HandPhase::Betting;
        let actor = if is_terminal {
            None
        } else {
            game.actor(&state)
        };
        ensure!(
            is_terminal || actor.is_some(),
            "non-terminal state without an actor at {history:?}"
        );
        ensure!(
            actor.is_none() || state.street == Street::Preflop,
            "reachable postflop decision on {:?} at {history:?}",
            state.street
        );
        let actions = if actor.is_some() {
            game.node_actions(&state)
        } else {
            Vec::new()
        };
        ensure!(
            actor.is_none() || !actions.is_empty(),
            "decision without legal actions"
        );
        let terminal = if is_terminal {
            let active: Vec<_> = (0..self.seats)
                .filter(|&i| state.seats[SeatId(i as u8)].status != SeatStatus::Folded)
                .collect();
            let mut t = Terminal {
                active,
                state: state.clone(),
                payoffs: Vec::new(),
                cache: RwLock::new(FxHashMap::default()),
            };
            if matches!(state.phase, HandPhase::Uncontested { .. }) {
                t.payoffs.push(game.l0_uncontested_utilities(&state)?);
            } else {
                t.payoffs = match t.active.len() {
                    2 => [[2, 1], [1, 1], [1, 2]]
                        .iter()
                        .map(|r| t.ranked(game, r))
                        .collect::<Result<_>>()?,
                    3 => Ordering3::ALL
                        .iter()
                        .map(|o| t.ranked(game, &o.ranks()))
                        .collect::<Result<_>>()?,
                    k => vec![t.ranked(game, &vec![1; k])?],
                };
                // Chip EV of a folded player is independent of every showdown ordering.
                for i in 0..self.seats {
                    if !t.active.contains(&i) {
                        ensure!(
                            t.payoffs.iter().all(|u| u[i] == t.payoffs[0][i]),
                            "folded payoff depends on ordering"
                        );
                    }
                }
                if t.active.len() >= 4 {
                    // Check folded utilities before sampling, covering every
                    // possible sole winner and opposite strict rank orders.
                    let mut orders: Vec<Vec<u16>> = (0..t.active.len())
                        .map(|winner| {
                            (0..t.active.len())
                                .map(|i| if i == winner { 2 } else { 1 })
                                .collect()
                        })
                        .collect();
                    orders.push((1..=t.active.len() as u16).collect());
                    orders.push((1..=t.active.len() as u16).rev().collect());
                    for order in orders {
                        let u = t.ranked(game, &order)?;
                        for (i, &utility) in u.iter().enumerate() {
                            if !t.active.contains(&i) {
                                ensure!(
                                    utility == t.payoffs[0][i],
                                    "folded payoff depends on ordering"
                                );
                            }
                        }
                    }
                }
            }
            Some(t)
        } else {
            None
        };
        self.nodes.push(Node {
            parent,
            history,
            actor,
            labels: actions
                .iter()
                .map(HoldemGame::<FeatureHashAbstraction>::action_label)
                .collect(),
            children: Vec::new(),
            terminal,
        });
        if let Some(actor) = actor {
            for a in 0..actions.len() {
                let child = self.visit(
                    game,
                    game.next_state_with(&state, &actions, a),
                    history.child(actor, a),
                    Some((index, a)),
                )?;
                self.nodes[index].children.push(child);
            }
        }
        Ok(index)
    }

    pub fn path(&self, mut node: usize) -> Vec<String> {
        let mut path = Vec::new();
        while let Some((parent, action)) = self.nodes[node].parent {
            path.push(self.nodes[parent].labels[action].clone());
            node = parent;
        }
        path.reverse();
        path
    }

    pub fn export(&self) -> TreeExport {
        TreeExport {
            format: "p2-l0-tree",
            version: 1,
            nodes: self
                .nodes
                .iter()
                .enumerate()
                .filter_map(|(i, n)| {
                    n.actor.map(|actor| DecisionExport {
                        path: self.path(i),
                        actor,
                        actions: n.labels.clone(),
                    })
                })
                .collect(),
        }
    }

    pub fn terminal_counts(&self) -> Vec<usize> {
        let mut counts = vec![0; self.seats + 1];
        for t in self.nodes.iter().filter_map(|n| n.terminal.as_ref()) {
            counts[t.active.len()] += 1;
        }
        counts
    }
}

#[derive(Serialize)]
pub struct TreeExport {
    pub format: &'static str,
    pub version: u32,
    pub nodes: Vec<DecisionExport>,
}

#[derive(Serialize)]
pub struct DecisionExport {
    pub path: Vec<String>,
    pub actor: usize,
    pub actions: Vec<String>,
}
