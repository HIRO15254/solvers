"""Offline VM20 usage/lifecycle proposal. Never reads archives or changes budget."""
import argparse
from decimal import Decimal as D, ROUND_CEILING
import importlib.util
import json
from pathlib import Path
import re
import urllib.parse

HERE = Path(__file__).resolve().parent
ACQUISITION_SHA = '2a03d7be28d9c68990c3e329f29f7c729e06d99c333809b8690c5bb0aad3f9ab'
SPEC = importlib.util.spec_from_file_location("vm20_collector", HERE / "collect.py")
c = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(c)
require = c.require


def read(path):
    require(path.stat().st_size <= 2 * 1024**2, "Compact evidence size exceeded")
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "Duplicate JSON key")
            result[key] = value
        return result
    def invalid(_):
        raise ValueError("Nonfinite JSON token")
    return json.loads(path.read_bytes(), parse_float=D, parse_constant=invalid, object_pairs_hook=unique)


def pin(path):
    require(path.stat().st_size <= 2 * 1024**2, "Compact evidence size exceeded")
    return c.pin(path.read_bytes())


def seconds(start, end):
    delta = c.stamp(end) - c.stamp(start)
    return D(delta.days * 86400 + delta.seconds) + D(delta.microseconds) / 1000000


def ceil_minutes(duration):
    require(duration > 0, "Nonpositive lifecycle duration")
    return int(((duration + 120) / 60).to_integral_value(rounding=ROUND_CEILING))


def calculate(whole, high, disk, observed_sent, known_transfer):
    require(0 < high <= whole and 0 < disk <= whole, "Invalid whole/high/disk lifecycle windows")
    require(known_transfer >= 0 and (observed_sent is None or observed_sent >= 0), "Invalid transfer")
    whole_minute, high_minute, disk_minute = map(ceil_minutes, (whole, high, disk))
    seen = max(D(known_transfer), observed_sent if observed_sent is not None else D(0))
    network = max(D('.5'), (seen / (1024**3) * 2).to_integral_value(rounding=ROUND_CEILING) / 2)
    components = {"e2_standard_2_whole_lifecycle_regular_price": D(whole_minute) / 60 * D('.06701142'),
                  "overlapping_e2_highcpu32_regular_price": D(high_minute) / 60 * D('.79152384'),
                  "ipv4_entire_rounded_lifecycle": D(whole_minute) / 60 * D('.0025'),
                  "same_20gib_disk_entire_lifecycle": D(disk_minute) / 60 * 20 * D('.000137'),
                  "total_egress_allowance": network * D('.30'), "original_uncertainty_reserve": D(1)}
    total = sum(components.values())
    hold = (total * 10).to_integral_value(rounding=ROUND_CEILING) / 10
    return {"whole_lifecycle_minutes": whole_minute, "overlapping_highcpu_minutes": high_minute,
            "disk_minutes": disk_minute,
            "rounding": "Add120 seconds to whole lifecycle, overlapping highCPU window, and full disk window; ceil each to minute. No stopped-time subtraction.",
            "network_allowance_gib": str(network), "components_usd": {k: str(v) for k, v in components.items()},
            "model_usd": str(total), "proposed_hold_usd": str(hold), "previous_hold_usd": "1.85",
            "proposed_restore_usd": str(D('1.85') - hold) if hold < D('1.85') else None,
            "rounding_margin_usd": str(hold - total)}


