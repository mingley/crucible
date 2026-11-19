"""Answer-isolated verifier tests for MVCCStore scan vs GC snapshot isolation."""

from __future__ import annotations

from contextlib import closing
import pathlib
import sys
import threading
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from mvcc_store import MVCCEngine  # noqa: E402


class MVCCRetractionVerifierTests(unittest.TestCase):
    def test_mid_scan_gc_preserves_exact_snapshot_and_blocks_dirty_reads(self) -> None:
        engine = MVCCEngine(num_shards=4)
        self.assertFalse(engine.allow_unpinned_gc)

        initial = {f"row/{i:02d}": f"v1-{i:02d}".encode("utf-8") for i in range(8)}
        for k, v in initial.items():
            engine.put(k, v)

        snap_ts = engine.clock.current_ts()
        scan_iter = engine.scan(prefix="row/", read_ts=snap_ts)
        first_item = next(scan_iter)
        self.assertEqual(first_item, ("row/00", b"v1-00"))

        # Concurrent writer overwrites remaining keys, deletes others, and inserts new ones,
        # then triggers garbage collection while scan_iter is paused mid-flight!
        for i in range(1, 5):
            engine.put(f"row/{i:02d}", f"v2-mutated-{i:02d}".encode("utf-8"))
        for i in range(5, 8):
            engine.delete(f"row/{i:02d}")
        engine.put("row/99", b"phantom-post-snapshot")

        engine.run_gc()

        observed = dict([first_item] + list(scan_iter))
        # Must equal exact v1 snapshot — neither missing deleted rows nor leaking v2 writes!
        self.assertEqual(observed, initial)

        # Once scan completes, watermark lease must be released so GC can reclaim old history.
        self.assertEqual(engine.watermarks.active_count(), 0)
        reclaimed_after = engine.run_gc()
        self.assertGreaterEqual(reclaimed_after, 7)

    def test_early_scan_close_releases_watermark_in_finally(self) -> None:
        engine = MVCCEngine(num_shards=4)
        for i in range(6):
            engine.put(f"item/{i}", b"old")

        with closing(engine.scan(prefix="item/")) as it:
            _ = next(it)
            self.assertGreaterEqual(engine.watermarks.active_count(), 1)

        self.assertEqual(engine.watermarks.active_count(), 0)
        for i in range(6):
            engine.put(f"item/{i}", b"new")
        self.assertEqual(engine.run_gc(), 6)

    def test_concurrent_threaded_scans_and_gc(self) -> None:
        engine = MVCCEngine(num_shards=4)
        expected = {f"k/{i:02d}": f"base-{i}".encode("utf-8") for i in range(12)}
        for k, v in expected.items():
            engine.put(k, v)
        pinned_ts = engine.clock.current_ts()

        ready = threading.Event()
        proceed = threading.Event()
        results: list[dict[str, bytes]] = []
        errors: list[BaseException] = []

        def reader() -> None:
            try:
                it = engine.scan(prefix="k/", read_ts=pinned_ts)
                first = next(it)
                ready.set()
                proceed.wait(timeout=5.0)
                results.append(dict([first] + list(it)))
            except BaseException as exc:  # noqa: BLE001
                errors.append(exc)

        t = threading.Thread(target=reader)
        t.start()
        self.assertTrue(ready.wait(timeout=5.0))

        for i in range(12):
            if i % 2 == 0:
                engine.put(f"k/{i:02d}", b"overwritten")
            else:
                engine.delete(f"k/{i:02d}")
        engine.run_gc()
        proceed.set()
        t.join(timeout=5.0)

        self.assertEqual(errors, [])
        self.assertEqual(results, [expected])
        self.assertEqual(engine.watermarks.active_count(), 0)


if __name__ == "__main__":
    unittest.main()
