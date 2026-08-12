+++
title = "Getting started"
description = "Install the sepa crate, validate an IBAN, and build your first SEPA credit transfer and direct debit file in Rust. Covers feature flags and the shape of a pain message."
weight = 1
+++

## Install

```sh
cargo add sepa
```

The crate needs Rust **1.88** or newer. Two dependencies are mandatory —
`thiserror` and `quick-xml` — and three features are opt-in:

```sh
cargo add sepa --features serde   # Serialize/Deserialize on all public types
cargo add sepa --features time    # IsoDate <-> time::Date
cargo add sepa --features chrono  # IsoDate <-> chrono::NaiveDate
```

`time` and `chrono` are conversion-only. The crate's own calendar arithmetic is
dependency-free, so dates behave identically whichever features are enabled.

## Validate before you build

Identifiers are types, not strings. Constructing one runs the real checks, so
everything downstream can assume they passed.

```rust
use sepa::{validate_bic, validate_creditor_id, validate_iban};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iban = validate_iban("DE89 3704 0044 0532 0130 00")?;
    assert_eq!(iban.as_str(), "DE89370400440532013000");
    assert_eq!(iban.to_string(), "DE89 3704 0044 0532 0130 00");
    assert!(iban.is_sepa());

    let bic = validate_bic("COBADEFFXXX")?;
    assert_eq!(bic.country_code(), "DE");

    let ci = validate_creditor_id("DE98ZZZ09999999999")?;
    assert_eq!(ci.business_code(), "ZZZ");
    Ok(())
}
```

`as_str` gives the normalised value for the wire; `to_string` groups it in
fours for display.

Validation goes past the checksum. Mod-97 detects an altered character about 96
times in 97 and never says which one, so IBANs are also checked against the
country's registered BBAN structure:

```rust
use sepa::iban::{BbanCharClass, IbanError};
use sepa::validate_iban;

fn main() {
    // A capital O typed for a zero. The German BBAN is 18 digits.
    let err = validate_iban("DE8937O400440532013000").unwrap_err();
    assert!(matches!(
        err,
        IbanError::InvalidBbanFormat {
            position: 7,
            expected: BbanCharClass::Digit,
            ..
        }
    ));
}
```

## Your first file

A message carries one or more **payment groups**, each becoming a `PmtInf`
block. The execution date, debtor account, batch booking and — for direct
debits — the sequence type all live at that level.

```rust
use sepa::{CreditTransferEntry, CreditTransferGroup, IsoDate, Pain001Builder, validate_iban};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let debtor = validate_iban("DE89370400440532013000")?;
    let creditor = validate_iban("NL91ABNA0417164300")?;

    let xml = Pain001Builder::new("Acme GmbH")
        .msg_id("CT-2026-07-001")
        .add_group(
            CreditTransferGroup::new("Acme GmbH", &debtor)
                .execution_date(IsoDate::new(2026, 7, 20)?)
                .add_entry(
                    CreditTransferEntry::new("Supplier AG", creditor, 12_000, "INV-2026-001")
                        .with_description("Rechnung 2026-07"),
                ),
        )
        .build()?;

    assert!(xml.contains("<InstdAmt Ccy=\"EUR\">120.00</InstdAmt>"));
    Ok(())
}
```

Two things to notice. `12_000` is **cents** — every amount in the crate is an
`i64` in the currency's minor unit, so no `f64` rounding can reach a payment.
And `IsoDate::new` returns a `Result`: an impossible date is rejected where it
is written rather than by the bank days later.

## Writing large files

`build()` returns a `String`. For a batch of tens of thousands, stream it
instead — validation still runs first, so a rejected batch leaves the target
untouched rather than writing a truncated document.

```rust
use sepa::Pain008Builder;
use std::fs::File;
use std::io::BufWriter;

fn write_batch(builder: &Pain008Builder) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create("collections.xml")?;
    builder.write_to(&mut BufWriter::new(file))?;
    Ok(())
}
```

## Next

- [Credit transfers](/docs/credit-transfers/) — SCT, instant, scheduled instant
- [Direct debits](/docs/direct-debits/) — mandates, sequence types, CORE and B2B
- [Validation rules](/docs/validation/) — what banks reject that the XSD allows
