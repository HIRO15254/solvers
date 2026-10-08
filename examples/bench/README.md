# Performance workloads

These `solvers.nlh/v1` workloads measure solver throughput and public-tree sizing. They are not tutorials. Run them explicitly in release mode; reference trees and EHS2 cache construction can require substantial memory and time.

These files are also canonical regression baselines used by [the public-tree identity tests](../../crates/mw-preflop/tests/nlh_tree_identity.rs). Changing their table, ranges, menus or solver settings changes the baseline. Review such changes deliberately and update the corresponding structural pins and validation evidence together; keep comment and layout edits computationally neutral.

`hu_pushfold_5bb.toml`, `hu_pushfold_10bb.toml` and `hu_pushfold_20bb.toml` are benchmark B1 of [the P2 method redesign plan](../../docs/plans/p2-method-redesign.jp.md): heads-up push/fold games checked against an independent exact implementation. The identity tests do not pin them.

`6max_100bb_nl50_partial_simple_reference_checkdown.toml` and `6max_100bb_nl50_partial_reference_checkdown.toml` are benchmark B4 of the same plan: the Simple and General reference preflop trees (6,845 and 16,912 preflop decisions) with postflop checkdown. They copy the corresponding reference workloads and change only the postflop menus (and, for General, the postflop aggression limits and bucket counts, which checkdown leaves unused). The identity tests do not pin them.

`hu_20bb_postflop.toml` and `6max_20bb.toml` are benchmarks B6 and B7 of the same plan, for the L1 leaf model (abstracted heads-up postflop at preflop leaves where two players see the flop). B6 is a small heads-up 20-BB tree with limps, opens and all-ins and one postflop bet (half pot or all in) per street. B7 is `6max_20bb_checkdown.toml` (B3) with the checkdown rule removed, so its postflop menus apply. The identity tests do not pin them.
