import json, glob, re, os, statistics as st, sys
from collections import defaultdict
rows = defaultdict(list)
for f in sorted(glob.glob(os.path.join(sys.argv[1], '*.json'))):
    tag = os.path.basename(f)[:-5]
    try:
        d = json.load(open(f))
    except Exception:
        print('bad', tag); continue
    rows[re.sub(r'_r\d$', '', tag)].append(d['secsPerIter'])
groups = defaultdict(dict)
for k, v in rows.items():
    m = re.match(r'(.*_t\d+)_(exact|f64fold|t8a_\w+)$', k)
    groups[m.group(1)][m.group(2)] = v
for g, vs in groups.items():
    ex = vs['exact']
    print('==', g)
    for v in ['exact', 'f64fold', 't8a_l', 't8a_lb', 't8a_lw', 't8a_lbw']:
        if v in vs:
            x = vs[v]
            print(f'  {v:8s} ' + ' / '.join(f'{y:.4f}' for y in x)
                  + f'   min {(min(x)/min(ex)-1)*100:+.1f}%  med {(st.median(x)/st.median(ex)-1)*100:+.1f}%')
