"""Token-bucket burst carryover math and per-request quota consumption."""

from __future__ import annotations

from dataclasses import dataclass

from .config import RatePolicy


@dataclass(frozen=True)
class RateDecision:
    """Outcome of evaluating a single request against tenant quota."""

    allowed: bool
    limit: int
    remaining_sustained: int
    remaining_burst: int
    reset_after_ns: int
    window_index: int


def rollover_burst_credits(
    prev_unused_sustained: int,
    prev_burst_credits: int,
    policy: RatePolicy,
    idle_windows: int,
) -> int:
    """Compute earned burst credits carried into a newly entered epoch window.

    ``idle_windows`` is the count of full untouched windows between the previous
    active window and the newly entered window (0 for consecutive windows).
    """
    if idle_windows < 0:
        raise ValueError("idle_windows must be non-negative")
    pool = max(0, prev_burst_credits) + max(0, prev_unused_sustained)
    # Partial PG-441 implementation: consecutive-window clamp only.
    return min(policy.burst_cap, pool)


def try_consume(
    sustained_limit: int,
    used_sustained: int,
    burst_credits: int,
    cost: int,
) -> tuple[bool, int, int]:
    """Attempt to spend ``cost`` tokens from sustained quota then burst credits.

    Returns ``(allowed, next_used_sustained, next_burst_credits)``.
    Rejected requests leave both counters unchanged.
    """
    if cost <= 0:
        raise ValueError("cost must be positive")
    sustained_avail = max(0, sustained_limit - used_sustained)
    if sustained_avail + burst_credits < cost:
        return False, used_sustained, burst_credits

    from_sustained = min(sustained_avail, cost)
    overflow = cost - from_sustained
    return True, used_sustained + from_sustained, burst_credits - overflow
