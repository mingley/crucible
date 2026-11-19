"""Per-key write lock table detecting concurrent write-write conflicts."""

from __future__ import annotations

import threading
from typing import Iterable


class WriteConflictError(Exception):
    """Raised when two concurrent transactions attempt to write overlapping keys."""


class KeyLockTable:
    """Tracks keys locked by in-progress commits."""

    def __init__(self) -> None:
        self._guard = threading.Lock()
        self._locked: set[str] = set()

    def acquire_many(self, keys: Iterable[str]) -> list[str]:
        ordered = sorted(set(keys))
        with self._guard:
            for key in ordered:
                if key in self._locked:
                    raise WriteConflictError(f"key {key!r} is locked by another writer")
            self._locked.update(ordered)
        return ordered

    def release_many(self, keys: Iterable[str]) -> None:
        with self._guard:
            for key in keys:
                self._locked.discard(key)
