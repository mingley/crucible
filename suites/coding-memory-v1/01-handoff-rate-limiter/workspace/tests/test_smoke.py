"""Baseline smoke tests for PulseGate single-window and consecutive-window paths."""

from __future__ import annotations

import unittest

from pulse_gate import (
    GatewayConfig,
    GatewayMiddleware,
    GatewayRequest,
    ManualClock,
    RatePolicy,
    SlidingWindowLimiter,
)


class SmokeRateLimiterTests(unittest.TestCase):
    def test_single_window_sustained_enforcement(self) -> None:
        clock = ManualClock(initial_ns=500_000_000)  # 500ms into 1000ms window
        cfg = GatewayConfig(default_policy=RatePolicy(sustained_per_window=3, burst_cap=4, window_ms=1000))
        limiter = SlidingWindowLimiter(cfg, clock=clock)
        mw = GatewayMiddleware(cfg, limiter)

        for expected_rem in (2, 1, 0):
            resp = mw.handle(GatewayRequest(tenant_id="t-alpha", path="/v1/items"))
            self.assertEqual(resp.status_code, 200)
            self.assertEqual(resp.headers["X-RateLimit-Remaining"], str(expected_rem))
            self.assertEqual(resp.headers["X-RateLimit-Reset-Ms"], "500")

        denied = mw.handle(GatewayRequest(tenant_id="t-alpha", path="/v1/items"))
        self.assertEqual(denied.status_code, 429)

    def test_consecutive_window_earned_burst(self) -> None:
        clock = ManualClock(initial_ns=100_000_000)
        cfg = GatewayConfig(default_policy=RatePolicy(sustained_per_window=5, burst_cap=4, window_ms=1000))
        limiter = SlidingWindowLimiter(cfg, clock=clock)

        # Spend 1 of 5 in window 0 -> 4 unused sustained tokens.
        d0 = limiter.evaluate("t-beta", cost=1)
        self.assertTrue(d0.allowed)
        self.assertEqual(d0.remaining_sustained, 4)

        # Advance to immediately consecutive window 1 (idle_windows == 0).
        clock.set_ns(1_200_000_000)
        d1 = limiter.evaluate("t-beta", cost=8)
        self.assertTrue(d1.allowed)
        self.assertEqual(d1.remaining_sustained, 0)
        self.assertEqual(d1.remaining_burst, 1)


if __name__ == "__main__":
    unittest.main()
