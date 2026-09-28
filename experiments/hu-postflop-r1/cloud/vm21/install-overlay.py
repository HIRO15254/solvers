"""Install a caller-pinned small overlay over the unchanged VM19 source package."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath

BASE_MANIFEST = {'bytes': 75417, 'sha256': 'c376fe25a9ba2fbf2d61f5fca71bf366223fb374b7a1ef6a20cbe2b11403d04a'}
BASE_ARCHIVE = 'ad156105b5323c91c4c9e715525e9efdf75ec07acb193db3eb3106581fec466f'
REVISION = '6a5545efb0bee4a4940d260b9b97a8cf841edec1'
SCHEMA = 'r1-sparse-rank-groups-overlay/v1'


def need(value, message):
    if not value:
        raise ValueError(message)


def pin(raw):
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def relative(name):
    path = PurePosixPath(name)
    need(path.parts and not path.is_absolute() and '..' not in path.parts and str(path) == name
         and '\\' not in name and ':' not in name, 'Unsafe member name')
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package', type=Path, required=True)
    parser.add_argument('--incoming', type=Path, required=True)
    parser.add_argument('--manifest-sha256', required=True)
    args = parser.parse_args()
    need(not args.package.is_symlink() and not args.incoming.is_symlink(), 'Symlink root')
    package, incoming = args.package.resolve(strict=True), args.incoming.resolve(strict=True)
    need(package != incoming and not incoming.is_relative_to(package), 'Incoming must be separate')
    manifest_raw = (incoming / 'overlay-manifest.json').read_bytes()
    need(len(manifest_raw) <= 256 * 1024 and pin(manifest_raw)['sha256'] == args.manifest_sha256,
         'Caller manifest pin differs')
    overlay = json.loads(manifest_raw)
    need(overlay['schema'] == SCHEMA and overlay['source_revision'] == REVISION
         and overlay['base_manifest'] == BASE_MANIFEST, 'Overlay provenance differs')
    base_raw = (package / 'manifest.json').read_bytes()
    need(pin(base_raw) == BASE_MANIFEST, 'Old source manifest changed')
    base = json.loads(base_raw)
    installation = json.loads((package / 'installation.json').read_bytes())
    need(installation['manifest'] == BASE_MANIFEST and installation['archive_sha256'] == BASE_ARCHIVE
         and installation['source_revision'] == REVISION and installation['destination'] == str(package)
         and installation['source_files'] == len(base['source_pins'])
         and installation['builds_or_solves_started'] == 0, 'Old installation identity differs')
    # Only small source/control files are read. No state or native binary exists here.
    for name, identity in base['files'].items():
        path = package / relative(name)
        need(not path.is_symlink() and path.resolve().is_relative_to(package) and pin(path.read_bytes()) == identity,
             'Old package bytes changed: ' + name)
    files, names = overlay['files'], overlay['upload_names']
    need(0 < len(files) <= 128 and set(files) == set(names) and len(set(names.values())) == len(names),
         'Overlay member mapping differs')
    pending = {}
    for name, identity in files.items():
        rel = relative(name)
        need(str(rel).startswith('experiments/hu-postflop-r1/'), 'Overlay outside research namespace')
        upload = relative(names[name])
        need(len(upload.parts) == 1 and not (incoming / upload).is_symlink(), 'Unsafe upload member')
        destination = package / rel
        need(name not in base['files'] and not destination.exists()
             and destination.resolve().is_relative_to(package), 'Overlay collision/path escape')
        raw = (incoming / upload).read_bytes()
        need(pin(raw) == identity and len(raw) <= 1024**2, 'Overlay bytes differ')
        pending[name] = raw
    need(sum(map(len, pending.values())) <= 4 * 1024**2, 'Overlay exceeds bound')
    own = 'experiments/hu-postflop-r1/cloud/vm21/install-overlay.py'
    need(own in pending and pin(Path(__file__).read_bytes()) == files[own], 'Running installer not pinned')
    need(not (package / 'overlay-manifest.json').exists()
         and not (package / 'overlay-installation.json').exists(), 'Overlay already installed')
    for name, raw in pending.items():
        destination = package / relative(name)
        destination.parent.mkdir(parents=True, exist_ok=True)
        with destination.open('xb') as stream:
            stream.write(raw)
        need(pin(destination.read_bytes()) == files[name], 'Installation readback differs')
    with (package / 'overlay-manifest.json').open('xb') as stream:
        stream.write(manifest_raw)
    receipt = {'schema': 'r1-sparse-rank-groups-overlay-installation/v1',
               'manifest': pin(manifest_raw), 'base_manifest': BASE_MANIFEST,
               'destination': str(package), 'source_revision': REVISION,
               'builds_or_solves_started': 0, 'source_files': len(base['source_pins']),
               'overlay_files': len(files), 'base_archive_sha256': BASE_ARCHIVE}
    with (package / 'overlay-installation.json').open('x') as stream:
        json.dump(receipt, stream, indent=2)
        stream.write('\n')
    print(json.dumps(receipt))


if __name__ == '__main__':
    main()
