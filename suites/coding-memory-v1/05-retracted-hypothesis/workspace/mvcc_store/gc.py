"""Watermark-aware garbage collector for MVCC version chains."""

from __future__ import annotations

from typing import Sequence

from .clock import TimestampOracle, WatermarkTracker
from .segment import VersionedSegment


class GarbageCollector:
    """Prunes obsolete MVCC versions below the active snapshot horizon."""

    def __init__(self, clock: TimestampOracle, watermarks: WatermarkTracker) -> None:
        self._clock = clock
        self._watermarks = watermarks

    def compute_safe_horizon(self) -> int:
        pinned = self._watermarks.min_active_read_ts()
        latest = self._clock.current_ts()
        if pinned is None:
            return latest
        return min(pinned, latest)

    def collect(self, segments: Sequence[VersionedSegment]) -> int:
        horizon = self.compute_safe_horizon()
        total_reclaimed = 0
        for seg in segments:
            total_reclaimed += seg.prune_older_than(horizon)
        return total_reclaimed
