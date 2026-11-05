"""Flop samples for the class-level Postflop benchmark.

draw: systematic sampling proportional to the raw flop count over the 1755
suit-isomorphism classes, excluding flops already used by earlier
measurements. The pilot sample (24, seed 1) is drawn first and then excluded
from the master sample (192, seed 0). Every drawn flop has weight 1/n.

subset: nested every-s-th subsets of a sample file (K in 8,16,32,64,96,192),
optionally split into alternating dev/held halves.
"""
import argparse
import random
from pathlib import Path

from quadrature import canonical, raw_counts

HERE = Path(__file__).resolve().parent
POPULATION = HERE / 'results/quadrature/flops1755.txt'
GROUPS = HERE / 'subsets/k25-groups.txt'
EXCLUDED = ([HERE / f'results/p1/{n}.txt' for n in ('flops199', 'flops100', 'flops30', 'pick8')]
            + sorted((HERE / 'results').glob('flop-recall/flops*.txt'))
            + sorted((HERE / 'results').glob('fourbet-menu/flops*.txt'))
            + sorted((HERE / 'results').glob('low-spr/flops*.txt'))
            + [HERE / 'gcp/replan/p1f/flops.txt']
            + [HERE / f'subsets/{n}.txt' for n in ('k8', 'k25', 'k49', 'k95', 'k49-fit')])
SIZES = (8, 16, 32, 64, 96, 192)


def read_flops(path):
    return [tuple(line.split()[:3]) for line in Path(path).read_text(encoding='utf-8').splitlines() if line.strip()]


def population():
    """(flop, group, raw) in flops1755 order; flops are canonical."""
    counts = raw_counts()
    groups = {canonical(f): int(line.split()[3])
              for line in GROUPS.read_text(encoding='utf-8').splitlines() if line.strip()
              for f in [line.split()[:3]]}
    rows = []
    for flop in read_flops(POPULATION):
        k = canonical(flop)
        rows.append((k, groups[k], counts[k]))
    return rows


def excluded(paths=EXCLUDED):
    return {canonical(f) for p in paths for f in read_flops(p)}


def systematic(rows, n, seed):
    """Choose n rows with probability proportional to raw, by equal steps on the cumulative line."""
    order = sorted(range(len(rows)), key=lambda i: (rows[i][1], i))
    total = sum(rows[i][2] for i in order)
    h = total / n
    s = random.Random(seed).random() * h
    chosen, k, acc = [], 0, 0.0
    for i in order:
        acc_end = acc + rows[i][2]
        while k < n and s + k * h < acc_end:
            if chosen and chosen[-1] == i:
                raise ValueError(f'flop {rows[i][0]} drawn twice')
            chosen.append(i)
            k += 1
        acc = acc_end
    if len(chosen) != n:
        raise ValueError(f'drew {len(chosen)} of {n}')
    return [rows[i] for i in chosen]


def draw(exclusion_paths=EXCLUDED, pilot=24, master=192):
    rows = population()
    out = excluded(exclusion_paths)
    remaining = [r for r in rows if r[0] not in out]
    pilot_rows = systematic(remaining, pilot, 1)
    taken = {r[0] for r in pilot_rows}
    master_rows = systematic([r for r in remaining if r[0] not in taken], master, 0)
    return rows, out, pilot_rows, master_rows


def subset_indices(count, k, half=None):
    if k not in SIZES:
        raise ValueError(f'K must be one of {SIZES}')
    if count % k:
        raise ValueError(f'{count} lines are not divisible by K={k}')
    s = count // k
    if half is None:
        return [i for i in range(count) if i % s == 0]
    return [i for i in range(count) if i % (2 * s) == (0 if half == 'dev' else s)]


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest='command', required=True)
    d = sub.add_parser('draw')
    d.add_argument('--out-dir', required=True)
    d.add_argument('--meta', required=True)
    d.add_argument('--exclusion', required=True)
    s = sub.add_parser('subset')
    s.add_argument('file')
    s.add_argument('k', type=int)
    s.add_argument('--half', choices=('dev', 'held'))
    s.add_argument('--out', required=True)
    args = parser.parse_args()
    if args.command == 'subset':
        lines = [l for l in Path(args.file).read_text(encoding='utf-8').splitlines() if l.strip()]
        picked = [lines[i] for i in subset_indices(len(lines), args.k, args.half)]
        Path(args.out).write_text(''.join(l + '\n' for l in picked), encoding='utf-8', newline='\n')
        return
    rows, out, pilot_rows, master_rows = draw()
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    for name, chosen in (('pilot24', pilot_rows), ('master192', master_rows)):
        (out_dir / f'{name}.txt').write_text(''.join(' '.join(r[0]) + '\n' for r in chosen),
                                            encoding='utf-8', newline='\n')
    meta = ['set,i,flop,group,raw'] + [f'{name},{i},{" ".join(r[0])},{r[1]},{r[2]}'
                                         for name, chosen in (('pilot24', pilot_rows), ('master192', master_rows))
                                         for i, r in enumerate(chosen)]
    Path(args.meta).parent.mkdir(parents=True, exist_ok=True)
    Path(args.meta).write_text('\n'.join(meta) + '\n', encoding='utf-8', newline='\n')
    table = ['# 除いたflop（群ごと）', '',
             f'除いたflop：{len(out)}個（1755個のうち）。割合は元のflopの数で数える。40%を超える群に`*`を付ける。', '',
             '|群|元のflopの数|除いた数|割合|', '|---:|---:|---:|---:|']
    for g in sorted({r[1] for r in rows}):
        total = sum(r[2] for r in rows if r[1] == g)
        gone = sum(r[2] for r in rows if r[1] == g and r[0] in out)
        share = gone / total
        table.append(f'|{g}|{total}|{gone}|{share:.1%}{" *" if share > 0.4 else ""}|')
    Path(args.exclusion).write_text('\n'.join(table) + '\n', encoding='utf-8', newline='\n')


if __name__ == '__main__':
    main()
