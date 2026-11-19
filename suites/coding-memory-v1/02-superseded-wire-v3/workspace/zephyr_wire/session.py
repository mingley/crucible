"""Stateful peer session tracking sequence numbers and liveness pings."""

from __future__ import annotations

from .constants import PING_NONCE_LEN, U32_MAX, WireFormatError
from .router import decode_control_ping, encode_control_ping


class PeerSession:
    """Tracks outbound control pings and verifies echoed ping frames."""

    def __init__(self, initial_seq: int = 1) -> None:
        if not (0 <= initial_seq <= U32_MAX):
            raise WireFormatError("initial_seq out of u32 range")
        self._next_seq = initial_seq
        self._inflight: dict[int, bytes] = {}

    @property
    def inflight_count(self) -> int:
        return len(self._inflight)

    def issue_ping(self, nonce: bytes) -> tuple[int, bytes]:
        if not isinstance(nonce, (bytes, bytearray)) or len(nonce) != PING_NONCE_LEN:
            raise WireFormatError("nonce must be exactly 8 bytes")
        seq = self._next_seq
        self._next_seq = (self._next_seq + 1) & U32_MAX
        nonce_bytes = bytes(nonce)
        wire = encode_control_ping(seq, nonce_bytes)
        self._inflight[seq] = nonce_bytes
        return seq, wire

    def verify_inbound_ping(self, raw: bytes) -> int:
        seq, nonce = decode_control_ping(raw)
        expected = self._inflight.get(seq)
        if expected is None or expected != nonce:
            raise WireFormatError("unmatched ping sequence or nonce")
        del self._inflight[seq]
        return seq
