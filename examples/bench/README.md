# Performance workloads

These `solvers.nlh/v1` workloads measure solver throughput and public-tree sizing. They are not tutorials. Run them explicitly in release mode; reference trees and EHS2 cache construction can require substantial memory and time.

These files are also canonical regression baselines used by [the public-tree identity tests](../../crates/mw-preflop/tests/nlh_tree_identity.rs). Changing their table, ranges, menus or solver settings changes the baseline. Review such changes deliberately and update the corresponding structural pins and validation evidence together; keep comment and layout edits computationally neutral.
