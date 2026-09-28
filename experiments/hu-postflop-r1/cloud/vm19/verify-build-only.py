"""Verify the fixed VM19 build proof only; execute exclusively on the recovery VM."""
import argparse
import datetime as dt
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys

sys.dont_write_bytecode = True
PACKAGE = Path('/opt/r1/flop-cpu-profile-package')
PROFILE = PACKAGE / 'experiments/hu-postflop-r1/flop-scaling/cpu-profile'
PROOF = Path('/opt/r1/flop-cpu-profile-proof01')
PINS = {
    'analyze.py': '5f7a03b4fc5bff751aa9843456a531d1c46ac0303e15ae4c55820562770d2c61',
    'run.py': 'e8c428023386f76cf3c719c467b980e7c4e36e72f408692d85fc90fe4eb1fa66',
}


def need(value, message):
    if not value:
        raise ValueError(message)


def pin(path):
    raw = path.read_bytes()
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def quiescent():
    states = {}
    for unit in ('solvers-r1-vm19-build2', 'solvers-r1-vm19-measure32'):
        result = subprocess.run(['systemctl', 'show', unit, '-p', 'ActiveState', '-p', 'MainPID', '-p', 'ControlGroup'],
                                check=True, capture_output=True, text=True, timeout=8)
        state = dict(line.split('=', 1) for line in result.stdout.splitlines() if '=' in line)
        need(state.get('ActiveState') in ('inactive', 'failed') and state.get('MainPID') == '0', 'Workload active')
        if state.get('ControlGroup'):
            group = Path('/sys/fs/cgroup') / state['ControlGroup'].lstrip('/')
            need(not any(p.read_text().strip() for p in group.rglob('cgroup.procs')), 'Workload descendants remain')
        states[unit] = state
    return states


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', type=Path, default=Path('/opt/r1/flop-cpu-profile-analysis01.json'))
    args = parser.parse_args()
    report_path = args.report
    need(sys.platform == 'linux', 'Cloud-only invocation required')
    need(report_path.is_absolute() and report_path.parent == Path('/opt/r1') and report_path.suffix == '.json', 'Report must be directly under /opt/r1')
    need(not report_path.exists(), 'Fresh report required')
    started = dt.datetime.now(dt.timezone.utc).isoformat()
    units = quiescent()
    for name, sha in PINS.items():
        need(not (PROFILE / name).is_symlink() and pin(PROFILE / name)['sha256'] == sha, 'Trusted reader/runner changed')
    spec = importlib.util.spec_from_file_location('vm19_trusted_build_reader', PROFILE / 'analyze.py')
    reader = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = reader
    spec.loader.exec_module(reader)

    class BuildEvidence(reader.Evidence):
        def __init__(self):
            need(not PROOF.is_symlink(), 'Proof root symlink')
            self.root = PROOF.resolve(strict=True)
            built = reader.read(self.root / 'build.json')
            need(built['schema'] == 'r1.cpu-profile-build/v1', 'Build schema differs')
            self.files = dict(built['files'])
            need('build.json' not in self.files, 'Recursive build inventory')
            self.files['build.json'] = reader.pin(self.root / 'build.json')
            for name in self.files:
                reader.relative(name)
            expected_directories = {'canonical'}
            for name in self.files:
                expected_directories.update(str(p) for p in PurePosixPath(name).parents if str(p) != '.')
            actual, total = {}, 0
            for path in sorted(self.root.rglob('*')):
                name = path.relative_to(self.root).as_posix()
                need(not path.is_symlink(), 'Proof symlink')
                if path.is_dir():
                    need(name in expected_directories, 'Unexpected proof directory')
                    continue
                need(path.is_file(), 'Nonregular proof member')
                total += path.stat().st_size
                need(total <= reader.run.MAX_RETAINED and len(actual) < 4096, 'Build proof size/count exceeded')
                actual[name] = reader.pin(path)
            need(actual == self.files, 'Build-only exact membership/bytes differs')
            need(not any(n in self.files for n in ('measurement.json', 'execution.json', 'retained.json')), 'Measurement evidence exists')
            self.plan = reader.read(self.root / 'plan.json')
            self.origin = PurePosixPath(self.plan['output'])
            need(self.origin.is_absolute(), 'Original absolute Linux root required')
            self.states = set()

    evidence = BuildEvidence()
    reader.verify_plan(evidence)
    binaries, execution = reader.verify_build(evidence)
    tests = execution['stages'][-1]
    record = reader.read(evidence.require(tests['record']))
    text = evidence.require(record['outputs']['stdout']).read_text()
    reader.run.validate_tests(text)
    summaries = re.findall(r'^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;.*$', text, re.MULTILINE)
    need(summaries and all(int(row[1]) == 0 for row in summaries), 'Core test summaries missing or failed')
    need(BuildEvidence().files == evidence.files, 'Proof changed during verification')
    quiescent()
    report = {
        'schema': 'r1.vm19-build-only-audit/v1', 'status': 'build_only_verified',
        'started_at_utc': started, 'ended_at_utc': dt.datetime.now(dt.timezone.utc).isoformat(),
        'scope': 'Completed two-vCPU build and core tests plus software-perf preflight; no measurement or solver-quality claim',
        'proof': str(PROOF), 'proof_files': len(evidence.files),
        'reader_pins': {name: pin(PROFILE / name) for name in PINS}, 'verifier': pin(Path(__file__)),
        'plan': evidence.files['plan.json'], 'build_receipt': evidence.files['build.json'],
        'build_execution': evidence.files['build-execution.json'], 'binaries': binaries,
        'build_host': evidence.plan['host'], 'build_stage_count': len(execution['stages']),
        'stages': [{'name': r['name'], 'process_seconds': r['process_seconds'], 'host': r['host_before']} for r in execution['stages']],
        'core_test_summaries': [dict(zip(('passed', 'failed', 'ignored', 'measured', 'filtered_out'), map(int, row))) for row in summaries],
        'required_named_regressions': list(reader.run.TEST_NAMES),
        'required_named_regressions_verified': True, 'units_before': units,
        'measurement_present': False, 'completed_solves': 0, 'performance_claim': False,
        'quality_claim': False, 'full_workspace_verification': False,
    }
    with report_path.open('x', encoding='utf-8') as stream:
        json.dump(report, stream, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    print(json.dumps(report))


if __name__ == '__main__':
    main()
