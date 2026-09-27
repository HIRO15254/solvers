"""Small source-only checks; Rust tests are retained but have not been executed."""
from pathlib import Path
import importlib.util
import unittest

HERE=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location("root_action_preparer",HERE/"prepare.py")
prepare=importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare)

class SourceScope(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.original=(prepare.ROOT/prepare.SOURCE).read_bytes()
        cls.method=(HERE/"root_plan.rs.in").read_text(encoding="utf-8")
        cls.tests=(HERE/"plan_tests.rs.in").read_text(encoding="utf-8")
        cls.candidate=prepare.transform(cls.original,cls.method,cls.tests)

    def test_exact_application_and_inverse(self):
        self.assertEqual(prepare.inverse(self.candidate,self.method,self.tests),self.original)
        self.assertEqual((HERE/"solver.rs").read_bytes(),self.candidate)
        self.assertTrue(prepare.verify_scope(self.original,self.candidate,self.method,self.tests)["inverse_exact"])

    def test_changed_baseline_and_ambiguous_anchors_rejected(self):
        with self.assertRaises(ValueError):prepare.transform(self.original+b"\n",self.method,self.tests)
        with self.assertRaises(ValueError):prepare.replace_once("xx","x","y")

    def test_quality_or_arithmetic_mutation_rejected(self):
        for old,new in ((b"    let action_plan = ActionPlan::new(ctx.tree);",b"    let action_plan = ActionPlan::for_cfr(ctx.tree);"),
                        (b"node_cfv[h] += row[h] * cfvs[a * num_hands + h];",b"node_cfv[h] = row[h] * cfvs[a * num_hands + h];")):
            self.assertIn(old,self.candidate)
            changed=self.candidate.replace(old,new,1)
            with self.assertRaises(ValueError):prepare.verify_scope(self.original,changed,self.method,self.tests)

    def test_only_two_cfr_entries_and_existing_quality_guard(self):
        text=self.candidate.decode()
        self.assertEqual(text.count("ActionPlan::for_cfr(&self.game.tree)"),2)
        self.assertEqual(text.count("ActionPlan::new(&self.game.tree)"),1)
        self.assertEqual(text.count("ActionPlan::new(ctx.tree)"),2)
        self.assertIn("nodes[0] = ActionSplit",self.method)
        self.assertNotIn("par_budget",self.method)
        self.assertNotIn("unsafe",self.method)
        self.assertIn("return Self::new(tree)",self.method)
        self.assertIn("tree.node(0).kind != NodeKind::Action",self.method)

if __name__=="__main__":
    unittest.main()
