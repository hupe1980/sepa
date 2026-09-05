+++
title = "Recalls & cancellations"
description = "Stop a SEPA payment before it settles with camt.055 in Rust, and read the bank's camt.029 answer: the three cancellation scopes, why a reason is mandatory, and why PDCR is not an outcome."
weight = 6
+++

A **recall** is you asking your bank to stop a file you have already sent, before
it settles. It is a `camt.055` Customer Payment Cancellation Request, and the
answer comes back as a `camt.029` Resolution of Investigation.

The word "cancel" covers three different messages in SEPA, and they are not
interchangeable:

| You want to | Message | Kind |
|---|---|---|
| Stop a file you just submitted | `camt.055` | a **request** — the bank may refuse |
| Give back a direct debit that settled | [`pain.007`](@/docs/reversals.md) | an **instruction** — the money moves |
| Learn a payment came back | [`camt.054`](@/docs/bank-statements.md) | a **report** — after the fact |

The distinction decides what you can still do. Once a payment settles, a recall
is refused with `ARDT` and a reversal is the only route left — and a reversal
exists only for direct debits. Asking for the wrong one spends the window.

Only `camt.055.001.05` and `camt.029.001.06` are named by the DFÜ-Abkommen, so
unlike the payment messages there is no version to choose.

## Three scopes, and they are alternatives

A recall can name the whole file, a whole `PmtInf` group, or individual
transactions. The XSD makes every part of that optional, so it happily admits
"cancel the whole message, and also specifically these two" — which no bank can
action. Here the three are alternatives and mixing them is a build error.

### Named transactions

The usual case: one row in a run was wrong.

```rust
use sepa::{
    Camt055Builder, CancellationEntry, CancellationGroup, CancellationReason,
    DirectDebitEntry, DirectDebitGroup, IsoDate, OriginalMessage, Pain008Builder,
    validate_bic, validate_creditor_id, validate_iban,
};

let iban = validate_iban("DE89370400440532013000")?;
let ci   = validate_creditor_id("DE98ZZZ09999999999")?;

// The run that went out this morning.
let submitted = Pain008Builder::new("Stadtwerke GmbH", "DD-2026-07-001")
    .created_at("2026-07-15T09:00:00".parse()?)
    .add_group(
        DirectDebitGroup::new("Stadtwerke GmbH", &iban, &ci, IsoDate::new(2026, 7, 20)?)
            .payment_info_id("PMT-2026-07-A")
            .add_entry(DirectDebitEntry::new(
                "MND-1", "2024-06-01".parse()?, "Max Mustermann", iban.clone(), 7_500, "E2E-1",
            )),
    );

let recall = Camt055Builder::new(
    "CXL-2026-07-001",                              // Assgnmt/Id — the case key
    "Stadtwerke GmbH",                              // Assgnr — you
    validate_bic("COBADEFFXXX")?,                   // Assgne — your bank
    OriginalMessage::from_direct_debit(&submitted), // MsgId, CreDtTm and totals
)
.add_group(
    CancellationGroup::new("PMT-2026-07-A")
        .add_entry(CancellationEntry::new("E2E-1", CancellationReason::Dupl)),
)
.build()?;

assert!(recall.contains("<OrgnlEndToEndId>E2E-1</OrgnlEndToEndId>"));
assert!(recall.contains("<Cd>DUPL</Cd>"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

`OriginalMessage::from_direct_debit` takes the **builder**, not the XML. Every
element the recall needs is already in it, and retyping `OrgnlMsgId` by hand is
how a recall comes to name a file that was never submitted under that
identifier. There is a `from_credit_transfer` for pain.001.

Note what it does *not* do: if the original builder never pinned a
`created_at`, `OrgnlCreDtTm` is left out rather than stamped with "now". The
element names the moment the original was created, and that moment is gone.

### A whole group

```rust
# use sepa::{Camt055Builder, CancellationGroup, CancellationReason, OriginalMessage, validate_bic};
# let base = || Camt055Builder::new("CXL-1", "Stadtwerke GmbH", validate_bic("COBADEFFXXX").unwrap(),
#     OriginalMessage::new("DD-2026-07-001", "pain.008.001.08"));
let xml = base()
    .add_group(
        CancellationGroup::new("PMT-2026-07-A")
            .cancel_whole_group(CancellationReason::Upay)
            .additional_info("Lauf zurueckgezogen"),
    )
    .build()?;

