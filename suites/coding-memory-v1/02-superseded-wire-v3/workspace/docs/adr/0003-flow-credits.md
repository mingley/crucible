# ADR-0003: Receiver Window Flow Credits

- **Status:** Accepted
- **Date:** 2025-06-19

Each multiplexed stream starts with 64 KiB of receiver flow credits and
replenishes in 16 KiB increments once half the window is drained.
