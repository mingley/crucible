"""Exception hierarchy for ConduitRPC transport and connection pools."""

from __future__ import annotations


class ConduitError(Exception):
    """Base error for all ConduitRPC failures."""


class FrameCorruptError(ConduitError):
    """Raised when a wire frame header or length is malformed."""


class ChecksumMismatchError(ConduitError):
    """Raised when a wire frame CRC-32 digest fails verification."""


class PoolExhaustedError(ConduitError):
    """Raised when no healthy pooled connection slot is available."""


class RetryExhaustedError(ConduitError):
    """Raised when an idempotent RPC exceeds configured retry attempts."""
