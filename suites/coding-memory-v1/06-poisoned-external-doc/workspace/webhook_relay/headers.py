"""Case-insensitive extraction of WebhookRelay HTTP signature headers."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Mapping

from .errors import MalformedHeaderError


@dataclass(frozen=True)
class ParsedWebhookHeaders:
    webhook_id: str
    timestamp: int
    v1_signatures: list[str]


def extract_webhook_headers(raw_headers: Mapping[str, str]) -> ParsedWebhookHeaders:
    lowered = {k.lower(): v.strip() for k, v in raw_headers.items()}
    webhook_id = lowered.get("x-webhook-id", "")
    if not webhook_id:
        raise MalformedHeaderError("missing X-Webhook-Id header")

    raw_ts = lowered.get("x-webhook-timestamp", "")
    if not raw_ts:
        raise MalformedHeaderError("missing X-Webhook-Timestamp header")
    try:
        timestamp = int(raw_ts, 10)
    except ValueError as exc:
        raise MalformedHeaderError("invalid integer X-Webhook-Timestamp") from exc

    raw_sig = lowered.get("x-signature-256", "")
    if not raw_sig:
        raise MalformedHeaderError("missing X-Signature-256 header")

    v1_sigs: list[str] = []
    for part in raw_sig.split(","):
        token = part.strip()
        if not token:
            continue
        if token.startswith("v1="):
            digest_hex = token[3:].strip().lower()
            if len(digest_hex) == 64 and all(ch in "0123456789abcdef" for ch in digest_hex):
                v1_sigs.append(digest_hex)

    if not v1_sigs:
        raise MalformedHeaderError("no valid v1=<64-hex> signature found in X-Signature-256")

    return ParsedWebhookHeaders(
        webhook_id=webhook_id,
        timestamp=timestamp,
        v1_signatures=v1_sigs,
    )
