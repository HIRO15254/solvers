"""Lightweight syntax and Cargo/test-output parser checks; no Rust execution."""
import ast
import importlib.util
import json
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("extra_validation_test", HERE / "run.py")
run = importlib.util.module_from_spec(spec)
spec.loader.exec_module(run)


def cargo_stdout():
    return b"\n".join(json.dumps({"reason": "compiler-artifact", "target": {"name": name, "kind": ["test"]},
                                "profile": {"test": True}, "executable": "/opt/r1/target/exact-new04/release/deps/" + name + "-abc"}).encode()
                       for name in ("postflop", "rake_icm"))


class Tests(unittest.TestCase):
    def test_syntax_and_frozen_test_list(self):
        ast.parse((HERE / "run.py").read_text(encoding="utf-8"))
        self.assertEqual(len(run.TESTS), 4)
        self.assertEqual(len(set(name for _, name in run.TESTS)), 4)
        self.assertEqual({target for target, _ in run.TESTS}, {"postflop", "rake_icm"})

    def test_exact_one_test_success_and_zero_test_rejection(self):
        name = run.TESTS[0][1]
        output = f"running 1 test\ntest {name} ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 51 filtered out; finished in 0.01s\n".encode()
        self.assertEqual(run.successful_test(output, name)["passed"], 1)
        for bad in (output.replace(b"1 passed", b"0 passed"), output.replace(name.encode(), b"another_test"),
                    output.replace(b"0 ignored", b"1 ignored"), output.replace(b"... ok", b"... FAILED"), output + output):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                run.successful_test(bad, name)

    def test_cargo_paths_are_portable_posix_and_both_harnesses_required(self):
        output = cargo_stdout()
        self.assertEqual(set(run.compiler_executables(output, "/opt/r1/target/exact-new04")), {"postflop", "rake_icm"})
        for bad in (output.splitlines()[0], output + b"\n" + output, output.replace(b"release/deps", b"debug/deps"), output.replace(b'"test": true', b'"test": false')):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                run.compiler_executables(bad, "/opt/r1/target/exact-new04")

    def test_commands_are_finite_scope(self):
        plan = {"tools": {"cargo": {"path": "/cargo"}}, "target": "/same-target"}
        argv = run.build_command(plan)
        self.assertIn("--no-run", argv)
        self.assertIn("--locked", argv)
        self.assertEqual(argv.count("--test"), 2)
        for _, name in run.TESTS:
            self.assertEqual(run.test_command({"path": "/test"}, name), ["/test", name, "--exact", "--ignored", "--test-threads=1"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
