"""Structured immutable compliance audit trail for TalonLedger."""

from __future__ import annotations

from dataclasses import dataclass

from .money import MinorUnits


@dataclass(frozen=True)
class AuditEvent:
    seq: int
    kind: str
    tx_id: str
    idempotency_key: str
    amount_minor: MinorUnits
    reason: str


class AuditLog:
    """Ordered log of settlement and reversal compliance events."""

    def __init__(self) -> None:
        self._events: list[AuditEvent] = []

    @property
    def events(self) -> tuple[AuditEvent, ...]:
        return tuple(self._events)

    def emit(
        self,
        kind: str,
        tx_id: str,
        idempotency_key: str,
        amount_minor: MinorUnits,
        reason: str,
    ) -> AuditEvent:
        event = AuditEvent(
            seq=len(self._events) + 1,
            kind=kind,
            tx_id=tx_id,
            idempotency_key=idempotency_key,
            amount_minor=amount_minor,
            reason=reason,
        )
        self._events.append(event)
        return event
