# Shared tools

This directory contains reusable repository helpers. Python tests live in
`tools/tests/`. The dated Multiway validation scripts, frozen runners, and
their tests are archived with the corresponding experiments in
[`experiments/multiway-2026-09/scripts/`](../experiments/multiway-2026-09/scripts/).

- `workspace_audit.py`: read-only workspace and Git hygiene report.
- `check_docs.py`: checks the canonical documentation entrypoints, local Markdown
  link destinations, and retired plan/specification paths in current documentation.
- `plot_convergence.py`: plot solver progress files.
- `../packaging/third-party.sh`: generate release license notices using Cargo
  metadata/tree and the locked crates.io/npm source archives. Requires Cargo
  and Python **3.11+**, not cargo-about, npm installation, or a Rust build.
  From the repository root run `sh packaging/third-party.sh`; it writes
  `packaging/THIRD-PARTY-LICENSES.txt`. Network access is needed initially for
  Cargo sources and the integrity-checked production npm tarballs. Keep Cargo's
  registry archive cache and `.cache/third-party/npm/` for subsequent
  `sh packaging/third-party.sh --offline --check` runs. `--output PATH` selects
  another output; `PYTHON` selects a Python executable (including a path with
  spaces on Windows Git Bash). The four-target union covers the `cli` and
  `daemon` normal/build dependency graph with `--no-default-features`, including
  proc macros/build tools conservatively but excluding dev-only dependencies.
  Production npm dependencies (including peers/transitives) and all vendored
  font assets are inventoried separately. The generator rejects any SPDX
  branch outside `packaging/third-party.json`'s explicit allowlist, including
  GPL/AGPL/LGPL/MPL and unknown licenses, and never substitutes generic license
  text. OFL-1.1 is allowed for fonts only. Historical non-SPDX/multi-component
  Rust licenses have version, lock checksum, and text-hash-pinned reviews in
  that config (including legacy OpenSSL/SSLeay attribution and bundled zstd's
  BSD alternative, not its GPL alternative). Update those reviews when
  dependencies change. Output has LF endings, stable ordering, lock/config
  fingerprints, and no timestamps or machine-local source paths. Complete
  notices are replaced atomically only after every input passes. Release
  packaging must require this file and check it after dependency/font updates.

Run the lightweight checks with the Python standard library (Python 3.11+):

```sh
python -m unittest discover -s tools/tests -v
python tools/check_docs.py
python tools/workspace_audit.py --json
```

The documentation checker scans root Markdown files and Markdown under `docs/`,
`tools/`, `examples/`, and `crates/`. It skips fenced and inline code links,
remote URLs, and heading anchors. Historical `experiments/` documents are not
scanned, but links into them from current documentation must resolve. Research
surveys under `docs/research/` and decision records under `docs/decisions/` may
mention retired document paths as historical text; their actual links must
still resolve. This is a file-link check, not a complete Markdown parser or a
remote-link/anchor validator. It does not duplicate task state from Linear.

Tool tests use disposable `.cache/tool-tests/` fixtures, never Cargo's `target/`.
The workspace audit reports `experiments/` separately alongside build output,
scratch runs, caches, source, and documentation; it does not delete anything.

CI runs the Python checks alongside the standard Rust verification, and exercises
CLI checkpoint/resume and daemon HTTP behavior on Windows. Expensive acceptance
tests remain in the manually dispatched acceptance workflow.

The old Multiway benchmark and cloud runners are retained with the
[`multiway-2026-09` experiment](../experiments/multiway-2026-09/README.md)
or in Git history. They are not current development commands.

The products and their quality goals are defined in
[docs/products.jp.md](../docs/products.jp.md); the migration steps are in the
[restructure plan](../docs/plans/two-product-restructure.jp.md).
