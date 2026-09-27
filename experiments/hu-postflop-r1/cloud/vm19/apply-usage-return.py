"""Apply the independently reviewed three-VM E2 lifecycle return once."""
import datetime as dt
from decimal import Decimal as D
import hashlib
import importlib.util
import json
import os
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent
AUDIT = CLOUD / 'usage-lifecycle-vm16-vm17'
REPORT_SHA = '5e2c6be9727a5360eaaa84838b05ba5a957888b82f15ecf2f2b54c315e4a81f2'
SOURCE_SHA = 'ecfe33c0c05bd04c216dfe43d01efb70b6eefb6112aaf5cde39b8d9c0df4018b'
BUDGET_SHA = 'dd3d8ae625fd9785f42ee9b2ec96bc54f79dbc143ec96e12bce61c1caea6c535'


def need(value, message):
    if not value:
        raise ValueError(message)


def pin(raw):
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def write_new(path, raw):
    with path.open('xb') as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())


def held(ledger):
    return sum(D(str(r['billed_usd'] if r['reservation_released'] else r['reserved_usd']))
               for r in ledger['reservations'])


def main():
    before = (CLOUD / 'budget.json').read_bytes()
    need(pin(before)['sha256'] == BUDGET_SHA and before == (AUDIT / 'inputs/budget.json').read_bytes(),
         'Reviewed ledger changed')
    report_raw, source_raw = (AUDIT / 'report.json').read_bytes(), (AUDIT / 'analyze.py').read_bytes()
    need(pin(report_raw)['sha256'] == REPORT_SHA and pin(source_raw)['sha256'] == SOURCE_SHA,
         'Independently reviewed source/report changed')
    spec = importlib.util.spec_from_file_location('reviewed_e2_usage', AUDIT / 'analyze.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    report = module.build_report()
    need(report == json.loads(report_raw), 'Evidence replay differs')
    need(report['proposal'] == {'held_before_usd': '39', 'restore_usd': '0.9', 'held_after_usd': '38.1',
                               'available_after_usd': '1.9', 'applied': False}, 'Reviewed proposal differs')
    ledger = json.loads(before)
    need(ledger['authorized_limit'] == 40 and held(ledger) == D(39), 'Authorization/held total differs')
    stamp = dt.datetime.now(dt.timezone.utc).isoformat()
    changes = []
    for vm, old, new, minutes in [(16, '1.9', '1.4', (26, 13, 26)),
                                  (17, '1.8', '1.5', (39, 21, 40)),
                                  (18, '1.5', '1.4', (34, 13, 34))]:
        observation, = [r for r in report['vms'] if r['vm'] == vm]
        arithmetic = observation['arithmetic']
        need(tuple(arithmetic[k] for k in ('whole_minutes', 'overlapping_large_minutes', 'disk_minutes')) == minutes,
             'Reviewed lifecycle bounds differ')
        whole, large, disk = map(D, minutes)
        model = whole / 60 * (D('.06701142') + D('.0025')) + large / 60 * D('.79152384')
        model += disk / 60 * 40 * D('.000137') + D('.5') * D('.30') + 1
        need(abs(D(arithmetic['total_usd']) - model) < D('1e-24') and model < D(new), 'Independent arithmetic differs')
        row, = [r for r in ledger['reservations'] if r['id'] == f'r1-20260927-{vm}']
        need(D(str(row['reserved_usd'])) == D(old) and row['billed_usd'] is None and not row['reservation_released'],
             'Reservation already changed')
        need(row['tax_price_delay_and_other_reserve_usd'] == 1 and row['maximum_download_gib'] == .5,
             'Original uncertainty/transfer reserve differs')
        event = {'event': 'usage_based_e2_lifecycle_reservation_reconciliation', 'at': stamp, 'id': row['id'],
                 'old_reserved_usd': float(old), 'new_reserved_usd': float(new),
                 'restored_to_available_budget_usd': float(D(old) - D(new)),
                 'evidence': 'experiments/hu-postflop-r1/cloud/usage-lifecycle-vm16-vm17/report.json',
                 'evidence_sha256': REPORT_SHA, 'billed_usd': None, 'final_invoice': False,
                 'note': 'User-authorized unused reservation reuse; actual E2 regular prices, overlapping conservative lifecycle windows, original1USD uncertainty and512MiB allowance retained. Missing usage remains unknown.'}
        row['reserved_usd'] = float(new)
        row.setdefault('events', []).append(event)
        changes.append(event)
    need(held(ledger) == D('38.1'), 'Reconciled held total differs')
    receipt = {'at_utc': stamp, 'authorized_limit_usd': 40, 'held_before_usd': 39, 'held_after_usd': 38.1,
               'restored_usd': .9, 'available_after_usd': 1.9, 'changes': changes,
               'report': pin(report_raw), 'analyzer': pin(source_raw), 'budget_before': pin(before),
               'application_source': pin(Path(__file__).read_bytes()),
               'independent_review': {'agent': 'profile_raw_storage', 'result': 'No blocking findings',
                                      'checks': '576 input pins; IDs; deletion and absence; lifecycle bounds; prices; transfers; independent Decimal arithmetic'},
               'billed_usd': None, 'final_invoice': False, 'cloud_mutations': False, 'archive_access': False}
    ledger.setdefault('usage_reconciliations', []).append(receipt)
    after = (json.dumps(ledger, indent=2, ensure_ascii=False) + '\n').encode()
    receipt = {**receipt, 'budget_after': pin(after)}
    need(not (HERE / 'usage-applied.json').exists(), 'Application receipt already exists')
    write_new(HERE / 'budget-before-usage-return.json', before)
    temporary = CLOUD / 'budget-vm19-return.tmp'
    write_new(temporary, after)
    os.replace(temporary, CLOUD / 'budget.json')
    need((CLOUD / 'budget.json').read_bytes() == after, 'Written ledger differs')
    write_new(HERE / 'usage-applied.json', (json.dumps(receipt, indent=2) + '\n').encode())
    print(json.dumps({k: receipt[k] for k in ('held_before_usd', 'held_after_usd', 'restored_usd', 'available_after_usd', 'budget_after')}))


if __name__ == '__main__':
    main()
