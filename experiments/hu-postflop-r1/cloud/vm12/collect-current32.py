"""Recover current32 bytes locally; no instance mutation or portable campaign check.

Run only after measurement writers stop. An uncertain SSH/SCP is never retried.
The remote recovery wrapper checks unit quiescence. Retained code is not imported.
"""
from __future__ import annotations
import argparse
import datetime as dt
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
BASE = HERE.parents[1]
OUT = Path('E:/codex-work/solvers/r1-current32-recovery01')
TARGET = BASE / 'current-scaling32/proof01'
GCLOUD = 'C:/Program Files (x86)/Google/Cloud SDK/google-cloud-sdk/bin/gcloud.cmd'
VM = 'solvers-r1-20260926-12'
COMMON = ['--project=solvers-abstraction-20260723', '--zone=us-central1-b', '--quiet']
ARCHIVE = 'current32-proof01.tar.gz'
DOWNLOADS = {ARCHIVE, ARCHIVE + '.manifest.json', ARCHIVE + '.sha256',
             'current32-recovery01.json', 'current32-recovery-check01.json'}
LABELS = ('current32-recovery', 'current32-download-archive', 'current32-download-records')


def require(condition, message):
    if not condition:
        raise ValueError(message)


def now():
    return dt.datetime.now(dt.timezone.utc).isoformat()


def save(path, value):
    with path.open('x', encoding='utf-8', newline='\n') as stream:
        json.dump(value, stream, indent=2, allow_nan=False)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())


def command(label, argv, env):
    record = {'argv': argv, 'started_utc': now(), 'timeout_seconds': 240}
    save(HERE / (label + '.intent.json'), record)
    try:
        completed = subprocess.run(argv, env=env, capture_output=True, timeout=240, shell=False)
        stdout, stderr, code = completed.stdout, completed.stderr, completed.returncode
    except subprocess.TimeoutExpired as error:
        stdout, stderr, code = error.stdout or b'', error.stderr or b'', None
        record['error'] = 'timeout; remote outcome uncertain; no automatic retry'
    except OSError as error:
        stdout, stderr, code = b'', b'', None
        record['error'] = repr(error)
    record.update(exit_code=code, ended_utc=now())
    for kind, raw in (('stdout', stdout), ('stderr', stderr)):
        path = HERE / (label + '.' + kind + '.log')
        with path.open('xb') as stream:
            stream.write(raw)
        record[kind] = {'path': str(path), 'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}
    save(HERE / (label + '.result.json'), record)
    require(code == 0, label + ' failed; original output retained; no automatic retry')
    print(label + ' completed', flush=True)


def retain_downloads():
    """Preserve any downloaded regular files, even when a later check fails."""
    TARGET.mkdir(parents=True, exist_ok=True)
    require(not TARGET.is_symlink(), 'retention destination must not be a symlink')
    for path in sorted(OUT.iterdir()):
        require(not path.is_symlink(), 'download symlink forbidden')
        if path.is_file():
            with path.open('rb') as source, (TARGET / path.name).open('xb') as target:
                shutil.copyfileobj(source, target)


def extract_verified(bundle, archive):
    manifest_path = Path(str(archive) + '.manifest.json')
    manifest = bundle.decode(manifest_path.read_bytes())
    expected = {row['archive_member']: {key: row[key] for key in ('bytes', 'sha256')} for row in manifest['files']}
    expected['recovery-manifest.json'] = bundle.fingerprint(manifest_path)[0]
    proof = OUT / 'proof'
    proof.mkdir(exist_ok=False)
    actual = {}
    with tarfile.open(archive, 'r:gz') as reader:
        for member in reader:
            name = bundle.safe_member(member.name)
            require(member.isfile() and name in expected and name not in actual, 'unexpected archive member during extraction')
            target = proof / name
            target.parent.mkdir(parents=True, exist_ok=True)
            digest, size = hashlib.sha256(), 0
            with reader.extractfile(member) as source, target.open('xb') as output:
                for block in iter(lambda: source.read(1024 * 1024), b''):
                    output.write(block)
                    digest.update(block)
                    size += len(block)
            actual[name] = {'bytes': size, 'sha256': digest.hexdigest()}
            require(actual[name] == expected[name], 'extracted bytes differ: ' + name)
    require(actual == expected, 'extracted file set differs')
    return {'directory': str(proof), 'regular_files': len(actual), 'status': 'exact_archive_bytes_extracted'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--hostkey', required=True, help='independently confirmed VM12 SHA256 SSH host-key fingerprint')
    args = parser.parse_args()
    require(re.fullmatch(r'SHA256:[A-Za-z0-9+/]{43}=?', args.hostkey), 'invalid SHA256 host-key fingerprint')
    require(not OUT.exists() and not OUT.is_symlink(), 'recovery directory already exists')
    if TARGET.exists():
        require(TARGET.is_dir() and not TARGET.is_symlink(), 'invalid retained proof directory')
        reserved = DOWNLOADS | {'archive-check.json', 'extraction-check.json', 'collection-error.json'}
        require(not any((TARGET / name).exists() or (TARGET / name).is_symlink() for name in reserved),
                'retained recovery file already exists')
    for label in LABELS:
        for suffix in ('intent.json', 'result.json', 'stdout.log', 'stderr.log'):
            require(not (HERE / (label + '.' + suffix)).exists(), 'command capture already exists')
    env = os.environ.copy()
    env.update(CLOUDSDK_PYTHON='C:/Python313/python.exe', CLOUDSDK_ENCODING='utf-8', PYTHONIOENCODING='utf-8')
    OUT.mkdir(parents=True, exist_ok=False)
    try:
        command(LABELS[0], [GCLOUD, 'compute', 'ssh', VM, *COMMON,
                '--ssh-flag=-batch', '--ssh-flag=-hostkey', '--ssh-flag=' + args.hostkey,
                '--command=sudo bash /opt/r1/current-deployment01/recover-current32.sh'], env)
        scp_flags = [*COMMON, '--scp-flag=-batch', '--scp-flag=-hostkey', '--scp-flag=' + args.hostkey]
        command(LABELS[1], [GCLOUD, 'compute', 'scp', VM + ':/tmp/' + ARCHIVE + '*', str(OUT), *scp_flags], env)
        command(LABELS[2], [GCLOUD, 'compute', 'scp', VM + ':/tmp/current32-recovery*.json',
                str(OUT), *scp_flags], env)
        require({p.name for p in OUT.iterdir()} == DOWNLOADS, 'downloaded file set differs')
        spec = importlib.util.spec_from_file_location('trusted_current32_recovery_bundle', HERE.parent / 'bundle-final-proof.py')
        bundle = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(bundle)
        archive = OUT / ARCHIVE
        checked = bundle.check_bundle(archive)
        save(OUT / 'archive-check.json', checked)
        save(OUT / 'extraction-check.json', extract_verified(bundle, archive))
        # Retention runs in finally even for incomplete/failed attempts. Never
        # synthesize missing campaign state or reinterpret byte integrity as success.
        require(checked['retention_issue_count'] == 0 and checked['missing_required_files'] == [],
                'recovery retained, but current32 evidence is incomplete; inspect original failure before cleanup')
    except BaseException as error:
        save(OUT / 'collection-error.json', {'time_utc': now(), 'error': repr(error),
                                            'scope': 'Collection failure only; no inferred campaign status or retry'})
        raise
    finally:
        retain_downloads()
    print(json.dumps(checked), flush=True)
    print('Original bytes recovered and extracted. Full trusted campaign verification belongs to the cleanup guard.', flush=True)


if __name__ == '__main__':
    main()
