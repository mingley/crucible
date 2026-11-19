"""SLA policy definitions for ChronosSched jobs."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class SlaPolicy:
    name: str
    budget_ms: int
    grace_ticks: int = 0

    def __post_init__(self) -> None:
        if self.budget_ms < 0:
            raise ValueError("budget_ms must be non-negative")
        if self.grace_ticks < 0:
            raise ValueError("grace_ticks must be non-negative")
