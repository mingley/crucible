"""Cadence expression parser for ChronosSched job definitions."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class CadenceSpec:
    interval_ms: int
    window_span_ms: int


def parse_cadence(expr: str) -> CadenceSpec:
    """Parse ``'every <interval>ms span <span>ms'`` cadence expressions."""
    tokens = expr.strip().split()
    if len(tokens) != 4 or tokens[0] != "every" or tokens[2] != "span":
        raise ValueError(f"invalid cadence expression: {expr!r}")
    if not tokens[1].endswith("ms") or not tokens[3].endswith("ms"):
        raise ValueError(f"cadence units must be in ms: {expr!r}")
    interval_ms = int(tokens[1][:-2], 10)
    span_ms = int(tokens[3][:-2], 10)
    if interval_ms <= 0 or span_ms <= 0:
        raise ValueError("cadence durations must be positive")
    return CadenceSpec(interval_ms=interval_ms, window_span_ms=span_ms)
