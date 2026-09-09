# Multiway postflop abstraction: next diagnostic

Date: 2026-09-09. This read-only audit changes no v1 contract, production
default, solver state, or experiment result.

## Verified representation boundary

Production postflop bucketing reduces each physical `(board, hole combo)` to one
scalar `E[HS²]`. Flop and turn average squared heads-up hand strength over all
legal runouts; river uses current `HS²`:
[flop](../../crates/abstraction/src/buckets.rs#L333),
[turn](../../crates/abstraction/src/buckets.rs#L375), and
[river](../../crates/abstraction/src/buckets.rs#L411). The table builder then
applies one global percentile threshold sequence per street
([build_table](../../crates/abstraction/src/buckets.rs#L484)). Lookup
canonicalizes the physical board and combo but returns only that bucket number
([bucket](../../crates/abstraction/src/buckets.rs#L865)).

Increasing `K` therefore resolves the same one-dimensional score more finely.
It can separate situations whose scalar scores differ enough, but it cannot
recover distinctions orthogonal to that scalar. Two situations with equal or
near-equal `E[HS²]` may still differ in current hand rank, board rank/suit
texture, blockers, or the distribution of future hand strength.

The board influences the score, but its identity is not stored independently in
the policy key. `HistoryKey::child` hashes prior history, actor, and action index
([source](../../crates/multiway/src/solver/mod.rs#L352)); current-street recall
stores only the current bucket
([source](../../crates/multiway/src/solver/mod.rs#L333)). `InfoKey` then combines
that private information with public betting history and player context
([source](../../crates/multiway/src/solver/mod.rs#L397)).

The table adapter ignores `active_opponents`
([source](../../crates/multiway/src/abstraction.rs#L231)), though Holdem passes it
([source](../../crates/multiway/src/holdem.rs#L267)). Its hand-strength target is
uniform heads-up strength
([hand_strength](../../crates/abstraction/src/ehs.rs#L21)), rather than a
range-weighted or multiway equity target.

The existing feature hash records hand rank, hole structure, board/combined
rank counts, a rank mask, canonical suit texture, and active-opponent count
([invariant_features](../../crates/multiway/src/abstraction.rs#L367)). These
dimensions are available internally, but the hash is neither a similarity
abstraction nor a production candidate.

## Existing sensitivity evidence

The Simple partial-tree seed-0 sensitivity changed only postflop bucket counts
from K32 to K128. Mean preflop MAE moved from 14.0391 to 14.1068 percentage
points, while pooled RMSE increased 5.33%
([record](multiway-simple-screen-2026-09-09.md#L114)). This single training seed,
partial tree, and finite solve do not show that K128 is worse or that abstraction
caused the error. They do show that this run provides no evidence for improving
accuracy by increasing `K` alone.

## Proposed bounded prototype

Add a deterministic collision-audit example behind the existing
`research-abstractions` feature. It would not construct or serialize a solver.
For one fixed set of sampled physical `(board, hole combo)` states, it would:

1. query the production table at K32, K128, and K256;
2. compute exact `E[HS]` and `E[HS²]` with the existing
   [`ehs2`](../../crates/abstraction/src/ehs.rs#L56), derive
   `Var(HS) = E[HS²] - E[HS]²`, and record the existing rank/texture
   descriptor;
3. report within-bucket dispersion of `E[HS]` and `Var(HS)`, plus rank/texture
   collision counts, by street and K;
4. use fixed sample IDs and identical states for every K, and label incomplete
   or bounded runs explicitly.

Rapidly falling within-bucket dispersion would support insufficient scalar
resolution. A plateau as K increases would expose information that scalar
`E[HS²]` does not retain. The
prototype is about 180--250 lines plus focused tests for deterministic replay,
suit isomorphism, dead-card rejection, exact moments, and a known collision; an
initial implementation should fit within one engineering day.

The audit only characterizes representation loss. `Var(HS)` is future-equity
dispersion, not a complete draw label; rank/texture collisions are not an
action-value target. It cannot show that abstraction loss causes the observed
GTOW/preflop error or prove that a richer abstraction improves strategy. A
positive result would justify a paired research-only joint-moment solve before
any v1 proposal.
