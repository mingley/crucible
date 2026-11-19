"""Exception classes for WebhookRelay signature and replay validation."""

from __future__ import annotations


class WebhookError(Exception):
    """Base error for rejected webhook deliveries."""


class MalformedHeaderError(WebhookError):
    """Raised when required webhook headers are missing or syntactically invalid."""


class TimestampSkewError(WebhookError):
    """Raised when X-Webhook-Timestamp falls outside configured tolerance."""


class InvalidSignatureError(WebhookError):
    """Raised when no v1 HMAC-SHA256 signature matches an active secret."""


class ReplayDetectedError(WebhookError):
    """Raised when a previously verified X-Webhook-Id is re-delivered."""
