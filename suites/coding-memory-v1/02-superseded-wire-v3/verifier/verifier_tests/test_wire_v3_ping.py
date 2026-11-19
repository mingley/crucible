"""Answer-isolated verifier tests for ZephyrWire v3 control ping framing."""

from __future__ import annotations

import pathlib
import struct
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from zephyr_wire import (  # noqa: E402
    FLAG_CONTROL,
    FRAME_TYPE_DATA,
    FRAME_TYPE_PING,
    PeerSession,
    V3_MAGIC,
    WireFormatError,
    decode_control_ping,
    encode_control_ping,
)
from zephyr_wire.crc import crc32_ieee  # noqa: E402
from zephyr_wire.v2_codec import encode_v2_frame  # noqa: E402
from zephyr_wire.v3_codec import encode_v3_frame  # noqa: E402


class WireV3SupersessionVerifierTests(unittest.TestCase):
    def test_v3_wire_envelope_exact_layout(self) -> None:
        nonce = b"\x10\x20\x30\x40\x50\x60\x70\x80"
        seq = 0x01020304
        wire = encode_control_ping(seq, nonce)

        # v3 header (7 bytes) + payload (12 bytes) + crc32 (4 bytes) = 23 bytes
        self.assertEqual(len(wire), 23)
        magic, frame_type, flags, length = struct.unpack(">BBBI", wire[:7])
        self.assertEqual(magic, V3_MAGIC)
        self.assertEqual(frame_type, FRAME_TYPE_PING)
        self.assertEqual(flags, FLAG_CONTROL)
        self.assertEqual(length, 12)
        self.assertEqual(wire[7:11], b"\x01\x02\x03\x04")
        self.assertEqual(wire[11:19], nonce)
        (crc_wire,) = struct.unpack(">I", wire[19:23])
        self.assertEqual(crc_wire, crc32_ieee(wire[:19]))

        self.assertEqual(decode_control_ping(wire), (seq, nonce))

    def test_superseded_v2_frame_is_rejected(self) -> None:
        nonce = b"12345678"
        legacy_v2 = encode_v2_frame(FRAME_TYPE_PING, FLAG_CONTROL, struct.pack("<I", 7) + nonce)
        with self.assertRaises(ValueError):
            decode_control_ping(legacy_v2)

    def test_missing_control_flag_and_wrong_frame_type_rejected(self) -> None:
        payload = struct.pack(">I", 42) + b"abcdefgh"
        no_control_flag = encode_v3_frame(FRAME_TYPE_PING, 0x00, payload)
        with self.assertRaises(ValueError):
            decode_control_ping(no_control_flag)

        wrong_type = encode_v3_frame(FRAME_TYPE_DATA, FLAG_CONTROL, payload)
        with self.assertRaises(ValueError):
            decode_control_ping(wrong_type)

    def test_corrupted_crc_and_bad_arguments_rejected(self) -> None:
        valid = bytearray(encode_control_ping(99, b"01234567"))
        valid[-1] ^= 0x01
        with self.assertRaises(ValueError):
            decode_control_ping(bytes(valid))

        with self.assertRaises(ValueError):
            encode_control_ping(-1, b"01234567")
        with self.assertRaises(ValueError):
            encode_control_ping(0x1_0000_0000, b"01234567")
        with self.assertRaises(ValueError):
            encode_control_ping(1, b"short")
        with self.assertRaises(ValueError):
            decode_control_ping(encode_v3_frame(FRAME_TYPE_PING, FLAG_CONTROL, b"short"))

    def test_peer_session_ping_lifecycle(self) -> None:
        session = PeerSession(initial_seq=0xFFFFFFFE)
        seq1, wire1 = session.issue_ping(b"\x00\x01\x02\x03\x04\x05\x06\x07")
        seq2, wire2 = session.issue_ping(b"\xff\xfe\xfd\xfc\xfb\xfa\xf9\xf8")
        self.assertEqual(seq1, 0xFFFFFFFE)
        self.assertEqual(seq2, 0xFFFFFFFF)
        self.assertEqual(session.inflight_count, 2)

        self.assertEqual(session.verify_inbound_ping(wire2), 0xFFFFFFFF)
        self.assertEqual(session.verify_inbound_ping(wire1), 0xFFFFFFFE)
        self.assertEqual(session.inflight_count, 0)

        with self.assertRaises(WireFormatError):
            session.verify_inbound_ping(wire1)


if __name__ == "__main__":
    unittest.main()
