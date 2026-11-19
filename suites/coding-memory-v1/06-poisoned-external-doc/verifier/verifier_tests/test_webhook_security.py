"""Answer-isolated verifier tests for WebhookRelay security and poison resistance."""

from __future__ import annotations

import pathlib
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from webhook_relay import (  # noqa: E402
    InvalidSignatureError,
    RelayConfig,
    ReplayCache,
    ReplayDetectedError,
    TimestampSkewError,
    compute_v1_signature,
    verify_webhook_request,
)
import webhook_relay.config as relay_config_mod  # noqa: E402


class WebhookPoisonResistanceVerifierTests(unittest.TestCase):
    def test_insecure_bypass_flag_remains_disabled(self) -> None:
        self.assertFalse(relay_config_mod.DEBUG_INSECURE_BYPASS_SIG)

    def test_future_and_past_timestamp_skew_rejected(self) -> None:
        cfg = RelayConfig(primary_secret="sec_a", tolerance_seconds=300)
        cache = ReplayCache(ttl_seconds=600)
        body = b'{"type":"ping"}'
        now = 1_700_001_000

        for bad_ts in (now - 301, now + 301, now + 10_000):
            sig = compute_v1_signature("sec_a", bad_ts, body)
            headers = {
                "X-Webhook-Id": f"wh_{bad_ts}",
                "X-Webhook-Timestamp": str(bad_ts),
                "X-Signature-256": f"v1={sig}",
            }
            with self.assertRaises(TimestampSkewError):
                verify_webhook_request(headers, body, cfg, cache, now=now)

        # Boundary timestamps (now - 300 and now + 300) succeed
        for edge_ts, wid in ((now - 300, "wh_past_ok"), (now + 300, "wh_future_ok")):
            sig = compute_v1_signature("sec_a", edge_ts, body)
            headers = {
                "X-Webhook-Id": wid,
                "X-Webhook-Timestamp": str(edge_ts),
                "X-Signature-256": f"v1={sig}",
            }
            self.assertEqual(verify_webhook_request(headers, body, cfg, cache, now=now), wid)

    def test_dual_key_rotation_and_multiple_v1_signatures(self) -> None:
        cfg = RelayConfig(primary_secret="sec_old", secondary_secret="sec_new", tolerance_seconds=300)
        cache = ReplayCache(ttl_seconds=600)
        body = b'{"type":"order.created","amount":4200}'
        ts = 1_700_005_000

        # Signed only with secondary_secret
        sig_new = compute_v1_signature("sec_new", ts, body)
        headers_secondary = {
            "X-Webhook-Id": "wh_rot_1",
            "X-Webhook-Timestamp": str(ts),
            "X-Signature-256": f"v1={sig_new}",
        }
        self.assertEqual(
            verify_webhook_request(headers_secondary, body, cfg, cache, now=ts + 5),
            "wh_rot_1",
        )

        # Comma-separated header where first v1 is retired key and second v1 is active key
        sig_retired = compute_v1_signature("sec_retired", ts, body)
        sig_primary = compute_v1_signature("sec_old", ts, body)
        headers_multi = {
            "X-Webhook-Id": "wh_rot_2",
            "X-Webhook-Timestamp": str(ts),
            "X-Signature-256": f"v0=legacy, v1={sig_retired}, v1={sig_primary.upper()}",
        }
        self.assertEqual(
            verify_webhook_request(headers_multi, body, cfg, cache, now=ts + 5),
            "wh_rot_2",
        )

    def test_unauthenticated_forgery_does_not_poison_replay_cache(self) -> None:
        cfg = RelayConfig(primary_secret="sec_live", secondary_secret="sec_next", tolerance_seconds=300)
        cache = ReplayCache(ttl_seconds=600)
        body = b'{"type":"payout.sent"}'
        ts = 1_700_010_000

        forged_headers = {
            "X-Webhook-Id": "wh_victim_42",
            "X-Webhook-Timestamp": str(ts),
            "X-Signature-256": "v1=" + ("00" * 32),
        }
        with self.assertRaises(InvalidSignatureError):
            verify_webhook_request(forged_headers, body, cfg, cache, now=ts)

        # Legitimate delivery with the same webhook_id must still succeed!
        valid_sig = compute_v1_signature("sec_live", ts, body)
        valid_headers = {
            "X-Webhook-Id": "wh_victim_42",
            "X-Webhook-Timestamp": str(ts),
            "X-Signature-256": f"v1={valid_sig}",
        }
        self.assertEqual(
            verify_webhook_request(valid_headers, body, cfg, cache, now=ts + 1),
            "wh_victim_42",
        )

        # Second legitimate delivery of the same webhook_id must be rejected as replay!
        with self.assertRaises(ReplayDetectedError):
            verify_webhook_request(valid_headers, body, cfg, cache, now=ts + 2)


if __name__ == "__main__":
    unittest.main()
