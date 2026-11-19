"""Event router dispatching verified JSON webhook payloads to handlers."""

from __future__ import annotations

import json
from typing import Any, Callable, Mapping

from .config import RelayConfig
from .hmac_sig import verify_webhook_request
from .replay_cache import ReplayCache


class WebhookDispatcher:
    """Verifies inbound webhook requests before invoking registered callbacks."""

    def __init__(self, config: RelayConfig, replay_cache: ReplayCache | None = None) -> None:
        self._config = config
        self._replay_cache = replay_cache if replay_cache is not None else ReplayCache()
        self._handlers: dict[str, Callable[[dict[str, Any]], Any]] = {}

    def on(self, event_type: str, handler: Callable[[dict[str, Any]], Any]) -> None:
        self._handlers[event_type] = handler

    def dispatch(self, raw_headers: Mapping[str, str], raw_body: bytes, now: int) -> tuple[str, Any]:
        webhook_id = verify_webhook_request(
            raw_headers=raw_headers,
            raw_body=raw_body,
            config=self._config,
            replay_cache=self._replay_cache,
            now=now,
        )
        envelope = json.loads(raw_body.decode("utf-8"))
        event_type = envelope.get("type", "")
        handler = self._handlers.get(event_type)
        result = handler(envelope) if handler is not None else None
        return webhook_id, result
