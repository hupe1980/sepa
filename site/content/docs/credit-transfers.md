+++
title = "Credit transfers"
description = "Build credit transfer files (pain.001) in Rust: SCT, SCT Instant, OCT Inst one-leg-out, scheduled instant, structured references and purpose codes."
weight = 3
+++

A credit transfer is money you send. The message is `pain.001`, defaulting to
`pain.001.001.09`, the version the EPC rulebooks mandate.

SCT, SCT Instant and One-Leg Out Instant share that message and differ in a few
coded values — a wrong one passes `xmllint` and is rejected by the bank.
`CreditTransferKind` selects the scheme:

| Scheme | `CreditTransferKind` | `SvcLvl` | `LclInstrm` | `ChrgBr` |
|---|---|---|---|---|
| SCT | `Standard` (default) | `SEPA` | — | `SLEV` |
| SCT Instant | `Instant` | `SEPA` | `INST` | `SLEV` |
| OCT Inst | `OneLegOutInstant` | `EOLO` | `INST` | `CRED`/`DEBT`/`SHAR` |

The scheme and the schema version are separate choices; see
[schema versions](/docs/schema-versions/).

## Ordinary transfers

```rust
use sepa::{CreditTransferEntry, CreditTransferGroup, IsoDate, Pain001Builder, validate_iban};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let debtor = validate_iban("DE89370400440532013000")?;
    let creditor = validate_iban("NL91ABNA0417164300")?;

    let xml = Pain001Builder::new("Acme GmbH", "CT-2026-07-001", "2026-07-15T09:00:00".parse()?)
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

Add several groups to carry different execution dates or debtor accounts in
one file.

## SCT Instant

Instant is a property of the group, not the message:

```rust
use sepa::pain001::CreditTransferKind;
use sepa::{CreditTransferEntry, CreditTransferGroup, IsoDate, Pain001Builder, validate_iban};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let debtor = validate_iban("DE89370400440532013000")?;
    let creditor = validate_iban("NL91ABNA0417164300")?;

    let xml = Pain001Builder::new("Acme GmbH", "CT-INST-001", "2026-07-15T09:00:00".parse()?)
        .add_group(
            CreditTransferGroup::new("Acme GmbH", &debtor, IsoDate::new(2026, 7, 20)?)
                .kind(CreditTransferKind::Instant)
                .add_entry(CreditTransferEntry::new("Payee", creditor, 5_000, "E2E-1")),
        )
        .build()?;

    assert!(xml.contains("<LclInstrm><Cd>INST</Cd></LclInstrm>"));
    Ok(())
}
```

The EPC removed the 100 000 EUR scheme cap on instant transfers on
5 October 2025, so the crate applies the ordinary SEPA ceiling of
999 999 999.99 EUR to every instrument. Any lower limit is your PSP's policy.

## One-leg-out instant transfers (OCT Inst)

OCT Inst is the euro leg of an instant payment whose other leg leaves SEPA
(rulebook EPC158-22, customer-to-PSP guidelines EPC250-22, in force since
5 October 2025). It is the one scheme here that is **not euro-only**.

```rust
use sepa::pain001::CreditTransferKind;
use sepa::{CreditTransferEntry, CreditTransferGroup, IsoDate, Pain001Builder, validate_iban};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let debtor = validate_iban("DE89370400440532013000")?;
    let payee = validate_iban("NL91ABNA0417164300")?;

    let xml = Pain001Builder::new("Acme GmbH", "OCT-2026-001", "2026-07-15T09:00:00".parse()?)
        .add_group(
            CreditTransferGroup::new("Acme GmbH", &debtor, IsoDate::new(2026, 7, 20)?)
                .kind(CreditTransferKind::OneLegOutInstant)
                .add_entry(
                    CreditTransferEntry::new("Payee", payee, 5_000, "OCT-1")
                        // AT-T003/AT-T004 — the payer ordered in USD.
                        .with_currency("USD".parse()?)
                        // AT-T020 — what the payee should receive.
                        .with_non_euro_leg_currency("USD".parse()?),
                ),
        )
        .build()?;

    assert!(xml.contains("<SvcLvl><Cd>EOLO</Cd></SvcLvl>"));
    assert!(xml.contains("<LclInstrm><Cd>INST</Cd></LclInstrm>"));
    assert!(xml.contains(r#"<InstdAmt Ccy="USD">50.00</InstdAmt>"#));
    assert!(xml.contains("<InstrForCdtrAgt><InstrInf>USD</InstrInf></InstrForCdtrAgt>"));
    // SLEV is mandatory for SEPA and *forbidden* here. SHAR is the default.
    assert!(xml.contains("<ChrgBr>SHAR</ChrgBr>"));
    Ok(())
}
```

The crate enforces its rules:

- **There is no `OCTI` code.** The scheme is implied by `SvcLvl/Cd=EOLO` plus
  `LclInstrm/Cd=INST` — a rule no XSD can express.
- **`ChrgBr` inverts.** SEPA mandates `SLEV`; OCT Inst forbids it. The wrong
  one is a `ValidationError::ChargeBearerNotAllowed` at `build()`.
- **AT-T020 has no element of its own.** EPC250-22 puts it in
  `InstrForCdtrAgt/InstrInf`; `with_non_euro_leg_currency` writes it there.
- **`pain.001.001.09` only.** `EOLO` on an older schema is a
  `ValidationError::UnsupportedBySchema`.

Amounts use integer hundredths (at most two fraction digits). Ultimate
parties, purpose codes and structured references work as for SCT; a transfer
back carries `Purp` = `RRCT`, one answering a Request-to-Pay `RRTP`.

## Scheduled instant transfers

`pain.001.001.09` types `ReqdExctnDt` as a date **or** a date-time. The timed
form — the German *terminierte Echtzeitüberweisung* — is an instant transfer
due at a stated moment.

```rust
use sepa::pain001::CreditTransferKind;
use sepa::{CreditTransferEntry, CreditTransferGroup, IsoDateTime, Pain001Builder, validate_iban};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let debtor = validate_iban("DE89370400440532013000")?;
    let creditor = validate_iban("NL91ABNA0417164300")?;
    // A time of day only names an instant once it says which zone it is in.
    let due: IsoDateTime = "2026-07-20T11:00:00Z".parse()?;

    let xml = Pain001Builder::new("Acme GmbH", "CT-TIMED", "2026-07-15T09:00:00".parse()?)
        .add_group(
            CreditTransferGroup::new("Acme GmbH", &debtor, due)
                .kind(CreditTransferKind::Instant)   // required for a timed execution
                .add_entry(CreditTransferEntry::new("Payee", creditor, 5_000, "E2E-1")),
        )
        .build()?;

    assert!(xml.contains("<ReqdExctnDt><DtTm>2026-07-20T11:00:00Z</DtTm></ReqdExctnDt>"));
    Ok(())
}
```

The DK validation subset adds two rules no XSD expresses: `DtTm` is *"Only
allowed for SCTinst"*, and *"Only UTC time format or local time with UTC offset
format can be used"*. `build()` enforces both:

```rust
# use sepa::pain001::CreditTransferKind;
# use sepa::{CreditTransferEntry, CreditTransferGroup, IsoDateTime, Pain001Builder, ValidationError, validate_iban};
# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let debtor = validate_iban("DE89370400440532013000")?;
# let creditor = validate_iban("NL91ABNA0417164300")?;
let err = Pain001Builder::new("Acme GmbH", "CT-TIMED", "2026-07-15T09:00:00".parse()?)
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
        requires: "an instant scheme — CreditTransferKind::Instant or ::OneLegOutInstant",
    },
);
# Ok(())
# }
```

The older schema versions type `ReqdExctnDt` as a bare date, so a timed
execution on one of them is rejected rather than reduced to the day.

## Structured references

An ISO 11649 RF reference carries check digits and comes back on the statement,
which supports automatic invoice matching.

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

`Purp` and `CtgyPurp` are **different code sets**: `RENT` is a purpose but not
a category purpose; `DIVD` and `DIVI` both mean dividend but belong to different
sets. They are separate types (`Purpose`, `CategoryPurpose`).

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

- [Postal addresses](/docs/addresses/) — supported structured and hybrid forms
- [Status reports](/docs/status-reports/) — what the bank sends back
- [Schema versions](/docs/schema-versions/) — targeting a specific bank
