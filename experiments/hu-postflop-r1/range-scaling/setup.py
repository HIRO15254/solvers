"""Extract one verified immutable source pack into a new VM directory."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import shutil
import tarfile


def pin(data):
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--upload', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    manifest_path = args.upload / 'source-candidate-manifest.json'
    archive_path = args.upload / 'source-candidate.tar.gz'
    manifest = json.loads(manifest_path.read_text())
    assert pin(archive_path.read_bytes()) == {'bytes': manifest['archive_bytes'], 'sha256': manifest['archive_sha256']}
    expected = {row['path']: {key: row[key] for key in ('bytes', 'sha256')} for row in manifest['files']}
    assert len(expected) == len(manifest['files'])
    directories = set(manifest['directory_entries'])
    args.out.mkdir(parents=True, exist_ok=False)
    source = args.out / 'source'
    source.mkdir()
    with tarfile.open(archive_path, 'r:gz') as archive:
        members = archive.getmembers()
        assert len({m.name for m in members}) == len(members)
        assert {m.name for m in members} == set(expected) | directories
        for member in members:
            path = PurePosixPath(member.name)
            assert not path.is_absolute() and '..' not in path.parts and '\\' not in member.name
            target = source / member.name
            if member.name in directories:
                assert member.isdir()
                target.mkdir(parents=True, exist_ok=True)
            else:
                assert member.isfile()
                data = archive.extractfile(member).read()
                assert pin(data) == expected[member.name]
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(data)
    assert {p.relative_to(source).as_posix(): pin(p.read_bytes()) for p in source.rglob('*') if p.is_file()} == expected
    for path in (manifest_path, archive_path):
        shutil.copyfile(path, args.out / path.name)
    record = {'source': str(source), 'source_archive': pin(archive_path.read_bytes()),
              'source_manifest': pin(manifest_path.read_bytes()), 'setup_script': pin(Path(__file__).read_bytes()),
              'boot_id': Path('/proc/sys/kernel/random/boot_id').read_text().strip()}
    (args.out / 'setup.json').write_text(json.dumps(record, indent=2) + '\n')
    print(json.dumps(record))


if __name__ == '__main__':
    main()
