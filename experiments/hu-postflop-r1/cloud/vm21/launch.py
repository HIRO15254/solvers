"""Create VM21 once, only after root freezes package pins and applies its reservation."""
import datetime as dt
import os
import subprocess
from pathlib import Path
import reserve as r

GCLOUD = 'C:/Program Files (x86)/Google/Cloud SDK/google-cloud-sdk/bin/gcloud.cmd'


def main():
    row, check = r.authorized()
    now = dt.datetime.now(dt.timezone.utc)
    r.need(0 <= (now - r.utc(check['at_utc'])).total_seconds() <= 1800,
           'Reservation review must be at most 30 minutes old')
    # Created only after independent review of the final deployment. No provisional defaults.
    approval = r.read(r.HERE / 'launch-approval.json')
    r.need(approval['schema'] == 'r1.vm21-launch-approval/v1'
           and approval['resource'] == r.NAME and approval['package_review_passed'] is True,
           'Final reviewed package required')
    for name in ['../vm19/deployment01.tar.gz', '../vm19/source-manifest.json', '../vm19/bootstrap.sh',
                 'overlay-manifest.json', 'install-overlay.py', 'launch.py', 'reserve.py', 'phase.py', 'capture-command.py']:
        r.need(r.pin(r.HERE / name) == approval['files'][name], 'Frozen launch input changed: ' + name)
    for kind in ['instances', 'disks', 'addresses']:
        stem = 'preflight-' + kind + '01'
        receipt = r.read(r.HERE / (stem + '.result.json'))
        r.need(receipt['exit_code'] == 0 and receipt['stdout'] == r.pin(r.HERE / (stem + '.stdout.log'))
               and receipt['stderr'] == r.pin(r.HERE / (stem + '.stderr.log'))
               and r.read(r.HERE / (stem + '.stdout.log')) == [], 'Resource preflight failed: ' + kind)
        r.need(receipt['argv'][1:] == ['compute', kind, 'list', '--project=' + r.PROJECT,
                                     '--filter=name~solvers-r1-', '--format=json', '--quiet'],
               'Resource preflight scope differs')
        r.need(0 <= (now - r.utc(receipt['ended_utc'])).total_seconds() <= 900,
               'Resource preflight must be at most 15 minutes old')
    # Re-read time after all local checks; floor to seconds, never lengthen the 45-minute bound.
    now = dt.datetime.now(dt.timezone.utc)
    stop = (now + dt.timedelta(seconds=2700)).replace(microsecond=0)
    argv = ['compute', 'instances', 'create', r.NAME, '--project=' + r.PROJECT, '--zone=' + r.ZONE,
            '--machine-type=e2-standard-2', '--provisioning-model=SPOT', '--instance-termination-action=STOP',
            '--termination-time=' + stop.strftime('%Y-%m-%dT%H:%M:%SZ'), '--no-restart-on-failure',
            '--maintenance-policy=TERMINATE', '--image-family=ubuntu-2404-lts-amd64', '--image-project=ubuntu-os-cloud',
            '--boot-disk-type=pd-balanced', '--boot-disk-size=20GB', '--boot-disk-auto-delete',
            '--no-service-account', '--no-scopes', '--labels=campaign=hu-postflop-r1,reservation=' + r.ID,
            '--metadata-from-file=startup-script=' + str(r.CLOUD / 'vm19/bootstrap.sh'),
            '--format=json(id,name,status,creationTimestamp,scheduling,disks)', '--quiet']
    record = {'reservation_id': r.ID, 'fresh_instance_and_build': True, 'attempted_at': now.isoformat(),
              'termination_time': stop.isoformat(), 'build_deadline_utc': (stop - dt.timedelta(seconds=1500)).isoformat(),
              'argv': argv, 'client_timeout_seconds': 180, 'launch_approval': r.pin(r.HERE / 'launch-approval.json')}
    r.fresh_json(r.CLOUD / ('launch-' + r.ID + '.json'), record)
    env = dict(os.environ, CLOUDSDK_PYTHON='C:/Python313/python.exe', CLOUDSDK_ENCODING='utf-8', PYTHONIOENCODING='utf-8')
    try:
        done = subprocess.run([GCLOUD, *argv], capture_output=True, env=env, timeout=180)
        code, stdout, stderr = done.returncode, done.stdout, done.stderr
    except subprocess.TimeoutExpired as error:
        code, stdout, stderr = None, error.stdout or b'', error.stderr or b''
        record['error'] = 'Creation timeout: inspect named resource; never retry creation.'
    for path, data in [(r.CLOUD / ('create-result-' + r.ID + '.json'), stdout),
                       (r.HERE / 'create.stderr.log', stderr)]:
        with path.open('xb') as stream:
            stream.write(data)
    record.update(exit_code=code, ended_utc=dt.datetime.now(dt.timezone.utc).isoformat())
    record['stdout'] = r.pin(r.CLOUD / ('create-result-' + r.ID + '.json'))
    record['stderr'] = r.pin(r.HERE / 'create.stderr.log')
    r.fresh_json(r.HERE / 'create.receipt.json', record)
    raise SystemExit(0 if code == 0 else 1)


if __name__ == '__main__':
    main()
