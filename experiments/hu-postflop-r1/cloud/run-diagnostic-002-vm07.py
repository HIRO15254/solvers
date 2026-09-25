"""VM07-only diagnostic launcher, after successful source07 checks/builds.

Run under systemd RuntimeMaxSec=2430, MemoryMax=48G, TimeoutStopSec=15,
KillMode=control-group. The original VM deadline/budget remains unchanged.
"""
import hashlib
import importlib.util
import json
from pathlib import Path, PurePosixPath
import subprocess
import sys
import tarfile

ROOT = Path('/opt/r1')
SOURCE_SHA = 'a6d346a033cf90f68adf131c7a14f5a754417cf0d952e1008d5736e7e9d6dd2a'
INPUT_SHA = '1a03ea69897c995cc614d763767b3f4f95e0173c03bd52cdebfe6c48e8a11a19'
RUNNER_SHA = 'b6897e38e38d978b6403838c797d329be5f855fb8522e7f67218383b3c59cbf6'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    source = ROOT / 'current'
    output = ROOT / 'diagnostic-002-vm07'
    inputs = ROOT / 'diagnostic-inputs07'
    build_file = ROOT / 'codec-build07/result.json'
    build = json.loads(build_file.read_text())
    if build['status'] != 'passed' or len(build['stages']) != 8 or any(
        s['status'] != 'passed' or s['exit_code'] != 0 for s in build['stages']
    ):
        raise ValueError('All source07 checks/builds must pass before diagnostic')
    if Path('/proc/sys/kernel/random/boot_id').read_text().strip() != build['boot_id']:
        raise ValueError('Boot changed after native build')
    binary = ROOT / 'target/codec-current/release/solvers'
    audit = ROOT / 'target/codec-current/release/examples/hu_saved_profile_audit'
    for path in (binary, audit):
        entry = next(row for row in build['binaries'] if row['path'] == str(path))
        if sha(path) != entry['sha256'] or path.stat().st_size != entry['bytes']:
            raise ValueError('Built binary changed')
    archive = Path('/home/PC_User/diagnostic-002-inputs.tar.gz')
    runner = Path('/home/PC_User/run-diagnostic.py')
    if sha(ROOT / 'source-07.tar.gz') != SOURCE_SHA or sha(archive) != INPUT_SHA or sha(runner) != RUNNER_SHA:
        raise ValueError('Source/input/runner hash mismatch')
    inputs.mkdir(exist_ok=False)
    total = 0
    with tarfile.open(archive, 'r:gz') as stream:
        for member in stream:
            name = PurePosixPath(member.name)
            if not member.isfile() or len(name.parts) != 2 or name.parts[0] != 'HU-R0-002' or name.parts[1] in ('.', '..'):
                raise ValueError('Unexpected input archive member')
            total += member.size
            if member.size > 1024**2 or total > 8 * 1024**2:
                raise ValueError('Input archive exceeds bounds')
            target = inputs / name.parts[1]
            with stream.extractfile(member) as incoming, target.open('xb') as outgoing:
                outgoing.write(incoming.read())
    code = subprocess.run([
        sys.executable, str(runner), '--repo', str(source), '--case-id', 'HU-R0-002',
        '--input-dir', str(inputs), '--input-archive', str(archive),
        '--input-archive-sha256', INPUT_SHA, '--output', str(output),
        '--binary', str(binary), '--binary-sha256', sha(binary),
        '--source-id', 'source-07:' + SOURCE_SHA,
    ], check=False).returncode
    if code:
        return code
    spec = importlib.util.spec_from_file_location('diagnostic_saved_supervisor', source / 'tools/run_supervised.py')
    supervisor = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(supervisor)
    audit_dir = ROOT / 'diagnostic-002-saved-audit07'
    audit_dir.mkdir(exist_ok=False)
    binding = {key: supervisor.identity(path) for key, path in (
        ('launcher', Path(__file__)), ('build_record', build_file), ('source_archive', ROOT / 'source-07.tar.gz'),
        ('runner', runner), ('input_archive', archive), ('binary', binary), ('audit_binary', audit),
        ('execution', output / 'execution.json'), ('comparison', output / 'diagnostic-comparison.json'))}
    supervisor.atomic_json(audit_dir / 'binding.json', binding, initial=True)
    solution = output / 'run/solution.sol'
    return supervisor.main([
        '--record', str(audit_dir / 'supervisor.json'), '--stdout', str(audit_dir / 'stdout.json'),
        '--stderr', str(audit_dir / 'stderr.log'), '--cwd', str(source),
        '--timeout-seconds', '600', '--grace-seconds', '5', '--kill-wait-seconds', '5',
        '--memory-limit-bytes', str(40 * 1024**3), '--min-free-memory-bytes', str(8 * 1024**3),
        '--disk-path', '/opt/r1', '--disk-reserve-bytes', str(10 * 1024**3),
        '--identity-file', str(solution), '--identity-file', str(audit),
        '--', str(audit), '--sol', str(solution), '--threads', '1',
    ])


if __name__ == '__main__':
    raise SystemExit(main())
