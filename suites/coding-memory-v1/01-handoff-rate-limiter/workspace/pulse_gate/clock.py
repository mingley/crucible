"""Nanosecond monotonic clock abstractions and millisecond conversion helpers."""

from __future__ import annotations

import time
from typing import Protocol

NS_PER_MS = 1_000_000


class Clock(Protocol):
    """Monotonic nanosecond clock source."""

    def now_ns(self) -> int: ...


class MonotonicClock:
    """Production monotonic clock backed by ``time.monotonic_ns``."""

    def now_ns(self) -> int:
        return time.monotonic_ns()


class ManualClock:
    """Deterministic nanosecond clock for testing."""

    def __init__(self, initial_ns: int = 0) -> None:
        if initial_ns < 0:
            raise ValueError("initial_ns must be non-negative")
        self._now_ns = initial_ns

    def now_ns(self) -> int:
        return self._now_ns

    def set_ns(self, value_ns: int) -> None:
        if value_ns < self._now_ns:
            raise ValueError("monotonic clock cannot move backwards")
        self._now_ns = value_ns

    def advance_ms(self, delta_ms: int) -> None:
        self.advance_ns(delta_ms * NS_PER_MS)

    def advance_ns(self, delta_ns: int) -> None:
        if delta_ns < 0:
            raise ValueError("delta_ns must be non-negative")
        self._now_ns += delta_ns


def ms_to_ns(ms: int) -> int:
    return ms * NS_PER_MS


def ns_to_ms_floor(ns: int) -> int:
    """Truncate nanoseconds down to whole milliseconds."""
    if ns <= 0:
        return 0
    return ns // NS_PER_MS


def ns_to_ms_ceil(ns: int) -> int:
    """Round nanoseconds up to the next whole millisecond.

    Used for client-facing retry/reset backoff headers so sub-millisecond
    remainders never advise an immediate retry inside the same active window.
    """
    if ns <= 0:
        return 0
    return (ns + NS_PER_MS - 1) // NS_PER_MS
