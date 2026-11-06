"""Deterministic tests of the class-level benchmark tools."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest

import numpy as np

import bench_decision
import bench_flops
from bench_metric import combine, error_part, read_flop_diag
from p1_classes import class_values
from compare import our_index

HERE = Path(__file__).resolve().parent


class FlopDiagnosticTests(unittest.TestCase):
    def test_reads_class_values_and_matchup_mass_for_each_flop_and_seat(self):
        text = ("flop Ks 7h 2d\nhero 2\n"
                "class,ev_l1,ev_best,ev_checkdown,l1_minus_checkdown,best_minus_l1,matchup_mass\n"
                "AA,-1.25,0,0,0,0,12\nhero 5\nAKs,2.5,0,0,0,0,4\n"
                "flop As 8h 3d\nhero 2\nAA,3.75,0,0,0,0,6\n")
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "diag.txt"
            path.write_text(text, encoding="utf-8")
            self.assertEqual(read_flop_diag(path), {
                "Ks 7h 2d": {2: {"AA": (-1.25, 12.0)}, 5: {"AKs": (2.5, 4.0)}},
                "As 8h 3d": {2: {"AA": (3.75, 6.0)}},
            })


class FlopSampleTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.rows, cls.out, cls.pilot, cls.master = bench_flops.draw()

    def test_samples_are_disjoint_and_avoid_excluded_flops(self):
        pilot = {r[0] for r in self.pilot}
        master = {r[0] for r in self.master}
        self.assertEqual((len(pilot), len(master)), (24, 192))
        self.assertFalse(pilot & master)
        self.assertFalse((pilot | master) & self.out)

    def test_same_seed_same_sample(self):
        self.assertEqual(bench_flops.draw()[3], self.master)

    def test_groups_are_proportional_to_raw_counts(self):
        pilot = {r[0] for r in self.pilot}
        population = [r for r in self.rows if r[0] not in self.out and r[0] not in pilot]
        total = sum(r[2] for r in population)
        for g in {r[1] for r in population}:
            weight = sum(r[2] for r in population if r[1] == g)
            count = sum(1 for r in self.master if r[1] == g)
            self.assertLess(abs(count - 192 * weight / total), 1, g)

    def test_subsets_nest_and_halves_partition(self):
        sets = {k: set(bench_flops.subset_indices(192, k)) for k in bench_flops.SIZES}
        for small, large in ((8, 16), (16, 32), (32, 64), (64, 192), (32, 96), (96, 192)):
            self.assertLessEqual(sets[small], sets[large])
        for k in bench_flops.SIZES:
            dev = set(bench_flops.subset_indices(192, k, "dev"))
            held = set(bench_flops.subset_indices(192, k, "held"))
            self.assertFalse(dev & held)
            self.assertEqual(dev | held, sets[k])
        with self.assertRaises(ValueError):
            bench_flops.subset_indices(24, 16)


def leaf_profile(labels, actors, overrides):
    nodes = []
    for k, actor in enumerate(actors):
        probabilities = [[1.0] for _ in range(169)]
        for (seat, name), p in overrides.items():
            if seat == actor:
                probabilities[our_index(name)] = [p]
        nodes.append(dict(path=labels[:k], actor=actor, actions=[labels[k]], probabilities=probabilities))
    return nodes


class P1LeafOptionTests(unittest.TestCase):
    LABELS = 'fold/fold/fold/raise-to:2500/raise-to:12000/raise-to:23000/fold/call:11000'.split('/')

    def ranges(self, *options):
        nodes = leaf_profile(self.LABELS, [3, 4, 5, 0, 1, 2, 0, 1],
                             {(2, '72o'): 0.0, (2, '32o'): 0.0, (2, 'AKo'): 0.3})
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'profile.json').write_text(json.dumps({'nodes': nodes}), encoding='utf-8')
            (root / 'extra.txt').write_text(f"{'/'.join(self.LABELS)} BB 72o 0.001\nother/line BB 32o 0.5\n",
                                            encoding='utf-8')
            options = [str(root / o) if o == 'extra.txt' else o for o in options]
            subprocess.run([sys.executable, str(HERE / 'p1_leaf.py'), str(root / 'profile.json'),
                            '/'.join(self.LABELS), 'raise', str(root / 'leaf.toml'), *options], check=True)
            text = (root / 'leaf.toml').read_text(encoding='utf-8')
        bb = next(line for line in text.splitlines() if line.startswith('BB = '))
        return dict(x.split(':') for x in bb[6:-1].split(','))

    def test_min_weight_drops_classes(self):
        self.assertIn('AKo', self.ranges())
        self.assertNotIn('AKo', self.ranges('--min-weight', '0.5'))

    def test_extra_adds_class(self):
        self.assertNotIn('72o', self.ranges())
        ranges = self.ranges('--extra', 'extra.txt')
        self.assertEqual(ranges['72o'], '0.001000')
        self.assertNotIn('32o', ranges)


class P1ClassValueTests(unittest.TestCase):
    def test_inclusion_exclusion_and_class_values(self):
        rows = [dict(seat='oop', combo='AsAh', weight='1', ev='2'),
                dict(seat='oop', combo='AdAc', weight='0.5', ev='4'),
                dict(seat='oop', combo='KsQs', weight='1', ev='-1'),
                dict(seat='ip', combo='AsKd', weight='1', ev='0'),
                dict(seat='ip', combo='QdJd', weight='2', ev='1'),
                dict(seat='ip', combo='AhAs', weight='0.25', ev='3')]
        sides = class_values(rows)

        def brute(combo, side):
            cards = {combo[:2], combo[2:]}
            return sum(float(r['weight']) for r in rows if r['seat'] != side
                       and not cards & {r['combo'][:2], r['combo'][2:]})

        aa = [(1.0, brute('AsAh', 'oop'), 2.0), (0.5, brute('AdAc', 'oop'), 4.0)]
        self.assertEqual([c for _, c, _ in aa], [2.0, 3.25])
        value, mass, own = sides['oop']['classes']['AA']
        self.assertAlmostEqual(value, sum(w * c * e for w, c, e in aa) / sum(w * c for w, c, _ in aa))
        self.assertAlmostEqual(mass, 5.25)
        self.assertAlmostEqual(own, 1.5)
        kq = brute('KsQs', 'oop')
        total = sum(w * c * e for w, c, e in aa) + kq * -1
        self.assertAlmostEqual(sides['oop']['mean'], total / (sum(w * c for w, c, _ in aa) + kq))


class ClassErrorTests(unittest.TestCase):
    def part(self, e):
        m = np.ones_like(e)
        w = np.zeros((1, 169))
        w[0, :3] = [1.0, 2.0, 3.0]
        return combine([error_part(e, m, w, np.ones((1, 169), bool))])

    def test_zero_error(self):
        self.assertEqual(self.part(np.zeros((1, 169, 8)))['A'], 0.0)

    def test_constant_error_is_all_level(self):
        r = self.part(np.full((1, 169, 8), 0.3))
        self.assertAlmostEqual(r['A'], 0.3)
        self.assertAlmostEqual(r['level'], 0.3)
        self.assertAlmostEqual(r['shape'], 0.0)

    def test_noise_correction_removes_flop_noise(self):
        e = np.tile(np.array([0.5, -0.5] * 4), (1, 169, 1))
        self.assertEqual(self.part(e)['A'], 0.0)


class DecisionLossTests(unittest.TestCase):
    def test_flip_costs_the_value_gap(self):
        ref = dict(line_probability=0.5, range=[1.0, 0.0], range_combos=24.0,
                   actions=[dict(ev=[1.0, 0.0]), dict(ev=[0.9, 0.0])])
        tree = SimpleNamespace(
            decisions=[((), ref, ['raise', 'call'])], nodes={(): dict(actor=2)}, ref_classes=['AKs', 'AA'],
            n=[4, 6],
            downstream=lambda path, choice, actor: [(1, 1.0, np.ones(2))] if choice == 'raise' else [])
        decisions = bench_decision.decide(tree, {(1, 2): {'AKs': -0.2}})
        self.assertEqual(decisions[0]['l1'][0], 1)
        self.assertAlmostEqual(bench_decision.loss(decisions), 100 * 0.5 * 4 / 24 * 0.1)


class OverrideBuildTests(unittest.TestCase):
    def setUp(self):
        import bench_override
        self.tool = bench_override
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.first, self.second = self.root / "p1", self.root / "p2"
        self.bands = [("srp", ["line"], ["Ks 7h 2d", "As 8h 3d"])]
        diag = self.root / "srp" / "v"
        diag.mkdir(parents=True)
        (diag / "diag1.txt").write_text(
            "flop Ks 7h 2d\nhero 2\nAA,10,0,0,0,0,1\nKK,20,0,0,0,0,1\n"
            "flop As 8h 3d\nhero 2\nAA,100,0,0,0,0,1\nKK,200,0,0,0,0,1\n",
            encoding="utf-8")
        for directory, task, value in ((self.first, "L01-multi", 3), (self.second, "L01-raise", 7)):
            path = directory / task
            path.mkdir(parents=True)
            (path / "Ks7h2d.json").write_text(json.dumps(dict(
                line="line", seats={"2": {"classes": {"AA": [value], "KK": [value + 1]}}})),
                encoding="utf-8")

    def build(self, **kwargs):
        return self.tool.build(self.bands, self.root, "v", **kwargs)["entries"][0]["values"]

    def test_first_directory_wins_and_missing_flops_are_excluded(self):
        self.assertEqual(self.build(p1_dir=[self.first, self.second]), {"AA": 3, "KK": 4})
        self.assertEqual(self.build(p1_dir=[self.second, self.first]), {"AA": 7, "KK": 8})
        self.assertEqual(self.build(flops_of=[self.first, self.second]), {"AA": 10, "KK": 20})

    def test_missing_line_is_error(self):
        with self.assertRaisesRegex(ValueError, "no P1 task"):
            self.build(p1_dir=[self.root / "missing"])

    def test_duplicate_line_is_error(self):
        import shutil
        shutil.copytree(self.first / "L01-multi", self.first / "L02-raise")
        with self.assertRaisesRegex(ValueError, "duplicate P1 line"):
            self.build(p1_dir=[self.first])

    def test_classes_of_restricts_and_requires_seat(self):
        path = self.root / "classes.json"
        path.write_text(json.dumps(dict(entries=[dict(line="line", seat=2, values={"KK": 0})])),
                        encoding="utf-8")
        self.assertEqual(self.build(classes_of=path), {"KK": 110})
        path.write_text('{"entries": []}', encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "no entry"):
            self.build(classes_of=path)


class ImproveVerdictTests(unittest.TestCase):
    def test_pooled_variance_uses_within_label_pairs(self):
        from improve_table import pooled_variance
        self.assertEqual(pooled_variance([[1, 3], [100, 104], [9]]), 5)
        with self.assertRaises(ValueError):
            pooled_variance([[1]])

    def test_verdict_boundaries(self):
        from improve_table import verdict
        self.assertEqual(verdict(-0.2, 0.1, 0.05), "equal")
        self.assertEqual(verdict(0.2, 0.1, 0.05), "equal")
        self.assertEqual(verdict(-0.20001, 0.1, 0.05), "better")
        self.assertEqual(verdict(0.20001, 0.1, 0.05), "worse")
        self.assertEqual(verdict(0.05, 0, 0.05), "equal")

    def test_names(self):
        from improve_table import FULL_NAME, RULER_NAME
        m = RULER_NAME.fullmatch("ovm-r16g49-sd12")
        self.assertEqual((m.group(1), m["label"], m["seed"]), ("ovm", "r16g49", "12"))
        self.assertIsNone(RULER_NAME.fullmatch("ov-e2-sd1-extra"))
        self.assertIsNone(RULER_NAME.fullmatch("ov-e2-sd"))
        self.assertEqual(FULL_NAME.fullmatch("aws-im-d-r8g25q-sd0")["label"], "r8g25q")

    def test_plateau(self):
        from improve_table import plateau
        self.assertEqual(plateau({2000: 1, 4000: 0.99, 6000: 0.8}), 4000)
        self.assertEqual(plateau({2000: 1, 4000: 0.9, 6000: 0.8}), "6000まで改善")


class ImproveTableTests(unittest.TestCase):
    def test_ruler_and_full_read_real_file_layouts(self):
        import improve_table as tool
        reference = dict(classes=["AA"], nodes=[dict(history=[], range=[1], line_probability=1,
                         actions=[dict(code="F", strategy=[1], ev=[0])])])
        profile = dict(nodes=[dict(path=[], actions=["fold"], probabilities=[[1]] * 169)])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)

            def put(path, data):
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(json.dumps(data), encoding="utf-8")

            def evaluation(path, value):
                put(path, dict(evaluation=dict(nash_conv=value)))

            for game in ("ov", "ovm"):
                truth = "truth-p1" if game == "ov" else "truth-multi"
                for label in ("truth", "e2"):
                    for seed in (0, 1):
                        name = f"{game}-{label}-sd{seed}"
                        d = root / "ruler" / name
                        evaluation(d / f"{truth}.eval.json", 0.01 + seed * 0.002)
                        put(d / f"{name}.json", dict(checkpoints=[dict(nash_conv=0.03)]))
                        put(d / f"{name}.profile.json", profile)
            r = tool.ruler(root / "ruler", reference)
            self.assertAlmostEqual(r["variance"], 0.02)
            self.assertEqual(r["games"]["ovm"]["truth"]["verdict"], "equal")
            self.assertEqual(r["games"]["ov"]["e2"]["seeds"][0]["training_nash_conv"], 3)
            e2, e2_eval = [], []
            for seed in (0, 1):
                d = root / f"e2-{seed}"
                put(d / "it002000.profile.json", profile)
                e2.append(d)
                ev = root / f"eval-{seed}"
                evaluation(ev / "truth-p1.eval.json", 0.01 + 0.002 * seed)
                evaluation(ev / "truth-multi.eval.json", 0.02)
                e2_eval.append(ev)
            run = "im-d-candidate-sd0"
            d = root / "results" / f"aws-{run}"
            put(d / "base.json", dict(checkpoints=[dict(nash_conv=0.03)]))
            (d / "variants.txt").write_text("base\n", encoding="utf-8")
            (d / "vm.log").write_text("type=c7a.8xlarge\n", encoding="utf-8")
            (d / "base.log").write_text("L1: 10 slots; 160 storage bytes\n", encoding="utf-8")
            (d / "base.1.time").write_text(
                "Maximum resident set size (kbytes): 1048576\n"
                "Elapsed (wall clock) time (h:mm:ss or m:ss): 1:02:03\n", encoding="utf-8")
            evaluation(d / "base.truth-p1.eval.json", 0.01)
            evaluation(d / "base.truth-multi.eval.json", 0.02)
            put(root / "cache" / run / "snap-base/it002000.profile.json", profile)
            put(root / "cache" / run / "snap-base/it004000.profile.json", profile)
            args = SimpleNamespace(e2=e2, e2_eval=e2_eval, results=root / "results",
                                   cache=root / "cache", e2_plateau=e2[0])
            f = tool.full(args, reference)
            row, = f["runs"]
            self.assertEqual(row["times"][0]["rss_gib"], 1)
            self.assertEqual(row["times"][0]["c7a_seconds"], 3723)
            self.assertEqual(row["checkpoints"][0]["nash_conv"], 3)
            self.assertEqual(row["plateau"], 4000)
            self.assertIn("|candidate|loss|", tool.markdown(f, "full"))
            self.assertIn("|ov|truth|", tool.markdown(r, "ruler"))


class JobEvaluationTests(unittest.TestCase):
    def test_completed_variant_evaluations_retry_and_drop_only_after_success(self):
        # Run the actual variant loop, bypassing only cloud setup and artifact packaging.
        import os
        import shutil
        bash = shutil.which("bash")
        if os.name == "nt":
            git = shutil.which("git")
            if git:
                candidate = Path(git).parent.parent / "usr/bin/bash.exe"
                if candidate.exists():
                    bash = str(candidate)
        source = (HERE.parent / "relative-reach/gcp/job.sh").read_text(encoding="utf-8")
        loop = source[source.index("for v in $(tr"):source.index("(cd ~ && sha256sum")]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ("results", "final", "target/release/examples"):
                (root / name).mkdir(parents=True)
            variant = "traise5+hdq+sd1"
            (root / "variants.txt").write_text(variant + "\n", encoding="utf-8")
            (root / "results/progress.txt").write_text(f"END {variant} 0\n", encoding="utf-8")
            (root / "eval-overrides.txt").write_text("truth-p1.json\ntruth-multi.json\n", encoding="utf-8")
            (root / "drop_final_postflop").touch()
            postflop = root / f"final/{variant}.postflop.bin"
            postflop.write_bytes(b"postflop")
            solver = root / "target/release/examples/trunk_solve"
            solver.write_text(
                '#!/bin/bash\nprintf "%s\\n" "$@" >> "$HOME/calls"\n'
                'out=; override=\nwhile [ "$#" -gt 0 ]; do\n'
                'case "$1" in --output) out=$2; shift;; --l1-eval-override) override=$2; shift;; esac\n'
                'shift\ndone\n'
                'if [[ $override == *truth-multi.json ]] && [ ! -f "$HOME/succeed" ]; then exit 7; fi\n'
                'printf "{}\\n" > "$out"\n', encoding="utf-8", newline="\n")
            solver.chmod(0o755)
            script = 'export HOME="$(pwd)"; R="$HOME/results"; default_config=b4s-gtow.toml\n' + loop + "\ntrue\n"
            subprocess.run([bash, "-c", script], cwd=root, check=True)
            self.assertTrue(postflop.exists())
            self.assertIn("EVAL_FAILED", (root / "results/progress.txt").read_text())
            self.assertTrue((root / f"results/{variant}.truth-p1.eval.json").exists())
            (root / "succeed").touch()
            subprocess.run([bash, "-c", script], cwd=root, check=True)
            self.assertFalse(postflop.exists())
            self.assertTrue((root / "results/postflop.sha256").exists())
            calls = (root / "calls").read_text()
            self.assertIn("draws,pairs", calls)
            self.assertIn("--seed\n1", calls)
            self.assertEqual(calls.count("--evaluate-profile"), 3)


if __name__ == '__main__':
    unittest.main()
