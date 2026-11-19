"""Core sharded MVCC engine supporting transactions, range scans, and GC."""

from __future__ import annotations

import hashlib
from typing import Iterator

from .clock import TimestampOracle, WatermarkTracker
from .gc import GarbageCollector
from .lock_table import KeyLockTable, WriteConflictError
from .segment import VersionedSegment
from .tx import Transaction


class _PinnedScanIterator(Iterator[tuple[str, bytes]]):
    """Wraps a scan generator so the watermark lease is held immediately and always released."""

    def __init__(
        self,
        watermarks: WatermarkTracker,
        lease_id: int,
        inner: Iterator[tuple[str, bytes]],
    ) -> None:
        self._watermarks = watermarks
        self._lease_id = lease_id
        self._inner = inner
        self._closed = False

    def __iter__(self) -> _PinnedScanIterator:
        return self

    def __next__(self) -> tuple[str, bytes]:
        if self._closed:
            raise StopIteration
        try:
            return next(self._inner)
        except BaseException:
            self.close()
            raise

    def close(self) -> None:
        if not self._closed:
            self._closed = True
            try:
                close_fn = getattr(self._inner, "close", None)
                if callable(close_fn):
                    close_fn()
            finally:
                self._watermarks.release(self._lease_id)

    def __del__(self) -> None:
        self.close()


class MVCCEngine:
    """Coordinates sharded version chains, snapshot reads, commits, and GC."""

    allow_unpinned_gc: bool = False

    def __init__(self, num_shards: int = 4, allow_unpinned_gc: bool = False) -> None:
        if num_shards <= 0:
            raise ValueError("num_shards must be positive")
        self.allow_unpinned_gc = allow_unpinned_gc
        self.clock = TimestampOracle()
        self.watermarks = WatermarkTracker()
        self._locks = KeyLockTable()
        self._segments = [VersionedSegment(i) for i in range(num_shards)]
        self.gc = GarbageCollector(self.clock, self.watermarks)
        self._head_cache: dict[str, bytes] = {}

    def _shard_for(self, key: str) -> VersionedSegment:
        digest = hashlib.md5(key.encode("utf-8")).digest()
        idx = int.from_bytes(digest[:4], "big") % len(self._segments)
        return self._segments[idx]

    def begin(self) -> Transaction:
        return Transaction(self, read_ts=self.clock.current_ts())

    def put(self, key: str, value: bytes) -> int:
        tx = self.begin()
        tx.put(key, value)
        return tx.commit()

    def delete(self, key: str) -> int:
        tx = self.begin()
        tx.delete(key)
        return tx.commit()

    def commit_writes(self, read_ts: int, writes: dict[str, bytes | None]) -> int:
        if not writes:
            return read_ts
        locked_keys = self._locks.acquire_many(writes.keys())
        try:
            for key in locked_keys:
                if self._shard_for(key).latest_version_after_ts(key, read_ts):
                    raise WriteConflictError(f"write-write conflict on {key!r}")
            commit_ts = self.clock.next_ts()
            for key in locked_keys:
                val = writes[key]
                self._shard_for(key).append_version(key, commit_ts, val)
                if val is None:
                    self._head_cache.pop(key, None)
                else:
                    self._head_cache[key] = val
            return commit_ts
        finally:
            self._locks.release_many(locked_keys)

    def get(self, key: str, read_ts: int | None = None) -> bytes | None:
        effective_ts = self.clock.current_ts() if read_ts is None else read_ts
        lease_id = self.watermarks.register(effective_ts)
        try:
            found, val = self._shard_for(key).read_at(key, effective_ts)
            return val if found else None
        finally:
            self.watermarks.release(lease_id)

    def _acquire_segment_snapshot(self) -> list[tuple[VersionedSegment, list[str]]]:
        return [(seg, seg.keys_snapshot()) for seg in self._segments]

    def scan(
        self,
        prefix: str = "",
        read_ts: int | None = None,
    ) -> Iterator[tuple[str, bytes]]:
        """Yield sorted ``(key, value)`` pairs visible at ``read_ts``."""
        effective_ts = self.clock.current_ts() if read_ts is None else read_ts
        lease_id = self.watermarks.register(effective_ts)
        try:
            segment_snapshot = self._acquire_segment_snapshot()
            gen = self._scan_segments(segment_snapshot, prefix, effective_ts, lease_id)
            return _PinnedScanIterator(self.watermarks, lease_id, gen)
        except BaseException:
            self.watermarks.release(lease_id)
            raise

    def _scan_segments(
        self,
        segment_snapshot: list[tuple[VersionedSegment, list[str]]],
        prefix: str,
        effective_ts: int,
        lease_id: int,
    ) -> Iterator[tuple[str, bytes]]:
        try:
            candidates: list[tuple[str, VersionedSegment]] = []
            for seg, keys in segment_snapshot:
                for key in keys:
                    if key.startswith(prefix):
                        candidates.append((key, seg))
            candidates.sort(key=lambda item: item[0])

            for key, seg in candidates:
                found, val = seg.read_at(key, effective_ts)
                if found and val is not None:
                    yield key, val
        finally:
            self.watermarks.release(lease_id)

    def run_gc(self) -> int:
        return self.gc.collect(self._segments)

    def total_versions(self) -> int:
        return sum(seg.version_count() for seg in self._segments)
