"""Unit tests for ChronosSched Episode 1 execution window bugfix."""

from __future__ import annotations

import unittest

from chronos_sched import compute_execution_window, contains_tick


class Episode1CronWindowTests(unittest.TestCase):
    def test_pre_cutover_250ms_ticks_and_ceiling_span(self) -> None:
        # Before first leap cutover: 10_000 ms -> tick 40; span 600ms -> ceil(600/250) = 3 ticks -> [40, 43)
        start_t, end_t = compute_execution_window("every 2000ms span 600ms", 10_000)
        self.assertEqual((start_t, end_t), (40, 43))
        self.assertTrue(contains_tick((start_t, end_t), 42))
        self.assertFalse(contains_tick((start_t, end_t), 43))

    def test_cumulative_leap_cutover_offsets_applied_to_wall_time_only(self) -> None:
        # At 1_750_000_000_500 ms: raw tick = 7_000_000_002, cumulative leap offset = 4 + 3 = 7 -> 7_000_000_009
        # Span 750ms -> 3 ticks (never shifted by leap table) -> [7_000_000_009, 7_000_000_012)
        start_t, end_t = compute_execution_window("every 5000ms span 750ms", 1_750_000_000_500)
        self.assertEqual((start_t, end_t), (7_000_000_009, 7_000_000_012))


if __name__ == "__main__":
    unittest.main()
