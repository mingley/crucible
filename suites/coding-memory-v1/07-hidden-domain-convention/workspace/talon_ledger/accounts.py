"""Chart of accounts registry and balance queries for TalonLedger."""

from __future__ import annotations

from dataclasses import dataclass

from .journal import Journal
from .money import MinorUnits, as_minor_units, validate_currency


@dataclass(frozen=True)
class Account:
    account_id: str
    currency: str
    kind: str  # "CLIENT" or "HOUSE"


class AccountBook:
    """Maintains registered ledger accounts and computes balances from journal lines."""

    def __init__(self) -> None:
        self._accounts: dict[str, Account] = {}

    def register(self, account_id: str, currency: str, kind: str = "CLIENT") -> Account:
        acct = Account(account_id=account_id, currency=validate_currency(currency), kind=kind)
        self._accounts[account_id] = acct
        return acct

    def require_pair(self, acct_a: str, acct_b: str, currency: str) -> None:
        norm_ccy = validate_currency(currency)
        for aid in (acct_a, acct_b):
            if aid not in self._accounts:
                raise KeyError(f"unknown account {aid!r}")
            if self._accounts[aid].currency != norm_ccy:
                raise ValueError(f"currency mismatch for account {aid!r}")

    def balance_of(self, journal: Journal, account_id: str) -> MinorUnits:
        if account_id not in self._accounts:
            raise KeyError(f"unknown account {account_id!r}")
        total = sum(
            int(line.signed_minor)
            for line in journal.lines
            if line.account_id == account_id
        )
        return as_minor_units(total)
