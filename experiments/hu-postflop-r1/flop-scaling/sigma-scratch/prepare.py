"""Prepare a pinned, two-site research patch; optionally apply to a separate source copy."""
import argparse
import difflib
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
SOURCE_REVISION = 'a0baa8bb56d1a11b4f619913518f55c548f6f80a'
SCRATCH = 'crates/engine/src/scratch.rs'
SOLVER = 'crates/engine/src/solver.rs'
STORAGE = 'crates/engine/src/storage.rs'
SOURCES = {
    SCRATCH: {'bytes': 1117, 'sha256': 'd5a5a5379b365896d94554c6b5e16ec1808e51dd605c6c0ae6dfd024fe6c5d49'},
    SOLVER: {'bytes': 56421, 'sha256': '69063b31c3433b2eb4798b2f41015311200301a7ba1887b05bbefdb936d4e81a'},
    STORAGE: {'bytes': 44287, 'sha256': '1929ce7947c6b60d9aea8d75540065c8c9a0596a3f59d82bbf8f4b6f687fd1a5'},
}
METHOD = '''    /// Pops an initialized buffer for a caller that overwrites every element.
    /// Existing elements retain their values; only a newly grown tail is zeroed.
    /// The caller must overwrite all elements before reading any of them.
    pub(crate) fn take_for_overwrite(&mut self, len: usize) -> Vec<f32> {
        let mut buf = self.free.pop().unwrap_or_default();
        buf.resize(len, 0.0);
        buf
    }

'''
INSERT_BEFORE = '    /// Returns a buffer to the pool for reuse.\n'
SIGMA = '            let mut sigma = scratch.take(sref.len());\n'
FOLLOWERS = (
    '            views.own().regret_matching(sref, sref.index, &mut sigma);',
    '            storage.regret_matching(sref, sref.index, &mut sigma);',
)


def pin(data):
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def once(source, old, new):
    if source.count(old) != 1:
        raise ValueError('Unique source anchor changed')
    return source.replace(old, new, 1)


def transform(original, tests):
    if set(original) != set(SOURCES) or any(pin(original[name]) != expected for name, expected in SOURCES.items()):
        raise ValueError('Pinned production source or storage contract changed')
    old_scratch = original[SCRATCH].decode('utf-8')
    old_solver = original[SOLVER].decode('utf-8')
    scratch = once(old_scratch, INSERT_BEFORE, METHOD + INSERT_BEFORE) + tests.decode('utf-8')
    solver = old_solver
    for follower in FOLLOWERS:
        solver = once(solver, SIGMA + follower, SIGMA.replace('.take(', '.take_for_overwrite(') + follower)
    candidates = {SCRATCH: scratch.encode(), SOLVER: solver.encode()}
    verify_scope(original, candidates, tests)
    return candidates


def verify_scope(original, candidates, tests):
    new_scratch = candidates[SCRATCH].decode('utf-8')
    appendix = tests.decode('utf-8')
    if not appendix.startswith('\n#[cfg(test)]\n') or appendix.count('    #[test]\n') != 5:
        raise ValueError('Unexpected test appendix')
    if not new_scratch.endswith(appendix):
        raise ValueError('Missing exact test appendix')
    restored_scratch = once(new_scratch[:-len(appendix)], METHOD, '')
    restored_solver = candidates[SOLVER].decode('utf-8')
    if restored_solver.count('scratch.take_for_overwrite(') != 2:
        raise ValueError('Expected exactly two overwrite call sites')
    for follower in FOLLOWERS:
        restored_solver = once(restored_solver, SIGMA.replace('.take(', '.take_for_overwrite(') + follower, SIGMA + follower)
    if restored_scratch.encode() != original[SCRATCH] or restored_solver.encode() != original[SOLVER]:
        raise ValueError('Candidate escaped the method, two sigma sites, or test appendix')


def outputs(original):
    tests = (HERE / 'tests.rs.in').read_bytes()
    candidate = transform(original, tests)
    patch = ''.join(''.join(difflib.unified_diff(
        original[name].decode().splitlines(True), candidate[name].decode().splitlines(True),
        'a/' + name, 'b/' + name)) for name in (SCRATCH, SOLVER)).encode()
    record = {
        'schema': 'r1.sigma-scratch-proposal/v1', 'source_revision': SOURCE_REVISION,
        'source': SOURCES, 'candidate': {name: pin(data) for name, data in candidate.items()},
        'patch': pin(patch),
        'controls': {name: pin((HERE / name).read_bytes()) for name in ('prepare.py', 'tests.rs.in')},
        'scope': {'new_method': 'Scratch::take_for_overwrite', 'cfr_sigma_sites': 2,
                  'all_other_production_bytes_unchanged': True, 'storage_unchanged': True,
                  'oracle_unchanged': True, 'rust_tests': 5, 'rust_tests_executed': False},
        'production_adopted': False, 'compiled': False, 'performance': None, 'full_state_quality': None,
    }
    return candidate, {'candidate.patch': patch, 'provenance.json': (json.dumps(record, indent=2) + '\n').encode()}


def read_original(root):
    root = root.resolve(strict=True)
    paths = {name: root / name for name in SOURCES}
    if any(path.is_symlink() or not path.resolve(strict=True).is_relative_to(root) for path in paths.values()):
        raise ValueError('Source path escapes the source copy')
    return {name: path.read_bytes() for name, path in paths.items()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--apply-to', type=Path)
    parser.add_argument('--receipt', type=Path)
    args = parser.parse_args()
    if args.apply_to:
        root = args.apply_to.resolve(strict=True)
        if args.check or args.receipt is None or root == ROOT.resolve() or args.receipt.exists():
            raise ValueError('Apply requires a separate source copy and a fresh receipt')
    else:
        if args.receipt is not None:
            raise ValueError('Receipt requires --apply-to')
        root = ROOT
    original = read_original(root)
    candidate, generated = outputs(original)
    if args.check or args.apply_to:
        for name, content in generated.items():
            if (HERE / name).read_bytes() != content:
                raise ValueError('Frozen artifact differs: ' + name)
    if args.apply_to:
        # Verify every input and artifact before either source file is changed.
        receipt = args.receipt.resolve()
        if not receipt.parent.is_dir() or receipt in [(root / name).resolve() for name in SOURCES]:
            raise ValueError('Receipt parent is missing or receipt overlaps a source file')
        for name, content in candidate.items():
            (root / name).write_bytes(content)
        actual = read_original(root)
        if any(actual[name] != data for name, data in candidate.items()) or actual[STORAGE] != original[STORAGE]:
            raise ValueError('Application readback differs')
        with receipt.open('x', encoding='utf-8', newline='\n') as stream:
            json.dump({'schema': 'r1.sigma-scratch-application/v1', 'source_copy': str(root),
                       'before': {name: pin(data) for name, data in original.items()},
                       'after': {name: pin(data) for name, data in actual.items()},
                       'patch': pin(generated['candidate.patch']), 'compiled': False}, stream, indent=2)
            stream.write('\n')
    elif not args.check:
        for name, content in generated.items():
            (HERE / name).write_bytes(content)
    if not args.apply_to and read_original(ROOT) != original:
        raise ValueError('Production source changed during preparation')
    print(json.dumps({'mode': 'apply' if args.apply_to else 'check' if args.check else 'prepare',
                      'candidate': {name: pin(data) for name, data in candidate.items()},
                      'compiled': False, 'performance_measured': False}, separators=(',', ':')))


if __name__ == '__main__':
    main()
