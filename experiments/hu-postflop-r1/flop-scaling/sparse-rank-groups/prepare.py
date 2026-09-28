"""Prepare/check a small exact research patch, or apply it to a separate pinned source copy."""
import argparse
import difflib
import hashlib
import json
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
SOURCE = 'crates/holdem/src/kernel.rs'
SOURCE_SHA = 'd59724dd4d9a1c797bebf83422c06b75c90020796bfbf8f492417910770bb076'
START = 'pub(crate) fn showdown_kernel_compact('
END = '\n/// Fold evaluation with seat-local reach and CFV indices.'


def pin(data):
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def once(source, old, new):
    if source.count(old) != 1:
        raise ValueError('Unique source anchor changed')
    return source.replace(old, new, 1)


def transform(original):
    if pin(original)['sha256'] != SOURCE_SHA:
        raise ValueError('Original production kernel changed')
    text = original.decode()
    start, end = text.index(START), text.index(END)
    before = text[start:end]
    after = once(before, '    let mut below_card = [0.0; 52];\n',
                 '    let mut below_card = [0.0; 52];\n    let mut group_card = [0.0; 52];\n')
    after = once(after, '        let (group_total, group_card) = compact_compat_sums(group, opponent, opp_reach);',
                 '''        // Keep one scratch array for the sweep. The preceding group cleared
        // every card it touched after all tie values had consumed its sums.
        let mut group_total = 0.0;
        for &entry in group {
            let r = entry.reach(opponent, opp_reach);
            if r != 0.0 {
                group_total += r;
                group_card[entry.cards[0] as usize] += r;
                group_card[entry.cards[1] as usize] += r;
            }
        }''')
    after = once(after, '''                below_card[entry.cards[1] as usize] += r;
''', '''                below_card[entry.cards[1] as usize] += r;
                // These sums are dead after tie evaluation. Repeated cards
                // may be cleared more than once; untouched cards stay zero.
                group_card[entry.cards[0] as usize] = 0.0;
                group_card[entry.cards[1] as usize] = 0.0;
''')
    sums = text[text.index('fn compact_compat_sums('):text.index('\n/// Showdown evaluation with seat-local reach')]
    sums = sums.replace('fn compact_compat_sums(', 'fn reference_sums(', 1)
    reference = before.replace('pub(crate) fn showdown_kernel_compact(', 'fn reference_compact(', 1)
    reference = once(reference, '''    let analysis = analyze_mass(opp_reach);
    if !analysis.is_f64_exact() {
        showdown_compact_exact(sorted, utilities, player, opp_reach, out, analysis);
        return;
    }
''', '    assert!(analyze_mass(opp_reach).is_f64_exact());\n')
    reference = reference.replace('compact_compat_sums(', 'reference_sums(')
    appendix = (HERE / 'tests.rs.in').read_text(encoding='utf-8').replace('__REFERENCE_SUMS__', sums).replace('__REFERENCE_COMPACT__', reference)
    candidate = (text[:start] + after + text[end:] + appendix).encode()
    return candidate, before, after


def verify_scope(original, candidate, before, after):
    old, new = original.decode(), candidate.decode()
    start, end = old.index(START), old.index(END)
    new_end = new.index(END)
    if new[:start] != old[:start] or new[start:new_end] != after or not new[new_end:].startswith(old[end:]):
        raise ValueError('Change escaped F64 compact-showdown function and test appendix')
    if after.replace('    let mut group_card = [0.0; 52];\n', '', 1) == before:
        raise ValueError('Candidate did not add a sweep')
    for suffix in ('fold_kernel_compact', 'showdown_compact_exact', 'fold_compact_exact'):
        if old.count('fn ' + suffix + '(') != new.count('fn ' + suffix + '('):
            raise ValueError('Untouched function membership changed')
    return {'production_prefix_and_suffix_exact': True, 'initial_all_sums_unchanged': True,
            'fold_and_integer_fallback_unchanged': True, 'only_f64_compact_group_scratch_changed': True,
            'oracle_unchanged': True, 'new_rust_tests': 4, 'rust_tests_executed': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--apply-to', type=Path)
    parser.add_argument('--receipt', type=Path)
    args = parser.parse_args()
    if args.apply_to:
        if args.check or args.receipt is None or args.apply_to.resolve() == ROOT.resolve():
            raise ValueError('Apply requires a separate source copy and fresh receipt')
        copy_root = args.apply_to.resolve(strict=True)
        source = copy_root / SOURCE
        if source.is_symlink() or not source.resolve(strict=True).is_relative_to(copy_root):
            raise ValueError('Source copy path escapes its root')
        before = source.read_bytes()
        provenance = json.loads((HERE / 'provenance.json').read_bytes())
        candidate = (HERE / 'kernel.rs').read_bytes()
        if (pin(before)['sha256'] != SOURCE_SHA or pin(before) != provenance['source']
                or pin(candidate) != provenance['candidate'] or args.receipt.exists()):
            raise ValueError('Original/candidate pin or fresh receipt differs')
        difference = ''.join(difflib.unified_diff(before.decode().splitlines(True), candidate.decode().splitlines(True), 'a/' + SOURCE, 'b/' + SOURCE)).encode()
        if (HERE / 'candidate.patch').read_bytes() != difference or pin(difference) != provenance['patch']:
            raise ValueError('Candidate does not match reviewed patch')
        source.write_bytes(candidate)
        if source.read_bytes() != candidate:
            raise ValueError('Application readback differs')
        with args.receipt.open('x', encoding='utf-8') as stream:
            json.dump({'schema': 'r1.sparse-rank-groups-application/v1', 'source_copy': str(args.apply_to.resolve()),
                       'file': SOURCE, 'before': pin(before), 'after': pin(candidate), 'compiled': False}, stream, indent=2)
            stream.write('\n')
        return
    original = (ROOT / SOURCE).read_bytes()
    generated, before, after = transform(original)
    version = subprocess.run(['rustfmt', '--version'], capture_output=True, check=True, timeout=10)
    formatted = subprocess.run(['rustfmt', '--edition', '2024', '--config', 'skip_children=true', '--emit', 'stdout'],
                               input=generated, capture_output=True, check=True, timeout=10)
    candidate = formatted.stdout
    checks = verify_scope(original, candidate, before, after)
    difference = ''.join(difflib.unified_diff(original.decode().splitlines(True), candidate.decode().splitlines(True), 'a/' + SOURCE, 'b/' + SOURCE)).encode()
    record = {'schema': 'r1.sparse-rank-groups-proposal/v1', 'source_path': SOURCE, 'source': pin(original),
              'candidate': pin(candidate), 'patch': pin(difference),
              'controls': {name: pin((HERE / name).read_bytes()) for name in ('prepare.py', 'tests.rs.in')},
              'formatter': version.stdout.decode().strip(), 'scope': checks,
              'production_adopted': False, 'compiled': False, 'performance': None, 'full_state_quality': None}
    outputs = {'kernel.rs': candidate, 'candidate.patch': difference, 'provenance.json': (json.dumps(record, indent=2) + '\n').encode()}
    if (ROOT / SOURCE).read_bytes() != original:
        raise ValueError('Production source changed during preparation')
    for name, data in outputs.items():
        path = HERE / name
        if args.check:
            if path.read_bytes() != data:
                raise ValueError('Retained research file changed: ' + name)
        else:
            with path.open('xb') as stream:
                stream.write(data)
    print(json.dumps({'status': 'source_prepared_not_compiled', 'candidate': pin(candidate), 'scope': checks}))


if __name__ == '__main__':
    main()
