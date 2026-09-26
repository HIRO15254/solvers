"""Deterministic VM08 sources and fixed SOL inputs; no credentials or Cargo outputs."""
import datetime as dt
import gzip
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import posixpath
import subprocess
import sys
import tarfile
from urllib.parse import unquote

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
OUT = Path('E:/codex-work/solvers/r1-vm08')
BULK = 'd3bbb2766e2e63d0f065f055eef1bac77016f377'
LEGACY = '88ffa5dd4583e5c8bae84e20e9b8390cb719e4f0'
BENCH = 'crates/formats/examples/sol_codec_bench.rs'
WRITER = 'crates/formats/src/sol_indexed.rs'
PREFIXES = ('.github/', 'crates/', 'docs/', 'examples/', 'tools/')
EXACT = {'.cargo/config.toml', '.gitattributes', '.gitignore', 'Cargo.toml', 'Cargo.lock',
         'README.md', 'AGENTS.md', 'CLAUDE.md', 'LICENSE', 'LICENSE-APACHE', 'LICENSE-MIT', 'LICENSE-POLICY.md'}
FIXTURE = 'experiments/hu-postflop-r1/reference/HU-R0-002/'
FIXTURE_NAMES = {'diagnostic.toml', 'observed.json', 'oop-range.txt', 'ip-range.txt',
                 'range-integrity.json', 'check_diagnostic.py', 'build_diagnostic.py',
                 'check_menus.py', 'check_ranges.py', 'README.md', 'test_check_diagnostic.py',
                 'test_check_menus.py', 'diagnostic-input-check.json'}
FIXTURE_NAMES |= {f'menu-capture-{n:02d}.{suffix}' for n in range(1, 20) for suffix in ('json', 'txt')}


def pin(data):
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def git(*args, input=None):
    return subprocess.check_output(['git', *args], cwd=ROOT, input=input)


def safe(name):
    path = PurePosixPath(name)
    assert name and not path.is_absolute() and '..' not in path.parts and '\\' not in name, name
    assert not set(path.parts) & {'.git', 'target', '.cache', 'runs', '__pycache__'}, name
    assert path.name.lower() not in {'credentials', 'credentials.db', 'credentials.json', 'credentials.toml', '.env', 'id_rsa', 'id_ed25519'}, name
    assert path.suffix.lower() not in {'.exe', '.dll', '.pdb', '.pem', '.key', '.p12'}, name


def tree(revision):
    entries = {}
    for line in git('ls-tree', '-r', '-z', revision).split(b'\0'):
        if not line:
            continue
        header, name = line.split(b'\t', 1)
        mode, kind, digest = header.decode().split()
        entries[name.decode()] = (mode, kind, digest)
    return entries


def blobs(entries, names):
    names = sorted(names)
    for name in names:
        safe(name)
        assert entries[name][0] in ('100644', '100755') and entries[name][1] == 'blob', name
    data = git('cat-file', '--batch', input=('\n'.join(entries[n][2] for n in names) + '\n').encode())
    stream, result = io.BytesIO(data), {}
    for name in names:
        header = stream.readline().decode().split()
        assert header[0] == entries[name][2] and header[1] == 'blob'
        result[name] = stream.read(int(header[2]))
        assert stream.read(1) == b'\n'
    assert not stream.read()
    return result


def current_bytes(name):
    safe(name)
    path = ROOT / name
    assert path.resolve().is_relative_to(ROOT.resolve()) and not path.is_symlink(), name
    assert not (path.lstat().st_file_attributes & 0x400), name
    return path.read_bytes()


