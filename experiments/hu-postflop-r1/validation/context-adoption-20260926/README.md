# Windows context-reuse adoption checks — 2026-09-26

This is historical validation of the candidate. The subsequent
[Linux release comparison](../../codec/context-reuse/linux-spot-20260926/README.md)
missed its predeclared adoption screen; context reuse was removed from production.

Production source `d3bbb2766e2e63d0f065f055eef1bac77016f377` plus the retained
dirty patch was checked in the main checkout. All 199 pinned source files matched
after execution. The only changed build source is `crates/formats/src/sol_indexed.rs`;
its exact bytes are retained, and the other source bytes are recovered from Git.
The patch also records the already-dirty architecture document.

| Stage | Cargo / supervisor exit | Elapsed seconds | Sampled process-tree peak RSS bytes |
|---|---|---:|---:|
| `cargo fmt --all --check` | 0 / 0 | 2.245437 | 83,181,568 |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | 0 / 0 | 55.761191 | 536,285,184 |
| `cargo test --locked --offline -p formats -- --test-threads=1` | 0 / **1** | 54.564993 | 474,619,904 |
| Same test command, one cleanup confirmation | 0 / 0 | 2.853144 | 98,721,792 |

Both test executions contain **79 passed, 0 failed, 0 ignored** (60 unit + 6 byte
serde + 13 integrity; 0 doctests). These are 79 distinct tests, not 158. Both logs
include the four current context-reuse tests: frame boundary equality,
large/small/large history and pledged-size isolation, `write_all` sink-error
preservation, and `finish` sink-error preservation. The initial stderr explicitly
compiles `formats` from the main checkout, not the earlier research source copy.

The initial test stage remains a supervisor **failure**: Cargo exited 0, but
descendants remained after root exit; CTRL_BREAK failed with WinError 6 at
AttachConsole, and forced Job cleanup completed. The permitted single confirmation
used the identical command and completed normally. The complete target EXE/DLL
inventory and original bytes' hashes match before/after that confirmation.

The total driver elapsed time was 131.526 seconds. Initial stages were limited to
120 seconds each, the sole confirmation to 30 seconds, and the driver to at most
450 seconds. Resource limits were a 768 MiB sampled process-tree RSS trigger,
3 GiB free host memory, 4 GiB free build-volume disk, and 5-second graceful/kill
intervals. Cargo, Rayon and test concurrency were one; DEV/TEST debug information
and incremental compilation were disabled, RUSTFLAGS was empty, and offline mode
was enforced. Actual stable toolchain executables were used with the existing
main-checkout `target/r1-local-tests` directory. Tools are identified by hashes;
compiler binaries and the reproducible Cargo target are not retained here.

This is shared-host Windows validation while another experiment was active.
RSS includes compiler/test subprocesses and console hosts; it is not writer-phase
memory or a performance comparison. Full workspace tests, release measurement,
Linux signal checks and R1 acceptance remain outside this evidence.

`manifest.json` maps 23 original files to deterministic gzip payloads under `raw/`,
including plans, source pins, source bytes, patch, binary identity inventories,
all four supervisor records, stdout/stderr and resource samples. At acquisition,
the original files remained in `E:/codex-work/solvers/r1-context-adoption-20260926`;
this task did not delete them. Gzip files preserve original bytes and both hashes.

From the repository root, run:

```text
python experiments/hu-postflop-r1/validation/context-adoption-20260926/verify.py
```

The verifier needs the base commit in local Git history, but no E: files, Rust
toolchain or network. It checks gzip/original hashes, all 199 source pins against
Git plus the retained changed file, source-after equality, exact stage commands,
tool/source identities before and after, resource limits and raw sampling,
cleanup outcomes, the one confirmation's binary equality, all 79 test results,
and all four current context-reuse test names. `verification.json` is its output.
