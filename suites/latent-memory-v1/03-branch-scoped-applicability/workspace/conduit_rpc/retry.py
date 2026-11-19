"""Idempotent retry policy for ConduitRPC calls."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Callable, TypeVar

from .errors import ChecksumMismatchError, RetryExhaustedError

T = TypeVar("T")


@dataclass(frozen=True)
class RetryPolicy:
    max_attempts: int = 3

    def __post_init__(self) -> None:
        if self.max_attempts < 1:
            raise ValueError("max_attempts must be >= 1")


def execute_with_retry(policy: RetryPolicy, fn: Callable[[], T]) -> T:
    last_err: Exception | None = None
    for _ in range(policy.max_attempts):
        try:
            return fn()
        except ChecksumMismatchError as err:
            last_err = err
    raise RetryExhaustedError("RPC failed checksum verification across all attempts") from last_err
