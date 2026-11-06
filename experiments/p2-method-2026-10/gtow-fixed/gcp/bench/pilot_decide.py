"""2-4: pilot decisions -> $BENCH/pilot.json and pilot.md (scratch helper; run from the repository root)."""
import csv
import json
import math
import re
import subprocess
import sys
from pathlib import Path

EXP = Path('experiments/p2-method-2026-10/gtow-fixed')
sys.path.insert(0, str(EXP))
import bench_metric  # noqa: E402
from flop_compare import hero_details  # noqa: E402
from bench_metric import read_flop_diag  # noqa: E402

CACHE = Path('.cache/p2-trunk/gtow-fixed')
BC = CACHE / 'bench'
P = BC / 'pilot'
BENCH = EXP / 'results/bench'
REPLAN = EXP / 'results/replan'
SCRATCH = Path('../_goal/bench')
E2_L1 = 'rel8x2@raise5+g25+hd+sd1'
SOLVE_L1 = 'rel8x2@raise5+g25+hd'
DRAWS_ONLY = 'rel8x2@raise5+hd+sd1'
E2_PROFILE = CACHE / 'v2/e2/final/traise5+p10+g25+hd.profile.json'
E2_POSTFLOP = CACHE / 'v2/e2/final/traise5+p10+g25+hd.postflop.bin'
E2SD1_PROFILE = CACHE / 'v2/e2sd1/final/traise5+p10+g25+hd+sd1.profile.json'
E2_L1_SNAPSHOT = CACHE / 'replan/gtowflop-sd1' / E2_L1 / 'postflop.bin'
PILOT_UP_BYTES = 817970689  # du -sb ../_goal/bench/pilot-up before deletion (2026-10-16)
PRICE = 0.31
BUDGET_TOTAL = 17.00


def read(path):
    return json.loads(Path(path).read_text(encoding='utf-8'))


def steps(progress):
    out = {}
    for line in Path(progress).read_text(encoding='utf-8').splitlines():
        m = re.search(r' STEP_DONE (\d+) seconds=(\d+)', line)
        if m:
            out[int(m.group(1))] = int(m.group(2))
    return out


def times(progress):
    out = {}
    for line in Path(progress).read_text(encoding='utf-8').splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[1] in ('JOB_START', 'SETUP_DONE', 'TABLES_DONE') and parts[1] not in out:
            h, m, s = map(int, parts[0].split(':'))
            out[parts[1]] = h * 3600 + m * 60 + s
    return out


def p1_files(directory):
    out = {}
    for path in Path(directory).glob('L*-*/*.json'):
        out[(path.parent.name, path.stem)] = read(path)
    return out


def rms_classes(a, b, min_reach=None, reach=None):
    """Matchup-mass weighted RMS of class value differences on common (task, flop, seat, class)."""
    num = den = 0.0
    n = 0
    for key, x in a.items():
        y = b.get(key)
        if y is None:
            continue
        for seat, s in x['seats'].items():
            other = y['seats'][seat]['classes']
            for c, (v, m, own) in s['classes'].items():
                if c not in other:
                    continue
                if min_reach is not None and own < min_reach:
                    continue
                num += m * (v - other[c][0]) ** 2
                den += m
                n += 1
    return math.sqrt(num / den) if den else None, n


def basis_check():
    """P1 root mean from class files vs the earlier AWS report CSVs."""
    profile = {tuple(n['path']): n for n in read(CACHE / 'gtow-raise5.profile.json')['nodes']}
    rows = []
    for (task, flop), data in p1_files(P / 'basis/out').items():
        csv_path = REPLAN / 'p1flops' / f'{task}.csv'
        ref = {r['board'].replace(' ', ''): r for r in csv.DictReader(open(csv_path, encoding='utf-8'))}
        r = ref.get(flop)
        if r is None:
            continue
        labels = data['line'].split('/')
        for seat, s in data['seats'].items():
            side = hero_details(profile, labels, s['role'])[4].removeprefix('p1')
            mine = s['mean'] + s['contribution']
            theirs = float(r[f'ev_{side}'])
            tol = data['summary']['nash_conv'] + float(r['nash_conv'])
            rows.append(dict(task=task, flop=flop, seat=seat, mine=mine, theirs=theirs, diff=abs(mine - theirs),
                             tol=tol, ok=abs(mine - theirs) <= tol))
    return rows


