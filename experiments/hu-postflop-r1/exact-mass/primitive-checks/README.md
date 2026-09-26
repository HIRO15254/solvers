# Candidate04 isolated mass checks

The final `mass.rs` / `mass_tests.rs` passed an isolated Windows Rust test build,
all **19 tests**, and standalone Clippy with `-D warnings`. The test harness
reported 0 failed and 0 ignored, in 0.37 seconds. This is primitive correctness
evidence; full workspace/Linux validation and performance measurements are
separate campaigns.

`success/` retains the original supervisor records, stdout, stderr, samples and
invocation. Each stage had a 45-second timeout, 300,000,000-byte sampled RSS limit,
3 GiB free-memory minimum and 1 GiB disk reserve. The launcher requested one CPU
and the harness used one test thread. The records preserve original absolute
paths, timestamps, source/tool/binary identities and successful cleanup.

`manifest.json` hashes every retained raw file. `mass-tests.exe.gz` expands to the
exact binary identified before and after the successful test process. Source
files are not duplicated: their pins match the separately frozen
[candidate04 manifest](../source-new04/source-candidate-manifest.json), whose
archive SHA is `51068bb8464b57b91d11fe1ce1be848f14cd83e26865661afeeee3c70a3e9cd7`.
Compiler/Python executables and dependent DLLs are not bundled; the recorded
executable identities remain available for provenance. `verification.json`
records the collection-time checks of raw hashes, successful identity equality,
sample counts/peak/last empty containment, source pins and decompressed binary.
Retained raw payloads total 411,468 bytes; no PDB, build cache or duplicate source
snapshot is included.

`failed-compile/` is a separate earlier attempt. Its isolated `-D warnings` build
rejected `MassScale::WIDE` as unused; no tests ran. The final tests explicitly
check zero-scale equality to `WIDE`, and the successful rerun also includes the
requested raw-bit extrema optimization. This failure is not counted among the
19 successful tests. Before that compilation, a launcher affinity call used an
incorrect ctypes signature and stopped before creating a child; it produced no
Rust or supervisor result and is not a test result.

To recheck the retained file and binary bytes without executing retained code,
run Python from this directory:

```python
import gzip, hashlib, json
from pathlib import Path
m = json.loads(Path("manifest.json").read_text())
for row in m["raw_files"]:
    data = Path(row["path"]).read_bytes()
    assert len(data) == row["bytes"]
    assert hashlib.sha256(data).hexdigest() == row["sha256"]
data = gzip.decompress(Path(m["binary"]["retained_path"]).read_bytes())
pin = m["binary"]["original_pin"]
assert len(data) == pin["bytes"]
assert hashlib.sha256(data).hexdigest() == pin["sha256"]
```
