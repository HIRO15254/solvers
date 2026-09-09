import importlib.util
import sys
import unittest
from pathlib import Path
from unittest import mock


TOOLS = Path(__file__).resolve().parents[1]
MODULE_PATH = TOOLS / "gcp_efficiency_queue.py"
SPEC = importlib.util.spec_from_file_location("gcp_efficiency_queue", MODULE_PATH)
queue = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.path.insert(0, str(TOOLS))
sys.modules[SPEC.name] = queue
SPEC.loader.exec_module(queue)


class GcpEfficiencyQueueTests(unittest.TestCase):
    def test_shutdown_is_deferred_by_systemd_timer(self):
        run = mock.Mock()
        queue.schedule_guest_shutdown(run)
        run.assert_called_once_with([
            "systemd-run", "--unit=solvers-efficiency-shutdown", "--on-active=5m",
            "--collect", "--", "/sbin/shutdown", "-h", "now",
        ], check=True)


if __name__ == "__main__":
    unittest.main()
