+++
title = "sepa — SEPA payment files for Rust"
description = "SEPA payment messages for Rust: checked identifiers, XML builders, bank-report parsers and integer money."
template = "index.html"
+++

## Why another SEPA crate

XSD validity does not establish SEPA profile compliance or bank acceptance.
For example, an ISO name field may allow 140 characters while the applicable
current customer-to-PSP SEPA profile limits it to 70. Verification of Payee
obligations depend on the applicable jurisdiction, PSP and implementation date.

This crate validates supported message rules and preserves unresolved reported
facts. It does not execute payments or certify a bank's acceptance.

<ul class="feature-grid">
<li><strong>Checked value types</strong><p>An <code>Iban</code> has passed mod-97 <em>and</em> the recorded national BBAN structure. An <code>IsoDate</code> is a real calendar day. Structured/hybrid addresses require town and country; optional content is validated when building.</p></li>
<li><strong>Integer money</strong><p>Monetary values use integer hundredths. Currency and direction remain explicit; no floating-point rounding.</p></li>
<li><strong>Faithful observations</strong><p>Unreadable amounts and directions remain visible. Signing returns <code>None</code> when the reported facts do not support a figure.</p></li>
<li><strong>Errors name the row</strong><p>A failure reports the ISO element <em>and</em> the group and transaction index — actionable in a collection run of ten thousand.</p></li>
<li><strong>Independent schema fixtures</strong><p>The generated fixture matrix is checked in CI against pinned ISO schemas and German GBIC 5 subsets. This is test evidence, not certification of every possible document.</p></li>
<li><strong>No transport or runtime</strong><p>Build strings or stream to your own writer. Storage and transport stay yours. Supply message IDs and payment dates; pin <code>created_at()</code> for reproducible output.</p></li>
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

    let xml = Pain008Builder::new("Stadtwerke GmbH", "DD-2026-07-001")
        .add_group(
            DirectDebitGroup::new("Stadtwerke GmbH", &creditor, &ci, IsoDate::new(2026, 7, 20)?)
                .sequence_type(SequenceType::Frst)
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
            DirectDebitGroup::new("Stadtwerke GmbH", &creditor, &ci, IsoDate::new(2026, 7, 18)?)
                .sequence_type(SequenceType::Rcur)
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

## Schemes covered

The scheme — not the message — carries the rules that decide whether a bank
accepts a file. Two schemes can share a message and share none of its rules.

| Scheme | | |
|---|---|---|
| **SCT** — Credit Transfer | `pain.001` · `pain.002` · `camt.05x` | ✅ |
| **SCT Inst** — Instant Credit Transfer | as SCT, `LclInstrm=INST` | ✅ |
| **SDD Core** | `pain.008` · `pain.007` · `pain.002` · `camt.05x` | ✅ |
| **SDD B2B** | as SDD Core, `LclInstrm=B2B` | ✅ |
| **OCT Inst** — One-Leg Out Instant | the same three messages, `SvcLvl=EOLO` | ✅ |

Verification of Payee *results* are read from `pain.002`. VoP, SEPA
Request-To-Pay and SPAA *requests* are REST APIs — SRTP's `pain.013`/`pain.014`
travel as JSON — so they are out of scope for a library that builds XML.

## Messages covered

| Direction | Message | What it is |
|---|---|---|
| 📤 Send | `pain.001` | Credit transfer — SCT, SCT Instant, **OCT Inst**, scheduled instant |
| 📤 Send | `pain.008` | Direct debit — CORE and B2B |
| 📤 Send | `pain.007` | Direct debit reversal — after settlement |
| 📤 Send | `camt.055` | Payment cancellation request; not proof of cancellation or return |
| 📥 Receive | `pain.002` | Status report, including Verification of Payee |
| 📥 Receive | `camt.029` | Resolution of investigation — the answer to a recall |
| 📥 Receive | `camt.052` | Intraday account report |
| 📥 Receive | `camt.053` | End-of-day statement |
| 📥 Receive | `camt.054` | Debit/credit notification and returns |

Plus IBAN and BIC validation, SEPA Creditor Identifiers, ISO 11649 RF
references, structured postal addresses, and the real EPC217-08 character
conversion table.

[Read the documentation →](/docs/)
