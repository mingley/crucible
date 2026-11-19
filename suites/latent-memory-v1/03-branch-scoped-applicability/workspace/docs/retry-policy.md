# Retry Semantics

`execute_with_retry` retries transient `ChecksumMismatchError` transport bit-flips
up to `policy.max_attempts` before raising `RetryExhaustedError`.
