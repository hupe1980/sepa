+++
title = "Validation & character set"
description = "The EPC rules that decide whether a bank accepts a SEPA file — amount ranges, name lengths, identifiers — and the EPC217-08 conversion table."
weight = 10
+++

Schema validity does not establish scheme or bank acceptance. Generic ISO types
can admit values outside the selected builder's policy, for example:

- `<InstdAmt Ccy="EUR">0</InstdAmt>` — the generic type allows zero and five decimals
- a 140-character name — the EPC limit is 70
- `<BICFI>NOTPROVIDED</BICFI>` — the literal happens to satisfy the BIC pattern
- a direct debit with no Creditor Identifier

`build()` and `write_to()` enforce the library's selected constraints; successful
output does not certify bank acceptance.

| Field | Rule |
|---|---|
| `MsgId`, `PmtInfId`, `EndToEndId`, `MndtId` | 1–35 chars, no leading or trailing `/`, no `//` |
| `PmtInfId` | unique across the groups of one message |
| Any party name | 1–70 chars, where the schema permits 140 |
| Unstructured remittance | 1–140 chars *after transliteration*, one occurrence |
| Structured remittance | the whole `Strd` block ≤ 140 chars **including the XML tags** |
| `CdtrRefInf/Tp/Issr` | 1–35 chars, and inside the SEPA character set |
| Amount | 0.01 – 999 999 999.99 EUR |
| Batch | at least one transaction |
| Address | town and country present, at most two free-text lines |
| `ReqdExctnDt/DtTm` | only with `LclInstrm = INST`, and only with a UTC offset |
| Ultimate party | at group level or transaction level, never both |

Further rules:

- **Lengths count the emitted text.** They are checked after transliteration
  (`Müller` → `Mueller`) and include leading/trailing whitespace, as the
  `Max35Text`/`Max70Text` types preserve it. Nothing is truncated.
- **Blank-only values and identifier slash rules** are library policies, not
  XSD rules.
- **XML 1.0-forbidden characters** are rejected with
  `ValidationError::InvalidXmlCharacter`. The escaper also quotes markup and
  writes carriage returns as `&#13;`.
- **`PmtInfId` must be unique within a message**: the bank echoes it in
  `pain.002` and in a camt statement's `NtryDtls/Btch`, so a duplicate makes a
  booking unattributable.
- **Purpose codes.** For `Purpose::Other` and `CategoryPurpose::Other`, builders
  check the emitted value is 1–4 uppercase ASCII letters/digits. Lowercase or
  padded values are refused, not rewritten; string parsing trims and uppercases.
  Well-formed unknown codes are accepted — this is not code-list membership.
- **Serde-restored inputs** are validated when built, not when deserialised;
  an incomplete `Party` is refused at use.

A builder that fails validation leaves a `write_to()` sink untouched; an I/O
failure during writing is not transactional.

## Errors name the row

Every error carries the location of the offending group and transaction:

```rust
use sepa::{
    DirectDebitEntry, DirectDebitGroup, IsoDate, Pain008Builder, ValidationError,
    validate_creditor_id, validate_iban,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iban = validate_iban("DE89370400440532013000")?;
    let ci = validate_creditor_id("DE98ZZZ09999999999")?;
    let date = IsoDate::new(2026, 7, 20)?;

    let err = Pain008Builder::new("Stadtwerke GmbH", "DD-1", "2026-07-15T09:00:00".parse()?)
        .add_group(
            DirectDebitGroup::new("Stadtwerke GmbH", &iban, &ci, date)
                .add_entry(DirectDebitEntry::new(
                    "MND-1", date, "Erste", iban.clone(), 100, "E2E-1",
                ))
                .add_entry(DirectDebitEntry::new(
                    "MND-2", date, "Zweiter", iban.clone(), 0, "E2E-2",
                )),
        )
        .build()
        .unwrap_err();

    assert_eq!(err.location.transaction, Some(1));
    assert!(matches!(err.kind, ValidationError::AmountOutOfRange { .. }));
    assert!(err.to_string().starts_with("PmtInf[0]/Tx[1]: "));
    Ok(())
}
```

Dates are not in the rule table: an `IsoDate` is validated when constructed.

The supported year range is **0001–9999** for construction, parsing, serde and
the `time` conversions; the XSD allows wider years. Timestamp
conversions keep whole-minute offsets within ±14:00; other offsets are
re-expressed in UTC without changing the instant. A date that cannot be
represented returns a typed error rather than panicking or clamping. (A local
date with a kept offset is accepted even if its UTC date falls outside
0001–9999.)

## Nothing is silently truncated

A 140-character remittance line is 141 once an umlaut is converted, and is
rejected:

