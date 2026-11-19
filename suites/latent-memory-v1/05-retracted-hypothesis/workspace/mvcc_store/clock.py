"""Monotonic commit-timestamp oracle and active reader watermark registry."""

from __future__ import annotations

import threading


class TimestampOracle:
    """Issues strictly increasing integer timestamps for MVCC snapshots and commits."""

    def __init__(self, initial: int = 1) -> None:
        self._lock = threading.Lock()
        self._ts = initial

    def current_ts(self) -> int:
        with self._lock:
            return self._ts

    def next_ts(self) -> int:
        with self._lock:
            self._ts += 1
            return self._ts


class WatermarkTracker:
    """Tracks active snapshot reader timestamps so GC preserves pinned history."""

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._next_lease = 1
        self._active: dict[int, int] = {}

    def register(self, read_ts: int) -> int:
        with self._lock:
            lease_id = self._next_lease
            self._next_lease += 1
            self._active[lease_id] = read_ts
            return lease_id

    def release(self, lease_id: int) -> None:
        with self._lock:
            self._active.pop(lease_id, None)

    def min_active_read_ts(self) -> int | None:
        with self._lock:
            if not self._active:
                return None
            return min(self._active.values())

    def active_count(self) -> int:
        with self._lock:
            return len(self._active)
