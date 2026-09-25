# Storage and bucket-context description corrections

2026-09-10. The earlier
[endpoint-deviation report](endpoint-deviation-20260910/plan.md) describes
its real Holdem test as covering “sparse/dense storage.” This wording is too
broad. Both tested constructors use dense storage for Street recall:
`MultiwaySolver::new` uses ordinary dense allocation and
`new_preallocated_with_threads` additionally commits pages and uses the
configured initialization pool. The boolean named `dense` in that fixture
selects those constructors, not sparse versus dense storage.

The source of truth is the `new_internal` match on `game.recall_mode()` in
`crates/multiway/src/solver/mod.rs`. Street always creates `Some(DenseStorage)`;
Full is the sparse research/test mode. The endpoint diagnostic requires Street
recall. Its real Holdem equality, profile variants, thread determinism and
read-only assertions remain valid, but that fixture does not prove sparse
endpoint evaluation.

No numerical measurement or checkpoint is changed by this correction. The
earlier report and its hash remain retained historical evidence; interpret
its storage claim with this explicit correction. The postflop continuation
experiment tests the Full-recall rejection directly, and tests an unavailable
dense arena only as an intentionally invalid internal state. Its sparse-worker
unit test separately establishes early rejection before RNG/events.

## Recorded street opponent counts

The preflop endpoint tests also exposed an inaccurate description of
`bucket_active_opponents` as a count fixed at the street's start. Existing
`BettingState` updates `street_active_players[current_street]` after each
action. `HoldemGame::dense_node_context` reads that record for bucket
cardinality and separately reads the non-folded mask for the information-key
context. At a current preflop decision after one of three players folds, both
counts are one opponent, not one versus two. Folded players' cards still
remain in the physical deal and block other hands.

The new fixture initially asserted two bucket opponents there; it has been
corrected against the unchanged betting and Holdem implementations. Current
normative/architecture/guide descriptions now say the recorded bucket context,
without claiming it is fixed at street start. State version 4's cache-key
separation remains unchanged. Earlier reports using the street-start wording
should be read with this correction; no old numerical evidence is revised.
