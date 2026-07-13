# sepa

[![Crates.io](https://img.shields.io/crates/v/sepa.svg)](https://crates.io/crates/sepa)
[![Docs.rs](https://img.shields.io/docsrs/sepa)](https://docs.rs/sepa)
[![CI](https://github.com/hupe1980/sepa/actions/workflows/ci.yml/badge.svg)](https://github.com/hupe1980/sepa/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![MSRV](https://img.shields.io/badge/rustc-1.85+-orange.svg)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0.html)

> **Pure SEPA payment utilities for Rust.**  
> Zero I/O. No async. No domain assumptions. No `f64` in monetary arithmetic.

---

## Features

| Module | What it provides |
|---|---|
| `iban` | IBAN validation — ISO 13616 mod-97 algorithm |
| `bic` | BIC validation — ISO 9362 format rules |
| `pain008` | pain.008.003.02 XML builder — SEPA Core Direct Debit (Lastschrift) |
| `pain001` | pain.001.003.03 XML builder — SEPA Credit Transfer (Überweisung) |
| `camt054` | Typed CAMT.054 entry — Bank-to-Customer Notification (ISO 20022) |
| `creditor_id` | SEPA Creditor Identifier validation — EPC AT-02 (mod-97) |
| `ct_to_eur_str` | Integer-safe `i64 ct → "1234.56"` formatter, no f64 |

---

## Quick start

```toml
[dependencies]
sepa = "0.2"
```

```rust
use sepa::{validate_iban, validate_bic, Pain008Builder, Pain001Builder};
use sepa::{DirectDebitEntry, CreditTransferEntry};
use sepa::pain008::SequenceType;

let iban = validate_iban("DE89 3704 0044 0532 0130 00").unwrap();
assert_eq!(iban.as_str(), "DE89370400440532013000");

// pain.008 — Direct Debit (Lastschrift)
let dd_xml = Pain008Builder::new("Creditor GmbH", &iban)
    .msg_id("DD-2026-07-001")
    .sequence_type(SequenceType::Rcur)
    .add_entry(
        DirectDebitEntry::new(
            "MND-001",
            "2024-06-01",
            "Max Mustermann",
            validate_iban("NL91ABNA0417164300").unwrap(),
            7_500,          // 75.00 EUR
            "E2E-001",
        ).with_description("Abschlag Juli 2026"),
    )
    .build_xml();

// pain.001 — Credit Transfer (Überweisung)
let ct_xml = Pain001Builder::new("Debtor GmbH", &iban)
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
```

### CAMT.054 (`json` feature)

```toml
sepa = { version = "0.2", features = ["json"] }
```

```rust
use sepa::camt054::parse_simple_json;

let raw = serde_json::json!({
    "iban": "DE89370400440532013000",
    "amount_eur": "155.42",
    "reference": "Invoice-001",
    "date": "2026-07-10"
});

let entry = parse_simple_json(&raw).unwrap();
assert_eq!(entry.amount_ct, 15_542);
assert_eq!(entry.to_ledger_ct(), -15_542); // credit → negative in open-items
```

---

## Standards

| Standard | Module |
|---|---|
| ISO 13616-1 | `iban` |
| ISO 9362 | `bic` |
| ISO 20022 pain.001.003.03 | `pain001` |
| ISO 20022 pain.008.003.02 | `pain008` |
| ISO 20022 camt.054 | `camt054` |
| EPC SEPA Core Rulebook | all |

---

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
