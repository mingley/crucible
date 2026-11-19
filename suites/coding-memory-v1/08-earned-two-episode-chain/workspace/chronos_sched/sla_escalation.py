"""SLA breach tick calculation accounting for EpochTick leap offsets and blackouts."""

from __future__ import annotations

from typing import Sequence

from .policies import SlaPolicy


def compute_sla_breach_tick(
    enqueue_wall_ms: int,
    policy: SlaPolicy,
    blackout_windows: Sequence[tuple[int, int]] = (),
) -> int:
    """Compute the EpochTick at which an enqueued job breaches ``policy``.

    Must support multi-day budgets and blackout intervals efficiently via
    interval arithmetic rather than single-tick iteration.
    """
    raise NotImplementedError("TODO(CS-219): implement compute_sla_breach_tick")
