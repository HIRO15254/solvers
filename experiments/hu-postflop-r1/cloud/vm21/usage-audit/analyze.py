"""Replay VM21 compact evidence; only disk allowance can shrink. No ledger/API.

Reuse VM20's pinned strict JSON and DELTA metric validators, rebinding only their
collector identity/timestamp helpers. Do not call VM20's cost/report routines.
"""
import argparse
from decimal import Decimal as D, ROUND_CEILING
import hashlib
import importlib.util
import json
from pathlib import Path
import re

HERE = Path(__file__).resolve().parent
PRIOR = HERE.parent.parent / 'usage-audit-vm20'
ACQUISITION_SHA = 'dba8951a42eb7e748c278667d0bdd3316feb2501a459742e6274e22752047918'
COLLECTOR_SHA = 'e5d143a12feaf57daaefb122cb15c74251dd65564438caea2e9a8a5196780bae'
METRIC_HELPER_SHA = 'cfff88d4810ff2732c4112307e5bc2729eb00f4b577350c5aabf3f1a8c708f5e'
OLD_COLLECTOR_SHA = 'e014d02efaa40af98a2e1e65de501536ad87b07ed720d449bf4281572b4400ba'


def frozen(path, sha):
    data = path.read_bytes()
    if len(data) > 2 * 1024**2 or hashlib.sha256(data).hexdigest() != sha:
        raise ValueError('Frozen helper/evidence differs: ' + str(path))


def module(path, sha, name):
    frozen(path, sha)
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


frozen(PRIOR / 'collect.py', OLD_COLLECTOR_SHA)
c = module(HERE / 'collect.py', COLLECTOR_SHA, 'vm21_usage_collector')
h = module(PRIOR / 'analyze.py', METRIC_HELPER_SHA, 'vm20_metric_validation')
h.c = c
need, read, pin, seconds = c.need, h.read, h.pin, h.seconds


def calculate(reservation, disk_seconds, sent_bytes, charged_payload):
    need(0 < disk_seconds < 24 * 3600, 'Disk lifetime does not justify smaller allowance')
    need(0 <= sent_bytes <= 320 * 1024**2, 'Observed traffic exceeds original envelope')
    need(0 <= charged_payload <= 256 * 1024**2, 'Started transfer charge exceeds payload envelope')
    disk_minutes = int(((disk_seconds + 120) / 60).to_integral_value(rounding=ROUND_CEILING))
    need(disk_minutes < 24 * 60, 'Rounded disk lifetime does not shrink original allowance')
    # Retain original full CPU and IPv4 bounds, rates, transfer and uncertainty.
    components = {
        'full_original_small_cpu_47_minutes': D(47) / 60 * reservation['small_compute_usd_hour'],
        'overlapping_full_original_large_cpu_10_minutes': D(10) / 60 * reservation['large_compute_usd_hour'],
        'full_original_ipv4_47_minutes': D(47) / 60 * reservation['ipv4_usd_hour'],
        'same_disk_creation_to_absence_plus_slack': D(disk_minutes) / 60 * 20 * reservation['disk_usd_gib_hour'],
        'original_egress_320_mib': reservation['maximum_download_gib'] * reservation['reserved_egress_usd_gib'],
        'original_uncertainty': D(reservation['tax_price_delay_and_other_reserve_usd']),
    }
    model = sum(components.values())
    hold = (model / D('.05')).to_integral_value(rounding=ROUND_CEILING) * D('.05')
    return {'disk_seconds': str(disk_seconds), 'disk_minutes_after_120s_slack_and_ceiling': disk_minutes,
            'components_usd': {key: str(value) for key, value in components.items()},
            'model_usd': str(model), 'previous_hold_usd': '1.35', 'proposed_hold_usd': str(hold),
            'proposed_restore_usd': str(D('1.35') - hold), 'rounding_margin_usd': str(hold - model),
            'rounding': 'Ceil to next 0.05 USD; unchanged VM19/20 upper-rounding rule.',
            'only_reduced_component': '20 GiB disk: original24h replaced by creation through confirmed absence plus120s, rounded upward to minute.',
            'billed_usd': None}


