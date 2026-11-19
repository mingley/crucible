"""Bounded-TTL in-memory cache preventing duplicate webhook processing."""

from __future__ import annotations


class ReplayCache:
    """Stores verified ``webhook_id`` values until their TTL expires."""

    def __init__(self, ttl_seconds: int = 600) -> None:
        if ttl_seconds <= 0:
            raise ValueError("ttl_seconds must be positive")
        self._ttl_seconds = ttl_seconds
        self._expires_at: dict[str, int] = {}

    def _evict_expired(self, now: int) -> None:
        expired = [wid for wid, exp in self._expires_at.items() if exp <= now]
        for wid in expired:
            del self._expires_at[wid]

    def has_seen(self, webhook_id: str, now: int) -> bool:
        self._evict_expired(now)
        return webhook_id in self._expires_at

    def record(self, webhook_id: str, now: int) -> None:
        self._evict_expired(now)
        self._expires_at[webhook_id] = now + self._ttl_seconds

    def size(self, now: int) -> int:
        self._evict_expired(now)
        return len(self._expires_at)
