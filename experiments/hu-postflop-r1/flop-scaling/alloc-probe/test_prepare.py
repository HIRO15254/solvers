"""Lightweight transformation and SeqCst gate-model checks; no native execution."""
from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("allocation_prepare", HERE / "prepare.py")
prepare = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(prepare)


def stale_entry_model(*, compare_epoch: bool) -> tuple[int, int]:
    """Enumerate one delayed allocator and one serialized stop/reset/start.

    Each transition is an atomic step in a single SeqCst total order. A System
    call has no counter side effects; its arbitrary delay is represented by
    delaying the write transition. This is a model, not a Rust race execution.
    """
    completed = violations = 0

    def walk(w: int, c: int, epoch: int, inflight: int, seen: int, counted: bool, drained: bool) -> None:
        nonlocal completed, violations
        if w == 5 and c == 4:
            completed += 1
            return
        if w < 5:
            if w == 0:
                walk(1 if epoch & 1 else 5, c, epoch, inflight, epoch, False, drained)
            elif w == 1:
                walk(2, c, epoch, inflight + 1, seen, False, drained)
            elif w == 2:
                admitted = epoch == seen if compare_epoch else bool(epoch & 1)
                walk(3 if admitted else 5, c, epoch, inflight if admitted else inflight - 1, seen, admitted, drained)
            elif w == 3:
                if counted and seen == 1 and drained:
                    violations += 1
                else:
                    walk(4, c, epoch, inflight, seen, counted, drained)
            elif w == 4:
                walk(5, c, epoch, inflight - 1, seen, counted, drained)
        if c < 4:
            if c == 0:
                walk(w, 1, 2, inflight, seen, counted, drained)
            elif c == 1 and inflight == 0:
                walk(w, 2, epoch, inflight, seen, counted, True)
            elif c == 2:
                walk(w, 3, epoch, inflight, seen, counted, drained)
            elif c == 3:
                walk(w, 4, 3, inflight, seen, counted, drained)

    walk(0, 0, 1, 0, 0, False, False)
    return completed, violations


class TransformTests(unittest.TestCase):
    def setUp(self) -> None:
        self.base = prepare.BASE.read_bytes()
        self.allocator = (HERE / "allocator.rs.in").read_bytes()

    def test_exact_generated_files_and_manifest(self) -> None:
        generated, provenance = prepare.expected()
        for name, raw in generated.items():
            self.assertEqual((HERE / name).read_bytes(), raw)
            self.assertEqual(provenance["generated"][name], prepare.pin(raw))

    def test_only_declared_two_transformations(self) -> None:
        derived = prepare.transform(self.base, self.allocator)
        nl = b"\r\n" if b"\r\n" in self.base else b"\n"
        inserted = (self.allocator.replace(b"\r\n", b"\n").rstrip(b"\n") + b"\n\n").replace(b"\n", nl)
        self.assertEqual(derived.count(inserted), 1)
        reversed_bytes = derived.replace(inserted, b"", 1).replace(
            prepare.NEW_EVENT.replace(b"\n", nl), prepare.OLD_EVENT.replace(b"\n", nl), 1
        )
        self.assertEqual(reversed_bytes, self.base)

    def test_changed_base_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "exact pin"):
            prepare.transform(self.base.replace(b"1..=2", b"1..=3", 1), self.allocator)

    def test_appended_base_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "exact pin"):
            prepare.transform(self.base + b"\n", self.allocator)

    def test_already_instrumented_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "exact pin"):
            prepare.transform(prepare.transform(self.base, self.allocator), self.allocator)

    def test_event_io_outside_gate(self) -> None:
        event = prepare.NEW_EVENT.decode()
        self.assertLess(event.index("allocation_probe::stop()"), event.index("allocation_probe::emit"))
        self.assertLess(event.index("allocation_probe::emit"), event.index("println!"))
        self.assertLess(event.index("flush()"), event.index("allocation_probe::start()"))
        self.assertIn("if let Some(counts)", event)

    def test_selftest_uses_identical_allocator(self) -> None:
        generated, _ = prepare.expected()
        self.assertTrue(generated["selftest.rs"].startswith(self.allocator.rstrip(b"\r\n") + b"\n\n"))
        self.assertIn(b"[1, 64, 0, 1, 32, 0, 1, 128, 64, 0, 2, 160]", generated["selftest.rs"])

    def test_epoch_model_rejects_stale_entry_after_restart(self) -> None:
        completed, violations = stale_entry_model(compare_epoch=True)
        self.assertEqual(completed, 15)
        self.assertEqual(violations, 0)

    def test_bool_only_gate_negative_control_detects_race(self) -> None:
        _, violations = stale_entry_model(compare_epoch=False)
        self.assertGreater(violations, 0)


if __name__ == "__main__":
    unittest.main(verbosity=2)
