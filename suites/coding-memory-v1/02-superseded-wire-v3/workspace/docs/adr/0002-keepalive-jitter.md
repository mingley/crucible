# ADR-0002: Keepalive Jitter Bounds

- **Status:** Accepted
- **Date:** 2025-04-03

Idle peer connections schedule control pings every `15s` with uniform jitter in
`[-1.5s, +1.5s]` to prevent synchronized heartbeat spikes after regional
failovers.
