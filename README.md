# sepa

[![Crates.io](https://img.shields.io/crates/v/sepa.svg)](https://crates.io/crates/sepa)
[![Docs.rs](https://img.shields.io/docsrs/sepa)](https://docs.rs/sepa)
[![CI](https://github.com/hupe1980/sepa/actions/workflows/ci.yml/badge.svg)](https://github.com/hupe1980/sepa/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#-license)
[![MSRV](https://img.shields.io/badge/rustc-1.88+-orange.svg)](https://blog.rust-lang.org/2025/06/26/Rust-1.88.0/)

> 🏦 **SEPA payment utilities for Rust.**
> ⚡ Zero I/O. No async. 🔢 No `f64` in monetary arithmetic. 📅 No hand-formatted dates.
> ✅ Every generated file is validated in CI against the real ISO 20022 XSDs — *and* against
> the stricter German GBIC 5 validation subsets.

---

## ✨ What's in it

| Module | What it provides |
|---|---|
| `iban` | IBAN validation — ISO 13616 mod-97 **+ national BBAN structure** + 89-country registry + SEPA membership |
| `bic` | BIC/SWIFT validation — ISO 9362, location-code rules, real country codes |
| `creditor_id` | SEPA Creditor Identifier — EPC AT-02 |
| `pain001` | 📤 SEPA Credit Transfer — SCT, SCT Instant, scheduled instant |
| `pain008` | 📤 SEPA Direct Debit — CORE + B2B |
| `pain007` | 📤 SEPA Direct Debit **reversal** |
| `pain002` | 📥 Payment Status Report **+ Verification of Payee** |
| `camt052` / `camt053` / `camt054` | 📥 Intraday report, end-of-day statement, notifications & returns |
| `camt` | Shared entry model — batch bookings, `Btch/PmtInfId` back-reference, reconciliation guard |
| `address` | `PstlAdr` — structured & hybrid, ready for the **15 Nov 2026** cut-over |
| `reference` | ISO 11649 RF Creditor Reference — validate **and** generate |
| `party` · `purpose` · `country` | Ultimate parties, the two distinct purpose code sets, ISO 3166 |
| `date` | `IsoDate` / `IsoDateTime` — validated at construction, `time` + `chrono` interop |
| `validate` · `charset` | EPC rules the XSD does not enforce; the real EPC217-08 conversion table |

Common types are re-exported at the crate root.
**[Guides and documentation →](https://hupe1980.github.io/sepa/)** ·
[API reference on docs.rs](https://docs.rs/sepa)

---

## 🚀 Quick start

```sh
cargo add sepa
```

Optional features: `serde`, `time`, `chrono` — see [Dependencies](#-dependencies).

```rust
use sepa::{
    CreditTransferEntry, CreditTransferGroup, DirectDebitEntry, DirectDebitGroup,
    IsoDate, Pain001Builder, Pain008Builder, SequenceType,
    validate_creditor_id, validate_iban,
};

let creditor = validate_iban("DE89 3704 0044 0532 0130 00")?;
let debtor   = validate_iban("NL91ABNA0417164300")?;
let ci       = validate_creditor_id("DE98ZZZ09999999999")?;

// pain.001 — Überweisung
let ct = Pain001Builder::new("Acme GmbH")
    .msg_id("CT-2026-07-001")
    .add_group(
        CreditTransferGroup::new("Acme GmbH", &creditor)
            .execution_date(IsoDate::new(2026, 7, 20)?)   // typed, never a string
            .add_entry(
                CreditTransferEntry::new("Supplier AG", debtor.clone(), 12_000, "INV-001")
                    .with_description("Rechnung 2026-07"), // 12_000 ct, never f64
            ),
    )
    .build()?;

// pain.008 — a collection run with FRST and RCUR in ONE file.
// SeqTp lives on the group, so each sequence type needs its own.
let dd = Pain008Builder::new("Stadtwerke GmbH")
    .msg_id("DD-2026-07-001")
    .add_group(
        DirectDebitGroup::new("Stadtwerke GmbH", &creditor, &ci)
            .sequence_type(SequenceType::Frst)
            .collection_date(IsoDate::new(2026, 7, 20)?)
            .add_entry(DirectDebitEntry::new(
                "MND-1", "2026-06-01".parse()?, "Neu Kunde", debtor.clone(), 5_000, "E2E-1",
            )),
    )
    .add_group(
        DirectDebitGroup::new("Stadtwerke GmbH", &creditor, &ci)
            .sequence_type(SequenceType::Rcur)
            .collection_date(IsoDate::new(2026, 7, 18)?)
            .add_entry(DirectDebitEntry::new(
                "MND-2", "2024-06-01".parse()?, "Alt Kunde", debtor, 7_500, "E2E-2",
            )),
    )
    .build()?;

assert!(dd.contains("<SeqTp>FRST</SeqTp>") && dd.contains("<SeqTp>RCUR</SeqTp>"));
# let _ = ct;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`build()` returns a `Result` — the batch is checked against the EPC rules
*before* any XML exists. Each `add_group` becomes one `PmtInf`; sequence type,
execution date, debtor account and batch booking all live at that level.

---

## 💡 Design

**Invalid states are unconstructible, not caught late.** An `Iban` has passed
mod-97 *and* the registered national BBAN structure. An `IsoDate` is a real
calendar day. A `PostalAddress` carries the town and country the EPC requires
from 15 November 2026 — so the address format that is about to be rejected
cannot be built at all.

```rust
use sepa::{IsoDate, PostalAddress, validate_iban};
use sepa::iban::{BbanCharClass, IbanError};

// A capital O typed for a zero — mod-97 lets this through 96 times in 97.
assert!(matches!(
    validate_iban("DE8937O400440532013000"),
    Err(IbanError::InvalidBbanFormat { position: 7, expected: BbanCharClass::Digit, .. }),
));

assert!("2026-02-30".parse::<IsoDate>().is_err());      // February never has 30 days
assert!(PostalAddress::new("Atlantis", "ZZ").is_err()); // ZZ is not a country
```

**Failures name the row.** In a collection run of ten thousand,
"`InstdAmt` is out of range" is only actionable once it says which one:

```rust
# use sepa::{DirectDebitEntry, DirectDebitGroup, IsoDate, Pain008Builder,
#            ValidationError, validate_creditor_id, validate_iban};
# let iban = validate_iban("DE89370400440532013000")?;
# let ci = validate_creditor_id("DE98ZZZ09999999999")?;
# let date = IsoDate::new(2026, 7, 20)?;
let err = Pain008Builder::new("Stadtwerke GmbH")
    .msg_id("DD-1")
    .add_group(
        DirectDebitGroup::new("Stadtwerke GmbH", &iban, &ci)
            .collection_date(date)
            .add_entry(DirectDebitEntry::new("MND-1", date, "Erste", iban.clone(), 100, "E2E-1"))
            .add_entry(DirectDebitEntry::new("MND-2", date, "Zweiter", iban.clone(), 0, "E2E-2")),
    )
    .build()
    .unwrap_err();

assert_eq!(err.location.transaction, Some(1));   // the second collection
assert!(matches!(err.kind, ValidationError::AmountOutOfRange { .. }));
assert!(err.to_string().starts_with("PmtInf[0]/Tx[1]: "));
# Ok::<(), Box<dyn std::error::Error>>(())
```

**Text is transliterated, not mangled.** The EPC217-08 table is transcribed from
the published spreadsheet — 1010 mappings, not an approximation — so `Æ→A` (not
`AE`), `Щ→SHT`, `€→E`:

```rust
use sepa::{Transliteration, transliterate};

assert_eq!(transliterate("Müller & Söhne", Transliteration::German), "Mueller + Soehne");
assert_eq!(transliterate("Ψυχή", Transliteration::Epc), "PSychi");
```

Use `.charset(CharsetPolicy::Strict)` to reject instead of rewrite.

---

## 🔄 The whole direct debit lifecycle

| Event | Message | Module |
|---|---|---|
| You collect | pain.008 | `pain008` |
| The bank accepts or rejects | pain.002 | `pain002` |
| The money arrives, batch-booked | camt.053 / camt.054 | `camt053`, `camt054` |
| The debtor claims it back | camt.054 return | `camt054` |
| **You** send it back | pain.007 | `pain007` |

A reversal has to restate the collection it undoes. Rather than retyping a dozen
fields, hand it the objects you already sent:

```rust
# use sepa::{DirectDebitEntry, DirectDebitGroup, IsoDate, Pain007Builder,
#            ReversalEntry, ReversalGroup, ReversalReason,
#            validate_creditor_id, validate_iban};
# let creditor = validate_iban("DE89370400440532013000")?;
# let debtor = validate_iban("NL91ABNA0417164300")?;
# let ci = validate_creditor_id("DE98ZZZ09999999999")?;
let group = DirectDebitGroup::new("Stadtwerke GmbH", &creditor, &ci)
    .collection_date(IsoDate::new(2026, 7, 20)?);
let entry = DirectDebitEntry::new(
    "MND-42", "2024-06-01".parse()?, "Max Mustermann", debtor, 7_500, "E2E-1",
);

let xml = Pain007Builder::new("Stadtwerke GmbH", "DD-2026-07-001")
    .msg_id("RVSL-001")
    .add_group(
        ReversalGroup::new("DD-2026-07-001")
            .add_entry(ReversalEntry::reverse(&group, &entry, ReversalReason::Ms02)),
    )
    .build()?;

assert!(xml.contains("<RvsdInstdAmt Ccy=\"EUR\">75.00</RvsdInstdAmt>"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

The mandate, creditor identifier, scheme, sequence type, collection date and
both parties are copied across, so the reversal cannot disagree with the
collection it reverses. Reversing more than was collected is rejected.

---

## ✅ Verification of Payee

Mandatory for euro credit transfers since **9 October 2025**. The payer's PSP
checks the payee name against the account before execution and reports the
outcome inside the pain.002 — so a status report is no longer only about
acceptance and rejection:

```rust,no_run
use sepa::{VerificationOutcome, parse_pain002};

# let xml = "";
for block in &parse_pain002(xml)?.payment_info_statuses {
    for tx in &block.transactions {
        match tx.status.as_ref().and_then(|s| s.verification()) {
            // The payee's *actual* name comes back for the payer to confirm.
            Some(VerificationOutcome::CloseMatch) => println!("{:?}", tx.additional_info),
            Some(VerificationOutcome::NoMatch) => println!("proceeding shifts liability to you"),
            Some(VerificationOutcome::NotApplicable) => println!("no answer from payee's PSP"),
            Some(VerificationOutcome::Match) | None => {}
        }
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

A verification status is deliberately **not** an acceptance: `RCVC` says a name
matched, which is a different question from whether the payment was taken — so
`is_accepted()` stays `false` and `verification()` answers the other question.

---

## 📥 Reading bank files

Parsing is built on [`quick-xml`](https://crates.io/crates/quick-xml), so
comments, entities, CDATA and self-closing tags are handled correctly, and
`<ns2:Document>` parses identically to `<Document xmlns=…>`. `<!DOCTYPE>` is
rejected outright as defence in depth against entity expansion.

```rust,no_run
use sepa::parse_camt053;

# let xml = "";
let stmt = &parse_camt053(xml)?.statements[0];

for entry in &stmt.entries {
    println!("{:+} ct  {}", entry.signed_ct(), entry.reference().unwrap_or(""));

    if entry.batch_booked {
        // `Btch` names the PmtInfId of the group you sent — that is how a
        // booking is matched to your own file without guessing from amounts.
        if let Some(b) = &entry.batch {
            println!("  batch from {:?}", b.payment_info_id);
        }
        // Do the parts account for the whole? If not, escalate rather than post.
        assert!(entry.details_reconcile());
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

Bank input is kept verbatim *and* typed alongside: `booking_date()` returns an
`IsoDate` and `booking_date_raw` the text that arrived, so a non-conforming file
is readable rather than rejected. `reference()` joins every `RmtInf/Ustrd`
occurrence — German banks split a long *Verwendungszweck* into 35-character
chunks, and reading only the first cuts it where the invoice number sits.

---

## 📋 Standards & schema versions

| Message | Versions (default first) |
|---|---|
| pain.001 | `.001.09` · `.001.03` · `.003.03` (DK, EOL) |
| pain.008 | `.001.08` · `.001.02` · `.003.02` (DK, EOL) |
| pain.007 | `.001.09` — the only version SEPA defines |
| pain.002 | parses `.001.10`, `.001.03`, DK variants |
| camt.052/053/054 | parses `.001.02` – `.001.13` |

Select with `.schema(…)`; the enums implement `FromStr` over both the message
identifier and the namespace URN, so the target version can come from config.

<details>
<summary><b>Why not ISO's newest versions?</b></summary>

ISO advises using the most recent message definition, and has published
`pain.001.001.13`, `pain.008.001.12` and `pain.002.001.15`. **For SEPA that
advice is wrong** — the version is fixed by the scheme rulebook, and sending
`pain.001.001.13` to a SEPA bank gets it rejected. `.001.09` / `.001.08` have
been the mandated versions since 19 November 2023, and nothing on the EPC's
roadmap moves SEPA past them. (15 November 2026 is a different rule: structured
addresses, not a newer message.) Those older versions live in the [ISO 20022
Message Archive](https://www.iso20022.org/catalogue-messages/iso-20022-messages-archive),
not the current-version catalogue.
</details>

| Standard | Used for |
|---|---|
| ISO 13616-1 + SWIFT IBAN Registry r102 | 89-country BBAN structure + length table |
| EPC409-09 v8.0 | SEPA scheme country list (42 codes) |
| ISO 9362 · ISO 3166-1 alpha-2 | BIC format; 249 country codes + `XK` |
| EPC262-08 · ISO 11649 | Creditor Identifier and RF reference check digits |
| EPC153-22 v2.1 | Structured addresses, 15 Nov 2026 cut-over |
| EPC103-24 | Verification of Payee outcomes |
| EPC217-08 | SEPA character set + conversion table |
| DK Anlage 3 GBIC 5 (TVS) | Stricter German validation subsets, applied in CI |
| EPC SEPA Rulebooks 2023/2025 | SCT, SCT Inst, SDD Core, SDD B2B |

### Scope — deliberately not included

| Not included | Why |
|---|---|
| Verification of Payee *requests* | The VoP scheme is a REST API with live PSP endpoints; this crate does no I/O. The VoP **results** in pain.002 *are* parsed. |
| `camt.055` recall / `camt.029` resolution | Recalling a credit transfer after settlement. A genuine candidate, simply not built yet. |
| IBAN → BIC derivation | Needs a licensed, constantly-changing directory. A stale table silently misroutes payments. |
| Banking-day calendars | `IsoDate::plus_days` is calendar arithmetic and says so. TARGET2 holidays are a policy input. |
| Currencies other than EUR | The SEPA schemes are EUR-only. `camt` parsing *does* report the entry currency, because statements are not. |

---

## 🔬 Testing

| Layer | What it checks |
|---|---|
| Unit + integration | 357 tests, plus 99 documentation tests |
| **XSD validation** | every generated document, every schema version, via `xmllint` in CI |
| **DK subset validation** | the default SCT/SDD output *also* against GBIC 5 — stricter than ISO, and what German banks apply |
| **Fuzzing** | three `cargo-fuzz` targets over parsers, identifiers, addresses and builders |
| Exhaustive sweep | all 1,114,112 Unicode code points × both transliteration styles |
| Registry conformance | the 78 real IBAN examples SWIFT publishes |

Schema validation is necessary but not sufficient — `<InstdAmt>0</InstdAmt>`, a
140-character `<Nm>` and `<BICFI>NOTPROVIDED</BICFI>` all pass the XSD and are
rejected by banks. So do a `ZZ` Creditor Identifier, two payment groups sharing
a `PmtInfId`, and a `ReqdExctnDt/DtTm` on a non-instant transfer. Those rules
live in `validate` and are enforced by `build()`.

## 📦 Dependencies

| Crate | Role |
|---|---|
| [`thiserror`](https://crates.io/crates/thiserror) | Error derives |
| [`quick-xml`](https://crates.io/crates/quick-xml) | Correct XML parsing |
| [`serde`](https://crates.io/crates/serde) | `serde` feature — `Serialize` / `Deserialize` on every public type |
| [`time`](https://crates.io/crates/time) · [`chrono`](https://crates.io/crates/chrono) | Date interop (`time` / `chrono` features) |

`time` and `chrono` are conversion-only: the crate's own calendar arithmetic is
dependency-free and behaves identically whichever features are enabled.

## 🔖 Versioning

This crate tracks a moving regulatory target, so releases are documented
per-version in [CHANGELOG.md](CHANGELOG.md), which always calls out changes to
**emitted output** and to **public API** separately. While `0.x`, a minor bump
(`0.5 → 0.6`) may break both; a patch release changes neither. Pin
`sepa = "0.6"` and read the changelog before moving on.

## 📄 License

[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
