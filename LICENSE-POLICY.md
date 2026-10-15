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
| Tabler Icons webfont (`web/vendor/cryst/fonts/tabler-icons.woff2`) | MIT (upstream) | vendored GUI asset |
| JetBrains Mono (`web/vendor/cryst/fonts/jetbrains-mono-variable-latin.woff2`) | SIL OFL 1.1 (upstream) | vendored GUI asset |
| Noto Sans JP (Google Fonts, fetched by the browser at runtime) | SIL OFL 1.1 | GUI font, not stored in the repository |
| Cryst Design System (`web/vendor/cryst/`, copied 2026-10-13 from the canonical artifact `https://claude.ai/artifact/TXKBwjMEB9xMt52GBbERVs`) | MIT OR Apache-2.0 (owned by the repository owner and licensed with this repository, decided 2026-10-15) | vendored GUI design system |

## Contribution rule

Every PR that introduces externally-inspired code must state the source and
its license in the PR description. When in doubt, treat the source as
read-only and reimplement.
