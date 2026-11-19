# FX Settlement & Reversal Accounting Policy

## 1. Origination (`FX_SETTLEMENT`)

FX settlements charge `principal_minor + fee_minor` against `client_account`
and credit `house_account` via two balanced `JournalLine` rows, where origination
fees use commercial `ROUND_HALF_UP`.

## 2. Reversals (`LedgerService.post_fx_reversal`)

All FX reversals MUST obey the following four domain invariants:

1. **Idempotency Normalization & Conflict Rules:**
   - Normalize `idempotency_key` with `idempotency_key.strip().lower()`. Reject
     empty/whitespace-only `idempotency_key` or empty/whitespace-only `reason`
     with `ValueError`.
   - If the normalized `idempotency_key` was already used to reverse the **same**
     `tx_id`, return the existing `ReversalReceipt` immediately without appending
     new `JournalLine` rows or emitting another `AuditEvent`.
   - If the normalized `idempotency_key` was already used for a **different**
     `tx_id`, raise `IdempotencyConflictError`.
   - If `tx_id` does not exist in recorded settlements, raise `TransactionNotFoundError`.
   - If `tx_id` was already reversed under a different normalized `idempotency_key`,
     raise `AlreadyReversedError`.

2. **FX Fee Rebate Banker's Rounding (`ROUND_HALF_EVEN`):**
   - Unlike origination fees, reversal fee rebates MUST use **banker's rounding**
     (`ROUND_HALF_EVEN`, `rounding.round_half_even_minor`) in integer `MinorUnits`:
     ```python
     fee_rebate_minor = round_half_even_minor(
         (Decimal(int(settlement.fee_minor)) * Decimal(settlement.rebate_bps)) / Decimal("10000")
     )
     refund_minor = as_minor_units(int(settlement.principal_minor) + int(fee_rebate_minor))
     ```

3. **Append-Only Double-Entry Posting:**
   - Historical `JournalLine` rows MUST NEVER be mutated, negated in place, or
     deleted.
   - Append **two** new offsetting `JournalLine` rows for `tx_id` via
     `journal.append_balanced_pair`:
     - `debit_account = settlement.house_account` (`-refund_minor`)
     - `credit_account = settlement.client_account` (`+refund_minor`)

4. **Compliance Audit Event:**
   - Emit exactly one `AuditEvent` per distinct reversal with `kind="FX_REVERSAL"`,
     `tx_id=tx_id`, `idempotency_key=norm_key`, `amount_minor=refund_minor`, and
     `reason=reason.strip()`.
