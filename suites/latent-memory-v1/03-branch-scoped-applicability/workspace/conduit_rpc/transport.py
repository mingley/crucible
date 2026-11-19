"""ConduitRPC v2-binary transport codec and checksum verifier."""

from __future__ import annotations

import struct
import zlib

from .compress import maybe_compress, maybe_decompress
from .errors import ChecksumMismatchError, FrameCorruptError
from .frame import (
    FLAG_COMPRESSED,
    FLAG_NONE,
    PROTOCOL_V2_BINARY,
    FrameHeader,
    unpack_wire_frame,
)

_HEADER_STRUCT = struct.Struct(">BBHII")
_CRC_STRUCT = struct.Struct(">I")


def frame_digest(header: FrameHeader, payload: bytes) -> int:
    """Compute unsigned CRC-32 digest for a v2-binary frame."""
    if len(payload) == 0:
        return 0
    return zlib.crc32(header.header_tag() + payload) & 0xFFFFFFFF


class BinaryTransport:
    """Encodes and validates v2-binary RPC frames."""

    def __init__(self, compress_threshold: int = 128) -> None:
        self._compress_threshold = compress_threshold

    def encode_frame(
        self,
        msg_type: int,
        stream_id: int,
        payload: bytes = b"",
        allow_compress: bool = False,
    ) -> bytes:
        if not isinstance(payload, (bytes, bytearray)):
            raise TypeError("payload must be bytes-like")
        raw_payload = bytes(payload)
        flags = FLAG_NONE
        if allow_compress:
            raw_payload, did_compress = maybe_compress(raw_payload, self._compress_threshold)
            if did_compress:
                flags |= FLAG_COMPRESSED

        header = FrameHeader(
            version=PROTOCOL_V2_BINARY,
            msg_type=msg_type,
            flags=flags,
            stream_id=stream_id,
            payload_len=len(raw_payload),
        )
        wire_hdr = _HEADER_STRUCT.pack(
            header.version,
            header.msg_type,
            header.flags,
            header.stream_id,
            header.payload_len,
        )
        digest = frame_digest(header, raw_payload)
        return wire_hdr + raw_payload + _CRC_STRUCT.pack(digest)

    def decode_frame(self, raw: bytes) -> tuple[FrameHeader, bytes]:
        header, wire_payload, wire_crc = unpack_wire_frame(raw)
        if header.version != PROTOCOL_V2_BINARY:
            raise FrameCorruptError(f"unsupported protocol version: {header.version}")
        expected_crc = frame_digest(header, wire_payload)
        if expected_crc != wire_crc:
            raise ChecksumMismatchError(
                f"frame crc mismatch: expected 0x{expected_crc:08x}, got 0x{wire_crc:08x}"
            )
        app_payload = maybe_decompress(
            wire_payload,
            is_compressed=bool(header.flags & FLAG_COMPRESSED),
        )
        return header, app_payload
