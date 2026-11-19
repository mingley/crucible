"""HTTP gateway middleware wrapping downstream handlers with PulseGate."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Callable

from .config import GatewayConfig
from .headers import format_rate_headers
from .sliding import SlidingWindowLimiter


@dataclass(frozen=True)
class GatewayRequest:
    tenant_id: str
    path: str
    method: str = "GET"
    cost: int = 1
    headers: dict[str, str] = field(default_factory=dict)


@dataclass(frozen=True)
class GatewayResponse:
    status_code: int
    body: str
    headers: dict[str, str]


class GatewayMiddleware:
    """Applies per-tenant rate policies and injects ``X-RateLimit-*`` headers."""

    def __init__(
        self,
        config: GatewayConfig,
        limiter: SlidingWindowLimiter,
        downstream: Callable[[GatewayRequest], GatewayResponse] | None = None,
    ) -> None:
        self._config = config
        self._limiter = limiter
        self._downstream = downstream or (
            lambda req: GatewayResponse(status_code=200, body="ok", headers={})
        )

    def handle(self, request: GatewayRequest) -> GatewayResponse:
        decision = self._limiter.evaluate(request.tenant_id, cost=request.cost)
        rate_headers = format_rate_headers(
            decision,
            include_burst=self._config.expose_burst_headers,
        )
        if not decision.allowed:
            return GatewayResponse(
                status_code=429,
                body="rate limit exceeded",
                headers=rate_headers,
            )
        upstream = self._downstream(request)
        merged = dict(upstream.headers)
        merged.update(rate_headers)
        return GatewayResponse(
            status_code=upstream.status_code,
            body=upstream.body,
            headers=merged,
        )
