"""EpochTick (250ms) conversions and leap-window drift compensation table."""

from __future__ import annotations

TICK_MS = 250

# Historical leap-smear realignment cutovers: (cutover_wall_ms, added_offset_ticks).
# Every cutover with cutover_wall_ms <= wall_ms contributes cumulatively.
LEAP_OFFSET_TABLE: tuple[tuple[int, int], ...] = (
    (1_700_000_000_000, 4),
    (1_750_000_000_000, 3),
    (1_800_000_000_000, 5),
)


def leap_offset_ticks(wall_ms: int) -> int:
    """Return cumulative EpochTick offset active at ``wall_ms``."""
    if wall_ms < 0:
        raise ValueError("wall_ms must be non-negative")
    total = 0
    for cutover_ms, delta_ticks in LEAP_OFFSET_TABLE:
        if wall_ms >= cutover_ms:
            total += delta_ticks
    return total


def raw_unadjusted_tick(wall_ms: int) -> int:
    """Divide ``wall_ms`` by ``TICK_MS`` without leap-table compensation."""
    if wall_ms < 0:
        raise ValueError("wall_ms must be non-negative")
    return wall_ms // TICK_MS


def wall_ms_to_tick(wall_ms: int) -> int:
    """Convert an epoch wall-clock millisecond timestamp into an ``EpochTick``."""
    return raw_unadjusted_tick(wall_ms) + leap_offset_ticks(wall_ms)


def duration_ms_to_ticks_ceil(duration_ms: int) -> int:
    """Convert a relative duration in milliseconds into ceiling ``EpochTick`` units.

    Relative durations never apply ``LEAP_OFFSET_TABLE``.
    """
    if duration_ms < 0:
        raise ValueError("duration_ms must be non-negative")
    if duration_ms == 0:
        return 0
    return (duration_ms + TICK_MS - 1) // TICK_MS
