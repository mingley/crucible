"""Answer-isolated verifier tests for ConduitRPC v2-binary checksum applicability."""

from __future__ import annotations

import pathlib
import struct
import sys
import unittest
import zlib

ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from conduit_rpc import (  # noqa: E402
    BinaryTransport,
    ChecksumMismatchError,
    ConnectionPool,
    FLAG_NONE,
    FrameHeader,
    MSG_ACK,
    MSG_CALL,
    MSG_HEARTBEAT,
    PROTOCOL_V2_BINARY,
    pack_wire_frame,
    unpack_wire_frame,
)


class ConduitApplicabilityVerifierTests(unittest.TestCase):
    def test_empty_frames_authenticate_header_tag_and_interoperate_with_pool(self) -> None:
        tx = BinaryTransport()
        pool = ConnectionPool(capacity=4, transport=tx)
        stream_id = pool.acquire_stream()

        hb_wire = pool.build_peer_heartbeat(stream_id=stream_id)
        ack_wire = pool.handle_heartbeat_and_ack(hb_wire)

        ack_hdr, ack_payload, ack_crc = unpack_wire_frame(ack_wire)
        self.assertEqual(ack_hdr.msg_type, MSG_ACK)
        self.assertEqual(ack_hdr.stream_id, stream_id)
        self.assertEqual(ack_payload, b"")
        expected_crc = zlib.crc32(ack_hdr.header_tag()) & 0xFFFFFFFF
        self.assertNotEqual(expected_crc, 0)
        self.assertEqual(ack_crc, expected_crc)

        # Tampering with stream_id on an empty frame must fail checksum validation.
        tampered = bytearray(ack_wire)
        tampered[4:8] = struct.pack(">I", stream_id + 99)
        with self.assertRaises(ChecksumMismatchError):
            tx.decode_frame(bytes(tampered))

    def test_trailing_null_bytes_preserved_exact_in_v2_binary(self) -> None:
        tx = BinaryTransport(compress_threshold=1024)
        payloads = [
            b"\x00",
            b"\x00\x00\x00\x00",
            b"\xde\xad\xbe\xef\x00",
            b"record\x00\x01\x00\x00",
            struct.pack(">II", 256, 0),
        ]
        for idx, raw_payload in enumerate(payloads, start=1):
            # Peer -> BinaryTransport
            hdr = FrameHeader(
                version=PROTOCOL_V2_BINARY,
                msg_type=MSG_CALL,
                flags=FLAG_NONE,
                stream_id=idx,
                payload_len=len(raw_payload),
            )
            peer_wire = pack_wire_frame(hdr, raw_payload)
            decoded_hdr, decoded_body = tx.decode_frame(peer_wire)
            self.assertEqual(decoded_hdr.stream_id, idx)
            self.assertEqual(decoded_body, raw_payload)

            # BinaryTransport -> Peer
            our_wire = tx.encode_frame(MSG_CALL, stream_id=idx, payload=raw_payload)
            self.assertEqual(our_wire, peer_wire)

    def test_corrupted_trailing_null_detected(self) -> None:
        tx = BinaryTransport(compress_threshold=1024)
        wire = bytearray(tx.encode_frame(MSG_CALL, stream_id=12, payload=b"\x10\x20\x00\x00"))
        # Flip penultimate payload byte from 0x00 to 0x01 without updating CRC
        wire[-6] = 0x01
        with self.assertRaises(ChecksumMismatchError):
            tx.decode_frame(bytes(wire))


if __name__ == "__main__":
    unittest.main()
