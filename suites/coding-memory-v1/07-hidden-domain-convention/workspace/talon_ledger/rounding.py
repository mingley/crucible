"""Deterministic Decimal-to-MinorUnits rounding primitives."""

from __future__ import annotations

from decimal import Decimal, ROUND_HALF_EVEN, ROUND_HALF_UP

from .money import MinorUnits, as_minor_units

_UNIT = Decimal("1")


def round_half_up_minor(amount: Decimal) -> MinorUnits:
    """Standard commercial half-up rounding used for initial fee collection."""
    quantized = amount.quantize(_UNIT, rounding=ROUND_HALF_UP)
    return as_minor_units(int(quantized))


def round_half_even_minor(amount: Decimal) -> MinorUnits:
    """Banker's half-even rounding in integer minor units."""
    quantized = amount.quantize(_UNIT, rounding=ROUND_HALF_EVEN)
    return as_minor_units(int(quantized))
