# sepa

[![Crates.io](https://img.shields.io/crates/v/sepa.svg)](https://crates.io/crates/sepa)
[![Docs.rs](https://img.shields.io/docsrs/sepa)](https://docs.rs/sepa)
[![CI](https://github.com/hupe1980/sepa/actions/workflows/ci.yml/badge.svg)](https://github.com/hupe1980/sepa/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#-license)
[![MSRV](https://img.shields.io/badge/rustc-1.88+-orange.svg)](https://blog.rust-lang.org/2025/06/26/Rust-1.88.0/)

> 🏦 **SEPA payment utilities for Rust.**
> ⚡ No transport or async runtime. 🔢 Integer monetary arithmetic.
> 📅 No hand-formatted dates — and no defaulted ones.
> ✅ Generated fixtures are checked in CI against pinned ISO 20022 XSDs and
> German GBIC 5 subsets. This is evidence, not certification of every possible input.

---

## ✨ What's in it

| Module | What it provides |
|---|---|
| `iban` | IBAN validation **and generation** — ISO 13616 mod-97 + national BBAN structure + 89-country registry + SEPA membership |
| `bic` | BIC/SWIFT validation — ISO 9362:2022, real country codes, per-schema pattern gate |
| `creditor_id` | SEPA Creditor Identifier — EPC AT-02 |
| `pain001` | 📤 Credit transfers — **SCT, SCT Instant and OCT Inst**, plus scheduled instant |
| `pain008` | 📤 SEPA Direct Debit — CORE + B2B |
| `pain007` | 📤 SEPA Direct Debit **reversal** |
| `pain002` | 📥 Payment Status Report **+ Verification of Payee**, reasons at all three levels |
| `camt055` | 📤 Payment Cancellation Request — **recall** a file, a group or single transactions |
| `camt029` | 📥 Resolution of Investigation — the bank's answer to a recall |
| `camt052` / `camt053` / `camt054` | 📥 Intraday report, end-of-day statement, notifications & returns |
| `camt` | Shared entry model — batch bookings, `Btch/PmtInfId` back-reference, return fees, reconciliation guard |
| `address` | `PstlAdr` — this library emits structured & hybrid addresses |
| `reference` | ISO 11649 RF Creditor Reference — validate **and** generate |
| `party` · `purpose` · `country` · `currency` | Ultimate parties, the two distinct purpose code sets, ISO 3166, ISO 4217 |
| `date` | `IsoDate` / `IsoDateTime` — validated at construction, `time` + `chrono` interop |
| `validate` · `charset` | EPC rules the XSD does not enforce; the real EPC217-08 conversion table |

### Coverage, by scheme

The EPC runs **five payment schemes**, and the scheme — not the message — is
what carries the rules that decide whether a bank accepts a file. All five are
covered:

| Scheme | Messages | |
|---|---|---|
| **SCT** — Credit Transfer | pain.001 out · pain.002, camt.05x back | ✅ |
| **SCT Inst** — Instant Credit Transfer | pain.001 `INST`, incl. scheduled | ✅ |
| **SDD Core** | pain.008 out · pain.007 reversal · pain.002, camt.05x back | ✅ |
| **SDD B2B** | pain.008 `B2B` | ✅ |
| **OCT Inst** — One-Leg Out Instant | the same three messages, `EOLO` rules | ✅ |

DK-profile Verification of Payee *results* are interpreted from pain.002. VoP, SRTP and SPAA
*requests* are REST APIs and are out of scope — see the scope table below.

Common types are re-exported at the crate root.
**[Guides and documentation →](https://hupe1980.github.io/sepa/)** ·
[API reference on docs.rs](https://docs.rs/sepa)

---

## 🚀 Quick start

```sh
cargo add sepa@0.9
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
let ct = Pain001Builder::new("Acme GmbH", "CT-2026-07-001")
    .add_group(
        CreditTransferGroup::new("Acme GmbH", &creditor, IsoDate::new(2026, 7, 20)?)   // typed, never a string
            .add_entry(
                CreditTransferEntry::new("Supplier AG", debtor.clone(), 12_000, "INV-001")
                    .with_description("Rechnung 2026-07"), // 12_000 ct, never f64
            ),
    )
    .build()?;

// pain.008 — a collection run with FRST and RCUR in ONE file.
// SeqTp lives on the group, so each sequence type needs its own.
let dd = Pain008Builder::new("Stadtwerke GmbH", "DD-2026-07-001")
    .add_group(
        DirectDebitGroup::new("Stadtwerke GmbH", &creditor, &ci, IsoDate::new(2026, 7, 20)?)
            .sequence_type(SequenceType::Frst)
            .add_entry(DirectDebitEntry::new(
                "MND-1", "2026-06-01".parse()?, "Neu Kunde", debtor.clone(), 5_000, "E2E-1",
            )),
    )
    .add_group(
        DirectDebitGroup::new("Stadtwerke GmbH", &creditor, &ci, IsoDate::new(2026, 7, 18)?)
            .sequence_type(SequenceType::Rcur)
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

Note what the constructors take: **`MsgId` and the payment date are arguments,
not defaults** — see [Design](#-design) for why neither has a safe one.

---

## 💡 Design

Readers require an ISO `Document`, exactly one matching body and an explicitly
supported resolved namespace. Missing/unknown namespaces, foreign lookalikes and
malformed trailing input are refused; there is no tolerant mode. All five readers
expose `parse_*_with_limits` with shared positive finite `ParseLimits`.
Defaults bound input (32 MiB), nodes (250,000), depth (256), text and attributes;
these are intake limits, not process-RSS or CPU guarantees.

Currency and direction are not guessed. Raw money survives unreadable and empty
values; signing refuses negative or contradictory observations. Charge inclusion
must be resolved separately before posting. Serde cannot restore trusted derived
detail signing. See [reader contracts](https://hupe1980.github.io/sepa/docs/reader-contracts/).

**Message IDs and payment dates are caller inputs.** Supply durable IDs and choose
dates using your banking calendar. Builders default only the creation timestamp
from the clock; pin `.created_at(...)` for reproducible output.

**Validated values enforce their construction contracts.** An `Iban` has passed
mod-97 *and* the registered national BBAN structure. An `IsoDate` is a real
calendar day in 0001–9999, enforced by component construction, parsing, serde
and optional date-library conversions. This is a library range, not the full
XSD year space. Timestamp conversions preserve whole-second instants by using
UTC when an external offset has seconds or exceeds ±14:00; offsets are never
rounded to minutes. An unrepresentable local date or required UTC date returns
a typed date error rather than panicking or clamping. Exact whole-minute offsets
within ±14:00 remain valid even when their UTC date falls outside the library range.
`PostalAddress` requires town and country for this library's
structured/hybrid scope, including serde restoration. EPC153-22 v2.2 still permits
unstructured addresses and sets no replacement cutoff. Address obligations depend
on scheme, participant geography and law; do not omit required data to fit an API.

Mutable payment inputs are checked at use, not certified by serde restoration.
`Party::default()` and incomplete restored parties cannot be emitted. Direct or
restored `Purpose::Other` and `CategoryPurpose::Other` values must contain 1–4
uppercase ASCII letters/digits without padding; builders check the exact spelling
they emit. Parse a raw string when trimming/uppercasing is intended. Well-shaped
unknown codes remain supported; this is not a current code-list membership check.

Text bounds count the complete emitted value after the selected character policy,
including surrounding whitespace. Blank-only refusal is library policy, not
`xs:string` whitespace collapse. Shared text validation rejects XML 1.0-forbidden
characters with `ValidationError::InvalidXmlCharacter`; identifiers are never
transliterated. Ordinary text still follows transliteration or strict refusal.
The shared XML escaper preserves literal carriage returns as `&#13;` wherever
raw text reaches it; this does not add a UTF-8 passthrough character policy.

```rust
use sepa::{IsoDate, PostalAddress, validate_iban};
use sepa::iban::{BbanCharClass, IbanError};

// A capital O occupies a digit-only BBAN position.
assert!(matches!(
    validate_iban("DE8937O400440532013000"),
    Err(IbanError::InvalidBbanFormat { position: 7, expected: BbanCharClass::Digit, .. }),
));

assert!("2026-02-30".parse::<IsoDate>().is_err());      // February never has 30 days
assert!(PostalAddress::new("Atlantis", "ZZ").is_err()); // ZZ is not a country
```

The same structure works in reverse. All three of SEPA's check-digit schemes
can *generate*, not only verify — and building an IBAN from a national bank code
still goes through the registry, so a malformed one fails here rather than at
the bank:

```rust
use sepa::{Iban, RfReference, creditor_id_check_digits, iban_check_digits};

assert_eq!(Iban::from_bban("DE", "3704 0044 0532 0130 00")?.as_str(), "DE89370400440532013000");
assert_eq!(iban_check_digits("NL", "ABNA0417164300"), "91");
assert_eq!(creditor_id_check_digits("09999999999", "DE"), "98");
assert_eq!(RfReference::generate("2348231")?.as_str(), "RF712348231");
# Ok::<(), Box<dyn std::error::Error>>(())
```

**A stricter validator is not a safer one.** ISO 9362:2022 widened the BIC's
business party prefix to four *alphanumerics*, and `E097AEXX` is a real BIC that
every `[A-Z]{6}` check refuses. `validate_bic` follows the current standard — and
because the pre-2019 schemas cannot hold such a BIC, `build()` refuses it *there*
by name rather than emitting a document the XSD rejects:

```rust
use sepa::{BicPattern, validate_bic};

let modern = validate_bic("E097AEXX")?;             // valid since ISO 9362:2022
assert!(modern.fits(BicPattern::Alphanumeric));     // pain.008.001.08 takes it
assert!(!modern.fits(BicPattern::LettersOnly));     // pain.008.001.02 cannot
# Ok::<(), Box<dyn std::error::Error>>(())
```

**One scheme's rule is another's rejection.** The five EPC payment schemes
share their messages and differ in their rules, so what varies is the *scheme*,
not the message. Combinations the rulebooks forbid are refused by name:

```rust
use sepa::{ChargeBearer, CreditTransferEntry, CreditTransferGroup, CreditTransferKind,
           IsoDate, Pain001Builder, ValidationError, validate_iban};
# let debtor = validate_iban("DE89370400440532013000")?;
# let payee  = validate_iban("NL91ABNA0417164300")?;
# let day    = IsoDate::new(2026, 7, 20)?;

// OCT Inst — the euro leg of a payment leaving SEPA, paying out in USD.
let xml = Pain001Builder::new("Acme GmbH", "OCT-001")
    .add_group(
        CreditTransferGroup::new("Acme GmbH", &debtor, day)
            .kind(CreditTransferKind::OneLegOutInstant)
            .add_entry(
                CreditTransferEntry::new("Payee", payee.clone(), 5_000, "OCT-1")
                    .with_currency("USD".parse()?)          // AT-T003/T004
                    .with_non_euro_leg_currency("USD".parse()?), // AT-T020
            ),
    )
    .build()?;

assert!(xml.contains("<SvcLvl><Cd>EOLO</Cd></SvcLvl>"));
assert!(xml.contains("<ChrgBr>SHAR</ChrgBr>"));   // SLEV is *forbidden* here
assert!(xml.contains("<InstrForCdtrAgt><InstrInf>USD</InstrInf></InstrForCdtrAgt>"));

// The same charge bearer under SEPA, where only SLEV is legal:
let err = Pain001Builder::new("Acme GmbH", "CT-001")
    .add_group(
        CreditTransferGroup::new("Acme GmbH", &debtor, day)
            .charge_bearer(ChargeBearer::Shar)
            .add_entry(CreditTransferEntry::new("Payee", payee, 5_000, "E2E-1")),
    )
    .build()
    .unwrap_err();
assert!(matches!(err.kind, ValidationError::ChargeBearerNotAllowed { .. }));
# Ok::<(), Box<dyn std::error::Error>>(())
```

**Failures name the row.** In a collection run of ten thousand,
"`InstdAmt` is out of range" is only actionable once it says which one:

```rust
# use sepa::{DirectDebitEntry, DirectDebitGroup, IsoDate, Pain008Builder,
#            ValidationError, validate_creditor_id, validate_iban};
# let iban = validate_iban("DE89370400440532013000")?;
# let ci = validate_creditor_id("DE98ZZZ09999999999")?;
# let date = IsoDate::new(2026, 7, 20)?;
let err = Pain008Builder::new("Stadtwerke GmbH", "DD-1")
    .add_group(
        DirectDebitGroup::new("Stadtwerke GmbH", &iban, &ci, date)
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
| **You change your mind, before settlement** | camt.055 → camt.029 | `camt055`, `camt029` |
| The bank accepts or rejects | pain.002 | `pain002` |
| The money arrives, batch-booked | camt.053 / camt.054 | `camt053`, `camt054` |
| The debtor claims it back | camt.054 return | `camt054` |
| **You** send it back, after settlement | pain.007 | `pain007` |

Three of those undo a payment and they are not interchangeable. **camt.055 is a
request** — the bank may refuse it, and nothing is cancelled until the camt.029
answer says so. **pain.007 is an instruction**, and only applies once a
collection has actually been taken. Ask for the wrong one and the window in
which anything could still be done is gone.

A reversal has to restate the collection it undoes. Rather than retyping a dozen
fields, hand it the objects you already sent:

```rust
# use sepa::{DirectDebitEntry, DirectDebitGroup, IsoDate, Pain007Builder,
#            ReversalEntry, ReversalGroup, ReversalReason,
#            validate_creditor_id, validate_iban};
# let creditor = validate_iban("DE89370400440532013000")?;
# let debtor = validate_iban("NL91ABNA0417164300")?;
# let ci = validate_creditor_id("DE98ZZZ09999999999")?;
let group = DirectDebitGroup::new("Stadtwerke GmbH", &creditor, &ci, IsoDate::new(2026, 7, 20)?);
let entry = DirectDebitEntry::new(
    "MND-42", "2024-06-01".parse()?, "Max Mustermann", debtor, 7_500, "E2E-1",
);

let xml = Pain007Builder::new("Stadtwerke GmbH", "DD-2026-07-001", "RVSL-001")
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

### Recalling before it settles

`camt.055` has three scopes — the whole file, a whole `PmtInf`, or named
transactions — and the schema cheerfully admits combinations of them that mean
nothing. Here they are alternatives, and mixing them is a build error rather
than a file the bank cannot action:

```rust
use sepa::{Camt055Builder, CancellationEntry, CancellationGroup, CancellationReason,
           OriginalMessage, parse_camt029, validate_bic};
# use sepa::{DirectDebitEntry, DirectDebitGroup, IsoDate, Pain008Builder,
#            validate_creditor_id, validate_iban};
# let iban = validate_iban("DE89370400440532013000")?;
# let ci = validate_creditor_id("DE98ZZZ09999999999")?;
# let submitted = Pain008Builder::new("Stadtwerke GmbH", "DD-2026-07-001")
#     .created_at("2026-07-15T09:00:00".parse()?)
#     .add_group(DirectDebitGroup::new("Stadtwerke GmbH", &iban, &ci, IsoDate::new(2026,7,20)?)
#         .payment_info_id("PMT-2026-07-A")
#         .add_entry(DirectDebitEntry::new("MND-1","2024-06-01".parse()?,"Max",iban.clone(),7_500,"E2E-1")));
let xml = Camt055Builder::new(
    "CXL-2026-07-001",
    "Stadtwerke GmbH",                    // Assgnr — you
    validate_bic("COBADEFFXXX")?,         // Assgne — your bank
    OriginalMessage::from_direct_debit(&submitted),  // copies MsgId, CreDtTm, totals
)
.add_group(
    CancellationGroup::new("PMT-2026-07-A")
        .add_entry(CancellationEntry::new("E2E-1", CancellationReason::Dupl)),
)
.build()?;

assert!(xml.contains("<Cd>DUPL</Cd>"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

A reason is a constructor argument, not an option: ISO types `CxlRsnInf` as
optional and no bank can act on a reasonless recall.

Then read the answer — and note that `PDCR` is **neither** outcome:

```rust,no_run
use sepa::{parse_camt029, RejectionReason};

# let xml = "";
let answer = parse_camt029(xml)?;
if !answer.is_final() {
    println!("still pending — do not post it either way");
} else if answer.is_accepted() {
    println!("cancellation accepted");
} else if answer.rejection_reasons().iter().any(|r| r.is_already_returned()) {
    println!("bank reports an existing return; reconcile it before further action");
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

---

## ✅ Verification of Payee

The reader interprets DK-profile VoP result codes in pain.002, separately from
payment acceptance. VoP requests use an inter-PSP API outside this crate.
Legal obligations and implementation dates depend on the jurisdiction and PSP;
these result codes do not determine liability or settlement.

```rust,no_run
use sepa::{VerificationOutcome, parse_pain002};

# let xml = "";
for block in &parse_pain002(xml)?.payment_info_statuses {
    for tx in &block.transactions {
        match tx.status.as_ref().and_then(|s| s.verification()) {
            // Additional information is reported text, not authentication.
            Some(VerificationOutcome::CloseMatch) => println!("{:?}", tx.additional_info),
            Some(VerificationOutcome::NoMatch) => println!("reported name mismatch"),
            Some(VerificationOutcome::NotApplicable) => println!("verification not applicable"),
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
`<ns2:Document>` parses identically to `<Document xmlns=…>` when the prefix resolves
to the same supported namespace. `<!DOCTYPE>` is
rejected outright as defence in depth against entity expansion.

```rust,no_run
use sepa::parse_camt053;

# let xml = "";
let stmt = &parse_camt053(xml)?.statements[0];

for entry in &stmt.entries {
    // `Option`, and the None is the point: the statement may not determine a
    // ledger figure, and nothing here guesses one. `amount_raw` and
    // `indicator_raw` keep what the bank actually sent.
    match entry.signed_ct() {
        Some(ct) => println!("{ct:+} ct  {}", entry.reference().unwrap_or("")),
        None => println!("unresolved: {:?} {:?}", entry.amount.amount_raw, entry.amount.direction_raw),
    }

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

Decoded field observations are retained alongside interpretations: `booking_date()` returns an
`IsoDate` and `booking_date_raw` the text that arrived, so a non-conforming file
is readable rather than rejected. `reference()` joins every `RmtInf/Ustrd`
occurrence — German banks split a long *Verwendungszweck* into 35-character
chunks, and reading only the first cuts it where the invoice number sits.

**Nothing on the read path invents a number, and nothing drops a row.** Money
has a magnitude *and* a direction; lose either and there is no figure, so
`signed_ct()` is an `Option`. An absent or misspelled `<CdtDbtInd>` is never
taken for a credit — that would read a debit as a credit of the same size. An
`Amt` this crate cannot represent leaves the **entry still present**, because a
booking missing from a statement looks identical to one that never happened.

All five reader documents expose located `financial_diagnostics()` for missing,
invalid, inconsistent or ambiguous financial facts. Raw monetary text is decoded
XML, not byte-for-byte source evidence. See
[reader contracts](https://hupe1980.github.io/sepa/docs/reader-contracts/) for amount
provenance, independent observations and signing requirements.

---

## 📋 Standards & schema versions

| Message | Versions (default first) |
|---|---|
| pain.001 | `.001.09` · `.001.03` · `.003.03` (DK, EOL) |
| pain.008 | `.001.08` · `.001.02` · `.003.02` (DK, EOL) |
| pain.007 | `.001.09` — the only version SEPA defines |
| pain.002 | parses `.001.10`, `.001.03`, `.002.03`, `.003.03` |
| camt.055 / camt.029 | `.001.05` / `.001.06` — the pair the DFÜ-Abkommen names; no version choice |
| camt.052/053 | parses `.001.02`, `.001.06`, `.001.08` |
| camt.054 | parses `.001.08` |

Select with `.schema(…)`; the enums implement `FromStr` over both the message
identifier and the namespace URN, so the target version can come from config.

<details>
<summary><b>Why not ISO's newest versions?</b></summary>

Use the version required by the applicable scheme profile and your bank.
Newer ISO versions such as `pain.001.001.13`, `pain.008.001.12` and
`pain.002.001.15` are not automatically accepted SEPA versions. `.001.09` / `.001.08` have
been the mandated versions since 19 November 2023. The EPC's address migration
is a different rule — structured addresses, not a newer message — and is
routinely mistaken for this one.

The versions SEPA mandates live in the [ISO 20022 Message
Archive](https://www.iso20022.org/catalogue-messages/iso-20022-messages-archive),
not the current-version catalogue.
</details>

| Standard | Used for |
|---|---|
| ISO 13616-1 + SWIFT IBAN Registry r102 | 89-country BBAN structure + length table |
| EPC409-09 v8.0 | SEPA scheme country list (42 codes) |
| ISO 9362:2022 · ISO 3166-1 alpha-2 | BIC format; 249 country codes + `XK` |
| EPC262-08 · ISO 11649 | Creditor Identifier and RF reference check digits |
| EPC153-22 v2.2 | Address guidance; library structured/hybrid scope |
| EPC103-24 | Verification of Payee outcomes |
| EPC217-08 | SEPA character set + conversion table |
| DK Anlage 3 GBIC 5 (TVS) | Stricter German validation subsets, applied in CI |
| ISO 20022 camt.055.001.05 / camt.029.001.06 | Customer recall and its resolution |
| EPC SEPA Rulebooks 2023/2025 | SCT, SCT Inst, SDD Core, SDD B2B |
| EPC158-22 / EPC250-22 | One-Leg Out Instant Credit Transfer (OCT Inst) rulebook and C2PSP guidelines |

### Scope — deliberately not included

| Not included | Why |
|---|---|
| Verification of Payee *requests* | Live PSP API integration is outside this crate. DK-profile VoP **results** in pain.002 are parsed. |
| Extended Remittance Information (EPC092-19) | The ERI option raises `Strd` from one 140-character block to 999 × 280, but binds only PSPs that adhered to it separately — sending it to one that did not is a rejection. The base scheme is what every SEPA PSP accepts. |
| IBAN → BIC derivation | Needs a licensed, constantly-changing directory. A stale table silently misroutes payments. |
| Banking-day calendars | `IsoDate::plus_days` is calendar arithmetic and says so. TARGET2 holidays are a policy input. |
| Non-euro amounts *outside* OCT Inst | The four SEPA-branded schemes are euro-only; a non-euro amount under one is refused, not emitted. OCT Inst is the EPC's own exception and is supported. `camt` parsing always reports the entry currency. |
| SEPA Request-To-Pay (SRTP) and SPAA | Both are API schemes. SRTP looks in scope because it uses `pain.013`/`pain.014`, but its inter-SP binding is JSON over REST — there is no XML document to build. |

---

## 🔬 Testing

| Layer | What it checks |
|---|---|
| Unit + integration | Value invariants, message construction, report interpretation and compiled documentation examples |
| **Conformance** | Adversarial tests over what a schema gate cannot see: input wrongly **refused** (oracle: the XML Schema lexical spaces), values **invented**, rows **dropped** |
| **XSD validation** | Generated fixtures for every schema version the crate emits |
| **DK subset validation** | the default SCT/SDD output *also* against GBIC 5 — stricter than ISO, and what German banks apply |
| **Fuzzing** | three `cargo-fuzz` targets in CI, asserting invariants rather than absence of panics: an accepted batch must emit only SEPA-legal text, and a parser must never report a figure the input did not carry. Seeded from the shared fixtures — random bytes are never a well-formed ISO 20022 document |
| **Vendored data** | eighteen pinned XSDs re-hashed in CI; an unrecorded digest fails the build. Each is checked to *reject* as well as accept, so a permissive mirror cannot pass as a gate |
| **Regulatory watch list** | Maintainer source-review dates and a separate freshness gate |
| **Inline fixtures** | every ISO 20022 literal in `src/` is found by a source walk and validated against the schema its own namespace names, so a fixture cannot be a document no bank could send |
| Exhaustive sweep | all 1,112,064 Unicode scalar values × both transliteration styles, asserting SEPA-legal output |
| Registry conformance | the 78 real IBAN examples SWIFT publishes |

Schema validation is necessary but not sufficient. `<InstdAmt>0</InstdAmt>`, a
140-character `<Nm>`, `<BICFI>NOTPROVIDED</BICFI>`, a `ZZ` Creditor Identifier,
two payment groups sharing a `PmtInfId`, a `ReqdExctnDt/DtTm` on a non-instant
transfer and a `Strd` block over the EPC's 140-character cap all pass the XSD
and are rejected by banks. Those rules live in `validate` and are enforced by
`build()`.

## 📦 Dependencies

| Crate | Role |
|---|---|
| [`thiserror`](https://crates.io/crates/thiserror) | Error derives |
| [`quick-xml`](https://crates.io/crates/quick-xml) | Correct XML parsing |
| [`serde`](https://crates.io/crates/serde) | `serde` feature — supported values, payment inputs and reports |
| [`time`](https://crates.io/crates/time) · [`chrono`](https://crates.io/crates/chrono) | Date interop (`time` / `chrono` features) |

`time` and `chrono` are conversion-only: the crate's own calendar arithmetic is
dependency-free and behaves identically whichever features are enabled.

## 🔖 Versioning

While `0.x`, minor releases may break the Rust API, stored forms, reader behavior
or emitted output. [CHANGELOG.md](CHANGELOG.md) distinguishes those contracts.
Patch releases preserve public signatures and valid output; security fixes may
tighten input checks. Pin the version deliberately and review migration notes.
Examples target **0.9.0**. For an unpublished checkout, use a local path or pinned
source revision.

## 📄 License

[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
