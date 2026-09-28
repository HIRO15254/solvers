"""Apply two independently reviewed usage proposals atomically; no cloud call."""
import datetime as dt
from decimal import Decimal as D
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent
PINS = {
    'budget.json': 'ad2613a9d22c6ebe4284068ece3186e8e05e9a098eb9d8e6e907cef40f8e903f',
    'usage-followup-20260928/report.json': '83cd8ceb6897a4dc53e9eb8865435a69e036013a174ddc2ba30d81ca3024e72d',
    'usage-followup-20260928/validate.py': 'b69bd2c983d805b5dcf4e4b0aca0e73cfb214ffba6939e1737a126f45244e0b6',
    'usage-audit-vm19/report.json': '803d7263f80f6c60d3132f7c4216577e382ac0de718a54cafc23ca7dc7790358',
    'usage-audit-vm19/analyze.py': 'a6c2963f58aa5d2bb4930590a70075552ef1392ab3bf11b1b7792896ec8a870d',
}


def pin(raw):
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def load(name):
    path = CLOUD / name
    spec = importlib.util.spec_from_file_location('apply_' + path.parent.name.replace('-', '_'), path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def write(path, raw):
    with path.open('xb') as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())


def held(ledger):
    return sum(D(str(r['billed_usd'] if r['reservation_released'] else r['reserved_usd'])) for r in ledger['reservations'])


def main():
    raws = {name: (CLOUD / name).read_bytes() for name in PINS}
    assert all(pin(raw)['sha256'] == PINS[name] for name, raw in raws.items())
    budget_path = CLOUD / 'budget.json'
    ledger = json.loads(raws['budget.json'])
    assert ledger['authorized_limit'] == 40 and held(ledger) == D('39.95')
    older = load('usage-followup-20260928/validate.py').build_report(budget_path)
    recent = load('usage-audit-vm19/analyze.py').build_report()
    assert older == json.loads(raws['usage-followup-20260928/report.json'])
    assert recent == json.loads(raws['usage-audit-vm19/report.json'])
    assert older['summary']['restore_usd'] == '2.10'
    assert recent['cost']['proposed_restore_usd'] == '0.55'
    # Independent Decimal arithmetic, without reusing the proposal calculators.
    allocation = {}
    for row in older['rows']:
        costs = row['costs_usd']
        assert D(costs['unchanged_one_gib_network_allowance']) == D('.30')
        assert D(costs['unchanged_original_uncertainty']) == (2 if row['vm'] == '08' else 1)
        assert sum(map(D, costs.values())) == D(row['modeled_total_usd']) <= D(row['proposed_hold_usd'])
        assert sum(map(D, row['reservation_allocation_usd'].values())) == D(row['proposed_hold_usd'])
        for key, value in row['reservation_allocation_usd'].items():
            assert key not in allocation
            allocation[key] = (D(value), 'usage-followup-20260928/report.json')
    cost = recent['cost']
    assert (cost['original_window_minutes'], cost['recovery_window_minutes'], cost['disk_minutes']) == (43, 9, 1180)
    model = D(52) / 60 * (D('.06701142') + D('.0025')) + D(1180) / 60 * 20 * D('.000137') + D('.15') + 1
    assert abs(model - D(cost['model_usd'])) < D('1e-24') and model < D('1.30')
    allocation['r1-20260927-19'] = (D('1.30'), 'usage-audit-vm19/report.json')
    stamp = dt.datetime.now(dt.timezone.utc).isoformat()
    changes = []
    for row in ledger['reservations']:
        if row['id'] not in allocation:
            continue
        new, source = allocation.pop(row['id'])
        old = D(str(row['reserved_usd']))
        assert not row['reservation_released'] and row['billed_usd'] is None and new <= old
        if new == old:
            continue
        event = {'event': 'usage_based_reservation_reconciliation', 'at': stamp, 'id': row['id'],
                 'old_reserved_usd': float(old), 'new_reserved_usd': float(new),
                 'restored_to_available_budget_usd': float(old-new), 'evidence': 'experiments/hu-postflop-r1/cloud/' + source,
                 'evidence_sha256': PINS[source], 'billed_usd': None, 'final_invoice': False,
                 'note': 'Original uncertainty and transfer allowances retained. Missing Monitoring intervals remain unknown. VM19 overnight disk lifetime included.'}
        row['reserved_usd'] = float(new)
        row.setdefault('events', []).append(event)
        changes.append(event)
    assert not allocation and held(ledger) == D('37.30')
    receipt = {'at_utc': stamp, 'authorized_limit_usd': 40, 'held_before_usd': 39.95, 'held_after_usd': 37.30,
               'restored_usd': 2.65, 'available_after_usd': 2.70, 'changes': changes,
               'budget_before': pin(raws['budget.json']), 'evidence': {k: pin(v) for k,v in raws.items() if k != 'budget.json'},
               'application_source': pin(Path(__file__).read_bytes()), 'billed_usd': None, 'final_invoice': False,
               'cloud_mutations': False, 'archive_access': False}
    ledger.setdefault('usage_reconciliations', []).append(receipt)
    after = (json.dumps(ledger, indent=2, ensure_ascii=False) + '\n').encode()
    receipt = {**receipt, 'budget_after': pin(after)}
    write(HERE / 'budget-before-usage-return.json', raws['budget.json'])
    write(HERE / 'usage-applied.json', (json.dumps(receipt, indent=2, ensure_ascii=False) + '\n').encode())
    temporary = CLOUD / 'budget-vm20-usage.tmp'
    write(temporary, after)
    assert budget_path.read_bytes() == raws['budget.json']
    os.replace(temporary, budget_path)
    assert budget_path.read_bytes() == after
    print(json.dumps({k: receipt[k] for k in ('restored_usd', 'held_after_usd', 'available_after_usd', 'budget_after')}))


if __name__ == '__main__':
    main()
