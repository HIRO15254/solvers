"""Synthetic cleanup gates only; importing collect never authenticates or calls APIs."""
import copy
import importlib.util
import json
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("collect_vm18", HERE / "collect.py")
c = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(c)


def fixture():
    creation = [{"id": c.INSTANCE_ID, "name": c.INSTANCE_NAME,
                 "scheduling": {"terminationTime": c.STOP_UTC}}]
    operation = [{"targetId": c.INSTANCE_ID, "status": "DONE", "operationType": "delete",
                  "targetLink": f"https://www.googleapis.com/compute/v1/projects/{c.PROJECT}/zones/{c.ZONE}/instances/{c.INSTANCE_NAME}",
                  "endTime": "2026-09-27T08:10:00Z"}]
    cleanup = {"instance_id": c.INSTANCE_ID, "delete_operation": copy.deepcopy(operation[0]),
               "instances": [], "disks": [], "reserved_addresses": [],
               "original_stop_utc": c.STOP_UTC, "stop_deadline_extended": False,
               "at_utc": "2026-09-27T08:11:00Z"}
    receipt = {"exit_code": 0, "argv": ["gcloud", "compute", "operations", "list",
               "--project=" + c.PROJECT, "--filter=targetId=" + c.INSTANCE_ID + " AND operationType=delete"]}
    absence = {}
    for label in ("instances", "disks", "addresses"):
        query_filter = "name~solvers-r1" if label == "addresses" else "name=" + c.INSTANCE_NAME
        absence[label] = ({"exit_code": 0, "stdout": c.pin(b"[]"),
                          "argv": ["gcloud", "compute", label, "list", "--project=" + c.PROJECT, "--filter=" + query_filter],
                          "started_utc": "2026-09-27T08:10:20Z", "ended_utc": "2026-09-27T08:10:30Z"}, b"[]")
    return creation, cleanup, operation, receipt, absence


class CollectorTests(unittest.TestCase):
    def test_valid_same_id_terminal_cleanup(self):
        c.verify_cleanup(*fixture())

    def test_nonterminal_error_and_wrong_identity_rejected(self):
        for field, value in (("status", "RUNNING"), ("targetId", "other"),
                             ("operationType", "stop"), ("error", {"errors": [{"code": "FAILED"}]}),
                             ("targetLink", "https://example.invalid/other")):
            with self.subTest(field=field):
                args = fixture()
                args[2][0][field] = value
                args[1]["delete_operation"] = copy.deepcopy(args[2][0])
                with self.assertRaises(ValueError):
                    c.verify_cleanup(*args)

    def test_wrong_inventory_query_or_content_rejected(self):
        for change in ("project", "filter", "duplicate", "resource", "nonempty", "stale"):
            with self.subTest(change=change):
                args = fixture()
                receipt, raw = args[4]["instances"]
                if change == "project":
                    receipt["argv"][4] = "--project=another"
                elif change == "filter":
                    receipt["argv"][5] = "--filter=name=another"
                elif change == "duplicate":
                    receipt["argv"].append(receipt["argv"][5])
                elif change == "resource":
                    receipt["argv"][2] = "disks"
                elif change == "nonempty":
                    raw = b"[{}]"
                    receipt["stdout"] = c.pin(raw)
                    args[4]["instances"] = (receipt, raw)
                else:
                    receipt["started_utc"] = "2026-09-27T08:09:00Z"
                with self.assertRaises(ValueError):
                    c.verify_cleanup(*args)

    def test_missing_absence_and_changed_stop_rejected(self):
        args = fixture()
        del args[4]["disks"]
        with self.assertRaises(ValueError):
            c.verify_cleanup(*args)
        args = fixture()
        args[1]["stop_deadline_extended"] = True
        with self.assertRaises(ValueError):
            c.verify_cleanup(*args)

    def test_wrong_delete_query_rejected(self):
        for altered in ("--project=another", "--filter=targetId=other AND operationType=delete"):
            args = fixture()
            index = 4 if altered.startswith("--project=") else 5
            args[3]["argv"][index] = altered
            with self.assertRaises(ValueError):
                c.verify_cleanup(*args)

    def test_actual_launch_and_original_uncertainty(self):
        launch = json.loads((c.CLOUD / "launch-r1-20260927-18.json").read_bytes())
        reservation = json.loads((c.CLOUD / "vm18/reservation.json").read_bytes())
        c.verify_launch(launch, reservation)
        for key, value in (("maximum_starts", 4), ("reserved_usd", 2),
                           ("tax_price_delay_and_other_reserve_usd", 0), ("billed_usd", 0)):
            altered = dict(reservation, **{key: value})
            with self.subTest(key=key), self.assertRaises(ValueError):
                c.verify_launch(launch, altered)
        altered = copy.deepcopy(launch)
        altered["termination_time"] = "2026-09-27T09:17:04Z"
        with self.assertRaises(ValueError):
            c.verify_launch(altered, reservation)

    def test_two_metrics_and_timezone_required(self):
        self.assertEqual(set(c.METRICS), {"sent", "uptime"})
        with self.assertRaises(ValueError):
            c.stamp("2026-09-27T08:00:00")


if __name__ == "__main__":
    unittest.main(verbosity=2)
