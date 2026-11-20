+++
title = "IBAN, BIC & identifiers"
description = "Validate IBANs, BICs, SEPA Creditor Identifiers and ISO 11649 RF references in Rust — structure and check digits, not just mod-97."
weight = 2
+++

Constructors, string parsing and serde restoration run the same checks.
Validity covers format and checksum rules, not allocation, account existence or
reachability.

| Type | Standard | What is checked |
|---|---|---|
| `Iban` | ISO 13616 | mod-97 **and** the country's registered BBAN structure — validate *and* generate |
| `Bic` | ISO 9362:2022 | format, a real country code, and the pattern the target schema uses |
| `CreditorId` | EPC AT-02 | check digits over the national identifier only — validate *and* generate |
| `RfReference` | ISO 11649 | ISO 7064 MOD 97-10 — validate *and* generate |

## IBAN: the checksum is the easy half

Each BBAN character is checked against the SWIFT IBAN Registry structure —
`8!n10!n` for Germany, `4!a10!n` for the Netherlands:

```rust
use sepa::iban::{BbanCharClass, IbanError};
use sepa::validate_iban;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iban = validate_iban("DE89 3704 0044 0532 0130 00")?;
    assert_eq!(iban.as_str(), "DE89370400440532013000"); // normalised, for the wire
    assert_eq!(iban.to_string(), "DE89 3704 0044 0532 0130 00"); // grouped, for people
    assert_eq!(iban.country_code(), "DE");
    assert_eq!(iban.bban(), "370400440532013000");

    // The letter O where a digit belongs. Mod-97 alone would accept this.
    let err = validate_iban("DE8937O400440532013000").unwrap_err();
    assert!(matches!(
        err,
        IbanError::InvalidBbanFormat {
            position: 7,
            expected: BbanCharClass::Digit,
            ..
        }
    ));
    Ok(())
}
```

The error names the **position**, the required character class and the
country's structure string.

- **89 countries** are in the registry, tested against SWIFT's 78 published
  example IBANs.
- **Length is derived from the structure**, not a separate table.
- **A country outside the registry** gets mod-97 and a length range only.

### Building one from a bank code and an account number

`Iban::from_bban` computes the check digits and still applies the registry
structure:

```rust
use sepa::{Iban, iban::{iban_check_digits, IbanError}};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Bank code 37040044, account 0532013000 — separators are ignored.
    let iban = Iban::from_bban("DE", "3704 0044 0532 0130 00")?;
    assert_eq!(iban.as_str(), "DE89370400440532013000");

    // Or just the digits, when you are filling a field yourself.
    assert_eq!(iban_check_digits("NL", "ABNA0417164300"), "91");

    // The structure still applies: a German BBAN is all digits.
    assert!(matches!(
        Iban::from_bban("DE", "37O400440532013000"),
        Err(IbanError::InvalidBbanFormat { .. })
    ));
    Ok(())
}
```

### Being a valid IBAN is not being reachable

`is_sepa()` reports whether the country participates in the SEPA schemes
(42 do).

```rust
use sepa::validate_iban;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    assert!(validate_iban("DE89370400440532013000")?.is_sepa());
    // Structurally valid, and not payable by SEPA credit transfer.
    assert!(!validate_iban("TR330006100519786457841326")?.is_sepa());
    Ok(())
}
```

## BIC: the pattern accepts banks that do not exist

`COBAZZFF` matches the SEPA BIC pattern and addresses nothing, because `ZZ` is
not a country. Characters 5–6 are checked against the 249 assigned ISO 3166-1
alpha-2 codes plus `XK`, which SWIFT issues Kosovan BICs under.

```rust
use sepa::{validate_bic, BicError};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bic = validate_bic("COBA DE FF XXX")?; // spacing is normalised away
    assert_eq!(bic.as_str(), "COBADEFFXXX");
    assert_eq!(bic.country_code(), "DE");
    assert_eq!(bic.branch_code(), Some("XXX"));
    assert!(bic.is_primary_office());

    assert!(matches!(
        validate_bic("COBAZZFFXXX"),
        Err(BicError::UnknownCountryCode { .. })
    ));
    // The EPC's "no BIC supplied" placeholder is not a BIC.
    assert!(matches!(validate_bic("NOTPROVIDED"), Err(BicError::Placeholder)));
    Ok(())
}
```

`NOTPROVIDED` is the EPC placeholder for an unknown debtor agent. It passes the
XSD BIC pattern, so `validate_bic` rejects it explicitly; the builders emit it
only in the `Othr/Id` position the guidelines specify.

### Alphanumeric prefixes and schema patterns

ISO 9362:2022 widened the business party prefix (the first four characters) from
letters to **alphanumerics**, and SWIFT allocates BICs under it. `validate_bic`
accepts the current standard; whether a given schema can hold the BIC is checked
separately:

