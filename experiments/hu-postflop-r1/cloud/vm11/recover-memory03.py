"""Recover the quiesced memory03 attempt; no instance mutation or retained code execution."""
from pathlib import Path
import datetime
import hashlib
import importlib.util
import json
import os
import shutil
import subprocess
import tarfile

HERE = Path(__file__).resolve().parent
BASE = HERE.parents[1]
OUT = Path('E:/codex-work/solvers/r1-memory-recovery03')
TARGET = BASE / 'focused-memory/proof03'
GCLOUD = 'C:/Program Files (x86)/Google/Cloud SDK/google-cloud-sdk/bin/gcloud.cmd'
VM = 'solvers-r1-20260926-11'
HOSTKEY = 'SHA256:d6Jt8pBawJIIqWlBiQ1OQ9kVqLPL03Z/WChjsTVawgw'
COMMON = ['--project=solvers-abstraction-20260723', '--zone=us-central1-b', '--quiet']


def save(path, value):
    with path.open('x', encoding='utf-8') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')


def command(label, argv, env):
    record = {'argv': argv, 'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat()}
    save(HERE / (label + '.intent.json'), record)
    completed = subprocess.run(argv, env=env, capture_output=True, timeout=240)
    record.update(exit_code=completed.returncode, ended_utc=datetime.datetime.now(datetime.timezone.utc).isoformat())
    for kind, raw in [('stdout', completed.stdout), ('stderr', completed.stderr)]:
        path = HERE / (label + '.' + kind + '.log')
        with path.open('xb') as stream:
            stream.write(raw)
        record[path.name] = {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}
    save(HERE / (label + '.result.json'), record)
    if completed.returncode:
        raise RuntimeError(label + ' failed; retained output, no automatic retry')
    print(label + ' completed', flush=True)


def main():
    assert not OUT.exists(), 'Recovery directory already exists'
    env = os.environ.copy()
    env.update(CLOUDSDK_PYTHON='C:/Python313/python.exe', CLOUDSDK_ENCODING='utf-8', PYTHONIOENCODING='utf-8')
    command('memory03-recovery', [GCLOUD, 'compute', 'ssh', VM, *COMMON,
            '--ssh-flag=-batch', '--ssh-flag=-hostkey', '--ssh-flag=' + HOSTKEY,
            '--command=sudo bash /tmp/recover-memory03.sh'], env)
    OUT.mkdir(parents=True, exist_ok=False)
    command('memory03-download', [GCLOUD, 'compute', 'scp', VM + ':/tmp/focused-memory03*', str(OUT), *COMMON,
            '--scp-flag=-batch', '--scp-flag=-hostkey', '--scp-flag=' + HOSTKEY], env)
    spec = importlib.util.spec_from_file_location('trusted_memory03_bundle', HERE.parent / 'bundle-final-proof.py')
    bundle = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(bundle)
    archive = OUT / 'focused-memory03.tar.gz'
    checked = bundle.check_bundle(archive)
    assert checked['retention_issue_count'] == 0, checked
    # This successful campaign's schema has its compiler stages in result.json.
    assert checked['missing_required_files'] == ['build.json'], checked
    save(OUT / 'archive-check.json', checked)
    proof = OUT / 'proof'
    proof.mkdir(exist_ok=False)
    with tarfile.open(archive, 'r:gz') as reader:
        assert all(member.isfile() for member in reader.getmembers())
        reader.extractall(proof, filter='data')
    TARGET.mkdir(parents=True, exist_ok=True)
    for path in sorted(OUT.iterdir()):
        if path.is_file():
            target = TARGET / path.name
            assert not target.exists(), str(target)
            shutil.copyfile(path, target)
    print(json.dumps(checked), flush=True)
    print('Recovered and archived-byte-verified; run trusted focused check before claiming results or cleanup.', flush=True)


if __name__ == '__main__':
    main()
