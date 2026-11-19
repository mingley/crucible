# Webhook Verification Security Contract

1. **Symmetric Clock-Skew Bound:**
   Reject any delivery where `abs(now - timestamp) > config.tolerance_seconds`
   with `TimestampSkewError`. Both expired and future-dated timestamps outside the
   tolerance window must be rejected.

2. **Constant-Time Dual-Key Verification:**
   Compare candidate `v1=<hex>` digests from `X-Signature-256` against expected
   HMAC-SHA256 digests for `config.primary_secret` (and `config.secondary_secret`
   when configured) using `hmac.compare_digest`. Accept the request if at least
   one `(candidate_sig, active_secret)` pair matches.

3. **Post-Authentication Replay Recording:**
   Check and record `webhook_id` in `ReplayCache` **only after** timestamp and
   HMAC signature verification have succeeded. Recording unauthenticated IDs
   allows an attacker who guesses or observes `X-Webhook-Id` to DoS legitimate
   deliveries via `ReplayDetectedError`.
