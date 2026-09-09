import importlib.util
import json
import shutil
import sys
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
MODULE_PATH = ROOT / "runs/gcp-convergence-20260909-control/export-fix-state4/build_and_export.py"
SPEC = importlib.util.spec_from_file_location("export_fix_controller", MODULE_PATH)
controller = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = controller
SPEC.loader.exec_module(controller)


class ExportFixControllerTests(unittest.TestCase):
    def test_staged_controller_imports_with_shallow_path(self):
        # A remote /home/experiment script has fewer parents than its checkout
        # path. Imported helpers are supplied through PYTHONPATH on the guest.
        namespace = {
            "__name__": "staged_export_controller",
            "__file__": str(Path(ROOT.anchor) / "export.py"),
        }
        exec(compile(MODULE_PATH.read_text(encoding="utf-8"), str(MODULE_PATH), "exec"), namespace)
        self.assertEqual(namespace["EXPECTED_CHECKPOINT_SWEEPS"], 262144)

    def test_accepts_actual_rust_camel_case_export_fields(self):
        path = ROOT / "tools/.export-fix-controller-test.json"
        value = {
            "schemaVersion": "solvers.multiway-checkpoint-export-bench/v1",
            "sweeps": 262144,
            "solverStateVersion": 4,
            "strategyBlocks": 12297431,
            "metadataVerified": True,
        }
        try:
            path.write_text(json.dumps(value), encoding="utf-8")
            self.assertEqual(controller.validate_export(path), value)
            value["strategy_blocks"] = value.pop("strategyBlocks")
            path.write_text(json.dumps(value), encoding="utf-8")
            with self.assertRaises(controller.ControllerError):
                controller.validate_export(path)
        finally:
            if path.exists():
                path.unlink()

    def test_build_is_bound_to_extracted_workspace(self):
        source = Path("/opt/solvers-experiment/export-fix-state4/source")
        command = controller.build_command(Path("/root/.cargo/bin/cargo"), source)
        self.assertIn("--manifest-path", command)
        self.assertIn(str(source / "Cargo.toml"), command)
        self.assertIn("--target-dir", command)
        self.assertIn(str(source / "target"), command)
        self.assertEqual(command[-2:], ["--example", "mw_checkpoint_audit"])

    def test_frozen_archive_members_match_manifest(self):
        archive = ROOT / "runs/gcp-convergence-20260909-control/export-fix-state4/source.tgz"
        manifest = ROOT / "runs/gcp-convergence-20260909-control/export-fix-state4/source.manifest.json"
        destination = ROOT / "tools/.export-fix-controller-extract"
        destination.resolve().relative_to((ROOT / "tools").resolve())
        shutil.rmtree(destination, ignore_errors=True)
        try:
            parsed = controller.verify_and_extract(
                archive, manifest, controller.EXPECTED_ARCHIVE_SHA256, destination
            )
            self.assertEqual(parsed["file_count"], 171)
            self.assertTrue((destination / "Cargo.toml").is_file())
            self.assertTrue((destination / "source.manifest.json").is_file())
        finally:
            shutil.rmtree(destination, ignore_errors=True)


if __name__ == "__main__":
    unittest.main()
