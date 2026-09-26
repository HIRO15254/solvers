"""Read original host pins and current sysfs after a failed service has exited."""
import json
from pathlib import Path

root = Path('/opt/r1/final-proof01')
plan = json.loads((root / 'plan.json').read_bytes())
build = json.loads((root / 'build.json').read_bytes())
result = {
    'scope': 'Post-failure observations; not the missing exact failure-time host snapshot',
    'planned_host': plan['host'],
    'build_stages': [{key: row[key] for key in ('stage', 'status', 'host_before', 'host_after', 'error') if key in row}
                     for row in build['stages']],
    'boot_id_now': Path('/proc/sys/kernel/random/boot_id').read_text().strip(),
    'cpu_models_now': sorted({line.split(':', 1)[1].strip() for line in Path('/proc/cpuinfo').read_text().splitlines()
                              if line.startswith('model name')}),
    'topology_now': [],
    'cgroup_paths_now': {},
}
for row in plan['host']['topology']:
    cpu = row['cpu']
    path = Path(f'/sys/devices/system/cpu/cpu{cpu}/topology')
    result['topology_now'].append({'cpu': cpu, 'core': (path / 'core_id').read_text().strip(),
                                   'socket': (path / 'physical_package_id').read_text().strip()})
cg = Path(plan['host']['cgroup']['path'])
for path in (cg, *cg.parents):
    if path.is_relative_to('/sys/fs/cgroup'):
        result['cgroup_paths_now'][str(path)] = {name: (path / name).read_text().strip()
                                               if (path / name).is_file() else None
                                               for name in ('cpu.max', 'cgroup.controllers', 'cgroup.subtree_control')}
print(json.dumps(result, indent=2))
