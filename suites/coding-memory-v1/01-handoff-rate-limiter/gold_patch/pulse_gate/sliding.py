"""Epoch-aligned window evaluation and state transitions for PulseGate."""

from __future__ import annotations

from .bucket import RateDecision, rollover_burst_credits, try_consume
from .clock import Clock, MonotonicClock, ms_to_ns
from .config import GatewayConfig
from .storage import InMemoryWindowStore, TenantWindowState


class SlidingWindowLimiter:
    """Evaluates tenant traffic across discrete epoch windows with burst carryover."""

    def __init__(
        self,
        config: GatewayConfig,
        clock: Clock | None = None,
        store: InMemoryWindowStore | None = None,
    ) -> None:
        self._config = config
        self._clock = clock if clock is not None else MonotonicClock()
        self._store = store if store is not None else InMemoryWindowStore()

    @property
    def store(self) -> InMemoryWindowStore:
        return self._store

    def evaluate(self, tenant_id: str, cost: int = 1) -> RateDecision:
        """Evaluate a request of ``cost`` units for ``tenant_id``."""
        if not tenant_id:
            raise ValueError("tenant_id must be non-empty")
        policy = self._config.policy_for(tenant_id)
        window_ns = ms_to_ns(policy.window_ms)
        now_ns = self._clock.now_ns()
        current_window = now_ns // window_ns
        window_end_ns = (current_window + 1) * window_ns
        reset_after_ns = window_end_ns - now_ns

        state = self._store.get(tenant_id)
        if state is None:
            used_sustained = 0
            burst_credits = 0
        elif current_window == state.window_index:
            used_sustained = state.used_sustained
            burst_credits = state.burst_credits
        elif current_window > state.window_index:
            idle_windows = (current_window - state.window_index) - 1
            prev_unused = max(0, policy.sustained_per_window - state.used_sustained)
            burst_credits = rollover_burst_credits(
                prev_unused_sustained=prev_unused,
                prev_burst_credits=state.burst_credits,
                policy=policy,
                idle_windows=idle_windows,
            )
            used_sustained = 0
        else:
            raise RuntimeError("clock moved backwards across window boundary")

        allowed, next_used, next_burst = try_consume(
            sustained_limit=policy.sustained_per_window,
            used_sustained=used_sustained,
            burst_credits=burst_credits,
            cost=cost,
        )
        if allowed or state is None or current_window > state.window_index:
            self._store.put(
                tenant_id,
                TenantWindowState(
                    window_index=current_window,
                    used_sustained=next_used,
                    burst_credits=next_burst,
                    updated_ns=now_ns,
                ),
            )

        remaining_sustained = max(0, policy.sustained_per_window - next_used)
        return RateDecision(
            allowed=allowed,
            limit=policy.sustained_per_window,
            remaining_sustained=remaining_sustained,
            remaining_burst=next_burst,
            reset_after_ns=reset_after_ns,
            window_index=current_window,
        )
