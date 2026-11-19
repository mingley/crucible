"""Snapshot-isolated read-write transaction handle for MVCCStore."""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .engine import MVCCEngine


class Transaction:
    """Buffers writes locally and validates write-write conflicts at commit."""

    def __init__(self, engine: MVCCEngine, read_ts: int) -> None:
        self._engine = engine
        self.read_ts = read_ts
        self._writes: dict[str, bytes | None] = {}
        self._closed = False

    def get(self, key: str) -> bytes | None:
        if key in self._writes:
            return self._writes[key]
        return self._engine.get(key, read_ts=self.read_ts)

    def put(self, key: str, value: bytes) -> None:
        if self._closed:
            raise RuntimeError("transaction already closed")
        self._writes[key] = bytes(value)

    def delete(self, key: str) -> None:
        if self._closed:
            raise RuntimeError("transaction already closed")
        self._writes[key] = None

    def commit(self) -> int:
        if self._closed:
            raise RuntimeError("transaction already closed")
        self._closed = True
        return self._engine.commit_writes(self.read_ts, self._writes)
