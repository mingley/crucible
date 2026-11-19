"""Per-tenant window state storage for PulseGate."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class TenantWindowState:
    """Immutable snapshot of a tenant's active epoch window counters."""

    window_index: int
    used_sustained: int
    burst_credits: int
    updated_ns: int


class InMemoryWindowStore:
    """Single-process state store keyed by tenant identifier."""

    def __init__(self) -> None:
        self._entries: dict[str, TenantWindowState] = {}

    def get(self, tenant_id: str) -> TenantWindowState | None:
        return self._entries.get(tenant_id)

    def put(self, tenant_id: str, state: TenantWindowState) -> None:
        self._entries[tenant_id] = state

    def delete(self, tenant_id: str) -> None:
        self._entries.pop(tenant_id, None)

    def tenant_count(self) -> int:
        return len(self._entries)
