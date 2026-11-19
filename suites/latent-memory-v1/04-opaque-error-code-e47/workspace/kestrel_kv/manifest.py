"""Versioned segment manifest and compaction fence integrity checks."""

from __future__ import annotations

from dataclasses import dataclass

from .errors import E47_MANIFEST_DRIFT, KestrelError
from .sstable import SSTable


@dataclass(frozen=True)
class SegmentMeta:
    segment_id: int
    min_key: str
    max_key: str
    block_count: int


class Manifest:
    """Tracks live SSTable segments and validates structural fence invariants."""

    def __init__(self) -> None:
        self._next_id = 1
        self._segments: list[SegmentMeta] = []

    def allocate_segment_id(self) -> int:
        seg_id = self._next_id
        self._next_id += 1
        return seg_id

    @property
    def segments(self) -> list[SegmentMeta]:
        return list(self._segments)

    @staticmethod
    def validate_segment_fences(table: SSTable) -> None:
        """Verify UTF-8 lexicographical fence bounds across all blocks in ``table``."""
        if not table.index:
            raise KestrelError(E47_MANIFEST_DRIFT, "empty SSTable index")
        for idx, entry in enumerate(table.index):
            min_b = entry.min_key.encode("utf-8")
            max_b = entry.max_key.encode("utf-8")
            fence_b = entry.fence_key.encode("utf-8")
            if min_b > max_b:
                raise KestrelError(E47_MANIFEST_DRIFT, "inverted block min/max keys")
            if fence_b < max_b:
                raise KestrelError(
                    E47_MANIFEST_DRIFT,
                    f"segment fence invariant violated at block {idx}: fence < block_max_key",
                )
            if idx + 1 < len(table.index):
                next_min_b = table.index[idx + 1].min_key.encode("utf-8")
                if not (fence_b < next_min_b):
                    raise KestrelError(
                        E47_MANIFEST_DRIFT,
                        f"segment fence invariant violated at block {idx}: fence >= next_block_min_key",
                    )

    def register_flushed(self, table: SSTable) -> SegmentMeta:
        meta = SegmentMeta(
            segment_id=table.segment_id,
            min_key=table.min_key,
            max_key=table.max_key,
            block_count=len(table.index),
        )
        self._segments.append(meta)
        return meta

    def commit_compaction(self, obsolete_ids: set[int], merged: SSTable | None) -> None:
        if merged is not None:
            self.validate_segment_fences(merged)
        remaining = [m for m in self._segments if m.segment_id not in obsolete_ids]
        if merged is not None:
            remaining.append(
                SegmentMeta(
                    segment_id=merged.segment_id,
                    min_key=merged.min_key,
                    max_key=merged.max_key,
                    block_count=len(merged.index),
                )
            )
        self._segments = remaining
