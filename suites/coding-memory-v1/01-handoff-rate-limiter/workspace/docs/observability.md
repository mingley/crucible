# Metrics & Structured Logging

PulseGate exports counters per tenant tier:

- `pulsegate_requests_allowed_total`
- `pulsegate_requests_throttled_total`
- `pulsegate_burst_tokens_spent_total`

High-cardinality tenant IDs are aggregated into top-K sketches every 10 seconds
to bound time-series cardinality under multi-tenant bursts.
