"""Immutable sorted-string table (SSTable) with fenced block index and Bloom filter."""

from __future__ import annotations

from bisect import bisect_left
from dataclasses import dataclass
from typing import Sequence

from .block import DataBlockBuilder, DataBlockReader, shortest_separator
from .bloom import BloomFilter
from .errors import E31_SSTABLE_ORDER, KestrelError


@dataclass(frozen=True)
class IndexEntry:
    fence_key: str
    block_offset: int
    block_len: int
    min_key: str
    max_key: str


class SSTable:
    """Sorted immutable table partitioned into fixed-entry data blocks."""

    def __init__(
        self,
        segment_id: int,
        data_blob: bytes,
        index: list[IndexEntry],
        bloom: BloomFilter,
    ) -> None:
        self.segment_id = segment_id
        self._blob = data_blob
        self.index = index
        self.bloom = bloom

    @property
    def min_key(self) -> str:
        return self.index[0].min_key

    @property
    def max_key(self) -> str:
        return self.index[-1].max_key

    @classmethod
    def build(
        cls,
        segment_id: int,
        records: Sequence[tuple[str, bytes | None]],
        block_max_entries: int = 4,
    ) -> SSTable:
        if not records:
            raise ValueError("cannot build empty SSTable")
        if block_max_entries <= 0:
            raise ValueError("block_max_entries must be positive")

        bloom = BloomFilter()
        raw_blocks: list[tuple[str, str, bytes]] = []
        builder = DataBlockBuilder()
        prev_key: bytes | None = None

        for key, val in records:
            kb = key.encode("utf-8")
            if prev_key is not None and kb <= prev_key:
                raise KestrelError(E31_SSTABLE_ORDER, "SSTable records must be strictly sorted")
            prev_key = kb
            bloom.add(key)
            builder.add(key, val)
            if builder.count >= block_max_entries:
                raw_blocks.append((builder.min_key, builder.max_key, builder.finish()))
                builder = DataBlockBuilder()

        if builder.count > 0:
            raw_blocks.append((builder.min_key, builder.max_key, builder.finish()))

        blob = bytearray()
        index: list[IndexEntry] = []
        for idx, (b_min, b_max, chunk) in enumerate(raw_blocks):
            if idx + 1 < len(raw_blocks):
                next_min = raw_blocks[idx + 1][0]
                fence = shortest_separator(b_max, next_min)
            else:
                fence = b_max
            offset = len(blob)
            blob.extend(chunk)
            index.append(
                IndexEntry(
                    fence_key=fence,
                    block_offset=offset,
                    block_len=len(chunk),
                    min_key=b_min,
                    max_key=b_max,
                )
            )
        return cls(segment_id=segment_id, data_blob=bytes(blob), index=index, bloom=bloom)

    def lookup(self, key: str) -> tuple[bool, bytes | None]:
        """Return ``(found, value_or_tombstone)`` using Bloom + fence binary search."""
        if not self.bloom.might_contain(key):
            return False, None
        target = key.encode("utf-8")
        fences = [entry.fence_key.encode("utf-8") for entry in self.index]
        pos = bisect_left(fences, target)
        if pos >= len(self.index):
            return False, None
        entry = self.index[pos]
        block_bytes = self._blob[entry.block_offset : entry.block_offset + entry.block_len]
        for rec_key, rec_val in DataBlockReader(block_bytes).iter_records():
            if rec_key == key:
                return True, rec_val
        return False, None

    def iter_all(self) -> list[tuple[str, bytes | None]]:
        out: list[tuple[str, bytes | None]] = []
        for entry in self.index:
            chunk = self._blob[entry.block_offset : entry.block_offset + entry.block_len]
            out.extend(DataBlockReader(chunk).iter_records())
        return out