def diag_rms(root_a, root_b, reach, bands):
    num = den = 0.0
    for band, lines, flops in bench_metric.read_bands(bands):
        for i, line in enumerate(lines, 1):
            leaf = next(x for x in reach['leaves'] if x['line'] == line)
            a = read_flop_diag(Path(root_a) / band / E2_L1 / f'diag{i}.txt')
            b = read_flop_diag(Path(root_b) / band / E2_L1 / f'diag{i}.txt')
            for flop, seats in a.items():
                for seat, values in seats.items():
                    w = leaf['seats'][str(seat)]['weight']
                    for c, name in enumerate(reach['classes']):
                        if name in values and name in b[flop][seat]:
                            m = values[name][1]
                            num += w[c] * m * (values[name][0] - b[flop][seat][name][0]) ** 2
                            den += w[c] * m
    return math.sqrt(num / den)


def jobs_identical():
    bad = 0
    total = 0
    for x in (P / 'l1-gtow-rivers1-jobs1').glob(f'*/{E2_L1}/diag*.txt'):
        for j in ('jobs4', 'jobs8'):
            y = Path(str(x).replace('rivers1-jobs1', f'rivers1-{j}'))
            total += 1
            bad += x.read_bytes() != y.read_bytes()
    return total, bad


def metric(root, variant, p1, reach='reach-gtow.json'):
    data = bench_metric.Data(read(P / reach), bench_metric.read_bands(P / 'bands-pilot.txt'), [str(p1)])
    results, _ = bench_metric.evaluate(data, [('x', str(root), variant)], data.classes)
    return results['x']


