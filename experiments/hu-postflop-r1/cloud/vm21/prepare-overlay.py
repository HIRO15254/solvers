"""Pin/copy small deployment controls only; never build or create source archives."""
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
RESEARCH = 'experiments/hu-postflop-r1/flop-scaling/sparse-rank-groups/'
CLOUD = 'experiments/hu-postflop-r1/cloud/vm21/'
FILES = [RESEARCH + n for n in ('kernel.rs', 'candidate.patch', 'provenance.json', 'prepare.py',
                               'tests.rs.in', 'README.jp.md', 'run.py', 'analyze.py', 'protocol.jp.md')]
FILES += [CLOUD + n for n in ('install-overlay.py', 'prepare-overlay.py', 'protocol.jp.md',
                            'start.py', 'run.sh', 'status.py', 'prepare-dispatch.py', 'dispatch-derivation.json')]
FILES += ['experiments/hu-postflop-r1/flop-scaling/chance-grain/adapter/' + n
          for n in ('solve.rs', 'prepare.py', 'provenance.json')]
FILES += [CLOUD + n for n in ('recover.sh', 'split-on-cloud.py')]
BASE = {'bytes': 75417, 'sha256': 'c376fe25a9ba2fbf2d61f5fca71bf366223fb374b7a1ef6a20cbe2b11403d04a'}


def pin(raw):
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def main():
    manifest_path = HERE / 'overlay-manifest.json'
    staging = ROOT / '.cache/r1-vm21-overlay'
    assert not staging.exists() and not manifest_path.exists(), 'Fresh overlay required'
    assert pin((HERE.parent / 'vm19/source-manifest.json').read_bytes()) == BASE
    files = {name: (ROOT / name).read_bytes() for name in FILES}
    assert len(files) == len(FILES) and sum(map(len, files.values())) <= 4 * 1024**2
    names = {name: f'{i:02d}-' + Path(name).name for i, name in enumerate(FILES)}
    manifest = {'schema': 'r1-sparse-rank-groups-overlay/v1',
                'source_revision': '6a5545efb0bee4a4940d260b9b97a8cf841edec1', 'base_manifest': BASE,
                'files': {name: pin(raw) for name, raw in files.items()}, 'upload_names': names}
    raw_manifest = (json.dumps(manifest, indent=2) + '\n').encode()
    staging.mkdir(parents=True)
    for name, raw in files.items():
        with (staging / names[name]).open('xb') as stream:
            stream.write(raw)
    for destination in (staging / 'overlay-manifest.json', manifest_path):
        with destination.open('xb') as stream:
            stream.write(raw_manifest)
    print(json.dumps({'manifest': pin(raw_manifest), 'files': len(files),
                      'staging': str(staging), 'bytes': sum(map(len, files.values())),
                      'local_native_execution': False, 'archive_access': False}))


if __name__ == '__main__':
    main()
