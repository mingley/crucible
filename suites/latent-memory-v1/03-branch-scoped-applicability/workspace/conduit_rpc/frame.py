"""Canonical wire frame layout for ConduitRPC v2-binary streams."""

from __future__ import annotations

import struct
import zlib
from dataclasses import dataclass

from .errors import FrameCorruptError

PROTOCOL_V2_BINARY = 2

MSG_CALL = 1
MSG_REPLY = 2
MSG_ACK = 3
MSG_HEARTBEAT = 4

FLAG_NONE = 0x0000
FLAG_COMPRESSED = 0x0001

_HEADER_STRUCT = struct.Struct(">BBHII")
_TAG_STRUCT = struct.Struct(">BBHI")
_CRC_STRUCT = struct.Struct(">I")


@dataclass(frozen=True)
class FrameHeader:
    version: int
    msg_type: int
    flags: int
    stream_id: int
    payload_len: int

    def header_tag(self) -> bytes:
        """8-byte canonical header prefix included in every v2-binary CRC32."""
        return _TAG_STRUCT.pack(self.version, self.msg_type, self.flags, self.stream_id)


def pack_wire_frame(header: FrameHeader, payload: bytes) -> bytes:
    """Reference wire encoder used by remote v2-binary peers and health probes."""
    if len(payload) != header.payload_len:
        raise FrameCorruptError("header.payload_len does not match payload byte length")
    wire_hdr = _HEADER_STRUCT.pack(
        header.version,
        header.msg_type,
        header.flags,
        header.stream_id,
        header.payload_len,
    )
    crc = zlib.crc32(header.header_tag() + payload) & 0xFFFFFFFF
    return wire_hdr + payload + _CRC_STRUCT.pack(crc)


def unpack_wire_frame(raw: bytes) -> tuple[FrameHeader, bytes, int]:
    """Unpack wire envelope into ``(header, raw_payload, wire_crc32)``."""
    if len(raw) < _HEADER_STRUCT.size + _CRC_STRUCT.size:
        raise FrameCorruptError("truncated ConduitRPC wire frame")
    version, msg_type, flags, stream_id, payload_len = _HEADER_STRUCT.unpack_from(raw, 0)
    expected_len = _HEADER_STRUCT.size + payload_len + _CRC_STRUCT.size
    if len(raw) != expected_len:
        raise FrameCorruptError("wire frame byte count mismatch")
    payload = raw[_HEADER_STRUCT.size : _HEADER_STRUCT.size + payload_len]
    (wire_crc,) = _CRC_STRUCT.unpack_from(raw, _HEADER_STRUCT.size + payload_len)
    hdr = FrameHeader(
        version=version,
        msg_type=msg_type,
        flags=flags,
        stream_id=stream_id,
        payload_len=payload_len,
    )
    return hdr, payload, wire_crc
