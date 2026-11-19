# ADR-0006: Peer Disconnect Reason Codes

- **Status:** Accepted
- **Date:** 2026-03-05

`FRAME_TYPE_CLOSE` (`0x03`) carries a 2-byte big-endian close code (`0x0000`
normal closure, `0x0001` protocol error, `0x0002` idle timeout).
