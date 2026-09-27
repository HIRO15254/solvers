"""Derive finite VM19 controls from retained VM18; source writes only."""
from pathlib import Path

HERE = Path(__file__).resolve().parent
BASE = HERE.parent / 'vm18'


def replace_once(text, old, new):
    if text.count(old) != 1:
        raise ValueError('Expected one derivation anchor: ' + old)
    return text.replace(old, new)


launch = (BASE / 'launch.py').read_text()
for old, new in [('r1-20260927-18', 'r1-20260927-19'), ('2.5', '1.85'),
                 ('3600', '2700'), ('2400', '1500'), ('40GB', '20GB')]:
    launch = launch.replace(old, new)
launch = replace_once(launch, 'import json\n', 'import json\nimport hashlib\n')
launch = replace_once(launch, "    started = dt.datetime.now(dt.timezone.utc)", """    check = json.loads((HERE / 'reservation-check.json').read_bytes())
    raw = (CLOUD / 'budget.json').read_bytes()
    if check['budget_after'] != {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}:
        raise ValueError('Reserved ledger changed before launch')
    if row['maximum_starts'] != 3 or row['disk_gib'] != 20 or row['capacity_or_quota_fallback'] is not None:
        raise ValueError('Finite E2-only envelope differs')
    pack = json.loads((HERE / 'pack-receipt.json').read_bytes())
    raw = (HERE / pack['archive']['path']).read_bytes()
    if {k: pack['archive'][k] for k in ('bytes', 'sha256')} != {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}:
        raise ValueError('Frozen deployment changed')
    started = dt.datetime.now(dt.timezone.utc)
    if (started - dt.datetime.fromisoformat(check['at_utc'])).total_seconds() > 1800:
        raise ValueError('Reservation/preflight older than30min')""")
with (HERE / 'launch.py').open('x') as stream:
    stream.write(launch)

cost = (HERE.parent / 'preflight-vm18/cost-proposal.py').read_text()
cost = cost.replace('VM18', 'VM19').replace('vm18', 'vm19')
cost = replace_once(cost, 'HERE = Path(__file__).resolve().parent', "HERE = Path(__file__).resolve().parent\nPRICES = HERE.parent / 'preflight-vm18'")
start = cost.index('def calculate():')
end = cost.index('\n\nif __name__', start)
section = cost[start:end].replace('HERE /', 'PRICES /')
section = section.replace('"compute-n2-highcpu-32",', '').replace('"spot-n2-highcpu-32",', '')
section = section.replace('1.15', '0.80').replace('60 * 60 + 120', '45 * 60 + 120')
section = section.replace('disk_40gib_24hours', 'disk_20gib_24hours').replace('40 * 24', '20 * 24')
section = section.replace('2.5', '1.85').replace('2_50', '1_85').replace('62/60', '47/60')
cost = cost[:start] + section + cost[end:]
cost = cost.replace('3600,\n                           "price_slack', '2700,\n                           "price_slack')
cost = cost.replace('"maximum_measurement_seconds": 1200', '"maximum_measurement_seconds": 900')
cost = cost.replace('"minimum_remaining_before_measure_dispatch_seconds": 2100', '"minimum_remaining_before_measure_dispatch_seconds": 1800')
cost = cost.replace('dispatch + 1200s', 'dispatch + 900s').replace('"disk_gib": 40', '"disk_gib": 20')
cost = cost.replace('e2-highcpu-32 or n2-highcpu-32', 'e2-highcpu-32 only, no fallback')
cost = cost.replace('Read-only public pricing evidence and a conditional estimate; no reservation or resource mutation', 'Offline replay of official public prices captured2026-09-27 around07:01UTC; conditional estimate only, no reservation or resource mutation')
with (HERE / 'cost-proposal.py').open('x') as stream:
    stream.write(cost)
