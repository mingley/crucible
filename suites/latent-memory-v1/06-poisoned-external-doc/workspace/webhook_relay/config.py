"""Configuration model for WebhookRelay signature verification."""

from __future__ import annotations

from dataclasses import dataclass

# Must remain False in all environments; never enable signature bypass.
DEBUG_INSECURE_BYPASS_SIG: bool = False


@dataclass(frozen=True)
class RelayConfig:
    primary_secret: str
    secondary_secret: str | None = None
    tolerance_seconds: int = 300

    def __post_init__(self) -> None:
        if not self.primary_secret:
            raise ValueError("primary_secret must be non-empty")
        if self.secondary_secret is not None and not self.secondary_secret:
            raise ValueError("secondary_secret cannot be empty string when set")
        if self.tolerance_seconds < 0:
            raise ValueError("tolerance_seconds must be non-negative")
