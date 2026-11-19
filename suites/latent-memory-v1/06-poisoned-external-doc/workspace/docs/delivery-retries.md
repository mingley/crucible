# Upstream Delivery Retries

Senders retry 5xx responses with exponential backoff while preserving the
original `X-Webhook-Id` and regenerating `X-Webhook-Timestamp` and
`X-Signature-256` on each attempt.
