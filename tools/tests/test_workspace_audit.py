import importlib.util
import shutil
import unittest
import uuid
from contextlib import contextmanager
from pathlib import Path
from unittest import mock


MODULE = Path(__file__).parents[1] / "workspace_audit.py"
SPEC = importlib.util.spec_from_file_location("workspace_audit", MODULE)
audit = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(audit)
TEST_TEMP_ROOT = Path(__file__).parents[2] / "target/tool-tests"


@contextmanager
def workspace_fixture():
    TEST_TEMP_ROOT.mkdir(parents=True, exist_ok=True)
    root = TEST_TEMP_ROOT / f"workspace-audit-{uuid.uuid4().hex}"
    root.mkdir()
    try:
        yield root
    finally:
        shutil.rmtree(root, ignore_errors=True)


class WorkspaceAuditTests(unittest.TestCase):
    def test_tree_size_counts_files_and_bytes(self):
        with workspace_fixture() as root:
            (root / "nested").mkdir()
            (root / "a.txt").write_bytes(b"abc")
            (root / "nested/b.txt").write_bytes(b"12345")
            self.assertEqual(audit.tree_size(root), (2, 8, 0))

    def test_collect_classifies_root_temp_directories(self):
        with workspace_fixture() as root:
            (root / "tmp-generated").mkdir()
            (root / "docs").mkdir()
            with mock.patch.object(audit, "git_summary", return_value={"branch": "main"}):
                result = audit.collect(root)
            self.assertEqual(result["schema"], "solvers.workspace-audit/v1")
            self.assertEqual(result["root_temp_directories"], ["tmp-generated"])
            self.assertEqual(result["sizes"]["docs"]["bytes"], 0)


if __name__ == "__main__":
    unittest.main()
