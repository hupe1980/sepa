+++
title = "Reader contracts"
description = "Strict identity, finite intake limits and faithful monetary observations."
weight = 12
+++

## Strict Identity

Every reader requires one ISO `Document` with exactly one matching body. Scoped
bindings resolve prefix spellings. Foreign namespaces cannot supply standard
fields or unqualified `Ccy`. Missing/unknown namespaces, extra roots and unfinished
trailing input are refused. There is no permissive compatibility mode.

| Reader | Accepted suffixes |
|---|---|
| pain.002 | 001.03, 001.10, 002.03, 003.03 |
| camt.029 | 001.06 |
| camt.052 | 001.02, 001.06, 001.08 |
| camt.053 | 001.02, 001.06, 001.08 |
| camt.054 | 001.08 |

Parsing does not perform inbound XSD validation or establish bank acceptance,
execution or settlement.

## Finite Intake

All `parse_*` functions use shared `ParseLimits`. Matching `parse_*_with_limits`
functions accept positive finite overrides; depth cannot exceed 256. Zero never
means unlimited. Exhaustion yields no partial report.

| Resource | Default |
|---|---|
| Input | 32 MiB |
| Elements, including root/empty | 250,000 |
| Depth, root at one | 256 |
| Decoded text per element | 1 MiB |
| Aggregate decoded text | 16 MiB |
| Attributes per element, including namespace declarations | 64 |
| Decoded individual attribute value | 8 KiB |
| Aggregate decoded attribute values | 8 MiB |

Sizes are UTF-8 bytes. Checks precede retained-tree growth, but tokenizer/decoding
allocations and caller-owned input still exist. Limits are not absolute process-RSS
or CPU guarantees. The reader remains buffered.

## Financial Observations

Amount, currency and direction are independent facts. Missing currency is not EUR;
missing charge direction is not debit. Decoded monetary raw text retains padding
and empty values with XML line-ending normalization, not byte-for-byte source
fragments. Unrepresentable decimals are not rounded. Hundredths are not an ISO
currency minor-unit policy.

Signing refuses missing/unusable currency, negative magnitudes and contradictory
raw/typed observations. Charge sums require comparable currencies, known directions
and agreement with a reported aggregate. Aggregate-only charges are not zero.
Unknown inclusion is not permission to post an extra charge.

Net totals require comparable currencies and an exact representable result;
overflow never saturates. Currency syntax checks do not prove registry membership
or PSP support. Equivalent amounts and transfer currencies remain independent,
and status-less money/count buckets remain visible.

## Located Financial Diagnostics

All five document types expose `financial_diagnostics()`. Each
`FinancialDiagnostic` contains a logical field path, a typed `FinancialIssue`
and optional decoded raw text. Indices address retained records and are zero-based;
transaction-detail indices are flattened, not executable XPath or byte offsets.

Diagnostics distinguish missing facts, invalid decimals, negative magnitudes,
sub-cent precision, overflow, malformed currency/direction, inconsistent typed
values, conflicting choices and incomparable or mismatched aggregates. They inspect
current observations on demand without changing them. An absent optional original
amount is not a missing posting; a control sum needs neither currency nor direction.

| Observation | Contract |
|---|---|
| Original counts and status counts | Retain raw text and optional unsigned values. `InvalidCount`, `Overflow` and `InconsistentValue` distinguish failures; absent counts are not zero. |
| Original control sums | Retain raw text and exact hundredths at group/payment-info level; no currency or posting amount is inferred. |
| Equivalent transfer currency | Checked independently of the equivalent amount currency. |
| Detail `amount_source` and `transaction_amount` | Retain direct/nested routes. Conflicting or incomplete nested facts cannot supply trusted signing. |
| Charge `source` | Mixed flat/record forms remain visible but cannot be aggregated. |
| `legacy_referenced_amount` | Retains a simultaneous historical instructed amount; simultaneous canonical/historical routes are ambiguous even when values agree. |
| `original_settlement_amount` | Independent interbank settlement money, never an instructed-amount fallback. A difference alone is not a conflicting choice. |

Aggregate comparisons require usable comparable facts. Raw observations remain
available when calculation fails. An empty diagnostic list is not XSD validation,
authentication or posting permission.

Serde cannot restore trusted derived detail signing. Re-read the authoritative
document for a trusted detail result; do not inject cached money.
The cache retains its parsed denomination; changing parent currency does not
reinterpret that cached figure. Reconciliation refuses the changed context.
