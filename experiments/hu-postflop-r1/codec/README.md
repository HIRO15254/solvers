# SOL v3 byte-serde codec experiment

This page specifies the protocol frozen before VM07 measurement. The measured
results are recorded separately in [vm07-report.md](vm07-report.md), including
the Flop writer's failed improvement gate. The protocol compares
the unmodified codec at `88ffa5dd4583e5c8bae84e20e9b8390cb719e4f0` with the candidate
byte-serde implementation on the **same three retained source06 Full SOL files**.
It does not run CFR, change solve iterations, evaluate saved BR, or certify R1.
The source06 artifact metadata remains a pre-save snapshot of that earlier solve.

[inputs.json](inputs.json) pins the original candidate repetition-1 River, Turn
and Flop artifacts from `vm06-current.tar.gz`, including the bundle, sidecar,
member, original VM path, size and SHA-256. These are not source03 artifacts,
baseline v1 files, or zero-step restore outputs. `prepare` reads only these three
members, checks their retained identities and v3 headers, and creates new local
files. It neither extracts arbitrary archive paths nor claims a fresh audit of
all 943 bundle payloads. The complete retained evidence audit is elsewhere in
the campaign. Expanded files are not another cloud transfer.

## One identical example in both source trees

The research example is
[sol_codec_bench.rs](../../../../crates/formats/examples/sol_codec_bench.rs).
No new dependency or public codec API is needed. The experiment uses:

- `read_sol`: metadata/directory validation and every stored chunk validation.
- `SolReader::open`, `metadata`, `stored_srefs`, `read_node(0)`: public indexed reads.
- `write_sol`: existing bounded-group writer, including its flush/sync/atomic persist.

Create a clean baseline checkout/archive at the exact commit above. Copy **only**
this example to `crates/formats/examples/sol_codec_bench.rs` there. Check that the
only source difference from that checkout is the example; do not copy the
candidate `sol.rs`, `sol_indexed.rs`, Cargo files or tests. Check the example's
SHA-256 in both trees. Build both with the same Rust toolchain, release profile,
target and `RUSTFLAGS` on the same CPU and boot. Use different fresh target
directories; never reuse `target-cpu=native` output across a CPU change.
This revision pins the baseline Git archive SHA-256
`3de4afef13082dca9e17c38c9cc5f5d1734855f017d812ecb04e9cdbe0f7da27` and
candidate source07 archive
`a6d346a033cf90f68adf131c7a14f5a754417cf0d952e1008d5736e7e9d6dd2a`.
The candidate archive includes the frozen example; the baseline archive precedes
the example-only copy. A different candidate requires an explicit new source pin
and plan before measurement, rather than silently replacing the frozen source.

```text
cargo build --release --locked -p formats --example sol_codec_bench
```

The root task owns build supervision, required workspace checks, source archives,
compiler identities and cloud lifecycle. This runner never builds or starts a VM.
Before freeze, provide a separate `r1.codec-build-attestation/v1` JSON with this shape (all FileRefs are
actual absolute paths with `bytes` and `sha256`, not placeholders):

```json
{
  "schema": "r1.codec-build-attestation/v1",
  "status": "completed",
  "settings": {"profile": "release", "rustflags": "same for both", "target": "same target"},
  "host": {"copy": "exact run_codec.host() output on the build host"},
  "compiler": {"path": "/path/to/actual/rustc", "bytes": 1, "sha256": "..."},
  "example": {"path": "/source/crates/formats/examples/sol_codec_bench.rs", "bytes": 1, "sha256": "..."},
  "validation_file": {"path": "/codec-build/result.json", "bytes": 1, "sha256": "..."},
  "validation_stages": [{"path": "/codec-build/00-toolchain/supervisor.json", "bytes": 1, "sha256": "..."}],
  "sides": {
    "baseline": {"revision": "88ffa5dd4583e5c8bae84e20e9b8390cb719e4f0", "source": {"path": "/baseline-source.tar.gz", "bytes": 1, "sha256": "..."}, "binary": {"path": "/baseline-target/release/examples/sol_codec_bench", "bytes": 1, "sha256": "..."}},
    "candidate": {"revision": "40-character repository HEAD; archive also pins dirty changes", "source": {"path": "/candidate-source.tar.gz", "bytes": 1, "sha256": "..."}, "binary": {"path": "/candidate-target/release/examples/sol_codec_bench", "bytes": 1, "sha256": "..."}}
  }
}
```

`validation_stages` must contain all eight supervisor FileRefs in order; the example
above abbreviates that list. `validation_file` points to the unchanged
`cloud/validate-codec.py` result, schema `r1.codec-build/v1`, status `passed`.
The required stages are toolchain, fmt, clippy, workspace-tests, python-tools,
release-cli, release-codec-current, release-codec-baseline. The inspector requires
all eight completed supervisor records, zero child/supervisor exits, unchanged
inputs and cleanup, and rehashes their pinned logs/identities. It also rehashes the
full source manifests, checks the identical example in each, and binds selected
binary paths to their respective release stage and retained binary identities.

The producer must retain its exact commands, successful supervised build records,
and source-file manifests supporting that declaration. The runner rehashes these
referenced source archives/compiler/example/binaries; it does **not** independently
prove that a compiler produced a binary from an archive. Build provenance review
remains necessary. One shared settings object is an assertion that both commands
used those settings, not a substitute for their logs.

