//! FROZEN scalar CFR + best-response oracle for differential testing.
//!
//! This crate deliberately uses the naive per-history representation the
//! engine avoids: hash-map infosets keyed by strings, recursion over
//! individual card assignments, vanilla CFR with simultaneous updates.
//! It exists so the vectorized engine has an independent implementation to
//! disagree with. DO NOT optimize it, and do not share code with the
//! `engine` or `game` crates; its value is its independence.

use std::collections::HashMap;

pub mod games;

/// A two-player zero-or-general-sum game in scalar form.
pub trait RefGame {
    type State: Clone;

    /// Root states with their chance probabilities (e.g. all deals).
    fn initial_states(&self) -> Vec<(Self::State, f64)>;

    fn is_terminal(&self, s: &Self::State) -> bool;

    /// Utility for `player` (0 or 1) at a terminal state.
    fn utility(&self, s: &Self::State, player: usize) -> f64;

    /// `None` for chance nodes, otherwise the acting player.
    fn player_to_act(&self, s: &Self::State) -> Option<usize>;

    /// Chance outcomes with probabilities (only for chance nodes).
    fn chance_outcomes(&self, s: &Self::State) -> Vec<(Self::State, f64)>;

    fn num_actions(&self, s: &Self::State) -> usize;

    fn next(&self, s: &Self::State, action: usize) -> Self::State;

    /// Information-set key for the acting player, shared with the engine
    /// adapters: `"{private card}|{history}"`.
    fn infoset_key(&self, s: &Self::State) -> String;
}

/// A strategy profile queried by infoset key. Unknown infosets play uniform.
pub trait RefStrategy {
    fn strategy(&self, infoset: &str, num_actions: usize) -> Vec<f64>;
}

impl RefStrategy for HashMap<String, Vec<f64>> {
    fn strategy(&self, infoset: &str, num_actions: usize) -> Vec<f64> {
        match self.get(infoset) {
            Some(sigma) => sigma.clone(),
            None => vec![1.0 / num_actions as f64; num_actions],
        }
    }
}

/// Expected utility of a strategy profile for `player`, averaged over all
/// root states.
pub fn expected_value<G: RefGame>(game: &G, profile: &dyn RefStrategy, player: usize) -> f64 {
    fn walk<G: RefGame>(game: &G, profile: &dyn RefStrategy, player: usize, s: &G::State) -> f64 {
        if game.is_terminal(s) {
            return game.utility(s, player);
        }
        if game.player_to_act(s).is_none() {
            return game
                .chance_outcomes(s)
                .iter()
                .map(|(next, p)| p * walk(game, profile, player, next))
                .sum();
        }
        let n = game.num_actions(s);
        let sigma = profile.strategy(&game.infoset_key(s), n);
        (0..n)
            .map(|a| sigma[a] * walk(game, profile, player, &game.next(s, a)))
            .sum()
    }
    game.initial_states()
        .iter()
        .map(|(s, p)| p * walk(game, profile, player, s))
        .sum()
}

/// Value of the best response of `player` against the opponent's part of
/// `profile`.
///
/// Uses the standard two-pass construction: group states by the responder's
/// infoset with opponent/chance reach weights, then compute max-over-actions
/// values bottom-up with memoization on (infoset, action) subtrees. The
/// recursion carries the full state so opponent infosets stay resolvable.
pub fn best_response_value<G: RefGame>(game: &G, profile: &dyn RefStrategy, player: usize) -> f64 {
    // Reach-weighted state bundles per responder infoset, built lazily
    // during recursion: at a responder node we must act on the whole
    // infoset, so we first collect every state consistent with it.
    // For the small games this oracle serves, we can afford to enumerate
    // states repeatedly.
    let mut choices: HashMap<String, usize> = HashMap::new();
    let roots = game.initial_states();
    let mut bundles: HashMap<String, Vec<(G::State, f64)>> = HashMap::new();
    collect_bundles(game, profile, player, &roots, &mut bundles);
    let mut total = 0.0;
    for (s, p) in &roots {
        total += p * br_walk(game, profile, player, s, &bundles, &mut choices);
    }
    total
}

/// Enumerates all responder infosets with the opponent/chance reach of each
/// member state.
fn collect_bundles<G: RefGame>(
    game: &G,
    profile: &dyn RefStrategy,
    player: usize,
    roots: &[(G::State, f64)],
    bundles: &mut HashMap<String, Vec<(G::State, f64)>>,
) {
    fn rec<G: RefGame>(
        game: &G,
        profile: &dyn RefStrategy,
        player: usize,
        s: &G::State,
        reach: f64,
        bundles: &mut HashMap<String, Vec<(G::State, f64)>>,
    ) {
        if game.is_terminal(s) {
            return;
        }
        match game.player_to_act(s) {
            None => {
                for (next, p) in game.chance_outcomes(s) {
                    rec(game, profile, player, &next, reach * p, bundles);
                }
            }
            Some(actor) if actor == player => {
                bundles
                    .entry(game.infoset_key(s))
                    .or_default()
                    .push((s.clone(), reach));
                // The responder plays every action somewhere in the BR
                // computation; recurse with unchanged reach (their own
                // probability is not part of counterfactual reach).
                for a in 0..game.num_actions(s) {
                    rec(game, profile, player, &game.next(s, a), reach, bundles);
                }
            }
            Some(_) => {
                let n = game.num_actions(s);
                let sigma = profile.strategy(&game.infoset_key(s), n);
                for (a, &prob) in sigma.iter().enumerate().take(n) {
                    rec(
                        game,
                        profile,
                        player,
                        &game.next(s, a),
                        reach * prob,
                        bundles,
                    );
                }
            }
        }
    }
    for (s, p) in roots {
        rec(game, profile, player, s, *p, bundles);
    }
}

