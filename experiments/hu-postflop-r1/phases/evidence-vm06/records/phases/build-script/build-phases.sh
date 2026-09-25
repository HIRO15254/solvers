#!/bin/bash
# Caller provides an exclusive finite cgroup and the unchanged VM deadline.
set -euxo pipefail
export CARGO_HOME=/opt/r1/cargo
export RUSTUP_HOME=/opt/r1/rustup
export RUSTUP_TOOLCHAIN=1.97.0
export PATH=/opt/r1/rustup/toolchains/1.97.0-x86_64-unknown-linux-gnu/bin:/opt/r1/cargo/bin:$PATH
export CARGO_BUILD_JOBS=4
export CARGO_INCREMENTAL=0
export LC_ALL=C
export R1_BUILD_SCRIPT
R1_BUILD_SCRIPT=$(realpath "${BASH_SOURCE[0]}")

phase_identity() {
python3 - "$1" <<'PY'
import datetime, hashlib, json, os, pathlib, subprocess, sys
base = pathlib.Path('/opt/r1')
out = base / 'phase-build'
mode = sys.argv[1]
def require(ok, message):
    if not ok: raise ValueError(message)
def identity(path):
    path = pathlib.Path(path).resolve(strict=True)
    data = path.read_bytes()
    return {'path': str(path), 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
def verify(item): require(identity(item['path']) == item, 'identity changed: ' + item['path'])
def read(path): return json.loads(pathlib.Path(path).read_text())
for name in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER'):
    require(not os.environ.get(name), 'unpinned compiler override: ' + name)
proof_path = base / 'comparison-build/identity-after.json'
proof = read(proof_path)
require(proof['schema'] == 'r1.comparison-build-identity/v1' and proof['state'] == 'completed', 'comparison build not completed')
require((base / 'comparison-build/complete').is_file(), 'comparison completion marker absent')
verify(proof['before_record'])
for item in [*proof['inputs'], *proof['binaries'].values()]: verify(item)
boot = pathlib.Path('/proc/sys/kernel/random/boot_id').read_text().strip()
raw = subprocess.check_output(['lscpu'], text=True)
fields = ('Architecture', 'CPU op-mode(s)', 'Vendor ID', 'Model name', 'CPU family', 'Model', 'Stepping', 'Flags')
cpu = {}
for line in raw.splitlines():
    if ':' not in line: continue
    key, value = (part.strip() for part in line.split(':', 1))
    if key in fields:
        require(key not in cpu, 'duplicate CPU field: ' + key)
        cpu[key] = sorted(value.split()) if key == 'Flags' else value
require(set(cpu) == set(fields) and cpu == proof['cpu'] and boot == proof['boot_id'], 'comparison CPU/boot changed before or during phase build')
if mode == 'before':
    for path in (out, base / 'phase-baseline', base / 'phase-candidate',
                 base / 'target/phase-baseline', base / 'target/phase-candidate', base / 'build-phases-complete'):
        require(not path.exists() and not path.is_symlink(), 'phase output must not exist: ' + str(path))
    tools = [identity(base / 'phase-tools' / name) for name in
             ('apply_instrumentation.py', 'r1_phase.rs.in', 'source-versions.json')]
    record = {'schema': 'r1.phase-build-identity/v1', 'state': 'before', 'boot_id': boot, 'cpu': cpu,
              'comparison_build': identity(proof_path), 'inputs': [identity(os.environ['R1_BUILD_SCRIPT']), *tools],
              'old_phase_targets_reused': False, 'cache_copy': 'reflink-auto; no hardlinks'}
    out.mkdir()
elif mode == 'after':
    before_path = out / 'identity-before.json'
    before = read(before_path)
    require(before['comparison_build'] == identity(proof_path), 'comparison proof changed')
    for item in before['inputs']: verify(item)
    versions = {}
    for version in ('baseline', 'candidate'):
        source = base / ('phase-' + version)
        manifest_path = source / 'r1-phase-source-manifest.json'
        manifest = read(manifest_path)
        for name, expected in manifest['after'].items():
            rel = pathlib.PurePosixPath(name)
            path = source / name
            require(not rel.is_absolute() and '..' not in rel.parts and not path.is_symlink()
                    and path.resolve().is_relative_to(source.resolve()), 'unsafe phase source path')
            require(identity(path)['sha256'] == expected, 'phase source changed: ' + name)
        versions[version] = {'binary': identity(base / 'target' / ('phase-' + version) / 'release/solvers'),
                             'source_manifest': identity(manifest_path)}
    record = {**before, 'state': 'completed', 'before_record': identity(before_path), 'versions': versions}
else:
    raise ValueError('unknown phase identity stage')
record['recorded_at_utc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
with (out / ('identity-' + mode + '.json')).open('x') as stream:
    json.dump(record, stream, indent=2, sort_keys=True)
    stream.write('\n')
PY
}

phase_identity before
mkdir /opt/r1/phase-baseline /opt/r1/phase-candidate
tar --touch -xzf /opt/r1/baseline-source.tar.gz -C /opt/r1/phase-baseline
tar --touch -xzf /opt/r1/candidate-v3-source.tar.gz -C /opt/r1/phase-candidate
python3 /opt/r1/phase-tools/apply_instrumentation.py --source baseline9632 --root /opt/r1/phase-baseline
python3 /opt/r1/phase-tools/apply_instrumentation.py --source candidate03 --root /opt/r1/phase-candidate
# Reuse only reproducible Cargo build outputs; immutable comparison binaries
# remain in their original target directories, never overwritten by this build.
cp -a --reflink=auto /opt/r1/target/baseline /opt/r1/target/phase-baseline
cp -a --reflink=auto /opt/r1/target/candidate-v3 /opt/r1/target/phase-candidate
for version in baseline candidate; do
    cd /opt/r1/phase-$version
    # Force instrumented source mtimes after the copied Cargo outputs.
    python3 - <<'PY'
import json, os, pathlib
root = pathlib.Path.cwd()
for name in json.loads((root / 'r1-phase-source-manifest.json').read_text())['after']:
    path = root / name
    if path.is_symlink() or not path.resolve().is_relative_to(root):
        raise ValueError('unsafe phase source mtime path')
    os.utime(path, None)
PY
    export CARGO_TARGET_DIR=/opt/r1/target/phase-$version
    {
        rustc -Vv
        cargo -V
        lscpu
        sha256sum Cargo.lock .cargo/config.toml r1-phase-source-manifest.json
        /usr/bin/time -v cargo build --locked --release -p cli --bin solvers
        sha256sum /opt/r1/target/phase-$version/release/solvers
        date -u --iso-8601=seconds
    } > /opt/r1/phase-$version-build.log 2>&1
done
phase_identity after
date -u --iso-8601=seconds > /opt/r1/build-phases-complete
