+++
title = "IBAN, BIC & identifiers"
description = "Validate IBANs, BICs, SEPA Creditor Identifiers and ISO 11649 RF references in Rust — structure and check digits, not just mod-97."
weight = 2
+++

Four identifiers, four different check-digit schemes, and one rule they share:
each is a **type whose constructor is the only way in**. A validated identifier
is a proof you carry through the rest of your code, not a `String` somebody
remembers to check.

| Type | Standard | What is checked |
|---|---|---|
| `Iban` | ISO 13616 | mod-97 **and** the country's registered BBAN structure — validate *and* generate |
| `Bic` | ISO 9362:2022 | format, a real country code, and the pattern the target schema uses |
| `CreditorId` | EPC AT-02 | check digits over the national identifier only — validate *and* generate |
| `RfReference` | ISO 11649 | ISO 7064 MOD 97-10 — validate *and* generate |

## IBAN: the checksum is the easy half

Mod-97 catches an altered character about 96 times in 97, and it never says
*which* one. So a capital `O` typed for a zero in a German account number gets
through roughly 99 % of the time it is the only mistake: the operator sees "IBAN
valid", the bank sees an account that does not exist, and the payment comes back
days later with a return charge.

The SWIFT IBAN Registry publishes each country's BBAN structure — `8!n10!n` for
Germany, `4!a10!n` for the Netherlands — and every character is checked against
it:

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

The error names the **position**, the character class the registry requires and
the country's structure string — enough to point an operator at the keystroke to
fix.

Three details worth knowing:

- **89 countries** are in the registry, checked in CI against the 78 real
  example IBANs SWIFT publishes, so a transcription slip fails the build rather
  than a payment.
- **Length comes from the structure**, not a second table, so the two cannot
  disagree.
- **A country outside the registry** gets mod-97 and a length range and nothing
  more. The crate does not invent a structure it has not been given.

### Building one from a bank code and an account number

The check digits are derivable, so there is no reason to paste a snippet for
them — and doing it through `Iban::from_bban` means the result still has to pass
the registry structure, which a bare `98 − n mod 97` does not:

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

`is_sepa()` answers a different question: whether the country participates in
the SEPA schemes at all. Forty-two do; plenty of structurally perfect IBANs are
not among them.

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

`NOTPROVIDED` is rejected on purpose. It is what the EPC puts in `DbtrAgt` when
no agent is known, it passes an XSD exactly like a real BIC, and treating it as
one addresses a bank that does not exist. The builders emit it only in the
`Othr/Id` position the guidelines specify, never in a `BIC` element.

### The prefix is alphanumeric, and being stricter is not being safer

ISO 9362:2022 widened the business party prefix — the first four characters —
from letters to **alphanumerics**, and SWIFT allocates BICs under it. Any
validator still testing `[A-Z]{6}` for the first six characters rejects a real
BIC, which is a loud failure the payer sees.

ISO 20022 tracks the same split, and this crate emits messages on both sides of
it. So `validate_bic` accepts the current standard, and *which schema can hold a
given BIC* is a separate question the builders ask:

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

Put a BIC the older schema cannot hold into a `pain.008.001.02` batch and
`build()` refuses it by name, rather than emitting a document `xmllint` would
reject at the bank.

The element name is **not** the signal: `pain.008.001.08` writes `BICFI` over
the wide pattern, `pain.008.001.02` writes `BIC` over the narrow one, and
`camt.055.001.05` writes `BICFI` over the *narrow* one. Two different ISO 20022
types are even named `BICFIIdentifier` with different patterns, which is why a
violation reports the pattern rather than a type name.

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

Applying the IBAN rule instead — whole string, expected remainder 1 — rejects
every genuine Creditor Identifier, because it folds `ZZZ` into the sum. It is
the single most common way a home-grown AT-02 implementation goes wrong.

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

An ISO 11649 RF Creditor Reference is the one identifier here built to make a
round trip: you put it on the invoice, the debtor's bank carries it in
`RmtInf/Strd/CdtrRefInf/Ref`, and it returns on your camt statement — so the
payment matches the invoice with nobody reading anything.

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

Attach it with `with_reference(…)` on a credit transfer or a direct debit and
the crate writes the block the EPC requires — including the two spellings
implementations routinely get wrong: the element is `CdOrPrtry` (capital `O`),
and `Issr` must be `ISO` when `Ref` carries an RF reference.

## Where they come from

| Table | Source | Size |
|---|---|---|
| BBAN structures and lengths | SWIFT IBAN Registry, release 102 | 89 countries |
| SEPA scheme countries | EPC409-09 v8.0 | 42 codes |
| Country codes | ISO 3166-1 alpha-2, plus `XK` | 250 codes |

All three are vendored, and all three are checked in CI against the examples
their publishers ship. One table answers "is this two letters or a country" for
BIC characters 5–6, `PstlAdr/Ctry` and a Creditor Identifier alike, so they
cannot drift apart.

## Next

- [Getting started](/docs/getting-started/) — your first payment file
- [Validation rules](/docs/validation/) — what banks reject that the XSD allows
