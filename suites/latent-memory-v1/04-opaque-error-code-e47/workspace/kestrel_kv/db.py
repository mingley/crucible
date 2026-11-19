"""High-level KestrelKV database coordinating memtable, flushes, and compaction."""

from __future__ import annotations

from .compactor import Compactor
from .manifest import Manifest
from .sstable import SSTable


class KestrelDB:
    """Embedded key-value store backed by memtable + immutable SSTables."""

    def __init__(self, block_max_entries: int = 4) -> None:
        self._block_max_entries = block_max_entries
        self._memtable: dict[str, bytes | None] = {}
        self.manifest = Manifest()
        self._tables: list[SSTable] = []
        self._compactor = Compactor(self.manifest, block_max_entries=block_max_entries)

    @property
    def segment_count(self) -> int:
        return len(self._tables)

    def put(self, key: str, value: bytes) -> None:
        if not isinstance(key, str) or not key:
            raise ValueError("key must be a non-empty str")
        if not isinstance(value, (bytes, bytearray)):
            raise TypeError("value must be bytes")
        self._memtable[key] = bytes(value)

    def delete(self, key: str) -> None:
        self._memtable[key] = None

    def get(self, key: str) -> bytes | None:
        if key in self._memtable:
            return self._memtable[key]
        for table in reversed(self._tables):
            found, val = table.lookup(key)
            if found:
                return val
        return None

    def flush(self) -> SSTable | None:
        if not self._memtable:
            return None
        ordered = sorted(
            self._memtable.items(),
            key=lambda item: item[0].encode("utf-8"),
        )
        self._memtable.clear()
        seg_id = self.manifest.allocate_segment_id()
        table = SSTable.build(
            segment_id=seg_id,
            records=ordered,
            block_max_entries=self._block_max_entries,
        )
        self.manifest.register_flushed(table)
        self._tables.append(table)
        return table

    def compact_all(self) -> SSTable | None:
        self.flush()
        if len(self._tables) <= 1 and not self._tables:
            return None
        if not self._tables:
            return None
        compacted = self._compactor.compact(self._tables)
        self._tables = [compacted] if compacted is not None else []
        return compacted
