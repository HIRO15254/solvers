"""Freeze selected current source (including new Rust files), never build caches."""
import argparse
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--record', type=Path, required=True)
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location('vm08_pack', ROOT / 'experiments/hu-postflop-r1/cloud/vm08/pack.py')
    pack = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(pack)
    args.record.mkdir(parents=True, exist_ok=False)
    args.out.mkdir(parents=True, exist_ok=False)
    pack.OUT, pack.HERE = args.out.resolve(), args.record.resolve()
    head = pack.git('rev-parse', 'HEAD').decode().strip()
    checker = {'__name__': 'pack_docs_parser', '__file__': str(ROOT / 'tools/check_docs.py')}
    exec(compile(pack.current_bytes('tools/check_docs.py'), str(ROOT / 'tools/check_docs.py'), 'exec'), checker)
    contents, directories, linked, overlay = pack.selection('candidate', head, pack.current_bytes(pack.BENCH), checker)
    for raw in pack.git('ls-files', '--others', '--exclude-standard', '-z').split(b'\0'):
        name = raw.decode()
        if name.startswith(pack.PREFIXES):
            contents[name] = pack.current_bytes(name)
    for campaign in (HERE, HERE.parent / 'action-scaling'):
        for path in campaign.rglob('*'):
            if path.is_file() and (path.parent == campaign and path.suffix in {'.py', '.json', '.md'} or path.parent.name == 'configs' and path.suffix == '.toml'):
                name = path.relative_to(ROOT).as_posix()
                contents[name] = pack.current_bytes(name)
    # The action runner imports these checked retention helpers before launch.
    for filename in ('retain.py', 'verify_retained.py'):
        name = 'experiments/hu-postflop-r1/codec/context-reuse/linux-spot-20260926/' + filename
        contents[name] = pack.current_bytes(name)
    entries = pack.tree(head)
    baseline = pack.blobs(entries, [name for name in contents if name in entries])
    changes = sorted(name for name in contents if contents[name] != baseline.get(name))
    result = pack.make_pack('candidate', contents, directories, head, True,
        {'role': 'range-scaling-candidate', 'changed_paths_against_base': changes,
         'documentation_link_files': linked, 'benchmark_overlay': overlay})
    assert all(pack.current_bytes(name) == data for name, data in contents.items()), 'source changed while packing'
    (args.record / 'pack-report.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
