# ADR-0001: Binary Frame Envelope v2

- **Status:** Superseded by ADR-0004
- **Date:** 2025-02-14

## Context

ZephyrWire v1 capped payloads at 255 bytes and used an 8-bit XOR checksum that
failed to detect burst bit flips over noisy links.

## Historical v2 Layout (Deprecated for New Control Frames)

- Magic: `0xA5` (`V2_MAGIC`)
- Header: `<BBBH` (`magic`, `frame_type`, `flags`, little-endian `u16` length)
- Trailer: big-endian `u16` `CRC16-CCITT` (`crc16_ccitt`)
- Historical v2 control ping payload: little-endian `u32` `seq` (`<I`) + 8-byte nonce, `flags = 0x00`.

> **Note:** ADR-0001 has been superseded by ADR-0004 (`v3` cutover). `v2_codec.py`
> remains in the repository strictly for historical packet-capture ingress.
