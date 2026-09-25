# Shared tools

This directory contains reusable repository helpers. Python tests live in
`tools/tests/`. The dated Multiway validation scripts, frozen runners, and
their tests are archived with the corresponding experiments in
[`experiments/multiway-2026-09/scripts/`](../experiments/multiway-2026-09/scripts/).

- `workspace_audit.py`: read-only workspace and Git hygiene report.
- `check_docs.py`: checks the canonical documentation entrypoints, local Markdown
  link destinations, and retired plan paths in current documentation.
- `plot_convergence.py`: plot solver progress files.

Run the lightweight checks with the Python standard library (Python 3.10+):

```sh
python -m unittest discover -s tools/tests -v
python tools/check_docs.py
python tools/workspace_audit.py --json
```

The documentation checker scans root Markdown files and Markdown under `docs/`,
`tools/`, `examples/`, and `crates/`. It skips fenced and inline code links,
remote URLs, and heading anchors. Historical `experiments/` documents are not
scanned, but links into them from current documentation must resolve. Research
surveys may mention old plan paths as historical text; their actual links must
still resolve. This is a file-link check, not a complete Markdown parser or a
remote-link/anchor validator. It does not duplicate task state from Linear.

Tool tests use disposable `.cache/tool-tests/` fixtures, never Cargo's `target/`.
The workspace audit reports `experiments/` separately alongside build output,
scratch runs, caches, source, and documentation; it does not delete anything.

CI runs the Python checks alongside the standard Rust verification, explicitly
compiles research feature paths, runs small sampling API tests, and exercises
CLI checkpoint/resume and daemon HTTP behavior on Windows. Expensive acceptance
tests remain in the manually dispatched acceptance workflow.

The old Multiway benchmark and cloud runners are retained with the
[`multiway-2026-09` experiment](../experiments/multiway-2026-09/README.md)
or in Git history. They are not current development commands.

Current development priorities are in the
[product roadmap](../docs/product-roadmap.jp.md).
