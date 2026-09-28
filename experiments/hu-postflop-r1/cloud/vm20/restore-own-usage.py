"""Apply the audited VM20 unused reservation; no cloud or archive access."""
import datetime as dt
from decimal import Decimal as D
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent
AUDIT = CLOUD / 'usage-audit-vm20'
PINS = {
    'budget.json': '4d1f06be12404791c6791881ed2311db6a29c8b7d948f63767f7a5147aa0f055',
    'usage-audit-vm20/report02.json': '01f63fea326fb7c66ba64814b994e41f6ce0e2f1965d72a6e416cf89804d47fd',
    'usage-audit-vm20/analyze02.py': 'b7cc4dfdf3519b4edb512f68f9c4a2f1aadd213d5398f8433532b06a018dd488',
    'usage-audit-vm20/analyze.py': 'cfff88d4810ff2732c4112307e5bc2729eb00f4b577350c5aabf3f1a8c708f5e',
    'usage-audit-vm20/report.json': '53a4b5efc92ec9d0030d61b204a53cc4e7170556e08575a5599e84696e1046e1',
}


def pin(raw):
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def encode(value):
    return (json.dumps(value, indent=2, ensure_ascii=False) + '\n').encode()


def write_new(path, raw):
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
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(AUDIT))
    spec = importlib.util.spec_from_file_location('vm20_audited_proposal', AUDIT / 'analyze02.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    report = module.build_report()
    assert report == json.loads(raws['usage-audit-vm20/report02.json'])
    cost = report['cost']
    assert (cost['whole_lifecycle_minutes'], cost['overlapping_highcpu_minutes'], cost['disk_minutes']) == (35, 10, 35)
    model = D(35)/60*(D('.06701142')+D('.0025')+20*D('.000137')) + D(10)/60*D('.79152384') + D('.15') + 1
    assert abs(model - D(cost['model_usd'])) < D('1e-24')
    assert D(cost['components_usd']['original_uncertainty_reserve']) == 1
    assert D(cost['network_allowance_gib']) == D('.5')
    assert D('1.30') < model <= D('1.35')
    assert D(cost['proposed_hold_usd']) == D('1.35') and D(cost['proposed_restore_usd']) == D('.50')
    ledger = json.loads(raws['budget.json'])
    assert ledger['authorized_limit'] == 40 and held(ledger) == D('39.15')
    row, = [r for r in ledger['reservations'] if r['id'] == 'r1-20260928-20']
    assert row['reserved_usd'] == 1.85 and row['billed_usd'] is None and not row['reservation_released']
    stamp = dt.datetime.now(dt.timezone.utc).isoformat()
    event = {'event': 'usage_based_reservation_reconciliation', 'at': stamp, 'id': row['id'],
             'old_reserved_usd': 1.85, 'new_reserved_usd': 1.35, 'restored_to_available_budget_usd': .50,
             'evidence': 'experiments/hu-postflop-r1/cloud/usage-audit-vm20/report02.json',
             'evidence_sha256': PINS['usage-audit-vm20/report02.json'], 'billed_usd': None, 'final_invoice': False,
             'note': 'Original 1USD and 512MiB allowances retained; whole lifecycle plus overlapping highCPU window, each with120s padding. Missing Monitoring usage remains unknown. Explicit0.05USD upper rounding.'}
    row['reserved_usd'] = 1.35
    row.setdefault('events', []).append(event)
    assert held(ledger) == D('38.65')
    receipt = {'at_utc': stamp, 'authorized_limit_usd': 40, 'held_before_usd': 39.15,
               'held_after_usd': 38.65, 'restored_usd': .50, 'available_after_usd': 1.35,
               'change': event, 'budget_before': pin(raws['budget.json']),
               'evidence': {k: pin(v) for k, v in raws.items() if k != 'budget.json'},
               'application_source': pin(Path(__file__).read_bytes()), 'billed_usd': None,
               'final_invoice': False, 'cloud_mutations': False, 'archive_access': False}
    ledger.setdefault('usage_reconciliations', []).append(receipt)
    after = encode(ledger)
    receipt = {**receipt, 'budget_after': pin(after)}
    write_new(HERE / 'budget-before-own-usage-return.json', raws['budget.json'])
    write_new(HERE / 'usage-return-vm20-applied.json', encode(receipt))
    temp = CLOUD / 'budget-vm20-return.tmp'
    write_new(temp, after)
    path = CLOUD / 'budget.json'
    assert path.read_bytes() == raws['budget.json']
    os.replace(temp, path)
    assert path.read_bytes() == after
    print(json.dumps({k: receipt[k] for k in ('restored_usd', 'held_after_usd', 'available_after_usd', 'budget_after')}))


if __name__ == '__main__':
    main()
