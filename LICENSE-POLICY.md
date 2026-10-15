# License Policy

This repository is licensed under **MIT OR Apache-2.0** (dual license, at your option).
To keep that possible, we follow a strict clean-room policy with respect to
copyleft and unlicensed reference material.

## Read-only references (NO code reuse, design study only)

The following projects may be **read for design understanding only**. Copying,
porting, or closely paraphrasing their code into this repository is
**prohibited** — doing so would force this repository onto AGPL-3.0 or make it
undistributable.

| Project | License | Status |
|---|---|---|
| b-inary/postflop-solver | AGPL-3.0-or-later | read-only |
| b-inary/wasm-postflop | AGPL-3.0-or-later | read-only |
| b-inary/desktop-postflop | AGPL-3.0-or-later | read-only |
| bupticybee/TexasSolver | AGPL-3.0 | read-only |
| Any repository without an explicit license | all rights reserved | read-only |

Design *ideas* (algorithms, data-layout strategies, API shapes, parameter
choices) are not copyrightable and may be reimplemented independently. When a
design idea is taken from one of these projects, cite the project in a comment
or in `docs/architecture.md`, and write the implementation from scratch.

## Permitted reuse

| Source | License | Permitted use |
|---|---|---|
| aya_poker (crates.io) | Zlib OR Apache-2.0 OR MIT | dependency |
| rayon, serde, clap, etc. (crates.io) | MIT/Apache-2.0 | dependency |
| K. Waugh, "A Fast and Optimal Hand Isomorphism Algorithm" reference code | BSD | port with attribution |
| google-deepmind/open_spiel | Apache-2.0 | correctness oracle values (offline), code reference |
| Academic papers (CFR+, DCFR, MCCFR, ...) | n/a (ideas) | reimplement freely, cite in docs |
| Tabler Icons 3.34.1 (`web/vendor/cryst/fonts/tabler-icons.woff2`) | MIT (`LICENSE-TablerIcons.txt` beside font) | vendored GUI asset |
| JetBrains Mono 2.211 (`web/vendor/cryst/fonts/jetbrains-mono-variable-latin.woff2`) | SIL OFL 1.1 (`OFL-JetBrainsMono.txt` beside font) | vendored GUI asset |
| Noto Sans JP 2.004 (`web/vendor/cryst/fonts/noto-sans-jp-variable-jis1.woff2`) | SIL OFL 1.1 (`OFL-NotoSansJP.txt` beside font) | offline GUI font, variable weights 400–700; Latin, kana and JIS level-1 kanji for Japanese user text |
| Cryst Design System (`web/vendor/cryst/`, copied 2026-10-13 from the canonical artifact `https://claude.ai/artifact/TXKBwjMEB9xMt52GBbERVs`) | MIT OR Apache-2.0 (owned by the repository owner and licensed with this repository, decided 2026-10-15) | vendored GUI design system |

## GTO Wizard data (local research only)

User decision, 2026-10-15: GTO Wizard (GTOW) solutions and values copied from
them are **not licensed for redistribution by this project**. Raw solutions,
combo/class ranges, action frequencies, equities, EVs, and comparison reports
containing those values are local research material only. They must remain
git-ignored and must not enter the public repository, release archives, or
third-party license notices as if our MIT/Apache license covered them.
`git rm --cached` removes the index entry, **not the local research file**.
Existing Git history still contains the removed data. The user explicitly
decided on 2026-10-15 to preserve that history as-is: no history rewrite.
This does not authorize reintroducing or distributing the data in current
source snapshots or release archives.

The per-file inventory with a one-line reason is
`packaging/gtow-local-only.json`; the excluded paths are recorded in `.gitignore`. In particular:

| Local-only paths | Reason |
|---|---|
| `experiments/hu-postflop-reference/cases/ks7h2d-flop/README.md` | copied reference frequencies, equities and EV comparisons |
| `experiments/hu-postflop-reference/cases/ks7h2d-flop/gw-check-*.toml` | copied weighted preflop ranges |
| `experiments/hu-postflop-reference/cases/ks7h2d3c8d-river/README.md` | copied river frequencies, equities and EV comparisons |
| `experiments/hu-postflop-reference/cases/ks7h2d3c8d-river/gw-check-*.toml` | copied weighted node-entry ranges |
| `experiments/hu-postflop-reference/fixtures-2026-10/README.md` | embedded GTOW EV/frequency comparison tables |
| `experiments/hu-postflop-reference/fixtures-2026-10/cases/c*/gtow-flop.json` | raw combo ranges, strategies and EVs |
| `experiments/hu-postflop-reference/fixtures-2026-10/cases/c*/gtow-menus.json` | acquired solution-node data and derived menus |
| `experiments/hu-postflop-reference/fixtures-2026-10/cases/c*/input.toml` | generated inputs containing copied GTOW root ranges |
| `experiments/hu-postflop-reference/fixtures-2026-10/cases/c*/comparison.json` | reference EVs/frequencies and derived differences |
| `experiments/multiway-2026-09/multiway-convergence-round5-20260909/references/gtowizard-*.json` | observed class/node frequencies and reference hand panels |
| `experiments/p1-efficiency-2026-10/templates-20261009/configs/gw_single.toml` | duplicate of the copied weighted-range Single Size input |
| `experiments/multiway-2026-09/multiway-convergence-round{4,5}-20260909/README.md`, round4 `result.json`, round5 `simple-screen.md` | embedded copied GTOW frequencies or combo totals |
| `experiments/multiway-2026-09/simple-depth-coverage-20260910/{README.md,reference.json}` | copied deep-node frequencies and combo counts |
| `experiments/p2-method-2026-10/l1-6max/README.md`, `results/{rfi.md,summary.md,buckets/rfi-b128.md}` | embedded copied per-position and per-class opening frequencies |
| `experiments/p1-efficiency-2026-10/templates-20261009/results/raw/gw_range_x.csv`, `gw_strategy_*.csv` | copied combo-weight columns, even where strategy probabilities are our output |

Our independently implemented tree configurations (including the
`examples/bench/6max_100bb_nl50_partial*_reference*.toml` workloads), solver
timings, convergence measurements, case-selection metadata and tree statistics
are not GTOW solutions merely because their names mention GTOW. These remain
tracked where they do not contain copied ranges or solution values. Public
source citations and descriptions of published tree designs remain references,
not grants to redistribute solution data. Tests requiring local-only data must
skip clearly when absent; public CI must not depend on these files.

## Distribution notices

Run `bash packaging/third-party.sh` to regenerate
`packaging/THIRD-PARTY-LICENSES.txt` from the locked P1 CLI/daemon dependency
graph, production web dependencies and bundled fonts. The generator accepts
only explicitly reviewed permissive licenses and fails closed for unknown or
copyleft licenses (including GPL, AGPL, LGPL and MPL). Font license files remain
next to their bundled assets. Regenerate notices whenever dependencies or
vendored assets change; development-only npm packages and P2-only crates do not
belong to the P1 release.

## Contribution rule

Every PR that introduces externally-inspired code must state the source and
its license in the PR description. When in doubt, treat the source as
read-only and reimplement.
