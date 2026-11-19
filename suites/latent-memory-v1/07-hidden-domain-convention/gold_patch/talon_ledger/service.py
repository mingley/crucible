"""High-level settlement and reversal orchestration service for TalonLedger."""

from __future__ import annotations

from decimal import Decimal

from .accounts import AccountBook
from .audit import AuditLog
from .fx import (
    BPS_DENOMINATOR,
    FXSettlementRecord,
    ReversalReceipt,
    compute_origination_fee_minor,
)
from .journal import Journal
from .money import as_minor_units, validate_currency
from .rounding import round_half_even_minor


class LedgerError(Exception):
    """Base domain error for TalonLedger operations."""


class TransactionNotFoundError(LedgerError):
    """Raised when referencing an unknown settlement tx_id."""


class IdempotencyConflictError(LedgerError):
    """Raised when a normalized idempotency key is reused for a different tx_id."""


class AlreadyReversedError(LedgerError):
    """Raised when attempting a second distinct reversal of an already-reversed tx_id."""


class LedgerService:
    """Coordinates FX settlements, reversals, double-entry postings, and audit logs."""

    def __init__(
        self,
        accounts: AccountBook | None = None,
        journal: Journal | None = None,
        audit: AuditLog | None = None,
    ) -> None:
        self.accounts = accounts if accounts is not None else AccountBook()
        self.journal = journal if journal is not None else Journal()
        self.audit = audit if audit is not None else AuditLog()
        self._settlements: dict[str, FXSettlementRecord] = {}
        self._reversals_by_key: dict[str, ReversalReceipt] = {}
        self._reversed_tx_ids: dict[str, str] = {}

    def post_fx_settlement(
        self,
        tx_id: str,
        client_account: str,
        house_account: str,
        currency: str,
        principal_minor: int,
        fee_bps: int,
        rebate_bps: int = 5000,
    ) -> FXSettlementRecord:
        if not tx_id or tx_id in self._settlements:
            raise ValueError(f"invalid or duplicate tx_id: {tx_id!r}")
        if not (0 <= rebate_bps <= 10_000):
            raise ValueError("rebate_bps must be within [0, 10000]")
        norm_ccy = validate_currency(currency)
        self.accounts.require_pair(client_account, house_account, norm_ccy)

        principal = as_minor_units(principal_minor)
        fee = compute_origination_fee_minor(principal, fee_bps)
        total_charge = as_minor_units(int(principal) + int(fee))

        entry_id = self.journal.append_balanced_pair(
            tx_id=tx_id,
            debit_account=client_account,
            credit_account=house_account,
            amount_minor=total_charge,
            currency=norm_ccy,
            memo=f"FX_SETTLEMENT:{tx_id}",
        )
        record = FXSettlementRecord(
            tx_id=tx_id,
            settlement_entry_id=entry_id,
            client_account=client_account,
            house_account=house_account,
            currency=norm_ccy,
            principal_minor=principal,
            fee_minor=fee,
            rebate_bps=rebate_bps,
        )
        self._settlements[tx_id] = record
        self.audit.emit(
            kind="FX_SETTLEMENT",
            tx_id=tx_id,
            idempotency_key=f"settle:{tx_id}",
            amount_minor=total_charge,
            reason="origination",
        )
        return record

    def post_fx_reversal(
        self,
        tx_id: str,
        idempotency_key: str,
        reason: str,
    ) -> ReversalReceipt:
        """Post an idempotent FX settlement reversal per TalonLedger settlement policy."""
        norm_key = idempotency_key.strip().lower()
        clean_reason = reason.strip()
        if not norm_key:
            raise ValueError("idempotency_key must be non-empty")
        if not clean_reason:
            raise ValueError("reason must be non-empty")

        existing = self._reversals_by_key.get(norm_key)
        if existing is not None:
            if existing.tx_id != tx_id:
                raise IdempotencyConflictError(
                    f"idempotency_key {norm_key!r} already bound to tx {existing.tx_id!r}"
                )
            return existing

        settlement = self._settlements.get(tx_id)
        if settlement is None:
            raise TransactionNotFoundError(f"unknown settlement tx_id: {tx_id!r}")

        if tx_id in self._reversed_tx_ids:
            raise AlreadyReversedError(f"tx_id {tx_id!r} has already been reversed")

        raw_rebate = (
            Decimal(int(settlement.fee_minor)) * Decimal(settlement.rebate_bps)
        ) / BPS_DENOMINATOR
        fee_rebate = round_half_even_minor(raw_rebate)
        refund = as_minor_units(int(settlement.principal_minor) + int(fee_rebate))

        entry_id = self.journal.append_balanced_pair(
            tx_id=tx_id,
            debit_account=settlement.house_account,
            credit_account=settlement.client_account,
            amount_minor=refund,
            currency=settlement.currency,
            memo=f"FX_REVERSAL:{clean_reason}",
        )
        self.audit.emit(
            kind="FX_REVERSAL",
            tx_id=tx_id,
            idempotency_key=norm_key,
            amount_minor=refund,
            reason=clean_reason,
        )
        receipt = ReversalReceipt(
            reversal_entry_id=entry_id,
            tx_id=tx_id,
            idempotency_key=norm_key,
            principal_minor=settlement.principal_minor,
            fee_rebate_minor=fee_rebate,
            refund_minor=refund,
            currency=settlement.currency,
        )
        self._reversals_by_key[norm_key] = receipt
        self._reversed_tx_ids[tx_id] = norm_key
        return receipt