def transfer_summary(refs, raw, path):
    names = sorted((n for n in refs if n.startswith('vm21/transfer-') and n.endswith('.intent.json')),
                   key=lambda n: c.stamp(raw(n)['at_utc']))
    need(names, 'No retained transfer intents')
    charged, results, files, prior = 0, [], [], []
    for name in names:
        intent = raw(name)
        need(intent['schema'] == 'r1.vm21-transfer-intent/v1' and intent['resource'] == c.INSTANCE_NAME,
             'Transfer identity differs')
        need(len(intent['files']) == 1 and intent['prior_payload_bytes'] == charged,
             'Transfer ordering/count differs')
        need(intent['prior_intents'] == prior, 'Prior intent chain differs')
        file, = intent['files']
        size = file['bytes']
        need(type(size) is int and size >= 0 and intent['charged_payload_bytes'] == size,
             'Invalid transfer size')
        need(file['path'].startswith('/opt/r1/sparse-rank-') and re.fullmatch('[a-f0-9]{64}', file['sha256']),
             'Invalid transfer object')
        need(c.stamp(c.LAUNCH) < c.stamp(intent['at_utc']) < c.stamp(c.ORIGINAL_STOP),
             'Transfer outside original deadline')
        charged += size
        need(intent['cumulative_payload_bytes'] == charged
             and intent['total_reserved_outbound_bytes'] == charged + 64 * 1024**2
             and charged + 64 * 1024**2 <= 320 * 1024**2, 'Cumulative transfer envelope differs')
        control = intent['control_evidence']
        need(control['reserved_control_protocol_bytes'] == 64 * 1024**2
             and sum(v['bytes'] for v in control['files']) == control['observed_output_bytes']
             and control['observed_output_bytes'] <= 8 * 1024**2, 'Control output bound differs')
        need(intent['transport'] == pin(path(refs['vm21/split01.stdout.log']))
             and intent['split_receipt'] == pin(path(refs['vm21/split01.result.json'])), 'Transfer manifest binding differs')
        result_name = name.removesuffix('.intent.json') + '.result.json'
        status = 'missing_result_still_fully_charged'
        if result_name in refs:
            result = raw(result_name)
            need(result['intent'] == pin(path(refs[name])) and result['charged_payload_bytes'] == size
                 and result['charged_payload_refund_bytes'] == 0, 'Transfer result changed/refunded')
            status = result['status']
            if status == 'intended_file_verified':
                verified, = result['verified_files']
                need(result['exit_code'] == 0 and verified['bytes'] == size
                     and verified['sha256'] == file['sha256']
                     and Path(verified['path']).name == Path(file['path']).name, 'Verified transfer identity differs')
        results.append({'intent': name, 'payload_bytes': size, 'status': status})
        files.append(file)
        prior.append({'path': Path(name).name, 'pin': pin(path(refs[name])), 'charged_payload_bytes': size})
    complete = raw('vm21/transfer-proof01.completed.json')
    need(complete['status'] == 'all_intended_files_verified'
         and complete['cumulative_payload_bytes'] == charged
         and complete['reserved_control_protocol_bytes'] == 64 * 1024**2
         and all(r['status'] == 'intended_file_verified' for r in results), 'Completion not verified')
    download = raw('vm21/download-check.json')
    need(download['status'] == 'downloaded_archive_stream_hash_verified'
         and download['local_archive_decompression'] is False
         and sum(p['bytes'] for p in download['parts']) == download['bytes'], 'Archive transfer check invalid')
    for part in download['parts']:
        matches = [f for f in files if Path(f['path']).name == part['path']]
        need(len(matches) == 1 and all(matches[0][k] == part[k] for k in ('bytes', 'sha256')), 'Archive part binding differs')
    return {'intents': results, 'started_payload_bytes_including_uncertain': charged,
            'uncertain_or_failed_intents': sum(r['status'] != 'intended_file_verified' for r in results),
            'control_protocol_reserved_bytes': 64 * 1024**2,
            'total_started_payload_plus_control_reserve': charged + 64 * 1024**2,
            'verified_archive_bytes': download['bytes'], 'verified_archive_sha256': download['sha256'],
            'observed_control_output_bytes_after_transfer': complete['control_evidence_after']['observed_output_bytes'],
            'local_archive_read_or_decompression_by_this_validator': False,
            'wire_or_billable_bytes': None}


