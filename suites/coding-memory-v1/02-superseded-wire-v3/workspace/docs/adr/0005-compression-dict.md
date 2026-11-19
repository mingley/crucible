# ADR-0005: Static Dictionary Prewarming for Telemetry Streams

- **Status:** Proposed
- **Date:** 2026-02-10

Optional `FLAG_COMPRESSED` (`0x01`) data frames may negotiate a 4 KiB preset
dictionary during stream handshake. Control frames are never compressed.
