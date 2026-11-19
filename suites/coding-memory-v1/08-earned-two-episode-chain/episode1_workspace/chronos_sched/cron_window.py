"""Execution window computation in half-open EpochTick coordinates."""

from __future__ import annotations

from .parser import parse_cadence


def compute_execution_window(cadence_expr: str, trigger_wall_ms: int) -> tuple[int, int]:
    """Return half-open ``(start_tick, end_tick)`` execution window for ``trigger_wall_ms``."""
    spec = parse_cadence(cadence_expr)
    # Buggy draft: assumes 1 tick = 1000ms, omits leap offsets, and uses inclusive end_tick.
    start_tick = trigger_wall_ms // 1000
    span_ticks = max(1, spec.window_span_ms // 1000)
    return start_tick, start_tick + span_ticks - 1
