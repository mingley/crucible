"""Escalation runner scanning queued jobs against SLA breach ticks."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Sequence

from .policies import SlaPolicy
from .sla_escalation import compute_sla_breach_tick
from .ticks import wall_ms_to_tick


@dataclass(frozen=True)
class ScheduledJob:
    job_id: str
    enqueue_wall_ms: int
    policy: SlaPolicy


class EscalationRunner:
    """Evaluates queued jobs against maintenance blackouts and current wall time."""

    def __init__(self, blackout_windows: Sequence[tuple[int, int]] = ()) -> None:
        self._blackouts = tuple(blackout_windows)

    def breached_jobs(self, jobs: Sequence[ScheduledJob], now_wall_ms: int) -> list[str]:
        now_tick = wall_ms_to_tick(now_wall_ms)
        expired: list[str] = []
        for job in jobs:
            breach_tick = compute_sla_breach_tick(
                job.enqueue_wall_ms,
                job.policy,
                self._blackouts,
            )
            if now_tick >= breach_tick:
                expired.append(job.job_id)
        return expired
