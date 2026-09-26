# Candidate04: four additional ignored tests

[Candidate04 execution and independent retained-byte check](report04.jp.md):
all four exact ignored tests passed once each; original evidence is retained.

This runner uses the already validated candidate04 source and its existing target.
It accepts only source manifest SHA-256
`1a84947f6daa9ca1c57d5f48e4914e176643dbe9c787262ded95dcf6aba042d6`
and archive SHA-256
`51068bb8464b57b91d11fe1ce1be848f14cd83e26865661afeeee3c70a3e9cd7`.
No frozen source, existing validation record or other campaign script is edited.
The initially prepared candidate03 version was never transferred or executed;
these pins supersede it before this additional validation's first VM run.

After candidate04 full validation completes, and while no benchmark or other build
uses the same target, launch in a bounded systemd cgroup: memory maximum 12 GiB,
swap zero, finite runtime and absolute stop before the VM deadline.

```sh
python3 -B /opt/r1/exact-control/exact-mass/extra-validation/run.py \
  --root /opt/r1/exact-new04 \
  --target /opt/r1/target/exact-new04 \
  --deadline-utc 2026-09-26T19:35:00Z
```

Keep the pinned trusted sibling `showdown-kernel/run.py` beside the deployed
`exact-mass` directory. Its Store/supervisor-evidence helpers are reused. The
supervisor is imported from candidate04 after checking its unchanged SHA. The
runner verifies the previous full validation's eight successful stage records,
source/archive/target, same boot and Rust 1.97.0 tool identities before starting.

First, a 600-second `cargo test --locked --release --no-run --message-format=json`
stage prepares only the `holdem` integration-test harnesses `postflop` and
`rake_icm`, reusing the same source/target and two Cargo jobs. Their executable
paths come from Cargo's JSON artifact messages, must lie in this release target,
and are retained with SHA-256. Each subsequent process is the identified harness
with one exact test name, `--exact --ignored --test-threads=1`:

1. `iso_quotient_matches_full_tree_per_hand`
2. `member_branch_matches_suit_permuted_rep_branch`
3. `i16_storage_matches_f32_on_small_turn_spot`
4. `pure_hu_icm_postflop_solve_matches_chip_ev`

Each test has a 600-second supervisor limit, sampled RSS 10 GiB, free-memory
minimum 1 GiB, disk reserve 4 GiB and a 20-second deadline allowance. All stages
check the host and rehash the exact source file set before and after execution.
Harnesses and tools are supervisor identity pins before/after every test. A named
`test ... ok` line and exactly `1 passed; 0 failed; 0 ignored` are required; zero
test selection cannot pass. A failure skips remaining stages without retry.
This is correctness validation, not a performance comparison.

Results are written to `root/extra-validation`: plan/result/verification,
retention index and SHA-addressed original-byte payloads. These retain the
source/archive, runner/helper, prior full-validation result and raw records,
new build and all test raw stdout/stderr/samples, and both harness binaries.
Compiler/Python executables are identity-only. A preparation failure before plan
completion still leaves a terminal result and available CAS; it does not claim a
successful full portable check.

The existing bundle tool can collect this terminal directory without duplicate
raw stages or the Cargo target:

```sh
python3 -B /opt/r1/exact-control/bundle.py \
  --out /opt/r1/exact-new04/extra-validation
```

Keep its archive/size/SHA report, transfer and rehash the archive, then extract to
`LOCAL_ROOT/extra-validation`. With these trusted scripts available locally:

```sh
python3 -B run.py --root LOCAL_ROOT --check
```

Portable verification reads retained bytes and never executes retained code.
It checks the fixed source archive/manifest, successful prerequisite records,
actual harness paths/identities, raw supervisor outputs, four positive test
summaries and recorded source-after consistency. The source-after record remains
the runner's live rehash assertion, not an independent remote filesystem snapshot.
The prerequisite check binds and retains the existing completed validation; it
does not replace that campaign's full verifier or recount all workspace tests.

`test_run.py` is lightweight Python syntax/parser testing only. It does not build
or run Rust, contact GCP, or certify the actual four-test outcome.
