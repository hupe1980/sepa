+++
title = "Direct debits"
description = "Build SEPA Direct Debit files (pain.008) in Rust: CORE and B2B, FRST and RCUR in one file, mandate references, amendments and the SMNDA marker."
weight = 4
+++

A direct debit is money you collect. The message is `pain.008`, defaulting to
`pain.008.001.08`. Every collection needs a **mandate** (the debtor's
authorisation) and a **Creditor Identifier**, which `DirectDebitGroup::new`
takes as an argument.

## A realistic collection run

`SeqTp` sits at payment-group level, so first and recurring collections go in
separate groups of the same file.

```rust
use sepa::{
    DirectDebitEntry, DirectDebitGroup, IsoDate, Pain008Builder, SequenceType,
    validate_creditor_id, validate_iban,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let creditor = validate_iban("DE89370400440532013000")?;
    let debtor = validate_iban("NL91ABNA0417164300")?;
    let ci = validate_creditor_id("DE98ZZZ09999999999")?;

    let xml = Pain008Builder::new(
        "Stadtwerke GmbH",
        "DD-2026-07-001",
        "2026-07-15T09:00:00".parse()?,
    )
        .add_group(
            DirectDebitGroup::new("Stadtwerke GmbH", &creditor, &ci, IsoDate::new(2026, 7, 20)?)
                .sequence_type(SequenceType::Frst)
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
            DirectDebitGroup::new("Stadtwerke GmbH", &creditor, &ci, IsoDate::new(2026, 7, 18)?)
                .sequence_type(SequenceType::Rcur)
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
| `Core` | Consumers and businesses | Default. Eight-week no-reason refund for authorised collections |
| `B2b` | Businesses only | No no-reason refund for authorised collections; the debtor's bank verifies the mandate |

Both can appear in one file, as separate groups:

```rust
use sepa::pain008::DirectDebitScheme;
use sepa::{DirectDebitGroup, IsoDate, validate_creditor_id, validate_iban};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iban = validate_iban("DE89370400440532013000")?;
    let ci = validate_creditor_id("DE98ZZZ09999999999")?;

    let b2b = DirectDebitGroup::new("Stadtwerke GmbH", &iban, &ci, IsoDate::new(2026, 7, 20)?)
        .scheme(DirectDebitScheme::B2b);

    assert_eq!(b2b.entry_count(), 0);
    Ok(())
}
```

## Mandate amendments

Report applicable mandate changes on the next collection. Missing or invalid
mandate information can result in an `MD02` rejection.

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
`mandate_id_changed` and `debtor_iban_changed`. The crate rejects an amendment
carrying no actual change.

> **`SMNDA` placement.** *Same mandate, new debtor account* goes in
> `OrgnlDbtrAcct` (EPC placement since November 2016); the legacy German schema
> only allows `OrgnlDbtrAgt`, which the crate uses when that schema is
> selected. Stating the previous IBAN and marking `SMNDA` fill the same
> element, so setting both is an error.

An amendment does **not** reset the sequence type to `FRST`. Carry on with
`RCUR` if that is where the mandate was.

## See also

- [Recalls](/docs/recalls/) — stopping this file *before* it settles
- [Reversals](/docs/reversals/) — sending a settled collection back
- [Bank statements](/docs/bank-statements/) — matching a batch booking to this file
