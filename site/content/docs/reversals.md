+++
title = "Reversals"
description = "Send a settled SEPA direct debit back with pain.007 in Rust: reversal versus refund versus reject, partial reversals and the mandatory OrgnlTxRef."
weight = 5
+++

A **reversal** is you, the creditor, undoing a collection that already settled.
It is your own correction, and it is not the same thing as the two debtor-side
events people often confuse it with:

| What happened | Who starts it | Message you see |
|---|---|---|
| The collection never settled | The bank | `pain.002` with `RJCT` |
| The debtor claims the money back | The debtor | `camt.054` return |
| **You collected in error** | **You** | **`pain.007`** |

A reversal is the *late* option. If the collection has not settled yet, a
[recall](@/docs/recalls.md) — `camt.055` — asks the bank to stop it instead, and
costs nothing. A recall refused with `ARDT` is the bank saying the window has
closed and a reversal is what is left.

Only `pain.007.001.09` is defined for SEPA, so there is no version to choose.

## Reversing what you sent

A reversal has to restate the collection it undoes — the mandate, the creditor
identifier, the scheme, the sequence type, the dates and both parties. Rather
than retyping a dozen fields and risking a mismatch, hand it the objects you
already built:

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

    let xml = Pain007Builder::new("Stadtwerke GmbH", "DD-2026-07-001", "RVSL-2026-07-001")
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

Give back less than you took by setting the reversed amount. Reversing *more*
than was collected is rejected — that would credit the debtor money you never
took.

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

## Why the original reference is required

ISO says a reversal may refer to the original *"by means of references only or
by means of references and a set of elements from the original instruction"* —
so in plain ISO, `OrgnlTxRef` is optional.

The German validation subset makes it mandatory, along with the mandate
reference and signature date inside it. A references-only reversal is therefore
not something a German bank accepts, and `OriginalCollection` is a required
argument here rather than an `Option` you could forget.

Its fields are set in pairs for the same reason: the subset makes
`SvcLvl`/`LclInstrm`/`SeqTp` all mandatory once payment-type information is
present, and a party needs both a name and an account. Building them together
means a half-filled block cannot be expressed.

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

`MS02` — no reason given by the customer — is the ordinary choice for
"collected in error", and is what the German banking industry's own example
uses. `AM05` covers a duplicate, `FRAD` a fraudulent original, `TECH` a
technical fault. Unrecognised codes are carried through rather than rejected,
because ISO revises the list quarterly.

## See also

- [Recalls](@/docs/recalls.md) — the earlier and cheaper option, before settlement
- [Direct debits](@/docs/direct-debits.md) — the collection this undoes
