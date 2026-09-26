"""Replay while forbidding access to the ignored pip-install report."""
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('replay_audit', HERE / 'audit.py')
audit = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = audit
spec.loader.exec_module(audit)
original_read = Path.read_bytes
ignored = (audit.REPO / '.cache/r1-policy-audit-deps.json').resolve()
attempts = []


def guarded_read(path):
    if path.resolve() == ignored:
        attempts.append(str(path))
        raise AssertionError('replay attempted to read ignored pip report')
    return original_read(path)


outputs = [HERE / name for name in ('replay-result.json', 'replay-policy.json',
                                   'replay-pins.json', 'replay-dependencies.json')]
audit.require(all(not path.exists() for path in outputs), 'replay outputs already exist')
with patch.object(Path, 'read_bytes', guarded_read):
    audit.run(*outputs, 30)

initial = json.loads((HERE / 'result.json').read_bytes())
final = json.loads((HERE / 'final-result.json').read_bytes())
replay = json.loads(outputs[0].read_bytes())
audit.require(initial['results'] == final['results'] == replay['results'], 'numeric replay differs')
audit.require(initial['counts'] == final['counts'] == replay['counts'], 'coverage replay differs')
policies = [HERE / name for name in ('decoded-policy.json', 'final-decoded-policy.json', 'replay-policy.json')]
audit.require(len({path.read_bytes() for path in policies}) == 1, 'full policy replay differs')
audit.require(json.loads(outputs[3].read_bytes())['receipt_source']['path'].endswith('/dependencies.json'),
              'replay did not use durable dependencies')
audit.write_new(HERE / 'replay-proof.json', {
    'state': 'verified', 'ignored_pip_report_read_attempts': attempts,
    'installed_versions_checked_by_auditor': True,
    'initial_final_replay_all_numeric_results_equal': True,
    'all_dense_policy_bytes_equal': True,
    'verification_script': audit.identity(Path(__file__)),
    'auditor': audit.identity(HERE / 'audit.py'),
    'outputs': [audit.identity(path) for path in outputs],
    'shared_policy_sha256': hashlib.sha256(policies[0].read_bytes()).hexdigest(),
})
print(json.dumps({'replay': 'verified', 'ignored_pip_report_reads': len(attempts)}))
