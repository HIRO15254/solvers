"""Pure admission/command tests; no solver, supervisor, or cloud execution."""
import unittest

import run


class RunnerTests(unittest.TestCase):
    def test_global_limit_never_extends_original_deadline(self):
        self.assertEqual(run.budget_end(1000, 2000, 1001), 1240)
        self.assertEqual(run.budget_end(1000, 1100, 1001), 1100)
        for values in ((1000, 999, 1001), (1000, 1001, 1001), (1000, 2000, 999)):
            with self.assertRaises(ValueError):
                run.budget_end(*values)

    def test_stage_requires_full_bound_and_cleanup_tail(self):
        run.stage_fits(200, 134.99)
        for now in (135, 140, 201):
            with self.assertRaises(ValueError):
                run.stage_fits(200, now)

    def test_fixed_schedule_and_native_cli_shapes(self):
        rows = run.schedule()
        self.assertEqual(len(rows), 8)
        self.assertEqual([(r['case'], r['kind']) for r in rows],
                         [(case, kind) for case in ('006', '022') for kind in ('validate', 'solve', 'tree', 'audit')])
        plan = {'output': '/proof', 'cli': {'path': '/solvers'}, 'audit': {'path': '/audit'},
                'inputs': {case: {'path': '/' + case + '.toml'} for case in ('006', '022')}}
        commands = [run.command(plan, row) for row in rows]
        self.assertNotIn('--resources', commands[0])
        self.assertIn('--show-effective', commands[0])
        self.assertIn('--write-effective', commands[0])
        self.assertEqual(commands[1], ['/solvers', 'solve', '/006.toml', '--out', '/proof/stages/006-solve/run', '--sol-streets', 'full'])
        self.assertEqual(commands[2][-5:], ['tree', '--node', 'all', '--format', 'json'])
        self.assertEqual(commands[3][-2:], ['--threads', '1'])
        self.assertNotIn('--max-time', commands[1])

    def test_finite_supervisor_limits(self):
        self.assertEqual(run.LIMITS['timeout_seconds'], 45)
        self.assertEqual(run.LIMITS['memory_limit_bytes'], 10 * 1024**3)
        self.assertEqual(run.LIMITS['disk_reserve_bytes'], 4 * 1024**3)
        self.assertEqual(run.LIMITS['min_free_memory_bytes'], 1024**3)
        self.assertEqual((run.LIMITS['grace_seconds'], run.LIMITS['kill_wait_seconds']), (5, 5))


if __name__ == '__main__':
    unittest.main(verbosity=2)
