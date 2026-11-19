"""HTTP response header formatting for PulseGate rate decisions."""

from __future__ import annotations

from .bucket import RateDecision
from .clock import ns_to_ms_ceil

HEADER_LIMIT = "X-RateLimit-Limit"
HEADER_REMAINING = "X-RateLimit-Remaining"
HEADER_BURST_REMAINING = "X-RateLimit-Burst-Remaining"
HEADER_RESET_MS = "X-RateLimit-Reset-Ms"


def format_rate_headers(
    decision: RateDecision,
    include_burst: bool = True,
) -> dict[str, str]:
    """Serialize a ``RateDecision`` into standard gateway response headers."""
    reset_ms = ns_to_ms_ceil(decision.reset_after_ns)
    headers = {
        HEADER_LIMIT: str(decision.limit),
        HEADER_REMAINING: str(decision.remaining_sustained),
        HEADER_RESET_MS: str(reset_ms),
    }
    if include_burst:
        headers[HEADER_BURST_REMAINING] = str(decision.remaining_burst)
    return headers
