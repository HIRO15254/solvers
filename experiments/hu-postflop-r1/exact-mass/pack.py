"""Freeze exact source, without caches, for the exact-mass comparison with an identical quality benchmark overlay."""
import argparse
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--role', choices=['old', 'new'], required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--record', type=Path, required=True)
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location('source_pack', ROOT / 'experiments/hu-postflop-r1/cloud/vm08/pack.py')
    pack = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(pack)
    args.record.mkdir(parents=True, exist_ok=False)
    args.out.mkdir(parents=True, exist_ok=False)
    pack.OUT, pack.HERE = args.out.resolve(), args.record.resolve()
    pack.BENCH = 'crates/cli/examples/hu_scaling_bench.rs'
    head = pack.git('rev-parse', 'HEAD').decode().strip()
    entries = pack.tree(head)
    checker = {'__name__': 'pack_docs_parser', '__file__': str(ROOT / 'tools/check_docs.py')}
    exec(compile(pack.current_bytes('tools/check_docs.py'), str(ROOT / 'tools/check_docs.py'), 'exec'), checker)
    role = 'candidate' if args.role == 'new' else 'baseline'
    bench = pack.current_bytes(pack.BENCH)
    contents, directories, linked, overlay = pack.selection(role, head, bench, checker)
    if args.role == 'new':
        for raw in pack.git('ls-files', '--others', '--exclude-standard', '-z').split(b'\0'):
            name = raw.decode()
            if name.startswith(pack.PREFIXES):
                contents[name] = pack.current_bytes(name)
    campaign = HERE.parent / 'range-scaling'
    for path in campaign.rglob('*'):
        if path.is_file() and (path.parent == campaign and path.suffix in {'.py', '.json', '.md'}
                               or path.parent.name == 'configs' and path.suffix == '.toml'):
            name = path.relative_to(ROOT).as_posix()
            contents[name] = pack.current_bytes(name)
    baseline = pack.blobs(entries, [name for name in contents if name in entries])
    changes = sorted(name for name in contents if contents[name] != baseline.get(name))
    result = pack.make_pack('candidate', contents, directories, head, bool(changes),
        {'role': 'exact-mass-' + args.role, 'changed_paths_against_base': changes,
         'documentation_link_files': linked, 'benchmark_overlay': overlay})
    if args.role == 'new':
        assert all(pack.current_bytes(name) == data for name, data in contents.items()), 'source changed while packing'
    (args.record / 'pack-report.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
