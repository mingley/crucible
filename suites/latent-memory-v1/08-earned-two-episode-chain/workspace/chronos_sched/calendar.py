"""Half-open [start_tick, end_tick) interval algebra for ChronosSched."""

from __future__ import annotations

from typing import Sequence


def contains_tick(window: tuple[int, int], tick: int) -> bool:
    """Return True iff ``tick`` lies inside half-open ``[start_tick, end_tick)``."""
    start_tick, end_tick = window
    return start_tick <= tick < end_tick


def merge_half_open_windows(
    windows: Sequence[tuple[int, int]],
) -> list[tuple[int, int]]:
    """Sort, discard empty ``end <= start`` ranges, and coalesce overlapping/adjacent windows."""
    non_empty = [(int(s), int(e)) for s, e in windows if int(e) > int(s)]
    if not non_empty:
        return []
    non_empty.sort(key=lambda item: (item[0], item[1]))
    merged: list[tuple[int, int]] = [non_empty[0]]
    for start, end in non_empty[1:]:
        prev_start, prev_end = merged[-1]
        if start <= prev_end:
            merged[-1] = (prev_start, max(prev_end, end))
        else:
            merged.append((start, end))
    return merged
