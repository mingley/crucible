"""Answer-isolated verifier tests for TalonLedger FX reversal conventions."""

from __future__ import annotations

import pathlib
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from talon_ledger import (  # noqa: E402
    AlreadyReversedError,
    IdempotencyConflictError,
    LedgerService,
    TransactionNotFoundError,
)


class TalonLedgerConventionVerifierTests(unittest.TestCase):
    def setUp(self) -> None:
        self.svc = LedgerService()
        self.svc.accounts.register("acct_client_usd", "USD", kind="CLIENT")
        self.svc.accounts.register("acct_house_usd", "USD", kind="HOUSE")

    def test_bankers_half_even_rounding_on_halfway_fee_rebates(self) -> None:
        # fee_minor = 105, rebate_bps = 5000 -> 52.5 -> ROUND_HALF_EVEN = 52 (not 53!)
        self.svc.post_fx_settlement(
            tx_id="tx_even_down",
            client_account="acct_client_usd",
            house_account="acct_house_usd",
            currency="USD",
            principal_minor=10_000,
            fee_bps=105,
            rebate_bps=5000,
        )
        r1 = self.svc.post_fx_reversal("tx_even_down", "  REV-KEY-01 ", "duplicate wire")
        self.assertEqual(int(r1.fee_rebate_minor), 52)
        self.assertEqual(int(r1.refund_minor), 10_052)
        self.assertEqual(r1.idempotency_key, "rev-key-01")

        # fee_minor = 107, rebate_bps = 5000 -> 53.5 -> ROUND_HALF_EVEN = 54
        self.svc.post_fx_settlement(
            tx_id="tx_even_up",
            client_account="acct_client_usd",
            house_account="acct_house_usd",
            currency="USD",
            principal_minor=10_000,
            fee_bps=107,
            rebate_bps=5000,
        )
        r2 = self.svc.post_fx_reversal("tx_even_up", "REV-KEY-02", "counterparty recall")
        self.assertEqual(int(r2.fee_rebate_minor), 54)
        self.assertEqual(int(r2.refund_minor), 10_054)

    def test_append_only_double_entry_and_audit_event(self) -> None:
        self.svc.post_fx_settlement(
            tx_id="tx_200",
            client_account="acct_client_usd",
            house_account="acct_house_usd",
            currency="USD",
            principal_minor=20_000,
            fee_bps=100,  # fee = 200
            rebate_bps=7500,  # rebate = 150
        )
        before_lines = list(self.svc.journal.lines)
        self.assertEqual(len(before_lines), 2)

        receipt = self.svc.post_fx_reversal("tx_200", "Idemp-ABC", "  client cancel  ")
        after_lines = list(self.svc.journal.lines)

        # Historical rows untouched; exactly two offsetting rows appended
        self.assertEqual(after_lines[:2], before_lines)
        self.assertEqual(len(after_lines), 4)
        rev_debit, rev_credit = after_lines[2], after_lines[3]
        self.assertEqual(rev_debit.account_id, "acct_house_usd")
        self.assertEqual(int(rev_debit.signed_minor), -20_150)
        self.assertEqual(rev_credit.account_id, "acct_client_usd")
        self.assertEqual(int(rev_credit.signed_minor), 20_150)
        self.assertEqual(rev_debit.tx_id, "tx_200")
        self.assertEqual(rev_credit.tx_id, "tx_200")
        self.assertEqual(int(receipt.refund_minor), 20_150)

        # Net ledger across all accounts sums to zero; client bears only non-rebated 50 minor fee
        self.assertEqual(int(self.svc.accounts.balance_of(self.svc.journal, "acct_client_usd")), -50)
        self.assertEqual(int(self.svc.accounts.balance_of(self.svc.journal, "acct_house_usd")), 50)

        # Audit event emitted with kind="FX_REVERSAL"
        self.assertEqual(len(self.svc.audit.events), 2)
        audit_ev = self.svc.audit.events[-1]
        self.assertEqual(audit_ev.kind, "FX_REVERSAL")
        self.assertEqual(audit_ev.tx_id, "tx_200")
        self.assertEqual(audit_ev.idempotency_key, "idemp-abc")
        self.assertEqual(int(audit_ev.amount_minor), 20_150)
        self.assertEqual(audit_ev.reason, "client cancel")

    def test_idempotency_normalization_replay_and_conflicts(self) -> None:
        self.svc.post_fx_settlement("tx_a", "acct_client_usd", "acct_house_usd", "USD", 5_000, 100)
        self.svc.post_fx_settlement("tx_b", "acct_client_usd", "acct_house_usd", "USD", 6_000, 100)

        first = self.svc.post_fx_reversal("tx_a", "  Key-Mixed-Case-99\n", "reason one")
        line_count = len(self.svc.journal.lines)
        event_count = len(self.svc.audit.events)

        # Case/whitespace-variant replay on same tx_id returns identical receipt with zero side effects
        replay = self.svc.post_fx_reversal("tx_a", "key-mixed-case-99", "reason one")
        self.assertEqual(replay, first)
        self.assertEqual(len(self.svc.journal.lines), line_count)
        self.assertEqual(len(self.svc.audit.events), event_count)

        # Reusing same normalized key on a DIFFERENT tx_id -> IdempotencyConflictError
        with self.assertRaises(IdempotencyConflictError):
            self.svc.post_fx_reversal("tx_b", "  KEY-MIXED-CASE-99 ", "other")

        # Reversing already-reversed tx_a with a DIFFERENT key -> AlreadyReversedError
        with self.assertRaises(AlreadyReversedError):
            self.svc.post_fx_reversal("tx_a", "brand-new-key", "duplicate attempt")

        # Unknown tx_id -> TransactionNotFoundError
        with self.assertRaises(TransactionNotFoundError):
            self.svc.post_fx_reversal("tx_missing", "unused-key", "unknown")

        # Blank idempotency_key or blank reason -> ValueError
        with self.assertRaises(ValueError):
            self.svc.post_fx_reversal("tx_b", "   ", "valid reason")
        with self.assertRaises(ValueError):
            self.svc.post_fx_reversal("tx_b", "valid-key", "   ")


if __name__ == "__main__":
    unittest.main()