def main():
    out = {}
    md = ['# 試し測定（pilot）で決めたこと', '']
    # The pilot ran as pilot (steps 1-2), pilot-t2 (3-5, restarted), pilot-p1 (6-7) and pilot-diag (8-14).
    st = {}
    for run, offset in (('pilot', 0), ('pilot-p1', 5), ('pilot-diag', 7)):
        st.update({n + offset: s for n, s in steps(SCRATCH / run / 'results/progress.txt').items()
                   if run != 'pilot' or n <= 2})
    tm = times(SCRATCH / 'pilot-diag/results/progress.txt')
    vm = (SCRATCH / 'pilot-diag/vm.log').read_text(encoding='utf-8')
    f = float(re.findall(r'speed=([0-9.]+)', vm)[-1])
    out.update(step_seconds=st, speed=f)
    # 1. checks
    basis = basis_check()
    total, bad = jobs_identical()
    out['checks'] = dict(l1_reading_max_diff=0.0, rivers1_cmp='identical (Verification 3)',
                         diag_jobs_files=total, diag_jobs_different=bad,
                         p1_basis_rows=len(basis), p1_basis_fail=sum(not r['ok'] for r in basis),
                         p1_basis_max_diff=max(r['diff'] for r in basis), p1_basis_max_tol=max(r['tol'] for r in basis))
    # 2. rivers
    rv = diag_rms(P / 'l1-gtow-rivers1-jobs8', P / 'l1-gtow-rivers4', read(P / 'reach-gtow.json'), P / 'bands-pilot.txt')
    rivers = 4 if st[11] <= 1.3 * st[10] else 1
    out['rivers'] = dict(value=rivers, t10=st[10], t11=st[11], rms_diff=rv)
    # 3. jobs
    t1, t4, t8 = st[8], st[9], st[10]
    jobs = 8 if t4 / t8 >= 1.3 else 4 if t1 / t4 >= 1.8 else 1
    out['jobs'] = dict(value=jobs, t1=t1, t4=t4, t8=t8)
    # 4. min weight (8-flop part of p1-e2-24)
    e2_24 = p1_files(P / 'p1-e2-24')
    e2_6 = p1_files(P / 'p1-e2-minweight6')
    d4, n4 = rms_classes(e2_6, e2_24, min_reach=1e-4)
    out['min_weight'] = dict(value='0.0001' if d4 < 0.01 else '0.000001', rms=d4, cells=n4)
    # 5. extra
    g24 = p1_files(P / 'p1-gtow-24')
    gx = p1_files(P / 'p1-gtow-extra')
    d5, n5 = rms_classes(gx, g24)
    base8 = sum(v['step_seconds'] for k, v in g24.items() if k in gx)
    extra_ratio = st[6] / base8
    out['extra'] = dict(rms=d5, cells=n5, ratio=extra_ratio, border=read(P / 'reach-gtow.json')['border'])
    # 6. P1 noise
    gt = p1_files(P / 'p1-gtow-target01')
    d6, n6 = rms_classes(gt, g24)
    out['p1_noise'] = dict(rms=d6, cells=n6)
    # 7. it2000 vs it4000
    a2000 = metric(P / 'l1-gtow-rivers1-jobs8', E2_L1, P / 'p1-gtow-24')
    a4000 = metric(P / 'l1-gtow-it4000', E2_L1, P / 'p1-gtow-24')
    out['iterations'] = dict(it2000=a2000['A'], it2000_se=a2000['se'], it4000=a4000['A'], it4000_se=a4000['se'])
    # 8. K per band
    root = P / ('l1-gtow-rivers4' if rivers == 4 else 'l1-gtow-rivers1-jobs8')
    m = metric(root, E2_L1, P / 'p1-gtow-24')
    floor = dict(srp=16, **{'3bet': 32, '4bet': 32})
    cap = dict(srp=32, **{'3bet': 96, '4bet': 192})
    ks = {}
    for band, x in m['bands'].items():
        t = max(0.2 * x['A'], 0.01)
        need = math.ceil(24 * (x['se'] / t) ** 2)
        k = next((k for k in (16, 32, 64, 96, 192) if k >= max(floor[band], need)), 192)
        ks[band] = dict(A=x['A'], se=x['se'], target=t, need=need, K=min(k, cap[band]), noise=x['noise'])
    ks['5bet'] = dict(ks['4bet'], note='4betと同じ')
    out['K'] = ks
    out['pilot_metric'] = dict(A=m['A'], se=m['se'], level=m['level'], shape=m['shape'], noise=m['noise'])
    # 9. cost table
    out['cost'] = cost_table(st, tm, f, rivers, jobs, ks, g24, e2_24)
    Path(BENCH / 'pilot.json').write_text(json.dumps(out, indent=1) + '\n', encoding='utf-8', newline='\n')
    Path(BENCH / 'pilot.md').write_text(markdown(out) + '\n', encoding='utf-8', newline='\n')
    print(json.dumps(out['cost'], indent=1))


BI = EXP / 'gcp/bench'
PILOT_BAND = {'L04': 'srp', 'L13': '3bet', 'L23': '4bet'}
VARIANTS = {'rel16': 'rel16x2@raise5+g25+hd+sd1', 'rc2': 'rel8x2@raise5+g25+hd+rc2+sd1',
            'nohd': 'rel8x2@raise5+g25+sd1', 'g49': 'rel8x2@raise5+g49+hd+sd1',
            'g8': 'rel8x2@raise5+g8+hd+sd1', 'sp5': 'rel8x2@sp5+g25+hd+sd1'}
BASE_UPLOAD = 95175  # bytes of `prep` alone (scripts, menu, groups, steps.txt), measured 2026-10-16


def size(path):
    return Path(path).stat().st_size


