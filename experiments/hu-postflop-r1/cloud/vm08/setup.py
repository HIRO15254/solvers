"""Verify selected uploads and extract VM08 inputs into new directories only."""
import datetime
import hashlib
import json
from pathlib import Path
import shutil
import tarfile

UPLOAD = Path('/tmp/r1-vm08-upload')
ROOT = Path('/opt/r1/vm08')


def identity(path):
    data = path.read_bytes()
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def extract(archive, destination, expected, directories=()):
    destination.mkdir(exist_ok=False)
    with tarfile.open(archive, 'r:gz') as tar:
        members = tar.getmembers()
        names = [m.name for m in members]
        if len(set(names)) != len(names) or set(names) != set(expected) | set(directories):
            raise ValueError('Archive member set differs from manifest')
        for member in members:
            path = Path(member.name)
            if path.is_absolute() or '..' in path.parts:
                raise ValueError('Unsafe archive member')
            if member.name in directories:
                if not member.isdir():
                    raise ValueError('Expected a directory entry')
                (destination / member.name).mkdir(parents=True, exist_ok=True)
                continue
            if not member.isfile():
                raise ValueError('Expected a regular file')
            data = tar.extractfile(member).read()
            actual = {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
            if actual != expected[member.name]:
                raise ValueError('Archive payload identity mismatch: ' + member.name)
            target = destination / member.name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
    actual = {p.relative_to(destination).as_posix(): identity(p)
              for p in destination.rglob('*') if p.is_file()}
    if actual != expected:
        raise ValueError('Extracted file set or identity differs')


def main():
    if ROOT.exists():
        raise ValueError('Refuse an existing VM08 source directory')
    ROOT.mkdir()
    packs = ROOT / 'packs'
    packs.mkdir()
    sources = {}
    for arm in ['candidate', 'bulk', 'legacy']:
        manifest_path = UPLOAD / f'source-{arm}-manifest.json'
        manifest = json.loads(manifest_path.read_text())
        archive_path = UPLOAD / f'source-{arm}.tar.gz'
        if identity(archive_path) != {'bytes': manifest['archive_bytes'],
                                     'sha256': manifest['archive_sha256']}:
            raise ValueError('Source archive hash mismatch: ' + arm)
        expected = {f['path']: {k: f[k] for k in ['bytes', 'sha256']}
                    for f in manifest['files']}
        if len(expected) != len(manifest['files']):
            raise ValueError('Duplicate source file')
        destination = ROOT / ('source-' + arm)
        extract(archive_path, destination, expected, manifest.get('directory_entries', []))
        for path in [manifest_path, archive_path]:
            shutil.copyfile(path, packs / path.name)
        sources[arm] = {'root': str(destination), 'revision': manifest['base_commit'],
                        'manifest': str(packs / manifest_path.name),
                        'archive': str(packs / archive_path.name)}
    fixed_inputs = json.loads((UPLOAD / 'inputs.json').read_text())
    expected = {name + '.sol': {k: item[k] for k in ['bytes', 'sha256']}
                for name, item in fixed_inputs['cases'].items()}
    if set(expected) != {'river.sol', 'turn.sol', 'flop.sol'}:
        raise ValueError('Exactly three fixed inputs required')
    extract(UPLOAD / 'inputs.tar.gz', ROOT / 'inputs', expected)
    for name in ['inputs.json', 'inputs.tar.gz']:
        shutil.copyfile(UPLOAD / name, packs / name)
    (packs / 'sources.json').write_text(json.dumps(sources, indent=2) + '\n')
    record = {'created_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'sources': sources, 'input_files': expected,
              'boot_id': Path('/proc/sys/kernel/random/boot_id').read_text().strip(),
              'setup_script': identity(Path(__file__)),
              'uploaded_archives': {p.name: identity(p) for p in packs.glob('*.tar.gz')}}
    (ROOT / 'setup.json').write_text(json.dumps(record, indent=2) + '\n')
    print(json.dumps(record, indent=2))


if __name__ == '__main__':
    main()