/// Best-response value of a single state, where responder decisions are
/// taken infoset-wide (argmax of reach-weighted action values over the
/// bundle) and memoized by infoset key.
fn br_walk<G: RefGame>(
    game: &G,
    profile: &dyn RefStrategy,
    player: usize,
    s: &G::State,
    bundles: &HashMap<String, Vec<(G::State, f64)>>,
    choices: &mut HashMap<String, usize>,
) -> f64 {
    if game.is_terminal(s) {
        return game.utility(s, player);
    }
    match game.player_to_act(s) {
        None => game
            .chance_outcomes(s)
            .iter()
            .map(|(next, p)| p * br_walk(game, profile, player, next, bundles, choices))
            .sum(),
        Some(actor) if actor == player => {
            let key = game.infoset_key(s);
            let best_action = if let Some(&a) = choices.get(&key) {
                a
            } else {
                // Choose the action maximizing reach-weighted value over the
                // whole infoset bundle.
                let bundle = &bundles[&key];
                let n = game.num_actions(s);
                let mut best = (0usize, f64::NEG_INFINITY);
                for a in 0..n {
                    let mut v = 0.0;
                    for (state, reach) in bundle {
                        v += reach
                            * br_walk(
                                game,
                                profile,
                                player,
                                &game.next(state, a),
                                bundles,
                                choices,
                            );
                    }
                    if v > best.1 {
                        best = (a, v);
                    }
                }
                choices.insert(key, best.0);
                best.0
            };
            br_walk(
                game,
                profile,
                player,
                &game.next(s, best_action),
                bundles,
                choices,
            )
        }
        Some(_) => {
            let n = game.num_actions(s);
            let sigma = profile.strategy(&game.infoset_key(s), n);
            (0..n)
                .map(|a| {
                    sigma[a] * br_walk(game, profile, player, &game.next(s, a), bundles, choices)
                })
                .sum()
        }
    }
}

/// Per-player exploitability of a profile: `BR_p - EV_p`.
pub fn exploitability<G: RefGame>(game: &G, profile: &dyn RefStrategy) -> [f64; 2] {
    [
        best_response_value(game, profile, 0) - expected_value(game, profile, 0),
        best_response_value(game, profile, 1) - expected_value(game, profile, 1),
    ]
}

/// Vanilla CFR with simultaneous updates. Only used for this oracle's own
/// sanity tests; differential tests evaluate engine strategies directly.
pub struct VanillaCfr<'a, G: RefGame> {
    game: &'a G,
    regrets: HashMap<String, Vec<f64>>,
    strategy_sum: HashMap<String, Vec<f64>>,
}

impl<'a, G: RefGame> VanillaCfr<'a, G> {
    pub fn new(game: &'a G) -> Self {
        VanillaCfr {
            game,
            regrets: HashMap::new(),
            strategy_sum: HashMap::new(),
        }
    }

    pub fn run(&mut self, iterations: u64) {
        for _ in 0..iterations {
            for player in 0..2 {
                for (s, p) in self.game.initial_states() {
                    self.walk(&s, player, [p, p]);
                }
            }
        }
    }

    pub fn average_profile(&self) -> HashMap<String, Vec<f64>> {
        self.strategy_sum
            .iter()
            .map(|(k, sums)| {
                let total: f64 = sums.iter().sum();
                let sigma = if total > 0.0 {
                    sums.iter().map(|s| s / total).collect()
                } else {
                    vec![1.0 / sums.len() as f64; sums.len()]
                };
                (k.clone(), sigma)
            })
            .collect()
    }

    fn current(&self, key: &str, n: usize) -> Vec<f64> {
        match self.regrets.get(key) {
            Some(r) => {
                let total: f64 = r.iter().map(|&x| x.max(0.0)).sum();
                if total > 0.0 {
                    r.iter().map(|&x| x.max(0.0) / total).collect()
                } else {
                    vec![1.0 / n as f64; n]
                }
            }
            None => vec![1.0 / n as f64; n],
        }
    }

    /// Returns the utility for `player`; `reach[q]` is the probability of
    /// reaching `s` under player q's strategy (chance folded into roots and
    /// outcomes).
    fn walk(&mut self, s: &G::State, player: usize, reach: [f64; 2]) -> f64 {
        if self.game.is_terminal(s) {
            return self.game.utility(s, player);
        }
        if self.game.player_to_act(s).is_none() {
            return self
                .game
                .chance_outcomes(s)
                .iter()
                .map(|(next, p)| p * self.walk(next, player, reach))
                .sum();
        }
        let actor = self.game.player_to_act(s).unwrap();
        let key = self.game.infoset_key(s);
        let n = self.game.num_actions(s);
        let sigma = self.current(&key, n);

        let mut action_values = vec![0.0; n];
        let mut node_value = 0.0;
        for a in 0..n {
            let mut next_reach = reach;
            next_reach[actor] *= sigma[a];
            action_values[a] = self.walk(&self.game.next(s, a), player, next_reach);
            node_value += sigma[a] * action_values[a];
        }

        if actor == player {
            let cf_reach = reach[1 - player];
            let regrets = self
                .regrets
                .entry(key.clone())
                .or_insert_with(|| vec![0.0; n]);
            for a in 0..n {
                regrets[a] += cf_reach * (action_values[a] - node_value);
            }
            let sums = self.strategy_sum.entry(key).or_insert_with(|| vec![0.0; n]);
            for a in 0..n {
                sums[a] += reach[player] * sigma[a];
            }
        }
        node_value
    }
}
