# Protocol Wire Compatibility Notes (`v1-text` vs `v2-binary`)

## Legacy `v1-text` Branch (Retired Fixed-Slot Text Gateways)

On the legacy `protocol=v1-text` maintenance branch, text frames were padded to
64-byte boundaries with trailing NUL (`\x00`) bytes by C-string proxies. That
branch stripped trailing `\x00` bytes before digest verification.

> **Warning:** Never apply trailing `\x00` stripping to `protocol=v2-binary`!

## Active `v2-binary` Protocol (`PROTOCOL_V2_BINARY = 2`)

1. **Exact Binary Preservation:**
   `v2-binary` transports arbitrary length-prefixed binary payloads (packed
   big-endian integers such as `0x00000100` or `0x00000000`, serialized structs,
   and `zlib`-compressed streams). Every byte within `payload_len` — including
   trailing `\x00` bytes and all-zero payloads — is significant application data
   and MUST NOT be stripped or modified.

2. **Empty-Frame Header Authentication:**
   Every `v2-binary` frame CRC-32 covers `header.header_tag() + wire_payload`:

   ```python
   zlib.crc32(header.header_tag() + payload) & 0xFFFFFFFF
   ```

   This rule applies uniformly when `len(payload) == 0` (`MSG_ACK`,
   `MSG_HEARTBEAT`) so `stream_id`, `msg_type`, and `flags` remain integrity-protected
   on zero-length control frames and match `frame.pack_wire_frame`.
