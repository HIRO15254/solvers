# NLH examples

All inputs use `solvers.nlh/v1`. Validate first with `solvers validate FILE`; see [the input specification](../docs/nlh-input-v1.jp.md). Sizes describe the example settings, not a quality guarantee.

| File | Product | What it shows | Rough solve size |
|---|---|---|---|
| [river_small.toml](hu-postflop/river_small.toml) | P1 | Small HU river, explicit ranges and BB bet | Seconds |
| [turn_small.toml](hu-postflop/turn_small.toml) | P1 | Turn-to-river chance expansion | Seconds |
| [flop_srp.toml](hu-postflop/flop_srp.toml) | P1 | Specification §14 BTN vs BB cash SRP with rake | Large; up to 1h |
| [tournament_icm.toml](hu-postflop/tournament_icm.toml) | P1 | Tournament payouts and outside field | Seconds |
| [river_script.toml](hu-postflop/river_script.toml) | P1 | External [river.tree](hu-postflop/river.tree) and tree parameters | Seconds |
| [3max_smoke.toml](mw-preflop/3max_smoke.toml) | P2 | Small 3-max all-in tree | Seconds after cache preparation |
| [6max_100bb_cash.toml](mw-preflop/6max_100bb_cash.toml) | P2 | Specification §14 cash tree, no limps, checkdown | Large; up to 12h |
| [tournament_icm.toml](mw-preflop/tournament_icm.toml) | P2 | Tournament ICM | Small tree; cache preparation needed |
| [selectors_checkdown.toml](mw-preflop/selectors_checkdown.toml) | P2 | Actor selector and postflop checkdown | Small tree; cache preparation needed |
| [straddle.toml](mw-preflop/straddle.toml) | P2 | Live UTG straddle | Large; cache preparation needed |

[bench/](bench/README.md) contains performance workloads. Exact regression inputs belong beside their tests in `crates/*/tests/fixtures/`.
