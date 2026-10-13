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

Serde restoration of mutable payment inputs does not certify them for output.
Incomplete `Party` values are refused at use. For `Purpose::Other` and
`CategoryPurpose::Other`, builders validate the exact emitted value: 1–4 uppercase
ASCII letters/digits, without padding. Direct/restored lowercase or padded codes
are refused, not rewritten. String parsing intentionally trims/uppercases when
that conversion is desired. Well-shaped unknown codes remain supported; this is
a representation policy, not current ISO code-list membership certification.

Two of those are worth calling out because they are easy to get wrong and
impossible for a schema to catch.

**Lengths bind on what the bank receives, not on what you passed in.**
Transliteration can lengthen a string — `Müller` becomes `Mueller` — so a
140-character German remittance line can cross the limit on its way out. The
check runs after conversion and rejects; nothing is ever silently truncated,
because the tail of a remittance line is where the invoice number sits.

Bounds count the complete emitted text, including leading/trailing whitespace.
The pinned `Max35Text` and `Max70Text` types derive from whitespace-preserving
`xs:string`; a trimmed length is not the length written to XML. Blank-only refusal
and identifier slash rules are explicit library policies, not universal XSD rules.

Shared text validation rejects XML 1.0-forbidden characters with the located
`ValidationError::InvalidXmlCharacter`. Ordinary text is checked after the selected
transliteration policy; strict mode refuses unsupported SEPA characters, and
identifiers are never transliterated. The XML escaper independently quotes markup
and encodes carriage returns as `&#13;` wherever raw text reaches it. This is a
syntax safeguard, not a new UTF-8 passthrough mode. Invalid builders leave an
existing `write_to()` sink untouched; later I/O failure is not transactional.

**`PmtInfId` must be unique within a message.** It is the key a bank echoes back
in the `pain.002` and in the `NtryDtls/Btch` block of a camt statement, so two
groups sharing one make a booking unattributable.

## Errors name the row

In a run of ten thousand collections, "the amount is out of range" is only
actionable once it says which one:

```rust
use sepa::{
    DirectDebitEntry, DirectDebitGroup, IsoDate, Pain008Builder, ValidationError,
    validate_creditor_id, validate_iban,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iban = validate_iban("DE89370400440532013000")?;
    let ci = validate_creditor_id("DE98ZZZ09999999999")?;
    let date = IsoDate::new(2026, 7, 20)?;

    let err = Pain008Builder::new("Stadtwerke GmbH", "DD-1")
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

Dates are absent from the rule table on purpose. An `IsoDate` is validated
where it is constructed, so an impossible collection date never reaches a
batch — there is nothing for `build()` to catch.

The supported year range is **0001–9999**, including component construction,
parsing, serde restoration and optional `time`/`chrono` conversions. Wider years
are allowed by XSD, but are outside this library's representation. This is not
a bank-acceptance guarantee. Timestamp conversions retain exact whole-minute
offsets within ±14:00; offsets containing seconds or outside that bound are
re-expressed at UTC without changing the whole-second instant. UTC conversion
must also remain within the supported date range. An unrepresentable local date
or required UTC date returns a typed date error rather than panicking or clamping.
Exact whole-minute offsets within ±14:00 do not require UTC conversion: valid
local endpoint dates remain accepted even if their UTC date is outside 0001–9999.

## Nothing is silently truncated

A remittance line of exactly 140 German characters is 141 once the umlauts are
converted. That is a rejection, not a quiet trim:

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

Use it to pre-check your own data with the same numbers the builders apply,
rather than copying them into your code where they can drift.

Tests walk populated generated fixtures against this table and an explicit list
of values bounded by their types. Keep those fixtures populated when adding a
writer field; this is not proof for every possible document.

## The `Strd` block counts its own markup

The EPC caps structured remittance information at 140 characters *including the
XML tags*, which is why the block is emitted minified — pretty-printing alone
overruns it. No per-field check can see that limit: a 35-character `Ref` and a
35-character `Issr` are each perfectly legal and together break it.

The block is therefore measured by rendering it, with the same function the
writer uses, so what is checked is exactly what is emitted:

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
unsupported text; strict mode refuses it. Bank acceptance remains separate.

```text
a–z  A–Z  0–9  / - ? : ( ) . , ' +  and space
```

Conversion uses the **published EPC217-08 table**, transcribed from the
spreadsheet the EPC distributes — 1010 mappings — rather than an approximation.
That matters beyond plain accents, where generic transliterators diverge from
the standard:

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

The German style is the default because losing the umlaut changes how a name
reads — and on a bank statement, who it appears to be. It overrides exactly
seven characters, following the alternative the German banking industry
sanctions.

Set `CharsetPolicy::Strict` on a builder to reject out-of-set text instead of
rewriting it, when your data is already sanitised and you want a hard guarantee
that nothing is silently changed.

> Identifiers are **never** transliterated, whatever the policy. An identifier
> is the key the bank echoes back on the statement, so quietly rewriting
> `MND-Straße` to `MND-Strasse` would break your own reconciliation. Out-of-set
> characters in an identifier are a hard error.

## Reader Boundaries

Readers support the namespaces, finite intake limits and retained fields in
[reader contracts](@/docs/reader-contracts.md), not every lawful bank document
or complete inbound XSD validation. Raw observations remain available when
their typed value is unusable. Dates support optional timezones; money supports
a leading `+` but refuses significant precision below a hundredth.
`IsoDateTime` represents whole seconds, not fractional-second precision.

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

An entry is **never dropped** for being unreadable either — a missing booking
looks identical to one that never happened. Totals follow the same rule:
`net_movement_ct()` returns `None` rather than quietly summing only the rows it
could read.
