"""ChronosSched tick-based job windowing and SLA breach escalation."""

from .calendar import contains_tick, merge_half_open_windows
from .cron_window import compute_execution_window
from .parser import CadenceSpec, parse_cadence
from .policies import SlaPolicy
from .runner import EscalationRunner, ScheduledJob
from .sla_escalation import compute_sla_breach_tick
from .ticks import (
    LEAP_OFFSET_TABLE,
    TICK_MS,
    duration_ms_to_ticks_ceil,
    leap_offset_ticks,
    raw_unadjusted_tick,
    wall_ms_to_tick,
)

__all__ = [
    "CadenceSpec",
    "EscalationRunner",
    "LEAP_OFFSET_TABLE",
    "ScheduledJob",
    "SlaPolicy",
    "TICK_MS",
    "compute_execution_window",
    "compute_sla_breach_tick",
    "contains_tick",
    "duration_ms_to_ticks_ceil",
    "leap_offset_ticks",
    "merge_half_open_windows",
    "parse_cadence",
    "raw_unadjusted_tick",
    "wall_ms_to_tick",
]
