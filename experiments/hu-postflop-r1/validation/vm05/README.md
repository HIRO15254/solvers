# Snapshot 02: required code checks

On 2026-09-25, GCP `solvers-r1-20260925-05` in `us-central1-b` ran the
[fixed commands](checks.json) using Rust 1.97.0, 4 Cargo build jobs and
2 test threads on an e2-highmem-8 Ubuntu 24.04 Spot VM.

The tested source is [current-02.tar.gz](../sources/current-02.tar.gz), SHA-256
`5edec6bea4ce887847c3430b3c45e5c5251402f2780f3289e95409e40b4b8fc6`, with
[per-file hashes](../updated/source-manifest.json). Its base commit is `9632d8b`;
it includes the dirty source rather than claiming that HEAD alone identifies the build.
It was extracted with `tar --touch` into a fresh target environment.

| Check | Result | Evidence |
|---|---|---|
| Toolchain | Rust 1.97.0 | [00.log](00.log) |
| cargo fmt --all --check | pass | [01.log](01.log) |
| cargo clippy --locked --workspace --all-targets -- -D warnings | pass | [02.log](02.log) |
| cargo test --locked --workspace -- --test-threads=2 | 891 passed, 0 failed, 31 ignored | [03.log](03.log) |
| Python tools/tests | 32 tests, 3 Windows-only skipped, pass | [04.log](04.log) |

All five downloaded log hashes were checked against `checks.json`. These are code
regression checks, not external-reference quality or performance certification.
Ignored tests are outside this execution; selected additional checks are described in
the [validation scope](../README.md).

Earlier attempts on VM `-02` are not substituted for these logs. Its snapshot 01
test fixture failed before solving because it omitted the tree kind. A later attempt
reused stale Cargo outputs because an archive had fixed old mtimes. Both were corrected;
that VM was then preempted and its uncollected raw logs were lost. The initial source
archive is retained for provenance, but the successful evidence above comes from VM `-05`.
