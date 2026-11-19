"""Smoke tests for TalonLedger account registration and FX settlement origination."""

from __future__ import annotations

import unittest

from talon_ledger import LedgerService


class TalonLedgerSmokeTests(unittest.TestCase):
    def test_fx_settlement_posts_balanced_lines(self) -> None:
        svc = LedgerService()
        svc.accounts.register("acct_client_usd", "USD", kind="CLIENT")
        svc.accounts.register("acct_house_usd", "USD", kind="HOUSE")

        rec = svc.post_fx_settlement(
            tx_id="tx_1001",
            client_account="acct_client_usd",
            house_account="acct_house_usd",
            currency="USD",
            principal_minor=10_000,
            fee_bps=105,  # 105 minor units
            rebate_bps=5000,
        )
        self.assertEqual(int(rec.fee_minor), 105)
        self.assertEqual(len(svc.journal.lines), 2)
        self.assertEqual(int(svc.accounts.balance_of(svc.journal, "acct_client_usd")), -10_105)
        self.assertEqual(int(svc.accounts.balance_of(svc.journal, "acct_house_usd")), 10_105)


if __name__ == "__main__":
    unittest.main()
