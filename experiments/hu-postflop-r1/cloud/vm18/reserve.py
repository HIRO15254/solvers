"""Reserve2.50USD for the frozen finite VM18 comparison; does not create resources."""
import datetime as dt
from decimal import Decimal as D
from fractions import Fraction
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent


def pin(raw):
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def main():
    path = CLOUD / 'budget.json'
    before = path.read_bytes()
    restored = json.loads((HERE / 'usage-applied.json').read_bytes())
    if pin(before) != restored['budget_after']:
        raise ValueError('Budget changed after reconciliation')
    ledger = json.loads(before)
    total = sum(D(str(r['reserved_usd'])) if not r['reservation_released'] else D(str(r['billed_usd'])) for r in ledger['reservations'])
    if total != D('37.5') or ledger['authorized_limit'] != 40 or any(r['id'] == 'r1-20260927-18' for r in ledger['reservations']):
        raise ValueError('Unexpected held budget or duplicate reservation')
    price_raw = (CLOUD / 'preflight-vm18/cost-proposal.json').read_bytes()
    price = json.loads(price_raw)
    estimate = Fraction(price['arithmetic']['total_usd']['fraction'])
    if estimate != Fraction(741731, 300000) or estimate >= Fraction('2.5'):
        raise ValueError('Resource estimate differs/exceeds budget')
    for label in ('instances', 'disks'):
        result = json.loads((HERE / f'preflight-{label}01.result.json').read_bytes())
        raw = (HERE / f'preflight-{label}01.stdout.log').read_bytes()
        if result['exit_code'] != 0 or result['stdout'] != pin(raw) or json.loads(raw) != []:
            raise ValueError('Unexpected existing resource/read failure')
        if result['argv'][1:] != ['compute', label, 'list', '--project=solvers-abstraction-20260723',
                                '--filter=name~solvers-r1-', '--format=json', '--quiet']:
            raise ValueError('Preflight command scope differs')
    stamp = dt.datetime.now(dt.timezone.utc).isoformat()
    row = {'id': 'r1-20260927-18', 'project': 'solvers-abstraction-20260723', 'zone': 'us-central1-b',
           'instance': 'solvers-r1-20260927-18', 'reserved_usd': 2.5, 'billed_usd': None, 'reservation_released': False,
           'maximum_runtime_seconds': 3600, 'maximum_starts': 3, 'billing_rounding_slack_seconds': 120,
           'machine_type': 'e2-standard-2', 'measurement_machine_type': 'e2-highcpu-32',
           'capacity_or_quota_fallback': 'n2-highcpu-32 on same stopped instance after explicit E2 capacity failure only; original STOP unchanged',
           'provisioning_model': 'SPOT', 'disk_type': 'pd-balanced', 'disk_gib': 40, 'maximum_download_gib': 0.5,
           'maximum_archive_bytes': 268435456, 'conservative_compute_usd_hour': 1.15,
           'disk_usd_gib_hour': .000137, 'ipv4_usd_hour': .0025, 'reserved_egress_usd_gib': .3,
           'tax_price_delay_and_other_reserve_usd': 1, 'instance_termination_action': 'STOP',
           'explicit_disk_cleanup_hours': 24, 'auto_restart': False, 'reserved_at_utc': stamp,
           'pricing_checked_utc': '2026-09-27', 'pricing_evidence': 'experiments/hu-postflop-r1/cloud/preflight-vm18/cost-proposal.json',
           'purpose': 'Portable2CPU build/core tests followed by32CPU fixed38-solve chance-depth1 versus2 comparison',
           'preflight_evidence': 'experiments/hu-postflop-r1/cloud/vm18/',
           'note': 'Single instance, launch+20min build deadline,20min measurement,15min recovery before fixed60min STOP. All38 solves share32CPU boot and binary; no local native work. Full62min highest regular rate plus40GiB24h disk,.5GiB egress,$1 uncertainty models2.472436667. Estimate only; billed unknown.'}
    ledger['reservations'].append(row)
    after = (json.dumps(ledger, indent=2, ensure_ascii=False) + '\n').encode()
    record = {'at_utc': stamp, 'held_before_usd': 37.5, 'held_after_usd': 40, 'available_usd': 0,
              'authorized_limit_usd': 40, 'estimate_usd': price['arithmetic']['total_usd'],
              'budget_before': pin(before), 'budget_after': pin(after), 'price_proposal': pin(price_raw),
              'cloud_launched': False, 'billed_usd': None}
    for name, raw in [('budget-before.json', before), ('reservation.json', (json.dumps(row, indent=2)+'\n').encode()),
                      ('reservation-check.json', (json.dumps(record, indent=2)+'\n').encode())]:
        with (HERE / name).open('xb') as stream:
            stream.write(raw)
    path.write_bytes(after)
    print(json.dumps(record))


if __name__ == '__main__':
    main()
