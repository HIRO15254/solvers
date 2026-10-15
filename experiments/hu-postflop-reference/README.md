# HU Postflop external reference checks (2026-07)

Historical comparisons with GTO Wizard. These cases record the reference
conditions, solver configuration, measured differences, and known tree
mismatches. They are evidence from July 2026, not the reference set or
acceptance criteria for the current
[HU postflop reference candidates](../../docs/plans/hu-postflop-validation/README.md).

| Case | Record and configuration | Scope |
|---|---|---|
| Ks7h2d flop | `cases/ks7h2d-flop/README.md` (local-only, not distributed) | BB vs BTN, 100bb chip EV, multistreet solve; copied reference ranges and comparison values |
| Ks7h2d3c8d river | `cases/ks7h2d3c8d-river/README.md` (local-only, not distributed) | BB vs BTN, 100bb cash with rake, river-only solve; copied reference ranges and comparison values |

The original [cross-check procedure](procedure-2026-07.md) explains how these
comparisons were made. Recheck its dated CLI and browser steps against the
current specifications before reusing them.
