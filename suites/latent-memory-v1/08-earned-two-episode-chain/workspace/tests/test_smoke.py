"""Smoke tests for ChronosSched cadence parsing and execution windows."""

from __future__ import annotations

import unittest

from chronos_sched import compute_execution_window, contains_tick


class ChronosSmokeTests(unittest.TestCase):
    def test_execution_window_with_leap_offset_and_half_open_bounds(self) -> None:
        # 1_700_000_001_000 ms -> raw tick 6_800_000_004 + leap offset 4 = 6_800_000_008
        start_t, end_t = compute_execution_window("every 5000ms span 750ms", 1_700_000_001_000)
        self.assertEqual(start_t, 6_800_000_008)
        self.assertEqual(end_t, 6_800_000_011)
        self.assertTrue(contains_tick((start_t, end_t), 6_800_000_010))
        self.assertFalse(contains_tick((start_t, end_t), 6_800_000_011))


if __name__ == "__main__":
    unittest.main()
