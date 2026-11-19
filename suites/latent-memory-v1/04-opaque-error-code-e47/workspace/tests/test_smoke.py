"""Baseline smoke tests for KestrelKV ASCII keys and compaction."""

from __future__ import annotations

import unittest

from kestrel_kv import KestrelDB, shortest_separator


class KestrelSmokeTests(unittest.TestCase):
    def test_ascii_separator_shortening(self) -> None:
        self.assertEqual(shortest_separator("item/100/alpha", "item/300/beta"), "item/2")
        self.assertEqual(shortest_separator("item/100", "item/1000"), "item/100")

    def test_ascii_flush_and_compaction(self) -> None:
        db = KestrelDB(block_max_entries=2)
        for idx in range(6):
            db.put(f"tenant/acct_{idx:02d}", f"v1-{idx}".encode("utf-8"))
        db.flush()

        db.put("tenant/acct_02", b"v2-updated")
        db.delete("tenant/acct_04")
        db.put("tenant/acct_09", b"v1-new")
        db.flush()

        db.compact_all()
        self.assertEqual(db.segment_count, 1)
        self.assertEqual(db.get("tenant/acct_00"), b"v1-0")
        self.assertEqual(db.get("tenant/acct_02"), b"v2-updated")
        self.assertIsNone(db.get("tenant/acct_04"))
        self.assertEqual(db.get("tenant/acct_09"), b"v1-new")


if __name__ == "__main__":
    unittest.main()
