//! Scalar Kuhn and Leduc, written independently of the `game` crate's toy
//! builder (sharing code would defeat the oracle's purpose). Histories and
//! action orderings deliberately match the toy builder's conventions —
//! `c`/`b`/`f`/`k`/`r` plus `[n]` for board cards — so infoset keys line up
//! for differential tests.

use crate::RefGame;

/// Limit-betting state shared by both toy games.
#[derive(Clone, Debug)]
pub struct LimitState {
    pub cards: [u8; 2],
    pub board: Option<u8>,
    pub round: usize,
    pub history: String,
    pub contrib: [f64; 2],
    pub to_act: usize,
    pub outstanding: f64,
    pub raises_used: u32,
    pub first_checked: bool,
    pub finished: Option<Finish>,
    /// Set when round 1 ended and the board card has not been dealt yet.
    pub awaiting_board: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum Finish {
    Fold { folder: usize },
    Showdown,
}

pub struct LimitRules {
    pub deck: usize,
    pub num_rounds: usize,
    pub bets: [f64; 2],
    pub max_raises: u32,
    pub board_before_round: usize, // usize::MAX for "never"
}

impl LimitRules {
    fn actions(&self, s: &LimitState) -> Vec<char> {
        if s.outstanding == 0.0 {
            let mut acts = vec!['c'];
            if s.raises_used < self.max_raises {
                acts.push('b');
            }
            acts
        } else {
            let mut acts = vec!['f', 'k'];
            if s.raises_used < self.max_raises {
                acts.push('r');
            }
            acts
        }
    }

    fn apply(&self, s: &LimitState, action_char: char) -> LimitState {
        let mut next = s.clone();
        next.history.push(action_char);
        let actor = s.to_act;
        let bet = self.bets[s.round];
        match action_char {
            'c' => {
                if s.first_checked {
                    self.end_round(&mut next);
                } else {
                    next.first_checked = true;
                    next.to_act = 1 - actor;
                }
            }
            'b' => {
                next.contrib[actor] += bet;
                next.outstanding = bet;
                next.raises_used += 1;
                next.to_act = 1 - actor;
            }
            'f' => {
                next.finished = Some(Finish::Fold { folder: actor });
            }
            'k' => {
                next.contrib[actor] += s.outstanding;
                next.outstanding = 0.0;
                self.end_round(&mut next);
            }
            'r' => {
                next.contrib[actor] += s.outstanding + bet;
                next.outstanding = bet;
                next.raises_used += 1;
                next.to_act = 1 - actor;
            }
            _ => unreachable!(),
        }
        next
    }

    fn end_round(&self, s: &mut LimitState) {
        if s.round + 1 < self.num_rounds {
            s.round += 1;
            s.to_act = 0;
            s.outstanding = 0.0;
            s.raises_used = 0;
            s.first_checked = false;
            if s.round == self.board_before_round && s.board.is_none() {
                s.awaiting_board = true;
            }
        } else {
            s.finished = Some(Finish::Showdown);
        }
    }

    fn initial(&self, cards: [u8; 2]) -> LimitState {
        LimitState {
            cards,
            board: None,
            round: 0,
            history: String::new(),
            contrib: [1.0, 1.0], // antes
            to_act: 0,
            outstanding: 0.0,
            raises_used: 0,
            first_checked: false,
            finished: None,
            awaiting_board: false,
        }
    }
}

fn fold_utility(s: &LimitState, folder: usize, player: usize) -> f64 {
    let pot = s.contrib[0] + s.contrib[1];
    let winner = 1 - folder;
    if player == winner {
        pot - s.contrib[player]
    } else {
        -s.contrib[player]
    }
}

fn showdown_utility(s: &LimitState, player: usize, cmp: std::cmp::Ordering) -> f64 {
    let pot = s.contrib[0] + s.contrib[1];
    let mine = s.contrib[player];
    match (cmp, player) {
        (std::cmp::Ordering::Greater, 0) | (std::cmp::Ordering::Less, 1) => pot - mine,
        (std::cmp::Ordering::Less, 0) | (std::cmp::Ordering::Greater, 1) => -mine,
        (std::cmp::Ordering::Equal, _) => pot / 2.0 - mine,
        _ => unreachable!(),
    }
}

/// Kuhn poker: deck {0=J, 1=Q, 2=K}, ante 1, one round, bet 1, one raise.
pub struct Kuhn {
    rules: LimitRules,
}

impl Default for Kuhn {
    fn default() -> Self {
        Kuhn {
            rules: LimitRules {
                deck: 3,
                num_rounds: 1,
                bets: [1.0, 0.0],
                max_raises: 1,
                board_before_round: usize::MAX,
            },
        }
    }
}

impl RefGame for Kuhn {
    type State = LimitState;