```rust
use sepa::{CharsetPolicy, RemittanceInfo, ValidationError};

fn main() {
    let text = format!("{}ü", "A".repeat(139));   // 140 characters in
    assert_eq!(text.chars().count(), 140);

    assert!(matches!(
        RemittanceInfo::unstructured(&text)
            .validate("RmtInf/Ustrd", CharsetPolicy::default()),
        Err(ValidationError::TooLong { max: 140, actual: 141, .. }),
    ));
}
```

## One table for every length limit

Every `Max*Text` bound the builders enforce lives in `validate::max_text_len`,
keyed by the element and its parent — which is what separates the names ISO
20022 reuses. `SvcLvl/Cd` is a four-character external code; `LclInstrm/Cd` is a
`Max35Text`. A bare `Id` is a container; `Othr/Id` is an identifier.

```rust
use sepa::validate::max_text_len;

fn main() {
    assert_eq!(max_text_len(Some("Cdtr"), "Nm"), Some(70));       // EPC, not the XSD's 140
    assert_eq!(max_text_len(Some("PstlAdr"), "BldgNb"), Some(16));
    assert_eq!(max_text_len(Some("SvcLvl"), "Cd"), Some(4));
    assert_eq!(max_text_len(Some("LclInstrm"), "Cd"), Some(35));
    assert_eq!(max_text_len(Some("DbtrAcct"), "IBAN"), None);     // bounded by its own type
}
```

Use it to pre-check your data with the same limits the builders apply.

## The `Strd` block counts its own markup

The EPC caps structured remittance at 140 characters *including the XML tags*,
so the block is emitted minified and measured by rendering it with the writer's
own function. A 35-character `Ref` and a 35-character `Issr` are each legal and
together exceed the cap:

```rust
use sepa::{CharsetPolicy, RemittanceInfo, ValidationError};

fn main() {
    let too_big = RemittanceInfo::Proprietary {
        reference: "R".repeat(35),
        issuer: Some("I".repeat(35)),
    };
    assert!(matches!(
        too_big.validate("RmtInf", CharsetPolicy::default()),
        Err(ValidationError::TooLong { field: "RmtInf/Strd", max: 140, .. }),
    ));
}
```

## The character set

Writers use the 73-character SEPA Basic Latin set. The default policy converts
unsupported text; `CharsetPolicy::Strict` refuses it.

```text
a–z  A–Z  0–9  / - ? : ( ) . , ' +  and space
```

Conversion uses the **published EPC217-08 table** (1010 mappings), which
differs from generic transliterators beyond plain accents:

| Input | EPC217-08 | Typical generic folding |
|---|---|---|
| `Æ` | `A` | `AE` |
| `Œ` | `O` | `OE` |
| `Щ` | `SHT` | `SHCH` |
| `€` | `E` | dropped |

```rust
use sepa::{Transliteration, transliterate};

fn main() {
    // German convention (the default): preserves how a name reads.
    assert_eq!(
        transliterate("Müller & Söhne", Transliteration::German),
        "Mueller + Soehne"
    );

    // The EPC table exactly as published: one character to one, for Latin.
    assert_eq!(
        transliterate("Müller & Söhne", Transliteration::Epc),
        "Muller + Sohne"
    );

    // Twenty Greek and Cyrillic letters have a real romanisation.
    assert_eq!(transliterate("Ψυχή", Transliteration::Epc), "PSychi");
}
```

The German style is the default; it overrides seven characters, following the
alternative the German banking industry sanctions. Set `CharsetPolicy::Strict`
on a builder to reject out-of-set text instead of rewriting it.

> Identifiers are **never** transliterated, whatever the policy — the bank
> echoes them back for reconciliation. Out-of-set characters in an identifier
> are an error.

## Reading bank documents

Readers accept the namespaces and limits in
[reader contracts](@/docs/reader-contracts.md); they do not perform inbound XSD
validation. Raw observations remain available when a typed value is unusable.
Dates accept optional timezones; amounts accept a leading `+` but refuse
precision below a hundredth; `IsoDateTime` holds whole seconds.

An unreadable `<CdtDbtInd>` is not a credit. Signing requires a usable magnitude,
currency and direction:

```rust,no_run
use sepa::parse_camt053;

# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let xml = String::new();
for entry in &parse_camt053(&xml)?.statements[0].entries {
    match entry.signed_ct() {
        Some(ct) => println!("{ct:+} ct"),
        // The statement did not determine this one. Escalate it — the raw
        // fields hold exactly what the bank sent.
        None => println!("unresolved: {:?} {:?}", entry.amount.amount_raw, entry.amount.direction_raw),
    }
}
# Ok(())
# }
```

An unreadable entry is **never dropped**, and `net_movement_ct()` returns
`None` rather than summing only the readable rows.
