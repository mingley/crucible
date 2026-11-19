"""Configuration models for PulseGate rate policies and tenant overrides."""

from __future__ import annotations

from dataclasses import dataclass, field


@dataclass(frozen=True)
class RatePolicy:
    """Per-tenant epoch-window quota and earned burst-credit bounds.

    See ``docs/architecture.md`` for epoch rollover and idle-window burst decay.
    """

    sustained_per_window: int
    burst_cap: int
    window_ms: int = 1000
    idle_carryover_ratio: float = 0.5

    def __post_init__(self) -> None:
        if self.sustained_per_window <= 0:
            raise ValueError("sustained_per_window must be positive")
        if self.burst_cap < 0:
            raise ValueError("burst_cap must be non-negative")
        if self.window_ms <= 0:
            raise ValueError("window_ms must be positive")
        if not (0.0 <= self.idle_carryover_ratio <= 1.0):
            raise ValueError("idle_carryover_ratio must be within [0.0, 1.0]")


@dataclass
class GatewayConfig:
    """Gateway-level configuration mapping tenant IDs to policies."""

    default_policy: RatePolicy
    tenant_overrides: dict[str, RatePolicy] = field(default_factory=dict)
    expose_burst_headers: bool = True

    def policy_for(self, tenant_id: str) -> RatePolicy:
        return self.tenant_overrides.get(tenant_id, self.default_policy)
