"""Prepare a VM21 reservation proposal; never change the shared ledger or call GCP."""
import datetime as dt
from decimal import Decimal as D
import hashlib
import json
import os
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent
ID = 'r1-20260928-21'
NAME = 'solvers-r1-20260928-21'
PROJECT = 'solvers-abstraction-20260723'
ZONE = 'us-central1-b'


def need(value, message):
    if not value:
        raise ValueError(message)


def pin(path):
    need(path.is_file() and not path.is_symlink(), 'Regular file required: ' + str(path))
    raw = path.read_bytes()
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def read(path):
    return json.loads(path.read_bytes())


def utc(value):
    stamp = dt.datetime.fromisoformat(value.replace('Z', '+00:00'))
    need(stamp.utcoffset() == dt.timedelta(0), 'Explicit UTC required')
    return stamp


def fresh_json(path, value):
    with path.open('x', encoding='utf-8', newline='\n') as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())


def arithmetic():
    # Upper-bound regular-price inputs supplied by root; this is not a quote or invoice.
    terms = {'small_compute': D(47) / 60 * D('.06701142'),
             'large_compute': D(10) / 60 * D('.80'),
             'disk_20_gib_24_hours': D(20) * 24 * D('.000137'),
             'egress_320_mib': D(320) / 1024 * D('.30'),
             'ipv4': D(47) / 60 * D('.0025'),
             'uncertainty': D(1)}
    total = sum(terms.values())
    need(total <= D('1.35'), 'Cost envelope exceeds reservation')
    return {'terms_usd': {k: str(v) for k, v in terms.items()},
            'total_usd': str(total), 'reserve_usd': '1.35',
            'headroom_usd': str(D('1.35') - total),
            'small_and_large_compute_overlap_intentionally': True,
            'observed_usage_or_bill': False}


def proposal():
    return {'id': ID, 'project': PROJECT, 'zone': ZONE, 'instance': NAME,
            'reserved_usd': 1.35, 'billed_usd': None, 'reservation_released': False,
            'maximum_runtime_seconds': 2700, 'maximum_starts': 3,
            'maximum_large_phase_seconds': 480, 'billing_rounding_slack_seconds': 120,
            'machine_type': 'e2-standard-2', 'measurement_machine_type': 'e2-highcpu-32',
            'provisioning_model': 'SPOT', 'capacity_or_quota_fallback': None,
            'disk_type': 'pd-balanced', 'disk_gib': 20, 'explicit_disk_cleanup_hours': 24,
            'maximum_download_gib': 0.3125, 'maximum_archive_bytes': 268435456,
            'small_compute_usd_hour': 0.06701142, 'large_compute_usd_hour': 0.80,
            'disk_usd_gib_hour': 0.000137, 'ipv4_usd_hour': 0.0025,
            'reserved_egress_usd_gib': 0.30, 'tax_price_delay_and_other_reserve_usd': 1,
            'instance_termination_action': 'STOP', 'auto_restart': False,
            'purpose': 'Sparse rank groups: fresh paired build/tests on 2 CPUs, bounded 16/32-worker measurement on one 32-CPU boot, 2-CPU recovery'}


def authorized():
    ledger_path = CLOUD / 'budget.json'
    ledger = read(ledger_path)
    rows = [row for row in ledger['reservations'] if row['id'] == ID]
    need(len(rows) == 1, 'Exactly one root-written VM21 reservation required')
    row = rows[0]
    for key, value in proposal().items():
        need(row.get(key) == value, 'Reservation envelope differs: ' + key)
    total = sum(D(str(r['billed_usd'] if r['reservation_released'] else r['reserved_usd']))
                for r in ledger['reservations'])
    need(ledger['authorized_limit'] == 40 and total <= D(40), 'Shared authorization exceeded')
    check = read(HERE / 'reservation-check.json')
    need(check['budget_after'] == pin(ledger_path), 'Root-reviewed ledger changed')
    need(check['reservation'] == pin(HERE / 'reservation.json'), 'Reservation receipt differs')
    need(read(HERE / 'reservation.json') == row, 'Reservation differs from shared ledger')
    need(arithmetic()['total_usd'] == check['estimate_usd'], 'Reviewed arithmetic differs')
    return row, check


def main():
    result = {'schema': 'r1.vm21-budget-proposal/v1', 'reservation': proposal(),
              'arithmetic': arithmetic(), 'pricing_verification_required': True,
              'ledger_written': False, 'cloud_mutations': False,
              'note': 'Root must verify current pricing/available funds and apply the reservation before launch. 47 small-CPU minutes plus 10 large-CPU minutes includes overlapping conservative charges; the large phase itself is at most 480 seconds.'}
    fresh_json(HERE / 'budget-proposal.json', result)
    print(json.dumps(result))


if __name__ == '__main__':
    main()
