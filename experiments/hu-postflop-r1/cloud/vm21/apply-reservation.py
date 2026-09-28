"""Root's finite VM21 reservation application, after source/control review."""
import datetime as dt
from decimal import Decimal as D
import hashlib
import json
import os
from pathlib import Path
import re
import reserve as r


def encoded(value):
    return (json.dumps(value, ensure_ascii=False, indent=2) + '\n').encode()


def digest(raw):
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def write_new(path, raw):
    with path.open('xb') as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())


def main():
    path = r.CLOUD / 'budget.json'
    before = path.read_bytes()
    restored = r.read(r.CLOUD / 'vm20/usage-return-vm20-applied.json')
    r.need(digest(before) == restored['budget_after'], 'Restored ledger changed')
    ledger = json.loads(before)
    held = sum(D(str(x['billed_usd'] if x['reservation_released'] else x['reserved_usd']))
               for x in ledger['reservations'])
    r.need(ledger['authorized_limit'] == 40 and held == D('38.65')
           and not any(x['id'] == r.ID for x in ledger['reservations']), 'Finite remaining funds differ')
    approval = r.read(r.HERE / 'launch-approval.json')
    r.need(approval['package_review_passed'] is True and approval['resource'] == r.NAME,
           'Completed package/control review required')
    for name, identity in approval['files'].items():
        r.need(r.pin(r.HERE / name) == identity, 'Reviewed launch source differs')
    prices = r.CLOUD / 'preflight-vm20'
    sources = r.read(prices / 'pricing-sources.json')
    for attempt in sources['attempts']:
        r.need(attempt['status'] == 'acquired' and attempt['source'].startswith('https://cloud.google.com/'),
               'Official current price source unavailable')
        for ref in attempt['retained']:
            r.need(r.pin(prices / ref['path']) == {k: ref[k] for k in ('bytes', 'sha256')},
                   'Retained official price changed')
    for machine, price in (('e2-standard-2', '.06701142'), ('e2-highcpu-32', '.79152384')):
        html = (prices / f'pricing-compute-{machine}-row.html').read_text()
        r.need(machine in html and D(re.findall(r'\$([0-9.]+) / 1 hour', html)[0]) == D(price),
               'Ordinary E2 price changed')
    now = dt.datetime.now(dt.timezone.utc)
    r.need(now.date() == dt.date(2026, 9, 28), 'Same-day price review required')
    for kind in ('instances', 'disks', 'addresses'):
        stem = 'preflight-' + kind + '01'
        receipt = r.read(r.HERE / (stem + '.result.json'))
        r.need(receipt['exit_code'] == 0 and r.read(r.HERE / (stem + '.stdout.log')) == [],
               'Campaign resources remain or inventory failed')
        for channel in ('stdout', 'stderr'):
            r.need(receipt[channel] == r.pin(r.HERE / (stem + '.' + channel + '.log')), 'Inventory changed')
        r.need(receipt['argv'][1:] == ['compute', kind, 'list', '--project=' + r.PROJECT,
                                     '--filter=name~solvers-r1-', '--format=json', '--quiet'], 'Inventory scope differs')
        r.need(0 <= (now - r.utc(receipt['ended_utc'])).total_seconds() < 900, 'Inventory older than15min')
    estimate = D(47)/60*(D('.06701142')+D('.0025')) + D(10)/60*D('.80') + D(20)*24*D('.000137') + D(320)/1024*D('.30') + 1
    r.need(abs(estimate-D(r.arithmetic()['total_usd'])) < D('1e-24') and estimate <= D('1.35'),
           'Independent arithmetic differs')
    row = {**r.proposal(), 'reserved_at_utc': now.isoformat(),
           'pricing_checked_utc': now.isoformat(),
           'pricing_evidence': 'experiments/hu-postflop-r1/cloud/preflight-vm20/pricing-sources.json',
           'new_envelope_evidence': 'experiments/hu-postflop-r1/cloud/vm21/budget-proposal.json'}
    ledger['reservations'].append(row)
    after = encoded(ledger)
    receipt = {'at_utc': now.isoformat(), 'held_before_usd': 38.65, 'held_after_usd': 40,
               'available_usd': 0, 'authorized_limit_usd': 40, 'estimate_usd': r.arithmetic()['total_usd'],
               'budget_before': digest(before), 'budget_after': digest(after),
               'reservation': digest(encoded(row)), 'launch_approval': r.pin(r.HERE / 'launch-approval.json'),
               'application_source': r.pin(Path(__file__)), 'cloud_launched': False, 'billed_usd': None}
    write_new(r.HERE / 'budget-before.json', before)
    write_new(r.HERE / 'reservation.json', encoded(row))
    write_new(r.HERE / 'reservation-check.json', encoded(receipt))
    temporary = r.CLOUD / 'budget-vm21-reserve.tmp'
    write_new(temporary, after)
    r.need(path.read_bytes() == before, 'Ledger raced before application')
    os.replace(temporary, path)
    r.need(path.read_bytes() == after, 'Ledger readback differs')
    print(json.dumps(receipt))


if __name__ == '__main__':
    main()
