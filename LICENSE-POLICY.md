# License Policy

This repository is licensed under **MIT OR Apache-2.0** (dual license, at your option).

## Third-party components

| Source | License | Use |
|---|---|---|
| aya_poker (crates.io) | Zlib OR Apache-2.0 OR MIT | dependency |
| rayon, serde, clap, etc. (crates.io) | MIT/Apache-2.0 | dependency |
| Academic papers (CFR+, DCFR, MCCFR, ...) | n/a (ideas) | algorithms reimplemented independently, cited in docs |
| Tabler Icons 3.34.1 (`web/vendor/cryst/fonts/tabler-icons.woff2`) | MIT (`LICENSE-TablerIcons.txt` beside font) | vendored GUI asset |
| JetBrains Mono 2.211 (`web/vendor/cryst/fonts/jetbrains-mono-variable-latin.woff2`) | SIL OFL 1.1 (`OFL-JetBrainsMono.txt` beside font) | vendored GUI asset |
| Noto Sans JP 2.004 (`web/vendor/cryst/fonts/noto-sans-jp-variable-jis1.woff2`) | SIL OFL 1.1 (`OFL-NotoSansJP.txt` beside font) | offline GUI font, variable weights 400–700; Latin, kana and JIS level-1 kanji for Japanese user text |

## Distribution notices

Run `bash packaging/third-party.sh` to regenerate
`packaging/THIRD-PARTY-LICENSES.txt` from the locked CLI/daemon dependency
graph of the release build, production web dependencies and bundled fonts. The
generator accepts only explicitly reviewed permissive licenses and fails closed
for unknown or copyleft licenses (including GPL, AGPL, LGPL and MPL). Font
license files remain next to their bundled assets. Regenerate notices whenever
dependencies or vendored assets change; development-only npm packages and
crates excluded from the release build do not belong to the release. Generate
on Windows: Cargo resolves build-dependencies for the host, and the Windows host
graph is the superset of the release hosts. The release workflow checks the
committed file on `windows-latest`.

## Contribution rule

Every PR that introduces third-party code or assets must state the source and
its license in the PR description, and add it to the table above.
