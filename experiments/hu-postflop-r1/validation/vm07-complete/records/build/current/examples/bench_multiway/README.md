# Multiway benchmark fixtures

These TOML files are stable input fixtures for Multiway Preflop parser and
legal-action checks. The current input contract is in
[`docs/multiway-preflop-v1.jp.md`](../../docs/multiway-preflop-v1.jp.md).

| Files | Purpose |
|---|---|
| `3max_2bb.toml`, `6max_2bb.toml` | Small all-in trees for deterministic smoke checks. |
| `6max_20bb_checkdown.toml` | Historical 20bb throughput fixture with postflop checkdown. |
| `6max_100bb_nl50_partial_reference.toml`, `6max_100bb_nl50_partial_reference_limp.toml`, `6max_100bb_nl50_partial_simple_reference.toml` | Partial reference menus covered by current CLI contract tests. They do not reproduce a complete GTO Wizard game. |
| `6max_position_selector.toml` | Position-selector parser fixture. |

The historical runner, exact benchmark conditions, results, and comparison
limits are indexed in the
[`2026-09 Multiway experiments`](../../experiments/multiway-2026-09/README.md).
The two 2bb fixtures still mention the runner's old `tools/` path in comments;
their bytes are preserved for historical hash comparisons.
