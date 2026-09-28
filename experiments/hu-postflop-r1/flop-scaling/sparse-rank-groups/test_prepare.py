"""Small source/application guards only; Rust tests remain uncompiled."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import uuid
import unittest

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location('sparse_prepare', HERE / 'prepare.py')
p = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(p)


class SourceChecks(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.original = (p.ROOT / p.SOURCE).read_bytes()

    def test_modified_baseline_rejected(self):
        with self.assertRaises(ValueError):
            p.transform(self.original + b'\n')

    def test_fold_or_integer_change_rejected(self):
        candidate, before, after = p.transform(self.original)
        p.verify_scope(self.original, candidate, before, after)
        for name in (b'fn fold_kernel_compact(', b'fn showdown_compact_exact('):
            changed = candidate.replace(name, name + b'/* changed */', 1)
            with self.assertRaises(ValueError):
                p.verify_scope(self.original, changed, before, after)

    def test_separate_copy_application_and_nonfresh_rejection(self):
        # Keep this small source-only copy and its application receipt as evidence.
        # tempfile's restrictive directory mode is incompatible with this Windows sandbox.
        temporary = HERE / ('source-application-' + uuid.uuid4().hex)
        temporary.mkdir()
        temporary = temporary.resolve()
        self.assertTrue(temporary.is_relative_to(HERE.resolve()))
        target = temporary / p.SOURCE
        target.parent.mkdir(parents=True)
        target.write_bytes(self.original)
        command = [sys.executable, '-X', 'utf8', '-B', str(HERE / 'prepare.py'), '--apply-to', str(temporary), '--receipt', str(temporary / 'applied.json')]
        result = subprocess.run(command, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertEqual(target.read_bytes(), (HERE / 'kernel.rs').read_bytes())
        receipt = json.loads((temporary / 'applied.json').read_bytes())
        self.assertEqual(receipt['before'], p.pin(self.original))
        self.assertEqual(receipt['after'], p.pin(target.read_bytes()))
        repeated = subprocess.run(command, capture_output=True, timeout=10)
        self.assertNotEqual(repeated.returncode, 0)
        self.assertEqual((p.ROOT / p.SOURCE).read_bytes(), self.original)

    def test_production_root_application_rejected(self):
        command = [sys.executable, '-X', 'utf8', '-B', str(HERE / 'prepare.py'), '--apply-to', str(p.ROOT), '--receipt', str(HERE / 'forbidden-production-application.json')]
        result = subprocess.run(command, capture_output=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((HERE / 'forbidden-production-application.json').exists())
        self.assertEqual((p.ROOT / p.SOURCE).read_bytes(), self.original)


if __name__ == '__main__':
    unittest.main()