def selection(role, revision, bench, checker):
    entries = tree(revision)
    names = {n for n in entries if n in EXACT or n.startswith(PREFIXES)}
    if 'tools/tests/test_r1_diagnostic_archive.py' in names:
        names.add('experiments/hu-postflop-r1/cloud/run-diagnostic.py')
        names.update(FIXTURE + n for n in FIXTURE_NAMES)
    get = current_bytes if role == 'candidate' else lambda n: blobs(entries, [n])[n]
    contents = {n: get(n) for n in sorted(names)} if role == 'candidate' else blobs(entries, names)
    directories, linked = set(), set()
    for name, data in list(contents.items()):
        if not name.endswith('.md') or not ('/' not in name or name.startswith(('docs/', 'tools/', 'examples/', 'crates/'))):
            continue
        for _, line in checker['prose_lines'](data.decode('utf-8-sig')):
            for target in checker['link_destinations'](line):
                if not target or target.startswith(('#', '//')) or checker['SCHEME'].match(target):
                    continue
                target = unquote(target.split('#', 1)[0].split('?', 1)[0])
                if not target:
                    continue
                resolved = posixpath.normpath(target.lstrip('/') if target.startswith('/') else posixpath.join(posixpath.dirname(name), target))
                safe(resolved)
                if resolved in contents:
                    continue
                is_file = (ROOT / resolved).is_file() if role == 'candidate' else resolved in entries
                if is_file:
                    contents[resolved] = get(resolved)
                    linked.add(resolved)
                else:
                    exists = (ROOT / resolved).is_dir() if role == 'candidate' else any(n.startswith(resolved.rstrip('/') + '/') for n in entries)
                    assert exists, f'{role}: missing actual documentation target {name} -> {resolved}'
                    directories.add(resolved)
    original_bench = contents.get(BENCH)
    contents[BENCH] = bench
    assert all(len(data) <= 8 * 1024**2 for data in contents.values()), 'large linked payload requires separate review'
    for name in contents:
        safe(name)
    return contents, directories, sorted(linked), original_bench != bench


def make_pack(label, contents, directories, revision, dirty, extra):
    root = OUT / ('inputs' if label == 'inputs' else 'source-' + label)
    archive = OUT / ('inputs.tar.gz' if label == 'inputs' else 'source-' + label + '.tar.gz')
    manifest_path = HERE / ('inputs-manifest.json' if label == 'inputs' else 'source-' + label + '-manifest.json')
    assert not root.exists() and not archive.exists() and not manifest_path.exists(), label
    root.mkdir(parents=True)
    for name in sorted(directories):
        (root / name).mkdir(parents=True, exist_ok=True)
    for name, data in sorted(contents.items()):
        target = root / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    files = [{'path': name, **pin(data)} for name, data in sorted(contents.items())]
    with archive.open('xb') as raw, gzip.GzipFile(fileobj=raw, mode='wb', filename='', mtime=0, compresslevel=6) as compressed:
        with tarfile.open(fileobj=compressed, mode='w|', format=tarfile.PAX_FORMAT) as stream:
            for name in sorted(directories):
                entry = tarfile.TarInfo(name + '/')
                entry.type, entry.mode = tarfile.DIRTYPE, 0o755
                stream.addfile(entry)
            for name, data in sorted(contents.items()):
                entry = tarfile.TarInfo(name)
                entry.size, entry.mode = len(data), 0o644
                stream.addfile(entry, io.BytesIO(data))
    manifest = {'base_commit': revision, 'dirty': dirty,
                'archive_sha256': pin(archive.read_bytes())['sha256'], 'archive_bytes': archive.stat().st_size,
                'files': files, 'directory_entries': sorted(directories), **extra}
    # Independently walk the materialized root and read the actual archive, including exact file sets.
    actual = {p.relative_to(root).as_posix(): pin(p.read_bytes()) for p in root.rglob('*') if p.is_file()}
    expected = {f['path']: {k: f[k] for k in ('bytes', 'sha256')} for f in files}
    assert actual == expected
    with tarfile.open(archive, 'r:gz') as stream:
        members = stream.getmembers()
        assert len({m.name for m in members}) == len(members)
        assert {m.name for m in members if m.isdir()} == directories
        assert {m.name: pin(stream.extractfile(m).read()) for m in members if m.isfile()} == expected
        assert all(m.isfile() or m.isdir() for m in members)
        assert all(m.mtime == m.uid == m.gid == 0 for m in members)
    manifest_path.write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
    docs = None
    if label != 'inputs':
        check = subprocess.run([sys.executable, str(root / 'tools/check_docs.py')], capture_output=True)
        docs = {'exit_code': check.returncode, 'stdout': check.stdout.decode(), 'stderr': check.stderr.decode()}
        assert check.returncode == 0, docs
        # Running the checker must not add any file to the source root.
        assert {p.relative_to(root).as_posix() for p in root.rglob('*') if p.is_file()} == set(contents)
    return {'local_root': str(root), 'archive': str(archive), 'manifest': str(manifest_path),
            'archive_bytes': manifest['archive_bytes'], 'archive_sha256': manifest['archive_sha256'],
            'file_count': len(files), 'original_bytes': sum(f['bytes'] for f in files), 'docs_check': docs,
            'exact_materialized_and_archive_file_sets': True}


