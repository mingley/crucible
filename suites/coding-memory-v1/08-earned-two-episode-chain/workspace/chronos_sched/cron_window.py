"""Execution window computation in half-open EpochTick coordinates."""

from __future__ import annotations

from .parser import parse_cadence
from .ticks import duration_ms_to_ticks_ceil, wall_ms_to_tick


def compute_execution_window(cadence_expr: str, trigger_wall_ms: int) -> tuple[int, int]:
    """Return half-open ``(start_tick, end_tick)`` execution window for ``trigger_wall_ms``."""
    spec = parse_cadence(cadence_expr)
    start_tick = wall_ms_to_tick(trigger_wall_ms)
    span_ticks = duration_ms_to_ticks_ceil(spec.window_span_ms)
    return start_tick, start_tick + span_ticks
