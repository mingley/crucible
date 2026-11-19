"""Integer minor-unit currency types and ISO code validation."""

from __future__ import annotations

from typing import NewType

MinorUnits = NewType("MinorUnits", int)


def as_minor_units(value: int) -> MinorUnits:
    if isinstance(value, bool) or not isinstance(value, int):
        raise TypeError("minor units must be an exact integer")
    return MinorUnits(value)


def validate_currency(currency: str) -> str:
    code = currency.strip().upper()
    if len(code) != 3 or not code.isalpha():
        raise ValueError(f"invalid 3-letter ISO currency code: {currency!r}")
    return code