def main():
    assert not (HERE / 'pack-report.json').exists(), 'do not overwrite pack evidence'
    OUT.mkdir(parents=True, exist_ok=True)
    head = git('rev-parse', 'HEAD').decode().strip()
    bench = current_bytes(BENCH)
    assert pin(bench) == {'bytes': 9370, 'sha256': 'ed972ffce351fcd31dfbab74771a06a059ef0edb43e44d65397e65f9a85cbcea'}
    assert pin(current_bytes(WRITER)) == {'bytes': 26229, 'sha256': 'f7c4488d81b619cd7a22f627b592148908ca6d359dfceaa2722876b5835b2f1f'}
    checker = {'__name__': 'pack_docs_parser', '__file__': str(ROOT / 'tools/check_docs.py')}
    exec(compile(current_bytes('tools/check_docs.py'), str(ROOT / 'tools/check_docs.py'), 'exec'), checker)
    report = {'schema': 'r1.vm08-pack-report/v1', 'created_at': dt.datetime.now(dt.timezone.utc).isoformat(),
              'packer': pin(Path(__file__).read_bytes()), 'sources': {},
              'selection': 'Tracked source/docs/examples/tools/root entrypoints, exact local documentation targets, and the 51-file HU-R0-002 Python fixture where required. No recursive experiments evidence, credentials, Cargo target, or compiled binaries.'}
    sources = {}
    for role, revision in [('candidate', head), ('bulk', BULK), ('legacy', LEGACY)]:
        contents, dirs, linked, overlay = selection(role, revision, bench, checker)
        baseline_entries = tree(revision)
        baseline = blobs(baseline_entries, [n for n in contents if n in baseline_entries])
        differences = sorted(n for n in contents if n not in baseline or contents[n] != baseline[n])
        report['sources'][role] = make_pack(role, contents, dirs, revision, bool(differences),
                                            {'role': role, 'changed_paths_against_base': differences,
                                             'benchmark_overlay': overlay, 'documentation_link_files': linked})
        if role == 'candidate':
            assert all(current_bytes(n) == data for n, data in contents.items()), 'current source changed while packing'
        remote = '/opt/r1/vm08/'
        sources[role] = {'root': remote + 'source-' + role, 'revision': revision,
                         'archive': remote + 'packs/source-' + role + '.tar.gz',
                         'manifest': remote + 'packs/source-' + role + '-manifest.json'}
        print(json.dumps({'role': role, **report['sources'][role]}), flush=True)
    retained = ROOT / 'experiments/hu-postflop-r1/codec/context-reuse/windows-debug-20260926'
    retention = json.loads((retained / 'manifest.json').read_text())
    plan_bytes = (retained / 'fresh/plan.json.gz').read_bytes()
    assert pin(plan_bytes) == {k: retention['blobs']['fresh/plan.json.gz'][k] for k in ('bytes', 'sha256')}
    plan = json.loads(gzip.decompress(plan_bytes))
    inputs, origins = {}, {}
    for case, record in plan['inputs'].items():
        data = Path(record['path']).read_bytes()
        assert pin(data) == {k: record[k] for k in ('bytes', 'sha256')}
        alias = retention['originals'][record['path']]
        compressed = (retained / alias).read_bytes()
        assert pin(compressed) == {k: retention['blobs'][alias][k] for k in ('bytes', 'sha256')}
        assert gzip.decompress(compressed) == data
        assert data[:8] == b'SLVRSOLV' and int.from_bytes(data[8:10], 'little') == 3
        inputs[case + '.sol'] = data
        origins[case] = {'original_path': record['path'], 'retained_blob': alias, 'sol_wire_version': 3, **pin(data)}
    report['inputs'] = make_pack('inputs', inputs, set(), head, False,
                                  {'role': 'fixed_inputs', 'origin_plan': 'experiments/hu-postflop-r1/codec/context-reuse/windows-debug-20260926/fresh/plan.json.gz', 'inputs': origins})
    (HERE / 'sources.json').write_text(json.dumps(sources, indent=2) + '\n', encoding='utf-8')
    (HERE / 'pack-report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({'report': str(HERE / 'pack-report.json'), 'total_archive_bytes': sum(r['archive_bytes'] for r in report['sources'].values()) + report['inputs']['archive_bytes']}))


if __name__ == '__main__':
    main()
