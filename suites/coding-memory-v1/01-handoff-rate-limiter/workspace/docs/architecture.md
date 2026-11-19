# PulseGate Architecture & Quota Model

## Overview

PulseGate enforces per-tenant ingress quotas at the edge HTTP gateway layer. Each
tenant is evaluated against epoch-aligned windows of length `window_ms`
(`window_ns = window_ms * 1_000_000`), indexed by:

```
current_window = now_ns // window_ns
```

Within an active window, requests draw from `sustained_per_window` first and then
from earned `burst_credits` (`0 <= burst_credits <= burst_cap`). Brand-new tenants
with no prior window record start with `burst_credits = 0`.

## Epoch Window Rollover & Burst-Credit Decay (PG-441)

When a tenant sends a request in `current_window > prev_window`, let:

```
idle_windows = (current_window - prev_window) - 1
pool = prev_burst_credits + max(0, sustained_per_window - prev_used_sustained)
```

`idle_windows` counts full untouched windows with zero traffic strictly between
`prev_window` and `current_window`:

- **`idle_windows == 0` (consecutive active windows):**
  Carried burst credits are clamped to `burst_cap`:
  `carried = min(policy.burst_cap, pool)`.
- **`idle_windows == 1` (single skipped window):**
  Carried burst credits decay to at most `int(policy.idle_carryover_ratio * policy.burst_cap)`
  (with default `idle_carryover_ratio = 0.5`, i.e., `int(0.5 * burst_cap)`):
  `carried = min(int(policy.idle_carryover_ratio * policy.burst_cap), pool)`.
- **`idle_windows >= 2` (two or more idle windows):**
  All carried burst credits expire completely:
  `carried = 0`.

## Client Backoff & `X-RateLimit-Reset-Ms` Rounding

Every gateway response includes `X-RateLimit-Limit`, `X-RateLimit-Remaining`,
and `X-RateLimit-Reset-Ms` (plus `X-RateLimit-Burst-Remaining` when
`expose_burst_headers` is enabled).

`reset_after_ns = (current_window + 1) * window_ns - now_ns` measures the exact
nanoseconds until the active window closes. `X-RateLimit-Reset-Ms` MUST round
`reset_after_ns` **up** using ceiling millisecond conversion (`ns_to_ms_ceil`),
returning `0` only when `reset_after_ns <= 0`. Truncating/flooring sub-millisecond
remainders emits `0` while the exhausted window is still open and causes retry storms.
