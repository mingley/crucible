"""Answer-isolated verifier tests for KestrelKV E47_MANIFEST_DRIFT diagnosis."""

from __future__ import annotations

import pathlib
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from kestrel_kv import (  # noqa: E402
    E47_MANIFEST_DRIFT,
    IndexEntry,
    KestrelDB,
    KestrelError,
    Manifest,
    SSTable,
    shortest_separator,
)


class KestrelE47VerifierTests(unittest.TestCase):
    def test_multibyte_shortest_separator_utf8_invariants(self) -> None:
        pairs = [
            ("äé/record_1", "æa/record_1"),
            ("αβ/key_99", "γδ/key_01"),
            ("日本/999", "月曜/001"),
            ("café/item/100/x", "café/item/300/y"),
            ("😀🔥/user_9", "😃a/user_1"),
            ("prefix\ud7ff/tail", "prefix\ue002/tail"),
        ]
        for left, right in pairs:
            sep = shortest_separator(left, right)
            # Must encode cleanly as UTF-8 (no lone surrogates) and satisfy fence bounds
            left_b = left.encode("utf-8")
            sep_b = sep.encode("utf-8")
            right_b = right.encode("utf-8")
            self.assertLessEqual(left_b, sep_b, msg=f"sep < left for ({left!r}, {right!r}): {sep!r}")
            self.assertLess(sep_b, right_b, msg=f"sep >= right for ({left!r}, {right!r}): {sep!r}")
            self.assertLess(len(sep), len(left), msg=f"separator did not shorten ({left!r}, {right!r}): {sep!r}")

    def test_multibyte_compaction_and_point_lookups_succeed(self) -> None:
        db = KestrelDB(block_max_entries=2)
        seg1 = {
            "äα/01": b"lat-1",
            "äé/02": b"lat-2",
            "æa/03": b"lat-3",
            "αβ/10": b"grk-1",
            "日本/10": b"cjk-1",
            "日本/99": b"cjk-2",
        }
        for k, v in seg1.items():
            db.put(k, v)
        db.flush()

        seg2 = {
            "γδ/01": b"grk-2",
            "月曜/01": b"cjk-3",
            "😀🔥/01": b"emo-1",
            "😃✨/02": b"emo-2",
            "日本/10": b"cjk-1-updated",
        }
        for k, v in seg2.items():
            db.put(k, v)
        db.delete("äα/01")
        db.flush()

        compacted = db.compact_all()
        self.assertIsNotNone(compacted)
        self.assertEqual(db.segment_count, 1)

        expected = dict(seg1)
        expected.update(seg2)
        del expected["äα/01"]

        self.assertIsNone(db.get("äα/01"))
        for key, val in expected.items():
            self.assertEqual(db.get(key), val, msg=f"lookup mismatch for {key!r}")

    def test_manifest_validation_still_rejects_corrupted_fences(self) -> None:
        table = SSTable.build(
            segment_id=77,
            records=[("k1", b"1"), ("k2", b"2"), ("k5", b"5"), ("k6", b"6")],
            block_max_entries=2,
        )
        # Deliberately corrupt block 0 fence_key below max_key to ensure check wasn't weakened.
        bad_entry = IndexEntry(
            fence_key="k0",
            block_offset=table.index[0].block_offset,
            block_len=table.index[0].block_len,
            min_key=table.index[0].min_key,
            max_key=table.index[0].max_key,
        )
        table.index[0] = bad_entry
        with self.assertRaises(KestrelError) as ctx:
            Manifest.validate_segment_fences(table)
        self.assertEqual(ctx.exception.code, E47_MANIFEST_DRIFT)


if __name__ == "__main__":
    unittest.main()