def summarize(points, key, interval_start, interval_end):
    if not points:
        return {"available": False, "points": 0, "observed_sum": None, "unobserved_usage": None}
    points = sorted(points, key=lambda p: c.stamp(p['interval']['startTime']))
    values, gaps, seen, previous = [], [], set(), None
    for point in points:
        start, end = (point['interval'][k] for k in ('startTime', 'endTime'))
        require(0 < seconds(start, end) <= 61 and (start, end) not in seen, "Invalid/repeated point interval")
        require(seconds(interval_start, start) >= -61 and seconds(end, interval_end) >= -61, "Point outside requested lifecycle")
        if previous is not None:
            gap = seconds(previous, start)
            require(gap >= 0, "Overlapping intervals")
            if gap:
                gaps.append({"start": previous, "end": start, "seconds": str(gap), "unobserved_usage": None})
        require(set(point['value']) == {key}, "Metric value type changed")
        value = D(str(point['value'][key]))
        require(value.is_finite() and value >= 0, "Invalid metric value")
        if key == 'int64Value':
            require(value == value.to_integral_value(), "Noninteger sent bytes")
        values.append(value)
        previous = end
        seen.add((start, end))
    return {"available": True, "points": len(points), "observed_sum": str(sum(values)),
            "first_start": points[0]['interval']['startTime'], "last_end": previous,
            "internal_gaps": gaps, "unobserved_usage": None,
            "interval_semantics": "Full raw DELTA points; one edge minute may overlap outside query; missing usage remains unknown"}


def verify_metrics(acquisition, load_ref):
    require(len(acquisition['requests']) == 2, "Request count exceeds bound")
    result = {}
    for label, metric in c.METRICS.items():
        requests = [q for q in acquisition['requests'] if q['label'] == label]
        require(len(requests) == 1 and [q['page'] for q in requests] == list(range(len(requests))), "Metric pages not contiguous/bounded")
        points, next_page = [], None
        for query in requests:
            require(query['http_status'] == 200 and query['method'] == 'GET' and query['metric'] == metric, "Unsuccessful/different metric query")
            uri = urllib.parse.urlparse(query['url'])
            require(uri.scheme == 'https' and uri.netloc == 'monitoring.googleapis.com' and uri.path == f'/v3/projects/{c.PROJECT}/timeSeries', "Monitoring endpoint differs")
            expected = {'filter': [f'metric.type = "{metric}" AND resource.type = "gce_instance" AND resource.labels.project_id = "{c.PROJECT}" AND resource.labels.instance_id = "{c.INSTANCE_ID}"'],
                        'interval.startTime': [acquisition['interval_start']], 'interval.endTime': [acquisition['interval_end']], 'view': ['FULL'], 'pageSize': ['1000']}
            if next_page:
                expected['pageToken'] = [next_page]
            require(urllib.parse.parse_qs(uri.query) == expected, "Metric query scope/paging changed")
            data = load_ref(query['response'])
            require(set(data) <= {'timeSeries', 'nextPageToken', 'unit'} and data.get('unit') == ('By' if label == 'sent' else 's{uptime}'), "Partial metric error, unit mismatch or unknown response fields")
            require(sum(len(s.get('points', [])) for s in data.get('timeSeries', [])) == query['point_count'], "Captured point count changed")
            for series in data.get('timeSeries', []):
                require(series['resource'] == {'type': 'gce_instance', 'labels': {'instance_id': c.INSTANCE_ID, 'project_id': c.PROJECT, 'zone': c.ZONE}}, "Metric resource identity changed")
                require(series['metric']['type'] == metric and series['metric']['labels'].get('instance_name') == c.INSTANCE_NAME, "Metric identity changed")
                require(series['metricKind'] == 'DELTA' and series['valueType'] == ('INT64' if label == 'sent' else 'DOUBLE'), "Metric semantics changed")
                if label == 'sent':
                    require(series['metric']['labels'] == {'instance_name': c.INSTANCE_NAME, 'loadbalanced': 'false'}, "Unexpected traffic series partition")
                else:
                    require(series['metric']['labels'] == {'instance_name': c.INSTANCE_NAME}, "Unexpected uptime partition")
                points.extend(series['points'])
            next_page = data.get('nextPageToken')
        require(not next_page, "Missing final metric page")
        result[label] = summarize(points, 'int64Value' if label == 'sent' else 'doubleValue', acquisition['interval_start'], acquisition['interval_end'])
    require(set(q['label'] for q in acquisition['requests']) == set(c.METRICS), "Unrecognized metric request")
    return result


