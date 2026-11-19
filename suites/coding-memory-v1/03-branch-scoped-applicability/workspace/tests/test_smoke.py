"""Smoke tests for ConduitRPC non-empty call/reply framing."""

from __future__ import annotations

import unittest

from conduit_rpc import BinaryTransport, MSG_CALL, MSG_REPLY


class ConduitSmokeTests(unittest.TestCase):
    def test_non_empty_call_and_reply_roundtrip(self) -> None:
        tx = BinaryTransport(compress_threshold=32)
        call_wire = tx.encode_frame(MSG_CALL, stream_id=3, payload=b"get_user:42")
        hdr, body = tx.decode_frame(call_wire)
        self.assertEqual(hdr.msg_type, MSG_CALL)
        self.assertEqual(hdr.stream_id, 3)
        self.assertEqual(body, b"get_user:42")

        large = b"payload-segment-" * 16
        reply_wire = tx.encode_frame(MSG_REPLY, stream_id=3, payload=large, allow_compress=True)
        rhdr, rbody = tx.decode_frame(reply_wire)
        self.assertEqual(rhdr.msg_type, MSG_REPLY)
        self.assertEqual(rbody, large)


if __name__ == "__main__":
    unittest.main()
