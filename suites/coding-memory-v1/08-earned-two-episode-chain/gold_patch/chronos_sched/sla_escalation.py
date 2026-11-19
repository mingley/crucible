"""SLA breach tick calculation accounting for EpochTick leap offsets and blackouts."""

from __future__ import annotations

from typing import Sequence

from .calendar import merge_half_open_windows
from .policies import SlaPolicy
from .ticks import duration_ms_to_ticks_ceil, wall_ms_to_tick


def compute_sla_breach_tick(
    enqueue_wall_ms: int,
    policy: SlaPolicy,
    blackout_windows: Sequence[tuple[int, int]] = (),
) -> int:
    """Compute the EpochTick at which an enqueued job breaches ``policy``.

    Returns the first non-blackout tick at or after ``wall_ms_to_tick(enqueue_wall_ms)``
    once ``duration_ms_to_ticks_ceil(policy.budget_ms) + policy.grace_ticks``
    non-blackout ticks have elapsed.
    """
    cursor = wall_ms_to_tick(enqueue_wall_ms)
    remaining = duration_ms_to_ticks_ceil(policy.budget_ms) + policy.grace_ticks
    merged = merge_half_open_windows(blackout_windows)

    for b_start, b_end in merged:
        if b_end <= cursor:
            continue
        if b_start > cursor:
            available = b_start - cursor
            if remaining < available:
                return cursor + remaining
            remaining -= available
            cursor = b_end
        else:
            cursor = b_end

    return cursor + remaining
