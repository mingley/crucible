"""FX settlement records, fee origin calculators, and reversal receipt models."""

from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal

from .money import MinorUnits
from .rounding import round_half_up_minor

BPS_DENOMINATOR = Decimal("10000")


@dataclass(frozen=True)
class FXSettlementRecord:
    tx_id: str
    settlement_entry_id: str
    client_account: str
    house_account: str
    currency: str
    principal_minor: MinorUnits
    fee_minor: MinorUnits
    rebate_bps: int


@dataclass(frozen=True)
class ReversalReceipt:
    reversal_entry_id: str
    tx_id: str
    idempotency_key: str
    principal_minor: MinorUnits
    fee_rebate_minor: MinorUnits
    refund_minor: MinorUnits
    currency: str


def compute_origination_fee_minor(principal_minor: MinorUnits, fee_bps: int) -> MinorUnits:
    """Origination fees round half-up per commercial origination schedule."""
    if fee_bps < 0:
        raise ValueError("fee_bps must be non-negative")
    raw = (Decimal(int(principal_minor)) * Decimal(fee_bps)) / BPS_DENOMINATOR
    return round_half_up_minor(raw)
