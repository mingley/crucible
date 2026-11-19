"""Answer-isolated verifier tests for ChronosSched SLA breach tick chain."""

from __future__ import annotations

import pathlib
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from chronos_sched import (  # noqa: E402
    EscalationRunner,
    ScheduledJob,
    SlaPolicy,
    compute_sla_breach_tick,
    wall_ms_to_tick,
)


class ChronosEarnedChainVerifierTests(unittest.TestCase):
    def test_leap_offset_tiers_and_ceiling_budget_ticks(self) -> None:
        # Across 1_750_000_000_000 (+4 + +3 = +7 ticks) and 1_800_000_000_000 (+12 ticks)
        policy = SlaPolicy(name="tier1", budget_ms=600, grace_ticks=2)  # ceil(600/250)=3 + 2 = 5 ticks
        t1 = compute_sla_breach_tick(1_750_000_001_000, policy)
        # base = 1_750_000_001_000 // 250 + 7 = 7_000_000_004 + 7 = 7_000_000_011; + 5 = 7_000_000_016
        self.assertEqual(t1, 7_000_000_016)

        t2 = compute_sla_breach_tick(1_800_000_000_000, policy)
        # base = 7_200_000_000 + 12 = 7_200_000_012; + 5 = 7_200_000_017
        self.assertEqual(t2, 7_200_000_017)

    def test_half_open_blackout_pausing_and_boundary_landing(self) -> None:
        enqueue_ms = 1_700_000_000_000  # start_tick = 6_800_000_000 + 4 = 6_800_000_004
        base = wall_ms_to_tick(enqueue_ms)
        self.assertEqual(base, 6_800_000_004)

        policy = SlaPolicy(name="std", budget_ms=1000, grace_ticks=0)  # 4 active ticks

        # Half-open blackout [base+2, base+5) pauses 3 ticks (base+2, base+3, base+4);
        # active ticks are base+0, base+1, base+5, base+6 -> breach at base+7
        self.assertEqual(
            compute_sla_breach_tick(enqueue_ms, policy, [(base + 2, base + 5)]),
            base + 7,
        )

        # Landing exactly on blackout start [base+4, base+10), plus adjacent [base+10, base+12):
        # 4 active ticks (base..base+3) finish at base+4, which is inside merged [base+4, base+12),
        # so first non-blackout breach tick is base+12!
        unsorted_blackouts = [
            (base + 10, base + 12),
            (base + 8, base + 8),  # empty half-open interval ignored
            (base + 4, base + 10),
        ]
        self.assertEqual(
            compute_sla_breach_tick(enqueue_ms, policy, unsorted_blackouts),
            base + 12,
        )

        # Enqueued directly inside an active blackout [base - 5, base + 3):
        # cursor jumps to base + 3, then 4 active ticks -> base + 7
        self.assertEqual(
            compute_sla_breach_tick(enqueue_ms, policy, [(base - 5, base + 3)]),
            base + 7,
        )

    def test_zero_budget_and_large_multi_day_intervals(self) -> None:
        enqueue_ms = 1_800_000_000_250
        base = wall_ms_to_tick(enqueue_ms)
        zero_policy = SlaPolicy(name="immediate", budget_ms=0, grace_ticks=0)
        self.assertEqual(compute_sla_breach_tick(enqueue_ms, zero_policy, [(base, base + 6)]), base + 6)

        # 30-day budget + 10-day blackout must execute instantaneously via interval math
        thirty_days_ms = 30 * 24 * 3600 * 1000
        ten_days_ticks = 10 * 24 * 3600 * 4
        long_policy = SlaPolicy(name="batch", budget_ms=thirty_days_ms, grace_ticks=10)
        breach = compute_sla_breach_tick(
            enqueue_ms,
            long_policy,
            [(base + 1_000, base + 1_000 + ten_days_ticks)],
        )
        expected = base + (thirty_days_ms // 250) + 10 + ten_days_ticks
        self.assertEqual(breach, expected)

    def test_escalation_runner_integration(self) -> None:
        enqueue_ms = 1_700_000_000_000
        base = wall_ms_to_tick(enqueue_ms)
        job_fast = ScheduledJob("job-fast", enqueue_ms, SlaPolicy("fast", budget_ms=500, grace_ticks=0))
        job_slow = ScheduledJob("job-slow", enqueue_ms, SlaPolicy("slow", budget_ms=2000, grace_ticks=0))
        runner = EscalationRunner(blackout_windows=[(base + 1, base + 4)])

        # job_fast needs 2 active ticks: base (1), skip [base+1..base+4), base+4 (2) -> breach at base+5
        # base+5 corresponds to enqueue_ms + 5 * 250 = 1_700_000_001_250
        self.assertEqual(runner.breached_jobs([job_fast, job_slow], enqueue_ms + 4 * 250), [])
        self.assertEqual(runner.breached_jobs([job_fast, job_slow], enqueue_ms + 5 * 250), ["job-fast"])

        with self.assertRaises(ValueError):
            compute_sla_breach_tick(-250, SlaPolicy("bad", budget_ms=250))


if __name__ == "__main__":
    unittest.main()
