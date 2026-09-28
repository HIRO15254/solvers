"""One explicit phase transition; cloud STOP bounds the 32-CPU boot even if the controller dies."""
import argparse
import datetime as dt
import subprocess
import sys
import reserve as r


def command(label, args, timeout=60):
    done = subprocess.run([sys.executable, '-B', str(r.HERE / 'capture-command.py'),
                           '--label', label, '--timeout', str(timeout), '--', *args], check=False)
    r.need(done.returncode == 0, 'SDK command failed/timed out; inspect its receipt, do not retry')
    receipt = r.read(r.HERE / (label + '.result.json'))
    r.need(receipt['exit_code'] == 0, 'SDK operation did not complete')
    for channel in ['stdout', 'stderr']:
        r.need(receipt[channel] == r.pin(r.HERE / (label + '.' + channel + '.log')), 'SDK receipt bytes differ')
    return r.read(r.HERE / (label + '.stdout.log'))


def scope(verb):
    return ['compute', 'instances', verb, r.NAME, '--project=' + r.PROJECT,
            '--zone=' + r.ZONE, '--quiet', '--format=json']


def observed(step, expected_machine, expected_stop, instance_id):
    state = command(step + '-state01', scope('describe'))
    base = 'https://www.googleapis.com/compute/v1/projects/' + r.PROJECT + '/zones/' + r.ZONE
    r.need(state['id'] == instance_id and state['name'] == r.NAME
           and state['selfLink'] == base + '/instances/' + r.NAME
           and state['status'] == 'TERMINATED'
           and state['machineType'] == base + '/machineTypes/' + expected_machine,
           'Phase requires the same stopped instance at the exact machine type')
    scheduling = state['scheduling']
    r.need(scheduling['provisioningModel'] == 'SPOT' and scheduling['instanceTerminationAction'] == 'STOP'
           and scheduling['automaticRestart'] is False and r.utc(scheduling['terminationTime']) == expected_stop,
           'Phase STOP/provisioning differs')
    return state


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('step', choices=['arm32', 'start32', 'restore2', 'start2'])
    args = parser.parse_args()
    r.authorized()
    created = r.read(r.HERE / 'create.receipt.json')
    r.need(created['exit_code'] == 0 and created['reservation_id'] == r.ID, 'Successful creation required')
    raw_path = r.CLOUD / ('create-result-' + r.ID + '.json')
    r.need(created['stdout'] == r.pin(raw_path), 'Creation response differs')
    result = r.read(raw_path)
    r.need(len(result) == 1 and result[0]['name'] == r.NAME, 'Creation identity differs')
    instance_id = result[0]['id']
    stop = r.utc(created['termination_time'])
    r.need(r.utc(result[0]['scheduling']['terminationTime']) == stop, 'Original STOP differs')
    r.need(dt.datetime.now(dt.timezone.utc) < stop, 'Original STOP expired; no recovery extension allowed')
    if args.step == 'arm32':
        observed(args.step, 'e2-highcpu-32', stop, instance_id)
        now = dt.datetime.now(dt.timezone.utc)
        phase_stop = (now + dt.timedelta(seconds=480)).replace(microsecond=0)
        r.need((stop - phase_stop).total_seconds() >= 720, 'Need at least 12 minutes after the high-CPU phase')
        plan = {'schema': 'r1.vm21-large-phase/v1', 'instance_id': instance_id,
                'armed_at_utc': now.isoformat(), 'phase_stop_utc': phase_stop.isoformat(),
                'original_stop_utc': stop.isoformat(), 'maximum_large_phase_seconds': 480,
                'max_measurement_window_seconds': 360,
                'note': 'STOP is fixed before start request; setup latency consumes this window.'}
        r.fresh_json(r.HERE / 'phase32-plan.json', plan)
        command('arm32-schedule01', scope('set-scheduling') + ['--termination-time=' + phase_stop.strftime('%Y-%m-%dT%H:%M:%SZ'),
                '--instance-termination-action=STOP', '--no-restart-on-failure'])
    else:
        plan = r.read(r.HERE / 'phase32-plan.json')
        r.need(plan['instance_id'] == instance_id and r.utc(plan['original_stop_utc']) == stop,
               'Large-phase identity differs')
        phase_stop = r.utc(plan['phase_stop_utc'])
        r.need(0 < (phase_stop - r.utc(plan['armed_at_utc'])).total_seconds() <= 480,
               'Large-phase deadline was extended')
        if args.step == 'start32':
            observed(args.step, 'e2-highcpu-32', phase_stop, instance_id)
            r.need((phase_stop - dt.datetime.now(dt.timezone.utc)).total_seconds() > 360,
                   'Insufficient high-CPU boot/control window; recover without start')
            command('start32-01', scope('start'), 90)
        elif args.step == 'restore2':
            observed(args.step, 'e2-standard-2', phase_stop, instance_id)
            command('restore2-schedule01', scope('set-scheduling') + ['--termination-time=' + stop.strftime('%Y-%m-%dT%H:%M:%SZ'),
                    '--instance-termination-action=STOP', '--no-restart-on-failure'])
        else:
            observed(args.step, 'e2-standard-2', stop, instance_id)
            r.need((stop - dt.datetime.now(dt.timezone.utc)).total_seconds() >= 600,
                   'Recovery requires at least 10 minutes within original STOP')
            command('start2-recovery01', scope('start'), 90)


if __name__ == '__main__':
    main()
