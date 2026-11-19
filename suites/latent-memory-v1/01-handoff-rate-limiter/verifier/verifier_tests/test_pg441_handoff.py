"""Answer-isolated specification tests for PG-441 in PulseGate."""

from __future__ import annotations

import pathlib
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from pulse_gate import (  # noqa: E402
    GatewayConfig,
    GatewayMiddleware,
    GatewayRequest,
    ManualClock,
    RatePolicy,
    SlidingWindowLimiter,
    rollover_burst_credits,
)


class PG441VerifierTests(unittest.TestCase):
    def test_single_idle_window_clamps_to_half_burst_cap(self) -> None:
        clock = ManualClock(initial_ns=100_000_000)
        policy = RatePolicy(sustained_per_window=10, burst_cap=8, window_ms=1000)
        cfg = GatewayConfig(default_policy=policy)
        limiter = SlidingWindowLimiter(cfg, clock=clock)

        # Window 0: spend 1, leaving 9 unused sustained tokens.
        d0 = limiter.evaluate("tenant-1", cost=1)
        self.assertTrue(d0.allowed)
        self.assertEqual(d0.window_index, 0)

        # Skip window 1 completely -> land in window 2 (idle_windows == 1).
        # Carried burst credits must clamp to int(0.5 * 8) = 4, NOT 8.
        clock.set_ns(2_050_000_000)
        # Sustained 10 + decayed burst 4 = 14 max capacity in window 2.
        denied = limiter.evaluate("tenant-1", cost=15)
        self.assertFalse(denied.allowed)
        self.assertEqual(denied.remaining_burst, 4)

        allowed = limiter.evaluate("tenant-1", cost=14)
        self.assertTrue(allowed.allowed)
        self.assertEqual(allowed.remaining_sustained, 0)
        self.assertEqual(allowed.remaining_burst, 0)

    def test_single_idle_window_preserves_smaller_pool_and_odd_burst_cap(self) -> None:
        policy = RatePolicy(sustained_per_window=10, burst_cap=9, window_ms=1000)
        # int(0.5 * 9) == 4
        self.assertEqual(
            rollover_burst_credits(prev_unused_sustained=2, prev_burst_credits=1, policy=policy, idle_windows=1),
            3,
        )
        self.assertEqual(
            rollover_burst_credits(prev_unused_sustained=7, prev_burst_credits=2, policy=policy, idle_windows=1),
            4,
        )

    def test_two_or_more_idle_windows_zero_burst_credits(self) -> None:
        clock = ManualClock(initial_ns=200_000_000)
        policy = RatePolicy(sustained_per_window=6, burst_cap=6, window_ms=1000)
        cfg = GatewayConfig(default_policy=policy)
        limiter = SlidingWindowLimiter(cfg, clock=clock)

        # Window 0: spend 1 -> 5 unused. Window 1 (consecutive): earn 5 burst credits.
        limiter.evaluate("tenant-2", cost=1)
        clock.set_ns(1_100_000_000)
        d1 = limiter.evaluate("tenant-2", cost=6)
        self.assertTrue(d1.allowed)
        self.assertEqual(d1.remaining_burst, 5)

        # Skip windows 2 and 3 completely -> land in window 4 (idle_windows == 2).
        # Burst credits must expire to 0; only sustained_per_window=6 is available.
        clock.set_ns(4_000_000_000)
        over = limiter.evaluate("tenant-2", cost=7)
        self.assertFalse(over.allowed)
        self.assertEqual(over.remaining_burst, 0)

        exact = limiter.evaluate("tenant-2", cost=6)
        self.assertTrue(exact.allowed)
        self.assertEqual(exact.remaining_sustained, 0)
        self.assertEqual(exact.remaining_burst, 0)

        # Also check large gap (idle_windows == 10).
        clock.set_ns(15_000_000_000)
        d_far = limiter.evaluate("tenant-2", cost=1)
        self.assertTrue(d_far.allowed)
        self.assertEqual(d_far.remaining_burst, 0)

    def test_reset_ms_header_uses_ceiling_rounding(self) -> None:
        clock = ManualClock(initial_ns=999_600_000)  # 0.4ms left in 1000ms window
        cfg = GatewayConfig(default_policy=RatePolicy(sustained_per_window=1, burst_cap=0, window_ms=1000))
        limiter = SlidingWindowLimiter(cfg, clock=clock)
        mw = GatewayMiddleware(cfg, limiter)

        first = mw.handle(GatewayRequest(tenant_id="t-ceil", path="/ping"))
        self.assertEqual(first.status_code, 200)
        self.assertEqual(first.headers["X-RateLimit-Reset-Ms"], "1")

        # 1 ns before window boundary -> still 1ms ceiling, never "0"
        clock.set_ns(999_999_999)
        second = mw.handle(GatewayRequest(tenant_id="t-ceil", path="/ping"))
        self.assertEqual(second.status_code, 429)
        self.assertEqual(second.headers["X-RateLimit-Reset-Ms"], "1")

        # Fractional multi-millisecond remainder: 250.000001ms remaining -> "251"
        clock.set_ns(1_749_999_999)
        third = mw.handle(GatewayRequest(tenant_id="t-ceil", path="/ping"))
        self.assertEqual(third.status_code, 200)
        self.assertEqual(third.headers["X-RateLimit-Reset-Ms"], "251")

        # Exact millisecond boundary: 250.000000ms remaining -> "250"
        clock.set_ns(1_750_000_000)
        fourth = mw.handle(GatewayRequest(tenant_id="t-ceil", path="/ping"))
        self.assertEqual(fourth.status_code, 429)
        self.assertEqual(fourth.headers["X-RateLimit-Reset-Ms"], "250")


if __name__ == "__main__":
    unittest.main()
