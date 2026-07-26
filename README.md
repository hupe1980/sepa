# sepa

[![Crates.io](https://img.shields.io/crates/v/sepa.svg)](https://crates.io/crates/sepa)
[![Docs.rs](https://img.shields.io/docsrs/sepa)](https://docs.rs/sepa)
[![CI](https://github.com/hupe1980/sepa/actions/workflows/ci.yml/badge.svg)](https://github.com/hupe1980/sepa/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![MSRV](https://img.shields.io/badge/rustc-1.88+-orange.svg)](https://blog.rust-lang.org/2025/06/26/Rust-1.88.0/)

> 🏦 **SEPA payment utilities for Rust.**
> ⚡ Zero I/O. No async. 🔢 No `f64` in monetary arithmetic. 📅 No hand-formatted dates.
> ✅ Generated files are validated against the real ISO 20022 XSDs in CI — every schema version.

---

## ✨ Features

| Module | What it provides |
|---|---|
| `iban` | 🔍 IBAN validation — ISO 13616 mod-97 **+ national BBAN structure** + 89-country registry + SEPA membership |
| `bic` | 🔍 BIC/SWIFT validation — ISO 9362, SEPA pattern, real country codes |
| `creditor_id` | 🔍 SEPA Creditor Identifier — EPC AT-02 |
| `pain001` | 📤 pain.001 XML builder — SCT + SCT Instant |
| `pain008` | 📤 pain.008 XML builder — SDD CORE + B2B |
| `pain002` | 📥 pain.002 XML parser — Payment Status Report (bank → customer) |
| `camt052` | 📥 camt.052 XML parser — intraday account report |
| `camt053` | 📥 camt.053 XML parser — Bank-to-Customer Statement (end-of-day) |
| `camt054` | 📥 camt.054 XML parser — debit/credit notification, incl. returns |
| `reference` | 🔗 ISO 11649 RF Creditor Reference — validate **and generate** |
| `party` | 👥 Ultimate debtor/creditor (`UltmtDbtr` / `UltmtCdtr`) |
| `purpose` | 🏷️ `Purp` and `CtgyPurp` codes, as the two distinct code sets |
| `date` | 📅 `IsoDate` / `IsoDateTime` — validated at construction, `time` + `chrono` interop |
| `validate` | ✅ EPC field rules the XSD does not enforce, with the failing group and transaction |
| `charset` | 🔤 SEPA Basic Latin set + the real **EPC217-08** conversion table |
| `ct_to_eur_str` / `ct_from_eur_str` | 💶 Integer-safe `i64 ct ↔ "1234.56"` round-trip |

All commonly used types are re-exported at the crate root (`sepa::SequenceType`, `sepa::DirectDebitScheme`, …).

---

## 🚀 Quick start

```toml
[dependencies]
sepa = "0.5"
```

Optional features: `serde`, `json`, `time`, `chrono` — see
[Dependencies](#-dependencies).

### 📤 Building payment files

`build()` returns a `Result`: the batch is checked against the EPC rules before
any XML is produced.

```rust
use sepa::{
    validate_iban, validate_creditor_id, IsoDate,
    Pain008Builder, DirectDebitGroup, DirectDebitEntry, SequenceType,
    Pain001Builder, CreditTransferGroup, CreditTransferEntry, LocalInstrument,
};

let creditor = validate_iban("DE89 3704 0044 0532 0130 00")?;
let debtor   = validate_iban("NL91ABNA0417164300")?;
assert_eq!(creditor.to_string(), "DE89 3704 0044 0532 0130 00"); // grouped Display
assert!(creditor.is_sepa());                                     // SEPA scheme area

// pain.001 — SEPA Credit Transfer (Überweisung), pain.001.001.09
let ct_xml = Pain001Builder::new("Debtor GmbH")
    .msg_id("CT-2026-07-001")
    .add_group(
        CreditTransferGroup::new("Debtor GmbH", &creditor)
            .execution_date(IsoDate::new(2026, 7, 20)?)  // typed, never a string
            .add_entry(
                CreditTransferEntry::new(
                    "Supplier AG",
                    debtor.clone(),
                    12_000,         // 120.00 EUR — integer cents, never f64
                    "REFUND-001",
                ).with_description("Erstattung 2025"),
            ),
    )
    .build()?;

// pain.008 — a direct debit run with FRST and RCUR in ONE file.
// Sequence type lives on the group, so each needs its own.
let ci = validate_creditor_id("DE98ZZZ09999999999")?;
let dd_xml = Pain008Builder::new("Stadtwerke GmbH")
    .msg_id("DD-2026-07-001")
    .add_group(
        DirectDebitGroup::new("Stadtwerke GmbH", &creditor, &ci)
            .sequence_type(SequenceType::Frst)
            .collection_date(IsoDate::new(2026, 7, 20)?)
            .add_entry(DirectDebitEntry::new(
                "MND-001", "2026-06-01".parse()?, "Neu Kunde", debtor.clone(), 5_000, "E2E-001",
            )),
    )
    .add_group(
        DirectDebitGroup::new("Stadtwerke GmbH", &creditor, &ci)
            .sequence_type(SequenceType::Rcur)
            .collection_date(IsoDate::new(2026, 7, 18)?)
            .add_entry(DirectDebitEntry::new(
                "MND-002", "2024-06-01".parse()?, "Alt Kunde", debtor, 7_500, "E2E-002",
            )),
    )
    .build()?;
assert!(dd_xml.contains("<SeqTp>FRST</SeqTp>"));
assert!(dd_xml.contains("<SeqTp>RCUR</SeqTp>"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

Each `add_group` becomes one `PmtInf` block. Sequence type, execution date,
debtor account, batch booking and category purpose all sit at that level — so
groups are what let one file mix `FRST` with `RCUR`, or carry two execution
dates, instead of forcing a separate submission per combination.

### 📅 Dates are values, not strings

`ReqdColltnDt`, `ReqdExctnDt`, `DtOfSgntr` and `CreDtTm` take an `IsoDate` /
`IsoDateTime`, exactly as the IBAN fields take a validated `Iban`. An impossible
date is rejected where it is written, so it can never reach a submitted file:

```rust
use sepa::IsoDate;

let d = IsoDate::new(2026, 7, 20)?;               // from components
let e: IsoDate = "2026-07-20".parse()?;           // from stored text
assert_eq!(d, e);
assert_eq!(d.plus_days(5)?.to_string(), "2026-07-25");   // collection offsets

assert!("2026-02-30".parse::<IsoDate>().is_err()); // February never has 30 days
assert!("20.07.2026".parse::<IsoDate>().is_err()); // nor is this ISO 8601
# Ok::<(), sepa::DateError>(())
```

With the `time` or `chrono` feature the corresponding types convert both ways,
so an application that already has a typed date never formats one:

```toml
sepa = { version = "0.5", features = ["time"] }
```

```rust,ignore
let collection: IsoDate = my_record.collection_date.try_into()?; // time::Date
```

### 🎯 Targeting a bank's schema version

The required version varies by bank and by regulatory cutover, so it is a
per-message choice — and parseable, for reading out of configuration:

```rust
use sepa::{Pain008Builder, pain008::DirectDebitSchema};

// The 2023 rulebook default …
assert_eq!(DirectDebitSchema::default().message_id(), "pain.008.001.08");

// … or whatever this particular bank still mandates.
let schema: DirectDebitSchema = "pain.008.001.02".parse()?;
let builder = Pain008Builder::new("Stadtwerke GmbH").schema(schema);
# let _ = builder;
# Ok::<(), sepa::UnknownSchema>(())
```

| Message | Available versions |
|---|---|
| pain.001 | `pain.001.001.09` (default) · `pain.001.001.03` · `pain.001.003.03` |
| pain.008 | `pain.008.001.08` (default) · `pain.008.001.02` · `pain.008.003.02` |

Each is validated against its own real XSD in CI, and the differences between
them are handled rather than papered over — the `BIC` → `BICFI` rename, the
`ReqdExctnDt` date/time choice, and the `SMNDA` marker's pre-2016 position in
the DK schema.

### 🔍 Identifier validation goes past the checksum

Mod-97 is a checksum, not a format check: it detects an altered character about
96 times in 97, and never says which one. The SWIFT registry publishes each
country's BBAN structure, so the crate checks that too — and names the position:

```rust
use sepa::{validate_iban, validate_bic};
use sepa::iban::{BbanCharClass, IbanError};
use sepa::bic::BicError;

// A capital O typed for a zero. The German BBAN is 18 digits.
let err = validate_iban("DE8937O400440532013000").unwrap_err();
assert!(matches!(
    err,
    IbanError::InvalidBbanFormat { position: 7, expected: BbanCharClass::Digit, .. },
));
assert!(err.to_string().contains("position 7 must be a digit"));

// The IBAN header is structural too — mod-97 would happily expand a digit here.
assert!(matches!(
    validate_iban("1289370400440532013000"),
    Err(IbanError::InvalidCountryCode { .. }),
));

// And a BIC country code has to be a country, not merely two letters.
assert!(matches!(
    validate_bic("COBAZZFF"),
    Err(BicError::UnknownCountryCode { .. }),
));
```

The registry table is the crate's single source of truth — `iban_country_length`
is derived from it, so length and structure cannot disagree — and it is checked
in CI against the 78 real IBAN examples the registry publishes.

### ✅ Validation and character handling

The published ISO schemas are far more permissive than the banks. A file can
validate cleanly against the XSD and still be rejected — a zero amount, a
141-character name, `<BICFI>NOTPROVIDED</BICFI>`. This crate enforces the EPC
rules that actually decide acceptance:

A failure comes back as a `BuildError`: the typed `ValidationError` naming the
ISO 20022 element, **plus the group and transaction it came from** — because in
a collection run of ten thousand rows, "`InstdAmt` is out of range" is only
actionable once it says which row.

```rust
use sepa::{
    DirectDebitEntry, DirectDebitGroup, IsoDate, Pain001Builder, Pain008Builder,
    ValidationError, validate_creditor_id, validate_iban,
};

// An empty message is schema-invalid (PmtInf and CdtTrfTxInf are both 1..n).
assert!(matches!(
    Pain001Builder::new("Acme GmbH").build().unwrap_err().kind,
    ValidationError::EmptyBatch,
));

let iban = validate_iban("DE89370400440532013000")?;
let ci = validate_creditor_id("DE98ZZZ09999999999")?;
let date = IsoDate::new(2026, 7, 20)?;

let err = Pain008Builder::new("Stadtwerke GmbH")
    .msg_id("DD-1")
    .add_group(
        DirectDebitGroup::new("Stadtwerke GmbH", &iban, &ci)
            .collection_date(date)
            .add_entry(DirectDebitEntry::new("MND-1", date, "Erste Kundin", iban.clone(), 100, "E2E-1"))
            .add_entry(DirectDebitEntry::new("MND-2", date, "Zweiter Kunde", iban.clone(), 0, "E2E-2")),
    )
    .build()
    .unwrap_err();

assert_eq!(err.location.transaction, Some(1));   // the second collection
assert!(matches!(err.kind, ValidationError::AmountOutOfRange { .. }));
assert!(err.to_string().starts_with("PmtInf[0]/Tx[1]: "));
# Ok::<(), Box<dyn std::error::Error>>(())
```

| Rule | Enforced |
|---|---|
| `MsgId`, `EndToEndId`, `MndtId` | 1–35 chars, no leading/trailing `/`, no `//` |
| Party name `Nm` | 1–70 chars (XSD permits 140) |
| Remittance `Ustrd` | 1–140 chars |
| `InstdAmt` | 0.01 – 999,999,999.99 EUR |
| Batch | at least one transaction |
| Direct debit | Creditor Identifier required |
| Ultimate party `Nm` | 1–70 chars; only `Nm` and `Id` permitted |
| Purpose codes | 1–4 alphanumerics, in the correct code set |
| Schema features | e.g. SCT Inst rejected on a schema with no `LclInstrm` |

Dates are absent from that table on purpose — an `IsoDate` is validated where it
is constructed, so `build()` never has to catch one.

Text outside the SEPA Basic Latin set is transliterated by default, since banks
reject it outright:

```rust
use sepa::{transliterate, Transliteration};

// German style (default) — preserves the reading of the name
assert_eq!(transliterate("Müller & Söhne GmbH", Transliteration::German),
           "Mueller + Soehne GmbH");

// The EPC217-08 table exactly as published — strictly one-to-one for Latin
assert_eq!(transliterate("Müller & Söhne GmbH", Transliteration::Epc),
           "Muller + Sohne GmbH");
```

The mapping is transcribed from the spreadsheet the EPC publishes, not
approximated — 1011 entries. That matters beyond Latin accents, where generic
transliterators diverge from the standard:

```rust
# use sepa::{transliterate, Transliteration};
// 26 Greek and Cyrillic letters have a published romanisation (ISO 843 / ISO 9)
assert_eq!(transliterate("Ψυχή", Transliteration::Epc), "PSychi");
assert_eq!(transliterate("Щука", Transliteration::Epc), "SHTuka");

// …and the ligatures do NOT expand, whatever a generic folder would do
assert_eq!(transliterate("Æon", Transliteration::Epc), "Aon");
```

Use `.charset(CharsetPolicy::Strict)` to reject non-SEPA characters instead of
rewriting them.

### 🧾 Corporate extras

Ultimate parties, purpose codes, structured references and mandate amendments —
all XSD-validated in the right element positions:

```rust
use sepa::{
    validate_iban, validate_creditor_id, DirectDebitEntry, MandateAmendment,
    Pain008Builder, Party, Purpose, RfReference,
};

let iban = validate_iban("DE89370400440532013000")?;
let debtor = validate_iban("NL91ABNA0417164300")?;

let xml = Pain008Builder::new("Stadtwerke GmbH")
    .msg_id("DD-2026-07")
    .add_group(
        sepa::DirectDebitGroup::new(
            "Stadtwerke GmbH", &iban, &validate_creditor_id("DE98ZZZ09999999999")?,
        )
        .collection_date(sepa::IsoDate::new(2026, 7, 20)?)
        .add_entry(
            DirectDebitEntry::new("MND-042", "2024-06-01".parse()?, "Max Mustermann", debtor, 7_500, "E2E-1")
                // Collecting on behalf of the network operator…
                .with_ultimate_creditor(Party::new("Netzbetreiber AG"))
                // …from someone other than the account holder.
                .with_ultimate_debtor(Party::new("Erika Mustermann"))
                .with_purpose(Purpose::Elec)
                // A self-checking invoice reference that round-trips to camt.
                .with_reference(RfReference::generate("INV-2026-0042")?)
                // The debtor switched account since the last collection.
                .with_amendment(MandateAmendment::debtor_account_changed()),
        ),
    )
    .build()?;

assert!(xml.contains("<AmdmntInd>true</AmdmntInd>"));
assert!(xml.contains("<Othr><Id>SMNDA</Id></Othr>"));
assert!(xml.contains("<Purp><Cd>ELEC</Cd></Purp>"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

### 💾 Streaming large batches (no intermediate `String`)

```rust,no_run
use sepa::{Pain008Builder, validate_iban, validate_creditor_id};
use std::io::BufWriter;
use std::fs::File;

let iban = validate_iban("DE89370400440532013000")?;
let builder = Pain008Builder::new("Payroll GmbH")
    .msg_id("PAYROLL-2026-07")
    .add_group(
        sepa::DirectDebitGroup::new("Payroll GmbH", &iban, &validate_creditor_id("DE98ZZZ09999999999")?)
            .collection_date(sepa::IsoDate::new(2026, 7, 20)?),
        // … .add_entry(…) × 10 000 …
    );

// `write_to` validates first, so a rejected batch leaves the file untouched
// rather than writing a truncated document.
let file = File::create("payroll.xml")?;
builder.write_to(&mut BufWriter::new(file))?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

### 📥 Parsing bank responses

Parsing is built on [`quick-xml`](https://crates.io/crates/quick-xml), so
comments, entity references, CDATA and self-closing tags are all handled
correctly, and `<ns2:Document>` and `<Document xmlns=…>` parse identically.
`<!DOCTYPE>` is rejected outright as defence in depth against entity-expansion
attacks.

**pain.002 — Payment Status Report**

```rust,no_run
use sepa::parse_pain002;

# let xml = "";
let doc = parse_pain002(xml)?;

if doc.is_fully_accepted() {
    println!("✅ Batch accepted");
} else {
    for tx in doc.rejected_transactions() {
        println!("❌ {} — {:?}", tx.original_end_to_end_id, tx.reason_codes);
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

**camt.053 — End-of-Day Statement**

Handles every generation from `camt.053.001.02` to `.13`, including the v07
changes (`<Sts><Cd>BOOK</Cd></Sts>`, `RltdPties/Dbtr/Pty/Nm`).

```rust,no_run
use sepa::parse_camt053;

# let xml = "";
let doc = parse_camt053(xml)?;
let stmt = &doc.statements[0];

println!("Account: {}", stmt.account_iban);
println!("Closing: {} ct", stmt.closing_balance().unwrap().signed_ct());

for entry in &stmt.entries {
    println!("{:+} ct  {}", entry.signed_ct(), entry.reference().unwrap_or(""));

    // Dates arrive as `ISODate` from one bank and `ISODateTime` from the next;
    // `booking_date()` is typed either way, `booking_date_raw` keeps the text.
    println!("  booked {:?}", entry.booking_date());

    // A batch-booked entry carries one detail per original transaction, and
    // `signed_ct()` resolves each one's amount and direction — from the detail,
    // from `AmtDtls/TxAmt`, or from the entry when there is only one detail.
    // `None` means the statement does not determine it, which is exactly when
    // the tempting fallback (reuse the entry total) would multiply the batch.
    if entry.batch_booked {
        for d in &entry.details {
            println!("  ↳ {:?} {:?} ct", d.end_to_end_id, d.signed_ct());
        }
        // Do the parts account for the whole? If not, escalate rather than post.
        assert!(entry.details_reconcile());
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

**camt.054 — Intraday Notifications** (`json` feature)

```toml
sepa = { version = "0.5", features = ["json"] }
```

Every parser returns a `Result` with a typed error, so a bank row that gets
skipped on import carries the reason it was skipped:

```rust
# #[cfg(feature = "json")]
# fn demo() -> Result<(), Box<dyn std::error::Error>> {
use sepa::camt054::{SimpleJsonError, parse_simple_json};

let entry = parse_simple_json(&serde_json::json!({
    "iban":       "DE89370400440532013000",
    "amount_eur": "155.42",
    "reference":  "Invoice-001",
    "date":       "2026-07-10"
}))?;
assert_eq!(entry.amount_ct, 15_542);
assert_eq!(entry.to_ledger_ct(), -15_542); // credit → negative in open-items

// A German-formatted amount names the field and the reason, rather than
// vanishing into a silent skip.
let err = parse_simple_json(&serde_json::json!({
    "iban": "DE89370400440532013000", "amount_eur": "155,42", "date": "2026-07-10",
})).unwrap_err();
assert!(matches!(err, SimpleJsonError::InvalidAmount { field: "amount_eur", .. }));
assert_eq!(err.to_string(), r#"field "amount_eur": "155,42" is not a decimal amount"#);
# Ok(())
# }
# #[cfg(feature = "json")]
# demo().unwrap();
```

---

## 📋 Standards

| Standard | Module | Notes |
|---|---|---|
| ISO 13616-1 + SWIFT IBAN Registry r101 | `iban` | 89-country BBAN structure + length table |
| EPC409-09 v8.0 | `iban` | SEPA scheme country list (42 codes) |
| ISO 9362 | `bic` | 8- and 11-char BIC, SEPA pattern, location-code rules |
| ISO 3166-1 alpha-2 | `bic` | BIC country codes (249 + `XK`) |
| EPC262-08 | `creditor_id` | Creditor Identifier check digits |
| ISO 20022 pain.001.001.09 | `pain001` | SCT + SCT Instant — **default** |
| ISO 20022 pain.001.001.03 | `pain001` | EPC version until Nov 2023 |
| ISO 20022 pain.001.003.03 | `pain001` | Legacy DK V2.7 |
| ISO 20022 pain.008.001.08 | `pain008` | SDD CORE + B2B — **default** |
| ISO 20022 pain.008.001.02 | `pain008` | EPC version until Nov 2023 |
| ISO 20022 pain.008.003.02 | `pain008` | Legacy DK V2.7 |
| ISO 20022 pain.002 (all variants) | `pain002` | namespace-agnostic parser |
| ISO 20022 camt.053 (v02–v13) | `camt053` | namespace-agnostic parser |
| EPC217-08 | `charset` | SEPA character set + conversion table |
| EPC SEPA Rulebooks 2023/2025 | all | SCT, SDD Core, SDD B2B, SCT Inst |

### Schema versions

`pain.001.001.09` and `pain.008.001.08` are the versions the EPC 2023 rulebooks
mandated from 19 November 2023, and are the defaults here. `pain.001.001.03` and
`pain.008.001.02` are the EPC versions they replaced, still accepted — and in
places still required — by banks and corporate channels. The German DK variants
`pain.001.003.03` / `pain.008.003.02` reached end-of-life in November 2022.
All six are selectable via `.schema(…)` and validated against their own XSD in CI.

### Versioning policy

This crate tracks a moving regulatory target, so releases are documented
per-version in [CHANGELOG.md](CHANGELOG.md), which always calls out:

- any change to the **schema version or shape** of emitted XML, and
- any **API signature** change.

While `0.x`, a bump of the **minor** version (`0.4 → 0.5`) may contain breaking
changes; the patch version is reserved for fixes that do not change emitted
output or public signatures. Pin `sepa = "0.5"` and read the changelog before
moving to `0.6`. A change to the *default* schema version will always be a minor
bump with a migration note.

---

## 🔬 Testing

| Layer | What it checks |
|---|---|
| Unit + integration | 310 unit + integration tests, plus 67 documentation tests |
| **XSD validation** | generated documents validated against the real ISO 20022 schemas with `xmllint`, **every schema version**, in CI |
| **Fuzzing** | `cargo-fuzz` targets over parsers, validators and builders (`fuzz/`) |
| Exhaustive sweep | all 1,114,112 Unicode code points × both transliteration styles |
| **Registry conformance** | the 78 real IBAN examples SWIFT publishes, structure and checksum |

Schema validation is necessary but not sufficient — `<InstdAmt>0</InstdAmt>`,
a 140-character name and `<BICFI>NOTPROVIDED</BICFI>` all pass the XSD and are
rejected by banks. Those rules live in `validate` and are enforced by `build()`.

---

## 📦 Dependencies

| Crate | Role |
|---|---|
| [`thiserror`](https://crates.io/crates/thiserror) | Ergonomic error derives |
| [`quick-xml`](https://crates.io/crates/quick-xml) | Correct, streaming XML parsing |
| [`serde`](https://crates.io/crates/serde) | Serialize/Deserialize on all types (`serde` feature) |
| [`serde_json`](https://crates.io/crates/serde_json) | `camt054::parse_simple_json` (`json` feature) |
| [`time`](https://crates.io/crates/time) | `IsoDate` ↔ `time::Date` conversion (`time` feature) |
| [`chrono`](https://crates.io/crates/chrono) | `IsoDate` ↔ `chrono::NaiveDate` conversion (`chrono` feature) |

`time` and `chrono` are conversion-only: the crate's own calendar arithmetic is
dependency-free, so it behaves identically whichever features are enabled.

---

## 📄 License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
