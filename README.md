# sepa

[![Crates.io](https://img.shields.io/crates/v/sepa.svg)](https://crates.io/crates/sepa)
[![Docs.rs](https://img.shields.io/docsrs/sepa)](https://docs.rs/sepa)
[![CI](https://github.com/hupe1980/sepa/actions/workflows/ci.yml/badge.svg)](https://github.com/hupe1980/sepa/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![MSRV](https://img.shields.io/badge/rustc-1.85+-orange.svg)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0.html)

> 🏦 **SEPA payment utilities for Rust.**
> ⚡ Zero I/O. No async. 🔢 No `f64` in monetary arithmetic.

---

## ✨ Features

| Module | What it provides |
|---|---|
| `iban` | 🔍 IBAN validation — ISO 13616 mod-97 + 56-country length registry |
| `bic` | 🔍 BIC/SWIFT validation — ISO 9362 |
| `creditor_id` | 🔍 SEPA Creditor Identifier — EPC AT-02 |
| `pain001` | 📤 pain.001 XML builder — SCT (`003.03`) + SCT Instant (`001.09`) |
| `pain008` | 📤 pain.008 XML builder — SDD CORE + B2B |
| `pain002` | 📥 pain.002 XML parser — Payment Status Report (bank → customer) |
| `camt053` | 📥 camt.053 XML parser — Bank-to-Customer Statement (end-of-day) |
| `camt054` | 📥 camt.054 typed entry — Bank-to-Customer Notification |
| `ct_to_eur_str` / `ct_from_eur_str` | 💶 Integer-safe `i64 ct ↔ "1234.56"` round-trip |

All commonly used types are re-exported at the crate root (`sepa::SequenceType`, `sepa::DirectDebitScheme`, …).

---

## 🚀 Quick start

```toml
[dependencies]
sepa = "0.3"
```

### 📤 Building payment files

```rust
use sepa::{
    validate_iban,
    Pain008Builder, DirectDebitEntry, SequenceType, DirectDebitScheme,
    Pain001Builder, CreditTransferEntry, LocalInstrument,
};

let creditor = validate_iban("DE89 3704 0044 0532 0130 00").unwrap();
assert_eq!(creditor.bban(), "370400440532013000");           // BBAN accessor
assert_eq!(creditor.to_string(), "DE89 3704 0044 0532 0130 00"); // grouped Display

// pain.008 — SEPA Core Direct Debit (Lastschrift)
let dd_xml = Pain008Builder::new("Stadtwerke GmbH", &creditor)
    .msg_id("DD-2026-07-001")
    .sequence_type(SequenceType::Rcur)
    .add_entry(
        DirectDebitEntry::new(
            "MND-001", "2024-06-01",
            "Max Mustermann",
            validate_iban("NL91ABNA0417164300").unwrap(),
            7_500,          // 75.00 EUR — integer cents, never f64
            "E2E-001",
        ).with_description("Abschlag Juli 2026"),
    )
    .build_xml();

// pain.008 — SEPA B2B Direct Debit
let _b2b = Pain008Builder::new("Stadtwerke GmbH", &creditor)
    .scheme(DirectDebitScheme::B2b)
    .add_entry(DirectDebitEntry::new(
        "MND-B2B-001", "2024-01-01", "Corporate AG",
        validate_iban("DE29100500005001065004").unwrap(), 50_000, "INV-001",
    ))
    .build_xml();

// pain.001 — SEPA Credit Transfer (Überweisung)
let ct_xml = Pain001Builder::new("Debtor GmbH", &creditor)
    .msg_id("CT-2026-07-001")
    .add_entry(
        CreditTransferEntry::new(
            "Supplier AG",
            validate_iban("NL91ABNA0417164300").unwrap(),
            12_000,         // 120.00 EUR
            "REFUND-001",
        ).with_description("Erstattung 2025"),
    )
    .build_xml();

// ⚡ pain.001 — SCT Instant (10-second settlement, pain.001.001.09 namespace)
let _inst = Pain001Builder::new("Debtor GmbH", &creditor)
    .local_instrument(LocalInstrument::Inst)
    .add_entry(CreditTransferEntry::new(
        "Recipient", creditor.clone(), 500, "INST-001",
    ))
    .build_xml();
```

### 💾 Streaming large batches (no intermediate `String`)

```rust
use sepa::{Pain008Builder, validate_iban};
use std::io::BufWriter;
use std::fs::File;

let iban = validate_iban("DE89370400440532013000").unwrap();
let builder = Pain008Builder::new("Payroll GmbH", &iban)
    .msg_id("PAYROLL-2026-07");
// … .add_entry(…) × 10 000 …

// Streams directly to file — zero intermediate String allocation
let file = File::create("payroll.xml").unwrap();
builder.write_xml_to_io(&mut BufWriter::new(file)).unwrap();
```

### 📥 Parsing bank responses

**pain.002 — Payment Status Report**

```rust
use sepa::{parse_pain002, PaymentStatus};

// xml: &str received from bank via EBICS/FinTS
let doc = parse_pain002(xml).unwrap();

if doc.is_fully_accepted() {
    println!("✅ Batch accepted ({})", doc.group_status.unwrap());
} else {
    for tx in doc.rejected_transactions() {
        println!("❌ {} — {:?}", tx.original_end_to_end_id, tx.reason_codes);
    }
}
```

**camt.053 — End-of-Day Statement**

```rust
use sepa::parse_camt053;

// xml: &str received from bank (camt.053.001.02 / .06 / .08 all accepted)
let doc = parse_camt053(xml).unwrap();
let stmt = &doc.statements[0];

println!("Account: {}", stmt.account_iban);
println!("Closing: {} ct", stmt.closing_balance().unwrap().signed_ct());

for entry in &stmt.entries {
    println!("{:+} ct  {}", entry.signed_ct(), entry.reference.as_deref().unwrap_or(""));
}
```

**camt.054 — Intraday Notifications** (`json` feature)

```toml
sepa = { version = "0.3", features = ["json"] }
```

```rust
use sepa::camt054::parse_simple_json;

let entry = parse_simple_json(&serde_json::json!({
    "iban":       "DE89370400440532013000",
    "amount_eur": "155.42",
    "reference":  "Invoice-001",
    "date":       "2026-07-10"
})).unwrap();
assert_eq!(entry.amount_ct, 15_542);
assert_eq!(entry.to_ledger_ct(), -15_542); // credit → negative in open-items
```

---

## 📋 Standards

| Standard | Module | Notes |
|---|---|---|
| ISO 13616-1 + SWIFT IBAN Registry | `iban` | 56-country length table |
| ISO 9362 | `bic` | 8- and 11-char BIC |
| EPC AT-02 | `creditor_id` | SEPA Creditor Identifier |
| ISO 20022 pain.001.003.03 | `pain001` | SCT — DK V2.7, German banking default |
| ISO 20022 pain.001.001.09 | `pain001` | SCT Instant — EPC Rulebook 2021+ |
| ISO 20022 pain.008.003.02 | `pain008` | SDD CORE + B2B |
| ISO 20022 pain.002 (all variants) | `pain002` | namespace-agnostic parser |
| ISO 20022 camt.053 (all variants) | `camt053` | namespace-agnostic parser |
| ISO 20022 camt.054 | `camt054` | notification types |
| EPC SEPA Rulebooks | all | SCT, SDD Core, SDD B2B, SCT Inst |

---

## 📦 Dependencies

| Crate | Role |
|---|---|
| [`thiserror`](https://crates.io/crates/thiserror) | Ergonomic error derives (required) |
| [`serde`](https://crates.io/crates/serde) | Serialize/Deserialize on all types (`serde` feature) |
| [`serde_json`](https://crates.io/crates/serde_json) | `camt054::parse_simple_json` (`json` feature) |

---

## 📄 License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