## Frozen operations and equality gates

Each case/operation gets one excluded warmup for each binary, then three measured
pairs in baseline/candidate, candidate/baseline, baseline/candidate order. Cases
run River, Turn, Flop; operations run in the table order. Every invocation is a
new supervised process. Input SHA-256 is checked before launch; the example hashes
it with BLAKE3 before the timer and again afterwards. This deliberately measures
**warm input files**, with no OS cache eviction. No adaptive iteration selection,
failed-sample replacement, retries, or fastest-sample selection is allowed.

| Operation | Timed work | Iterations |
|---|---|---:|
| `decode-all` | One normal `read_sol`, including open/metadata/all chunks | 1 |
| `read-root` | `open_seconds + operation_seconds` for fresh reader + node 0 | 1 |
| `read-repeat-chunk` | 64 node-0 reads on one reader, excluding separately recorded open | 64 |
| `stream-write` | `write_sol` from resident decoded payload, including sync/persist | 1 |

The repeated operation intentionally re-decodes the same root chunk; it does not
claim a cache or require access to the private directory. The stream writer is
the public bounded-group implementation, **not** a new streaming input API. Its
full preload is separately timed and excluded from writer time. Partial-read
processes do not preload the full payload. Repeated-read timing includes disposal
of prior returned pairs; it excludes canonical output/hash work. Three pairs are
small descriptive evidence, not a statistical confidence claim.

Outside timing, the example emits a canonical fixed-width binary representation
independent of serde/postcard: original config bytes, every metadata field with
f64 bits, mode/counts, ordered srefs, every raw strategy byte, value-scale f32 bits,
and every raw value byte. Full operations also emit their root subset in the same
encoding as partial operations. The wrapper requires direct byte equality and
SHA-256/size equality across both binaries, all repetitions and all applicable
operations; `stream-write` output must be exactly the original SOL bytes and must
pass another normal full read. Thus both changing to the same wrong partial node
and metadata-only equality cannot satisfy the full/partial cross-check.

Hard correctness gate: **zero differing bytes**, all 96 invocations complete,
supervisor cleanup/identity checks succeed, sources/inputs remain unchanged,
and CPU/boot stay fixed. Incomplete/failed runs cannot publish comparisons.
Per case/operation, a descriptive improvement claim additionally requires the
candidate median/baseline median ratio to be **at most 0.95**, with at least **2
of 3** paired timings strictly faster. This conservative reporting rule is fixed
before new measurement, is not a calibrated R1 performance target, and does not
turn a miss into a codec correctness failure. All raw pairs and ratios remain
visible. No aggregate success or end-to-end solve improvement is inferred.

Supervisor peak memory/wall time include input hashing, canonical output and
validation (including write/readback). They are retained process measurements,
not codec-phase peak estimates. The example's timers separate load/open/work;
its validation-output timer covers canonical/hash work, not the writer readback.
There is no phase-specific memory claim.

## Run after builds and checks

```text
python experiments/hu-postflop-r1/codec/run_codec.py prepare --bundle runs/r1-cloud/vm06-current.tar.gz --sidecar runs/r1-cloud/vm06-current.tar.gz.manifest.json --out runs/codec-inputs
python experiments/hu-postflop-r1/codec/run_codec.py freeze --inputs runs/codec-inputs --source-root /path/to/frozen-source07 --build-record /path/to/codec-build-attestation.json --out /path/to/new-codec-plan.json
python experiments/hu-postflop-r1/codec/run_codec.py run --plan /path/to/new-codec-plan.json --out /path/to/new-codec-run
python experiments/hu-postflop-r1/codec/run_codec.py check --run /path/to/new-codec-run
python experiments/hu-postflop-r1/codec/test_codec.py -v
```

Freeze must run on the measurement host after successful builds and before any
sample. It pins the plan/threshold hash, runner/example/supervisor/Python, source
archives/binaries/compiler/build evidence and exact input bytes. Defaults are
120 seconds per process, 4 GiB memory limit, 2 GiB minimum free memory and disk
reserve, with a 30-minute campaign deadline. Root must provide the outer Linux
cgroup and overall cloud deadline. If any invocation fails, retain the failed
state and logs; changing inputs/settings/protocol requires a new prospective plan.
Outputs and plans are new-only; campaign progress uses atomic replacement.

The updated runner package can live outside the frozen candidate source tree;
`--source-root` selects that tree's example and supervisor without changing its
source manifest. Every retained sample is bound to the frozen side's executable,
input, exact operation/iteration/output path, supervisor identities and outputs.
The checker also rejects failed or mismatched report schemas, versions, modes and
selections, and rechecks rewritten bytes against the original input. Changing
baseline/candidate labels cannot reverse the reported ratio.

`check` verifies the plan self-digest and fixed protocol, rehashes the same frozen
build/source/compiler/binary/runner/input identities, and recomputes all equality
and reporting gates. It does not require the *current* CPU or boot to match, but
does require all recorded absolute paths to remain accessible. It does not itself
map a Linux retained bundle to Windows paths or verify a compact-only export;
the campaign's separate retained-bundle verifier must provide that mapping and
availability audit. Missing raw canonical files cannot receive a byte-exact pass.

The protocol and its Python tests alone are not optimization evidence. The tests
exercise synthetic acceptance records and negative controls. The separate VM07
report records actual measurements and retained-byte verification within this scope.
