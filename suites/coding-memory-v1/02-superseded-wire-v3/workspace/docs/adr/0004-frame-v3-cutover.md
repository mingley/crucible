# ADR-0004: Active Wire Framing Cutover to v3

- **Status:** Accepted (Active Standard — Supersedes ADR-0001)
- **Date:** 2026-01-22

## Decision

All active ZephyrWire control-plane and outbound framing MUST use **v3**
(`zephyr_wire/v3_codec.py`):

1. **Envelope (`v3_codec`):**
   - Magic byte: `0xB7` (`V3_MAGIC`)
   - Header: `>BBBI` (`magic: u8`, `frame_type: u8`, `flags: u8`, big-endian `u32` payload length)
   - Trailer: big-endian `u32` IEEE CRC-32 (`crc32_ieee`) over `header + payload`

2. **Control Ping Frame (`encode_control_ping` / `decode_control_ping`):**
   - `frame_type` MUST equal `FRAME_TYPE_PING` (`0x01`).
   - `flags` MUST have `FLAG_CONTROL` (`0x04`) set:
     - `encode_control_ping` emits `flags = FLAG_CONTROL` (`0x04`).
     - `decode_control_ping` rejects any frame where `(flags & FLAG_CONTROL) == 0`
       (and rejects unknown flag bits outside `FLAG_CONTROL`, or more generally requires `flags & FLAG_CONTROL != 0`).
       Specifically: `encode_control_ping` sets `flags = FLAG_CONTROL` (`0x04`), and `decode_control_ping` requires `(frame.flags & FLAG_CONTROL) != 0`.
   - Payload is fixed at `PING_PAYLOAD_LEN = 12` bytes:
     - Bytes `0..4`: unsigned 32-bit **big-endian** sequence number (`struct.pack(">I", seq)`, `0 <= seq <= 0xFFFFFFFF`).
     - Bytes `4..12`: exact `8`-byte `nonce` (`PING_NONCE_LEN = 8`).
   - Validation errors (negative or `> 0xFFFFFFFF` `seq`, non-`bytes` or non-8-byte `nonce`, non-v3 magic such as `0xA5`, wrong `frame_type`, missing `FLAG_CONTROL` bit, wrong payload length, truncated/trailing bytes, or bad CRC-32) MUST raise `WireFormatError` (a subclass of `ValueError`).
