+++
title = "Credit transfers"
description = "Build SEPA Credit Transfer files (pain.001) in Rust: ordinary transfers, SCT Instant, scheduled instant, structured references and purpose codes."
weight = 3
+++

A credit transfer is money you send. The message is `pain.001`, and the crate
defaults to `pain.001.001.09` — the version the EPC rulebooks mandate.

## Ordinary transfers

```rust
use sepa::{CreditTransferEntry, CreditTransferGroup, IsoDate, Pain001Builder, validate_iban};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let debtor = validate_iban("DE89370400440532013000")?;
    let creditor = validate_iban("NL91ABNA0417164300")?;

    let xml = Pain001Builder::new("Acme GmbH", "CT-2026-07-001")
        .add_group(
            CreditTransferGroup::new("Acme GmbH", &debtor, IsoDate::new(2026, 7, 20)?)
                .add_entry(CreditTransferEntry::new(
                    "Supplier AG",
                    creditor,
                    12_000,
                    "INV-001",
                )),
        )
        .build()?;

    assert!(xml.contains("<ReqdExctnDt><Dt>2026-07-20</Dt></ReqdExctnDt>"));
    Ok(())
}
```

Several groups in one message let a single file carry **different execution
dates** or debtor accounts, instead of forcing a separate submission for each.

## SCT Instant

Instant is a property of the group, not the message:

```rust
use sepa::pain001::LocalInstrument;
use sepa::{CreditTransferEntry, CreditTransferGroup, IsoDate, Pain001Builder, validate_iban};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let debtor = validate_iban("DE89370400440532013000")?;
    let creditor = validate_iban("NL91ABNA0417164300")?;

    let xml = Pain001Builder::new("Acme GmbH", "CT-INST-001")
        .add_group(
            CreditTransferGroup::new("Acme GmbH", &debtor, IsoDate::new(2026, 7, 20)?)
                .local_instrument(LocalInstrument::Inst)
                .add_entry(CreditTransferEntry::new("Payee", creditor, 5_000, "E2E-1")),
        )
        .build()?;

    assert!(xml.contains("<LclInstrm><Cd>INST</Cd></LclInstrm>"));
    Ok(())
}
```

The scheme-wide 100 000 EUR cap on instant transfers was removed on
5 October 2025, so the crate enforces the ordinary SEPA ceiling of
999 999 999.99 EUR for every instrument. A lower limit is now a policy your PSP
sets, not a scheme rule.

## Scheduled instant transfers

`pain.001.001.09` types `ReqdExctnDt` as a date **or** a date-time. The timed
form is what the German banking industry calls a *terminierte
Echtzeitüberweisung*: an instant transfer due at a stated moment rather than
some time during the day.

```rust
use sepa::pain001::LocalInstrument;
use sepa::{CreditTransferEntry, CreditTransferGroup, IsoDateTime, Pain001Builder, validate_iban};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let debtor = validate_iban("DE89370400440532013000")?;
    let creditor = validate_iban("NL91ABNA0417164300")?;
    // A time of day only names an instant once it says which zone it is in.
    let due: IsoDateTime = "2026-07-20T11:00:00Z".parse()?;

    let xml = Pain001Builder::new("Acme GmbH", "CT-TIMED")
        .add_group(
            CreditTransferGroup::new("Acme GmbH", &debtor, due)
                .local_instrument(LocalInstrument::Inst)   // required for a timed execution
                .add_entry(CreditTransferEntry::new("Payee", creditor, 5_000, "E2E-1")),
        )
        .build()?;

    assert!(xml.contains("<ReqdExctnDt><DtTm>2026-07-20T11:00:00Z</DtTm></ReqdExctnDt>"));
    Ok(())
}
```

Two rules travel with the timed form, neither of which any XSD can express —
so a file that breaks them validates cleanly and is rejected on ingestion. The
DK validation subset annotates `DtTm` *"Only allowed for SCTinst"*, with the
usage rule *"Only UTC time format or local time with UTC offset format can be
used"*. `build()` enforces both:

```rust
# use sepa::pain001::LocalInstrument;
# use sepa::{CreditTransferEntry, CreditTransferGroup, IsoDateTime, Pain001Builder, ValidationError, validate_iban};
# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let debtor = validate_iban("DE89370400440532013000")?;
# let creditor = validate_iban("NL91ABNA0417164300")?;
let err = Pain001Builder::new("Acme GmbH", "CT-TIMED")
    .add_group(
        // No local instrument: an ordinary SCT settles some time during the
        // banking day, so a time of day on one instructs nothing.
        CreditTransferGroup::new("Acme GmbH", &debtor, "2026-07-20T11:00:00Z".parse::<IsoDateTime>()?)
            .add_entry(CreditTransferEntry::new("Payee", creditor, 5_000, "E2E-1")),
    )
    .build()
    .unwrap_err();

assert_eq!(
    err.kind,
    ValidationError::Requires {
        feature: "ReqdExctnDt/DtTm (timed execution)",
        requires: "PmtTpInf/LclInstrm = INST (SCT Inst)",
    },
);
# Ok(())
# }
```

> The older schema versions type `ReqdExctnDt` as a bare date and have no
> date-time branch at all. Asking for a timed execution on one of those is
> rejected rather than quietly reduced to the day — a payment meant to leave at
> 11:00 must not silently become "some time on the 20th".

## Structured references

Free text is readable by humans and useless to machines. An ISO 11649 RF
reference is self-checking and round-trips back to you on the statement, which
is what makes automatic reconciliation possible.

```rust
use sepa::{CreditTransferEntry, RfReference, validate_iban};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let creditor = validate_iban("NL91ABNA0417164300")?;

    // Separators are stripped before the check digits are computed, so
    // "inv-2026/0042" and "INV20260042" produce the same reference.
    let reference = RfReference::generate("inv-2026/0042")?;
    assert_eq!(reference, RfReference::generate("INV20260042")?);

    let entry = CreditTransferEntry::new("Supplier AG", creditor, 12_000, "E2E-1")
        .with_reference(reference);
    assert!(entry.remittance.is_some());
    Ok(())
}
```

## Purpose codes and ultimate parties

`Purp` and `CtgyPurp` are **two different code sets** that are easy to confuse.
`RENT` is a purpose and not a category purpose; `DIVD` and `DIVI` both mean
dividend but belong to different sets. The crate models them as separate types,
so one cannot be passed where the other belongs.

```rust
use sepa::{CreditTransferEntry, Party, Purpose, validate_iban};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let creditor = validate_iban("NL91ABNA0417164300")?;

    let entry = CreditTransferEntry::new("Supplier AG", creditor, 12_000, "E2E-1")
        .with_purpose(Purpose::Supp)
        // Who the money is really for, when that differs from the account holder.
        .with_ultimate_creditor(Party::new("Endbeguenstigter GmbH"));

    assert_eq!(entry.purpose, Some(Purpose::Supp));
    Ok(())
}
```

An ultimate party may be set at group level **or** transaction level, never
both — the German DFÜ-Abkommen forbids it, and `build()` enforces it.

## See also

- [Postal addresses](/docs/addresses/) — mandatory structure from 15 Nov 2026
- [Status reports](/docs/status-reports/) — what the bank sends back
- [Schema versions](/docs/schema-versions/) — targeting a specific bank
