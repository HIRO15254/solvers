#!/bin/bash
# Execute after audit recovery completes, in an exclusive finite systemd cgroup.
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

comparison_identity() {
python3 - "$1" <<'PY'
import datetime, hashlib, json, os, pathlib, subprocess, sys, tarfile
base = pathlib.Path('/opt/r1')
out = base / 'comparison-build'
mode = sys.argv[1]
def require(ok, message):
    if not ok: raise ValueError(message)
def identity(path):
    path = pathlib.Path(path).resolve(strict=True)
    data = path.read_bytes()
    return {'path': str(path), 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
def verify(item):
    require(identity(item['path']) == item, 'identity changed: ' + item['path'])
def read(path): return json.loads(pathlib.Path(path).read_text())
def cpu():
    raw = subprocess.check_output(['lscpu'], text=True)
    fields = ('Architecture', 'CPU op-mode(s)', 'Vendor ID', 'Model name', 'CPU family', 'Model', 'Stepping', 'Flags')
    result = {}
    for line in raw.splitlines():
        if ':' not in line: continue
        key, value = (part.strip() for part in line.split(':', 1))
        if key in fields:
            require(key not in result, 'duplicate CPU field: ' + key)
            result[key] = sorted(value.split()) if key == 'Flags' else value
    require(set(result) == set(fields), 'incomplete CPU identity')
    return result
for name in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER'):
    require(not os.environ.get(name), 'unpinned compiler override: ' + name)
boot = pathlib.Path('/proc/sys/kernel/random/boot_id').read_text().strip()
current_cpu = cpu()
if mode == 'before':
    for path in (out, base / 'target/baseline', base / 'target/candidate-v3'):
        require(not path.exists() and not path.is_symlink(), 'output must not exist: ' + str(path))
    completion = read(base / 'audit-pair-recovery/complete.json')
    require(completion['status'] == 'completed', 'audit recovery incomplete')
    verify(completion['result'])
    recovery = read(completion['result']['path'])
    require(recovery['status'] == 'completed' and recovery['boot_id'] == boot, 'recovery boot/status mismatch')
    require(recovery['recovery']['native_cpu_compatibility']['current'] == current_cpu, 'recovery CPU changed')
    inputs = [identity(os.environ['R1_BUILD_SCRIPT']), identity(base / 'audit-pair-recovery/complete.json'),
              completion['result'], recovery['identities']['current_example'],
              *recovery['identities']['compiler_binaries']]
    sources = {}
    for name, expected in [('baseline', 'fdd8c1c014a94c70b56efbd79a36c18f6f1634c20aaf9062660ee3a6133a0197'),
                           ('candidate-v3', 'ac5d493a129cd97be9322598867fa3ab5795ba6d3241a93e2719beaa2dd89970')]:
        item = identity(base / (name + '-source.tar.gz'))
        require(item['sha256'] == expected, 'unexpected source archive: ' + name)
        sources[name] = item
        inputs.append(item)
    for item in inputs: verify(item)
    for name in ('deps', 'build', '.fingerprint'):
        path = base / 'target/current-recovery/release' / name
        require(path.is_dir() and not path.is_symlink(), 'invalid fresh cache seed: ' + str(path))
    record = {'schema': 'r1.comparison-build-identity/v1', 'state': 'before', 'boot_id': boot,
              'cpu': current_cpu, 'inputs': inputs, 'sources': sources,
              'cache_seed': '/opt/r1/target/current-recovery/release',
              'old_targets_reused': False, 'compiler_binaries': recovery['identities']['compiler_binaries']}
    out.mkdir()
elif mode == 'after':
    before_path = out / 'identity-before.json'
    before = read(before_path)
    require(before['boot_id'] == boot and before['cpu'] == current_cpu, 'CPU/boot changed during comparison build')
    for item in before['inputs']: verify(item)
    binaries, contents = {}, {}
    for version, archive in before['sources'].items():
        source = base / version
        inventory = {}
        with tarfile.open(archive['path']) as tar:
            for member in tar.getmembers():
                if member.isdir(): continue
                rel = pathlib.PurePosixPath(member.name)
                require(member.isfile() and not rel.is_absolute() and '..' not in rel.parts, 'unsafe source member')
                path = source / member.name
                require(not path.is_symlink() and path.resolve().is_relative_to(source.resolve()), 'unsafe source path')
                expected = tar.extractfile(member).read()
                require(path.read_bytes() == expected, 'extracted source changed: ' + str(path))
                inventory[member.name] = {'bytes': len(expected), 'sha256': hashlib.sha256(expected).hexdigest()}
        contents[version] = inventory
        binaries[version] = identity(base / 'target' / version / 'release/solvers')
    record = {**before, 'state': 'completed', 'before_record': identity(before_path),
              'binaries': binaries, 'source_contents': contents}
else:
    raise ValueError('unknown identity stage')
record['recorded_at_utc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
with (out / ('identity-' + mode + '.json')).open('x') as stream:
    json.dump(record, stream, indent=2, sort_keys=True)
    stream.write('\n')
PY
}

comparison_identity before
for version in baseline candidate-v3; do
    mkdir /opt/r1/target/$version
    mkdir /opt/r1/target/$version/release
    cp -a --reflink=auto /opt/r1/target/current-recovery/release/deps /opt/r1/target/current-recovery/release/build /opt/r1/target/current-recovery/release/.fingerprint /opt/r1/target/$version/release/
    # Source mtimes must follow copied workspace outputs. Archive content is fixed.
    tar --touch -xzf /opt/r1/$version-source.tar.gz -C /opt/r1/$version
    export CARGO_TARGET_DIR=/opt/r1/target/$version
    cd /opt/r1/$version
    {
        rustc -Vv
        cargo -V
        uname -a
        lscpu
        sha256sum Cargo.lock .cargo/config.toml /opt/r1/$version-source.tar.gz
        /usr/bin/time -v cargo build --locked --release -p cli --bin solvers
        sha256sum /opt/r1/target/$version/release/solvers
        date -u --iso-8601=seconds
    } > /opt/r1/comparison-build/$version.log 2>&1
done
comparison_identity after
date -u --iso-8601=seconds > /opt/r1/comparison-build/complete
