"""Smoke tests for single-key primary webhook verification and dispatch."""

from __future__ import annotations

import json
import unittest

from webhook_relay import RelayConfig, WebhookDispatcher, compute_v1_signature


class WebhookSmokeTests(unittest.TestCase):
    def test_primary_secret_dispatch(self) -> None:
        cfg = RelayConfig(primary_secret="whsec_primary_01", tolerance_seconds=300)
        dispatcher = WebhookDispatcher(cfg)
        seen: list[str] = []
        dispatcher.on("invoice.paid", lambda ev: seen.append(ev["id"]))

        body = json.dumps({"id": "evt_100", "type": "invoice.paid"}).encode("utf-8")
        ts = 1_700_000_000
        sig = compute_v1_signature("whsec_primary_01", ts, body)
        headers = {
            "X-Webhook-Id": "msg_100",
            "X-Webhook-Timestamp": str(ts),
            "X-Signature-256": f"v1={sig}",
        }
        wid, _ = dispatcher.dispatch(headers, body, now=ts + 10)
        self.assertEqual(wid, "msg_100")
        self.assertEqual(seen, ["evt_100"])


if __name__ == "__main__":
    unittest.main()
