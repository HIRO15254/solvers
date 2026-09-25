import importlib.util
import shutil
import unittest
import uuid
from pathlib import Path


MODULE = Path(__file__).parents[1] / "check_docs.py"
SPEC = importlib.util.spec_from_file_location("check_docs", MODULE)
docs = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(docs)
TEST_TEMP_ROOT = Path(__file__).parents[2] / ".cache/tool-tests"


class DocumentationTests(unittest.TestCase):
    def setUp(self):
        TEST_TEMP_ROOT.mkdir(parents=True, exist_ok=True)
        self.root = TEST_TEMP_ROOT / f"docs-{uuid.uuid4().hex}"
        self.root.mkdir()
        self.addCleanup(self.cleanup_fixture)
        for name in docs.REQUIRED_FILES:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("", encoding="utf-8")

    def cleanup_fixture(self):
        self.assertTrue(self.root.resolve().is_relative_to(TEST_TEMP_ROOT.resolve()))
        shutil.rmtree(self.root)

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def test_reports_missing_link_with_source_line(self):
        self.write("docs/README.md", "# Docs\n\n[missing](absent.md#section)\n")
        self.assertEqual(docs.check(self.root), [
            "docs/README.md:3: missing local link target: absent.md#section"
        ])

    def test_required_entrypoints_cannot_disappear(self):
        (self.root / "docs/status.jp.md").unlink()
        self.assertEqual(docs.check(self.root), [
            "docs/status.jp.md: missing required documentation entrypoint"
        ])

    def test_local_inline_image_and_reference_destinations(self):
        self.write("docs/file (draft).md", "")
        self.write("docs/image.svg", "")
        self.write("docs/README.md", "\n".join([
            '[angle](<file (draft).md> "a title")',
            "[encoded](file%20%28draft%29.md#ignored)",
            "[balanced](file(draft).md)",
            "![image](image.svg)",
            "[reference][guide]",
            '[guide]: ../AGENTS.md "Guide"',
            "[root](/docs/status.jp.md)",
        ]))
        self.write("docs/file(draft).md", "")
        self.assertEqual(docs.check(self.root), [])

    def test_missing_reference_and_image_destinations_are_checked(self):
        self.write("docs/README.md", "[guide]: missing.md\n![image](missing.svg)")
        errors = docs.check(self.root)
        self.assertEqual(len(errors), 2)
        self.assertIn("missing.md", errors[0])
        self.assertIn("missing.svg", errors[1])

    def test_fenced_inline_code_remote_urls_and_anchors_are_ignored(self):
        self.write("docs/README.md", "\n".join([
            "````markdown", "[example](absent.md)", "```", "[still code](absent.md)", "````",
            "~~~md", "[example](absent.md)", "~~~",
            "`[example](absent.md)`", "[web](https://example.org/path)",
            "[mail](mailto:dev@example.org)", "[section](#missing-anchor)",
        ]))
        self.assertEqual(docs.check(self.root), [])

    def test_moved_plan_paths_are_rejected_in_current_prose(self):
        self.write("docs/development.md", "Use `docs/research/r0-execution-plan.jp.md`.")
        errors = docs.check(self.root)
        self.assertEqual(len(errors), 1)
        self.assertIn("use docs/plans/r0-execution-plan.jp.md", errors[0])

    def test_historical_prose_is_allowed_but_its_broken_links_are_not(self):
        self.write("docs/research/old-survey.md", "\n".join([
            "Previously `docs/research/r0-execution-plan.jp.md`.",
            "[missing](missing.md)",
        ]))
        self.assertEqual(docs.check(self.root), [
            "docs/research/old-survey.md:2: missing local link target: missing.md"
        ])

    def test_links_into_experiments_are_checked_without_scanning_history(self):
        self.write("experiments/old/README.md", "[obsolete](missing.md)")
        self.write("docs/README.md", "[evidence](../experiments/old/README.md)")
        self.assertEqual(docs.check(self.root), [])
        (self.root / "experiments/old/README.md").unlink()
        self.assertEqual(len(docs.check(self.root)), 1)


if __name__ == "__main__":
    unittest.main()