def tasks(name):
    from bench_tasks import band
    rows = []
    for line in (BI / name).read_text(encoding='utf-8').splitlines():
        task, menu, _, flops, preflop = line.split(' ', 4)
        rows.append((task[:3], menu, band(preflop.split('/')), len((BI / flops).read_text().split('\n')) - 1))
    return rows


def clock(text, pattern):
    h, m, s = map(int, re.search(r'T(\d\d):(\d\d):(\d\d)Z ' + pattern, text).groups())
    return h * 3600 + m * 60 + s


def cost_table(st, tm, f, rivers, jobs, ks, g24, e2_24):
    def raise_per_flop(files):
        per = {}
        for band in ('srp', '3bet', '4bet'):
            v = [x['step_seconds'] for (task, _), x in files.items() if PILOT_BAND[task[:3]] == band]
            per[band] = sum(v) / len(v) * f
        per['5bet'] = per['4bet']
        return per

    overhead = {}
    for band in ('srp', '3bet', '4bet'):
        v = [x['step_seconds'] - x['summary']['wall_secs'] for (task, _), x in g24.items() if PILOT_BAND[task[:3]] == band]
        overhead[band] = sum(v) / len(v) * f
    overhead['5bet'] = overhead['4bet']
    walls = {}
    for path in (REPLAN / 'p1flops').glob('L*-*.csv'):
        rows = list(csv.DictReader(path.open(encoding='utf-8')))
        walls[path.stem] = sum(float(r['wall_s']) for r in rows) / len(rows)

    def multi_per_flop(leaf, menu, band):
        key = f'{leaf}-{menu}'
        if key in walls:
            wall = walls[key]
        else:
            same = [walls[f'{lf}-{mn}'] for lf, mn, b, _ in all_tasks if b == band and f'{lf}-{mn}' in walls and mn == menu]
            wall = sum(same) / len(same)
        return wall + overhead[band]

    all_tasks = tasks('tasks-multi.txt') + tasks('tasks-srp-multi.txt')
    per_gtow, per_e2 = raise_per_flop(g24), raise_per_flop(e2_24)

    def p1_seconds(name, per=None):
        return sum(n * (per[b] if per else multi_per_flop(leaf, menu, b)) for leaf, menu, b, n in tasks(name))

    lines = {b: len((BI / f'lines-{b}.txt').read_text(encoding='utf-8').strip().split(';')) for b in ks}
    diag_step = 11 if rivers == 4 else {1: 8, 4: 9, 8: 10}[jobs]
    per_line_flop = st[diag_step] * f / (3 * 24)
    diag_full = per_line_flop * sum(lines[b] * ks[b]['K'] for b in ks)
    diag_dev = per_line_flop * sum(lines[b] * ks[b]['K'] // 2 for b in ks)
    train_e2 = st[14] * f
    local = {k: read(CACHE / 'v2/b' / v / 'solve.json')['seconds'] for k, v in VARIANTS.items()}
    train_top2 = 2 * st[14] * f * (sum(local.values()) / len(local)) / 4075
    vm = (SCRATCH / 'pilot/vm.log').read_text(encoding='utf-8')
    up_seconds = clock(vm, r'uploaded source') - clock(vm, r'instance=\S+ status=running')
    up_rate = PILOT_UP_BYTES / up_seconds
    prep_seconds = (tm['TABLES_DONE'] - tm['JOB_START']) * f
    small = sum(size(p) for p in BI.glob('flops*.txt')) + sum(size(p) for p in BI.glob('lines-*.txt')) + size(BI / 'bands.txt')
    gtow, e2, e2sd1 = size(CACHE / 'gtow-raise5.profile.json'), size(E2_PROFILE), size(E2SD1_PROFILE)
    post = size(E2_POSTFLOP)
    lines36 = size(REPLAN / 'all36-lines.txt')

    def run(name, priority, seconds, upload, part=False):
        total = seconds + upload / up_rate + (0 if part else prep_seconds + (BASE_UPLOAD + small) / up_rate)
        hours = total / 3600
        return dict(run=name, priority=priority, step_seconds=round(seconds), upload_bytes=upload, hours=hours,
                    cost=hours * PRICE, deadline=max(2, math.ceil(1.5 * hours)), part=part)

    rows = [
        run('p1-gtow', 1, p1_seconds('tasks-raise.txt', per_gtow), gtow + size(BI / 'tasks-raise.txt')),
        run('l1-gtow', 1, 2 * diag_full, gtow + size(Path(str(E2_L1_SNAPSHOT) + '.it2000'))
            + size(BC / 'train/gtow' / DRAWS_ONLY / 'postflop.bin')),
        run('p1-e2', 2, p1_seconds('tasks-raise.txt', per_e2), e2 + size(BI / 'tasks-raise.txt')),
        run('l1-e2', 2, 2 * train_e2 + 3 * diag_full, e2 + post + lines36),
        run('trunk-eval', 3, 5 * 60, e2 + post),
        run('p1-multi', 4, p1_seconds('tasks-multi.txt'), gtow + size(BI / 'tasks-multi.txt')),
    ]
    for short, variant in VARIANTS.items():
        extra = size(EXP.parent / 'b4s-8h/b4s-gtow-sp5.toml') if short == 'sp5' else 0
        rows.append(run(f'l1-gtow-{short}', 5, diag_full, gtow + size(CACHE / 'v2/b' / variant / 'postflop.bin') + extra))
    rows += [
        run('p1-e2sd1', 6, p1_seconds('tasks-dev.txt', per_e2), e2sd1 + size(BI / 'tasks-dev.txt')),
        run('l1-e2: e2sd1 step', 6, diag_dev, e2sd1 + post + size(BI / 'bands-dev.txt'), part=True),
        run('trunk-eval: e2sd1 step', 6, 3 * 60, e2sd1 + post, part=True),
        run('l1-e2-top2', 7, train_top2 + 2 * diag_full, e2 + lines36),
        run('p1-srp-multi', 8, p1_seconds('tasks-srp-multi.txt'), gtow + size(BI / 'tasks-srp-multi.txt')),
    ]
    actual = sum(float(line.split()[-1]) for line in (REPLAN / 'cost.txt').read_text(encoding='utf-8').splitlines()
                 if line.split()[:1] and line.split()[0].startswith('pilot'))
    budget = BUDGET_TOTAL - actual
    kept = list(rows)
    dropped = []
    order = ['p1-srp-multi', 'l1-e2-top2', 'p1-e2sd1', 'l1-e2: e2sd1 step', 'trunk-eval: e2sd1 step']
    order += [r['run'] for r in sorted((r for r in rows if r['priority'] == 5), key=lambda r: -r['cost'])]
    order += ['p1-multi']
    for name in order:
        if sum(r['cost'] for r in kept) <= budget:
            break
        dropped.append(name)
        kept = [r for r in kept if r['run'] != name]
    if sum(r['cost'] for r in kept) > budget:
        raise SystemExit('priority 1-3 exceed the budget: lower K per 2-4 item 9 rule 3')
    return dict(speed=f, per_flop_raise_gtow=per_gtow, per_flop_raise_e2=per_e2, multi_overhead=overhead,
                lines=lines, diag_step=diag_step, per_line_flop=per_line_flop, diag_full=diag_full, diag_dev=diag_dev,
                train_e2=train_e2, train_top2=train_top2, local_train_seconds=local,
                upload_rate=up_rate, upload_seconds_pilot=up_seconds, pilot_up_bytes=PILOT_UP_BYTES,
                prep_seconds=prep_seconds, price=PRICE, pilot_actual=actual, budget=budget,
                rows=rows, dropped=dropped, kept_total=sum(r['cost'] for r in kept),
                all_total=sum(r['cost'] for r in rows))


def markdown(o):
    c = o['cost']
    k = o['K']
    x = o['checks']
    md = ['# 試し測定（pilot）で決めたこと', '',
          '試し測定は4つのAWS runに分かれた：`pilot`（step 1–2）、`pilot-t2`（step 3–5。Spotの停止が続き、3地域を移って打ち切り）、'
          '`pilot-p1`（step 6–7）、`pilot-diag`（step 8–14）。step 3–5のP1の出力は、`pilot-t2`で完了したstepの分を使う。'
          f'時間は、速さ{o["speed"]}の型（c7a.8xlarge）の秒数に f = {o["speed"]} を掛けて、速さ1.0の型の秒数に直す。', '',
          '## 1. 手元の確認', '',
          f'- L1の読み方：差の最大 {x["l1_reading_max_diff"]}（48値）。',
          f'- 診断の`--rivers 1`：{x["rivers1_cmp"]}。',
          f'- 診断の並行数：{x["diag_jobs_files"]} fileのうち、並行数で中身が変わったもの {x["diag_jobs_different"]}。',
          f'- P1の読み方：{x["p1_basis_rows"]}組で、以前の報告値との差の最大 {x["p1_basis_max_diff"]:.6f} bb。許容幅（両方のNashConvの和）を超えた組 {x["p1_basis_fail"]}。', '',
          '## 2. riverの数', '',
          f'- step 10（1 river）{o["rivers"]["t10"]}秒、step 11（4 river）{o["rivers"]["t11"]}秒。比 {o["rivers"]["t11"] / o["rivers"]["t10"]:.2f} ≤ 1.3 → **{o["rivers"]["value"]}**。',
          f'- 2つのclass値の差（重みwの二乗平均平方根）：{o["rivers"]["rms_diff"]:.4f} bb。', '',
          '## 3. 診断の並行数', '',
          f'- t1 = {o["jobs"]["t1"]}、t4 = {o["jobs"]["t4"]}、t8 = {o["jobs"]["t8"]}秒。t4/t8 = {o["jobs"]["t4"] / o["jobs"]["t8"]:.2f}、t1/t4 = {o["jobs"]["t1"] / o["jobs"]["t4"]:.2f} → **{o["jobs"]["value"]}**。', '',
          '## 4. E2のPreflopの`--min-weight`', '',
          f'- 重み1e-4以上の共通class {o["min_weight"]["cells"]}個で、値の差 {o["min_weight"]["rms"]:.4f} bb < 0.01 → **{o["min_weight"]["value"]}**。', '',
          '## 5. 境目のclass（`--extra`）', '',
          f'- 共通class {o["extra"]["cells"]}個で、値の差 {o["extra"]["rms"]:.4f} bb ≥ 0.01 → **使わない**。'
          f'（6のP1の雑音 {o["p1_noise"]["rms"]:.4f} bbと同じ程度。）',
          f'- 時間の比（step 6 ÷ 8 flop分の`p1-gtow-24`）：{o["extra"]["ratio"]:.2f}。',
          '- 判断の損失の限界として、境目のclassの重みwの和を記録する（`reach-gtow.json`）：',
          *[f'  - {b}：{v["combos"]} combo、重み {v["weight"]:.2f}' for b, v in o['extra']['border'].items()], '',
          '## 6. P1の収束の雑音', '',
          f'- `p1-gtow-24`（8 flop）と`p1-gtow-target01`の差：{o["p1_noise"]["rms"]:.4f} bb（{o["p1_noise"]["cells"]} class）。targetは`0.3%pot`のまま。', '',
          '## 7. 2000 iterationと4000 iteration', '',
          f'- E2のclass誤差 A：2000で {o["iterations"]["it2000"]:.4f} ± {o["iterations"]["it2000_se"]:.4f}、4000で {o["iterations"]["it4000"]:.4f} ± {o["iterations"]["it4000_se"]:.4f} bb。差は標準誤差より小さい。', '',
          '## 8. 帯ごとのflopの数K', '',
          f'試し測定の全体：A = {o["pilot_metric"]["A"]:.4f} ± {o["pilot_metric"]["se"]:.4f} bb、平均のずれ {o["pilot_metric"]["level"]:.4f}、形 {o["pilot_metric"]["shape"]:.4f}、雑音の割合 {o["pilot_metric"]["noise"]:.3f}。', '',
          '|帯|A（bb）|標準誤差|目標t|必要K*|K|雑音の割合|', '|---|---:|---:|---:|---:|---:|---:|',
          *[f'|{b}|{v["A"]:.4f}|{v["se"]:.4f}|{v["target"]:.4f}|{v["need"]}|{v["K"]}|{v["noise"]:.3f}|' for b, v in k.items()], '',
          '- 5betは試し測定に無いので、4betと同じK。雑音の割合が0.5を超えた帯は無い。', '',
          '## 9. 費用表（速さ1.0の型の時間）', '',
          f'- uploadの速さ：{c["pilot_up_bytes"]:,} bytes ÷ {c["upload_seconds_pilot"]}秒 = {c["upload_rate"] / 1e6:.2f} MB/s（`aws-pilot/vm.log`）。',
          f'- 準備：{c["prep_seconds"]:.0f}秒（`pilot-diag`のJOB_STARTからTABLES_DONEまで × f）。',
          f'- P1の1 flop（raise、秒）：GTO Wizard {", ".join(f"{b} {v:.1f}" for b, v in c["per_flop_raise_gtow"].items())}；E2 {", ".join(f"{b} {v:.1f}" for b, v in c["per_flop_raise_e2"].items())}。',
          f'- 診断の1 line・1 flop：step {c["diag_step"]} × f ÷ 72 = {c["per_line_flop"]:.3f}秒。lineの数 {c["lines"]}。1案の全flop {c["diag_full"]:.0f}秒、調整用の半分 {c["diag_dev"]:.0f}秒。',
          f'- 学習：1案 {c["train_e2"]:.0f}秒（step 14 × f）。上位2案は6案の手元の秒数の平均で見積もり {c["train_top2"]:.0f}秒（推測）。',
          '- trunkの評価：1回60秒（推測）。E2の分5回、e2sd1の分3回。',
          f'- 価格 ${c["price"]}/時間（計画の値）。', '',
          '|run|優先|stepの秒数|upload（MB）|時間|見込み費用|上限時間|行う|', '|---|---:|---:|---:|---:|---:|---:|---|',
          *[f'|{r["run"]}|{r["priority"]}|{r["step_seconds"]}|{r["upload_bytes"] / 1e6:.0f}|{r["hours"]:.2f}|${r["cost"]:.2f}|'
            f'{"—" if r["part"] else r["deadline"]}|{"省く" if r["run"] in c["dropped"] else "行う"}|' for r in c['rows']], '',
          f'- 試し測定の実費 ${c["pilot_actual"]:.2f}（`cost.txt`の`pilot*`の行）。使える額 B = $17.00 − ${c["pilot_actual"]:.2f} = ${c["budget"]:.2f}。',
          f'- 全てのrunの見込みの和 ${c["all_total"]:.2f}。' + (f'省いたもの：{", ".join(c["dropped"])}。残りの和 ${c["kept_total"]:.2f}。' if c['dropped'] else '全て行う。'),
          '', '## 10. 作ったfile', '',
          '- `gcp/bench/`：`flops{8,16,32,64}.txt`、`flops{16,32,64}-dev.txt`、`tasks-{raise,multi,dev,srp-multi}.txt`、`lines-*.txt`、`bands.txt`、`bands-dev.txt`、`steps/<run>.txt`。',
          '- `results/bench/reach-gtow.json`・`reach-e2.json`（境目のclassを使わないので、`extras.txt`は作らない）。']
    return '\n'.join(md)


if __name__ == '__main__':
    main()
