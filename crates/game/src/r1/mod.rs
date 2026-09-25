//! Bounded R1 Stud/Draw prototypes, not production variant adapters.
//!
//! Rules, observations and utility baking live entirely on the build/query
//! side. The compiled games use the unchanged HU vector engine. See
//! `docs/plans/r1-common-game-boundary.jp.md` for the exact artificial rules.

mod settlement;

pub use settlement::{Award, Pot, Settled, Settlement, settle};

use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

use cards::{PerPlayer, Player};
use engine::{
    CompiledGame, PublicTree, ReachMap, SparseTransition, TempNode, TerminalEvaluator, TreeSpec,
};

use crate::UtilityModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundaryError {
    RecallLoss,
    HiddenExchangeAction,
    InvalidSettlement,
    ArithmeticOverflow,
    NonFiniteUtility,
}

impl fmt::Display for BoundaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::RecallLoss => "exact Draw lowering must retain discarded-card history",
            Self::HiddenExchangeAction => "HU public tree cannot expose a hidden exchange action",
            Self::InvalidSettlement => {
                "inconsistent pot, contribution, return or winner eligibility"
            }
            Self::ArithmeticOverflow => "settlement chip arithmetic overflow",
            Self::NonFiniteUtility => "utility or its baseline-relative difference is not finite",
        })
    }
}

impl std::error::Error for BoundaryError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    InitialDeal,
    OwnedPublicDeal,
    Exchange,
    PrivateReplacement,
    Betting,
    Settlement,
}

/// The history, not merely the current showdown card, indexes the vector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PrivateHistory {
    Initial(u8),
    Replaced { discarded: u8, replacement: u8 },
}

impl PrivateHistory {
    pub fn current(self) -> u8 {
        match self {
            Self::Initial(card) => card,
            Self::Replaced { replacement, .. } => replacement,
        }
    }

    fn original(self) -> u8 {
        match self {
            Self::Initial(card) => card,
            Self::Replaced { discarded, .. } => discarded,
        }
    }

