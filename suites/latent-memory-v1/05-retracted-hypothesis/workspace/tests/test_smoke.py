"""Smoke tests for MVCCStore transactions, conflict detection, and idle GC."""

from __future__ import annotations

import unittest

from mvcc_store import MVCCEngine, WriteConflictError


class MVCCSmokeTests(unittest.TestCase):
    def test_snapshot_read_and_write_conflict(self) -> None:
        engine = MVCCEngine(num_shards=4)
        engine.put("acct/1", b"100")
        engine.put("acct/2", b"200")

        tx1 = engine.begin()
        tx2 = engine.begin()

        tx1.put("acct/1", b"150")
        tx1.commit()

        # tx2 still sees snapshot value b"100", but conflicting write fails
        self.assertEqual(tx2.get("acct/1"), b"100")
        tx2.put("acct/1", b"999")
        with self.assertRaises(WriteConflictError):
            tx2.commit()

    def test_idle_gc_reclaims_superseded_versions(self) -> None:
        engine = MVCCEngine(num_shards=2)
        engine.put("k1", b"v1")
        engine.put("k1", b"v2")
        engine.put("k1", b"v3")
        self.assertEqual(engine.total_versions(), 3)
        reclaimed = engine.run_gc()
        self.assertEqual(reclaimed, 2)
        self.assertEqual(list(engine.scan()), [("k1", b"v3")])


if __name__ == "__main__":
    unittest.main()
