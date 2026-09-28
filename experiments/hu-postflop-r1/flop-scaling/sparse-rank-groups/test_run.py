"""Pure finite protocol tests; no host probe, build, solve, API or archive access."""
import copy
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('sparse_test_run', HERE / 'run.py')
run = importlib.util.module_from_spec(spec)
spec.loader.exec_module(run)


class Contracts(unittest.TestCase):
    def test_fixed_38_schedule_has_adjacent_paired_arms(self):
        rows = run.schedule()
        self.assertEqual(len(rows), 38)
        self.assertEqual(len({r['name'] for r in rows}), 38)
        self.assertEqual([r['kind'] for r in rows[:6]], ['smoke'] * 4 + ['canonical'] * 2)
        self.assertEqual([(r['workers'], r['arm']) for r in rows[:4]], [(1, 'baseline'), (1, 'candidate'), (32, 'baseline'), (32, 'candidate')])
        self.assertEqual(sum(r['warmup'] for r in rows), 8)
        self.assertEqual({r['depth'] for r in rows}, {2})
        for case in run.CASES:
            for workers in run.WORKERS:
                for arm in run.ARMS:
                    self.assertEqual([r['round'] for r in rows if r['kind'] == 'matrix' and (r['case'], r['workers'], r['arm']) == (case, workers, arm)], [0, 1, 2, 3])
        for a, b in zip(rows[6::2], rows[7::2]):
            self.assertEqual((a['case'], a['workers'], a['round']), (b['case'], b['workers'], b['round']))
            self.assertEqual([a['arm'], b['arm']], list(run.ARMS if a['round'] % 2 == 0 else run.ARMS[::-1]))
        self.assertEqual({run.canonical_key(r) for r in rows}, {'smoke-narrow', 'narrow', 'expanded'})

    def test_existing_absolute_stop_and_short_measurement_bounds(self):
        args = ['2026-09-28T00:00:00Z', '2026-09-28T00:45:00Z', '2026-09-28T00:17:40Z', 'measure',
                '2026-09-28T00:12:00Z', '2026-09-28T00:10:00Z', '2026-09-28T00:18:00Z']
        run.bounds(*args)
        for index, value in [(2, '2026-09-28T00:17:41Z'), (2, '2026-09-28T00:18:01Z'),
                             (2, '2026-09-28T00:16:00Z'), (4, '2026-09-28T00:11:39Z'),
                             (6, '2026-09-28T00:18:01Z'), (1, '2026-09-28T00:46:00Z')]:
            bad = args.copy(); bad[index] = value
            with self.assertRaises(ValueError):
                run.bounds(*bad)
        run.bounds(*args[:2], '2026-09-28T00:20:00Z', 'build', '2026-09-28T00:01:00Z')
        with self.assertRaises(ValueError):
            run.bounds(*args[:2], '2026-09-28T00:20:01Z', 'build', '2026-09-28T00:01:00Z')

    def test_two_fresh_targets_and_required_kernel_oracle_tests(self):
        self.assertEqual(len(run.build_rows()), 5)
        plan = {'tools': {'cargo': {'path': '/tool/cargo'}}, 'workspace': '/work'}
        for arm in run.ARMS:
            command = run.test_command(plan, arm)
            self.assertIn('/work/' + arm + '-target', command)
            self.assertTrue({'engine', 'holdem', 'cfr-ref', '--offline', '--locked', '--release', '-j2'} <= set(command))
            names = run.BASE_TEST_NAMES + run.KERNEL_TEST_NAMES + (run.CANDIDATE_TEST_NAMES if arm == 'candidate' else ())
            stdout = '\n'.join('test ' + name + ' ... ok' for name in names)
            run.validate_tests(stdout, arm)
            for changed in (stdout.replace(names[-1], 'missing'), stdout + '\ntest ' + names[-1] + ' ... ok'):
                with self.assertRaises(ValueError):
                    run.validate_tests(changed, arm)

    def test_stage_timeout_hook_keeps_frozen_supervision(self):
        with patch.object(run, 'base_stage', return_value='stage') as stage:
            for kind, expected in [('canonical', 60), ('smoke', 20), ('matrix', 20), ('build', 480)]:
                row = {'kind': kind}
                self.assertEqual(run.stage('out', 'plan', row, 'receipt', 480, 'cwd'), 'stage')
                self.assertEqual(stage.call_args.args[4], expected)
        self.assertIs(run.base.stage, run.stage)
        self.assertEqual(run.base.__file__, run.__file__)

    def test_only_candidate_kernel_may_differ(self):
        originals = {run.SOLVER: {'sha256': run.SOLVER_SHA}, run.KERNEL: {'sha256': run.KERNEL_SHA},
                     'Cargo.toml': {}, 'Cargo.lock': {}, '.cargo/config.toml': {}}
        adapter = {'sha256': run.CPU_ADAPTER_SHA}
        baseline = originals | {f'crates/holdem/examples/{run.EXAMPLE}.rs': adapter}
        sources = {'baseline': baseline, 'candidate': baseline | {run.KERNEL: run.pin(HERE / 'kernel.rs')}}
        run.source_bindings(originals, sources, adapter)
        bad = copy.deepcopy(sources); bad['candidate'][run.SOLVER]['sha256'] = 'changed'
        with self.assertRaises(ValueError):
            run.source_bindings(originals, bad, adapter)
        bad = copy.deepcopy(sources); bad['candidate']['extra.rs'] = {}
        with self.assertRaises(ValueError):
            run.source_bindings(originals, bad, adapter)

    def test_retained_vm19_manifest_and_explicit_adapter_overlay(self):
        path = HERE.parents[1] / 'cloud/vm19/source-manifest.json'
        self.assertEqual(run.pin(path), run.BASE_MANIFEST)
        manifest = run.read(path)
        self.assertEqual(manifest['source_revision'], run.REVISION)
        expected_overlay = {'experiments/hu-postflop-r1/flop-scaling/chance-grain/adapter/' + name
                            for name in ('solve.rs', 'prepare.py', 'provenance.json')}
        missing = set()
        for control in run.controls():
            if control.parent == HERE:
                continue
            name = control.relative_to(run.ROOT).as_posix()
            if name in manifest['files']:
                self.assertEqual(manifest['files'][name], run.pin(control))
            else:
                missing.add(name)
        self.assertEqual(missing, expected_overlay)
        originals = manifest['source_pins']
        adapter = run.pin(run.CPU_ADAPTER)
        baseline = originals | {f'crates/holdem/examples/{run.EXAMPLE}.rs': adapter}
        run.source_bindings(originals, {'baseline': baseline, 'candidate': baseline | {run.KERNEL: run.pin(HERE / 'kernel.rs')}}, adapter)


if __name__ == '__main__':
    unittest.main()