    fn key(self) -> String {
        match self {
            Self::Initial(card) => format!("h{card}"),
            Self::Replaced {
                discarded,
                replacement,
            } => format!("h{discarded}>{replacement}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicObservation {
    Stud {
        upcards: PerPlayer<u8>,
    },
    /// `None` is the exchange decision, not an unknown replacement card.
    Draw {
        exchanged: Option<bool>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Check,
    Bet,
    Fold,
    Call,
    Keep,
    ReplaceOne,
}

#[derive(Clone, Debug)]
pub struct Decision {
    pub actor: Player,
    pub phase: Phase,
    pub public: PublicObservation,
    pub betting_history: String,
    pub actions: [Action; 2],
    pub private_states: PerPlayer<Arc<[PrivateHistory]>>,
}

impl Decision {
    /// Stable artificial-game observation key. No other seat's private
    /// state or future card is accepted by this query interface.
    pub fn information_key(&self, private_index: usize) -> Option<String> {
        let private = self.private_states[self.actor].get(private_index)?.key();
        let public = match self.public {
            PublicObservation::Stud { upcards } => {
                format!("S:{},{}", upcards[Player::P0], upcards[Player::P1])
            }
            PublicObservation::Draw { exchanged } => match exchanged {
                None => "D:-".into(),
                Some(false) => "D:K".into(),
                Some(true) => "D:R".into(),
            },
        };
        Some(format!(
            "{public}|P{}|{private}|{}",
            self.actor.index(),
            self.betting_history
        ))
    }
}

/// These flags describe alternate information structures that the public
/// tree adapter must refuse, rather than silently solving a different game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawRules {
    pub remember_discard: bool,
    pub exchange_action_public: bool,
}

impl Default for DrawRules {
    fn default() -> Self {
        Self {
            remember_discard: true,
            exchange_action_public: true,
        }
    }
}

pub struct Prototype {
    pub game: CompiledGame<MatrixEvaluator>,
    /// Indexed by Action-node tags; chance/terminal tags are not decisions.
    pub decisions: Vec<Decision>,
    pub phases: Vec<Phase>,
}

struct MatrixTerminal {
    dims: PerPlayer<usize>,
    /// Both matrices are P0-major; eval transposes access for P1.
    utilities: PerPlayer<Vec<f32>>,
    compatible: Vec<bool>,
}

/// Small dense evaluator for the prototypes. No rule or utility callbacks
/// survive compilation, and no NLHE kernel is changed.
pub struct MatrixEvaluator {
    terminals: Vec<MatrixTerminal>,
}

impl MatrixEvaluator {
    pub fn payoff(&self, terminal: u32, p: Player, p0: usize, p1: usize) -> Option<f32> {
        let terminal = self.terminals.get(terminal as usize)?;
        if p0 >= terminal.dims[Player::P0] || p1 >= terminal.dims[Player::P1] {
            return None;
        }
        let index = p0 * terminal.dims[Player::P1] + p1;
        terminal.compatible[index].then_some(terminal.utilities[p][index])
    }
}

impl TerminalEvaluator for MatrixEvaluator {
    fn eval(&self, terminal: u32, p: Player, opp_reach: &[f32], out: &mut [f32]) {
        let terminal = &self.terminals[terminal as usize];
        assert_eq!(out.len(), terminal.dims[p]);
        assert_eq!(opp_reach.len(), terminal.dims[p.opponent()]);
        let width = terminal.dims[Player::P1];
        for (h, value) in out.iter_mut().enumerate() {
            *value = opp_reach
                .iter()
                .enumerate()
                .map(|(o, reach)| {
                    let index = match p {
                        Player::P0 => h * width + o,
                        Player::P1 => o * width + h,
                    };
                    terminal.utilities[p][index] * reach
                })
                .sum();
        }
    }
}

type PrivateSpace = PerPlayer<Arc<[PrivateHistory]>>;

fn initial_space(deck: u8) -> PrivateSpace {
    let states: Arc<[PrivateHistory]> = (0..deck).map(PrivateHistory::Initial).collect();
    PerPlayer::new(states.clone(), states)
}

struct Builder<'a> {
    utility: &'a dyn UtilityModel,
    terminals: Vec<MatrixTerminal>,
    decisions: Vec<Decision>,
}

#[derive(Clone)]
struct Betting {
    public: PublicObservation,
    private: PrivateSpace,
    actor: Player,
    history: String,
    contributions: PerPlayer<u32>,
    checked: bool,
    facing: bool,
}

impl Betting {
    fn new(public: PublicObservation, private: PrivateSpace, actor: Player) -> Self {
        Self {
            public,
            private,
            actor,
            history: String::new(),
            contributions: PerPlayer::new(1, 1),
            checked: false,
            facing: false,
        }
    }
}

impl Builder<'_> {
    fn action(&mut self, decision: Decision, children: Vec<TempNode>) -> TempNode {
        let player = decision.actor;
        let tag = self.decisions.len() as u32;
        self.decisions.push(decision);
        TempNode::Action {
            player,
            children,
            tag,
        }
    }

    fn betting(&mut self, state: Betting) -> Result<TempNode, BoundaryError> {
        let mut first = state.clone();
        let mut second = state.clone();
        let (actions, children) = if state.facing {
            first.history.push('f');
            second.history.push('k');
            second.contributions[state.actor] += 1;
            (
                [Action::Fold, Action::Call],
                vec![
                    self.terminal(&first, Some(state.actor))?,
                    self.terminal(&second, None)?,
                ],
            )
        } else {
            first.history.push('c');
            first.actor = state.actor.opponent();
            first.checked = true;
            second.history.push('b');
            second.actor = state.actor.opponent();
            second.facing = true;
            second.contributions[state.actor] += 1;
            let check = if state.checked {
                self.terminal(&first, None)?
            } else {
                self.betting(first)?
            };
            (
                [Action::Check, Action::Bet],
                vec![check, self.betting(second)?],
            )
        };
        Ok(self.action(
            Decision {
                actor: state.actor,
                phase: Phase::Betting,
                public: state.public,
                betting_history: state.history,
                actions,
                private_states: state.private,
            },
            children,
        ))
    }

    fn terminal(
        &mut self,
        state: &Betting,
        folder: Option<Player>,
    ) -> Result<TempNode, BoundaryError> {
        let dims = state.private.as_ref().map(|space| space.len());
        let length = dims[Player::P0] * dims[Player::P1];
        let mut utilities = PerPlayer::new(vec![0.0; length], vec![0.0; length]);
        let mut compatible = vec![false; length];
        for (h0, &p0) in state.private[Player::P0].iter().enumerate() {
            for (h1, &p1) in state.private[Player::P1].iter().enumerate() {
                let Some(order) = showdown(state.public, p0, p1) else {
                    continue;
                };
                let winner = match folder {
                    Some(Player::P0) => Award::P1,
                    Some(Player::P1) => Award::P0,
                    None => match order {
                        Ordering::Greater => Award::P0,
                        Ordering::Equal => Award::Both,
                        Ordering::Less => Award::P1,
                    },
                };
                let settled = settle(
                    &Settlement {
                        stacks_before: PerPlayer::new(8, 8),
                        contributions: state.contributions,
                        returned: PerPlayer::new(0, 0),
                        dead_money: 0,
                        pots: vec![Pot {
                            amount: state.contributions.0.iter().sum(),
                            rake: 0,
                            eligible: PerPlayer::new(
                                folder != Some(Player::P0),
                                folder != Some(Player::P1),
                            ),
                            high: winner,
                            low: None,
                        }],
                    },
                    self.utility,
                )?;
                let index = h0 * dims[Player::P1] + h1;
                compatible[index] = true;
                for p in Player::BOTH {
                    let value = settled.utility[p] as f32;
                    if !value.is_finite() {
                        return Err(BoundaryError::NonFiniteUtility);
                    }
                    utilities[p][index] = value;
                }
            }
        }
        let id = self.terminals.len() as u32;
        self.terminals.push(MatrixTerminal {
            dims,
            utilities,
            compatible,
        });
        Ok(TempNode::Terminal { id, tag: u32::MAX })
    }

    fn finish(self, spec: TreeSpec, normalizer: f64, phases: Vec<Phase>) -> Prototype {
        let root_ranges = spec.root_dims.map(|dim| vec![1.0; dim as usize]);
        Prototype {
            game: CompiledGame {
                tree: PublicTree::compile(spec),
                evaluator: MatrixEvaluator {
                    terminals: self.terminals,
                },
                root_ranges,
                normalizer,
                zero_sum: self.utility.is_zero_sum_affine(),
            },
            decisions: self.decisions,
            phases,
        }
    }
}

fn showdown(public: PublicObservation, p0: PrivateHistory, p1: PrivateHistory) -> Option<Ordering> {
    if p0.original() == p1.original() || p0.current() == p1.current() {
        return None;
    }
    match public {
        PublicObservation::Stud { upcards } => {
            if upcards.0.contains(&p0.current()) || upcards.0.contains(&p1.current()) {
                return None;
            }
            Some((p0.current() + upcards[Player::P0]).cmp(&(p1.current() + upcards[Player::P1])))
        }
        PublicObservation::Draw { .. } => Some(p0.current().cmp(&p1.current())),
    }
}

/// R1-STUD-01: six distinct ranks, private downcards, owned public
/// upcards, higher upcard acts first, sum-of-ranks showdown.
pub fn stud(utility: &dyn UtilityModel) -> Result<Prototype, BoundaryError> {
    let mut builder = Builder {
        utility,
        terminals: Vec::new(),
        decisions: Vec::new(),
    };
    let private = initial_space(6);
    let mut masks = Vec::new();
    let mut deals = Vec::new();
    for up0 in 0..6 {
        for up1 in 0..6 {
            if up0 == up1 {
                continue;
            }
            let map = ReachMap::Mask(masks.len() as u32);
            masks.push(
                (0..6)
                    .map(|card| if card == up0 || card == up1 { 0.0 } else { 1.0 })
                    .collect(),
            );
            let public = PublicObservation::Stud {
                upcards: PerPlayer::new(up0, up1),
            };
            let actor = if up0 > up1 { Player::P0 } else { Player::P1 };
            let child = builder.betting(Betting::new(public, private.clone(), actor))?;
            deals.push((1.0 / 12.0, PerPlayer::new(map, map), child));
        }
    }
    Ok(builder.finish(
        TreeSpec {
            root: TempNode::Chance {
                deals,
                tag: u32::MAX,
            },
            masks,
            transitions: Vec::new(),
            root_dims: PerPlayer::new(6, 6),
        },
        30.0,
        vec![
            Phase::InitialDeal,
            Phase::OwnedPublicDeal,
            Phase::Betting,
            Phase::Settlement,
        ],
    ))
}

/// R1-DRAW-01: public keep/replace-one decision, hidden replacement,
/// discarded card retained in P0's private history, then P1 acts first.
pub fn draw(rules: DrawRules, utility: &dyn UtilityModel) -> Result<Prototype, BoundaryError> {
    if !rules.remember_discard {
        return Err(BoundaryError::RecallLoss);
    }
    if !rules.exchange_action_public {
        return Err(BoundaryError::HiddenExchangeAction);
    }
    let mut builder = Builder {
        utility,
        terminals: Vec::new(),
        decisions: Vec::new(),
    };
    let private = initial_space(5);
    let mut histories = Vec::new();
    let mut entries = Vec::new();
    for discarded in 0..5 {
        for replacement in 0..5 {
            if discarded != replacement {
                entries.push((u32::from(discarded), histories.len() as u32, 1.0 / 3.0));
                histories.push(PrivateHistory::Replaced {
                    discarded,
                    replacement,
                });
            }
        }
    }
    let keep = builder.betting(Betting::new(
        PublicObservation::Draw {
            exchanged: Some(false),
        },
        private.clone(),
        Player::P1,
    ))?;
    let replacement = builder.betting(Betting::new(
        PublicObservation::Draw {
            exchanged: Some(true),
        },
        PerPlayer::new(histories.into(), private[Player::P1].clone()),
        Player::P1,
    ))?;
    let root = builder.action(
        Decision {
            actor: Player::P0,
            phase: Phase::Exchange,
            public: PublicObservation::Draw { exchanged: None },
            betting_history: String::new(),
            actions: [Action::Keep, Action::ReplaceOne],
            private_states: private,
        },
        vec![
            keep,
            TempNode::Chance {
                deals: vec![(
                    1.0,
                    PerPlayer::new(ReachMap::Transition(0), ReachMap::Identity),
                    replacement,
                )],
                tag: u32::MAX,
            },
        ],
    );
    Ok(builder.finish(
        TreeSpec {
            root,
            masks: Vec::new(),
            transitions: vec![SparseTransition {
                in_dim: 5,
                out_dim: 20,
                entries,
            }],
            root_dims: PerPlayer::new(5, 5),
        },
        20.0,
        vec![
            Phase::InitialDeal,
            Phase::Exchange,
            Phase::PrivateReplacement,
            Phase::Betting,
            Phase::Settlement,
        ],
    ))
}