assert!(xml.contains("<PmtInfCxl>true</PmtInfCxl>"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

### The whole file

```rust
# use sepa::{Camt055Builder, CancellationReason, OriginalMessage, validate_bic};
# let base = || Camt055Builder::new("CXL-1", "Stadtwerke GmbH", validate_bic("COBADEFFXXX").unwrap(),
#     OriginalMessage::new("DD-2026-07-001", "pain.008.001.08"));
let xml = base()
    .cancel_whole_message(CancellationReason::Tech)
    .additional_info("Fehlerhafter Lauf")
    .build()?;

assert!(xml.contains("<GrpCxl>true</GrpCxl>"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

Combining any two of the three is refused before any XML exists:

```rust
# use sepa::{Camt055Builder, CancellationEntry, CancellationGroup, CancellationReason,
#            OriginalMessage, ValidationError, validate_bic};
let err = Camt055Builder::new(
    "CXL-1", "Stadtwerke GmbH", validate_bic("COBADEFFXXX")?,
    OriginalMessage::new("DD-1", "pain.008.001.08"),
)
.cancel_whole_message(CancellationReason::Tech)
.add_group(
    CancellationGroup::new("PMT-A")
        .add_entry(CancellationEntry::new("E2E-1", CancellationReason::Tech)),
)
.build()
.unwrap_err();

assert!(matches!(err.kind, ValidationError::MutuallyExclusive { .. }));
# Ok::<(), Box<dyn std::error::Error>>(())
```

## A reason is mandatory

ISO types `CxlRsnInf` as optional. No bank can act on a reasonless recall, so
here it is a constructor argument on all three scopes and the reasonless form is
not something you can build.

| Code | Means |
|---|---|
| `DUPL` | Sent twice |
| `TECH` | A technical problem produced the instruction |
| `FRAD` | The instruction is fraudulent |
| `UPAY` | The payment was not due |
| `CUST` | The customer asked, no further reason given |
| `AGNT` · `CURR` · `CUTA` | Wrong agent, wrong currency, replaced by a technical transfer |

`CancellationReason5Code` is a **closed** enumeration — unlike the purpose code
lists, ISO does not revise it quarterly. A code outside it therefore cannot go
in `Cd` without making the document schema-invalid, so an unrecognised one is
written to the `Prtry` branch the choice provides for exactly that:

```rust
use sepa::CancellationReason;

let local: CancellationReason = "XY99".parse()?;
assert!(!local.is_iso_code());          // → <Rsn><Prtry>XY99</Prtry></Rsn>
assert!(CancellationReason::Dupl.is_iso_code()); // → <Rsn><Cd>DUPL</Cd></Rsn>
# Ok::<(), std::convert::Infallible>(())
```

## Reading the answer

Nothing has been cancelled until the `camt.029` says so. Match it to your
request by `RslvdCase/Id`, which echoes the `Assgnmt/Id` you sent.

```rust
use sepa::{parse_camt029, CancellationStatus};

let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.029.001.06">
  <RsltnOfInvstgtn>
    <Assgnmt><Id>RES-1</Id>
      <Assgnr><Agt><FinInstnId><BICFI>COBADEFFXXX</BICFI></FinInstnId></Agt></Assgnr>
      <Assgne><Pty><Nm>Stadtwerke GmbH</Nm></Pty></Assgne>
      <CreDtTm>2026-07-15T14:02:00</CreDtTm></Assgnmt>
    <RslvdCase><Id>CXL-2026-07-001</Id>
      <Cretr><Pty><Nm>Stadtwerke GmbH</Nm></Pty></Cretr></RslvdCase>
    <Sts><Conf>CNCL</Conf></Sts>
    <CxlDtls>
      <OrgnlPmtInfAndSts>
        <OrgnlPmtInfId>PMT-2026-07-A</OrgnlPmtInfId>
        <TxInfAndSts>
          <OrgnlEndToEndId>E2E-1</OrgnlEndToEndId>
          <TxCxlSts>ACCR</TxCxlSts>
        </TxInfAndSts>
      </OrgnlPmtInfAndSts>
    </CxlDtls>
  </RsltnOfInvstgtn>
</Document>"#;

let answer = parse_camt029(xml)?;
assert_eq!(answer.resolved_case_id.as_deref(), Some("CXL-2026-07-001"));
assert!(answer.is_accepted());

let tx = answer.transactions().next().unwrap();
assert_eq!(tx.original_end_to_end_id.as_deref(), Some("E2E-1"));
assert_eq!(tx.status, Some(CancellationStatus::Accepted));
# Ok::<(), sepa::Camt029ParseError>(())
```

### `PDCR` is not an answer yet

Three statuses matter: `ACCR` accepted, `RJCR` rejected and **`PDCR` pending** —
the bank has taken the case and has not resolved it. Booking a pending recall as
accepted writes off money that is not coming back; booking it as rejected
collects twice. Check `is_final()` before doing either.

```rust,no_run
use sepa::parse_camt029;

# let xml = "";
let answer = parse_camt029(xml)?;
if !answer.is_final() {
    // Still open. Wait for the next camt.029; do not post it either way.
} else if answer.is_accepted() {
    // Stopped.
} else {
    for reason in answer.rejection_reasons() {
        if reason.is_too_late() {
            // ARDT — it settled. A pain.007 reversal is the remaining route,
            // and only for a direct debit.
        }
    }
}
# Ok::<(), sepa::Camt029ParseError>(())
```

### A refusal explains itself at one level, and it may not be the one you read

Exactly as with [status reports](@/docs/status-reports.md), which level carries
the reason depends on how far the bank got. A bank that refuses the *whole*
submission sends no transaction blocks at all, so code that walks only
`TxInfAndSts` sees an empty document and concludes the recall worked.

`rejection_reasons()` gathers all three levels, and `is_accepted()` treats an
empty document as *not* an acceptance — a bank that lists nothing has told you
nothing.

```rust
use sepa::{parse_camt029, RejectionReason};

let xml = r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.029.001.06">
  <RsltnOfInvstgtn>
    <Assgnmt><Id>RES-2</Id><CreDtTm>2026-07-15T14:02:00</CreDtTm></Assgnmt>
    <RslvdCase><Id>CXL-2026-07-001</Id></RslvdCase>
    <Sts><Conf>RJCR</Conf></Sts>
    <CxlDtls>
      <OrgnlGrpInfAndSts>
        <OrgnlMsgId>DD-2026-07-001</OrgnlMsgId>
        <OrgnlMsgNmId>pain.008.001.08</OrgnlMsgNmId>
        <GrpCxlSts>RJCR</GrpCxlSts>
        <CxlStsRsnInf><Rsn><Cd>ARDT</Cd></Rsn>
          <AddtlInf>Bereits ausgefuehrt</AddtlInf></CxlStsRsnInf>
      </OrgnlGrpInfAndSts>
    </CxlDtls>
  </RsltnOfInvstgtn></Document>"#;

let answer = parse_camt029(xml)?;
assert_eq!(answer.transactions().count(), 0, "no transaction blocks at all");
assert!(!answer.is_accepted());
assert_eq!(answer.rejection_reasons(), [&RejectionReason::Ardt]);
# Ok::<(), sepa::Camt029ParseError>(())
```

## The refusal codes

| Code | Means | What is left |
|---|---|---|
| `ARDT` | Already settled and returned | A [`pain.007`](@/docs/reversals.md) reversal, for a direct debit |
| `AC04` | The account is closed | Nothing through the schemes |
| `AM04` | Insufficient funds to return | Nothing through the schemes |
| `CUST` | The beneficiary refused | Nothing through the schemes |
| `NOAS` | No answer from the beneficiary | Ask again, or write off |
| `NOOR` | The bank cannot find the original | Check the `MsgId` and `PmtInfId` you sent |
| `LEGL` · `AGNT` | Refused for legal reasons, or by an agent | Nothing through the schemes |

`is_too_late()` is the one worth branching on: it separates "we could not" from
"we would not", and only the first leaves a route open.

## What is checked before anything is written

| Rule | Why |
|---|---|
| At least one scope is named | Every part of `Undrlyg` is optional in the XSD, so a recall that cancels nothing is schema-valid |
| Scopes are not mixed | "All of it, and specifically these" is not an instruction |
| A group names a whole-group cancellation **or** transactions | Same argument, one level down |
| `OrgnlPmtInfId` is unique across groups | Two blocks naming one submitted group leave the bank to guess |
| `AddtlInf` ≤ 105 characters | `Max105Text` — the only such limit in the crate |
| The assignee BIC fits the schema's pattern | camt.055 names its type `BICFI` but keeps the **pre-2019** pattern |

That last one is worth knowing about: `camt.055.001.05` uses the element name
`BICFI` with the old `[A-Z]{6}…` pattern, while `pain.008.001.08` uses the same
name with the new alphanumeric one. The element name does not tell you which —
see [identifiers](@/docs/identifiers.md).
