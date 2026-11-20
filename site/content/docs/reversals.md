+++
title = "Reversals"
description = "Send a settled SEPA direct debit back with pain.007 in Rust: reversal versus refund versus reject, partial reversals and the mandatory OrgnlTxRef."
weight = 5
+++

A **reversal** is the creditor undoing a collection that already settled. It
differs from a reject or a return:

| What happened | Who starts it | Message you see |
|---|---|---|
| The collection never settled | The bank | `pain.002` with `RJCT` |
| The debtor claims the money back | The debtor | `camt.054` return |
| **You collected in error** | **You** | **`pain.007`** |

A [recall](@/docs/recalls.md) (`camt.055`) instead asks the bank to cancel a
payment before it settles. Availability, deadlines and fees depend on the bank
and scheme; an `ARDT` recall refusal reports an existing return, not permission
to reverse. SEPA defines only `pain.007.001.09`, so there is no version to
choose.

## Reversing what you sent

A reversal restates the collection it undoes — mandate, creditor identifier,
scheme, sequence type, dates and both parties. `ReversalEntry::reverse` takes
them from the group and entry you already built:

```rust
use sepa::{
    DirectDebitEntry, DirectDebitGroup, IsoDate, Pain007Builder, ReversalEntry,
    ReversalGroup, ReversalReason, validate_creditor_id, validate_iban,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let creditor = validate_iban("DE89370400440532013000")?;
    let debtor = validate_iban("NL91ABNA0417164300")?;
    let ci = validate_creditor_id("DE98ZZZ09999999999")?;

    // The collection that went out last week.
    let group = DirectDebitGroup::new("Stadtwerke GmbH", &creditor, &ci, IsoDate::new(2026, 7, 20)?);
    let entry = DirectDebitEntry::new(
        "MND-42",
        "2024-06-01".parse()?,
        "Max Mustermann",
        debtor,
        7_500,
        "E2E-1",
    );

    let xml = Pain007Builder::new(
        "Stadtwerke GmbH",
        "RVSL-2026-07-001",
        "2026-07-15T09:00:00".parse()?,
        "DD-2026-07-001",
    )
        .add_group(
            ReversalGroup::new("DD-2026-07-001")
                .add_entry(ReversalEntry::reverse(&group, &entry, ReversalReason::Ms02)),
        )
        .build()?;

    assert!(xml.contains("<RvsdInstdAmt Ccy=\"EUR\">75.00</RvsdInstdAmt>"));
    assert!(xml.contains("<MndtId>MND-42</MndtId>"));
    Ok(())
}
```

`Pain007Builder::new` takes the `MsgId` of the original message, and
`ReversalGroup::new` the `PmtInfId` of the group being reversed out of. For a
single-group collection that identifier is the message's own `MsgId`.

## Partial reversals

Set the reversed amount to give back less than was collected. Reversing *more*
than was collected is rejected.

```rust
use sepa::{OriginalCollection, ReversalEntry, ReversalReason};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let entry = ReversalEntry::new(
        "E2E-1",
        7_500, // what was collected
        ReversalReason::Ms02,
        OriginalCollection::new("MND-42", "2024-06-01".parse()?),
    )
    .reversed_amount(2_500); // what goes back

    assert_eq!(entry.effective_amount_ct(), 2_500);
    Ok(())
}
```

## The original reference is required

ISO makes `OrgnlTxRef` optional; the German validation subset makes it
mandatory, including the mandate reference and signature date. So
`OriginalCollection` is a required argument.

Its fields are set together: once payment-type information is present the subset
requires `SvcLvl`, `LclInstrm` and `SeqTp`, and a party needs both a name and an
account.

```rust
use sepa::{DirectDebitScheme, OriginalCollection, SequenceType, validate_iban};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let debtor = validate_iban("NL91ABNA0417164300")?;

    let original = OriginalCollection::new("MND-42", "2024-06-01".parse()?)
        .collection_date("2026-07-20".parse()?)
        .payment_type(DirectDebitScheme::Core, SequenceType::Frst)
        .debtor("Max Mustermann", debtor);

    assert_eq!(original.mandate_ref(), "MND-42");
    Ok(())
}
```

## Reasons

`MS02` (no reason given by the customer) is the usual choice for "collected in
error" and the one in the German banking industry's example. `AM05` covers a
duplicate, `FRAD` a fraudulent original, `TECH` a technical fault. Unrecognised
codes are carried through, since ISO revises the list quarterly.

## See also

- [Recalls](@/docs/recalls.md) — requesting cancellation of a submitted payment
- [Direct debits](@/docs/direct-debits.md) — the collection this undoes