def build_report():
    acquisition = read(HERE / 'acquisition.json')
    require(pin(HERE / 'acquisition.json')['sha256'] == ACQUISITION_SHA, 'Frozen acquisition changed')
    require(acquisition['status'] == 'completed' and acquisition['source'] == pin(HERE / 'collect.py'), 'Incomplete acquisition/source drift')
    require((acquisition['project'], acquisition['instance_id'], acquisition['instance_name']) == (c.PROJECT, c.INSTANCE_ID, c.INSTANCE_NAME), 'Acquisition identity changed')
    require(acquisition['maximum_monitoring_gets'] == 2 and acquisition['credential_material_retained'] is False, 'Acquisition scope changed')
    refs = acquisition['inputs']

    def path(ref):
        result = (HERE / ref['path']).resolve()
        require(result.is_relative_to(HERE.resolve()) and pin(result) == {k: ref[k] for k in ('bytes', 'sha256')}, 'Retained evidence changed/escaped')
        return result

    for ref in [*refs.values(), *(q['response'] for q in acquisition['requests'])]:
        path(ref)
    raw = lambda name: read(path(refs[name]))
    lifecycle = c.verify_lifecycle(lambda name: json.loads(path(refs[name]).read_bytes()))
    require(acquisition['interval_start'] == c.LAUNCH and acquisition['interval_end'] == lifecycle['absence'], 'Monitoring lifecycle differs')
    sdk_scp = []
    for name in acquisition['captured_sdk_commands']:
        receipt = raw(name)
        require(seconds(receipt['started_utc'], receipt['ended_utc']) >= 0, 'SDK receipt time reversed')
        for stream in ('stdout', 'stderr'):
            require(receipt[stream] == pin(path(refs[name.removesuffix('.result.json') + '.' + stream + '.log'])), 'SDK stream pin changed')
        if 'scp' in receipt['argv']:
            sdk_scp.append({'path': name, 'exit_code': receipt['exit_code'], 'argv': receipt['argv']})
    pricing = raw('preflight-vm20/pricing-sources.json')
    for attempt in pricing['attempts']:
        require(attempt['status'] == 'acquired' and attempt['source'].startswith('https://cloud.google.com/'), 'Nonofficial/unavailable price')
        for reference in attempt['retained']:
            require(pin(path(refs['preflight-vm20/' + reference['path']])) == {k: reference[k] for k in ('bytes', 'sha256')}, 'Price excerpt changed')
    for machine, expected in (('e2-standard-2', '0.06701142'), ('e2-highcpu-32', '0.79152384')):
        row = path(refs[f'preflight-vm20/pricing-compute-{machine}-row.html']).read_text()
        header = path(refs[f'preflight-vm20/pricing-compute-{machine}-header.html']).read_text()
        region = path(refs[f'preflight-vm20/pricing-compute-{machine}-region.html']).read_text()
        require(machine in row and re.findall(r'\$([0-9.]+) / 1 hour', row)[0] == expected, 'Regular price changed')
        require('Default' in header and 'Iowa' in region, 'Regular price header/region changed')
    reservation = raw('vm20/reservation.json')
    budget = raw('budget.json')
    held, = [r for r in budget['reservations'] if r['id'] == reservation['id']]
    require(held['reserved_usd'] == D('1.85') and held['billed_usd'] is None and held['reservation_released'] is False, 'Budget hold changed')
    require(budget['authorized_limit'] == 40, 'Authorized ceiling changed')
    metrics = verify_metrics(acquisition, lambda ref: read(path(ref)))
    transfer = raw('vm20/download-check.json')
    require(transfer['status'] == 'downloaded_archive_stream_hash_verified' and transfer['local_archive_decompression'] is False, 'Transport evidence missing')
    require(sum(part['bytes'] for part in transfer['parts']) == transfer['bytes'], 'Transport part sizes differ')
    known_transfer = transfer['bytes']
    whole = seconds(c.LAUNCH, lifecycle['absence'])
    high = seconds(lifecycle['high_start'], lifecycle['high_end'])
    disk = seconds(lifecycle['disk']['creationTimestamp'], lifecycle['disk_absence'])
    require(whole < 2700 and disk < 24 * 3600, 'Original runtime/disk allowance exceeded')
    cost = calculate(whole, high, disk, D(metrics['sent']['observed_sum']) if metrics['sent']['observed_sum'] is not None else None, known_transfer)
    require(metrics['sent']['available'] and metrics['uptime']['available'], 'No observed usage: do not propose return')
    held_total = sum(D(str(r['reserved_usd'])) for r in budget['reservations'] if not r.get('reservation_released', False))
    return {'schema': 'r1.vm20-usage-proposal/v1', 'status': 'proposal_only_no_ledger_change',
            'instance_id': c.INSTANCE_ID, 'instance_name': c.INSTANCE_NAME, 'project': c.PROJECT,
            'source': pin(HERE / 'analyze.py'), 'acquisition': pin(HERE / 'acquisition.json'),
            'inputs': {'count': len(refs), 'bytes': sum(r['bytes'] for r in refs.values()), 'sdk_receipts': len(acquisition['captured_sdk_commands'])},
            'lifecycle': {'launch': c.LAUNCH, 'original_stop_deadline': c.ORIGINAL_STOP,
                          'high_cpu_start_before_stop': lifecycle['high_start'], 'high_cpu_end_after_resize_down': lifecycle['high_end'],
                          'delete_operation_done': lifecycle['deletion']['endTime'], 'absence_observed': lifecycle['absence'],
                          'disk_created': lifecycle['disk']['creationTimestamp'], 'disk_absence_observed': lifecycle['disk_absence'],
                          'whole_lifecycle_seconds': str(whole), 'overlapping_highcpu_seconds': str(high),
                          'disk_lifecycle_seconds': str(disk), 'actual_machine_sequence': ['e2-standard-2', 'e2-highcpu-32', 'e2-standard-2'],
                          'instance_id_unchanged': True, 'disk_id': lifecycle['disk']['id'], 'three_inventory_absence_confirmed': True,
                          'stop_deadline_extended': False, 'preemption_or_overnight_disk_gap': False},
            'monitoring': metrics, 'known_verified_archive_transfer_bytes': known_transfer, 'captured_scp_attempts': sdk_scp,
            'price_basis': {'regular_e2_standard_2_usd_hour': '0.06701142', 'regular_e2_highcpu_32_usd_hour': '0.79152384',
                            'capture_date_utc': '2026-09-28', 'disk_ceiling_usd_gib_hour': '.000137', 'spot_ipv4_usd_hour': '.0025',
                            'egress_allowance_usd_gib': '.30', 'discounts_or_credits_assumed': False},
            'cost': cost, 'budget_snapshot': {'authorized_usd': '40', 'held_usd': str(held_total),
                                            'available_usd': str(D(40) - held_total), 'billed_usd': None,
                                            'ledger_pin': refs['budget.json'],
                                            'proposed_new_available_usd': str(D(40) - held_total + D(cost['proposed_restore_usd'])) if cost['proposed_restore_usd'] else None},
            'limitations': ['Observed Monitoring DELTA is not invoice or complete billing export; actual billed amount unknown.',
                            'Monitoring gaps and edges remain unknown, not zero usage; acquisition immediately after cleanup may have reporting delay.',
                            'All observed sent bytes count against egress allowance; archive is known payload, not total connection/sidecar traffic.',
                            'Entire lifecycle is charged at ordinary2CPU price including stopped intervals, plus overlapping32CPU regular-price window.',
                            'Original1USD uncertainty and at least512MiB total outbound allowance retained.',
                            'No performance/profile completeness, candidate adoption or R1 acceptance follows from this usage audit.'],
            'cloud_mutations': False, 'budget_mutations': False, 'local_archive_decompression': False}


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--report', type=Path)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    report = build_report()
    data = (json.dumps(report, indent=2) + '\n').encode()
    if args.check:
        require((HERE / 'report.json').read_bytes() == data, 'Stored report differs')
    elif args.report:
        with args.report.open('xb') as stream:
            stream.write(data)
    print(json.dumps({'status': report['status'], 'cost': report['cost'], 'usage': {k: {'points': v['points'], 'observed_sum': v['observed_sum']} for k, v in report['monitoring'].items()}}))

