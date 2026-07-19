# sepa

[![Crates.io](https://img.shields.io/crates/v/sepa.svg)](https://crates.io/crates/sepa)
[![Docs.rs](https://img.shields.io/docsrs/sepa)](https://docs.rs/sepa)
[![CI](https://github.com/hupe1980/sepa/actions/workflows/ci.yml/badge.svg)](https://github.com/hupe1980/sepa/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![MSRV](https://img.shields.io/badge/rustc-1.85+-orange.svg)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0.html)

> 🏦 **SEPA payment utilities for Rust.**
> ⚡ Zero I/O. No async. 🔢 No `f64` in monetary arithmetic.
> ✅ Generated files are validated against the real ISO 20022 XSDs in CI.

---

## ✨ Features

| Module | What it provides |
|---|---|
| `iban` | 🔍 IBAN validation — ISO 13616 mod-97 + 89-country registry + SEPA membership |
| `bic` | 🔍 BIC/SWIFT validation — ISO 9362, SEPA pattern |
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
| `validate` | ✅ EPC field rules the XSD does not enforce |
| `charset` | 🔤 SEPA Basic Latin set + the real **EPC217-08** conversion table |
| `ct_to_eur_str` / `ct_from_eur_str` | 💶 Integer-safe `i64 ct ↔ "1234.56"` round-trip |

All commonly used types are re-exported at the crate root (`sepa::SequenceType`, `sepa::DirectDebitScheme`, …).

---

## 🚀 Quick start

```toml
[dependencies]
sepa = "0.4"
```

### 📤 Building payment files

`build()` returns a `Result`: the batch is checked against the EPC rules before
any XML is produced.

```rust
use sepa::{
    validate_iban, validate_creditor_id,
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
            .execution_date("2026-07-20")
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
        DirectDebitGroup::new("Stadtwerke GmbH", &creditor, ci.clone())
            .sequence_type(SequenceType::Frst)
            .collection_date("2026-07-20")
            .add_entry(DirectDebitEntry::new(
                "MND-001", "2026-06-01", "Neu Kunde", debtor.clone(), 5_000, "E2E-001",
            )),
    )
    .add_group(
        DirectDebitGroup::new("Stadtwerke GmbH", &creditor, ci)
            .sequence_type(SequenceType::Rcur)
            .collection_date("2026-07-18")
            .add_entry(DirectDebitEntry::new(
                "MND-002", "2024-06-01", "Alt Kunde", debtor, 7_500, "E2E-002",
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

### ✅ Validation and character handling

The published ISO schemas are far more permissive than the banks. A file can
validate cleanly against the XSD and still be rejected — a zero amount, a
141-character name, `<BICFI>NOTPROVIDED</BICFI>`. This crate enforces the EPC
rules that actually decide acceptance:

```rust
use sepa::{Pain001Builder, ValidationError, validate_iban};

let iban = validate_iban("DE89370400440532013000").unwrap();

// An empty message is schema-invalid (PmtInf and CdtTrfTxInf are both 1..n).
assert!(matches!(
    Pain001Builder::new("Acme GmbH").build(),
    Err(ValidationError::EmptyBatch),
));
```

| Rule | Enforced |
|---|---|
| `MsgId`, `EndToEndId`, `MndtId` | 1–35 chars, no leading/trailing `/`, no `//` |
| Party name `Nm` | 1–70 chars (XSD permits 140) |
| Remittance `Ustrd` | 1–140 chars |
| `InstdAmt` | 0.01 – 999,999,999.99 EUR |
| Dates | real `YYYY-MM-DD` calendar dates |
| Batch | at least one transaction |
| Direct debit | Creditor Identifier required |
| Ultimate party `Nm` | 1–70 chars; only `Nm` and `Id` permitted |
| Purpose codes | 1–4 alphanumerics, in the correct code set |

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
            "Stadtwerke GmbH", &iban, validate_creditor_id("DE98ZZZ09999999999")?,
        )
        .collection_date("2026-07-20")
        .add_entry(
            DirectDebitEntry::new("MND-042", "2024-06-01", "Max Mustermann", debtor, 7_500, "E2E-1")
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
        sepa::DirectDebitGroup::new("Payroll GmbH", &iban, validate_creditor_id("DE98ZZZ09999999999")?)
            .collection_date("2026-07-20"),
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

    // A batch-booked entry carries one detail per original transaction.
    if entry.batch_booked {
        for d in &entry.details {
            println!("  ↳ {:?} {:?}", d.end_to_end_id, d.amount_ct);
        }
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

**camt.054 — Intraday Notifications** (`json` feature)

```toml
sepa = { version = "0.4", features = ["json"] }
```

```rust
# #[cfg(feature = "json")] {
use sepa::camt054::parse_simple_json;

let entry = parse_simple_json(&serde_json::json!({
    "iban":       "DE89370400440532013000",
    "amount_eur": "155.42",
    "reference":  "Invoice-001",
    "date":       "2026-07-10"
})).unwrap();
assert_eq!(entry.amount_ct, 15_542);
assert_eq!(entry.to_ledger_ct(), -15_542); // credit → negative in open-items
# }
```

---

## 📋 Standards

| Standard | Module | Notes |
|---|---|---|
| ISO 13616-1 + SWIFT IBAN Registry | `iban` | 89-country length table |
| EPC409-09 v8.0 | `iban` | SEPA scheme country list (42 codes) |
| ISO 9362 | `bic` | 8- and 11-char BIC, SEPA pattern |
| EPC262-08 | `creditor_id` | Creditor Identifier check digits |
| ISO 20022 pain.001.001.09 | `pain001` | SCT + SCT Instant — **default** |
| ISO 20022 pain.001.003.03 | `pain001` | Legacy DK V2.7 |
| ISO 20022 pain.008.001.08 | `pain008` | SDD CORE + B2B — **default** |
| ISO 20022 pain.008.003.02 | `pain008` | Legacy DK V2.7 |
| ISO 20022 pain.002 (all variants) | `pain002` | namespace-agnostic parser |
| ISO 20022 camt.053 (v02–v13) | `camt053` | namespace-agnostic parser |
| EPC217-08 | `charset` | SEPA character set + conversion table |
| EPC SEPA Rulebooks 2023/2025 | all | SCT, SDD Core, SDD B2B, SCT Inst |

### Schema versions

`pain.001.001.09` and `pain.008.001.08` are the versions the EPC 2023 rulebooks
mandated from 19 November 2023, and are the defaults here. The German DK
variants `pain.001.003.03` / `pain.008.003.02` reached end-of-life in November
2022 and remain available via `.schema(…)` for systems still pinned to them.

---

## 🔬 Testing

| Layer | What it checks |
|---|---|
| Unit + integration | 306 tests |
| **XSD validation** | generated documents validated against the real ISO 20022 schemas with `xmllint`, in CI |
| **Fuzzing** | `cargo-fuzz` targets over parsers, validators and builders (`fuzz/`) |
| Exhaustive sweep | all 1,114,112 Unicode code points × both transliteration styles |

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

---

## 📄 License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
