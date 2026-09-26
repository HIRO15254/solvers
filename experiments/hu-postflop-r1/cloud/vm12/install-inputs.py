"""Verify the complete deployment package before installing fixed experiment inputs."""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import shutil
import tarfile

PACKAGE = Path('/opt/r1/current-deployment01')
BASE = Path('/opt/r1')
FOUNDATION = (8231258, 'b7361b4adc579dc87c36e8c9cd37b1c307465db90fa6432eb1bc24d67163da37')
SOURCE = (1400466, '51068bb8464b57b91d11fe1ce1be848f14cd83e26865661afeeee3c70a3e9cd7')
SOURCE_MANIFEST = (68137, '1a84947f6daa9ca1c57d5f48e4914e176643dbe9c787262ded95dcf6aba042d6')
CONTROL_NAMES = ('run.py', 'protocol.json', 'source-pins.json', 'freeze.json', 'local-checks.json', 'README.md', 'test_run.py')
EXPECTED = {*(f'controls/current-scaling32/{name}' for name in CONTROL_NAMES),
            'controls/exact-mass/run.py', 'controls/showdown-kernel/run.py',
            'exact-proof04.tar.gz', 'bundle-final-proof.py', 'install-inputs.py',
            'fetch-dependencies.sh', 'run-current32.sh', 'start-current32.py', 'recover-current32.sh'}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(raw):
    return len(raw), hashlib.sha256(raw).hexdigest()


def safe_name(name):
    require(isinstance(name, str) and name and '\\' not in name and ':' not in name
            and not any(ord(c) < 32 for c in name), 'unsafe path')
    require(not PurePosixPath(name).is_absolute() and all(x not in ('', '.', '..') for x in name.split('/')), 'unsafe path')
    return name


def inventory(root):
    require(root.is_dir() and not root.is_symlink(), 'regular directory required')
    result = set()
    for path in root.rglob('*'):
        require(not path.is_symlink(), 'symlink forbidden')
        require(path.is_dir() or path.is_file(), 'non-regular package object')
        if path.is_file():
            result.add(path.relative_to(root).as_posix())
    return result


def verify_package(root):
    names = inventory(root)
    require(names == EXPECTED | {'manifest.json'}, 'package file inventory differs')
    def pairs(rows):
        value = {}
        for key, item in rows:
            require(key not in value, 'duplicate JSON key')
            value[key] = item
        return value
    manifest = json.loads((root / 'manifest.json').read_bytes(), object_pairs_hook=pairs)
    pins = {}
    for row in manifest['files']:
        name = safe_name(row['path'])
        require(name not in pins and type(row['bytes']) is int and row['bytes'] >= 0
                and re.fullmatch('[0-9a-f]{64}', row['sha256']), 'invalid/duplicate package pin')
        pins[name] = row
    require(set(pins) == EXPECTED, 'manifest file inventory differs')
    for name, row in pins.items():
        require(digest((root / name).read_bytes()) == (row['bytes'], row['sha256']), 'package pin differs: ' + name)
    return manifest


def safe_members(archive):
    members = archive.getmembers()
    require(len(members) <= 10000 and sum(m.size for m in members) <= 512 * 1024**2, 'archive exceeds bounds')
    names = {}
    for member in members:
        name = safe_name(member.name.rstrip('/') if member.isdir() else member.name)
        require(member.isfile() or member.isdir(), 'archive links/special members forbidden')
        require(name not in names, 'duplicate archive member')
        names[name] = member
    for name, member in names.items():
        for parent in PurePosixPath(name).parents:
            if str(parent) in names:
                require(names[str(parent)].isdir(), 'archive file/directory collision')
        require(member.size >= 0, 'negative member size')
    return members


def extract(tar_path, dest):
    require(not dest.exists() and not dest.is_symlink(), 'extraction destination must be new')
    with tarfile.open(tar_path, 'r:gz') as archive:
        members = safe_members(archive)
        dest.mkdir()
        for member in members:
            path = dest / member.name.rstrip('/')
            if member.isdir():
                path.mkdir(parents=True, exist_ok=True)
            else:
                path.parent.mkdir(parents=True, exist_ok=True)
                with archive.extractfile(member) as source, path.open('xb') as output:
                    shutil.copyfileobj(source, output)


def verify_source(root):
    require(digest((root / 'source-candidate.tar.gz').read_bytes()) == SOURCE, 'source archive pin differs')
    require(digest((root / 'source-candidate-manifest.json').read_bytes()) == SOURCE_MANIFEST, 'source manifest pin differs')
    manifest = json.loads((root / 'source-candidate-manifest.json').read_bytes())
    expected = {safe_name(row['path']): (row['bytes'], row['sha256']) for row in manifest['files']}
    require(len(expected) == len(manifest['files']) == 368, 'source file count differs')
    require(inventory(root / 'source') == set(expected), 'source file inventory differs')
    for name, pin in expected.items():
        require(digest((root / 'source' / name).read_bytes()) == pin, 'source bytes differ: ' + name)


def verify_installed(package):
    manifest = verify_package(package)
    for row in manifest['files']:
        if row['path'].startswith('controls/'):
            target = BASE / 'current-control' / row['path'].removeprefix('controls/')
            require(target.is_file() and not target.is_symlink()
                    and digest(target.read_bytes()) == (row['bytes'], row['sha256']), 'installed control differs')
    verify_source(BASE / 'current32')
    return manifest


def install(package):
    manifest = verify_package(package)
    foundation = package / 'exact-proof04.tar.gz'
    require(digest(foundation.read_bytes()) == FOUNDATION, 'fixed foundation pin differs')
    # Inspect both archives completely before creating any output.
    with tarfile.open(foundation, 'r:gz') as archive:
        safe_members(archive)
        source = archive.extractfile('payload/' + SOURCE[1]).read()
        source_manifest = archive.extractfile('payload/' + SOURCE_MANIFEST[1]).read()
    require(digest(source) == SOURCE and digest(source_manifest) == SOURCE_MANIFEST, 'foundation source pins differ')
    import io
    with tarfile.open(fileobj=io.BytesIO(source), mode='r:gz') as archive:
        members = safe_members(archive)
        require(len(members) == 370 and sum(m.isfile() for m in members) == 368, 'source archive member count differs')
    outputs = [BASE / name for name in ('exact-proof04', 'current32', 'current-control', 'current32-installation01.json')]
    require(all(not p.exists() and not p.is_symlink() for p in outputs), 'installation output already exists')
    extract(foundation, outputs[0])
    outputs[1].mkdir()
    (outputs[1] / 'source-candidate.tar.gz').write_bytes(source)
    (outputs[1] / 'source-candidate-manifest.json').write_bytes(source_manifest)
    extract(outputs[1] / 'source-candidate.tar.gz', outputs[1] / 'source')
    shutil.copytree(package / 'controls', outputs[2])
    verify_installed(package)
    with outputs[3].open('x') as stream:
        json.dump({'schema': 'r1.current32-installation/v1', 'package': str(package), 'inputs': manifest,
                   'foundation_sha256': FOUNDATION[1], 'source_sha256': SOURCE[1]}, stream, indent=2)
        stream.write('\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--verify-only', action='store_true')
    args = parser.parse_args()
    if args.verify_only:
        verify_installed(PACKAGE)
    else:
        install(PACKAGE)
