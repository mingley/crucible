"""Immutable append-only double-entry journal for TalonLedger."""

from __future__ import annotations

from dataclasses import dataclass

from .money import MinorUnits, as_minor_units, validate_currency


@dataclass(frozen=True)
class JournalLine:
    line_id: int
    entry_id: str
    tx_id: str
    account_id: str
    signed_minor: MinorUnits
    currency: str
    memo: str


class Journal:
    """Append-only double-entry ledger enforcing zero-sum balanced entries."""

    def __init__(self) -> None:
        self._lines: list[JournalLine] = []
        self._next_entry_seq = 1

    @property
    def lines(self) -> tuple[JournalLine, ...]:
        return tuple(self._lines)

    def lines_for_tx(self, tx_id: str) -> list[JournalLine]:
        return [line for line in self._lines if line.tx_id == tx_id]

    def append_balanced_pair(
        self,
        tx_id: str,
        debit_account: str,
        credit_account: str,
        amount_minor: MinorUnits,
        currency: str,
        memo: str,
    ) -> str:
        """Append two offsetting JournalLine rows (-amount on debit, +amount on credit)."""
        amt = int(as_minor_units(amount_minor))
        if amt <= 0:
            raise ValueError("amount_minor must be strictly positive")
        if debit_account == credit_account:
            raise ValueError("debit and credit accounts must be distinct")
        norm_ccy = validate_currency(currency)
        entry_id = f"ent_{self._next_entry_seq:06d}"
        self._next_entry_seq += 1

        debit_line = JournalLine(
            line_id=len(self._lines) + 1,
            entry_id=entry_id,
            tx_id=tx_id,
            account_id=debit_account,
            signed_minor=as_minor_units(-amt),
            currency=norm_ccy,
            memo=memo,
        )
        self._lines.append(debit_line)

        credit_line = JournalLine(
            line_id=len(self._lines) + 1,
            entry_id=entry_id,
            tx_id=tx_id,
            account_id=credit_account,
            signed_minor=as_minor_units(amt),
            currency=norm_ccy,
            memo=memo,
        )
        self._lines.append(credit_line)
        return entry_id
