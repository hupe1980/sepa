+++
title = "Getting started"
description = "Install the sepa crate, validate an IBAN, and build your first SEPA credit transfer and direct debit file in Rust. Feature flags and message shape."
weight = 1
+++

## Install

```sh
cargo add sepa@0.9
```

These guides target 0.9. For an unpublished checkout, use
`cargo add sepa --path /path/to/sepa` instead.

The crate needs Rust **1.88** or newer. Two dependencies are mandatory —
`thiserror` and `quick-xml` — and three features are opt-in:

```sh
cargo add sepa@0.9 --features serde   # Supported values, payment inputs and reports
cargo add sepa@0.9 --features time    # IsoDate <-> time::Date
cargo add sepa@0.9 --features chrono  # IsoDate <-> chrono::NaiveDate
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

IBAN validation also checks the registered BBAN structure. A checksum alone
does not establish that each character belongs in its national position:

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

    let xml = Pain001Builder::new("Acme GmbH", "CT-2026-07-001")
        .add_group(
            CreditTransferGroup::new("Acme GmbH", &debtor, IsoDate::new(2026, 7, 20)?)
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

`12_000` is **120.00 EUR**. Monetary values use integer hundredths, not floating
point or a general currency minor-unit scale. `IsoDate::new` rejects impossible
dates at construction.

Message IDs and payment dates are required caller inputs. Supply durable IDs and
choose dates using your banking calendar. `IsoDate::plus_days` performs calendar
arithmetic, not banking-day scheduling. Pin `.created_at(...)` for reproducible
output; otherwise the builder obtains the message creation timestamp from the clock.

## Writing large files

`build()` returns a `String`. For a batch of tens of thousands, stream it
instead — validation still runs first, so a rejected batch leaves the target
untouched rather than writing a truncated document.

```rust
use sepa::Pain008Builder;
use std::fs::File;
use std::io::{BufWriter, Write};

fn write_batch(builder: &Pain008Builder) -> Result<(), Box<dyn std::error::Error>> {
    builder.validate()?;
    let mut writer = BufWriter::new(File::create("collections.xml")?);
    builder.write_to(&mut writer)?;
    writer.flush()?;
    Ok(())
}
```

Later I/O failure can leave a partial file. For atomic replacement, write a
temporary file and rename it only after successful writing and flushing.

## Next

- [IBAN, BIC & identifiers](/docs/identifiers/) — what the checksums do and do not catch
- [Credit transfers](/docs/credit-transfers/) — SCT, instant, scheduled instant
- [Direct debits](/docs/direct-debits/) — mandates, sequence types, CORE and B2B
- [Validation rules](/docs/validation/) — what banks reject that the XSD allows
