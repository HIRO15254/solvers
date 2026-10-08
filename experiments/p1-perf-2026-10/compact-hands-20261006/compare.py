"""Run the same CLI solves/CSV exports and compare positive-support rows."""
import argparse
import csv
import json
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
SCRATCH = ROOT / 'runs/p1-t1/comparison'
NAMES = ['river_small', 'turn_small', 'tournament_icm', 'river_script',
         'river_multi', 'turn_iso_on', 'turn_iso_off', 'river_fractional']


def run(binary, label):
    binary = Path(binary).resolve()
    for name in NAMES:
        output = SCRATCH / label / name
        output.parent.mkdir(parents=True, exist_ok=True)
        config = HERE / 'configs' / (name + '.toml')
        commands = [[str(binary), 'solve', str(config), '--out', str(output), '--threads', '1']]
        commands += [[str(binary), 'export', str(output / 'solution.sol'), view,
                      '--node', 'all', '--format', 'csv', '--output', str(output / (view + '.csv'))]
                     for view in ['summary', 'strategy', 'ev']]
        with (output.parent / (name + '.log')).open('w', encoding='utf-8') as log:
            for command in commands:
                log.write(json.dumps(command) + '\n')
                log.flush()
                subprocess.run(command, stdout=log, stderr=log, check=True, cwd=ROOT)
        print(label, name, 'OK', flush=True)


def rows(path):
    with path.open(encoding='utf-8-sig', newline='') as f:
        return list(csv.DictReader(f))


def compare(new_label="new", old_label="old"):
    reports = []
    for name in NAMES:
        report = {'config': name, 'views': {}}
        for view in ['summary', 'strategy', 'ev']:
            old, new = [rows(SCRATCH / label / name / (view + '.csv')) for label in [old_label, new_label]]
            assert len(old) == len(new), (name, view, 'row counts', len(old), len(new))
            maximum = relative = 0.0
            exact = True
            for a, b in zip(old, new):
                assert a.keys() == b.keys()
                for key in a:
                    if key == 'wall_secs':
                        continue
                    if a[key] == b[key]:
                        continue
                    exact = False
                    for x, y in zip(a[key].split('|'), b[key].split('|'), strict=True):
                        try:
                            x, y = float(x), float(y)
                        except ValueError:
                            raise AssertionError((name, view, key, a[key], b[key])) from None
                        d = abs(x - y)
                        maximum = max(maximum, d)
                        relative = max(relative, d / max(abs(x), abs(y), 1e-30))
            report['views'][view] = {'rows': len(old), 'textExactExceptTime': exact,
                                     'maxAbs': maximum, 'maxRelative': relative}
            assert relative <= 1e-6, (name, view, report['views'][view])
        metrics = ['evP0', 'evP1', 'explP0', 'explP1', 'nashConv']
        old, new = [json.loads((SCRATCH / label / name / 'run.json').read_text()) for label in [old_label, new_label]]
        report['metrics'] = {k: {'old': old[k], 'new': new[k], 'exact': old[k] == new[k]} for k in metrics}
        assert all(abs(old[k] - new[k]) <= 1e-6 * max(abs(old[k]), abs(new[k]), 1e-30) for k in metrics)
        reports.append(report)
        print(name, json.dumps(report), flush=True)
    (HERE / 'comparison.json').write_text(json.dumps(reports, indent=2) + '\n', encoding='utf-8')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('mode', choices=['old', 'new', 'compare'])
    parser.add_argument('--binary')
    parser.add_argument('--label')
    parser.add_argument('--new-label', default='new')
    parser.add_argument('--old-label', default='old')
    args = parser.parse_args()
    if args.mode == 'compare':
        compare(args.new_label, args.old_label)
    else:
        run(args.binary, args.label or args.mode)