def build_report():
    frozen(HERE / 'acquisition.json', ACQUISITION_SHA)
    acquisition = read(HERE / 'acquisition.json')
    need(acquisition['status'] == 'completed' and acquisition['source'] == pin(HERE / 'collect.py'),
         'Acquisition/source changed')
    need((acquisition['project'], acquisition['instance_id'], acquisition['instance_name'])
         == (c.PROJECT, c.INSTANCE_ID, c.INSTANCE_NAME), 'Acquisition identity changed')
    need(acquisition['maximum_monitoring_gets'] == 2 and acquisition['credential_material_retained'] is False
         and acquisition['cloud_mutations'] is False and acquisition['budget_mutations'] is False,
         'Acquisition scope differs')
    refs = acquisition['inputs']

    def path(ref):
        value = (HERE / ref['path']).resolve()
        need(value.is_relative_to(HERE.resolve()) and not value.is_symlink()
             and pin(value) == {k: ref[k] for k in ('bytes', 'sha256')}, 'Retained input changed/escaped')
        return value

    for ref in [*refs.values(), *(q['response'] for q in acquisition['requests'])]:
        path(ref)
    raw = lambda name: read(path(refs[name]))
    lifecycle = c.verify_lifecycle(lambda name: json.loads(path(refs[name]).read_bytes()),
                                   acquisition['captured_sdk_commands'])
    need(acquisition['lifecycle'] == json.loads(json.dumps(lifecycle)), 'Lifecycle replay differs')
    need(acquisition['interval_start'] == c.LAUNCH and acquisition['interval_end'] == lifecycle['absence'],
         'Monitoring lifecycle differs')
    for name in acquisition['captured_sdk_commands']:
        receipt = raw(name)
        need(seconds(receipt['started_utc'], receipt['ended_utc']) >= 0, 'Receipt time reversed')
        for stream in ('stdout', 'stderr'):
            need(receipt[stream] == pin(path(refs[name.removesuffix('.result.json') + '.' + stream + '.log'])),
                 'SDK output pin differs')
    created = raw('vm21/create.receipt.json')
    need(created['exit_code'] == 0 and created['stdout'] == pin(path(refs['create-result-r1-20260928-21.json']))
         and created['stderr'] == pin(path(refs['vm21/create.stderr.log'])), 'Create output pins differ')
    ops = raw(lifecycle['operations_receipt'].removesuffix('.result.json') + '.stdout.log')
    ops = sorted(ops, key=lambda op: c.stamp(op['startTime']))
    need([o['operationType'] for o in ops] == ['insert', 'stop', 'setMachineType', 'setScheduling', 'start',
         'stop', 'setMachineType', 'setScheduling', 'start', 'delete'], 'Unexpected operation sequence')
    need(all(c.stamp(a['endTime']) < c.stamp(b['startTime']) for a, b in zip(ops, ops[1:])),
         'Overlapping lifecycle operations')
    phase = lifecycle['phase']
    need(c.stamp(phase['armed_at_utc']) < c.stamp(ops[4]['startTime'])
         < c.stamp(ops[5]['endTime']) < c.stamp(phase['phase_stop_utc']), 'Actual highCPU operations outside fixed window')
    need(c.stamp(lifecycle['absence']) < c.stamp(c.ORIGINAL_STOP), 'Cleanup exceeds original runtime bound')
    reservation = raw('vm21/reservation.json')
    original = raw('vm21/budget-proposal.json')
    need(all(reservation[k] == v for k, v in original['reservation'].items()), 'Original reservation terms changed')
    for key, expected in {'small_compute_usd_hour': D('.06701142'), 'large_compute_usd_hour': D('.80'),
                          'disk_usd_gib_hour': D('.000137'), 'ipv4_usd_hour': D('.0025'),
                          'reserved_egress_usd_gib': D('.30'), 'maximum_download_gib': D('.3125'),
                          'tax_price_delay_and_other_reserve_usd': 1}.items():
        need(reservation[key] == expected, 'Original rate/allowance changed')
    original_terms = original['arithmetic']['terms_usd']
    need(sum(D(v) for v in original_terms.values()) == D(original['arithmetic']['total_usd'])
         == D('1.347293945666666666666666667'), 'Original arithmetic changed')
    price = raw('preflight-vm20/pricing-sources.json')
    for attempt in price['attempts']:
        need(attempt['status'] == 'acquired' and attempt['source'].startswith('https://cloud.google.com/'),
             'Price source is not acquired official evidence')
        for ref in attempt['retained']:
            need(pin(path(refs['preflight-vm20/' + ref['path']])) == {k: ref[k] for k in ('bytes', 'sha256')},
                 'Price fragment differs')
    for machine, expected in [('e2-standard-2', '0.06701142'), ('e2-highcpu-32', '0.79152384')]:
        row = path(refs[f'preflight-vm20/pricing-compute-{machine}-row.html']).read_text()
        need(machine in row and re.findall(r'\$([0-9.]+) / 1 hour', row)[0] == expected, 'Official regular price differs')
    metrics = h.verify_metrics(acquisition, lambda ref: read(path(ref)))
    need(all(m['available'] for m in metrics.values()), 'No acquired usage: no return proposal')
    transfer = transfer_summary(refs, raw, path)
    disk_seconds = seconds(lifecycle['disk']['creationTimestamp'], lifecycle['disk_absence'])
    cost = calculate(reservation, disk_seconds, D(metrics['sent']['observed_sum']),
                     transfer['started_payload_bytes_including_uncertain'])
    expected_terms = {'full_original_small_cpu_47_minutes': 'small_compute',
                      'overlapping_full_original_large_cpu_10_minutes': 'large_compute',
                      'full_original_ipv4_47_minutes': 'ipv4', 'original_egress_320_mib': 'egress_320_mib',
                      'original_uncertainty': 'uncertainty'}
    need(all(D(cost['components_usd'][k]) == D(original_terms[v]) for k, v in expected_terms.items()),
         'Non-disk cost component changed')
    budget = raw('budget.json')
    held, = [r for r in budget['reservations'] if r['id'] == reservation['id']]
    need(held['reserved_usd'] == D('1.35') and held['billed_usd'] is None
         and held['reservation_released'] is False and budget['authorized_limit'] == 40, 'Budget snapshot differs')
    need(pin(path(refs['budget.json'])) == raw('vm21/reservation-check.json')['budget_after'], 'Budget changed since reservation')
    held_total = sum(D(str(r['reserved_usd'])) for r in budget['reservations'] if not r.get('reservation_released', False))
    need(held_total == 40 and D(cost['proposed_restore_usd']) >= 0, 'Unexpected current hold or increased cost')
    return {'schema': 'r1.vm21-usage-proposal/v1', 'status': 'proposal_only_no_ledger_change',
            'instance_id': c.INSTANCE_ID, 'instance_name': c.INSTANCE_NAME, 'project': c.PROJECT,
            'source': pin(HERE / 'analyze.py'), 'collector': pin(HERE / 'collect.py'),
            'acquisition': pin(HERE / 'acquisition.json'),
            'helper_reuse': {'path': str(PRIOR / 'analyze.py'), 'sha256': METRIC_HELPER_SHA,
                            'prior_collector_sha256': OLD_COLLECTOR_SHA,
                            'scope': 'Strict JSON/pin/time helpers and verify_metrics only; VM21 identity binding replaces VM20 identity; no prior cost/report routine called.'},
            'input_count': len(refs), 'input_bytes': sum(r['bytes'] for r in refs.values()),
            'lifecycle': {'launch': c.LAUNCH, 'original_stop': c.ORIGINAL_STOP,
                          'phase32': phase, 'operation_types': [o['operationType'] for o in ops],
                          'high_start_operation_start': ops[4]['startTime'], 'high_stop_operation_done': ops[5]['endTime'],
                          'observed_high_operation_window_seconds': str(seconds(ops[4]['startTime'], ops[5]['endTime'])),
                          'delete_done': lifecycle['deletion']['endTime'], 'latest_absence': lifecycle['absence'],
                          'disk_id': lifecycle['disk']['id'], 'disk_created': lifecycle['disk']['creationTimestamp'],
                          'disk_absence': lifecycle['disk_absence'], 'absence_receipts': lifecycle['absence_receipts'],
                          'large_running_evidence': [s['receipt'] for s in lifecycle['observed_states']
                              if s['state']['status'] == 'RUNNING' and s['state']['machineType'].endswith('/e2-highcpu-32')]},
            'monitoring': metrics, 'transfer': transfer, 'cost': cost,
            'budget_snapshot': {'ledger': refs['budget.json'], 'authorized_usd': '40', 'held_usd': str(held_total),
                                'available_usd': '0', 'proposed_new_held_usd': str(held_total - D(cost['proposed_restore_usd'])),
                                'proposed_new_available_usd': cost['proposed_restore_usd'], 'billed_usd': None},
            'limitations': ['Monitoring DELTA points are acquired usage observations, not invoice or complete billing export; actual billed amount remains unknown.',
                            'Gaps and query-edge usage remain unknown, not zero; preserved CPU/network/uncertainty bounds do not assume complete metrics.',
                            'Only confirmed disk lifecycle replaces its original24h reserve. Full47min smallCPU, overlapping10min highCPU at .80/h and IPv4 remain.',
                            'Original320MiB outbound allowance, .30 USD/GiB rate and1 USD uncertainty remain; transfer receipts are payload, not wire metering.',
                            'Usage accounting implies no performance acceptance or candidate adoption.'],
            'cloud_mutations': False, 'budget_mutations': False, 'local_native_or_archive_work': False}


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--report', type=Path)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    report = build_report()
    data = (json.dumps(report, indent=2) + '\n').encode()
    if args.check:
        need((HERE / 'report.json').read_bytes() == data, 'Report replay differs')
    elif args.report:
        with args.report.open('xb') as stream:
            stream.write(data)
    print(json.dumps({'status': report['status'], 'cost': report['cost'],
                      'monitoring': {k: {'points': v['points'], 'observed_sum': v['observed_sum']}
                                     for k, v in report['monitoring'].items()}}))
