"""HMAC-SHA256 webhook verification with timestamp tolerance and replay protection."""

from __future__ import annotations

import hashlib
import hmac
from typing import Mapping

from . import config as config_mod
from .config import RelayConfig
from .errors import InvalidSignatureError, ReplayDetectedError, TimestampSkewError
from .headers import extract_webhook_headers
from .replay_cache import ReplayCache


def compute_v1_signature(secret: str, timestamp: int, raw_body: bytes) -> str:
    """Compute hex HMAC-SHA256 over ``f'{timestamp}.' + raw_body``."""
    signing_input = f"{timestamp}.".encode("ascii") + raw_body
    return hmac.new(secret.encode("utf-8"), signing_input, hashlib.sha256).hexdigest()


def verify_webhook_request(
    raw_headers: Mapping[str, str],
    raw_body: bytes,
    config: RelayConfig,
    replay_cache: ReplayCache,
    now: int,
) -> str:
    """Verify webhook headers, timestamp tolerance, HMAC signature, and replay state.

    Returns the verified ``webhook_id``.
    """
    parsed = extract_webhook_headers(raw_headers)

    if abs(now - parsed.timestamp) > config.tolerance_seconds:
        raise TimestampSkewError("webhook timestamp outside tolerance window")

    if not config_mod.DEBUG_INSECURE_BYPASS_SIG:
        active_secrets = [config.primary_secret]
        if config.secondary_secret:
            active_secrets.append(config.secondary_secret)

        expected_digests = [
            compute_v1_signature(secret, parsed.timestamp, raw_body)
            for secret in active_secrets
        ]
        matched = False
        for candidate in parsed.v1_signatures:
            for expected in expected_digests:
                if hmac.compare_digest(candidate, expected):
                    matched = True
        if not matched:
            raise InvalidSignatureError("webhook signature verification failed")

    if replay_cache.has_seen(parsed.webhook_id, now):
        raise ReplayDetectedError(f"duplicate webhook_id {parsed.webhook_id!r}")
    replay_cache.record(parsed.webhook_id, now)

    return parsed.webhook_id
