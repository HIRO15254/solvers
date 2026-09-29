"""Read small retained records and propose disk-only adjustment; no API/ledger writes."""
import argparse
import datetime as dt
from decimal import Decimal as D, ROUND_CEILING
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLOUD = HERE.parent
PROJECT = 'solvers-abstraction-20260723'
ZONE = 'us-central1-b'
IDS = {'02': '7774326211091312507', '05': '1627891813360280286'}
EXPECTED_BUDGET = '5f57f5a932540fc8d7c88b25787d1c2e98903c729bd1eda184857415f905a825'


def need(value, message):
    if not value:
        raise ValueError(message)


def pin(path):
    need(path.is_file() and not path.is_symlink() and path.stat().st_size < 1024**2, 'Small regular input required')
    data = path.read_bytes()
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def stamp(value):
    result = dt.datetime.fromisoformat(value.replace('Z', '+00:00'))
    need(result.tzinfo is not None, 'Timezone required')
    return result.astimezone(dt.timezone.utc)


def report(budget_path=None):
    refs = {}

    def read(path):
        refs[path.relative_to(CLOUD).as_posix()] = pin(path)
        return json.loads(path.read_bytes(), parse_float=D)

    def receipt(kind):
        stem = HERE / (kind + '-01')
        r = read(stem.with_suffix('.result.json'))
        need(r['exit_code'] == 0 and r['project'] == PROJECT and r['read_only'] is True
             and stamp(r['started_utc']) <= stamp(r['ended_utc']), 'Unsuccessful SDK capture')
        resource, filter_value = ('operations', 'targetId=' + IDS[kind[2:]]) if kind.startswith('vm') else (kind, 'name~solvers-r1-')
        need(r['argv'][2:] == ['compute', resource, 'list', '--filter=' + filter_value,
             '--project=' + PROJECT, '--limit=100', '--format=json', '--quiet'], 'SDK query scope differs')
        for channel in ('stdout', 'stderr'):
            path = stem.with_suffix('.' + channel + '.log')
            refs[path.relative_to(CLOUD).as_posix()] = pin(path)
            need(r[channel] == pin(path), 'SDK stream pin differs')
        payload = read(stem.with_suffix('.stdout.log'))
        need(isinstance(payload, list) and len(payload) < 100, 'SDK result truncated or not an array')
        return r, payload

    # A caller may replay the exact pre-application snapshot. Its logical input
    # key stays budget.json so the report bytes remain unchanged after applying.
    budget_source = CLOUD / 'budget.json' if budget_path is None else Path(budget_path).resolve()
    budget_pin = pin(budget_source)
    need(budget_pin['sha256'] == EXPECTED_BUDGET, 'Reviewed current ledger/snapshot changed')
    refs['budget.json'] = budget_pin
    budget = json.loads(budget_source.read_bytes(), parse_float=D)
    old = read(CLOUD / 'usage-early-20260927/report.json')
    applied = read(CLOUD / 'usage-early-20260927/applied.json')
    absence = {}
    for kind in ('instances', 'disks', 'addresses'):
        r, payload = receipt(kind)
        need(payload == [], 'Campaign resource remains')
        absence[kind] = {'at_utc': r['ended_utc'], 'receipt': kind + '-01.result.json', 'empty': True}
    rows = []
    for vm, instance_id in IDS.items():
        name = 'solvers-r1-20260925-' + vm
        reservation_id = 'r1-20260925-' + vm
        launch = read(CLOUD / ('launch-' + reservation_id + '.json'))
        created = read(CLOUD / ('create-result-' + reservation_id + '.json'))
        if isinstance(created, list):
            created, = created
        need(created['id'] == instance_id and created['name'] == name, 'Original instance identity differs')
        schedule = created['scheduling']
        need(schedule['provisioningModel'] == 'SPOT' and schedule['instanceTerminationAction'] == 'DELETE'
             and schedule['automaticRestart'] is False, 'Original deletion policy differs')
        disks = created['disks'] if isinstance(created['disks'], list) else [created['disks']]
        disk, = disks
        base = f'https://www.googleapis.com/compute/v1/projects/{PROJECT}/zones/{ZONE}'
        need(disk['source'] == base + '/disks/' + name and disk['autoDelete'] is True
             and int(disk['diskSizeGb']) == 100, 'Original single disk identity/policy differs')
        r, operations = receipt('vm' + vm)
        need(len(operations) == 2 and {o['operationType'] for o in operations} == {'insert', 'compute.instances.preempted'},
             'Retained operations differ from reviewed insert/preemption pair')
        need(all(o['targetId'] == instance_id and o['targetLink'] == base + '/instances/' + name
             and o['status'] == 'DONE' and not o.get('error') for o in operations), 'Operation target/result differs')
        preempted, = [o for o in operations if o['operationType'] == 'compute.instances.preempted']
        start = launch['attempted_at']
        need(stamp(start) < stamp(preempted['endTime']) < stamp(r['started_utc'])
             and all(stamp(preempted['endTime']) < stamp(a['at_utc']) for a in absence.values()), 'Observation ordering differs')
        original, = [row for row in old['rows'] if row['vm'] == vm]
        need(original['instance_id'] == instance_id and original['launch_attempt'] == start, 'Existing usage identity differs')
        costs = original['cost_scenarios_usd']
        need(all(D(costs[k]) == v for k, v in {'undiscounted_compute_hourly': D('.37'),
             'spot_ipv4_hourly': D('.0025'), 'disk_100gib_24hours': D('.3288'),
             'egress_2gib_at_0_30': D('.6'), 'original_other_reserve': D('3.2')}.items()), 'Old model terms differ')
        held, = [entry for entry in budget['reservations'] if entry['id'] == reservation_id]
        old_application, = [entry for entry in applied['changes'] if entry['id'] == reservation_id]
        need(held['reserved_usd'] == old_application['new_reserved_usd'] == D('4.5')
             and held['billed_usd'] is None and held['reservation_released'] is False
             and len(held['events']) == 1 and held['events'][0] == {k: v for k, v in old_application.items() if k != 'id'},
             'Original usage adjustment no longer matches current hold; avoid double return')
        delta = stamp(preempted['endTime']) - stamp(start)
        seconds = D(delta.days * 86400 + delta.seconds) + D(delta.microseconds) / 1000000
        minutes = int(((seconds + 120) / 60).to_integral_value(rounding=ROUND_CEILING))
        disk_model = D(minutes) / 60 * 100 * D('.000137')
        fixed_other_costs = D(costs['whole_lifetime_plus_full_allowances']) - D(costs['disk_100gib_24hours'])
        model = fixed_other_costs + disk_model
        hold = (model / D('.05')).to_integral_value(rounding=ROUND_CEILING) * D('.05')
        rows.append({'vm': vm, 'instance_id': instance_id, 'reservation_id': reservation_id,
                     'basis': 'Preemption DONE under original DELETE policy and single autoDelete disk; not a distinct delete operation.',
                     'launch_attempt': start, 'preemption_done': preempted['endTime'],
                     'old_report_preemption_time_unchanged': original['delete_completed'],
                     'disk_source': disk['source'], 'disk_numeric_id': None,
                     'disk_lifecycle_seconds_for_model': str(seconds), 'disk_slack_seconds': 120,
                     'disk_minutes_after_upper_rounding': minutes,
                     'old_costs_usd_unchanged_except_disk': costs,
                     'old_total_usd': costs['whole_lifetime_plus_full_allowances'],
                     'unchanged_nondisk_cost_usd': str(fixed_other_costs),
                     'old_disk_usd': costs['disk_100gib_24hours'], 'new_disk_usd': str(disk_model),
                     'new_model_usd': str(model), 'current_hold_usd': '4.50', 'proposed_hold_usd': str(hold),
                     'proposed_return_usd': str(D('4.5') - hold), 'margin_above_model_usd': str(hold - model)})
    total_return = sum(D(row['proposed_return_usd']) for row in rows)
    held = sum(D(str(r['reserved_usd'])) for r in budget['reservations'] if not r.get('reservation_released', False))
    need(held == D('39.95') and budget['authorized_limit'] == 40, 'Current total budget differs')
    return {'schema': 'r1.early-delete-disk-only-proposal/v1', 'status': 'proposal_only_no_ledger_change',
            'rows': rows, 'current_absence': absence, 'source': pin(Path(__file__)), 'inputs': refs,
            'summary': {'authorized_usd': '40', 'held_before_usd': str(held), 'return_usd': str(total_return),
                        'proposed_held_usd': str(held - total_return), 'proposed_available_usd': str(D(40) - held + total_return)},
            'rounding': 'Disk duration includes120s margin, ceiling to whole minute; hold ceiling to0.05USD.',
            'official_semantics': 'https://docs.cloud.google.com/compute/docs/instances/spot',
            'limitations': ['No independent disk numeric ID, disk-delete operation, or historical raw disk-absence receipt is retained.',
                           'Preemption DONE plus DELETE/autoDelete configuration and current absence support this model; endTime is not independently measured disk billing end.',
                           'Current absence is a September28 observation and must not be relabeled as a September25 observation.',
                           'Original CPU, IPv4,2GiB network and3.2USD other reserve per VM remain completely unchanged; no Spot discount or credits.',
                           'Old VM05 summary time differs from fresh operation endTime by27.153ms; neither original record is overwritten; minute ceiling is unchanged.',
                           'Actual invoice and complete historical charge timing remain unknown.'],
            'billed_usd': None, 'budget_mutations': False, 'cloud_queries_during_calculation': 0,
            'local_native_or_archive_work': False}


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path)
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--budget', type=Path, help='Exact pinned pre-application budget snapshot for offline replay')
    args = parser.parse_args()
    result = report(args.budget)
    data = (json.dumps(result, indent=2) + '\n').encode()
    if args.check:
        need((HERE / 'proposal01.json').read_bytes() == data, 'Stored proposal differs')
    elif args.out:
        with args.out.open('xb') as stream:
            stream.write(data)
    print(json.dumps({'status': result['status'], 'summary': result['summary']}))
