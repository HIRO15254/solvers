"""Apply the independently reproducible four-VM usage proposal once; no cloud calls."""
import datetime as dt
from decimal import Decimal as D
import hashlib
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent
AUDIT = CLOUD / 'usage-lifecycle-20260927'


def pin(raw):
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def main():
    before = (CLOUD / 'budget.json').read_bytes()
    if before != (AUDIT / 'budget-before.json').read_bytes():
        raise ValueError('Budget changed since audited snapshot')
    if pin(before)['sha256'] != '8e28e34709452c791980f268ea44edd5f2bd2a14a1cc21eccdeb365d16549901':
        raise ValueError('Unexpected budget snapshot')
    spec = importlib.util.spec_from_file_location('usage_audit', AUDIT / 'analyze.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    report = module.build_report()
    report_raw = (AUDIT / 'report02.json').read_bytes()
    if pin(report_raw)['sha256'] != '814cc041fd1947eecfa53c2caf90d33632d8e280b26e66caa5a5ee573a487714':
        raise ValueError('Audited report pin differs')
    if module.read(AUDIT / 'report02.json') != report:
        raise ValueError('Saved audit differs from recomputation')
    ledger = json.loads(before)
    if ledger['authorized_limit'] != 40 or D(report['summary']['restore_usd']) != D('2.30'):
        raise ValueError('Authorization/restoration differs')
    stamp = dt.datetime.now(dt.timezone.utc).isoformat()
    report_pin = pin(report_raw)
    changes = []
    for item in report['rows']:
        row, = [r for r in ledger['reservations'] if r['id'] == item['reservation_id']]
        if row['reservation_released'] or D(str(row['reserved_usd'])) != D(item['held_before_usd']):
            raise ValueError('Reservation changed')
        event = {'event': 'usage_based_lifecycle_reservation_reconciliation', 'at': stamp,
                 'id': row['id'], 'old_reserved_usd': row['reserved_usd'],
                 'new_reserved_usd': float(D(item['proposed_hold_usd'])),
                 'restored_to_available_budget_usd': float(D(item['restore_usd'])),
                 'evidence': 'experiments/hu-postflop-r1/cloud/usage-lifecycle-20260927/report02.json',
                 'evidence_sha256': report_pin['sha256'], 'billed_usd': None, 'final_invoice': False,
                 'note': 'Original uncertainty retained; same-ID lifecycle and acquired traffic support smaller unused resource allowance. Missing traffic unknown.'}
        row['reserved_usd'] = event['new_reserved_usd']
        row.setdefault('events', []).append(event)
        changes.append(event)
    held = sum(D(str(r['reserved_usd'])) if not r['reservation_released'] else D(str(r['billed_usd'])) for r in ledger['reservations'])
    if held != D('37.50'):
        raise ValueError('Unexpected reconciled total')
    receipt = {'at_utc': stamp, 'authorized_limit_usd': 40, 'held_before_usd': 39.8,
               'held_after_usd': 37.5, 'restored_usd': 2.3, 'available_after_usd': 2.5,
               'changes': changes, 'report': report_pin, 'budget_before': pin(before),
               'billed_usd': None, 'final_invoice': False, 'cloud_mutations': False}
    ledger.setdefault('usage_reconciliations', []).append(receipt)
    after = (json.dumps(ledger, indent=2, ensure_ascii=False) + '\n').encode()
    receipt['budget_after'] = pin(after)
    with (HERE / 'budget-before-usage.json').open('xb') as stream:
        stream.write(before)
    with (HERE / 'usage-applied.json').open('x') as stream:
        json.dump(receipt, stream, indent=2)
        stream.write('\n')
    (CLOUD / 'budget.json').write_bytes(after)
    print(json.dumps({k: receipt[k] for k in ('held_before_usd', 'held_after_usd', 'restored_usd', 'available_after_usd', 'budget_after')}))


if __name__ == '__main__':
    main()