```rust
use sepa::{BicPattern, validate_bic};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let classic = validate_bic("COBADEFFXXX")?;
    assert!(classic.fits(BicPattern::LettersOnly));   // every schema takes it

    let modern = validate_bic("E097AEXX")?;           // legal since ISO 9362:2022
    assert!(modern.fits(BicPattern::Alphanumeric));   // pain.008.001.08 takes it
    assert!(!modern.fits(BicPattern::LettersOnly));   // pain.008.001.02 cannot
    Ok(())
}
```

`build()` rejects a BIC the selected schema cannot hold. The element name is
**not** the signal: `pain.008.001.08` writes `BICFI` with the wide pattern,
`pain.008.001.02` writes `BIC` with the narrow one, and `camt.055.001.05` writes
`BICFI` with the *narrow* one. Two ISO 20022 types named `BICFIIdentifier` have
different patterns, so a violation reports the pattern, not a type name.

## Creditor Identifier: not an IBAN, and not checked like one

The SEPA Creditor Identifier (`CdtrSchmeId`, EPC attribute AT-02) is mandatory
on every direct debit. Its check digits are computed over the **national
identifier only** — the three-character Creditor Business Code is excluded:

```text
DE98ZZZ09999999999
│ │ │   └─ national identifier — the only part in the checksum
│ │ └───── creditor business code, usually ZZZ
│ └─────── check digits
└───────── country code
```

Applying the IBAN rule to the whole string instead rejects every genuine
Creditor Identifier, because it folds `ZZZ` into the sum.

```rust
use sepa::validate_creditor_id;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ci = validate_creditor_id("DE98ZZZ09999999999")?;
    assert_eq!(ci.country_code(), "DE");
    assert_eq!(ci.business_code(), "ZZZ");
    assert_eq!(ci.national_id(), "09999999999");

    // The check digits are computed *over* the country code, so a made-up
    // country is self-consistent and needs the ISO 3166 table to catch.
    assert!(validate_creditor_id("ZZ81ZZZ09999999999").is_err());
    Ok(())
}
```

## RF references: the identifier designed to come back

An ISO 11649 RF Creditor Reference goes on the invoice, travels in
`RmtInf/Strd/CdtrRefInf/Ref`, and returns on your camt statement, so the payment
can be matched to the invoice automatically.

```rust
use sepa::RfReference;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Generate from your own invoice number; the check digits are computed.
    let rf = RfReference::generate("2026-0042")?;
    assert_eq!(rf.as_str(), "RF8820260042");
    assert_eq!(rf.to_string(), "RF88 2026 0042"); // printed in fours

    // Separators are stripped first, so these are the same reference.
    assert_eq!(RfReference::generate("INV/2026/0042")?, RfReference::generate("INV20260042")?);

    // Validate one that arrived from outside.
    let parsed: RfReference = "RF18 5390 0754 7034".parse()?;
    assert_eq!(parsed.reference(), "539007547034");
    Ok(())
}
```

Attach it with `with_reference(…)` on a credit transfer or direct debit; the
crate writes the EPC block, including `CdOrPrtry` (capital `O`) and `Issr` =
`ISO`.

## Logging and personal data

An IBAN is personal data, and `Debug` is what lands in logs, panics, test
output and tracing fields. So `Iban` debug-formats masked — the country and
the last four characters, enough to recognise an account, not enough to debit
one. `Display`, `as_str()` and serde still give the full value, so you only
ever print an account number on purpose.

Bank reports keep account identifiers exactly as the bank sent them —
lowercase, spaced or a proprietary `Othr/Id` — as an `AccountId`, masked the
same way, with `iban()` for a checked view:

```rust
use sepa::{AccountId, Iban};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iban: Iban = "DE89370400440532013000".parse()?;
    assert_eq!(format!("{iban:?}"), "Iban(DE…3000)");
    assert_eq!(iban.to_string(), "DE89 3704 0044 0532 0130 00"); // on purpose

    let reported = AccountId::new("de89 3704 0044 0532 0130 00");
    assert_eq!(format!("{reported:?}"), "AccountId(de…0 00)");
    assert_eq!(reported.iban()?, iban);
    Ok(())
}
```

Identifiers shorter than twelve characters are masked entirely. Every record
holding an `Iban` or `AccountId` inherits the masking, and errors never carry
the values that caused them. Names, postal addresses, remittance text and
mandate references are not masked — redacting them would make `Debug` useless
for debugging — so redact those in your logging layer if logs leave the trust
boundary your payment data lives in.

## Where they come from

| Table | Source | Size |
|---|---|---|
| BBAN structures and lengths | SWIFT IBAN Registry, release 102 | 89 countries |
| SEPA scheme countries | EPC409-09 v8.0 | 42 codes |
| Country codes | ISO 3166-1 alpha-2, plus `XK` | 250 codes |

The tables are vendored; tests cover consistency and SWIFT's example IBANs.
BICs, postal countries and Creditor Identifiers share the country-code table.

## Next

- [Getting started](/docs/getting-started/) — your first payment file
- [Validation rules](/docs/validation/) — what banks reject that the XSD allows
