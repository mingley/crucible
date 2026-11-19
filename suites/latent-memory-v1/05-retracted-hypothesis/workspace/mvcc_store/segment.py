"""Version-chained shard storage for MVCC keys."""

from __future__ import annotations

import threading
from dataclasses import dataclass


@dataclass(frozen=True)
class VersionRecord:
    commit_ts: int
    value: bytes | None  # None represents a deletion tombstone


class VersionedSegment:
    """Stores ascending ``commit_ts`` version chains for a shard of keys."""

    def __init__(self, shard_id: int) -> None:
        self.shard_id = shard_id
        self._lock = threading.RLock()
        self._chains: dict[str, list[VersionRecord]] = {}

    def latest_version_after_ts(self, key: str, after_ts: int) -> bool:
        with self._lock:
            chain = self._chains.get(key)
            return bool(chain and chain[-1].commit_ts > after_ts)

    def append_version(self, key: str, commit_ts: int, value: bytes | None) -> None:
        with self._lock:
            chain = self._chains.setdefault(key, [])
            if chain and commit_ts <= chain[-1].commit_ts:
                raise ValueError("commit_ts must be strictly increasing per key")
            chain.append(VersionRecord(commit_ts=commit_ts, value=value))

    def read_at(self, key: str, read_ts: int) -> tuple[bool, bytes | None]:
        with self._lock:
            chain = self._chains.get(key)
            if not chain:
                return False, None
            for rec in reversed(chain):
                if rec.commit_ts <= read_ts:
                    return True, rec.value
            return False, None

    def keys_snapshot(self) -> list[str]:
        with self._lock:
            return sorted(self._chains.keys())

    def version_count(self) -> int:
        with self._lock:
            return sum(len(chain) for chain in self._chains.values())

    def prune_older_than(self, safe_horizon_ts: int) -> int:
        """Reclaim historical versions superseded at or before ``safe_horizon_ts``.

        Preserves the newest version with ``commit_ts <= safe_horizon_ts`` (the
        floor needed by any reader at ``>= safe_horizon_ts``) plus all versions
        with ``commit_ts > safe_horizon_ts``. Pure tombstones at ``<= safe_horizon_ts``
        with no newer versions are removed when ``commit_ts < safe_horizon_ts``.
        """
        reclaimed = 0
        with self._lock:
            empty_keys: list[str] = []
            for key, chain in self._chains.items():
                le_versions = [v for v in chain if v.commit_ts <= safe_horizon_ts]
                gt_versions = [v for v in chain if v.commit_ts > safe_horizon_ts]
                if not le_versions:
                    continue
                floor_ver = le_versions[-1]
                dropped = len(le_versions) - 1
                if floor_ver.value is None and not gt_versions and floor_ver.commit_ts < safe_horizon_ts:
                    reclaimed += len(le_versions)
                    empty_keys.append(key)
                elif dropped > 0:
                    reclaimed += dropped
                    self._chains[key] = [floor_ver] + gt_versions
            for key in empty_keys:
                del self._chains[key]
        return reclaimed
