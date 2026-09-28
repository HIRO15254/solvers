"""Reserve1.85USD for VM20 after reviewed usage reconciliation; no cloud call."""
import datetime as dt
from decimal import Decimal as D
from fractions import Fraction
import hashlib
import importlib.util
import json
import os
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent


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


def main():
    path = CLOUD / 'budget.json'
    before = path.read_bytes()
    restored = json.loads((HERE / 'usage-applied.json').read_bytes())
    need(pin(before) == restored['budget_after'], 'Ledger changed since usage reconciliation')
    ledger = json.loads(before)
    total = sum(D(str(r['billed_usd'] if r['reservation_released'] else r['reserved_usd']))
                for r in ledger['reservations'])
    need(total == D('37.3') and ledger['authorized_limit'] == 40, 'Expected40USD authorization and37.30hold')
    need(not any(r['id'] == 'r1-20260928-20' for r in ledger['reservations']), 'Duplicate reservation')
    price_raw = (CLOUD / 'preflight-vm20/cost-proposal.json').read_bytes()
    price = json.loads(price_raw)
    spec = importlib.util.spec_from_file_location('vm20_cost', CLOUD / 'preflight-vm20/cost-proposal.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    need(price['arithmetic'] == module.calculate(), 'Price evidence replay differs')
    need(price['calculator'] == module.pin(CLOUD / 'preflight-vm20/cost-proposal.py'), 'Price source changed')
    need(Fraction(price['arithmetic']['total_usd']['fraction']) == Fraction(368877, 200000), 'Estimate differs')
    now = dt.datetime.now(dt.timezone.utc)
    for label in ('instances', 'disks', 'addresses'):
        result = json.loads((HERE / f'preflight-{label}01.result.json').read_bytes())
        raw = (HERE / f'preflight-{label}01.stdout.log').read_bytes()
        need(result['exit_code'] == 0 and result['stdout'] == pin(raw) and json.loads(raw) == [],
             'Unexpected existing resource or inventory read failure')
        need(result['argv'][1:] == ['compute', label, 'list', '--project=solvers-abstraction-20260723',
                                   '--filter=name~solvers-r1-', '--format=json', '--quiet'], 'Inventory scope differs')
        need(0 <= (now - dt.datetime.fromisoformat(result['ended_utc'])).total_seconds() < 900,
             'Inventory older than15min')
    stamp = now.isoformat()
    row = {'id': 'r1-20260928-20', 'project': 'solvers-abstraction-20260723', 'zone': 'us-central1-b',
           'instance': 'solvers-r1-20260928-20', 'reserved_usd': 1.85, 'billed_usd': None,
           'reservation_released': False, 'maximum_runtime_seconds': 2700, 'maximum_starts': 3,
           'billing_rounding_slack_seconds': 120, 'machine_type': 'e2-standard-2',
           'measurement_machine_type': 'e2-highcpu-32', 'capacity_or_quota_fallback': None,
           'provisioning_model': 'SPOT', 'disk_type': 'pd-balanced', 'disk_gib': 20,
           'maximum_download_gib': 0.5, 'maximum_archive_bytes': 268435456,
           'conservative_compute_usd_hour': .80, 'disk_usd_gib_hour': .000137,
           'ipv4_usd_hour': .0025, 'reserved_egress_usd_gib': .3,
           'tax_price_delay_and_other_reserve_usd': 1, 'instance_termination_action': 'STOP',
           'explicit_disk_cleanup_hours': 24, 'auto_restart': False, 'reserved_at_utc': stamp,
           'pricing_checked_utc': '2026-09-28',
           'pricing_evidence': 'experiments/hu-postflop-r1/cloud/preflight-vm20/cost-proposal.json',
           'preflight_evidence': 'experiments/hu-postflop-r1/cloud/vm20/',
           'deployment_template': 'experiments/hu-postflop-r1/cloud/vm19/pack-receipt.json',
           'purpose': 'Baseline CPU profile:2CPU portable build/core tests,32CPU boot for10 N64 solves,2CPU recovery',
           'note': 'E2 only.45min original STOP;20min build upper bound but measurement dispatch by launch+15min, then15min measurement and15min recovery.47min at0.80USD/h plus20GiB24h disk,512MiB egress and original1USD uncertainty =1.844385USD. No local native work; billed unknown.'}
    ledger['reservations'].append(row)
    after = (json.dumps(ledger, indent=2, ensure_ascii=False) + '\n').encode()
    receipt = {'at_utc': stamp, 'held_before_usd': 37.3, 'held_after_usd': 39.15, 'available_usd': .85,
               'authorized_limit_usd': 40, 'estimate_usd': price['arithmetic']['total_usd'],
               'budget_before': pin(before), 'budget_after': pin(after), 'price_proposal': pin(price_raw),
               'cloud_launched': False, 'billed_usd': None}
    write_new(HERE / 'budget-before.json', before)
    write_new(HERE / 'reservation.json', (json.dumps(row, indent=2) + '\n').encode())
    write_new(HERE / 'reservation-check.json', (json.dumps(receipt, indent=2) + '\n').encode())
    temporary = CLOUD / 'budget-vm20-reserve.tmp'
    write_new(temporary, after)
    os.replace(temporary, path)
    need(path.read_bytes() == after, 'Written ledger differs')
    print(json.dumps(receipt))


if __name__ == '__main__':
    main()
