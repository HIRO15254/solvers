"""Apply the independently replayed VM21 disk reservation return; no cloud access."""
import datetime as dt
from decimal import Decimal as D, ROUND_CEILING
import hashlib
import importlib.util
import json
import os
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent
AUDIT = HERE / 'usage-audit'
PINS = {
    'budget.json': '11bf3967e2196d23486d76796569f1ccd8c998f49d1d237121b78b406374adb1',
    'vm21/usage-audit/report.json': '2d6370ff4e19f731d555b59a4df133a750e2891d6ffb931b086f715a4612cd5b',
    'vm21/usage-audit/analyze.py': 'f60b8b6e07db948c88fae26c2df5857c79220712cac26cc71b21210bc104eae3',
    'vm21/usage-audit/collect.py': 'e5d143a12feaf57daaefb122cb15c74251dd65564438caea2e9a8a5196780bae',
    'vm21/usage-audit/acquisition.json': 'dba8951a42eb7e748c278667d0bdd3316feb2501a459742e6274e22752047918',
}


def pin(raw):
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def encode(value):
    return (json.dumps(value, indent=2, ensure_ascii=False) + '\n').encode()


def fresh(path, raw):
    with path.open('xb') as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())


def held(ledger):
    return sum(D(str(r['billed_usd'] if r['reservation_released'] else r['reserved_usd']))
               for r in ledger['reservations'])


def main():
    raws = {name: (CLOUD / name).read_bytes() for name in PINS}
    assert all(pin(raw)['sha256'] == PINS[name] for name, raw in raws.items())
    spec = importlib.util.spec_from_file_location('vm21_audited_usage', AUDIT / 'analyze.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    report = module.build_report()
    assert report == json.loads(raws['vm21/usage-audit/report.json'])
    life, cost = report['lifecycle'], report['cost']
    assert report['instance_id'] == '715936786015339093'
    assert life['disk_id'] == '5498906379380081237'
    delta = dt.datetime.fromisoformat(life['disk_absence']) - dt.datetime.fromisoformat(life['disk_created'])
    seconds = D(delta.days * 86400 + delta.seconds) + D(delta.microseconds) / 1000000
    minutes = ((seconds + 120) / 60).to_integral_value(rounding=ROUND_CEILING)
    assert seconds == D('1675.610970') and minutes == 30
    assert D(cost['disk_seconds']) == seconds and cost['disk_minutes_after_120s_slack_and_ceiling'] == minutes
    model = D(47)/60*(D('.06701142') + D('.0025')) + D(10)/60*D('.80') + D(20)*minutes/60*D('.000137') + D(320)/1024*D('.30') + 1
    assert abs(model - D(cost['model_usd'])) < D('1e-24')
    assert D(cost['components_usd']['original_uncertainty']) == 1
    assert D(cost['components_usd']['original_egress_320_mib']) == D('.09375')
    assert (model / D('.05')).to_integral_value(rounding=ROUND_CEILING) * D('.05') == D('1.30')
    assert D(cost['proposed_hold_usd']) == D('1.30') and D(cost['proposed_restore_usd']) == D('.05')
    ledger = json.loads(raws['budget.json'])
    assert ledger['authorized_limit'] == 40 and held(ledger) == 40
    row, = [r for r in ledger['reservations'] if r['id'] == 'r1-20260928-21']
    assert row['reserved_usd'] == 1.35 and row['billed_usd'] is None and not row['reservation_released']
    stamp = dt.datetime.now(dt.timezone.utc).isoformat()
    event = {'event': 'usage_based_reservation_reconciliation', 'at': stamp, 'id': row['id'],
             'old_reserved_usd': 1.35, 'new_reserved_usd': 1.30, 'restored_to_available_budget_usd': .05,
             'evidence': 'experiments/hu-postflop-r1/cloud/vm21/usage-audit/report.json',
             'evidence_sha256': PINS['vm21/usage-audit/report.json'], 'billed_usd': None, 'final_invoice': False,
             'note': 'Only disk24h replaced by confirmed lifetime plus120s/upward-minute rounding; original47min smallCPU, overlapping10min highCPU, IPv4,320MiB and1USD retained. Missing usage remains unknown.'}
    row['reserved_usd'] = 1.30
    row.setdefault('events', []).append(event)
    assert held(ledger) == D('39.95')
    receipt = {'at_utc': stamp, 'authorized_limit_usd': 40, 'held_before_usd': 40,
               'held_after_usd': 39.95, 'restored_usd': .05, 'available_after_usd': .05,
               'change': event, 'budget_before': pin(raws['budget.json']),
               'evidence': {k: pin(v) for k, v in raws.items() if k != 'budget.json'},
               'application_source': pin(Path(__file__).read_bytes()), 'billed_usd': None,
               'final_invoice': False, 'cloud_mutations': False, 'archive_access': False}
    ledger.setdefault('usage_reconciliations', []).append(receipt)
    after = encode(ledger)
    receipt = {**receipt, 'budget_after': pin(after)}
    fresh(HERE / 'budget-before-own-usage-return.json', raws['budget.json'])
    fresh(HERE / 'usage-return-vm21-applied.json', encode(receipt))
    temporary = CLOUD / 'budget-vm21-return.tmp'
    fresh(temporary, after)
    assert (CLOUD / 'budget.json').read_bytes() == raws['budget.json']
    os.replace(temporary, CLOUD / 'budget.json')
    assert (CLOUD / 'budget.json').read_bytes() == after
    print(json.dumps({k: receipt[k] for k in ('restored_usd', 'held_after_usd', 'available_after_usd', 'budget_after')}))


if __name__ == '__main__':
    main()
