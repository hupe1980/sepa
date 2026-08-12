+++
title = "Direct debits"
description = "Build SEPA Direct Debit files (pain.008) in Rust: CORE and B2B schemes, FRST and RCUR sequence types in one file, mandate references, mandate amendments and the SMNDA marker."
weight = 3
+++

A direct debit is money you collect. The message is `pain.008`, and the crate
defaults to `pain.008.001.08`.

Every collection needs a **mandate** — the debtor's authorisation — and a
**Creditor Identifier**, which is why `DirectDebitGroup::new` takes one up
front rather than letting you forget it.

## A realistic collection run

`SeqTp` sits at payment-group level, so first collections and recurring ones
each need their own group. That is what lets one file carry a whole run.

```rust
use sepa::{
    DirectDebitEntry, DirectDebitGroup, IsoDate, Pain008Builder, SequenceType,
    validate_creditor_id, validate_iban,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let creditor = validate_iban("DE89370400440532013000")?;
    let debtor = validate_iban("NL91ABNA0417164300")?;
    let ci = validate_creditor_id("DE98ZZZ09999999999")?;

    let xml = Pain008Builder::new("Stadtwerke GmbH")
        .msg_id("DD-2026-07-001")
        .add_group(
            DirectDebitGroup::new("Stadtwerke GmbH", &creditor, &ci)
                .sequence_type(SequenceType::Frst)
                .collection_date(IsoDate::new(2026, 7, 20)?)
                .add_entry(DirectDebitEntry::new(
                    "MND-1",
                    "2026-06-01".parse()?,
                    "Neu Kunde",
                    debtor.clone(),
                    5_000,
                    "E2E-1",
                )),
        )
        .add_group(
            DirectDebitGroup::new("Stadtwerke GmbH", &creditor, &ci)
                .sequence_type(SequenceType::Rcur)
                .collection_date(IsoDate::new(2026, 7, 18)?)
                .add_entry(DirectDebitEntry::new(
                    "MND-2",
                    "2024-06-01".parse()?,
                    "Alt Kunde",
                    debtor,
                    7_500,
                    "E2E-2",
                )),
        )
        .build()?;

    assert!(xml.contains("<SeqTp>FRST</SeqTp>"));
    assert!(xml.contains("<SeqTp>RCUR</SeqTp>"));
    Ok(())
}
```

| Sequence type | Meaning |
|---|---|
| `FRST` | First collection under a newly activated mandate |
| `RCUR` | Every subsequent collection |
| `FNAL` | Final collection; the mandate ends after it |
| `OOFF` | One-off — no `FRST`/`RCUR` lifecycle at all |

## CORE and B2B

| Scheme | Debtor | Notes |
|---|---|---|
| `Core` | Consumers | Default. Refundable for eight weeks without reason |
| `B2b` | Businesses only | Shorter cycle, no no-questions refund; the debtor's bank must hold the mandate |

Both can appear in the same file, again as separate groups:

```rust
use sepa::pain008::DirectDebitScheme;
use sepa::{DirectDebitGroup, validate_creditor_id, validate_iban};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iban = validate_iban("DE89370400440532013000")?;
    let ci = validate_creditor_id("DE98ZZZ09999999999")?;

    let b2b = DirectDebitGroup::new("Stadtwerke GmbH", &iban, &ci)
        .scheme(DirectDebitScheme::B2b);

    assert_eq!(b2b.entry_count(), 0);
    Ok(())
}
```

## Mandate amendments

When anything about the mandate changes, the next collection must say so.
Omitting the amendment gets the collection rejected with `MD02`.

```rust
use sepa::{DirectDebitEntry, MandateAmendment, validate_iban};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let debtor = validate_iban("NL91ABNA0417164300")?;

    let entry = DirectDebitEntry::new(
        "MND-1",
        "2024-06-01".parse()?,
        "Max Mustermann",
        debtor,
        7_500,
        "E2E-1",
    )
    // The debtor moved to a different account or bank.
    .with_amendment(MandateAmendment::debtor_account_changed());

    assert!(entry.amendment.is_some());
    Ok(())
}
```

The other cases are `creditor_id_changed`, `creditor_name_changed`,
`mandate_id_changed` and `debtor_iban_changed`. An amendment carrying no actual
change is rejected, because the debtor's bank rejects it too.

> **`SMNDA` moved.** *Same mandate, new debtor account* went in
> `OrgnlDbtrAgt` before November 2016 and in `OrgnlDbtrAcct` after. The crate
> emits the current placement, and automatically switches to the old one when
> you select the legacy German schema, whose type system only allows the old
> form. An amendment cannot both state the previous IBAN and mark it `SMNDA` —
> they fill the same element, and setting both is an error rather than a silent
> drop.

An amendment does **not** reset the sequence type to `FRST`. Carry on with
`RCUR` if that is where the mandate was.

## See also

- [Reversals](/docs/reversals/) — sending a settled collection back
- [Bank statements](/docs/bank-statements/) — matching a batch booking to this file
