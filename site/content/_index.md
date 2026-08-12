+++
title = "sepa — SEPA payment files for Rust"
description = "A Rust crate for SEPA payments: IBAN and BIC validation, pain.001 credit transfers, pain.008 direct debits, pain.007 reversals, pain.002 status reports with Verification of Payee, and camt.05x statement parsing. Zero I/O, integer-only money, XSD-validated output."
template = "index.html"
+++

## Why another SEPA crate

A SEPA file is rejected for reasons an XML schema never sees. The published ISO
20022 schemas accept a zero amount, a 141-character name and a `NOTPROVIDED`
BIC; the banks do not. And the rules move — structured addresses become
mandatory on **15 November 2026**, Verification of Payee has been mandatory
since **9 October 2025**.

This crate encodes the rules that actually decide acceptance, and makes the
mistakes that cost money hard to express in the first place.

<ul class="feature-grid">
<li><strong>Invalid states don't compile</strong><p>An <code>Iban</code> has passed mod-97 <em>and</em> the registered national BBAN structure. An <code>IsoDate</code> is a real calendar day. Addresses carry the town and country the EPC will require.</p></li>
<li><strong>Integer money only</strong><p>Amounts are <code>i64</code> cents everywhere. No <code>f64</code> touches a monetary value, so 10 000 transactions of one cent total exactly 100.00 EUR.</p></li>
<li><strong>Errors name the row</strong><p>A failure reports the ISO element <em>and</em> the group and transaction index — actionable in a collection run of ten thousand.</p></li>
<li><strong>Validated twice over</strong><p>Every generated document is checked in CI against the real ISO schemas and against the stricter German GBIC 5 validation subsets.</p></li>
<li><strong>Zero I/O, no async</strong><p>The crate builds and parses strings. Transport, retries and storage stay yours; nothing is hidden behind a runtime.</p></li>
<li><strong>Two required dependencies</strong><p><code>thiserror</code> and <code>quick-xml</code>. Serde, <code>time</code> and <code>chrono</code> are opt-in features.</p></li>
</ul>

## A collection run in one file

Sequence type lives on the payment group, so a real direct debit run — first
collections alongside recurring ones — is one message, not two submissions.

```rust
use sepa::{
    DirectDebitEntry, DirectDebitGroup, IsoDate, Pain008Builder, SequenceType,
    validate_creditor_id, validate_iban,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let creditor = validate_iban("DE89 3704 0044 0532 0130 00")?;
    let debtor = validate_iban("NL91ABNA0417164300")?;
    let ci = validate_creditor_id("DE98ZZZ09999999999")?;

    let xml = Pain008Builder::new("Stadtwerke GmbH")
        .msg_id("DD-2026-07-001")
        .add_group(
            DirectDebitGroup::new("Stadtwerke GmbH", &creditor, &ci)
                .sequence_type(SequenceType::Frst)
                .collection_date(IsoDate::new(2026, 7, 20)?)
                .add_entry(DirectDebitEntry::new(
                    "MND-1",
                    "2026-06-01".parse()?,
                    "Neu Kunde",
                    debtor.clone(),
                    5_000,
                    "E2E-1",
                )),
        )
        .add_group(
            DirectDebitGroup::new("Stadtwerke GmbH", &creditor, &ci)
                .sequence_type(SequenceType::Rcur)
                .collection_date(IsoDate::new(2026, 7, 18)?)
                .add_entry(DirectDebitEntry::new(
                    "MND-2",
                    "2024-06-01".parse()?,
                    "Alt Kunde",
                    debtor,
                    7_500,
                    "E2E-2",
                )),
        )
        .build()?;

    assert!(xml.contains("<SeqTp>FRST</SeqTp>"));
    assert!(xml.contains("<SeqTp>RCUR</SeqTp>"));
    Ok(())
}
```

`build()` returns a `Result`. The batch is checked against the EPC field rules
before any XML exists, so a rejected run never reaches your bank.

## Messages covered

| Direction | Message | What it is |
|---|---|---|
| 📤 Send | `pain.001` | Credit transfer — SCT, SCT Instant, scheduled instant |
| 📤 Send | `pain.008` | Direct debit — CORE and B2B |
| 📤 Send | `pain.007` | Direct debit reversal |
| 📥 Receive | `pain.002` | Status report, including Verification of Payee |
| 📥 Receive | `camt.052` | Intraday account report |
| 📥 Receive | `camt.053` | End-of-day statement |
| 📥 Receive | `camt.054` | Debit/credit notification and returns |

Plus IBAN and BIC validation, SEPA Creditor Identifiers, ISO 11649 RF
references, structured postal addresses, and the real EPC217-08 character
conversion table.

[Read the documentation →](/docs/)