    fn initial_states(&self) -> Vec<(LimitState, f64)> {
        let mut roots = Vec::new();
        for a in 0..self.rules.deck as u8 {
            for b in 0..self.rules.deck as u8 {
                if a != b {
                    roots.push((self.rules.initial([a, b]), 1.0 / 6.0));
                }
            }
        }
        roots
    }

    fn is_terminal(&self, s: &LimitState) -> bool {
        s.finished.is_some()
    }

    fn utility(&self, s: &LimitState, player: usize) -> f64 {
        match s.finished.unwrap() {
            Finish::Fold { folder } => fold_utility(s, folder, player),
            Finish::Showdown => showdown_utility(s, player, s.cards[0].cmp(&s.cards[1])),
        }
    }

    fn player_to_act(&self, s: &LimitState) -> Option<usize> {
        Some(s.to_act)
    }

    fn chance_outcomes(&self, _s: &LimitState) -> Vec<(LimitState, f64)> {
        unreachable!("kuhn has no in-tree chance nodes")
    }

    fn num_actions(&self, s: &LimitState) -> usize {
        self.rules.actions(s).len()
    }

    fn next(&self, s: &LimitState, action: usize) -> LimitState {
        self.rules.apply(s, self.rules.actions(s)[action])
    }

    fn infoset_key(&self, s: &LimitState) -> String {
        format!("{}|{}", s.cards[s.to_act], s.history)
    }
}

/// Leduc hold'em: deck of 6 (rank = card / 2), ante 1, bets 2 and 4, two
/// raises max per round, one board card before round 2.
pub struct Leduc {
    rules: LimitRules,
}

impl Default for Leduc {
    fn default() -> Self {
        Leduc {
            rules: LimitRules {
                deck: 6,
                num_rounds: 2,
                bets: [2.0, 4.0],
                max_raises: 2,
                board_before_round: 1,
            },
        }
    }
}

impl Leduc {
    fn showdown_cmp(&self, s: &LimitState) -> std::cmp::Ordering {
        let rank = |c: u8| c / 2;
        let board = rank(s.board.expect("leduc showdown needs a board"));
        let p0 = rank(s.cards[0]);
        let p1 = rank(s.cards[1]);
        match (p0 == board, p1 == board) {
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            _ => p0.cmp(&p1),
        }
    }
}

impl RefGame for Leduc {
    type State = LimitState;

    fn initial_states(&self) -> Vec<(LimitState, f64)> {
        let mut roots = Vec::new();
        for a in 0..self.rules.deck as u8 {
            for b in 0..self.rules.deck as u8 {
                if a != b {
                    roots.push((self.rules.initial([a, b]), 1.0 / 30.0));
                }
            }
        }
        roots
    }

    fn is_terminal(&self, s: &LimitState) -> bool {
        s.finished.is_some()
    }

    fn utility(&self, s: &LimitState, player: usize) -> f64 {
        match s.finished.unwrap() {
            Finish::Fold { folder } => fold_utility(s, folder, player),
            Finish::Showdown => showdown_utility(s, player, self.showdown_cmp(s)),
        }
    }

    fn player_to_act(&self, s: &LimitState) -> Option<usize> {
        if s.awaiting_board {
            None
        } else {
            Some(s.to_act)
        }
    }

    fn chance_outcomes(&self, s: &LimitState) -> Vec<(LimitState, f64)> {
        debug_assert!(s.awaiting_board);
        (0..self.rules.deck as u8)
            .filter(|c| *c != s.cards[0] && *c != s.cards[1])
            .map(|c| {
                let mut next = s.clone();
                next.board = Some(c);
                next.awaiting_board = false;
                next.history.push_str(&format!("[{c}]"));
                (next, 0.25)
            })
            .collect()
    }

    fn num_actions(&self, s: &LimitState) -> usize {
        self.rules.actions(s).len()
    }

    fn next(&self, s: &LimitState, action: usize) -> LimitState {
        self.rules.apply(s, self.rules.actions(s)[action])
    }

    fn infoset_key(&self, s: &LimitState) -> String {
        format!("{}|{}", s.cards[s.to_act], s.history)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{VanillaCfr, expected_value, exploitability};

    #[test]
    fn oracle_cfr_solves_kuhn() {
        let kuhn = Kuhn::default();
        let mut cfr = VanillaCfr::new(&kuhn);
        cfr.run(20_000);
        let profile = cfr.average_profile();
        let value = expected_value(&kuhn, &profile, 0);
        assert!(
            (value - (-1.0 / 18.0)).abs() < 2e-3,
            "oracle kuhn value = {value}"
        );
        let expl = exploitability(&kuhn, &profile);
        assert!(expl[0] < 2e-3 && expl[1] < 2e-3, "expl = {expl:?}");
    }

    #[test]
    fn uniform_profile_is_exploitable() {
        let kuhn = Kuhn::default();
        let uniform = std::collections::HashMap::new();
        let expl = exploitability(&kuhn, &uniform);
        assert!(expl[0] > 0.1 && expl[1] > 0.1, "expl = {expl:?}");
    }
}
